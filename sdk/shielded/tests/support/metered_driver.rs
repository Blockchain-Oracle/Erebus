//! Funded Anvil end to end for prepaid metered access (`docs/metropolis-operations.md`).
//!
//! One negotiated suite-2 settlement buys `METERED_QUANTITY` calls. The test drives the real
//! buyer client against the real access service binary and covers F1-F8: sequential calls, a
//! dropped first response, a service restart, a deleted buyer cursor, a replay after
//! exhaustion, `403` at the cap, serving with the chain stopped, and exactly one settlement.

use super::*;
use axum::{
    body::Bytes,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Router,
};
use erebus_shielded_prover::access::metered::{
    sign_metered_request, MeteredClient, MeteredClientError, MeteredOperation, MeteredProfile,
};

/// Prepaid calls bought by the one settlement.
const METERED_QUANTITY: u128 = 5;
const MAX_RESPONSE_BYTES: u32 = 65536;
const METERED_SERVICE_ID: [u8; 32] = [42; 32];
/// The buyer's suite-2 agreement key in this driver's fixture.
const BUYER_SEED: [u8; 32] = [61; 32];

async fn deploy_metered() -> super::shielded_driver::Deployed {
    super::shielded_driver::deploy_profile(false, Some(1), |template| {
        template.service.unit = "call".into();
        template.service.quantity = BaseUnits::new(METERED_QUANTITY);
        template.service.fulfillment_method = "http-metered-v1".into();
        template.service.fulfillment_digest = MeteredProfile::new(MAX_RESPONSE_BYTES)
            .unwrap()
            .profile_digest();
    })
    .await
}

/// Forwards metered requests to the service and drops the first committed call response, as a
/// lost network reply. The client's in-flight index must survive it and replay the same bytes.
#[derive(Clone)]
struct DropFirstCall {
    upstream: String,
    client: reqwest::Client,
    dropped: Arc<AtomicBool>,
}

async fn drop_forward(State(state): State<DropFirstCall>, body: Bytes) -> Response {
    let operation = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|request| request["operation"].as_str().map(str::to_owned));
    let forwarded = state
        .client
        .post(&state.upstream)
        .body(body.to_vec())
        .send()
        .await;
    let drop_it =
        operation.as_deref() == Some("call") && !state.dropped.swap(true, Ordering::SeqCst);
    match forwarded {
        Ok(response) if !drop_it => {
            let status = StatusCode::from_u16(response.status().as_u16())
                .unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
            let bytes = response.bytes().await.unwrap_or_default();
            if status.is_server_error() {
                let code = serde_json::from_slice::<Value>(&bytes)
                    .ok()
                    .and_then(|body| body["error"].as_str().map(str::to_owned));
                eprintln!("metered fixture upstream status {status}, code {code:?}");
            }
            (status, bytes).into_response()
        }
        Ok(_) => (StatusCode::SERVICE_UNAVAILABLE, r#"{"status":"dropped"}"#).into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            r#"{"status":"unavailable"}"#,
        )
            .into_response(),
    }
}

async fn start_drop_proxy(service_url: &str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let state = DropFirstCall {
        upstream: format!("{service_url}/v1/metered"),
        client: reqwest::Client::new(),
        dropped: Arc::new(AtomicBool::new(false)),
    };
    tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/metered", post(drop_forward))
                .with_state(state),
        )
        .await
        .unwrap();
    });
    url
}

fn write_feed(path: &Path, bytes: &[u8]) {
    let staged = path.with_extension("staged");
    fs::write(&staged, bytes).unwrap();
    fs::rename(staged, path).unwrap();
}

/// Removes one cached call: its record and its stored response.
fn drop_cached_call(cache: &Path, index: u64) {
    let calls = cache.join("metered").join("calls");
    for entry in fs::read_dir(&calls).unwrap() {
        let path = entry.unwrap().path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            if record["index"] == index {
                fs::remove_file(&path).unwrap();
                fs::remove_file(path.with_extension("0.tx")).unwrap();
            }
        }
    }
}

fn dir_bytes(root: &Path) -> u64 {
    fs::read_dir(root).map_or(0, |entries| {
        entries
            .filter_map(|entry| entry.ok())
            .map(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    dir_bytes(&path)
                } else {
                    entry.metadata().map(|metadata| metadata.len()).unwrap_or(0)
                }
            })
            .sum()
    })
}

