//! Metal Backend Implementation
//!
//! This module provides the Metal backend using objc2-metal 0.3.2.
//! It implements the `GpuBackend` trait for Apple GPUs (macOS/iOS).
//!
//! Note: This uses the objc2 ecosystem which is the modern replacement
//! for the deprecated gfx-rs/metal-rs crate.

use crate::backend::{BackendType, DeviceInfo, GpuBackend, GpuBuffer, GpuFunction, GpuModule, KernelSource};
use crate::{Error, Result};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBuffer, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLComputeCommandEncoder,
    MTLComputePipelineState, MTLDevice, MTLFunction, MTLLibrary, MTLResourceOptions, MTLSize,
};
use std::ptr::NonNull;
use tracing::{debug, info};

// Link to CoreGraphics for MTLCreateSystemDefaultDevice
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {}

/// Metal buffer wrapper
///
/// Note: Metal buffers are not Send/Sync, so this wrapper provides
/// the necessary unsafe impls for use in the GpuBackend trait.
pub struct MetalBuffer {
    buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
    size: usize,
}

// SAFETY: Metal buffers can be safely shared across threads when properly synchronized
// The MetalBackend handles synchronization via command queue barriers
unsafe impl Send for MetalBuffer {}
unsafe impl Sync for MetalBuffer {}

impl GpuBuffer for MetalBuffer {
    fn size(&self) -> usize {
        self.size
    }
}

impl MetalBuffer {
    /// Get the underlying Metal buffer
    pub fn inner(&self) -> &Retained<ProtocolObject<dyn MTLBuffer>> {
        &self.buffer
    }

    /// Get a raw pointer to the buffer contents
    pub fn contents(&self) -> NonNull<std::ffi::c_void> {
        self.buffer.contents()
    }
}

/// Metal library (compiled shaders) wrapper
pub struct MetalModule {
    library: Retained<ProtocolObject<dyn MTLLibrary>>,
}

// SAFETY: Metal libraries are immutable and can be shared across threads
unsafe impl Send for MetalModule {}
unsafe impl Sync for MetalModule {}

impl GpuModule for MetalModule {}

impl MetalModule {
    /// Get the underlying Metal library
    pub fn inner(&self) -> &Retained<ProtocolObject<dyn MTLLibrary>> {
        &self.library
    }
}

/// Metal compute pipeline wrapper
pub struct MetalFunction {
    pipeline: Retained<ProtocolObject<dyn MTLComputePipelineState>>,
    #[allow(dead_code)]
    function: Retained<ProtocolObject<dyn MTLFunction>>,
}

// SAFETY: Compute pipeline states are immutable and can be shared across threads
unsafe impl Send for MetalFunction {}
unsafe impl Sync for MetalFunction {}

impl GpuFunction for MetalFunction {}

impl MetalFunction {
    /// Get the compute pipeline state
    pub fn pipeline(&self) -> &Retained<ProtocolObject<dyn MTLComputePipelineState>> {
        &self.pipeline
    }
}

/// Metal Backend implementation
///
/// Provides GPU acceleration using Apple Metal via objc2-metal 0.3.2.
pub struct MetalBackend {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    command_queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    device_info: DeviceInfo,
}

// SAFETY: MetalBackend manages synchronization through command queue barriers
// All operations that modify GPU state are synchronized before returning
unsafe impl Send for MetalBackend {}
unsafe impl Sync for MetalBackend {}

impl MetalBackend {
    /// Create a new Metal backend using the system default device
    pub fn new() -> Result<Self> {
        debug!("Initializing Metal backend");

        let device = objc2_metal::MTLCreateSystemDefaultDevice()
            .ok_or_else(|| Error::DeviceInit {
                device_id: 0,
                message: "No Metal-capable device found".into(),
            })?;

        let name = device.name().to_string();
        info!("Metal device initialized: {}", name);

        let command_queue = device.newCommandQueue()
            .ok_or_else(|| Error::DeviceInit {
                device_id: 0,
                message: "Failed to create command queue".into(),
            })?;

        let device_info = Self::query_device_info(&device, &name);

        debug!("Metal backend ready: {:?}", device_info);

        Ok(Self {
            device,
            command_queue,
            device_info,
        })
    }

