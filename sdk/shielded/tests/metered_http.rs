//! Prepaid metered access over loopback HTTP: the library route with a counting certifier, and
//! the service binary's profile configuration.

use erebus_core::shielded_auth;
use erebus_shielded_prover::access::{
    metered::{http::FeedFile, MeteredOperation},
    AccessError,
};
use erebus_transport::disclosure::SelectedAgreement;
use serde_json::{json, Value};
use std::{
    net::TcpListener,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

#[path = "support/metered.rs"]
mod metered_support;

use metered_support::{
    body, call, fixture, post, private_file, signed, start, status, write_feed, Certifier, Feed,
    Seller, RESOURCE, SELLER, SERVICE_ID,
};

#[tokio::test(flavor = "multi_thread")]
async fn sequential_calls_replay_identically_and_status_lists_them() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed_path = seller.state.with_file_name("feed.bin");
    let server = start(
        &seller,
        &policy,
        &certifier,
        FeedFile::new(&feed_path, &policy.profile),
    )
    .await;

    let mut bodies = Vec::new();
    for index in 0..3u64 {
        write_feed(&feed_path, format!("tick {index}").as_bytes());
        let (code, reply) = call(&server, &evidence, index).await;
        assert_eq!(code, 200, "{reply}");
        assert_eq!(reply["status"], "issued");
        assert_eq!(reply["call_index"], index);
        assert_eq!(reply["quantity"], 3);
        assert_eq!(reply["remaining"], 2 - index);
        assert_eq!(reply["repeated"], false);
        assert_eq!(reply["late"], false);
        assert_eq!(reply["payment_verified"], true);
        assert_eq!(reply["delivery_verified"], false);
        assert_eq!(body(&reply), format!("tick {index}").as_bytes());
        bodies.push(reply);
    }

    // Retrying an index returns the stored bytes even though the feed has moved on.
    write_feed(&feed_path, b"tick moved");
    let (code, again) = call(&server, &evidence, 1).await;
    assert_eq!(code, 200, "{again}");
    assert_eq!(again["repeated"], true);
    assert_eq!(again["remaining"], 0);
    assert_eq!(body(&again), body(&bodies[1]));

    let (code, allowance) = status(&server, &evidence).await;
    assert_eq!(code, 200, "{allowance}");
    assert_eq!(allowance["quantity"], 3);
    assert_eq!(allowance["next_call_index"], 3);
    assert_eq!(allowance["remaining"], 0);
    assert_eq!(allowance["delivery_verified"], false);
    let calls = allowance["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 3);
    for (index, (listed, served)) in calls.iter().zip(&bodies).enumerate() {
        assert_eq!(listed["call_index"], index);
        assert_eq!(listed["response_sha256"], served["response_sha256"]);
        assert_eq!(listed["response_len"], body(served).len());
    }
    // Finality was observed once, on the first request.
    assert_eq!(certifier.calls(), 1);
    assert_eq!(seller.records(), (1, 3));
}

#[tokio::test(flavor = "multi_thread")]
async fn gaps_are_409_and_exhaustion_is_403_never_402() {
    let (policy, evidence) = fixture(2, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;

    let (code, gap) = call(&server, &evidence, 1).await;
    assert_eq!(
        (code, &gap["error"]),
        (409, &json!("out_of_order")),
        "{gap}"
    );
    assert_eq!(gap["delivery_verified"], false);
    assert_eq!(feed.fetches(), 0);

    let (code, first) = call(&server, &evidence, 0).await;
    assert_eq!(code, 200, "{first}");
    assert_eq!(call(&server, &evidence, 1).await.0, 200);
    for index in [2, 7] {
        let (code, exhausted) = call(&server, &evidence, index).await;
        assert_ne!(code, 402);
        assert_eq!(
            (code, &exhausted["error"]),
            (403, &json!("allowance_exhausted")),
            "{exhausted}"
        );
        assert!(exhausted.get("retry_without_payment").is_none());
    }
    // Exhaustion still serves old indices, from storage.
    let (code, replay) = call(&server, &evidence, 0).await;
    assert_eq!(code, 200, "{replay}");
    assert_eq!(replay["repeated"], true);
    assert_eq!(body(&replay), body(&first));
    assert_eq!(feed.fetches(), 2);
    assert_eq!(seller.records(), (1, 2));
}

#[tokio::test(flavor = "multi_thread")]
async fn pending_payment_is_202_and_writes_nothing() {
    let (policy, evidence) = fixture(2, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;

    certifier.answer(Err(AccessError::Pending));
    let (code, pending) = call(&server, &evidence, 0).await;
    assert_eq!(code, 202, "{pending}");
    assert_eq!(pending["error"], "payment_pending");
    assert_eq!(pending["status"], "payment_pending");
    assert_eq!(pending["retry_without_payment"], true);
    assert_eq!(status(&server, &evidence).await.0, 202);
    assert_eq!(seller.records(), (0, 0));
    assert_eq!(feed.fetches(), 0);

    certifier.answer(Err(AccessError::Observation));
    let (code, down) = call(&server, &evidence, 0).await;
    assert_eq!(
        (code, &down["error"]),
        (503, &json!("observation_unavailable"))
    );
    assert_eq!(seller.records(), (0, 0));

    certifier.answer(Ok(()));
    let (code, reply) = call(&server, &evidence, 0).await;
    assert_eq!(code, 200, "{reply}");
    assert_eq!(reply["repeated"], false);
    assert_eq!(seller.records(), (1, 1));
}

#[tokio::test(flavor = "multi_thread")]
async fn oversize_or_missing_feed_is_503_and_consumes_nothing() {
    let (policy, evidence) = fixture(2, 16);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed_path = seller.state.with_file_name("feed.bin");
    let server = start(
        &seller,
        &policy,
        &certifier,
        FeedFile::new(&feed_path, &policy.profile),
    )
    .await;

    let (code, missing) = call(&server, &evidence, 0).await;
    assert_eq!(
        (code, &missing["error"]),
        (503, &json!("resource_unavailable")),
        "{missing}"
    );
    write_feed(&feed_path, &[7; 17]);
    let (code, oversize) = call(&server, &evidence, 0).await;
    assert_eq!(
        (code, &oversize["error"]),
        (503, &json!("resource_unavailable")),
        "{oversize}"
    );
    assert_eq!(oversize["retry_without_payment"], true);
    let (_, allowance) = status(&server, &evidence).await;
    assert_eq!(allowance["next_call_index"], 0);
    assert_eq!(seller.records().1, 0);

    // The same index succeeds once the feed fits; it is a first issue, not a replay.
    write_feed(&feed_path, &[7; 16]);
    let (code, reply) = call(&server, &evidence, 0).await;
    assert_eq!(code, 200, "{reply}");
    assert_eq!(reply["repeated"], false);
    assert_eq!(body(&reply), [7; 16]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_snapshot_request_on_the_metered_route_is_rejected() {
    let (policy, evidence) = fixture(2, 64);
    let seller = Seller::new(&evidence);
    let certifier = Certifier::new();
    let feed = Feed::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;

    let metered = signed(&evidence, MeteredOperation::Call, 0);
    let snapshot = json!({
        "deal_commitment": metered.deal_commitment,
        "nonce": metered.nonce,
        "expires_at": metered.expires_at,
        "signature": metered.signature,
    });
    let (code, rejected) = post(&server.url, &snapshot).await;
    assert_eq!(
        (code, &rejected["error"]),
        (400, &json!("invalid_request")),
        "{rejected}"
    );
    // Unknown fields are rejected too, so a snapshot payment field cannot ride along.
    let mut extra = serde_json::to_value(&metered).unwrap();
    extra["payment"] = json!(null);
    assert_eq!(post(&server.url, &extra).await.0, 400);
    // A commitment that is not lowercase hex never names a file.
    let mut traversal = serde_json::to_value(&metered).unwrap();
    traversal["deal_commitment"] = json!("../".repeat(21) + "x");
    let (code, unauthenticated) = post(&server.url, &traversal).await;
    assert_eq!(
        (code, &unauthenticated["error"]),
        (401, &json!("unauthenticated"))
    );
    // A status signature does not authorize a call.
    let mut swapped = serde_json::to_value(signed(&evidence, MeteredOperation::Status, 0)).unwrap();
    swapped["operation"] = json!("call");
    assert_eq!(post(&server.url, &swapped).await.0, 401);

    assert_eq!(certifier.calls(), 0);
    assert_eq!(feed.fetches(), 0);
    assert_eq!(seller.records(), (0, 0));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_restarted_service_resumes_the_sequence_without_observing_again() {
    let (policy, evidence) = fixture(3, 64);
    let seller = Seller::new(&evidence);
    let feed = Feed::new();
    let certifier = Certifier::new();
    let server = start(&seller, &policy, &certifier, feed.clone()).await;
    let (code, first) = call(&server, &evidence, 0).await;
    assert_eq!(code, 200, "{first}");
    drop(server);

    let restarted_certifier = Certifier::new();
    let server = start(&seller, &policy, &restarted_certifier, feed.clone()).await;
    let (code, replay) = call(&server, &evidence, 0).await;
    assert_eq!(code, 200, "{replay}");
    assert_eq!(replay["repeated"], true);
    assert_eq!(body(&replay), body(&first));
    let (code, second) = call(&server, &evidence, 1).await;
    assert_eq!(code, 200, "{second}");
    assert_eq!(second["repeated"], false);
    assert_eq!(second["remaining"], 1);
    let (_, allowance) = status(&server, &evidence).await;
    assert_eq!(allowance["next_call_index"], 2);
    // The stored certificate carries finality across the restart.
    assert_eq!(restarted_certifier.calls(), 0);
    assert_eq!(feed.fetches(), 2);
}

// Service binary: profile configuration.

struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn shielded_backend(evidence: &SelectedAgreement) -> Value {
    // Closed loopback ports: startup never connects, and observation fails as unavailable.
    let pool = evidence.terms.domain.pool.as_ref().unwrap();
    json!({
        "mode": "shielded",
        "chain_id": evidence.terms.domain.namespace.reference().parse::<u64>().unwrap(),
        "pool": format!("0x{}", hex::encode(pool.as_bytes())),
        "rpc_url": format!("http://127.0.0.1:{}", port()),
        "peer_rpc_url": format!("http://127.0.0.1:{}", port()),
        "first_block": 1,
        "first_hash": vec![1u8; 32],
        "runtime_keccak256": "71".repeat(32),
        "poseidon_keccak256": "72".repeat(32),
        "deposit_verifier_keccak256": "73".repeat(32),
        "transfer_verifier_keccak256": "74".repeat(32),
        "withdraw_verifier_keccak256": "75".repeat(32),
    })
}

/// Writes an owner-only service configuration and returns its path.
fn configure(seller: &Seller, port: u16, profile: Value, backend: Value) -> std::path::PathBuf {
    let mut config = json!({
        "service_id": SERVICE_ID,
        "seller_key": shielded_auth::derive_key(&SELLER).unwrap().to_vec(),
        "suite_id": 2,
        "resource": RESOURCE,
        "evidence_root": seller.evidence,
        // The binary keeps its metered ledger in `<state_root>/metered`, which is `seller.state`.
        "state_root": seller.state.parent().unwrap(),
        "port": port,
        "backend": backend,
    });
    for (key, value) in profile.as_object().unwrap() {
        config[key] = value.clone();
    }
    let path = seller.state.with_file_name("service.json");
    let _ = std::fs::remove_file(&path);
    private_file(&path, &serde_json::to_vec(&config).unwrap());
    path
}

fn service(config: &Path) -> Process {
    Process(
        Command::new(env!("CARGO_BIN_EXE_erebus-access-service"))
            .env_clear()
            .env("EREBUS_ACCESS_CONFIG", config)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    )
}

async fn healthy(process: &mut Process, url: &str) -> Value {
    for _ in 0..100 {
        assert!(
            process.0.try_wait().unwrap().is_none(),
            "access service stopped"
        );
        if let Ok(response) = reqwest::get(format!("{url}/healthz")).await {
            return response.json().await.unwrap();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("access service did not become healthy");
}

/// The exit status and stderr of a service that must refuse to start.
fn refused(config: &Path) -> String {
    let mut process = service(config);
    for _ in 0..200 {
        if let Some(status) = process.0.try_wait().unwrap() {
            assert!(!status.success());
            let mut stderr = String::new();
            std::io::Read::read_to_string(&mut process.0.stderr.take().unwrap(), &mut stderr)
                .unwrap();
            return stderr;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("access service started with an invalid profile");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_snapshot_profile_is_the_default_and_has_no_metered_route() {
    let (_, evidence) = fixture(2, 64);
    let seller = Seller::new(&evidence);
    let payload = seller.state.with_file_name("payload.bin");
    std::fs::write(&payload, b"snapshot").unwrap();
    let port = port();
    let config = configure(
        &seller,
        port,
        json!({"payload_file": payload}),
        shielded_backend(&evidence),
    );
    let url = format!("http://127.0.0.1:{port}");
    let mut process = service(&config);
    assert_eq!(healthy(&mut process, &url).await["profile"], "snapshot");
    let client = reqwest::Client::new();
    let metered = client
        .post(format!("{url}/v1/metered"))
        .json(&signed(&evidence, MeteredOperation::Call, 0))
        .send()
        .await
        .unwrap();
    assert_eq!(metered.status().as_u16(), 404);
    // The snapshot route is still served; this metered agreement fails its snapshot check.
    let snapshot = client
        .post(format!("{url}/v1/access"))
        .json(&json!({"deal_commitment":"00".repeat(32),"nonce":vec![1u8; 32],"expires_at":1,"signature":[]}))
        .send()
        .await
        .unwrap();
    assert_ne!(snapshot.status().as_u16(), 404);
    assert!(!seller.state.exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_metered_profile_serves_only_the_metered_route() {
    let (_, evidence) = fixture(2, 64);
    let seller = Seller::new(&evidence);
    let port = port();
    let config = configure(
        &seller,
        port,
        json!({
            "profile": "metered",
            "max_response_bytes": 64,
            "feed_file": seller.state.with_file_name("feed.bin"),
        }),
        shielded_backend(&evidence),
    );
    let url = format!("http://127.0.0.1:{port}");
    let mut process = service(&config);
    let health = healthy(&mut process, &url).await;
    assert_eq!(
        (&health["profile"], &health["payment_submission"]),
        (&json!("metered"), &json!(false))
    );
    let client = reqwest::Client::new();
    let snapshot = client
        .post(format!("{url}/v1/access"))
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(snapshot.status().as_u16(), 404);
    // Observers are unreachable: the first request cannot certify finality and writes nothing.
    let response = client
        .post(format!("{url}/v1/metered"))
        .json(&signed(&evidence, MeteredOperation::Call, 0))
        .send()
        .await
        .unwrap();
    let code = response.status().as_u16();
    let reply: Value = response.json().await.unwrap();
    assert_eq!(
        (code, &reply["error"]),
        (503, &json!("observation_unavailable")),
        "{reply}"
    );
    assert_eq!(seller.records(), (0, 0));
}

#[test]
fn invalid_metered_configurations_are_refused_at_startup() {
    let (_, evidence) = fixture(2, 64);
    let seller = Seller::new(&evidence);
    let metered = json!({
        "profile": "metered",
        "max_response_bytes": 64,
        "feed_file": seller.state.with_file_name("feed.bin"),
    });
    // The x402 rail is refused before any chain connection is attempted.
    let x402 = json!({
        "mode": "x402_exact",
        "namespace": "eip155:31337",
        "rpc_url": format!("http://127.0.0.1:{}", port()),
        "peer_rpc_url": format!("http://127.0.0.1:{}", port()),
        "permit2_runtime_hash": vec![1u8; 32],
        "proxy_runtime_hash": vec![2u8; 32],
        "transaction_key_file": seller.state.with_file_name("gas.key"),
        "signer_journal_root": seller.state.with_file_name("signer"),
        "gas_limit": 100000,
        "max_fee_per_gas": "1",
        "max_priority_fee_per_gas": "1",
    });
    let stderr = refused(&configure(&seller, port(), metered.clone(), x402));
    assert!(stderr.contains("x402_exact"), "{stderr}");

    let public = json!({
        "mode": "public_bound",
        "namespace": "eip155:31337",
        "settlement_contract": format!("0x{}", "11".repeat(20)),
        "verifier_version": 1,
        "rpc_url": format!("http://127.0.0.1:{}", port()),
        "from_block": 0,
        "log_block_range": 100,
        "max_log_queries": 10,
        "max_ancestry": 10,
    });
    let stderr = refused(&configure(&seller, port(), metered.clone(), public));
    assert!(stderr.contains("shielded backend"), "{stderr}");

    for profile in [
        json!({"profile": "metered", "max_response_bytes": 64}),
        json!({"profile": "metered", "feed_file": "feed.bin"}),
        json!({"profile": "metered", "max_response_bytes": 65537, "feed_file": "feed.bin"}),
        json!({"profile": "metered", "max_response_bytes": 64, "feed_file": "feed.bin", "payload_file": "payload.bin"}),
        json!({"payload_file": "payload.bin", "feed_file": "feed.bin"}),
        json!({"profile": "streaming", "payload_file": "payload.bin"}),
    ] {
        let stderr = refused(&configure(
            &seller,
            port(),
            profile.clone(),
            shielded_backend(&evidence),
        ));
        assert!(!stderr.is_empty(), "{profile}");
    }
    assert!(!seller.state.exists());
}
