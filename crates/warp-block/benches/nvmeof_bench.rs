//! NVMe-oF Benchmarks
//!
//! Performance benchmarks for NVMe-oF components.

#![cfg(feature = "nvmeof")]

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::sync::Arc;

use bytes::Bytes;
use warp_block::nvmeof::{
    capsule::{CommandCapsule, ResponseCapsule},
    command::{NvmeCommand, NvmeCompletion},
    queue::{AdminQueue, IoQueue, QueueManager},
};

/// Benchmark NvmeCommand creation and serialization
fn bench_command_creation(c: &mut Criterion) {
    c.bench_function("nvme_command_new", |b| {
        b.iter(|| {
            let cmd = NvmeCommand::new();
            black_box(cmd)
        })
    });
}

/// Benchmark command capsule serialization
fn bench_capsule_serialization(c: &mut Criterion) {
    let cmd = NvmeCommand::new();
    let capsule = CommandCapsule::new(cmd);

    c.bench_function("command_capsule_to_bytes", |b| {
        b.iter(|| {
            let bytes = capsule.to_bytes();
            black_box(bytes)
        })
    });

    let bytes = capsule.to_bytes();
    c.bench_function("command_capsule_from_bytes", |b| {
        b.iter(|| {
            let parsed = CommandCapsule::from_bytes(&bytes).unwrap();
            black_box(parsed)
        })
    });
}

/// Benchmark response capsule serialization
fn bench_response_serialization(c: &mut Criterion) {
    let completion = NvmeCompletion::success(1, 0, 0);
    let capsule = ResponseCapsule::new(completion);

    c.bench_function("response_capsule_to_bytes", |b| {
        b.iter(|| {
            let bytes = capsule.to_bytes();
            black_box(bytes)
        })
    });

    let bytes = capsule.to_bytes();
    c.bench_function("response_capsule_from_bytes", |b| {
        b.iter(|| {
            let parsed = ResponseCapsule::from_bytes(&bytes).unwrap();
            black_box(parsed)
        })
    });
}

/// Benchmark queue operations
fn bench_queue_operations(c: &mut Criterion) {
    let queue = AdminQueue::new(256);

    c.bench_function("admin_queue_submit_complete", |b| {
        b.iter(|| {
            let cmd = NvmeCommand::new();
            let capsule = CommandCapsule::new(cmd);
            let cid = queue.submit(capsule).unwrap();

            let completion = NvmeCompletion::success(cid, 0, 0);
            let response = ResponseCapsule::new(completion);
            queue.complete(response).unwrap();

            black_box(cid)
        })
    });
}

/// Benchmark I/O queue throughput
fn bench_io_queue_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("io_queue_throughput");
    group.throughput(Throughput::Elements(1));

    let queue = IoQueue::new(1, 1024);

    group.bench_function("io_queue_submit", |b| {
        b.iter(|| {
            let cmd = NvmeCommand::new();
            let capsule = CommandCapsule::new(cmd);
            let cid = queue.submit(capsule).unwrap();

            // Complete immediately
            let completion = NvmeCompletion::success(cid, 1, 0);
            let response = ResponseCapsule::new(completion);
            queue.complete(response, 4096).unwrap();

            black_box(cid)
        })
    });

    group.finish();
}

/// Benchmark queue manager operations
fn bench_queue_manager(c: &mut Criterion) {
    c.bench_function("queue_manager_create_delete", |b| {
        let manager = QueueManager::new(64, 128);
        let mut qid = 1u16;

        b.iter(|| {
            manager.create_io_queue(qid, 32).unwrap();
            manager.delete_io_queue(qid).unwrap();
            qid = qid.wrapping_add(1);
            if qid == 0 {
                qid = 1;
            }
            black_box(qid)
        })
    });
}

/// Benchmark capsule with data
fn bench_capsule_with_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("capsule_with_data");

    for size in [512, 4096, 65536, 1048576].iter() {
        let data = Bytes::from(vec![0u8; *size]);
        let cmd = NvmeCommand::new();
        let capsule = CommandCapsule::with_data(cmd, data.clone());

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(
            criterion::BenchmarkId::new("serialize", size),
            size,
            |b, _| {
                b.iter(|| {
                    let bytes = capsule.to_bytes();
                    black_box(bytes)
                })
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_command_creation,
    bench_capsule_serialization,
    bench_response_serialization,
    bench_queue_operations,
    bench_io_queue_throughput,
    bench_queue_manager,
    bench_capsule_with_data,
);

criterion_main!(benches);
