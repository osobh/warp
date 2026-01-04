//! GPU and host buffer abstractions with lifetime safety

use crate::{Error, GpuContext, Result};
use cudarc::driver::{CudaContext, CudaSlice, CudaStream, DeviceRepr, ValidAsZeroBits};
use std::sync::Arc;

/// A GPU device buffer with automatic memory management
///
/// This buffer owns device memory and automatically frees it when dropped.
/// It provides type-safe access to GPU memory with compile-time lifetime checking.
pub struct GpuBuffer<T: DeviceRepr> {
    slice: CudaSlice<T>,
    ctx: Arc<CudaContext>,
    stream: Arc<CudaStream>,
}

impl<T: DeviceRepr + ValidAsZeroBits + Unpin + Clone + Default> GpuBuffer<T> {
    /// Allocate a new GPU buffer
    ///
    /// # Arguments
    /// * `context` - GPU context for allocation
    /// * `len` - Number of elements
    ///
    /// # Errors
    /// Returns error if allocation fails
    pub fn new(context: &GpuContext, len: usize) -> Result<Self> {
        let ctx = context.context().clone();
        let stream = context.stream().clone();
        let slice = stream.alloc_zeros::<T>(len)?;

        Ok(Self { slice, ctx, stream })
    }

    /// Create buffer from existing CudaSlice
    ///
    /// # Arguments
    /// * `slice` - Existing CUDA slice
    /// * `ctx` - CUDA context that allocated the slice
    /// * `stream` - CUDA stream for memory operations
    pub fn from_slice(slice: CudaSlice<T>, ctx: Arc<CudaContext>, stream: Arc<CudaStream>) -> Self {
        Self { slice, ctx, stream }
    }

    /// Get buffer length in elements
    #[inline]
    pub fn len(&self) -> usize {
        self.slice.len()
    }

    /// Check if buffer is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.slice.is_empty()
    }

    /// Get immutable reference to the underlying CudaSlice
    #[inline]
    pub fn as_slice(&self) -> &CudaSlice<T> {
        &self.slice
    }

    /// Get mutable reference to the underlying CudaSlice
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut CudaSlice<T> {
        &mut self.slice
    }

    /// Copy data from host to this GPU buffer
    ///
    /// # Arguments
    /// * `src` - Host data to copy
    ///
    /// # Errors
    /// Returns error if src.len() != self.len or transfer fails
    pub fn copy_from_host(&mut self, src: &[T]) -> Result<()> {
        debug_assert_eq!(
            src.len(),
            self.len(),
            "copy_from_host: source length {} must match buffer length {}",
            src.len(),
            self.len()
        );
        if src.len() != self.len() {
            return Err(Error::InvalidOperation(format!(
                "Source length {} does not match buffer length {}",
                src.len(),
                self.len()
            )));
        }

        // Clone data to GPU using the stream
        let new_slice = self.stream.clone_htod(src)?;
        self.slice = new_slice;
        Ok(())
    }

    /// Copy data from this GPU buffer to host
    ///
    /// # Returns
    /// Vector containing the copied data
    pub fn copy_to_host(&self) -> Result<Vec<T>> {
        Ok(self.stream.clone_dtoh(&self.slice)?)
    }

    /// Copy data from this GPU buffer to a pre-allocated host buffer
    ///
    /// # Arguments
    /// * `dst` - Destination host buffer
    ///
    /// # Errors
    /// Returns error if dst.len() != self.len or transfer fails
    pub fn copy_to_host_into(&self, dst: &mut [T]) -> Result<()> {
        if dst.len() != self.len() {
            return Err(Error::InvalidOperation(format!(
                "Destination length {} does not match buffer length {}",
                dst.len(),
                self.len()
            )));
        }

        self.stream.memcpy_dtoh(&self.slice, dst)?;
        Ok(())
    }

    /// Get the CUDA context
    #[inline]
    pub fn cuda_context(&self) -> &Arc<CudaContext> {
        &self.ctx
    }

    /// Get the CUDA stream
    #[inline]
    pub fn cuda_stream(&self) -> &Arc<CudaStream> {
        &self.stream
    }
}