async fn nonce(client: &reqwest::Client, url: &str, signer: &str) -> u64 {
    let value = rpc(
        client,
        url,
        "eth_getTransactionCount",
        json!([signer, "latest"]),
    )
    .await;
    u64::from_str_radix(value.as_str().unwrap().trim_start_matches("0x"), 16).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, Node dependencies and existing M5 prototype proving artifacts"]
async fn funded_metered_access_serves_replays_and_recovers() {
    let mut deployed = deploy_metered().await;
    let evidence_root = deployed.fixture.root.path().join("metered/evidence");
    fs::create_dir_all(&evidence_root).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&evidence_root, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut seller_config: Value =
        serde_json::from_slice(&fs::read(&deployed.fixture.seller_config).unwrap()).unwrap();
    seller_config["access_evidence_root"] = json!(evidence_root);
    fs::write(&deployed.fixture.seller_config, seller_config.to_string()).unwrap();
    let (buyer, seller) = deployed.fixture.pair(false);
    assert_eq!(buyer["agreement_verified"], true, "{buyer}");
    assert_eq!(buyer["deal_commitment"], seller["deal_commitment"]);
    let root = deployed.fixture.root.path().to_path_buf();
    let payment = native_binary("erebus-payment", env!("CARGO_BIN_EXE_erebus-payment"));

    // One settlement, funded and prepared before anything is submitted.
    let (_, funding) = run_payment(&payment, &root, &deployed.config_path, "funding").await;
    assert_eq!(funding["status"], "ready", "{funding}");
    let signer = deployed.config["signer_address"]
        .as_str()
        .unwrap()
        .to_string();
    let nonce_before = nonce(&deployed.client, &deployed.url, &signer).await;
    let (_, submitted) = run_payment(&payment, &root, &deployed.config_path, "settle").await;
    assert_eq!(submitted["submitted_this_call"], true, "{submitted}");
    let settlement_hash = submitted["transaction_hash"].as_str().unwrap().to_string();
    let mut recovered = submitted;
    let deadline = Instant::now() + Duration::from_secs(180);
    while recovered["payment_verified"] != json!(true) {
        assert!(
            Instant::now() < deadline,
            "settlement did not finalize: {recovered}"
        );
        tokio::time::sleep(Duration::from_secs(1)).await;
        let (_, reply) = run_payment(&payment, &root, &deployed.config_path, "settle").await;
        recovered = reply;
    }
    assert_eq!(recovered["submitted_this_call"], false);
    assert_eq!(recovered["winning_commitment"], buyer["deal_commitment"]);
    assert_eq!(
        nonce(&deployed.client, &deployed.url, &signer).await,
        nonce_before + 1,
        "the settlement is the only gas-account transaction"
    );

    // The seller's metered service over the same agreement and deployment.
    let selected =
        SelectedAgreement::decode(&fs::read(buyer["evidence_file"].as_str().unwrap()).unwrap())
            .unwrap();
    verify_selected_agreement(&selected).unwrap();
    let commitment = buyer["deal_commitment"].as_str().unwrap();
    assert_eq!(
        fs::read(evidence_root.join(format!("{commitment}.evidence"))).unwrap(),
        selected.encode().unwrap(),
        "the seller publishes its accepted agreement without a fixture watcher"
    );
    let feed = root.join("metered/feed.bin");
    write_feed(&feed, b"tick 0");
    let state_root = root.join("metered/state");
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let service_url = format!("http://127.0.0.1:{port}");
    let config_path = root.join("metered/service.json");
    private(
        &config_path,
        &serde_json::to_vec(&json!({
            "service_id": METERED_SERVICE_ID,
            "seller_key": selected.terms.seller_authorization_key.as_bytes(),
            "suite_id": 2,
            "resource": selected.terms.service.resource,
            "profile": "metered",
            "max_response_bytes": MAX_RESPONSE_BYTES,
            "feed_file": feed,
            "evidence_root": evidence_root,
            "state_root": state_root,
            "port": port,
            "backend": {
                "mode": "shielded",
                "chain_id": 31337,
                "pool": deployed.config["pool"],
                "rpc_url": deployed.config["rpc_url"],
                "peer_rpc_url": deployed.config["peer_rpc_url"],
                "first_block": deployed.config["first_block"],
                "first_hash": hex::decode(
                    deployed.config["first_hash"]
                        .as_str()
                        .unwrap()
                        .trim_start_matches("0x"),
                )
                .unwrap(),
                "runtime_keccak256": deployed.config["runtime_keccak256"],
                "poseidon_keccak256": deployed.config["poseidon_keccak256"],
                "deposit_verifier_keccak256": deployed.config["deposit_verifier_keccak256"],
                "transfer_verifier_keccak256": deployed.config["transfer_verifier_keccak256"],
                "withdraw_verifier_keccak256": deployed.config["withdraw_verifier_keccak256"],
            }
        }))
        .unwrap(),
    );
    let mut service =
        super::native_product::service(&config_path, &service_url, &deployed.client).await;

    // The buyer reaches the service through a proxy that drops one committed call response.
    let drop_url = start_drop_proxy(&service_url).await;
    let cache = root.join("metered/cache");
    let endpoint = format!("{drop_url}/v1/metered");
    let client = MeteredClient::open(&endpoint, METERED_SERVICE_ID, &cache, true).unwrap();

    // F2: the first call's response is lost after the seller committed index 0. The retry
    // re-signs index 0 and gets the stored bytes back.
    write_feed(&feed, b"tick 0");
    let started = Instant::now();
    assert_eq!(
        client
            .call(&selected, &BUYER_SEED, None, clock())
            .await
            .unwrap_err(),
        MeteredClientError::Unavailable
    );
    let first = client
        .call(&selected, &BUYER_SEED, None, clock())
        .await
        .unwrap();
    let first_call_ms = started.elapsed().as_millis();
    assert_eq!(first.call_index, 0);
    assert!(first.repeated && !first.cached);
    assert_eq!(fs::read(&first.response_file).unwrap(), b"tick 0");
    // Storage baseline after activation: the entitlement and the first call record.
    let state_after_activation = dir_bytes(&state_root);

    // F1: three calls succeed over one settlement.
    for index in 1..3u64 {
        write_feed(&feed, format!("tick {index}").as_bytes());
        let receipt = client
            .call(&selected, &BUYER_SEED, None, clock())
            .await
            .unwrap();
        assert_eq!(receipt.call_index, index);
        assert!(!receipt.repeated && !receipt.cached);
        assert_eq!(
            fs::read(&receipt.response_file).unwrap(),
            format!("tick {index}").as_bytes()
        );
    }

    // F3: a seller restart between calls resumes the sequence from durable state.
    drop(service);
    service = super::native_product::service(&config_path, &service_url, &deployed.client).await;
    write_feed(&feed, b"tick 3");
    let restarted = client
        .call(&selected, &BUYER_SEED, None, clock())
        .await
        .unwrap();
    assert_eq!(restarted.call_index, 3);
    assert!(!restarted.repeated);

    // F4: a deleted buyer cursor rebuilds from status without consuming, then takes index 4.
    fs::remove_dir_all(&cache).unwrap();
    let rebuilt = MeteredClient::open(&endpoint, METERED_SERVICE_ID, &cache, true).unwrap();
    let mut restored_status = None;
    for attempt in 0..3 {
        match rebuilt.status(&selected, &BUYER_SEED, clock()).await {
            Ok(status) => {
                restored_status = Some(status);
                break;
            }
            Err(MeteredClientError::Unavailable | MeteredClientError::Pending) if attempt < 2 => {
                // Restoring a cursor repeats read-only status/replay operations, never payment.
                assert_eq!(deployed.sends.load(Ordering::SeqCst), 1);
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(error) => panic!("buyer cursor restoration failed: {error:?}"),
        }
    }
    let status = restored_status.expect("buyer status recovered within three attempts");
    assert_eq!(status.seller_next_call_index, 4);
    assert_eq!(status.next_call_index, 4);
    assert_eq!(status.recovered_calls, 4);
    assert!(!status.exhausted);
    write_feed(&feed, b"tick 4");
    let access = native_binary("erebus-access", env!("CARGO_BIN_EXE_erebus-access"));
    let mut command = json!({
        "method": "metered_status",
        "evidence_file": buyer["evidence_file"],
        "buyer_key_file": root.join("buyer/agreement.key"),
        "service_url": endpoint,
        "service_id": hex::encode(METERED_SERVICE_ID),
        "cache_root": cache,
        "allow_loopback_http": true,
    });
    let (ok, command_status) = finish(spawn(&access, &root, command.clone()));
    assert!(ok, "{command_status}");
    assert_eq!(command_status["result"]["next_call_index"], 4);
    command["method"] = json!("metered_call");
    let (ok, fifth) = finish(spawn(&access, &root, command.clone()));
    assert!(ok, "{fifth}");
    assert_eq!(fifth["result"]["call_index"], 4);
    assert_eq!(fifth["result"]["repeated"], false);
    assert_eq!(fifth["result"]["response_hash_consistent"], true);
    assert_eq!(fifth["result"]["resource_verified"], false);
    assert_eq!(fifth["result"]["payment_verified"], false);
    assert_eq!(fifth["result"]["delivery_verified"], false);
    assert_eq!(
        fs::read(fifth["result"]["response_file"].as_str().unwrap()).unwrap(),
        b"tick 4"
    );
    command["call_index"] = json!(4);
    let (ok, cached_fifth) = finish(spawn(&access, &root, command));
    assert!(ok, "{cached_fifth}");
    assert_eq!(cached_fifth["result"]["call_index"], 4);
    assert_eq!(cached_fifth["result"]["cached"], true);

    // F5: an old index replays after exhaustion, from the seller, consuming nothing.
    drop_cached_call(&cache, 0);
    let replay_started = Instant::now();
    let replay = rebuilt
        .call(&selected, &BUYER_SEED, Some(0), clock())
        .await
        .unwrap();
    let replay_ms = replay_started.elapsed().as_millis();
    assert_eq!(replay.call_index, 0);
    assert!(replay.repeated && !replay.cached);
    assert_eq!(fs::read(&replay.response_file).unwrap(), b"tick 0");

    // F6: any index at or beyond N is 403 allowance_exhausted, never 402.
    let exhausted = sign_metered_request(
        &selected,
        METERED_SERVICE_ID,
        MeteredOperation::Call,
        METERED_QUANTITY as u64,
        [0x44; 32],
        clock() + 120,
        &BUYER_SEED,
    )
    .unwrap();
    let response = deployed
        .client
        .post(format!("{service_url}/v1/metered"))
        .json(&exhausted)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 403);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"], "allowance_exhausted", "{body}");

    // F8: the metered calls never submitted a transaction.
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 1);
    assert_eq!(
        nonce(&deployed.client, &deployed.url, &signer).await,
        nonce_before + 1
    );
    let receipt = rpc(
        &deployed.client,
        &deployed.url,
        "eth_getTransactionReceipt",
        json!([settlement_hash]),
    )
    .await;
    let gas_used = u64::from_str_radix(
        receipt["gasUsed"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
        16,
    )
    .unwrap();
    let measurements = json!({
        "label": "local Anvil, M5 prototype proving artifacts, debug build, known-entropy keys",
        "quantity": METERED_QUANTITY,
        "settlements": deployed.sends.load(Ordering::SeqCst),
        "settlement_transaction": settlement_hash,
        "settlement_gas_used": gas_used,
        "first_call_ms": first_call_ms,
        "repeat_call_ms": replay_ms,
        "service_state_bytes": dir_bytes(&state_root),
        "service_state_growth_bytes": dir_bytes(&state_root).saturating_sub(state_after_activation),
        "buyer_cache_bytes": dir_bytes(&cache),
    });
    let measurements_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("target/metered-e2e-measurements.json");
    fs::write(
        &measurements_path,
        serde_json::to_vec_pretty(&measurements).unwrap(),
    )
    .unwrap();
    eprintln!("metered E2E measurements (local Anvil): {measurements}");

    super::native_product::audit_payment(
        &deployed.fixture,
        &buyer,
        &seller,
        &deployed.config,
        &selected,
    );

    // F7: with the chain stopped, the activated service still serves status and repeats.
    deployed.stop_chain();
    drop_cached_call(&cache, 1);
    let offline = rebuilt
        .call(&selected, &BUYER_SEED, Some(1), clock())
        .await
        .unwrap();
    assert_eq!(offline.call_index, 1);
    assert!(offline.repeated && !offline.cached);
    assert_eq!(fs::read(&offline.response_file).unwrap(), b"tick 1");
    let offline_status = rebuilt
        .status(&selected, &BUYER_SEED, clock())
        .await
        .unwrap();
    assert_eq!(
        offline_status.seller_next_call_index,
        METERED_QUANTITY as u64
    );
    assert_eq!(offline_status.remaining, 0);
    assert!(offline_status.exhausted);
    drop(service);
}
