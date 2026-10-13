//! Authenticated, bounded HTTP transport for operator-sponsored shielded transfers.
//! Gas reservations are conservative lifetime caps; failed or ambiguous calls never refund them.

use std::{
    fmt,
    path::Path,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use erebus_core::suite::keccak256;
use erebus_journal::{JournalRecord, RecordId, Store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

use crate::{
    relay_request::{RelayRequest, MAX_RELAY_REQUEST_BYTES},
    relay_service::SponsoredRelay,
};

/// Fixed operator limits. One service instance has one authentication principal.
#[derive(Clone, Copy)]
pub struct RelayHttpPolicy {
    /// Maximum accepted HTTP calls in each persisted fixed window, including observations.
    pub requests_per_window: u32,
    /// Duration of a rate-limit window in seconds.
    pub window_seconds: u64,
    /// Lifetime gas-cap budget, reserved once per distinct operation.
    pub maximum_gas_budget: u128,
    /// Maximum distinct retained operations, bounded to 10,000.
    pub maximum_operations: u32,
    /// Deadline for a backend invocation. Timeout never establishes nonpayment.
    pub timeout_seconds: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct QuotaId(String);
impl fmt::Display for QuotaId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl RecordId for QuotaId {
    fn as_file_stem(&self) -> &str {
        &self.0
    }
    fn from_file_stem(stem: &str) -> Option<Self> {
        (stem == "quota").then(|| Self(stem.into()))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QuotaRecord {
    version: u32,
    id: QuotaId,
    policy: [u8; 32],
    window_start: u64,
    requests: u32,
    reserved: u128,
    operations: Vec<([u8; 32], [u8; 32])>,
}
impl JournalRecord for QuotaRecord {
    type Id = QuotaId;
    const CURRENT_VERSION: u32 = 1;
    const OLDEST_READABLE_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn record_id(&self) -> &QuotaId {
        &self.id
    }
    fn attempt_count(&self) -> usize {
        1
    }
}

struct Quota {
    store: Store<QuotaRecord>,
    policy: RelayHttpPolicy,
    pin: [u8; 32],
    cost: u128,
}
impl Quota {
    fn open(root: &Path, policy: RelayHttpPolicy, cost: u128) -> Result<Self, &'static str> {
        if !root.is_absolute()
            || policy.requests_per_window == 0
            || policy.requests_per_window > 100_000
            || !(1..=86_400).contains(&policy.window_seconds)
            || !(1..=10_000).contains(&policy.maximum_operations)
            || !(1..=900).contains(&policy.timeout_seconds)
            || cost == 0
            || policy.maximum_gas_budget < cost
        {
            return Err("invalid shielded relay transport policy");
        }
        let pin = keccak256(&[
            b"erebus-relay-quota-v1",
            &policy.requests_per_window.to_be_bytes(),
            &policy.window_seconds.to_be_bytes(),
            &policy.maximum_gas_budget.to_be_bytes(),
            &policy.maximum_operations.to_be_bytes(),
            &policy.timeout_seconds.to_be_bytes(),
            &cost.to_be_bytes(),
        ]);
        Ok(Self {
            store: Store::open(root).map_err(|_| "relay quota unavailable")?,
            policy,
            pin,
            cost,
        })
    }

    fn accept(&self, now: u64, request: Option<([u8; 32], [u8; 32])>) -> Result<(), &'static str> {
        self.update(now, request, true)
    }

    fn reserve(&self, now: u64, request: ([u8; 32], [u8; 32])) -> Result<(), &'static str> {
        self.update(now, Some(request), false)
    }

    fn update(
        &self,
        now: u64,
        request: Option<([u8; 32], [u8; 32])>,
        count: bool,
    ) -> Result<(), &'static str> {
        let _lock = self
            .store
            .lock_identity()
            .map_err(|_| "relay quota unavailable")?;
        let id = QuotaId("quota".into());
        let saved = self
            .store
            .read(&id)
            .map_err(|_| "relay quota unavailable")?;
        let existing = saved.is_some();
        if existing {
            if self
                .store
                .read_blob(&id, 0)
                .map_err(|_| "relay quota unavailable")?
                != self.pin
            {
                return Err("relay quota unavailable");
            }
        } else if self
            .store
            .blob_path(&id, 0)
            .try_exists()
            .map_err(|_| "relay quota unavailable")?
        {
            return Err("relay quota unavailable");
        }
        let mut record = saved.unwrap_or(QuotaRecord {
            version: 1,
            id,
            policy: self.pin,
            window_start: now,
            requests: 0,
            reserved: 0,
            operations: vec![],
        });
        let identities: std::collections::BTreeSet<_> =
            record.operations.iter().map(|(id, _)| *id).collect();
        if record.policy != self.pin
            || now < record.window_start
            || record.reserved > self.policy.maximum_gas_budget
            || record.operations.len() > self.policy.maximum_operations as usize
            || identities.len() != record.operations.len()
            || record
                .operations
                .iter()
                .any(|(id, digest)| *id == [0; 32] || *digest == [0; 32])
            || self.cost.checked_mul(record.operations.len() as u128) != Some(record.reserved)
        {
            return Err("relay quota unavailable");
        }
        if now - record.window_start >= self.policy.window_seconds {
            record.window_start = now;
            record.requests = 0;
        }
        if count && record.requests >= self.policy.requests_per_window {
            return Err("relay rate limit reached");
        }
        if let Some((identity, digest)) = request {
            if let Some((_, retained)) = record.operations.iter().find(|(id, _)| *id == identity) {
                if *retained != digest {
                    return Err("relay request conflicts with retained intent");
                }
            } else {
                let reserved = record
                    .reserved
                    .checked_add(self.cost)
                    .ok_or("relay budget exhausted")?;
                if reserved > self.policy.maximum_gas_budget
                    || record.operations.len() >= self.policy.maximum_operations as usize
                {
                    return Err("relay budget exhausted");
                }
                record.operations.push((identity, digest));
                record.reserved = reserved;
            }
        }
        if count {
            record.requests += 1;
        }
        if existing {
            self.store
                .write(&record)
                .map_err(|_| "relay quota unavailable")
        } else {
            self.store
                .write_blob_then_record::<erebus_journal::StoreError<QuotaId>>(
                    &mut record,
                    0,
                    &self.pin,
                    |_| Ok(()),
                )
                .map_err(|_| "relay quota unavailable")
        }
    }
}

/// One authenticated transport. Expose through an authenticated TLS gateway for remote use.
pub struct RelayHttpService {
    engine: SponsoredRelay,
    token: Zeroizing<[u8; 32]>,
    quota: Quota,
    active: Arc<Semaphore>,
}
impl RelayHttpService {
    /// Opens persistent quotas. The authentication token is never written or returned.
    pub fn new(
        engine: SponsoredRelay,
        token: [u8; 32],
        root: &Path,
        policy: RelayHttpPolicy,
    ) -> Result<Self, &'static str> {
        if token == [0; 32] {
            return Err("invalid shielded relay authentication");
        }
        let quota = Quota::open(
            root,
            policy,
            engine.gas_budget().map_err(|_| "invalid gas policy")?,
        )?;
        Ok(Self {
            engine,
            token: Zeroizing::new(token),
            quota,
            active: Arc::new(Semaphore::new(1)),
        })
    }

    /// Bounded proof-only endpoints; no keys, witnesses or agreement openings are accepted.
    pub fn router(self) -> Router {
        Router::new()
            .route(
                "/healthz",
                get(|| async { Json(json!({"status":"ok", "service":"erebus-shielded-relayer"})) }),
            )
            .route("/v1/relay", post(submit))
            .route("/v1/observe", post(observe))
            .layer(DefaultBodyLimit::max(MAX_RELAY_REQUEST_BYTES))
            .with_state(Arc::new(self))
    }
}

