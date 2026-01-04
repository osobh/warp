//! CPU compression implementations

mod lz4;
mod zstd;

pub use self::lz4::Lz4Compressor;
pub use self::zstd::ZstdCompressor;
