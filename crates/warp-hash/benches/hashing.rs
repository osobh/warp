use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use warp_hash::{hash, hash_chunks_parallel};

fn bench_hashing(c: &mut Criterion) {
    let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
    
    let mut group = c.benchmark_group("hashing");
    group.throughput(Throughput::Bytes(data.len() as u64));
    
    group.bench_function("blake3-1mb", |b| {
        b.iter(|| hash(black_box(&data)))
    });
    
    // Parallel hashing
    let chunks: Vec<&[u8]> = data.chunks(64 * 1024).collect(); // 64KB chunks
    group.bench_function("blake3-parallel-16x64kb", |b| {
        b.iter(|| hash_chunks_parallel(black_box(&chunks)))
    });
    
    group.finish();
}

criterion_group!(benches, bench_hashing);
criterion_main!(benches);
