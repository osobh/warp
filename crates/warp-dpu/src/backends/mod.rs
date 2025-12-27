//! DPU Backend Implementations
//!
//! Contains concrete implementations of the `DpuBackend` trait.

#[cfg(feature = "stub")]
pub mod stub;

#[cfg(feature = "bluefield")]
pub mod bluefield;

// Re-exports
#[cfg(feature = "stub")]
pub use stub::StubBackend;

#[cfg(feature = "bluefield")]
pub use bluefield::BlueFieldBackend;
