//! Service-level relayer fencing, local only.
//!
//! Covers the post-confirmation split: Codex owns the lost-broadcast case; this file
//! covers older-attempt-wins, raced copy, changed calldata under one identity and
//! concurrent duplicate requests against `SponsoredRelay` / `RelayHttpService`.
//! Exhausted sponsor funds is asserted in `relay_admission.rs`.

use std::{fs, path::PathBuf};

use erebus_core::{
    commitment::{DealCommitment, DealNullifier},
    domain::DeploymentDomain,
    ids::{AddressBytes, AssetId, ChainNamespace},
    settlement::{PreparedSettlement, SettlementContext},
    shielded::SHIELDED_GUARANTEES,
    suite::SHIELDED_POSEIDON_EDDSA_SUITE_ID,
    terms::{GuaranteeSet, SettlementMode},
};
use erebus_shielded_prover::{
    index_store::IndexDomain,
    relay_admission::{RelayAdmissionPolicy, TransferVerifier},
    relay_http::{RelayHttpPolicy, RelayHttpService},
    relay_request::RelayRequest,
    relay_service::{SponsorConfig, SponsorError, SponsoredRelay},
    rpc::PoolRuntimePins,
};
use sha2::{Digest, Sha256};

fn signal(data: &[u8], index: usize) -> [u8; 32] {
    data[4 + (8 + index) * 32..4 + (9 + index) * 32]
        .try_into()
        .unwrap()
}

fn context_and_prepared(data: Vec<u8>) -> (SettlementContext, PreparedSettlement) {
    let chain = u64::from_be_bytes(signal(&data, 0)[24..].try_into().unwrap());
    let pool = AddressBytes::new(signal(&data, 1)[12..].to_vec()).unwrap();
    let version = u32::from_be_bytes(signal(&data, 2)[28..].try_into().unwrap());
    let domain = DeploymentDomain {
        namespace: ChainNamespace::parse(&format!("eip155:{chain}")).unwrap(),
        settlement_contract: Some(pool.clone()),
        pool: Some(pool),
        verifier_version: version,
    };
    let guarantees = GuaranteeSet::from_bits(SHIELDED_GUARANTEES).unwrap();
    let context = SettlementContext {
        require_local_proving: true,
        mode: SettlementMode::Shielded,
        domain: domain.clone(),
        suite_id: SHIELDED_POSEIDON_EDDSA_SUITE_ID,
        asset: AssetId::parse(&format!(
            "eip155:{chain}/erc20:0x{}",
            hex::encode(&signal(&data, 3)[12..])
        ))
        .unwrap(),
        required_guarantees: guarantees,
    };
    let prepared = PreparedSettlement {
        operation_ref: [1; 32],
        deal_commitment: DealCommitment::from_bytes(signal(&data, 4)),
        deal_nullifier: DealNullifier::from_bytes(signal(&data, 5)),
        domain,
        mode: SettlementMode::Shielded,
        required_guarantees: guarantees,
        backend_evidence: data,
    };
    (context, prepared)
}

fn key(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../circuits/m5/build")
            .join(format!("{name}.zkey")),
    )
    .expect("run the M5 local prototype first")
}

fn sponsor_key() -> [u8; 32] {
    hex::decode("59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d")
        .unwrap()
        .try_into()
        .unwrap()
}

mod harness {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::{
        net::TcpListener,
        process::{Child, Command, Stdio},
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Duration,
    };

