//! Performance benchmarks for warp-store
//!
//! Benchmarks cover:
//! - Object put/get operations
//! - Ephemeral token generation/verification
//! - Transport tier selection
//! - Parcode field access (when feature enabled)

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId, Throughput};
use std::time::Duration;
use tempfile::TempDir;

use warp_store::{
    Store, StoreConfig, ObjectKey, ObjectData,
    EphemeralToken, AccessScope, Permissions,
    transport::{StorageTransport, TransportConfig, PeerLocation},
};

/// Setup a temporary store for benchmarks
async fn setup_store() -> (Store<warp_store::backend::LocalBackend>, TempDir) {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = StoreConfig {
        root_path: temp_dir.path().to_path_buf(),
        ..Default::default()
    };
    let store = Store::new(config).await.unwrap();
    store.create_bucket("bench", Default::default()).await.unwrap();
    (store, temp_dir)
}

/// Generate random data of specified size
fn generate_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

fn bench_put_get(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (store, _temp_dir) = rt.block_on(setup_store());

    let mut group = c.benchmark_group("put_get");

    for size in [1024, 4096, 16384, 65536, 262144, 1048576] {
        let data = generate_data(size);
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("put", size), &data, |b, data| {
            let key = ObjectKey::new("bench", "test-object").unwrap();
            b.to_async(&rt).iter(|| async {
                let key = key.clone();
                let data = ObjectData::from(data.clone());
                black_box(store.put(&key, data).await.unwrap());
            });
        });

        // Setup for get benchmark
        let key = ObjectKey::new("bench", &format!("get-object-{}", size)).unwrap();
        rt.block_on(async {
            store.put(&key, ObjectData::from(data.clone())).await.unwrap();
        });

        group.bench_with_input(BenchmarkId::new("get", size), &key, |b, key| {
            b.to_async(&rt).iter(|| async {
                black_box(store.get(key).await.unwrap());
            });
        });
    }

    group.finish();
}

fn bench_ephemeral_token(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let (store, _temp_dir) = rt.block_on(setup_store());

    let key = ObjectKey::new("bench", "secret.bin").unwrap();

    let mut group = c.benchmark_group("ephemeral_token");

    // Token generation
    group.bench_function("generate", |b| {
        b.iter(|| {
            black_box(store.create_ephemeral_url(&key, Duration::from_secs(3600)).unwrap());
        });
    });

    // Token with full options
    group.bench_function("generate_full", |b| {
        b.iter(|| {
            black_box(store.create_ephemeral_url_with_options(
                AccessScope::Object(key.clone()),
                Permissions::READ_WRITE,
                Duration::from_secs(3600),
                Some(vec!["10.0.0.0/8".parse().unwrap()]),
                None,
            ).unwrap());
        });
    });

    // Token verification
    let token = store.create_ephemeral_url(&key, Duration::from_secs(3600)).unwrap();
    group.bench_function("verify", |b| {
        b.iter(|| {
            black_box(store.verify_token(&token, None).unwrap());
        });
    });

    // Token encode/decode
    let encoded = token.encode();
    group.bench_function("encode", |b| {
        b.iter(|| {
            black_box(token.encode());
        });
    });

    group.bench_function("decode", |b| {
        b.iter(|| {
            black_box(EphemeralToken::decode(&encoded).unwrap());
        });
    });

    group.finish();
}

fn bench_transport_tier(c: &mut Criterion) {
    let config = TransportConfig::default();
    let transport = StorageTransport::new(config);
    let local = PeerLocation::local();

    let mut group = c.benchmark_group("transport_tier");

    // Same process tier detection
    let same_process_peer = PeerLocation::local();
    group.bench_function("tier_same_process", |b| {
        b.iter(|| {
            black_box(same_process_peer.optimal_tier(&local));
        });
    });

    // Same machine tier detection
    let same_machine_peer = PeerLocation::same_machine("/tmp/test.sock".into());
    group.bench_function("tier_same_machine", |b| {
        b.iter(|| {
            black_box(same_machine_peer.optimal_tier(&local));
        });
    });

    // Network tier detection
    let network_peer = PeerLocation::network(
        "peer-1".into(),
        "10.0.0.1:9000".parse().unwrap(),
        Some("us-east-1a".into()),
    );
    group.bench_function("tier_network", |b| {
        b.iter(|| {
            black_box(network_peer.optimal_tier(&local));
        });
    });

    // Route addition and lookup
    let key = ObjectKey::new("bench", "routed-key").unwrap();
    transport.add_route(&key, same_machine_peer.clone());

    group.bench_function("route_lookup", |b| {
        b.iter(|| {
            black_box(transport.get_tier(&key));
        });
    });

    group.bench_function("route_add", |b| {
        let key = ObjectKey::new("bench", "new-key").unwrap();
        b.iter(|| {
            transport.add_route(&key, same_machine_peer.clone());
        });
    });

    group.finish();
}

fn bench_object_key(c: &mut Criterion) {
    let mut group = c.benchmark_group("object_key");

    // Key parsing
    group.bench_function("parse_simple", |b| {
        b.iter(|| {
            black_box(ObjectKey::new("bucket", "key").unwrap());
        });
    });

    group.bench_function("parse_nested", |b| {
        b.iter(|| {
            black_box(ObjectKey::new("bucket", "path/to/deeply/nested/object.bin").unwrap());
        });
    });

    // Key comparison
    let key1 = ObjectKey::new("bucket", "key1").unwrap();
    let key2 = ObjectKey::new("bucket", "key2").unwrap();

    group.bench_function("compare", |b| {
        b.iter(|| {
            black_box(key1 == key2);
        });
    });

    // Prefix matching
    let key = ObjectKey::new("bucket", "prefix/subdir/file.bin").unwrap();
    group.bench_function("prefix_match", |b| {
        b.iter(|| {
            black_box(key.matches_prefix("prefix/"));
        });
    });

    group.finish();
}

fn bench_collective_context(c: &mut Criterion) {
    use warp_store::collective::{Rank, CollectiveContext};

    let mut group = c.benchmark_group("collective");

    // Context creation
    group.bench_function("context_new", |b| {
        b.iter(|| {
            black_box(CollectiveContext::new(64, Rank::new(0)));
        });
    });

    // Rank iteration
    let ctx = CollectiveContext::new(64, Rank::new(32));
    group.bench_function("all_ranks_iter", |b| {
        b.iter(|| {
            let ranks: Vec<Rank> = ctx.all_ranks().collect();
            black_box(ranks);
        });
    });

    group.bench_function("other_ranks_iter", |b| {
        b.iter(|| {
            let ranks: Vec<Rank> = ctx.other_ranks().collect();
            black_box(ranks);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_put_get,
    bench_ephemeral_token,
    bench_transport_tier,
    bench_object_key,
    bench_collective_context,
);

criterion_main!(benches);
