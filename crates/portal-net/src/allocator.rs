//! IP address allocator for the Portal virtual network
//!
//! This module provides efficient IP allocation within the 10.0.0.0/16 subnet.
//! It uses a bitmap-based approach with a `BTreeSet` for O(log n) operations and
//! minimal memory overhead.
//!
//! # Allocation Strategy
//!
//! - Host 0 (10.0.0.0) is reserved for the network address
//! - Host 1 (10.0.0.1) is reserved for the Hub
//! - Hosts 2-65535 are available for peer allocation
//! - Uses a `next_hint` to optimize sequential allocation
//!
//! # Examples
//!
//! ```
//! use portal_net::allocator::{IpAllocator, BitmapAllocator};
//! use portal_net::VirtualIp;
//!
//! let mut allocator = BitmapAllocator::new();
//!
//! // Allocate IPs
//! let ip1 = allocator.allocate().unwrap();
//! let ip2 = allocator.allocate().unwrap();
//!
//! // Check allocation status
//! assert!(allocator.is_allocated(ip1));
//!
//! // Release and reuse
//! allocator.release(ip1);
//! assert!(!allocator.is_allocated(ip1));
//!
//! // Reserve specific IPs
//! let specific = VirtualIp::new(100);
//! allocator.reserve(specific).unwrap();
//! ```

use crate::{PortalNetError, Result, VirtualIp};
use std::collections::BTreeSet;

/// Maximum host identifier (2^16 - 1)
const MAX_HOST: u16 = 65535;

/// Network address host (reserved)
const NETWORK_HOST: u16 = 0;

/// Hub host identifier (reserved)
const HUB_HOST: u16 = 1;

/// First allocatable host identifier
const FIRST_ALLOCATABLE: u16 = 2;

/// IP allocator trait
///
/// Defines the interface for IP address allocation within the Portal virtual network.
/// Implementations must be thread-safe when wrapped in appropriate synchronization
/// primitives (e.g., `Arc<Mutex<T>>`).
pub trait IpAllocator: Send + Sync {
    /// Allocates the next available IP address
    ///
    /// Returns `None` if no IP addresses are available.
    fn allocate(&mut self) -> Option<VirtualIp>;

    /// Reserves a specific IP address
    ///
    /// Returns an error if the IP is already allocated or is a reserved address.
    ///
    /// # Errors
    ///
    /// Returns `PortalNetError::Configuration` if:
    /// - The IP is already allocated
    /// - The IP is a reserved address (network or hub)
    fn reserve(&mut self, ip: VirtualIp) -> Result<()>;

    /// Releases a previously allocated IP address
    ///
    /// If the IP was not allocated, this operation is a no-op.
    /// This is safe to call on unallocated IPs.
    fn release(&mut self, ip: VirtualIp);

    /// Checks if an IP address is currently allocated
    fn is_allocated(&self, ip: VirtualIp) -> bool;

    /// Returns the number of available IP addresses
    ///
    /// This count excludes reserved addresses (network and hub).
    fn available_count(&self) -> usize;
}

/// Bitmap-based IP allocator for 10.0.0.0/16 subnet
///
/// Uses a `BTreeSet` to track allocated host identifiers, providing:
/// - O(log n) allocation, reservation, and release operations
/// - O(1) availability counting
/// - Minimal memory overhead (only stores allocated IPs)
///
/// # Thread Safety
///
/// This allocator is `Send + Sync`, making it suitable for use with
/// `Arc<Mutex<BitmapAllocator>>` in multi-threaded contexts.
///
/// # Examples
///
/// ```
/// use portal_net::allocator::{IpAllocator, BitmapAllocator};
/// use portal_net::VirtualIp;
///
/// // Create allocator with Hub pre-reserved
/// let mut allocator = BitmapAllocator::new();
/// assert!(allocator.is_allocated(VirtualIp::HUB));
///
/// // Create with custom reservations
/// let reserved = vec![VirtualIp::new(10), VirtualIp::new(20)];
/// let mut allocator = BitmapAllocator::with_reserved(&reserved);
/// assert!(allocator.is_allocated(VirtualIp::new(10)));
/// ```
#[derive(Debug, Clone)]
pub struct BitmapAllocator {
    /// Set of allocated host identifiers
    /// Uses `BTreeSet` for ordered iteration and efficient lookups
    allocated: BTreeSet<u16>,