    /// Check if Metal is available on this system
    pub fn is_available() -> bool {
        objc2_metal::MTLCreateSystemDefaultDevice().is_some()
    }

    /// Query device information
    fn query_device_info(device: &Retained<ProtocolObject<dyn MTLDevice>>, name: &str) -> DeviceInfo {
        // Get GPU family for compute capability equivalent
        let (family_major, family_minor) = Self::get_gpu_family(device);

        // Get recommended working set size as total memory estimate
        // On Apple Silicon, this represents unified memory available to GPU
        let total_memory = device.recommendedMaxWorkingSetSize() as usize;

        // Get max threads per threadgroup
        let size = device.maxThreadsPerThreadgroup();
        let max_threads = size.width as u32;

        // Estimate compute units based on device name
        let compute_units = Self::estimate_compute_units(name);

        // Estimate cores (Apple doesn't expose this directly)
        let estimated_cores = compute_units * 128; // Rough estimate for Apple GPUs

        DeviceInfo {
            name: name.to_string(),
            backend: BackendType::Metal,
            compute_capability: (family_major, family_minor),
            total_memory,
            max_threads_per_group: max_threads,
            compute_units,
            estimated_cores,
        }
    }

    /// Get GPU family version (similar to CUDA compute capability)
    fn get_gpu_family(device: &Retained<ProtocolObject<dyn MTLDevice>>) -> (u32, u32) {
        // Check for Apple GPU families (Apple Silicon)
        // Apple9 = M3 family
        if device.supportsFamily(objc2_metal::MTLGPUFamily::Apple9) {
            return (9, 0);
        }
        // Apple8 = M2 family
        if device.supportsFamily(objc2_metal::MTLGPUFamily::Apple8) {
            return (8, 0);
        }
        // Apple7 = M1 family
        if device.supportsFamily(objc2_metal::MTLGPUFamily::Apple7) {
            return (7, 0);
        }
        // Apple6 = A14
        if device.supportsFamily(objc2_metal::MTLGPUFamily::Apple6) {
            return (6, 0);
        }
        // Apple5 = A12
        if device.supportsFamily(objc2_metal::MTLGPUFamily::Apple5) {
            return (5, 0);
        }

        // Default for older devices
        (4, 0)
    }

    /// Estimate compute units based on device name
    fn estimate_compute_units(name: &str) -> u32 {
        let name_lower = name.to_lowercase();

        // M4 family (newest)
        if name_lower.contains("m4") {
            if name_lower.contains("ultra") { return 80; }
            if name_lower.contains("max") { return 40; }
            if name_lower.contains("pro") { return 20; }
            return 10;
        }

        // M3 family
        if name_lower.contains("m3") {
            if name_lower.contains("ultra") { return 80; }
            if name_lower.contains("max") { return 40; }
            if name_lower.contains("pro") { return 18; }
            return 10;
        }

        // M2 family
        if name_lower.contains("m2") {
            if name_lower.contains("ultra") { return 76; }
            if name_lower.contains("max") { return 38; }
            if name_lower.contains("pro") { return 19; }
            return 10;
        }

        // M1 family
        if name_lower.contains("m1") {
            if name_lower.contains("ultra") { return 64; }
            if name_lower.contains("max") { return 32; }
            if name_lower.contains("pro") { return 16; }
            return 8;
        }

        // Default
        8
    }

    /// Get the underlying Metal device
    pub fn device(&self) -> &Retained<ProtocolObject<dyn MTLDevice>> {
        &self.device
    }

    /// Get the command queue
    pub fn command_queue(&self) -> &Retained<ProtocolObject<dyn MTLCommandQueue>> {
        &self.command_queue
    }

