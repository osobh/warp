//! DPU throughput benchmarks

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use warp_dpu::{
    CpuCipher, CpuCompressor, CpuErasureCoder, CpuHasher, DpuCipher, DpuCompressor,
    DpuErasureCoder, DpuHasher,
};

fn bench_hasher(c: &mut Criterion) {
    let hasher = CpuHasher::new();
    let sizes = [1024, 4096, 16384, 65536, 262144, 1048576];

    let mut group = c.benchmark_group("hasher");
    for size in sizes {
        let data = vec![0u8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("blake3", size), &data, |b, data| {
            b.iter(|| hasher.hash(black_box(data)).unwrap())
        });
    }
    group.finish();
}

fn bench_cipher(c: &mut Criterion) {
    let cipher = CpuCipher::new();
    let key = [0u8; 32];
    let nonce = [0u8; 12];
    let sizes = [1024, 4096, 16384, 65536, 262144];

    let mut group = c.benchmark_group("cipher");
    for size in sizes {
        let data = vec![0u8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("encrypt", size), &data, |b, data| {
            b.iter(|| cipher.encrypt(black_box(data), &key, &nonce, None).unwrap())
        });
    }
    group.finish();
}

fn bench_compressor(c: &mut Criterion) {
    let zstd = CpuCompressor::zstd();
    let lz4 = CpuCompressor::lz4();
    let sizes = [4096, 16384, 65536, 262144];

    let mut group = c.benchmark_group("compressor");
    for size in sizes {
        // Create compressible data
        let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();

        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("zstd", size), &data, |b, data| {
            b.iter(|| zstd.compress(black_box(data)).unwrap())
        });
        group.bench_with_input(BenchmarkId::new("lz4", size), &data, |b, data| {
            b.iter(|| lz4.compress(black_box(data)).unwrap())
        });
    }
    group.finish();
}

fn bench_erasure(c: &mut Criterion) {
    let coder = CpuErasureCoder::with_config(4, 2);
    let sizes = [4096, 16384, 65536];

    let mut group = c.benchmark_group("erasure");
    for size in sizes {
        let data = vec![0u8; size];
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::new("encode", size), &data, |b, data| {
            b.iter(|| coder.encode(black_box(data)).unwrap())
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_hasher,
    bench_cipher,
    bench_compressor,
    bench_erasure
);
criterion_main!(benches);