    /// Hint for the next allocation to optimize sequential allocation
    /// Reduces the need to scan from the beginning each time
    next_hint: u16,
}

impl BitmapAllocator {
    /// Creates a new IP allocator with the Hub (10.0.0.1) pre-reserved
    ///
    /// The allocator is ready to use immediately, with network and hub
    /// addresses already reserved.
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_net::allocator::{IpAllocator, BitmapAllocator};
    /// use portal_net::VirtualIp;
    ///
    /// let allocator = BitmapAllocator::new();
    /// assert!(allocator.is_allocated(VirtualIp::HUB));
    /// assert_eq!(allocator.available_count(), 65534); // 65536 - 2 reserved (network + hub)
    /// ```
    #[must_use]
    pub fn new() -> Self {
        let mut allocated = BTreeSet::new();
        allocated.insert(NETWORK_HOST);
        allocated.insert(HUB_HOST);

        Self {
            allocated,
            next_hint: FIRST_ALLOCATABLE,
        }
    }

    /// Creates a new IP allocator with custom pre-reserved addresses
    ///
    /// The Hub and network addresses are always reserved, plus any additional
    /// addresses specified in the `reserved` slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use portal_net::allocator::{IpAllocator, BitmapAllocator};
    /// use portal_net::VirtualIp;
    ///
    /// let reserved = vec![VirtualIp::new(10), VirtualIp::new(20), VirtualIp::new(30)];
    /// let allocator = BitmapAllocator::with_reserved(&reserved);
    ///
    /// assert!(allocator.is_allocated(VirtualIp::HUB));
    /// assert!(allocator.is_allocated(VirtualIp::new(10)));
    /// assert!(allocator.is_allocated(VirtualIp::new(20)));
    /// assert!(allocator.is_allocated(VirtualIp::new(30)));
    /// ```
    #[must_use]
    pub fn with_reserved(reserved: &[VirtualIp]) -> Self {
        let mut allocator = Self::new();

        for &ip in reserved {
            let host = ip.host();
            // Only reserve allocatable IPs (skip network and hub as they're already reserved)
            if (FIRST_ALLOCATABLE..=MAX_HOST).contains(&host) {
                allocator.allocated.insert(host);
            }
        }

        allocator
    }

    /// Finds the next available host identifier starting from the hint
    ///
    /// This optimizes sequential allocation by starting the search from
    /// the last allocation point rather than always from the beginning.
    fn find_next_available(&self) -> Option<u16> {
        // Try from hint to MAX_HOST
        (self.next_hint..=MAX_HOST)
            .find(|host| !self.allocated.contains(host))
            .or_else(|| {
                // Wrap around and try from FIRST_ALLOCATABLE to hint
                (FIRST_ALLOCATABLE..self.next_hint).find(|host| !self.allocated.contains(host))
            })
    }
}

impl Default for BitmapAllocator {
    fn default() -> Self {
        Self::new()
    }
}

impl IpAllocator for BitmapAllocator {
    fn allocate(&mut self) -> Option<VirtualIp> {
        let host = self.find_next_available()?;
        self.allocated.insert(host);

        // Update hint to next position for optimization
        self.next_hint = if host < MAX_HOST {
            host + 1
        } else {
            FIRST_ALLOCATABLE
        };

        Some(VirtualIp::new(host))
    }