    /// Dispatch a compute kernel
    ///
    /// This is the core method for executing Metal compute kernels. It creates a command
    /// buffer, sets up the compute encoder, binds buffers and constants, dispatches the
    /// kernel, and waits for completion.
    ///
    /// # Arguments
    ///
    /// * `function` - The compiled compute pipeline to execute
    /// * `buffers` - Slice of buffers to bind (in order: buffer(0), buffer(1), ...)
    /// * `constants` - Raw bytes for constant buffer (bound after all buffers)
    /// * `grid_size` - Number of threadgroups in (x, y, z)
    /// * `threadgroup_size` - Threads per threadgroup in (x, y, z)
    ///
    /// # Returns
    ///
    /// Ok(()) on success, or an error if kernel dispatch fails
    pub fn dispatch_kernel(
        &self,
        function: &MetalFunction,
        buffers: &[&MetalBuffer],
        constants: &[u8],
        grid_size: (u32, u32, u32),
        threadgroup_size: (u32, u32, u32),
    ) -> Result<()> {
        // Create command buffer
        let cmd_buffer = self.command_queue.commandBuffer()
            .ok_or_else(|| Error::MetalOperation("Failed to create command buffer".into()))?;

        // Create compute command encoder
        let encoder = cmd_buffer.computeCommandEncoder()
            .ok_or_else(|| Error::MetalOperation("Failed to create compute command encoder".into()))?;

        // Set the compute pipeline state
        encoder.setComputePipelineState(&function.pipeline);

        // Bind buffers in order
        // SAFETY: buffers are valid Metal buffers from this backend
        for (idx, buffer) in buffers.iter().enumerate() {
            unsafe {
                encoder.setBuffer_offset_atIndex(
                    Some(&buffer.buffer),
                    0,
                    idx as usize,
                );
            }
        }

        // Set constants if provided (bound after all buffers)
        if !constants.is_empty() {
            let ptr = NonNull::new(constants.as_ptr() as *mut std::ffi::c_void)
                .ok_or_else(|| Error::InvalidOperation("Null pointer in constants".into()))?;

            // SAFETY: ptr points to valid constant data for the duration of this call
            unsafe {
                encoder.setBytes_length_atIndex(
                    ptr,
                    constants.len(),
                    buffers.len(),
                );
            }
        }

        // Create MTLSize structs for dispatch
        let grid = MTLSize {
            width: grid_size.0 as usize,
            height: grid_size.1 as usize,
            depth: grid_size.2 as usize,
        };

        let threadgroup = MTLSize {
            width: threadgroup_size.0 as usize,
            height: threadgroup_size.1 as usize,
            depth: threadgroup_size.2 as usize,
        };

        // Dispatch the compute kernel
        encoder.dispatchThreadgroups_threadsPerThreadgroup(grid, threadgroup);

        // End encoding
        encoder.endEncoding();

        // Commit and wait for completion
        cmd_buffer.commit();
        cmd_buffer.waitUntilCompleted();

        debug!(
            "Metal kernel dispatched: grid={:?} threadgroup={:?}",
            grid_size, threadgroup_size
        );

        Ok(())
    }

    /// Dispatch a compute kernel with automatic threadgroup sizing
    ///
    /// This variant automatically calculates the optimal threadgroup size based on
    /// the pipeline's execution width and the total number of threads needed.
    ///
    /// # Arguments
    ///
    /// * `function` - The compiled compute pipeline to execute
    /// * `buffers` - Slice of buffers to bind
    /// * `constants` - Raw bytes for constant buffer
    /// * `total_threads` - Total number of threads to dispatch in (x, y, z)
    pub fn dispatch_kernel_auto(
        &self,
        function: &MetalFunction,
        buffers: &[&MetalBuffer],
        constants: &[u8],
        total_threads: (u32, u32, u32),
    ) -> Result<()> {
        // Get the pipeline's thread execution width
        let thread_execution_width = function.pipeline.threadExecutionWidth() as u32;

        // Calculate optimal threadgroup size (use execution width for x, 1 for y/z)
        let tg_x = thread_execution_width.min(total_threads.0);
        let tg_y = 1u32.min(total_threads.1);
        let tg_z = 1u32.min(total_threads.2);

        // Calculate grid size (number of threadgroups)
        let grid_x = (total_threads.0 + tg_x - 1) / tg_x;
        let grid_y = (total_threads.1 + tg_y - 1) / tg_y;
        let grid_z = (total_threads.2 + tg_z - 1) / tg_z;

        self.dispatch_kernel(
            function,
            buffers,
            constants,
            (grid_x, grid_y, grid_z),
            (tg_x, tg_y, tg_z),
        )
    }