    pub struct Anvil(pub Child);
    impl Drop for Anvil {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    pub struct Server(pub tokio::task::JoinHandle<()>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    pub struct Recorder {
        pub upstream: String,
        pub client: reqwest::Client,
        pub sent: Mutex<Vec<String>>,
        pub drop_first: AtomicBool,
        pub reads: AtomicUsize,
    }

    async fn record(State(state): State<Arc<Recorder>>, Json(request): Json<Value>) -> Json<Value> {
        if request["method"] == "eth_sendRawTransaction" {
            let drop_it = state.drop_first.swap(false, Ordering::SeqCst);
            state
                .sent
                .lock()
                .unwrap()
                .push(request["params"][0].as_str().unwrap().to_owned());
            if drop_it {
                return Json(json!({"jsonrpc":"2.0","id":request["id"],
                    "error":{"code":-32000,"message":"injected unavailable broadcast"}}));
            }
        } else {
            state.reads.fetch_add(1, Ordering::SeqCst);
        }
        Json(
            state
                .client
                .post(&state.upstream)
                .json(&request)
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap(),
        )
    }

    pub async fn rpc(client: &reqwest::Client, url: &str, method: &str, params: Value) -> Value {
        let reply: Value = client
            .post(url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(reply.get("error").is_none(), "{method}: {reply}");
        reply["result"].clone()
    }

    pub fn bytes<const N: usize>(value: &Value) -> [u8; N] {
        hex::decode(value.as_str().unwrap().strip_prefix("0x").unwrap())
            .unwrap()
            .try_into()
            .unwrap()
    }

    pub struct Environment {
        pub _anvil: Anvil,
        pub _server: Server,
        pub url: String,
        pub recorder_url: String,
        pub recorder: Arc<Recorder>,
        pub client: reqwest::Client,
        pub context: SettlementContext,
        pub prepared: PreparedSettlement,
        pub policy: RelayAdmissionPolicy,
        pub key_bytes: Vec<u8>,
        pub sponsor: String,
        pub nonce_before: u64,
    }

    impl Environment {
        pub async fn boot(drop_first_broadcast: bool) -> Self {
            let port = TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port();
            let anvil = Anvil(
                Command::new("anvil")
                    .args([
                        "--port",
                        &port.to_string(),
                        "--chain-id",
                        "31337",
                        "--silent",
                    ])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .unwrap(),
            );
            let url = format!("http://127.0.0.1:{port}");
            let client = reqwest::Client::new();
            let mut ready = false;
            for _ in 0..100 {
                if client
                    .post(&url)
                    .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}))
                    .send()
                    .await
                    .is_ok()
                {
                    ready = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            assert!(ready, "Anvil did not start");
            let fixture_url = url.clone();
            let fixture: Value = tokio::task::spawn_blocking(move || {
                let output = Command::new("node")
                    .arg(
                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("../../circuits/m5/scripts/relay-admission-fixture.mjs"),
                    )
                    .arg(fixture_url)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                serde_json::from_slice(&output.stdout).unwrap()
            })
            .await
            .unwrap();
            assert_eq!(fixture["test_only"], true);
            let pool = bytes::<20>(&fixture["pool"]);
            let data = |name: &str| {
                hex::decode(fixture[name].as_str().unwrap().strip_prefix("0x").unwrap()).unwrap()
            };
            let (context, prepared) = context_and_prepared(data("valid"));
            let policy = RelayAdmissionPolicy {
                context: context.clone(),
                deployment: IndexDomain {
                    chain_id: 31337,
                    pool,
                    first_block: fixture["first_block"].as_u64().unwrap(),
                    first_hash: bytes(&fixture["first_hash"]),
                },
                runtime_pins: PoolRuntimePins {
                    pool: bytes(&fixture["runtime_keccak256"]),
                    poseidon: bytes(&fixture["poseidon_keccak256"]),
                    deposit: bytes(&fixture["deposit_verifier_keccak256"]),
                    transfer: bytes(&fixture["transfer_verifier_keccak256"]),
                    withdraw: bytes(&fixture["withdraw_verifier_keccak256"]),
                },
                sponsor: bytes(&fixture["sponsor"]),
                gas_limit: 3_000_000,
            };
            // Anvil's explicit finalized tag lags its head; finalize the creation first.
            rpc(&client, &url, "anvil_mine", json!(["0x40"])).await;
            let recorder = Arc::new(Recorder {
                upstream: url.clone(),
                client: client.clone(),
                sent: Mutex::new(Vec::new()),
                drop_first: AtomicBool::new(drop_first_broadcast),
                reads: AtomicUsize::new(0),
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let recorder_url = format!("http://{}", listener.local_addr().unwrap());
            let app = Router::new()
                .route("/", post(record))
                .with_state(recorder.clone());
            let server = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let sponsor = fixture["sponsor"].as_str().unwrap().to_owned();
            let nonce_before = u64::from_str_radix(
                rpc(
                    &client,
                    &url,
                    "eth_getTransactionCount",
                    json!([sponsor, "latest"]),
                )
                .await
                .as_str()
                .unwrap()
                .trim_start_matches("0x"),
                16,
            )
            .unwrap();
            Self {
                _anvil: anvil,
                _server: Server(server),
                url,
                recorder_url,
                recorder,
                client,
                context,
                prepared,
                policy,
                key_bytes: key("transfer"),
                sponsor,
                nonce_before,
            }
        }

        pub fn request(&self) -> Vec<u8> {
            RelayRequest::from_prepared(&self.context, &self.prepared)
                .unwrap()
                .encode()
                .unwrap()
        }

        pub fn config(&self, root: &std::path::Path) -> SponsorConfig {
            SponsorConfig {
                root: root.join("service"),
                signer_root: root.join("nonces"),
                admission: self.policy.clone(),
                fees: erebus_evm::chain::Eip1559Fees::new(3_000_000_000, 1_000_000_000).unwrap(),
                replacement_fees: None,
                rpc_url: self.recorder_url.clone(),
                peer_rpc_url: self.url.clone(),
                timeout: Duration::from_secs(10),
            }
        }

        pub async fn connect(&self, root: &std::path::Path) -> SponsoredRelay {
            SponsoredRelay::connect(
                self.config(root),
                TransferVerifier::from_zkey_bytes(
                    &self.key_bytes,
                    &format!("{:x}", Sha256::digest(&self.key_bytes)),
                )
                .unwrap(),
                &sponsor_key(),
            )
            .await
            .unwrap()
        }

        pub async fn mine_inclusion(&self) {
            rpc(&self.client, &self.url, "anvil_mine", json!(["0x2"])).await;
        }

        pub async fn mine_finality(&self) {
            rpc(&self.client, &self.url, "anvil_mine", json!(["0x40"])).await;
        }
    }
}

#[tokio::test]
#[ignore = "requires Anvil, node modules and current M5 local proving artifacts"]
async fn sponsor_older_attempt_wins_and_later_calls_only_observe() {
    let env = harness::Environment::boot(true).await;
    let root = tempfile::tempdir().unwrap();
    let service = env.connect(root.path()).await;
    let request = env.request();

    let first = service.relay(&request).await.unwrap();
    assert!(first.finalized.is_none());
    assert_eq!(env.recorder.sent.lock().unwrap().len(), 1, "one attempt");

    let repeated = service.relay(&request).await.unwrap();
    assert!(repeated.finalized.is_none());
    assert_eq!(
        env.recorder.sent.lock().unwrap().len(),
        1,
        "a duplicate request must not resend"
    );

    let resumed = service.resume(&request).await.unwrap();
    assert!(resumed.finalized.is_none());
    {
        let sent = env.recorder.sent.lock().unwrap();
        assert_eq!(sent.len(), 2, "explicit resume rebroadcasts exactly once");
        assert_eq!(
            sent[0], sent[1],
            "the older attempt's bytes must not change"
        );
    }
    let hash = resumed.transaction_hash.unwrap();
    env.mine_inclusion().await;
    let receipt = harness::rpc(
        &env.client,
        &env.url,
        "eth_getTransactionReceipt",
        serde_json::json!([format!("0x{}", hex::encode(hash))]),
    )
    .await;
    assert_eq!(receipt["status"], "0x1");
    env.mine_finality().await;

    let finalized = service.resume(&request).await.unwrap();
    assert!(finalized.finalized.unwrap().success);
    assert_eq!(
        env.recorder.sent.lock().unwrap().len(),
        2,
        "a finalized operation is never rebroadcast"
    );
    assert_eq!(service.relay(&request).await.unwrap(), finalized);
    assert_eq!(service.observe(&request).await.unwrap(), finalized);
    assert_eq!(
        harness::rpc(
            &env.client,
            &env.url,
            "eth_getTransactionCount",
            serde_json::json!([env.sponsor, "latest"]),
        )
        .await,
        format!("0x{:x}", env.nonce_before + 1)
    );
    assert_eq!(
        erebus_evm::chain::SignerJournal::open(
            root.path().join("nonces"),
            31337,
            env.policy.sponsor
        )
        .unwrap()
        .claim_nonce()
        .unwrap(),
        None
    );
}

#[tokio::test]
#[ignore = "requires Anvil, node modules and current M5 local proving artifacts"]
async fn raced_copy_landing_first_is_observed_without_a_second_broadcast() {
    let env = harness::Environment::boot(true).await;
    let root = tempfile::tempdir().unwrap();
    let service = env.connect(root.path()).await;
    let request = env.request();

    let held = service.relay(&request).await.unwrap();
    assert!(held.finalized.is_none());
    let copy = held.transaction_hash.unwrap();
    let raw = env.recorder.sent.lock().unwrap()[0].clone();
    assert_eq!(env.recorder.sent.lock().unwrap().len(), 1);

    // Anyone can relay the copied bytes; the service has not seen a response.
    harness::rpc(
        &env.client,
        &env.url,
        "eth_sendRawTransaction",
        serde_json::json!([raw]),
    )
    .await;
    env.mine_inclusion().await;
    let receipt = harness::rpc(
        &env.client,
        &env.url,
        "eth_getTransactionReceipt",
        serde_json::json!([format!("0x{}", hex::encode(copy))]),
    )
    .await;
    assert_eq!(receipt["status"], "0x1");
    env.mine_finality().await;

    let observed = service.relay(&request).await.unwrap();
    assert!(observed.finalized.unwrap().success);
    assert_eq!(observed.transaction_hash, Some(copy));
    assert_eq!(
        env.recorder.sent.lock().unwrap().len(),
        1,
        "a raced copy must not trigger a competing broadcast"
    );
    assert_eq!(service.resume(&request).await.unwrap(), observed);
    assert_eq!(env.recorder.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
#[ignore = "requires Anvil, node modules and current M5 local proving artifacts"]
async fn changed_calldata_under_the_same_identity_is_refused_before_signing() {
    let env = harness::Environment::boot(false).await;
    let root = tempfile::tempdir().unwrap();
    let service = env.connect(root.path()).await;
    let request = env.request();

    let pending = service.relay(&request).await.unwrap();
    assert!(pending.finalized.is_none());
    assert_eq!(env.recorder.sent.lock().unwrap().len(), 1);
    let claim = erebus_evm::chain::SignerJournal::open(
        root.path().join("nonces"),
        31337,
        env.policy.sponsor,
    )
    .unwrap()
    .claim_nonce()
    .unwrap();
    assert!(claim.is_some(), "the original attempt claims its lane");

    let mut changed: serde_json::Value = serde_json::from_slice(&request).unwrap();
    let calldata = changed["calldata"].as_str().unwrap().to_owned();
    let mut bytes = hex::decode(&calldata[2..]).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    changed["calldata"] = serde_json::Value::String(format!("0x{}", hex::encode(&bytes)));
    let changed = serde_json::to_vec(&changed).unwrap();
    assert!(matches!(
        service.request_identity(&changed),
        Err(SponsorError::Admission)
    ));
    assert!(matches!(
        service.relay(&changed).await,
        Err(SponsorError::Admission)
    ));
    let mut uppercase: serde_json::Value = serde_json::from_slice(&request).unwrap();
    uppercase["calldata"] =
        serde_json::Value::String(uppercase["calldata"].as_str().unwrap().to_uppercase());
    let uppercase = serde_json::to_vec(&uppercase).unwrap();
    assert!(matches!(
        service.relay(&uppercase).await,
        Err(SponsorError::Configuration)
    ));
    assert_eq!(
        env.recorder.sent.lock().unwrap().len(),
        1,
        "changed calldata must never sign or send"
    );
    assert_eq!(
        erebus_evm::chain::SignerJournal::open(
            root.path().join("nonces"),
            31337,
            env.policy.sponsor
        )
        .unwrap()
        .claim_nonce()
        .unwrap(),
        claim,
        "no new claim may appear"
    );

    env.mine_inclusion().await;
    env.mine_finality().await;
    let finalized = service.resume(&request).await.unwrap();
    assert!(finalized.finalized.unwrap().success);
    assert_eq!(env.recorder.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
#[ignore = "requires Anvil, node modules and current M5 local proving artifacts"]
async fn concurrent_duplicate_service_requests_broadcast_once() {
    let env = harness::Environment::boot(false).await;
    let root = tempfile::tempdir().unwrap();
    let service = env.connect(root.path()).await;
    let request = env.request();

    let transport = RelayHttpService::new(
        service,
        [7; 32],
        &root.path().join("http-quota"),
        RelayHttpPolicy {
            requests_per_window: 50,
            window_seconds: 60,
            maximum_gas_budget: 9_000_000_000_000_000,
            maximum_operations: 1,
            timeout_seconds: 60,
        },
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = harness::Server(tokio::spawn(async move {
        axum::serve(listener, transport.router()).await.unwrap();
    }));

    let mut handles = Vec::new();
    for _ in 0..4 {
        let client = env.client.clone();
        let endpoint = endpoint.clone();
        let body = request.clone();
        handles.push(tokio::spawn(async move {
            client
                .post(format!("{endpoint}/v1/relay"))
                .bearer_auth(hex::encode([7; 32]))
                .body(body)
                .send()
                .await
                .unwrap()
        }));
    }
    let mut accepted = 0usize;
    let mut statuses = Vec::new();
    for handle in handles {
        let response = handle.await.unwrap();
        let status = response.status().as_u16();
        statuses.push(status);
        if status == 200 {
            accepted += 1;
            let body: serde_json::Value = response.json().await.unwrap();
            assert_eq!(body["status"], "pending");
        } else {
            let body: serde_json::Value = response.json().await.unwrap();
            assert!(
                body["error"]
                    .as_str()
                    .unwrap()
                    .contains("retain the operation"),
                "losing duplicates must not report an unowned failure: {body}"
            );
        }
    }
    assert!(accepted >= 1, "{statuses:?}");
    assert!(statuses
        .iter()
        .all(|status| [200, 429, 503].contains(status)));
    assert_eq!(
        env.recorder.sent.lock().unwrap().len(),
        1,
        "concurrent duplicates must produce one broadcast"
    );
    drop(server);

    env.mine_inclusion().await;
    env.mine_finality().await;
    let restarted = env.connect(root.path()).await;
    let finalized = restarted.observe(&request).await.unwrap();
    assert!(finalized.finalized.unwrap().success);
    assert_eq!(env.recorder.sent.lock().unwrap().len(), 1);
}
