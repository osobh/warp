//! End-to-end integration tests for warp-store-api
//!
//! These tests spin up an actual HTTP server and test the full API flow.

use std::net::SocketAddr;
use std::time::Duration;

use reqwest::Client;
use tokio::net::TcpListener;
use tokio::time::timeout;

use warp_store::{Store, StoreConfig};
use warp_store_api::{ApiServer, ApiConfig};

/// Test server helper
struct TestServer {
    addr: SocketAddr,
    client: Client,
    _temp_dir: tempfile::TempDir,
}

impl TestServer {
    async fn new() -> Self {
        let temp_dir = tempfile::tempdir().unwrap();
        let store_config = StoreConfig {
            root_path: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let store = Store::new(store_config).await.unwrap();

        // Find an available port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let api_config = ApiConfig {
            bind_addr: addr,
            enable_s3: true,
            enable_native: true,
            ..Default::default()
        };

        let server = ApiServer::new(store, api_config).await;
        let router = server.router();

        // Spawn the server in the background
        let listener = TcpListener::bind(addr).await.unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });

        // Wait for server to be ready
        tokio::time::sleep(Duration::from_millis(50)).await;

        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap();

        Self {
            addr,
            client,
            _temp_dir: temp_dir,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }
}

// =============================================================================
// Health Check Tests
// =============================================================================

#[tokio::test]
async fn test_health_check() {
    let server = TestServer::new().await;

    let resp = server.client.get(server.url("/health")).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "OK");
}

// =============================================================================
// S3 Bucket Operations
// =============================================================================

#[tokio::test]
async fn test_s3_list_buckets_empty() {
    let server = TestServer::new().await;

    let resp = server.client.get(server.url("/")).send().await.unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<ListAllMyBucketsResult>"));
    assert!(body.contains("<Buckets>"));
}