    /// Write data directly to a Metal buffer
    ///
    /// # Arguments
    ///
    /// * `buffer` - The buffer to write to
    /// * `data` - The data to write
    /// * `offset` - Byte offset in the buffer
    pub fn write_buffer(&self, buffer: &MetalBuffer, data: &[u8], offset: usize) -> Result<()> {
        if offset + data.len() > buffer.size {
            return Err(Error::InvalidOperation(format!(
                "Write exceeds buffer size: {} + {} > {}",
                offset, data.len(), buffer.size
            )));
        }

        let ptr = buffer.buffer.contents();
        // SAFETY: We've verified the bounds, and ptr is valid for buffer.size bytes
        unsafe {
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                ptr.as_ptr().cast::<u8>().add(offset),
                data.len(),
            );
        }

        Ok(())
    }

    /// Read data from a Metal buffer
    ///
    /// # Arguments
    ///
    /// * `buffer` - The buffer to read from
    /// * `offset` - Byte offset in the buffer
    /// * `len` - Number of bytes to read
    pub fn read_buffer(&self, buffer: &MetalBuffer, offset: usize, len: usize) -> Result<Vec<u8>> {
        if offset + len > buffer.size {
            return Err(Error::InvalidOperation(format!(
                "Read exceeds buffer size: {} + {} > {}",
                offset, len, buffer.size
            )));
        }

        let ptr = buffer.buffer.contents();
        let mut result = vec![0u8; len];

        // SAFETY: We've verified the bounds, and ptr is valid for buffer.size bytes
        unsafe {
            std::ptr::copy_nonoverlapping(
                ptr.as_ptr().cast::<u8>().add(offset),
                result.as_mut_ptr(),
                len,
            );
        }

        Ok(result)
    }
}

impl GpuBackend for MetalBackend {
    type Buffer = MetalBuffer;
    type Module = MetalModule;
    type Function = MetalFunction;

    fn device_name(&self) -> &str {
        &self.device_info.name
    }

    fn backend_type(&self) -> BackendType {
        BackendType::Metal
    }

    fn device_info(&self) -> &DeviceInfo {
        &self.device_info
    }

    fn total_memory(&self) -> usize {
        self.device_info.total_memory
    }

    fn free_memory(&self) -> Result<usize> {
        // Metal doesn't provide a direct free memory API
        // Return current working set size as an approximation
        let current = self.device.currentAllocatedSize() as usize;
        let max = self.device_info.total_memory;
        Ok(max.saturating_sub(current))
    }

    fn allocate(&self, bytes: usize) -> Result<Self::Buffer> {
        let buffer = self.device.newBufferWithLength_options(
            bytes,
            MTLResourceOptions::StorageModeShared,
        ).ok_or_else(|| Error::OutOfMemory {
            size: bytes,
            available: 0,
        })?;

        Ok(MetalBuffer { buffer, size: bytes })
    }

    fn copy_to_device(&self, data: &[u8]) -> Result<Self::Buffer> {
        // SAFETY: data.as_ptr() is valid for the slice's lifetime
        let ptr = NonNull::new(data.as_ptr() as *mut std::ffi::c_void)
            .ok_or_else(|| Error::InvalidOperation("Null pointer in source data".into()))?;

        // SAFETY: ptr is valid and points to data.len() bytes of readable memory
        let buffer = unsafe {
            self.device.newBufferWithBytes_length_options(
                ptr,
                data.len(),
                MTLResourceOptions::StorageModeShared,
            )
        }.ok_or_else(|| Error::OutOfMemory {
            size: data.len(),
            available: 0,
        })?;

        Ok(MetalBuffer { buffer, size: data.len() })
    }