fn authenticated(headers: &HeaderMap, token: &[u8; 32]) -> bool {
    if headers.get_all("authorization").iter().count() != 1 {
        return false;
    }
    let Some(value) = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
    else {
        return false;
    };
    if value.len() != 64 {
        return false;
    }
    let mut supplied = Zeroizing::new([0u8; 32]);
    if hex::decode_to_slice(value, &mut supplied[..]).is_err() {
        return false;
    }
    supplied
        .iter()
        .zip(token)
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

fn failure(code: StatusCode, message: &'static str) -> (StatusCode, Json<Value>) {
    (
        code,
        Json(
            json!({"protocol_version":1,"status":"pending", "error":message,
        "retry_without_new_payment":true,"automatic_rebroadcast":false}),
        ),
    )
}
async fn submit(
    State(service): State<Arc<RelayHttpService>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    handle(service, headers, body, false).await
}
async fn observe(
    State(service): State<Arc<RelayHttpService>>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    handle(service, headers, body, true).await
}

async fn handle(
    service: Arc<RelayHttpService>,
    headers: HeaderMap,
    body: Bytes,
    observing: bool,
) -> (StatusCode, Json<Value>) {
    if !authenticated(&headers, &service.token) {
        return failure(StatusCode::UNAUTHORIZED, "relay authentication required");
    }
    let Ok(_permit) = service.active.clone().try_acquire_owned() else {
        return failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "relay busy; retain the operation",
        );
    };
    let Ok(now) = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
    else {
        return failure(StatusCode::SERVICE_UNAVAILABLE, "relay clock unavailable");
    };
    // Count every authenticated request before expensive proof verification.
    if let Err(error) = service.quota.accept(now, None) {
        return failure(StatusCode::TOO_MANY_REQUESTS, error);
    }
    let Ok(identity) = service.engine.request_identity(&body) else {
        return failure(StatusCode::BAD_REQUEST, "invalid shielded relay request");
    };
    if !observing {
        let Ok(canonical) = RelayRequest::decode(&body).and_then(|request| request.encode()) else {
            return failure(StatusCode::BAD_REQUEST, "invalid shielded relay request");
        };
        if let Err(error) = service
            .quota
            .reserve(now, (identity, keccak256(&[&canonical])))
        {
            return failure(StatusCode::TOO_MANY_REQUESTS, error);
        }
    }
    let action = async {
        if observing {
            service.engine.observe(&body).await
        } else {
            service.engine.relay(&body).await
        }
    };
    match tokio::time::timeout(
        Duration::from_secs(service.quota.policy.timeout_seconds),
        action,
    )
    .await
    {
        Ok(Ok(result)) => (
            StatusCode::OK,
            Json(json!({"protocol_version":1,
            "status": match result.finalized { Some(done) if done.success => "finalized", Some(_) => "reverted", None => "pending" },
            "operation_ref":hex::encode(result.operation_ref),
            "transaction_hash":result.transaction_hash.map(|hash| format!("0x{}",hex::encode(hash))),
            "broadcast_started":result.broadcast_started,"finalized":result.finalized,
            "retry_without_new_payment":true,"automatic_rebroadcast":false})),
        ),
        Ok(Err(_)) => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "relay unavailable; retain the operation",
        ),
        Err(_) => failure(
            StatusCode::GATEWAY_TIMEOUT,
            "relay deadline reached; observe the retained operation",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> RelayHttpPolicy {
        RelayHttpPolicy {
            requests_per_window: 10,
            window_seconds: 60,
            maximum_gas_budget: 20,
            maximum_operations: 2,
            timeout_seconds: 30,
        }
    }
    #[test]
    fn quota_survives_restart_and_duplicates_never_reserve_twice() {
        let root = tempfile::tempdir().unwrap();
        let quota = Quota::open(root.path(), policy(), 10).unwrap();
        quota.accept(100, Some(([1; 32], [2; 32]))).unwrap();
        drop(quota);
        let quota = Quota::open(root.path(), policy(), 10).unwrap();
        quota.accept(101, Some(([1; 32], [2; 32]))).unwrap();
        assert!(quota.accept(101, Some(([1; 32], [3; 32]))).is_err());
        quota.accept(102, Some(([2; 32], [3; 32]))).unwrap();
        assert!(quota.accept(103, Some(([3; 32], [4; 32]))).is_err());
        assert!(quota.accept(200, Some(([3; 32], [4; 32]))).is_err());
    }
    #[test]
    fn rate_clock_policy_and_corruption_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let quota = Quota::open(root.path(), policy(), 10).unwrap();
        for _ in 0..10 {
            quota.accept(100, None).unwrap();
        }
        assert!(quota.accept(101, None).is_err());
        assert!(quota.accept(99, None).is_err());
        quota.accept(160, None).unwrap();
        let mut changed = policy();
        changed.maximum_gas_budget = 30;
        assert!(Quota::open(root.path(), changed, 10)
            .unwrap()
            .accept(161, None)
            .is_err());
        std::fs::write(root.path().join("quota.json"), b"{").unwrap();
        assert!(quota.accept(161, None).is_err());
    }
    #[test]
    fn bearer_authentication_never_accepts_another_token() {
        let mut headers = HeaderMap::new();
        assert!(!authenticated(&headers, &[1; 32]));
        headers.insert(
            "authorization",
            format!("Bearer {}", hex::encode([1; 32])).parse().unwrap(),
        );
        assert!(authenticated(&headers, &[1; 32]));
        assert!(!authenticated(&headers, &[2; 32]));
        headers.insert("authorization", "Bearer invalid".parse().unwrap());
        assert!(!authenticated(&headers, &[1; 32]));
    }
    #[test]
    fn deleted_quota_is_not_a_fresh_budget_and_reserve_does_not_count_twice() {
        let root = tempfile::tempdir().unwrap();
        let mut limits = policy();
        limits.requests_per_window = 1;
        let quota = Quota::open(root.path(), limits, 10).unwrap();
        quota.accept(100, None).unwrap();
        quota.reserve(100, ([1; 32], [2; 32])).unwrap();
        assert!(quota.accept(100, None).is_err());
        std::fs::remove_file(root.path().join("quota.json")).unwrap();
        assert!(quota.accept(160, None).is_err());
    }
}
