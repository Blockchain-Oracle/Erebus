//! Installed `erebus-shielded-relayer` binary against a loopback-only Anvil chain 10143 fixture.
//!
//! The production chain restriction is preserved: the fixture starts Anvil with `--chain-id 10143`
//! and both fixture scripts accept only loopback URLs and the local chains 31337/10143.
//! `EREBUS_TEST_SHIELDED_RELAYER` may point at a packaged binary; otherwise the cargo-built one is used.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use axum::{extract::State, routing::post, Json, Router};
use erebus_core::{
    commitment::{DealCommitment, DealNullifier},
    domain::DeploymentDomain,
    ids::{AddressBytes, AssetId, ChainNamespace},
    settlement::{PreparedSettlement, SettlementContext},
    shielded::SHIELDED_GUARANTEES,
    suite::SHIELDED_POSEIDON_EDDSA_SUITE_ID,
    terms::{GuaranteeSet, SettlementMode},
};
use erebus_evm::chain::TransactionKey;
use erebus_shielded_prover::relay_request::RelayRequest;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const CHAIN_ID: u64 = 10143;

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

struct Anvil(Child);
impl Drop for Anvil {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Relayer(Child);
impl Relayer {
    fn stop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Relayer {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Recorder {
    upstream: String,
    client: reqwest::Client,
    sent: Mutex<Vec<String>>,
    drop_first: AtomicBool,
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

fn relayer_binary() -> PathBuf {
    std::env::var_os("EREBUS_TEST_SHIELDED_RELAYER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_erebus-shielded-relayer")))
}

async fn rpc(client: &reqwest::Client, url: &str, method: &str, params: Value) -> Value {
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

fn private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

struct Fixture {
    _anvil: Anvil,
    client: reqwest::Client,
    url: String,
    recorder: Arc<Recorder>,
    context: SettlementContext,
    prepared: PreparedSettlement,
    sponsor_hex: String,
    auth: [u8; 32],
    pool: String,
    root: tempfile::TempDir,
    config: PathBuf,
    _relay_server: tokio::task::JoinHandle<()>,
}

impl Fixture {
    async fn boot(drop_first: bool) -> Self {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
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
                    &CHAIN_ID.to_string(),
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
                .arg(CHAIN_ID.to_string())
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
        let data = hex::decode(
            fixture["valid"]
                .as_str()
                .unwrap()
                .strip_prefix("0x")
                .unwrap(),
        )
        .unwrap();
        let (context, prepared) = context_and_prepared(data);
        assert_eq!(
            context.domain.namespace.reference().parse::<u64>().unwrap(),
            CHAIN_ID
        );
        let root = tempfile::tempdir().unwrap();
        let sponsor_key = [24; 32];
        let sponsor = TransactionKey::from_bytes(&sponsor_key).unwrap().address();
        let sponsor_hex = format!("0x{}", hex::encode(sponsor));
        rpc(
            &client,
            &url,
            "anvil_setBalance",
            json!([sponsor_hex, "0xde0b6b3a7640000"]),
        )
        .await;
        let auth = [7; 32];
        let transaction_key = root.path().join("gas.key");
        let auth_key = root.path().join("auth.key");
        private(&transaction_key, &sponsor_key);
        private(&auth_key, &auth);
        let zkey_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../circuits/m5/build/transfer.zkey");
        let zkey = fs::read(&zkey_path).unwrap();
        let relay_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let recorder = Arc::new(Recorder {
            upstream: url.clone(),
            client: client.clone(),
            sent: Mutex::new(Vec::new()),
            drop_first: AtomicBool::new(drop_first),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let recorder_url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/", post(record))
            .with_state(recorder.clone());
        let relay_server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let config = root.path().join("relay.json");
        private(
            &config,
            &serde_json::to_vec(&json!({
                "version": 1,
                "chain_id": CHAIN_ID,
                "pool": fixture["pool"],
                "asset": fixture["token"],
                "verifier_version": 2,
                "first_block": fixture["first_block"],
                "first_hash": fixture["first_hash"],
                "runtime_pins": {
                    "pool": fixture["runtime_keccak256"], "poseidon": fixture["poseidon_keccak256"],
                    "deposit": fixture["deposit_verifier_keccak256"],
                    "transfer": fixture["transfer_verifier_keccak256"],
                    "withdraw": fixture["withdraw_verifier_keccak256"],
                },
                "transfer_zkey_file": zkey_path,
                "transfer_zkey_sha256": format!("{:x}", Sha256::digest(&zkey)),
                "transaction_key_file": transaction_key,
                "authentication_key_file": auth_key,
                "sponsor": sponsor_hex,
                "state_root": root.path().join("state"),
                "signer_journal_root": root.path().join("nonces"),
                "rpc_url": recorder_url,
                "peer_rpc_url": url,
                "port": relay_port,
                "gas_limit": 3_000_000,
                "max_fee_per_gas": "2000000000",
                "max_priority_fee_per_gas": "1000000000",
                "maximum_gas_budget": "6000000000000000",
                "maximum_operations": 1,
                "requests_per_window": 50,
                "window_seconds": 60,
                "timeout_seconds": 60,
                "allow_test_artifacts": true,
            }))
            .unwrap(),
        );
        // Finalize the deployment anchor before the service admits anything.
        rpc(&client, &url, "anvil_mine", json!(["0x40"])).await;
        let pool = fixture["pool"].as_str().unwrap().to_owned();
        Self {
            _anvil: anvil,
            client,
            url,
            recorder,
            context,
            prepared,
            sponsor_hex,
            auth,
            pool,
            root,
            config,
            _relay_server: relay_server,
        }
    }

    fn request(&self) -> Vec<u8> {
        RelayRequest::from_prepared(&self.context, &self.prepared)
            .unwrap()
            .encode()
            .unwrap()
    }

    fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}/v1/relay", self.port())
    }

    fn port(&self) -> u16 {
        serde_json::from_slice::<Value>(&fs::read(&self.config).unwrap()).unwrap()["port"]
            .as_u64()
            .unwrap() as u16
    }

    fn start(&self) -> Relayer {
        let log = self
            .root
            .path()
            .join(format!("relayer-{}.err", std::process::id()));
        let child = Command::new(relayer_binary())
            .env("EREBUS_SHIELDED_RELAY_CONFIG", &self.config)
            .stdout(Stdio::null())
            .stderr(Stdio::from(fs::File::create(&log).unwrap()))
            .spawn()
            .unwrap();
        Relayer(child)
    }

    async fn wait_healthz(&self) {
        for _ in 0..100 {
            if let Ok(reply) = self
                .client
                .get(format!("http://127.0.0.1:{}/healthz", self.port()))
                .send()
                .await
            {
                if reply.status().is_success() {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("relayer did not become healthy");
    }

    async fn post(&self, path: &str, body: &[u8]) -> (u16, Value) {
        let response = self
            .client
            .post(format!("http://127.0.0.1:{}{path}", self.port()))
            .bearer_auth(hex::encode(self.auth))
            .body(body.to_vec())
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        let value = response.json().await.unwrap_or(Value::Null);
        (status, value)
    }

    async fn resume(&self, request: &[u8]) -> Value {
        let mut child = Command::new(relayer_binary())
            .arg("--resume")
            .env("EREBUS_SHIELDED_RELAY_CONFIG", &self.config)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.as_mut().unwrap().write_all(request).unwrap();
        drop(child.stdin.take());
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    async fn mine(&self, blocks: &str) {
        rpc(&self.client, &self.url, "anvil_mine", json!([blocks])).await;
    }

    async fn nonce(&self, address: &str) -> u64 {
        u64::from_str_radix(
            rpc(
                &self.client,
                &self.url,
                "eth_getTransactionCount",
                json!([address, "latest"]),
            )
            .await
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
            16,
        )
        .unwrap()
    }
}

fn assert_exact_payment(fixture: &Fixture, receipt: &Value) {
    assert_eq!(receipt["status"], "0x1", "{receipt}");
    assert_eq!(receipt["from"], fixture.sponsor_hex, "{receipt}");
    assert_eq!(
        receipt["to"].as_str().unwrap().to_lowercase(),
        fixture.pool,
        "{receipt}"
    );
    assert_eq!(receipt["contractAddress"], Value::Null, "{receipt}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, node modules and current M5 local proving artifacts; chain 10143 binary fixture"]
async fn installed_relayer_binary_submits_observes_restarts_and_recovers_finalized_receipt() {
    let fixture = Fixture::boot(false).await;
    let request = fixture.request();
    let calldata = format!("0x{}", hex::encode(&fixture.prepared.backend_evidence));
    let mut relayer = fixture.start();
    fixture.wait_healthz().await;
    let before = fixture.nonce(&fixture.sponsor_hex).await;

    let (status, reply) = fixture.post("/v1/relay", &request).await;
    assert_eq!(status, 200, "{reply}");
    assert_eq!(reply["status"], "pending", "{reply}");
    assert_eq!(reply["broadcast_started"], true, "{reply}");
    assert_eq!(reply["automatic_rebroadcast"], false, "{reply}");
    let tx = reply["transaction_hash"].as_str().unwrap().to_owned();
    assert_eq!(fixture.recorder.sent.lock().unwrap().len(), 1);
    assert_eq!(fixture.nonce(&fixture.sponsor_hex).await, before + 1);

    let (status, observed) = fixture.post("/v1/observe", &request).await;
    assert_eq!(status, 200, "{observed}");
    assert_eq!(observed["status"], "pending", "{observed}");
    assert_eq!(observed["transaction_hash"], tx, "{observed}");

    relayer.stop();
    let _restarted = fixture.start();
    fixture.wait_healthz().await;
    let (status, observed) = fixture.post("/v1/observe", &request).await;
    assert_eq!(status, 200, "{observed}");
    assert_eq!(observed["status"], "pending", "{observed}");

    fixture.mine("0x2").await;
    let receipt = rpc(
        &fixture.client,
        &fixture.url,
        "eth_getTransactionReceipt",
        json!([tx]),
    )
    .await;
    assert_exact_payment(&fixture, &receipt);
    let transaction = rpc(
        &fixture.client,
        &fixture.url,
        "eth_getTransactionByHash",
        json!([tx]),
    )
    .await;
    assert_eq!(
        transaction["input"].as_str().unwrap().to_lowercase(),
        calldata.to_lowercase()
    );
    assert_eq!(transaction["value"], "0x0");
    fixture.mine("0x40").await;

    let (status, finalized) = fixture.post("/v1/observe", &request).await;
    assert_eq!(status, 200, "{finalized}");
    assert_eq!(finalized["status"], "finalized", "{finalized}");
    assert_eq!(finalized["transaction_hash"], tx, "{finalized}");
    assert_eq!(fixture.recorder.sent.lock().unwrap().len(), 1);
    assert_eq!(fixture.nonce(&fixture.sponsor_hex).await, before + 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, node modules and current M5 local proving artifacts; chain 10143 binary fixture"]
async fn installed_relayer_binary_recovers_a_dropped_broadcast_with_explicit_resume() {
    let fixture = Fixture::boot(true).await;
    let request = fixture.request();
    let mut relayer = fixture.start();
    fixture.wait_healthz().await;
    let before = fixture.nonce(&fixture.sponsor_hex).await;

    let (status, held) = fixture.post("/v1/relay", &request).await;
    assert_eq!(status, 200, "{held}");
    assert_eq!(held["status"], "pending", "{held}");
    assert_eq!(held["broadcast_started"], true, "{held}");
    let tx = held["transaction_hash"].as_str().unwrap().to_owned();
    assert_eq!(fixture.recorder.sent.lock().unwrap().len(), 1);

    relayer.stop();
    let _restarted = fixture.start();
    fixture.wait_healthz().await;
    let (status, observed) = fixture.post("/v1/observe", &request).await;
    assert_eq!(status, 200, "{observed}");
    assert_eq!(observed["status"], "pending", "{observed}");

    let resumed = fixture.resume(&request).await;
    assert_eq!(resumed["status"], "pending", "{resumed}");
    assert_eq!(resumed["transaction_hash"], tx, "{resumed}");
    {
        let sent = fixture.recorder.sent.lock().unwrap();
        assert_eq!(sent.len(), 2, "explicit resume rebroadcasts exactly once");
        assert_eq!(sent[0], sent[1], "resume must not change the signed bytes");
    }
    fixture.mine("0x2").await;
    let receipt = rpc(
        &fixture.client,
        &fixture.url,
        "eth_getTransactionReceipt",
        json!([tx]),
    )
    .await;
    assert_exact_payment(&fixture, &receipt);
    fixture.mine("0x40").await;
    let (status, finalized) = fixture.post("/v1/observe", &request).await;
    assert_eq!(status, 200, "{finalized}");
    assert_eq!(finalized["status"], "finalized", "{finalized}");
    assert_eq!(fixture.recorder.sent.lock().unwrap().len(), 2);
    assert_eq!(fixture.nonce(&fixture.sponsor_hex).await, before + 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, node modules and current M5 local proving artifacts; chain 10143 binary fixture"]
async fn installed_relayer_binary_serves_duplicate_requests_with_one_broadcast() {
    let fixture = Fixture::boot(false).await;
    let request = fixture.request();
    let _relayer = fixture.start();
    fixture.wait_healthz().await;
    let before = fixture.nonce(&fixture.sponsor_hex).await;

    let first = fixture.post("/v1/relay", &request);
    let second = fixture.post("/v1/relay", &request);
    let (first, second) = tokio::join!(first, second);
    let mut accepted = 0usize;
    let mut tx = None;
    for (status, reply) in [&first, &second] {
        assert!(
            [200, 429, 503].contains(status),
            "unexpected duplicate response: {status} {reply}"
        );
        if *status == 200 {
            accepted += 1;
            assert_eq!(reply["status"], "pending", "{reply}");
            tx = reply["transaction_hash"].as_str().map(str::to_owned);
        }
    }
    assert!(accepted >= 1, "{first:?} {second:?}");
    let (status, third) = fixture.post("/v1/relay", &request).await;
    assert_eq!(status, 200, "{third}");
    assert_eq!(third["transaction_hash"].as_str(), tx.as_deref(), "{third}");
    assert_eq!(
        fixture.recorder.sent.lock().unwrap().len(),
        1,
        "duplicates must not broadcast again"
    );
    assert_eq!(fixture.nonce(&fixture.sponsor_hex).await, before + 1);

    fixture.mine("0x2").await;
    fixture.mine("0x40").await;
    let (status, finalized) = fixture.post("/v1/observe", &request).await;
    assert_eq!(status, 200, "{finalized}");
    assert_eq!(finalized["status"], "finalized", "{finalized}");
    assert_eq!(fixture.recorder.sent.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, node modules and current M5 local proving artifacts; chain 10143 binary fixture"]
async fn buyer_transport_delegates_to_the_installed_binary_and_never_signs_directly() {
    use erebus_shielded_prover::relay_client::RelayEndpoint;
    let fixture = Fixture::boot(false).await;
    let mut relayer = fixture.start();
    fixture.wait_healthz().await;
    let before = fixture.nonce(&fixture.sponsor_hex).await;

    let buyer_key = [9; 32];
    let buyer = TransactionKey::from_bytes(&buyer_key).unwrap().address();
    let buyer_hex = format!("0x{}", hex::encode(buyer));
    let buyer_nonce = fixture.nonce(&buyer_hex).await;

    let endpoint = RelayEndpoint {
        url: fixture.endpoint(),
        identity: "01".repeat(32),
        authentication_key_file: fixture.root.path().join("auth.key"),
        sponsor_address: fixture.sponsor_hex.clone(),
        timeout_seconds: 30,
        allow_loopback_http: true,
    };
    let reply = endpoint
        .submit(&fixture.context, &fixture.prepared, &fixture.auth)
        .await
        .unwrap()
        .expect("the service must answer");
    assert_eq!(reply.status, "pending");
    assert_eq!(fixture.recorder.sent.lock().unwrap().len(), 1);
    assert_eq!(
        fixture.nonce(&buyer_hex).await,
        buyer_nonce,
        "the buyer transport never signs or submits directly"
    );

    relayer.stop();
    match endpoint
        .submit(&fixture.context, &fixture.prepared, &fixture.auth)
        .await
    {
        Err(_) | Ok(None) => {}
        Ok(Some(_)) => panic!("the stopped relayer must not answer"),
    }
    assert_eq!(
        fixture.recorder.sent.lock().unwrap().len(),
        1,
        "an unavailable relayer must not authorize a direct send"
    );
    assert_eq!(fixture.nonce(&buyer_hex).await, buyer_nonce);
    assert_eq!(fixture.nonce(&fixture.sponsor_hex).await, before + 1);
}

#[test]
fn fixture_scripts_refuse_non_loopback_endpoints_and_foreign_chains() {
    let run = |args: &[&str]| {
        Command::new("node")
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../circuits/m5/scripts/relay-admission-fixture.mjs"),
            )
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    assert!(
        !run(&["http://10.0.0.1:8545"]),
        "non-loopback must be refused"
    );
    assert!(
        !run(&["http://127.0.0.1:1", "1"]),
        "foreign chain must be refused"
    );
    let native = |args: &[&str]| {
        Command::new("node")
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../circuits/m5/scripts/native-fixture.mjs"),
            )
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    assert!(
        !native(&["http://10.0.0.1:8545"]),
        "non-loopback must be refused"
    );
    assert!(
        !native(&["http://127.0.0.1:1", "erc20", "1"]),
        "foreign chain must be refused"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, node modules and current M5 local proving artifacts; chain 10143 binary fixture"]
async fn installed_relayer_binary_answers_an_oversized_body_without_a_reset() {
    let fixture = Fixture::boot(false).await;
    let _relayer = fixture.start();
    fixture.wait_healthz().await;
    let oversized = vec![b' '; 2 * 1024 * 1024];
    let response = fixture
        .client
        .post(fixture.endpoint())
        .bearer_auth(hex::encode(fixture.auth))
        .body(oversized)
        .send()
        .await;
    match response {
        Ok(response) => {
            assert_eq!(
                response.status().as_u16(),
                413,
                "oversized bodies must be refused cleanly"
            );
        }
        Err(error) => panic!("oversized body produced a transport failure instead of 413: {error}"),
    }
}