    fn copy_to_host(&self, buffer: &Self::Buffer) -> Result<Vec<u8>> {
        // NonNull is guaranteed to be non-null, so no null check needed
        let ptr = buffer.buffer.contents();

        // SAFETY: ptr is valid and buffer.size was set during allocation
        let slice = unsafe {
            std::slice::from_raw_parts(ptr.as_ptr().cast::<u8>(), buffer.size)
        };

        Ok(slice.to_vec())
    }

    fn compile(&self, source: &KernelSource) -> Result<Self::Module> {
        let metal_source = source.metal.ok_or_else(|| {
            Error::InvalidOperation("No Metal source provided for Metal backend".into())
        })?;

        debug!("Compiling Metal shader ({} bytes)", metal_source.len());

        let source_str = NSString::from_str(metal_source);

        let library = self.device.newLibraryWithSource_options_error(&source_str, None)
            .map_err(|e| {
                Error::ShaderCompilation(format!("Metal shader compilation failed: {:?}", e))
            })?;

        Ok(MetalModule { library })
    }

    fn get_function(&self, module: &Self::Module, name: &str) -> Result<Self::Function> {
        let name_str = NSString::from_str(name);

        let function = module.library.newFunctionWithName(&name_str)
            .ok_or_else(|| {
                Error::InvalidOperation(format!("Function '{}' not found in Metal library", name))
            })?;

        let pipeline = self.device.newComputePipelineStateWithFunction_error(&function)
            .map_err(|e| {
                Error::MetalOperation(format!("Failed to create compute pipeline: {:?}", e))
            })?;

        Ok(MetalFunction { pipeline, function })
    }

    fn synchronize(&self) -> Result<()> {
        // Create a command buffer and wait for completion
        if let Some(cmd_buffer) = self.command_queue.commandBuffer() {
            cmd_buffer.commit();
            cmd_buffer.waitUntilCompleted();
        }
        Ok(())
    }
}

