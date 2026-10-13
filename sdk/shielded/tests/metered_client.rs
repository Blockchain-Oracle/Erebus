//! Buyer metered client: durable cursor, crash recovery, and seller reconciliation.
//!
//! Every test drives the real client against the real loopback seller route; only durable
//! write faults and the payment certifier are injected.

use std::{
    io,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use erebus_journal::{Boundary, FaultHook, Step};
use erebus_shielded_prover::access::metered::{MeteredClient, MeteredClientError};
use serde_json::Value;

#[path = "support/metered.rs"]
mod metered_support;

use metered_support::{fixture, now, start, Certifier, Feed, Seller, Server, BUYER, SERVICE_ID};

fn endpoint(server: &Server) -> String {
    format!("{}/v1/metered", server.url)
}

fn open(server: &Server, cache: &Path) -> MeteredClient {
    MeteredClient::open(&endpoint(server), SERVICE_ID, cache, true).unwrap()
}

fn open_with_faults(server: &Server, cache: &Path, faults: Arc<dyn FaultHook>) -> MeteredClient {
    MeteredClient::with_faults(&endpoint(server), SERVICE_ID, cache, true, faults).unwrap()
}

/// Fails the `target`-th durable `step` of one store, counting every boundary that reaches it.
/// The cursor store is `<cache>/metered`; the call store is its `calls` subdirectory.
struct FailNth {
    step: Step,
    in_calls: bool,
    target: usize,
    seen: AtomicUsize,
}

impl FailNth {
    fn cursor(step: Step, target: usize) -> Self {
        Self {
            step,
            in_calls: false,
            target,
            seen: AtomicUsize::new(0),
        }
    }

    fn calls(step: Step, target: usize) -> Self {
        Self {
            step,
            in_calls: true,
            target,
            seen: AtomicUsize::new(0),
        }
    }
}

fn in_calls_store(path: &Path) -> bool {
    path.components().any(|part| part.as_os_str() == "calls")
}

impl FaultHook for FailNth {
    fn after(&self, boundary: Boundary<'_>) -> io::Result<()> {
        if boundary.step != self.step || in_calls_store(boundary.path) != self.in_calls {
            return Ok(());
        }
        let seen = self.seen.fetch_add(1, Ordering::SeqCst) + 1;
        if seen == self.target {
            return Err(io::Error::other("injected durable boundary failure"));
        }
        Ok(())
    }
}

/// Removes one cached call: its record and its stored response.
fn drop_cached_call(cache: &Path, index: u64) {
    let calls = cache.join("metered").join("calls");
    for entry in std::fs::read_dir(&calls).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if record["index"] == index {
                std::fs::remove_file(&path).unwrap();
                std::fs::remove_file(path.with_extension("0.tx")).unwrap();
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sequential_calls_advance_one_cursor_and_replay_old_indices() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let client = open(&server, cache.path());

    let mut hashes = Vec::new();
    for index in 0..3u64 {
        let receipt = client.call(&evidence, &BUYER, None, now()).await.unwrap();
        assert_eq!(receipt.call_index, index);
        assert_eq!(receipt.quantity, 3);
        assert_eq!(receipt.next_call_index, index + 1);
        assert_eq!(receipt.remaining, 2 - index);
        assert!(!receipt.repeated && !receipt.cached);
        assert!(receipt.response_hash_consistent);
        assert!(receipt.seller_reported_payment_finalized);
        assert!(
            !receipt.resource_verified && !receipt.payment_verified && !receipt.delivery_verified
        );
        assert_eq!(
            std::fs::read(&receipt.response_file).unwrap(),
            format!("feed call {index} fetch {index}").as_bytes()
        );
        hashes.push(receipt.response_sha256);
    }

    // Every unit is consumed; a new index is refused locally without HTTP.
    assert_eq!(
        client
            .call(&evidence, &BUYER, None, now())
            .await
            .unwrap_err(),
        MeteredClientError::AllowanceExhausted
    );
    assert_eq!(feed.fetches(), 3);

    // An earlier index is served from the local cache, never from the seller.
    let replay = client
        .call(&evidence, &BUYER, Some(1), now())
        .await
        .unwrap();
    assert_eq!(replay.call_index, 1);
    assert!(replay.cached && replay.repeated);
    assert_eq!(replay.response_sha256, hashes[1]);
    assert_eq!(
        std::fs::read(&replay.response_file).unwrap(),
        b"feed call 1 fetch 1"
    );
    assert_eq!(seller.records(), (1, 3));
    assert_eq!(certifier.calls(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn status_reconciles_without_consuming() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let client = open(&server, cache.path());

    let receipt = client.call(&evidence, &BUYER, None, now()).await.unwrap();
    assert_eq!(receipt.call_index, 0);

    let status = client.status(&evidence, &BUYER, now()).await.unwrap();
    assert_eq!(status.quantity, 3);
    assert_eq!(status.seller_next_call_index, 1);
    assert_eq!(status.next_call_index, 1);
    assert_eq!(status.remaining, 2);
    assert_eq!(status.in_flight, None);
    assert!(!status.exhausted);
    assert_eq!(status.recovered_calls, 0);
    assert!(status.seller_reported_payment_finalized);
    assert!(!status.payment_verified && !status.delivery_verified);
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cursor_write_failure_before_http_keeps_the_index_in_flight() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();

    // The cursor is saved once to learn the seller's position, then again with index 0 in
    // flight. Failing that second write's directory sync leaves the index durable.
    let crashing = open_with_faults(
        &server,
        cache.path(),
        Arc::new(FailNth::cursor(Step::DirectorySynced, 2)),
    );
    assert_eq!(
        crashing
            .call(&evidence, &BUYER, None, now())
            .await
            .unwrap_err(),
        MeteredClientError::Storage
    );
    assert_eq!(seller.records(), (1, 0));
    assert_eq!(feed.fetches(), 0);
    drop(crashing);

    // The restarted client finds index 0 in flight and retries it, not index 1.
    let client = open(&server, cache.path());
    let receipt = client.call(&evidence, &BUYER, None, now()).await.unwrap();
    assert_eq!(receipt.call_index, 0);
    assert!(!receipt.repeated);
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_store_failure_after_http_retries_the_same_index_as_a_repeat() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();

    // The seller commits index 0, then the buyer's call store fails while making it durable.
    let crashing = open_with_faults(
        &server,
        cache.path(),
        Arc::new(FailNth::calls(Step::DirectorySynced, 1)),
    );
    assert_eq!(
        crashing
            .call(&evidence, &BUYER, None, now())
            .await
            .unwrap_err(),
        MeteredClientError::Storage
    );
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
    drop(crashing);

    // The retry re-signs index 0; the seller replays its stored bytes, consuming nothing.
    let client = open(&server, cache.path());
    let receipt = client.call(&evidence, &BUYER, None, now()).await.unwrap();
    assert_eq!(receipt.call_index, 0);
    assert!(receipt.repeated && !receipt.cached);
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cursor_advance_failure_after_storing_serves_from_cache_on_restart() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();

    // The call is stored, then the cursor advance fails before it is durable.
    let crashing = open_with_faults(
        &server,
        cache.path(),
        Arc::new(FailNth::cursor(Step::FileSynced, 3)),
    );
    assert_eq!(
        crashing
            .call(&evidence, &BUYER, None, now())
            .await
            .unwrap_err(),
        MeteredClientError::Storage
    );
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
    drop(crashing);

    // The retry finds the stored call and finishes without another request.
    let client = open(&server, cache.path());
    let receipt = client.call(&evidence, &BUYER, None, now()).await.unwrap();
    assert_eq!(receipt.call_index, 0);
    assert!(receipt.cached && receipt.repeated);
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cached_receipt_requires_call_directory_durability() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();

    // The seller commits index 0, then the buyer's call record rename reports an error at its
    // directory sync: the record is visible, and whether the physical sync happened is unknown.
    let crashing = open_with_faults(
        &server,
        cache.path(),
        Arc::new(FailNth::calls(Step::DirectorySynced, 2)),
    );
    assert_eq!(
        crashing
            .call(&evidence, &BUYER, None, now())
            .await
            .unwrap_err(),
        MeteredClientError::Storage
    );
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
    drop(crashing);

    // A recovery read must not serve the visible record until its directory sync succeeds.
    let undurable = open_with_faults(
        &server,
        cache.path(),
        Arc::new(FailNth::calls(Step::DirectorySynced, 1)),
    );
    assert_eq!(
        undurable
            .call(&evidence, &BUYER, None, now())
            .await
            .unwrap_err(),
        MeteredClientError::Storage
    );
    // The cursor never advanced past index 0, so the offline read is not complete either.
    assert_eq!(undurable.cached(&evidence, 0).unwrap(), None);
    drop(undurable);

    // Once the sync succeeds, the stored call finishes without another request.
    let client = open(&server, cache.path());
    let receipt = client.call(&evidence, &BUYER, None, now()).await.unwrap();
    assert_eq!(receipt.call_index, 0);
    assert!(receipt.cached && receipt.repeated);
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restarted_buyer_rebuilds_lost_replies_without_consuming() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let client = open(&server, cache.path());
    for _ in 0..2 {
        client.call(&evidence, &BUYER, None, now()).await.unwrap();
    }
    assert_eq!(seller.records(), (1, 2));

    // A buyer whose cache was wiped learns the seller is ahead and rebuilds by repeated reads.
    let fresh_cache = tempfile::tempdir().unwrap();
    let rebuilt = open(&server, fresh_cache.path());
    let status = rebuilt.status(&evidence, &BUYER, now()).await.unwrap();
    assert_eq!(status.seller_next_call_index, 2);
    assert_eq!(status.next_call_index, 2);
    assert_eq!(status.remaining, 1);
    assert_eq!(status.recovered_calls, 2);
    assert_eq!(seller.records(), (1, 2));
    assert_eq!(feed.fetches(), 2);

    // The rebuilt cache serves both indices without another request.
    let replay = rebuilt
        .call(&evidence, &BUYER, Some(0), now())
        .await
        .unwrap();
    assert!(replay.cached && replay.repeated);
    assert_eq!(seller.records(), (1, 2));
    assert_eq!(feed.fetches(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_lost_cache_entry_is_refetched_as_a_repeat() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let client = open(&server, cache.path());
    for _ in 0..3 {
        client.call(&evidence, &BUYER, None, now()).await.unwrap();
    }

    drop_cached_call(cache.path(), 1);
    let replay = client
        .call(&evidence, &BUYER, Some(1), now())
        .await
        .unwrap();
    assert_eq!(replay.call_index, 1);
    assert!(replay.repeated && !replay.cached);
    assert_eq!(
        std::fs::read(&replay.response_file).unwrap(),
        b"feed call 1 fetch 1"
    );
    assert_eq!(seller.records(), (1, 3));
    assert_eq!(feed.fetches(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_seller_that_lost_state_fails_closed() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let client = open(&server, cache.path());
    for _ in 0..2 {
        client.call(&evidence, &BUYER, None, now()).await.unwrap();
    }
    assert_eq!(seller.records(), (1, 2));

    // The same agreement against a seller that lost its ledger: the buyer must not reconsumed.
    let fresh_seller = Seller::new(&evidence);
    let fresh_certifier = Certifier::new();
    let fresh_server = start(&fresh_seller, &policy, &fresh_certifier, Feed::new()).await;
    let rebuilt = open(&fresh_server, cache.path());
    assert_eq!(
        rebuilt.status(&evidence, &BUYER, now()).await.unwrap_err(),
        MeteredClientError::SellerLedgerBehind
    );
    assert_eq!(seller.records(), (1, 2));
    // The fresh seller certified and activated an entitlement, but consumed no call.
    assert_eq!(fresh_seller.records(), (1, 0));
}

#[tokio::test(flavor = "multi_thread")]
async fn pending_finality_keeps_the_index_for_the_retry() {
    let (policy, evidence) = fixture(2, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let client = open(&server, cache.path());

    certifier.answer(Err(erebus_shielded_prover::access::AccessError::Pending));
    assert_eq!(
        client
            .call(&evidence, &BUYER, None, now())
            .await
            .unwrap_err(),
        MeteredClientError::Pending
    );
    assert_eq!(seller.records(), (0, 0));
    assert_eq!(feed.fetches(), 0);

    certifier.answer(Ok(()));
    let receipt = client.call(&evidence, &BUYER, None, now()).await.unwrap();
    assert_eq!(receipt.call_index, 0);
    assert!(!receipt.repeated);
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
    assert_eq!(certifier.calls(), 2);
}

/// Copies a ledger directory with its owner-only modes, as a backup restore would.
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir(to).unwrap();
    std::fs::set_permissions(to, std::fs::metadata(from).unwrap().permissions()).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Removes the buyer cursor, keeping the call cache.
fn drop_cursor(cache: &Path) {
    for entry in std::fs::read_dir(cache.join("metered")).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            std::fs::remove_file(&path).unwrap();
        }
    }
}

fn cursor_count(cache: &Path) -> usize {
    std::fs::read_dir(cache.join("metered"))
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .count()
}

/// The stored response of one cached call.
fn cached_body(cache: &Path, index: u64) -> std::path::PathBuf {
    let calls = cache.join("metered").join("calls");
    for entry in std::fs::read_dir(&calls).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let record: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if record["index"] == index {
                return path.with_extension("0.tx");
            }
        }
    }
    panic!("no cached call {index}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cached_call_above_a_restored_seller_fails_closed_without_a_cursor() {
    let (policy, evidence) = fixture(4, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let server = start(&seller, &policy, &certifier, Feed::new()).await;
    let cache = tempfile::tempdir().unwrap();
    let client = open(&server, cache.path());
    client.call(&evidence, &BUYER, None, now()).await.unwrap();
    // Seller backup taken at next = 1.
    let backup = tempfile::tempdir().unwrap();
    copy_dir(&seller.state, &backup.path().join("metered"));
    for _ in 1..=2 {
        client.call(&evidence, &BUYER, None, now()).await.unwrap();
    }
    assert_eq!(seller.records(), (1, 3));
    drop(server);

    // The buyer loses its cursor and C(1) but keeps C(0) and C(2); the seller is restored to
    // next = 1. C(2) proves the seller committed index 2, so index 1 must not be consumed again.
    drop_cursor(cache.path());
    drop_cached_call(cache.path(), 1);
    let restored = Seller::new(&evidence);
    copy_dir(&backup.path().join("metered"), &restored.state);
    assert_eq!(restored.records(), (1, 1));
    let feed = Feed::new();
    let restored_server = start(&restored, &policy, &Certifier::new(), feed.clone()).await;
    let rebuilt = open(&restored_server, cache.path());
    assert_eq!(
        rebuilt.status(&evidence, &BUYER, now()).await.unwrap_err(),
        MeteredClientError::SellerLedgerBehind
    );
    assert_eq!(cursor_count(cache.path()), 0);
    // A call without a cursor reconciles first and fails closed the same way.
    assert_eq!(
        rebuilt
            .call(&evidence, &BUYER, None, now())
            .await
            .unwrap_err(),
        MeteredClientError::SellerLedgerBehind
    );
    assert_eq!(cursor_count(cache.path()), 0);
    assert_eq!(restored.records(), (1, 1));
    assert_eq!(feed.fetches(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cached_record_without_its_body_is_refetched_during_recovery() {
    for truncate in [false, true] {
        let (policy, evidence) = fixture(2, 64);
        let seller = Seller::new(&evidence);
        let certifier = Certifier::new();
        let feed = Feed::new();
        let server = start(&seller, &policy, &certifier, feed.clone()).await;
        let cache = tempfile::tempdir().unwrap();
        let client = open(&server, cache.path());
        let first = client.call(&evidence, &BUYER, None, now()).await.unwrap();
        let bytes = std::fs::read(&first.response_file).unwrap();
        assert_eq!(bytes, b"feed call 0 fetch 0");

        // C(0)'s record survives; its body is lost or cut short, and so is the cursor.
        let body = cached_body(cache.path(), 0);
        if truncate {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&body)
                .unwrap()
                .set_len(4)
                .unwrap();
        } else {
            std::fs::remove_file(&body).unwrap();
        }
        drop_cursor(cache.path());

        let rebuilt = open(&server, cache.path());
        let status = rebuilt.status(&evidence, &BUYER, now()).await.unwrap();
        assert_eq!(status.seller_next_call_index, 1);
        assert_eq!(status.next_call_index, 1);
        assert_eq!(status.recovered_calls, 1);
        assert_eq!(std::fs::read(&body).unwrap(), bytes);
        assert_eq!(seller.records(), (1, 1));
        assert_eq!(feed.fetches(), 1);

        let replay = rebuilt
            .call(&evidence, &BUYER, Some(0), now())
            .await
            .unwrap();
        assert!(replay.cached && replay.repeated);
        assert_eq!(std::fs::read(&replay.response_file).unwrap(), bytes);
        assert_eq!(seller.records(), (1, 1));
        assert_eq!(feed.fetches(), 1);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_replay_of_a_damaged_body_refetches_it_as_a_repeat() {
    let (policy, evidence) = fixture(2, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();
    let client = open(&server, cache.path());
    let first = client.call(&evidence, &BUYER, None, now()).await.unwrap();
    let bytes = std::fs::read(&first.response_file).unwrap();
    std::fs::remove_file(cached_body(cache.path(), 0)).unwrap();

    let replay = client
        .call(&evidence, &BUYER, Some(0), now())
        .await
        .unwrap();
    assert!(replay.repeated && !replay.cached);
    assert_eq!(std::fs::read(&replay.response_file).unwrap(), bytes);
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_damaged_body_at_the_cursor_is_refetched_without_consuming() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let cache = tempfile::tempdir().unwrap();

    // C(0) is stored, then the cursor advance fails: the cursor stays at 0.
    let crashing = open_with_faults(
        &server,
        cache.path(),
        Arc::new(FailNth::cursor(Step::FileSynced, 3)),
    );
    assert_eq!(
        crashing
            .call(&evidence, &BUYER, None, now())
            .await
            .unwrap_err(),
        MeteredClientError::Storage
    );
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);
    drop(crashing);

    // C(0)'s record survives without its body.
    let body = cached_body(cache.path(), 0);
    std::fs::remove_file(&body).unwrap();

    // A plain call repairs index 0 as a repeated read; it consumes nothing new.
    let client = open(&server, cache.path());
    let repaired = client.call(&evidence, &BUYER, None, now()).await.unwrap();
    assert_eq!(repaired.call_index, 0);
    assert!(repaired.repeated && !repaired.cached);
    assert_eq!(
        std::fs::read(&repaired.response_file).unwrap(),
        b"feed call 0 fetch 0"
    );
    assert_eq!(std::fs::read(&body).unwrap(), b"feed call 0 fetch 0");
    assert_eq!(seller.records(), (1, 1));
    assert_eq!(feed.fetches(), 1);

    // The next plain call consumes index 1.
    let next = client.call(&evidence, &BUYER, None, now()).await.unwrap();
    assert_eq!(next.call_index, 1);
    assert!(!next.repeated && !next.cached);
    assert_eq!(seller.records(), (1, 2));
    assert_eq!(feed.fetches(), 2);
}
