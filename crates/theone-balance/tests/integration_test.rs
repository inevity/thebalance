//! tests/integration_test.rs

use mockito;
use one_balance_rust::{
    d1_storage,
    models::*,
    state::strategy::{ApiKey, ApiKeyStatus},
    AppState,
};
use std::sync::Arc;
use tokio;
use worker::{D1Database, Env};

// Helper to set up a test environment with a mock D1 database.
async fn setup_test_env() -> (Env, D1Database, String) {
    // For this test, we'll have to assume the environment is configured correctly.
    // This is a limitation of the current test setup.
    // In a real integration test, we would use something like miniflare.
    let env = Env::empty();
    let db = env.d1("DB").unwrap();
    let mock_server_url = mockito::server_url();
    (env, db, mock_server_url)
}

// Helper to add a key for testing purposes
async fn add_test_key(
    db: &D1Database,
    key_name: &str,
    consecutive_failures: i64,
    latency_ms: i64,
    success_rate: f64,
    status: &str,
) {
    d1_storage::add_keys(db, "test-provider", key_name)
        .await
        .unwrap();
    let keys = d1_storage::get_active_keys(db, "test-provider")
        .await
        .unwrap();
    let key = keys.iter().find(|k| k.key == key_name).unwrap();

    // Manually update the health metrics for the test
    let query = crate::dbmodels::Key::filter_by_id(key.id.clone())
        .update()
        .consecutive_failures(consecutive_failures)
        .latency_ms(latency_ms)
        .success_rate(success_rate);
    
    let executor = d1_storage::get_executor(db);
    executor.exec_update(query.stmt).await.unwrap();

    if status == "blocked" {
        d1_storage::update_status(db, &key.id, ApiKeyStatus::Blocked)
            .await
            .unwrap();
    }
}

#[tokio::test]
#[ignore] // Ignoring because it requires a live D1 instance.
async fn test_health_based_routing_and_circuit_breaker() {
    let (_env, db, _server_url) = setup_test_env().await;

    // 1. Arrange: Create a set of test keys with varying health metrics.
    add_test_key(&db, "key-1-healthy", 0, 100, 1.0, "active").await;
    add_test_key(&db, "key-2-unhealthy", 6, 500, 0.2, "active").await; // Should be filtered by circuit breaker
    add_test_key(&db, "key-3-slower", 1, 500, 0.9, "active").await;
    add_test_key(&db, "key-4-blocked", 0, 100, 1.0, "blocked").await; // Should be filtered by status

    // 2. Act: Call the function to get healthy, sorted keys.
    let sorted_keys = d1_storage::get_healthy_sorted_keys_via_cache(&db, "test-provider")
        .await
        .unwrap();

    // 3. Assert:
    // - The unhealthy key (Key 2) should be filtered out (circuit breaker).
    // - The blocked key (Key 4) should be filtered out.
    // - The remaining keys should be sorted by health, with Key 1 appearing before Key 3.
    assert_eq!(sorted_keys.len(), 2);
    assert_eq!(sorted_keys[0].key, "key-1-healthy");
    assert_eq!(sorted_keys[1].key, "key-3-slower");
}

// More tests to be added for:
// - Retry logic for transient errors.
// - Failover logic when a key fails.
// - Metric updates after requests.
// - End-to-end request flow.
