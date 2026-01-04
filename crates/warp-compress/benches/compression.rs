use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use warp_compress::{Compressor, Lz4Compressor, ZstdCompressor};

fn bench_compression(c: &mut Criterion) {
    let data: Vec<u8> = (0..1024 * 1024)
        .map(|i| ((i * 17 + 31) % 256) as u8)
        .collect();

    let mut group = c.benchmark_group("compression");
    group.throughput(Throughput::Bytes(data.len() as u64));

    let zstd = ZstdCompressor::default();
    group.bench_function("zstd-3", |b| b.iter(|| zstd.compress(black_box(&data))));

    let lz4 = Lz4Compressor::new();
    group.bench_function("lz4", |b| b.iter(|| lz4.compress(black_box(&data))));

    group.finish();
}

criterion_group!(benches, bench_compression);
criterion_main!(benches);