/// A host buffer for CPU-side operations
///
/// This wraps a standard Vec but provides a consistent interface
/// with GpuBuffer for easier interop.
pub struct HostBuffer<T> {
    data: Vec<T>,
}

impl<T> HostBuffer<T> {
    /// Create a new host buffer with specified capacity
    ///
    /// # Arguments
    /// * `capacity` - Initial capacity
    pub fn new(capacity: usize) -> Self
    where
        T: Default + Clone,
    {
        Self {
            data: vec![T::default(); capacity],
        }
    }

    /// Create host buffer from existing vector
    #[inline]
    pub fn from_vec(data: Vec<T>) -> Self {
        Self { data }
    }

    /// Get buffer length
    #[inline]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if buffer is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get immutable slice
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        &self.data
    }

    /// Get mutable slice
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.data
    }

    /// Consume and return inner Vec
    #[inline]
    pub fn into_vec(self) -> Vec<T> {
        self.data
    }

    /// Copy data from a slice
    pub fn copy_from_slice(&mut self, src: &[T])
    where
        T: Clone,
    {
        self.data.clear();
        self.data.extend_from_slice(src);
    }
}

impl<T: DeviceRepr + ValidAsZeroBits + Unpin + Clone + Default> HostBuffer<T> {
    /// Transfer this host buffer to GPU
    ///
    /// # Arguments
    /// * `context` - GPU context for allocation
    ///
    /// # Returns
    /// A new GPU buffer containing the data
    pub fn to_gpu(&self, context: &GpuContext) -> Result<GpuBuffer<T>> {
        let mut gpu_buf = GpuBuffer::new(context, self.len())?;
        gpu_buf.copy_from_host(self.as_slice())?;
        Ok(gpu_buf)
    }
}

impl<T> AsRef<[T]> for HostBuffer<T> {
    fn as_ref(&self) -> &[T] {
        &self.data
    }
}

impl<T> AsMut<[T]> for HostBuffer<T> {
    fn as_mut(&mut self) -> &mut [T] {
        &mut self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_buffer_creation() {
        let buffer = HostBuffer::<u8>::new(1024);
        assert_eq!(buffer.len(), 1024);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_host_buffer_from_vec() {
        let data = vec![1u8, 2, 3, 4, 5];
        let buffer = HostBuffer::from_vec(data.clone());
        assert_eq!(buffer.as_slice(), &data);
    }

    #[test]
    fn test_gpu_buffer_creation() {
        if let Ok(ctx) = GpuContext::new() {
            let buffer = GpuBuffer::<u8>::new(&ctx, 1024).unwrap();
            assert_eq!(buffer.len(), 1024);
            assert!(!buffer.is_empty());
        }
    }

    #[test]
    fn test_host_to_gpu_transfer() {
        if let Ok(ctx) = GpuContext::new() {
            let data = vec![1u8, 2, 3, 4, 5];
            let host_buffer = HostBuffer::from_vec(data.clone());

            let gpu_buffer = host_buffer.to_gpu(&ctx).unwrap();
            let result = gpu_buffer.copy_to_host().unwrap();

            assert_eq!(result, data);
        }
    }

    #[test]
    fn test_gpu_roundtrip() {
        if let Ok(ctx) = GpuContext::new() {
            let original = vec![0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9];
            let mut gpu_buf = GpuBuffer::new(&ctx, original.len()).unwrap();

            gpu_buf.copy_from_host(&original).unwrap();
            let result = gpu_buf.copy_to_host().unwrap();

            assert_eq!(result, original);
        }
    }

    #[test]
    fn test_copy_to_host_into() {
        if let Ok(ctx) = GpuContext::new() {
            let original = vec![42u8; 100];
            let mut gpu_buf = GpuBuffer::new(&ctx, original.len()).unwrap();
            gpu_buf.copy_from_host(&original).unwrap();

            let mut dst = vec![0u8; 100];
            gpu_buf.copy_to_host_into(&mut dst).unwrap();

            assert_eq!(dst, original);
        }
    }
}