    fn reserve(&mut self, ip: VirtualIp) -> Result<()> {
        let host = ip.host();

        // Check if it's a reserved address
        if host == NETWORK_HOST {
            return Err(PortalNetError::Configuration(
                "cannot reserve network address (10.0.0.0)".to_string(),
            ));
        }

        if host == HUB_HOST {
            return Err(PortalNetError::Configuration(
                "cannot reserve hub address (10.0.0.1)".to_string(),
            ));
        }

        // Check if already allocated
        if self.allocated.contains(&host) {
            return Err(PortalNetError::Configuration(format!(
                "IP address {ip} is already allocated"
            )));
        }

        self.allocated.insert(host);
        Ok(())
    }

    fn release(&mut self, ip: VirtualIp) {
        let host = ip.host();

        // Don't allow releasing reserved addresses
        if host == NETWORK_HOST || host == HUB_HOST {
            return;
        }

        // Remove from allocated set (safe even if not present)
        if self.allocated.remove(&host) {
            // Update hint if this is before current hint for better reuse
            if host < self.next_hint {
                self.next_hint = host;
            }
        }
    }

    fn is_allocated(&self, ip: VirtualIp) -> bool {
        self.allocated.contains(&ip.host())
    }

    fn available_count(&self) -> usize {
        // Total addressable hosts minus allocated
        // Total = MAX_HOST + 1 - FIRST_ALLOCATABLE (accounting for 0-indexed)
        let total = (MAX_HOST - FIRST_ALLOCATABLE + 1) as usize;
        let allocated = self.allocated.len().saturating_sub(2); // Subtract reserved (network + hub)
        total.saturating_sub(allocated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocator_new() {
        let allocator = BitmapAllocator::new();

        // Hub should be pre-reserved
        assert!(allocator.is_allocated(VirtualIp::HUB));

        // Network address should be reserved
        assert!(allocator.is_allocated(VirtualIp::new(NETWORK_HOST)));

        // First allocatable should be available
        assert!(!allocator.is_allocated(VirtualIp::new(FIRST_ALLOCATABLE)));

        // Should have correct available count (65536 total - 2 reserved)
        assert_eq!(allocator.available_count(), 65534);
    }

    #[test]
    fn test_allocate_sequential() {
        let mut allocator = BitmapAllocator::new();

        // First allocation should be host 2
        let ip1 = allocator.allocate().unwrap();
        assert_eq!(ip1.host(), 2);
        assert!(allocator.is_allocated(ip1));

        // Second allocation should be host 3
        let ip2 = allocator.allocate().unwrap();
        assert_eq!(ip2.host(), 3);
        assert!(allocator.is_allocated(ip2));

        // Third allocation should be host 4
        let ip3 = allocator.allocate().unwrap();
        assert_eq!(ip3.host(), 4);
        assert!(allocator.is_allocated(ip3));

        // Available count should decrease
        assert_eq!(allocator.available_count(), 65534 - 3);
    }

    #[test]
    fn test_allocate_release_reuse() {
        let mut allocator = BitmapAllocator::new();

        // Allocate several IPs
        let ip1 = allocator.allocate().unwrap();
        let ip2 = allocator.allocate().unwrap();
        let ip3 = allocator.allocate().unwrap();

        assert_eq!(ip1.host(), 2);
        assert_eq!(ip2.host(), 3);
        assert_eq!(ip3.host(), 4);

        // Release the middle one
        allocator.release(ip2);
        assert!(!allocator.is_allocated(ip2));
        assert!(allocator.is_allocated(ip1));
        assert!(allocator.is_allocated(ip3));

        // Next allocation should reuse the released IP
        let ip4 = allocator.allocate().unwrap();
        assert_eq!(ip4.host(), 3); // Reused ip2's slot

        // Available count should be back to original - 3
        assert_eq!(allocator.available_count(), 65534 - 3);
    }

    #[test]
    fn test_reserve_success() {
        let mut allocator = BitmapAllocator::new();

        // Reserve a specific IP
        let specific = VirtualIp::new(100);
        assert!(allocator.reserve(specific).is_ok());
        assert!(allocator.is_allocated(specific));

        // Sequential allocation should skip it
        // Allocate from 2 to 99 (98 IPs)
        for i in 0..98 {
            let ip = allocator.allocate().unwrap();
            assert_eq!(ip.host(), 2 + i);
        }

        // Next allocation should skip 100 (reserved) and get 101
        let next = allocator.allocate().unwrap();
        assert_ne!(next.host(), 100); // Should have skipped 100
        assert_eq!(next.host(), 101); // Should be right after
    }

    #[test]
    fn test_reserve_duplicate_fails() {
        let mut allocator = BitmapAllocator::new();

        // Reserve an IP
        let ip = VirtualIp::new(50);
        assert!(allocator.reserve(ip).is_ok());

        // Try to reserve it again
        let result = allocator.reserve(ip);
        assert!(result.is_err());
        assert!(matches!(result, Err(PortalNetError::Configuration(_))));
    }

    #[test]
    fn test_reserve_allocated_fails() {
        let mut allocator = BitmapAllocator::new();

        // Allocate an IP
        let ip = allocator.allocate().unwrap();

        // Try to reserve it
        let result = allocator.reserve(ip);
        assert!(result.is_err());
        assert!(matches!(result, Err(PortalNetError::Configuration(_))));
    }

    #[test]
    fn test_reserve_hub_fails() {
        let mut allocator = BitmapAllocator::new();

        // Try to reserve the hub address
        let result = allocator.reserve(VirtualIp::HUB);
        assert!(result.is_err());
        assert!(matches!(result, Err(PortalNetError::Configuration(_))));
    }

    #[test]
    fn test_reserve_network_fails() {
        let mut allocator = BitmapAllocator::new();

        // Try to reserve the network address
        let result = allocator.reserve(VirtualIp::new(NETWORK_HOST));
        assert!(result.is_err());
        assert!(matches!(result, Err(PortalNetError::Configuration(_))));
    }

    #[test]
    fn test_is_allocated() {
        let mut allocator = BitmapAllocator::new();

        let ip = VirtualIp::new(42);
        assert!(!allocator.is_allocated(ip));

        allocator.reserve(ip).unwrap();
        assert!(allocator.is_allocated(ip));

        allocator.release(ip);
        assert!(!allocator.is_allocated(ip));
    }

    #[test]
    fn test_exhaustion() {
        let mut allocator = BitmapAllocator::new();

        // Allocate all available IPs (65534)
        let mut allocated_ips = Vec::new();
        while let Some(ip) = allocator.allocate() {
            allocated_ips.push(ip);
        }

        // Should have allocated all available IPs
        assert_eq!(allocated_ips.len(), 65534);
        assert_eq!(allocator.available_count(), 0);

        // Next allocation should return None
        assert!(allocator.allocate().is_none());

        // Release one IP
        allocator.release(allocated_ips[0]);
        assert_eq!(allocator.available_count(), 1);

        // Should now be able to allocate again
        let ip = allocator.allocate();
        assert!(ip.is_some());
        assert_eq!(ip.unwrap().host(), allocated_ips[0].host());

        // Should be exhausted again
        assert!(allocator.allocate().is_none());
        assert_eq!(allocator.available_count(), 0);
    }

    #[test]
    fn test_available_count() {
        let mut allocator = BitmapAllocator::new();

        // Initial count (65536 - 2 reserved = 65534)
        assert_eq!(allocator.available_count(), 65534);

        // Allocate one
        allocator.allocate();
        assert_eq!(allocator.available_count(), 65533);

        // Allocate more
        allocator.allocate();
        allocator.allocate();
        assert_eq!(allocator.available_count(), 65531);

        // Reserve one
        allocator.reserve(VirtualIp::new(1000)).unwrap();
        assert_eq!(allocator.available_count(), 65530);

        // Release one
        allocator.release(VirtualIp::new(2));
        assert_eq!(allocator.available_count(), 65531);
    }

    #[test]
    fn test_with_reserved() {
        let reserved = vec![VirtualIp::new(10), VirtualIp::new(20), VirtualIp::new(30)];

        let allocator = BitmapAllocator::with_reserved(&reserved);

        // Hub should still be reserved
        assert!(allocator.is_allocated(VirtualIp::HUB));

        // Custom reservations should be present
        assert!(allocator.is_allocated(VirtualIp::new(10)));
        assert!(allocator.is_allocated(VirtualIp::new(20)));
        assert!(allocator.is_allocated(VirtualIp::new(30)));

        // Others should not be allocated
        assert!(!allocator.is_allocated(VirtualIp::new(11)));
        assert!(!allocator.is_allocated(VirtualIp::new(21)));
        assert!(!allocator.is_allocated(VirtualIp::new(31)));

        // Available count should reflect reservations (65534 - 3 custom)
        assert_eq!(allocator.available_count(), 65531);
    }

    #[test]
    fn test_with_reserved_ignores_network_and_hub() {
        // Try to pass network and hub in reserved list
        let reserved = vec![
            VirtualIp::new(NETWORK_HOST),
            VirtualIp::HUB,
            VirtualIp::new(10),
        ];

        let allocator = BitmapAllocator::with_reserved(&reserved);

        // Should only have network, hub, and 10 allocated
        // Network and hub are always reserved, 10 is custom
        assert!(allocator.is_allocated(VirtualIp::new(NETWORK_HOST)));
        assert!(allocator.is_allocated(VirtualIp::HUB));
        assert!(allocator.is_allocated(VirtualIp::new(10)));

        // Should have correct count (only one custom reservation counted)
        assert_eq!(allocator.available_count(), 65533);
    }

    #[test]
    fn test_release_unallocated() {
        let mut allocator = BitmapAllocator::new();

        let ip = VirtualIp::new(42);
        assert!(!allocator.is_allocated(ip));

        // Releasing unallocated IP should be safe (no-op)
        allocator.release(ip);
        assert!(!allocator.is_allocated(ip));

        // Available count should remain unchanged
        assert_eq!(allocator.available_count(), 65534);
    }

    #[test]
    fn test_release_hub_is_noop() {
        let mut allocator = BitmapAllocator::new();

        // Hub is pre-allocated
        assert!(allocator.is_allocated(VirtualIp::HUB));

        // Try to release hub (should be no-op)
        allocator.release(VirtualIp::HUB);

        // Hub should still be allocated
        assert!(allocator.is_allocated(VirtualIp::HUB));
    }

    #[test]
    fn test_release_network_is_noop() {
        let mut allocator = BitmapAllocator::new();

        // Network is pre-allocated
        assert!(allocator.is_allocated(VirtualIp::new(NETWORK_HOST)));

        // Try to release network (should be no-op)
        allocator.release(VirtualIp::new(NETWORK_HOST));

        // Network should still be allocated
        assert!(allocator.is_allocated(VirtualIp::new(NETWORK_HOST)));
    }

    #[test]
    fn test_allocator_clone() {
        let mut allocator = BitmapAllocator::new();

        // Allocate some IPs
        let ip1 = allocator.allocate().unwrap();
        let ip2 = allocator.allocate().unwrap();

        // Clone the allocator
        let cloned = allocator.clone();

        // Both should have the same state
        assert!(cloned.is_allocated(ip1));
        assert!(cloned.is_allocated(ip2));
        assert_eq!(cloned.available_count(), allocator.available_count());
    }

    #[test]
    fn test_allocator_default() {
        let allocator = BitmapAllocator::default();

        // Should be same as new()
        assert!(allocator.is_allocated(VirtualIp::HUB));
        assert_eq!(allocator.available_count(), 65534);
    }

    #[test]
    fn test_allocation_wraparound() {
        let mut allocator = BitmapAllocator::new();

        // Reserve some IPs near the end
        allocator.reserve(VirtualIp::new(MAX_HOST - 1)).unwrap();
        allocator.reserve(VirtualIp::new(MAX_HOST)).unwrap();

        // Set hint near the end
        allocator.next_hint = MAX_HOST - 2;

        // Allocate one (should get MAX_HOST - 2)
        let ip1 = allocator.allocate().unwrap();
        assert_eq!(ip1.host(), MAX_HOST - 2);

        // Next allocation should wrap around to find available IPs
        let ip2 = allocator.allocate().unwrap();
        // Should find an earlier available IP (likely host 2)
        assert!(ip2.host() < MAX_HOST - 2);
    }

    #[test]
    fn test_send_sync_trait_bounds() {
        // Compile-time test that BitmapAllocator is Send + Sync
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BitmapAllocator>();
    }

    #[test]
    fn test_concurrent_usage_pattern() {
        use std::sync::{Arc, Mutex};

        // Demonstrate thread-safe usage pattern
        let allocator = Arc::new(Mutex::new(BitmapAllocator::new()));

        let allocator_clone = Arc::clone(&allocator);

        // Simulate allocation from another context
        let ip = {
            let mut alloc = allocator_clone.lock().unwrap();
            alloc.allocate().unwrap()
        };

        // Verify from original context
        {
            let alloc = allocator.lock().unwrap();
            assert!(alloc.is_allocated(ip));
        }
    }

    #[test]
    fn test_large_scale_allocation() {
        let mut allocator = BitmapAllocator::new();

        // Allocate 1000 IPs
        let mut ips = Vec::new();
        for _ in 0..1000 {
            ips.push(allocator.allocate().unwrap());
        }

        // Verify all are allocated
        assert_eq!(ips.len(), 1000);
        for ip in &ips {
            assert!(allocator.is_allocated(*ip));
        }

        // Release every other one
        for (i, ip) in ips.iter().enumerate() {
            if i % 2 == 0 {
                allocator.release(*ip);
            }
        }

        // Should have 500 released
        assert_eq!(allocator.available_count(), 65534 - 500);

        // Allocate 500 more (should reuse released ones)
        let mut new_ips = Vec::new();
        for _ in 0..500 {
            new_ips.push(allocator.allocate().unwrap());
        }

        // Should be back to 1000 allocated
        assert_eq!(allocator.available_count(), 65534 - 1000);
    }

    #[test]
    fn test_edge_case_max_host() {
        let mut allocator = BitmapAllocator::new();

        // Reserve MAX_HOST
        let max_ip = VirtualIp::new(MAX_HOST);
        allocator.reserve(max_ip).unwrap();
        assert!(allocator.is_allocated(max_ip));

        // Should still be able to allocate normally
        let ip = allocator.allocate().unwrap();
        assert_eq!(ip.host(), FIRST_ALLOCATABLE);
    }

    #[test]
    fn test_fragmented_allocation() {
        let mut allocator = BitmapAllocator::new();

        // Create a fragmented allocation pattern
        // Allocate 100 IPs
        let mut ips = Vec::new();
        for _ in 0..100 {
            ips.push(allocator.allocate().unwrap());
        }

        // Release odd-indexed ones to create fragmentation
        for (i, ip) in ips.iter().enumerate() {
            if i % 2 == 1 {
                allocator.release(*ip);
            }
        }

        // Allocate should fill in the gaps
        let new_ip = allocator.allocate().unwrap();
        // Should reuse one of the released slots
        assert!(ips.contains(&new_ip) || new_ip.host() == 102);
    }

    #[test]
    fn test_allocator_debug_format() {
        let allocator = BitmapAllocator::new();
        let debug_str = format!("{:?}", allocator);

        // Should contain BitmapAllocator in debug output
        assert!(debug_str.contains("BitmapAllocator"));
    }

    #[test]
    fn test_error_messages() {
        let mut allocator = BitmapAllocator::new();

        // Test hub reservation error message
        let result = allocator.reserve(VirtualIp::HUB);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("hub"));

        // Test network reservation error message
        let result = allocator.reserve(VirtualIp::new(NETWORK_HOST));
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("network"));

        // Test duplicate allocation error message
        let ip = VirtualIp::new(50);
        allocator.reserve(ip).unwrap();
        let result = allocator.reserve(ip);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("already allocated"));
    }
}