#[tokio::test]
async fn test_s3_create_bucket() {
    let server = TestServer::new().await;

    // Create bucket
    let resp = server.client
        .put(server.url("/test-bucket"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // Verify bucket exists in listing
    let resp = server.client.get(server.url("/")).send().await.unwrap();
    let body = resp.text().await.unwrap();
    assert!(body.contains("test-bucket"));
}

#[tokio::test]
async fn test_s3_delete_bucket() {
    let server = TestServer::new().await;

    // Create bucket
    server.client
        .put(server.url("/delete-me"))
        .send()
        .await
        .unwrap();

    // Delete bucket
    let resp = server.client
        .delete(server.url("/delete-me"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 204);

    // Verify bucket no longer in listing
    let resp = server.client.get(server.url("/")).send().await.unwrap();
    let body = resp.text().await.unwrap();
    assert!(!body.contains("delete-me"));
}

// =============================================================================
// S3 Object Operations
// =============================================================================

#[tokio::test]
async fn test_s3_put_get_object() {
    let server = TestServer::new().await;

    // Create bucket first
    server.client
        .put(server.url("/objects"))
        .send()
        .await
        .unwrap();

    // Put object
    let data = b"Hello, warp-store!";
    let resp = server.client
        .put(server.url("/objects/hello.txt"))
        .body(data.to_vec())
        .header("Content-Type", "text/plain")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert!(resp.headers().contains_key("etag"));

    // Get object
    let resp = server.client
        .get(server.url("/objects/hello.txt"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.bytes().await.unwrap().as_ref(), data);
}

#[tokio::test]
async fn test_s3_head_object() {
    let server = TestServer::new().await;

    // Create bucket and object
    server.client.put(server.url("/head-test")).send().await.unwrap();

    let data = b"Test data for head";
    server.client
        .put(server.url("/head-test/file.bin"))
        .body(data.to_vec())
        .send()
        .await
        .unwrap();

    // Head object
    let resp = server.client
        .head(server.url("/head-test/file.bin"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert!(resp.headers().contains_key("content-length"));
    assert!(resp.headers().contains_key("etag"));
}

#[tokio::test]
async fn test_s3_delete_object() {
    let server = TestServer::new().await;

    // Create bucket and object
    server.client.put(server.url("/del-obj")).send().await.unwrap();

    server.client
        .put(server.url("/del-obj/to-delete.txt"))
        .body("delete me")
        .send()
        .await
        .unwrap();

    // Delete object
    let resp = server.client
        .delete(server.url("/del-obj/to-delete.txt"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 204);

    // Verify object is gone
    let resp = server.client
        .get(server.url("/del-obj/to-delete.txt"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_s3_list_objects() {
    let server = TestServer::new().await;

    // Create bucket with multiple objects
    server.client.put(server.url("/list-test")).send().await.unwrap();

    for i in 0..5 {
        server.client
            .put(server.url(&format!("/list-test/file{}.txt", i)))
            .body(format!("content {}", i))
            .send()
            .await
            .unwrap();
    }

    // List objects
    let resp = server.client
        .get(server.url("/list-test"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<ListBucketResult>"));
    assert!(body.contains("file0.txt"));
    assert!(body.contains("file4.txt"));
}

#[tokio::test]
async fn test_s3_list_objects_with_prefix() {
    let server = TestServer::new().await;

    // Create bucket with objects in subdirectories
    server.client.put(server.url("/prefix-test")).send().await.unwrap();

    for path in ["dir1/a.txt", "dir1/b.txt", "dir2/c.txt", "root.txt"] {
        server.client
            .put(server.url(&format!("/prefix-test/{}", path)))
            .body("content")
            .send()
            .await
            .unwrap();
    }

    // List with prefix
    let resp = server.client
        .get(server.url("/prefix-test?prefix=dir1/"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("dir1/a.txt"));
    assert!(body.contains("dir1/b.txt"));
    assert!(!body.contains("dir2/c.txt"));
    assert!(!body.contains("root.txt"));
}

// =============================================================================
// Native API Tests
// =============================================================================

#[tokio::test]
async fn test_native_stats() {
    let server = TestServer::new().await;

    // Create some buckets
    server.client.put(server.url("/bucket1")).send().await.unwrap();
    server.client.put(server.url("/bucket2")).send().await.unwrap();

    // Get stats
    let resp = server.client
        .get(server.url("/api/v1/stats"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["buckets"], 2);
}

#[tokio::test]
async fn test_native_ephemeral_token() {
    let server = TestServer::new().await;

    // Create bucket and object
    server.client.put(server.url("/ephemeral")).send().await.unwrap();
    server.client
        .put(server.url("/ephemeral/secret.bin"))
        .body("secret data")
        .send()
        .await
        .unwrap();

    // Create ephemeral token
    let resp = server.client
        .post(server.url("/api/v1/ephemeral"))
        .json(&serde_json::json!({
            "bucket": "ephemeral",
            "key": "secret.bin",
            "ttl_seconds": 3600
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(body["token"].is_string());
    assert!(body["expires_at"].is_string());
    assert!(body["url"].is_string());

    // Verify token
    let token = body["token"].as_str().unwrap();
    let verify_resp = server.client
        .post(server.url("/api/v1/ephemeral/verify"))
        .json(&serde_json::json!({
            "token": token
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(verify_resp.status(), 200);
    let verify_body: serde_json::Value = verify_resp.json().await.unwrap();
    assert_eq!(verify_body["valid"], true);
}

#[tokio::test]
async fn test_native_ephemeral_access() {
    let server = TestServer::new().await;

    // Create bucket and object
    server.client.put(server.url("/access-test")).send().await.unwrap();
    let content = "This is protected content";
    server.client
        .put(server.url("/access-test/protected.txt"))
        .body(content)
        .send()
        .await
        .unwrap();

    // Create ephemeral token with bucket scope (allows any key in bucket)
    let resp = server.client
        .post(server.url("/api/v1/ephemeral"))
        .json(&serde_json::json!({
            "bucket": "access-test",
            "key": "",
            "ttl_seconds": 3600,
            "scope": "bucket",
            "permissions": {
                "read": true,
                "write": false,
                "delete": false,
                "list": false
            }
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let token = body["token"].as_str().unwrap();

    // Access via ephemeral URL with the key in path
    let access_url = format!("/api/v1/access/{}/protected.txt", token);
    let resp = server.client
        .get(server.url(&access_url))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), content);
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[tokio::test]
async fn test_get_nonexistent_object() {
    let server = TestServer::new().await;

    server.client.put(server.url("/errors")).send().await.unwrap();

    let resp = server.client
        .get(server.url("/errors/does-not-exist.txt"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn test_delete_nonexistent_bucket() {
    let server = TestServer::new().await;

    let resp = server.client
        .delete(server.url("/nonexistent-bucket"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 404);
}

// =============================================================================
// Large Object Tests
// =============================================================================

#[tokio::test]
async fn test_large_object() {
    let server = TestServer::new().await;

    server.client.put(server.url("/large")).send().await.unwrap();

    // Create 1MB object
    let data: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();

    let resp = server.client
        .put(server.url("/large/big-file.bin"))
        .body(data.clone())
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    // Retrieve and verify
    let resp = server.client
        .get(server.url("/large/big-file.bin"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let received = resp.bytes().await.unwrap();
    assert_eq!(received.len(), data.len());
    assert_eq!(received.as_ref(), data.as_slice());
}

// =============================================================================
// Multipart Upload Tests
// =============================================================================

#[tokio::test]
async fn test_s3_multipart_upload() {
    let server = TestServer::new().await;

    // Create bucket
    server.client.put(server.url("/multipart")).send().await.unwrap();

    // 1. Create multipart upload
    let resp = server.client
        .post(server.url("/multipart/large-file.bin?uploads"))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<InitiateMultipartUploadResult>"));
    assert!(body.contains("<UploadId>"));

    // Extract upload ID from XML
    let upload_id = body
        .split("<UploadId>")
        .nth(1)
        .and_then(|s| s.split("</UploadId>").next())
        .unwrap();

    // 2. Upload parts
    let part1_data = b"Part 1 data ".to_vec();
    let resp = server.client
        .put(server.url(&format!(
            "/multipart/large-file.bin?uploadId={}&partNumber=1",
            upload_id
        )))
        .body(part1_data.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(resp.headers().contains_key("etag"));

    let part2_data = b"Part 2 data".to_vec();
    let resp = server.client
        .put(server.url(&format!(
            "/multipart/large-file.bin?uploadId={}&partNumber=2",
            upload_id
        )))
        .body(part2_data.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // 3. Complete multipart upload
    let resp = server.client
        .post(server.url(&format!(
            "/multipart/large-file.bin?uploadId={}",
            upload_id
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();
    assert!(body.contains("<CompleteMultipartUploadResult>"));

    // 4. Verify final object
    let resp = server.client
        .get(server.url("/multipart/large-file.bin"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let data = resp.bytes().await.unwrap();
    let expected: Vec<u8> = [part1_data, part2_data].concat();
    assert_eq!(data.as_ref(), expected.as_slice());
}

#[tokio::test]
async fn test_s3_multipart_upload_abort() {
    let server = TestServer::new().await;

    // Create bucket
    server.client.put(server.url("/abort-test")).send().await.unwrap();

    // Create multipart upload
    let resp = server.client
        .post(server.url("/abort-test/to-abort.bin?uploads"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body = resp.text().await.unwrap();

    let upload_id = body
        .split("<UploadId>")
        .nth(1)
        .and_then(|s| s.split("</UploadId>").next())
        .unwrap();

    // Upload a part
    server.client
        .put(server.url(&format!(
            "/abort-test/to-abort.bin?uploadId={}&partNumber=1",
            upload_id
        )))
        .body("some data")
        .send()
        .await
        .unwrap();

    // Abort the upload
    let resp = server.client
        .delete(server.url(&format!(
            "/abort-test/to-abort.bin?uploadId={}",
            upload_id
        )))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);

    // Object should not exist
    let resp = server.client
        .get(server.url("/abort-test/to-abort.bin"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}
