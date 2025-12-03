//! CPU compression implementations

mod zstd;
mod lz4;

pub use self::zstd::ZstdCompressor;
pub use self::lz4::Lz4Compressor;
