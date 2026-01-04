//! Encryption benchmarks for streaming pipeline
//!
//! Compares GPU vs CPU encryption performance at various data sizes.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use warp_stream::{StreamConfig, create_crypto_context};

/// Benchmark data sizes to test
const SIZES: &[usize] = &[
    64 * 1024,        // 64 KB
    256 * 1024,       // 256 KB
    1024 * 1024,      // 1 MB
    4 * 1024 * 1024,  // 4 MB
    16 * 1024 * 1024, // 16 MB
];

fn bench_cpu_encryption(c: &mut Criterion) {
    let mut group = c.benchmark_group("cpu_encryption");

    let key = [0x42u8; 32];
    let nonce = [0x24u8; 12];

    // Create CPU-only config
    let config = StreamConfig::new().with_gpu(false);
    let ctx = create_crypto_context(&key, &nonce, &config).unwrap();

    for size in SIZES {
        group.throughput(Throughput::Bytes(*size as u64));

        let data = vec![0xABu8; *size];

        group.bench_with_input(
            BenchmarkId::new("encrypt", format!("{}KB", size / 1024)),
            &data,
            |b, data| b.iter(|| black_box(ctx.encrypt(data, &key, &nonce).unwrap())),
        );
    }

    group.finish();
}

fn bench_gpu_encryption(c: &mut Criterion) {
    let mut group = c.benchmark_group("gpu_encryption");

    let key = [0x42u8; 32];
    let nonce = [0x24u8; 12];

    // Create GPU-enabled config
    let config = StreamConfig::new().with_gpu(true);
    let ctx = match create_crypto_context(&key, &nonce, &config) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("Skipping GPU benchmarks: {}", e);
            return;
        }
    };

    // Print which backend we're using
    eprintln!("GPU encryption benchmark using: {}", ctx.backend());

    for size in SIZES {
        group.throughput(Throughput::Bytes(*size as u64));

        let data = vec![0xABu8; *size];

        group.bench_with_input(
            BenchmarkId::new("encrypt", format!("{}KB", size / 1024)),
            &data,
            |b, data| b.iter(|| black_box(ctx.encrypt(data, &key, &nonce).unwrap())),
        );
    }

    group.finish();
}

fn bench_encrypt_decrypt_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("roundtrip");

    let key = [0x42u8; 32];
    let nonce = [0x24u8; 12];

    // CPU context
    let cpu_config = StreamConfig::new().with_gpu(false);
    let cpu_ctx = create_crypto_context(&key, &nonce, &cpu_config).unwrap();

    // GPU context (falls back to CPU if unavailable)
    let gpu_config = StreamConfig::new().with_gpu(true);
    let gpu_ctx = create_crypto_context(&key, &nonce, &gpu_config).unwrap();

    let size = 1024 * 1024; // 1 MB
    let data = vec![0xABu8; size];

    group.throughput(Throughput::Bytes(size as u64 * 2)); // encrypt + decrypt

    group.bench_function("cpu", |b| {
        b.iter(|| {
            let encrypted = cpu_ctx.encrypt(&data, &key, &nonce).unwrap();
            black_box(cpu_ctx.decrypt(&encrypted, &key, &nonce).unwrap())
        })
    });

    group.bench_function("gpu", |b| {
        b.iter(|| {
            let encrypted = gpu_ctx.encrypt(&data, &key, &nonce).unwrap();
            black_box(gpu_ctx.decrypt(&encrypted, &key, &nonce).unwrap())
        })
    });

    group.finish();
}

fn bench_streaming_chunks(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming_chunks");

    let key = [0x42u8; 32];
    let nonce = [0x24u8; 12];

    // Create contexts
    let cpu_config = StreamConfig::low_latency().with_gpu(false);
    let cpu_ctx = create_crypto_context(&key, &nonce, &cpu_config).unwrap();

    let gpu_config = StreamConfig::low_latency().with_gpu(true);
    let gpu_ctx = create_crypto_context(&key, &nonce, &gpu_config).unwrap();

    // Simulate streaming: 100 chunks of 64KB each
    let chunk_count = 100;
    let chunk_size = 64 * 1024;
    let total_size = chunk_count * chunk_size;

    let chunks: Vec<Vec<u8>> = (0..chunk_count).map(|_| vec![0xABu8; chunk_size]).collect();

    group.throughput(Throughput::Bytes(total_size as u64));

    group.bench_function("cpu_100x64KB", |b| {
        b.iter(|| {
            for chunk in &chunks {
                black_box(cpu_ctx.encrypt(chunk, &key, &nonce).unwrap());
            }
        })
    });

    group.bench_function("gpu_100x64KB", |b| {
        b.iter(|| {
            for chunk in &chunks {
                black_box(gpu_ctx.encrypt(chunk, &key, &nonce).unwrap());
            }
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_cpu_encryption,
    bench_gpu_encryption,
    bench_encrypt_decrypt_roundtrip,
    bench_streaming_chunks,
);

criterion_main!(benches);
