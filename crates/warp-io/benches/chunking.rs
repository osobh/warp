use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use warp_io::{Chunker, ChunkerConfig};
use std::io::Cursor;

fn bench_chunking(c: &mut Criterion) {
    // Generate test data with some patterns
    let data: Vec<u8> = (0..10 * 1024 * 1024)
        .map(|i| {
            if i % 1000 == 0 {
                0 // Periodic zeros (potential chunk boundaries)
            } else {
                ((i * 17 + 31) % 256) as u8
            }
        })
        .collect();
    
    let mut group = c.benchmark_group("chunking");
    group.throughput(Throughput::Bytes(data.len() as u64));
    
    let chunker = Chunker::default();
    group.bench_function("buzhash-10mb", |b| {
        b.iter(|| chunker.chunk(Cursor::new(black_box(&data))))
    });
    
    group.finish();
}

criterion_group!(benches, bench_chunking);
criterion_main!(benches);