impl std::fmt::Debug for MetalBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetalBackend")
            .field("device_info", &self.device_info)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metal_availability() {
        let available = MetalBackend::is_available();
        println!("Metal available: {}", available);
    }

    #[test]
    fn test_metal_backend_creation() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = MetalBackend::new().expect("Failed to create Metal backend");
        println!("Device: {}", backend.device_name());
        println!("Backend: {}", backend.backend_type());
        println!("Memory: {} bytes", backend.total_memory());

        let info = backend.device_info();
        println!("GPU Family: {}.{}", info.compute_capability.0, info.compute_capability.1);
        println!("Compute units: {}", info.compute_units);
        println!("Estimated cores: {}", info.estimated_cores);
    }

    #[test]
    fn test_metal_memory_operations() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = MetalBackend::new().expect("Failed to create Metal backend");

        // Test allocation
        let buffer = backend.allocate(1024).expect("Failed to allocate");
        assert_eq!(buffer.size(), 1024);

        // Test copy to device and back
        let data = vec![0x42u8; 1024];
        let device_buffer = backend.copy_to_device(&data).expect("Failed to copy to device");
        let host_data = backend.copy_to_host(&device_buffer).expect("Failed to copy to host");
        assert_eq!(host_data, data);
    }

    #[test]
    fn test_metal_shader_compilation() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = MetalBackend::new().expect("Failed to create Metal backend");

        let source = KernelSource::metal_only(r#"
            #include <metal_stdlib>
            using namespace metal;

            kernel void test_kernel(
                device float* data [[buffer(0)]],
                uint idx [[thread_position_in_grid]]
            ) {
                data[idx] *= 2.0f;
            }
        "#);

        let module = backend.compile(&source).expect("Failed to compile shader");
        let _func = backend.get_function(&module, "test_kernel").expect("Failed to get function");
    }

    #[test]
    fn test_metal_kernel_dispatch() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = MetalBackend::new().expect("Failed to create Metal backend");

        // Simple kernel that doubles each element
        let source = KernelSource::metal_only(r#"
            #include <metal_stdlib>
            using namespace metal;

            kernel void double_values(
                device float* data [[buffer(0)]],
                uint idx [[thread_position_in_grid]]
            ) {
                data[idx] *= 2.0f;
            }
        "#);

        let module = backend.compile(&source).expect("Failed to compile shader");
        let function = backend.get_function(&module, "double_values").expect("Failed to get function");

        // Create input data: [1.0, 2.0, 3.0, 4.0]
        let input_data: Vec<f32> = vec![1.0, 2.0, 3.0, 4.0];
        let input_bytes: Vec<u8> = input_data.iter()
            .flat_map(|f| f.to_le_bytes())
            .collect();

        let buffer = backend.copy_to_device(&input_bytes).expect("Failed to copy to device");

        // Dispatch kernel: 4 threads (one per element)
        backend.dispatch_kernel(
            &function,
            &[&buffer],
            &[],
            (1, 1, 1),  // 1 threadgroup
            (4, 1, 1),  // 4 threads per threadgroup
        ).expect("Failed to dispatch kernel");

        // Read back result
        let result_bytes = backend.copy_to_host(&buffer).expect("Failed to copy to host");
        let result: Vec<f32> = result_bytes
            .chunks(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        // Verify: each value should be doubled
        assert_eq!(result, vec![2.0, 4.0, 6.0, 8.0]);
        println!("Kernel dispatch test passed: {:?} -> {:?}", input_data, result);
    }

    #[test]
    fn test_metal_kernel_dispatch_auto() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = MetalBackend::new().expect("Failed to create Metal backend");

        // Kernel that adds 10 to each element
        let source = KernelSource::metal_only(r#"
            #include <metal_stdlib>
            using namespace metal;

            kernel void add_ten(
                device int* data [[buffer(0)]],
                constant uint& count [[buffer(1)]],
                uint idx [[thread_position_in_grid]]
            ) {
                if (idx < count) {
                    data[idx] += 10;
                }
            }
        "#);

        let module = backend.compile(&source).expect("Failed to compile shader");
        let function = backend.get_function(&module, "add_ten").expect("Failed to get function");

        // Create input data: [0, 1, 2, 3, ..., 99]
        let count = 100u32;
        let input_data: Vec<i32> = (0..count as i32).collect();
        let input_bytes: Vec<u8> = input_data.iter()
            .flat_map(|i| i.to_le_bytes())
            .collect();

        let buffer = backend.copy_to_device(&input_bytes).expect("Failed to copy to device");

        // Use auto dispatch with constants
        backend.dispatch_kernel_auto(
            &function,
            &[&buffer],
            &count.to_le_bytes(),  // Pass count as constant
            (count, 1, 1),         // Total threads
        ).expect("Failed to dispatch kernel");

        // Read back result
        let result_bytes = backend.copy_to_host(&buffer).expect("Failed to copy to host");
        let result: Vec<i32> = result_bytes
            .chunks(4)
            .map(|chunk| i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        // Verify: each value should have 10 added
        for (i, &val) in result.iter().enumerate() {
            assert_eq!(val, i as i32 + 10, "Mismatch at index {}", i);
        }
        println!("Auto dispatch test passed with {} elements", count);
    }

    #[test]
    fn test_metal_buffer_write_read() {
        if !MetalBackend::is_available() {
            println!("Skipping test - no Metal device available");
            return;
        }

        let backend = MetalBackend::new().expect("Failed to create Metal backend");

        // Allocate buffer
        let buffer = backend.allocate(256).expect("Failed to allocate");

        // Write data at different offsets
        let data1 = vec![0xAAu8; 64];
        let data2 = vec![0xBBu8; 64];

        backend.write_buffer(&buffer, &data1, 0).expect("Failed to write data1");
        backend.write_buffer(&buffer, &data2, 128).expect("Failed to write data2");

        // Read back and verify
        let read1 = backend.read_buffer(&buffer, 0, 64).expect("Failed to read data1");
        let read2 = backend.read_buffer(&buffer, 128, 64).expect("Failed to read data2");

        assert_eq!(read1, data1);
        assert_eq!(read2, data2);

        // Verify gap is zeroed
        let gap = backend.read_buffer(&buffer, 64, 64).expect("Failed to read gap");
        assert!(gap.iter().all(|&b| b == 0));

        println!("Buffer write/read test passed");
    }
}
