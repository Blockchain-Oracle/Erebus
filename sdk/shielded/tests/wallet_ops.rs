//! Installed `erebus-wallet` onboarding, native wrapping, and backup recovery against Anvil.
//!
//! The wallet generates fresh note material from the operating system; nothing here derives
//! note secrets from the M5 test fixture. Commands run as separate processes and verify
//! finalized effects, retry safety, and stale-backup recovery through fresh RPC endpoints.
//! Set `EREBUS_TEST_INSTALLED_BIN_DIR` to qualify binaries installed outside the checkout;
//! otherwise Cargo-built binaries run. Pool keys and token/wrapper contracts are test fixtures.

use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use axum::{extract::State, Router};
use erebus_core::shielded_auth::derive_key;
use erebus_evm::chain::TransactionKey;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const ANVIL_ACCOUNT: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

#[path = "support/random_asset.rs"]
mod random_asset;
const MINT_SELECTOR: [u8; 4] = [0x40, 0xc1, 0x0f, 0x19];
const BALANCE_SELECTOR: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];
const ALLOWANCE_SELECTOR: [u8; 4] = [0xdd, 0x62, 0xed, 0x3e];
const FEE_ON_TRANSFER_SELECTOR: [u8; 4] = [0xf1, 0x07, 0x77, 0x9f];
const FAIL_PAYOUT_SELECTOR: [u8; 4] = [0xc6, 0xbe, 0x62, 0xb9];

struct ServiceProcess(Child);

impl Drop for ServiceProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn native_binary(name: &str, fallback: &str) -> PathBuf {
    match std::env::var_os("EREBUS_TEST_INSTALLED_BIN_DIR") {
        Some(directory) => {
            let directory = PathBuf::from(directory).canonicalize().unwrap();
            let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .canonicalize()
                .unwrap();
            assert!(
                !directory.starts_with(checkout),
                "installed binaries must be outside the checkout"
            );
            let binary = directory.join(name).canonicalize().unwrap();
            assert!(binary.starts_with(&directory) && binary.is_file());
            binary
        }
        None => fallback.into(),
    }
}

fn private(path: &Path, bytes: &[u8]) {
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .unwrap()
        .read_exact(&mut bytes)
        .unwrap();
    bytes
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn rpc(client: &reqwest::Client, url: &str, method: &str, params: Value) -> Value {
    client
        .post(url)
        .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

fn word(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    out[32 - bytes.len()..].copy_from_slice(bytes);
    out
}

fn encode_call(selector: &[u8; 4], words: &[[u8; 32]]) -> String {
    let mut data = selector.to_vec();
    for word in words {
        data.extend_from_slice(word);
    }
    format!("0x{}", hex::encode(data))
}

/// Reads one ERC-20 allowance word.
async fn allowance(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    owner: &str,
    spender: [u8; 20],
) -> u128 {
    let owner: [u8; 20] = hex::decode(owner.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();
    let response = rpc(
        client,
        url,
        "eth_call",
        json!([{
            "to": token,
            "data": encode_call(&ALLOWANCE_SELECTOR, &[word(&owner), word(&spender)])
        }, "latest"]),
    )
    .await;
    let text = response["result"]
        .as_str()
        .unwrap()
        .trim_start_matches("0x");
    let bytes = hex::decode(text).unwrap();
    u128::from_be_bytes(bytes[16..].try_into().unwrap())
}

/// Waits for an allowance value, bounded, so a submitted approval can be included.
async fn wait_for_allowance(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    owner: &str,
    spender: [u8; 20],
    expected: u128,
) -> u128 {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let value = allowance(client, url, token, owner, spender).await;
        if value >= expected || Instant::now() >= deadline {
            return value;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Waits for a token balance, bounded, so a submitted transaction can be included.
async fn wait_for_balance(
    client: &reqwest::Client,
    url: &str,
    token: &str,
    account: &str,
    expected: u128,
) -> u128 {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let balance = token_balance(client, url, token, account).await;
        if balance == expected || Instant::now() >= deadline {
            return balance;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

async fn token_balance(client: &reqwest::Client, url: &str, token: &str, account: &str) -> u128 {
    let address: [u8; 20] = hex::decode(account.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();
    let response = rpc(
        client,
        url,
        "eth_call",
        json!([{"to": token, "data": encode_call(&BALANCE_SELECTOR, &[word(&address)])}, "latest"]),
    )
    .await;
    let text = response["result"]
        .as_str()
        .unwrap()
        .trim_start_matches("0x");
    let bytes = hex::decode(text).unwrap();
    u128::from_be_bytes(bytes[16..].try_into().unwrap())
}

struct Running(Option<Child>);

fn spawn(binary: &Path, cwd: &Path, request: Value) -> Running {
    let mut child = Command::new(binary)
        .current_dir(cwd)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    Running(Some(child))
}

/// Runs one wallet request and returns (ok, parsed output, exit code).
fn finish(running: Running) -> (bool, Value, i32) {
    let mut running = running;
    let output = running.0.take().unwrap().wait_with_output().unwrap();
    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let value = serde_json::from_str(&stdout)
        .or_else(|_| serde_json::from_str(&stderr))
        .unwrap_or_else(|_| json!({"status":"missing","stdout":stdout,"stderr":stderr}));
    (output.status.success(), value, code)
}

struct ArtifactServer {
    url: String,
    bodies: Arc<HashMap<String, Vec<u8>>>,
}

async fn artifact_server(files: HashMap<String, Vec<u8>>) -> ArtifactServer {
    use axum::extract::Path as RoutePath;
    async fn serve(
        State(bodies): State<Arc<HashMap<String, Vec<u8>>>>,
        RoutePath(name): RoutePath<String>,
    ) -> Vec<u8> {
        bodies.get(&name).cloned().unwrap_or_default()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let bodies = Arc::new(files);
    let state = Arc::clone(&bodies);
    tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/{name}", axum::routing::get(serve))
                .with_state(state),
        )
        .await
        .unwrap();
    });
    ArtifactServer {
        url: format!("http://{address}"),
        bodies,
    }
}

async fn fixture_manifest(deployment: &Value) -> Value {
    let build = PathBuf::from(deployment["artifact_build"].as_str().unwrap());
    let mut files = HashMap::new();
    for kind in ["deposit", "transfer", "withdraw"] {
        for suffix in ["wasm", "r1cs", "zkey"] {
            let path = if suffix == "wasm" {
                build.join(format!("{kind}_js/{kind}.wasm"))
            } else {
                build.join(format!("{kind}.{suffix}"))
            };
            files.insert(format!("{kind}.{suffix}"), fs::read(path).unwrap());
        }
    }
    let server = artifact_server(files).await;
    let circuits: Vec<Value> = ["deposit", "transfer", "withdraw"]
        .into_iter()
        .map(|kind| {
            let mut circuit = json!({"kind": kind});
            for suffix in ["wasm", "r1cs", "zkey"] {
                let name = format!("{kind}.{suffix}");
                let body = &server.bodies[&name];
                circuit[suffix] = json!({
                    "url": format!("{}/{name}", server.url),
                    "sha256": hex::encode(Sha256::digest(body)),
                    "bytes": body.len(),
                });
            }
            circuit
        })
        .collect();
    json!({
        "version": 1, "chain_id": 31337, "settlement_contract": deployment["pool"],
        "verifier_version": 2, "test_only": true, "circuits": circuits,
    })
}

/// A loopback JSON-RPC forwarder that can drop the next raw-transaction response once.
#[derive(Clone)]
struct LossyState {
    upstream: String,
    client: reqwest::Client,
    drop_next: Arc<AtomicBool>,
    sends: Arc<AtomicUsize>,
}

async fn lossy_proxy(upstream: String) -> String {
    lossy_proxy_with_send_count(upstream).await.0
}

async fn lossy_proxy_with_send_count(upstream: String) -> (String, Arc<AtomicUsize>) {
    use axum::{
        extract::State as AxumState, http::StatusCode, response::IntoResponse,
        routing::post as axum_post,
    };
    async fn forward(
        AxumState(state): AxumState<LossyState>,
        body: String,
    ) -> axum::response::Response {
        let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let is_send = |request: &Value| {
            matches!(
                request["method"].as_str(),
                Some("eth_sendRawTransaction" | "eth_sendTransaction")
            )
        };
        let sends = match request.as_array() {
            Some(requests) => requests.iter().filter(|request| is_send(request)).count(),
            None => usize::from(is_send(&request)),
        };
        state.sends.fetch_add(sends, Ordering::SeqCst);
        let dropping = sends > 0 && state.drop_next.swap(false, Ordering::SeqCst);
        // Always forward; dropping means only the response is lost, so the transaction can
        // still land while the caller sees an unknown outcome.
        match state
            .client
            .post(&state.upstream)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
        {
            Ok(response) => {
                let body = response.text().await.unwrap_or_default();
                if dropping {
                    return (StatusCode::BAD_GATEWAY, "dropped").into_response();
                }
                (StatusCode::OK, body).into_response()
            }
            Err(_) => (StatusCode::BAD_GATEWAY, "upstream").into_response(),
        }
    }
    async fn mark(AxumState(state): AxumState<LossyState>) -> &'static str {
        state.drop_next.store(true, Ordering::SeqCst);
        "ok"
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = LossyState {
        upstream,
        client: reqwest::Client::new(),
        drop_next: Arc::new(AtomicBool::new(false)),
        sends: Arc::new(AtomicUsize::new(0)),
    };
    let sends = state.sends.clone();
    tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/", axum_post(forward))
                .route("/drop-next", axum_post(mark))
                .with_state(state),
        )
        .await
        .unwrap();
    });
    (format!("http://{address}"), sends)
}

async fn wait_for_chain(client: &reqwest::Client, url: &str) {
    for _ in 0..200 {
        if client
            .post(url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}))
            .send()
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("anvil did not start");
}

async fn mine(client: &reqwest::Client, url: &str) {
    rpc(client, url, "anvil_mine", json!(["0x2"])).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil and generated M5 prototype artifacts; local prototype setup"]
async fn installed_wallet_deposits_random_notes_and_withdraws_one() {
    let port = free_port();
    let anvil = ServiceProcess(
        Command::new("anvil")
            .args([
                "--port",
                &port.to_string(),
                "--chain-id",
                "31337",
                "--slots-in-an-epoch",
                "1",
                "--block-time",
                "1",
                "--silent",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let url = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    wait_for_chain(&client, &url).await;
    let proxy_url = lossy_proxy(url.clone()).await;
    let peer_url = url.clone();

    // Deploy the pool through the existing native fixture; the fixture's own note is ignored.
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../circuits/m5/scripts/native-fixture.mjs");
    let fixture_url = url.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new("node")
            .arg(script)
            .arg(fixture_url)
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let deployment: Value = serde_json::from_slice(&output.stdout).unwrap();
    let pool = deployment["pool"].as_str().unwrap().to_owned();
    let token = deployment["token"].as_str().unwrap().to_owned();
    let first_block = deployment["first_block"].as_u64().unwrap();
    let first_hash = deployment["first_hash"].as_str().unwrap().to_owned();

    // Fresh participant material from the operating system.
    let transaction_key = random_bytes::<32>();
    let signer = TransactionKey::from_bytes(&transaction_key)
        .unwrap()
        .address();
    let signer_hex = format!("0x{}", hex::encode(signer));
    let agreement_seed = random_bytes::<32>();
    let owner = derive_key(&agreement_seed).unwrap();
    let wallet_key = random_bytes::<32>();
    let recipient: [u8; 20] = random_bytes();

    rpc(
        &client,
        &url,
        "anvil_setBalance",
        json!([signer_hex, "0x56bc75e2d63100000"]),
    )
    .await;
    // Mint the test token to the wallet's own gas signer from the unlocked deployment account.
    let mint = encode_call(
        &MINT_SELECTOR,
        &[word(&signer), word(&100u128.to_be_bytes())],
    );
    let sent = rpc(
        &client,
        &url,
        "eth_sendTransaction",
        json!([{"from": ANVIL_ACCOUNT, "to": token, "data": mint, "gas": "0x100000"}]),
    )
    .await;
    assert!(sent["result"].is_string(), "mint failed: {sent}");
    let pool_address: [u8; 20] = hex::decode(pool.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();
    mine(&client, &url).await;

    // Local fixture manifest only; the caller pins its exact bytes.
    let manifest = fixture_manifest(&deployment).await;
    let root = tempfile::tempdir().unwrap();
    let manifest_path = root.path().join("manifest.json");
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
    std::fs::write(&manifest_path, &manifest_bytes).unwrap();
    let manifest_digest = hex::encode(Sha256::digest(&manifest_bytes));

    let state_root = root.path().join("state");
    std::fs::create_dir(&state_root).unwrap();
    let transaction_key_file = root.path().join("transaction.key");
    let wallet_key_file = root.path().join("wallet.key");
    let agreement_key_file = root.path().join("agreement.key");
    private(&transaction_key_file, &transaction_key);
    private(&wallet_key_file, &wallet_key);
    private(&agreement_key_file, &owner);
    let config_path = root.path().join("wallet.json");
    private(
        &config_path,
        json!({
            "version": 1,
            "state_root": state_root,
            "namespace": "eip155:31337",
            "pool": pool,
            "verifier_version": 2,
            "asset": token,
            "wrapper": deployment["wmon"].as_str().unwrap(),
            "rpc_url": proxy_url,
            "peer_rpc_url": peer_url,
            "first_block": first_block,
            "first_hash": first_hash,
            "wallet_file": state_root.join("wallet/notes.enc"),
            "wallet_key_file": wallet_key_file,
            "agreement_key_file": agreement_key_file,
            "signer_address": signer_hex,
            "signer_journal_root": state_root.join("signer-journal"),
            "transaction_key_file": transaction_key_file,
            "artifact_manifest_file": manifest_path,
            "artifact_manifest_sha256": manifest_digest,
            "artifact_cache_root": state_root.join("artifacts"),
            "allow_test_artifacts": true,
            "allow_loopback_artifact_http": true,
            "gas_limit": 2_000_000u64,
            "max_fee_per_gas": "1000000000",
            "max_priority_fee_per_gas": "100000000",
            "timeout_seconds": 30,
            "max_scan_blocks": 1000,
            "artifact_timeout_seconds": 120,
        })
        .to_string()
        .as_bytes(),
    );

    let wallet_bin = native_binary("erebus-wallet", env!("CARGO_BIN_EXE_erebus-wallet"));
    let request = |method: &str, extra: Value| {
        let mut value = json!({"method": method, "config_file": config_path});
        if let Some(object) = extra.as_object() {
            for (key, extra_value) in object {
                value_insert(&mut value, key, extra_value.clone());
            }
        }
        value
    };

    // 1. Allowance onboarding through the installed wallet; wait until the chain reflects it.
    let approved = finish(spawn(
        &wallet_bin,
        root.path(),
        request("approve", json!({"amount":"1000"})),
    ));
    assert!(
        approved.0,
        "approve failed: {} (exit {})",
        approved.1, approved.2
    );
    assert_eq!(
        wait_for_allowance(&client, &url, &token, &signer_hex, pool_address, 1000).await,
        1000
    );

    // 2. A fee-on-transfer deposit proves, broadcasts, and reverts in the pool. Only a
    // finalized canonical receipt may reconcile it back to a retryable plan.
    let enable_fee = encode_call(&FEE_ON_TRANSFER_SELECTOR, &[word(&1u128.to_be_bytes())]);
    let fee = rpc(
        &client,
        &url,
        "eth_sendTransaction",
        json!([{"from": ANVIL_ACCOUNT, "to": token, "data": enable_fee, "gas": "0x100000"}]),
    )
    .await;
    assert!(fee["result"].is_string(), "setFeeOnTransfer failed: {fee}");
    mine(&client, &url).await;

    let failed = finish(spawn(
        &wallet_bin,
        root.path(),
        request("deposit", json!({"amount":"10"})),
    ));
    assert!(
        failed.0,
        "reverting deposit was not submitted: {} (exit {})",
        failed.1, failed.2
    );
    let promised = failed.1["result"]["commitment"]
        .as_str()
        .unwrap()
        .to_owned();
    let reverted = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        let reverted_plan = value["result"]["reverted"]
            .as_array()
            .is_some_and(|operations| {
                operations
                    .iter()
                    .any(|operation| operation["kind"] == "deposit")
            });
        let retryable = value["result"]["pending"]
            .as_array()
            .is_some_and(|operations| {
                operations.iter().any(|operation| {
                    operation["kind"] == "deposit"
                        && operation["state"] == "planned"
                        && operation["retained"] == false
                })
            });
        reverted_plan && retryable
    })
    .await;
    assert!(
        reverted.0,
        "finalized revert was never reconciled: {reverted:?}"
    );

    // 3. Clear the token restriction and retry; the same note opening is reused.
    let disable_fee = encode_call(&FEE_ON_TRANSFER_SELECTOR, &[word(&0u128.to_be_bytes())]);
    let fee = rpc(
        &client,
        &url,
        "eth_sendTransaction",
        json!([{"from": ANVIL_ACCOUNT, "to": token, "data": disable_fee, "gas": "0x100000"}]),
    )
    .await;
    assert!(fee["result"].is_string(), "setFeeOnTransfer failed: {fee}");
    mine(&client, &url).await;
    let retried = finish(spawn(
        &wallet_bin,
        root.path(),
        request("deposit", json!({"amount":"10"})),
    ));
    assert!(
        retried.0,
        "retry failed: {} (exit {})",
        retried.1, retried.2
    );
    let first_commitment = retried.1["result"]["commitment"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        first_commitment, promised,
        "a retry must reuse the persisted note opening"
    );
    let first_included = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        value["result"]["notes"]
            .as_array()
            .is_some_and(|notes| notes.iter().filter(|note| note["included"] == true).count() >= 1)
            && value["result"]["pending"]
                .as_array()
                .is_some_and(Vec::is_empty)
    })
    .await;
    assert!(
        first_included.0,
        "first deposit never finalized: {first_included:?}"
    );

    // 4. A second deposit of the same amount produces different random material.
    let second = finish(spawn(
        &wallet_bin,
        root.path(),
        request("deposit", json!({"amount":"10"})),
    ));
    assert!(
        second.0,
        "second deposit failed: {} (exit {})",
        second.1, second.2
    );
    let second_commitment = second.1["result"]["commitment"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_ne!(
        first_commitment, second_commitment,
        "note material must be random, not deterministic"
    );
    let two_included = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        value["result"]["notes"]
            .as_array()
            .is_some_and(|notes| notes.iter().filter(|note| note["included"] == true).count() >= 2)
            && value["result"]["pending"]
                .as_array()
                .is_some_and(Vec::is_empty)
    })
    .await;
    assert!(
        two_included.0,
        "second deposit never finalized: {two_included:?}"
    );
    assert_eq!(
        wait_for_balance(&client, &url, &token, &signer_hex, 80).await,
        80
    );

    // Retain a backup before withdrawal, when both notes still appear unspent.
    let wallet_backup = fs::read(state_root.join("wallet/notes.enc")).unwrap();

    // 5. Withdraw the first note and verify consumption plus exact balances.
    let withdrawal = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "withdraw",
            json!({"commitment": first_commitment, "recipient": format!("0x{}", hex::encode(recipient))}),
        ),
    ));
    assert!(
        withdrawal.0,
        "withdrawal failed: {} (exit {})",
        withdrawal.1, withdrawal.2
    );
    assert_eq!(
        wait_for_balance(&client, &url, &token, &signer_hex, 80).await,
        80,
        "withdrawal must not touch the signer's token balance"
    );
    let consumed = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        value["result"]["notes"].as_array().is_some_and(|notes| {
            notes
                .iter()
                .any(|note| note["consumed"] == true && note["commitment"] == first_commitment)
        }) && value["result"]["pending"]
            .as_array()
            .is_some_and(Vec::is_empty)
    })
    .await;
    assert!(consumed.0, "sync never observed consumption: {consumed:?}");
    assert_eq!(
        wait_for_balance(
            &client,
            &url,
            &token,
            &format!("0x{}", hex::encode(recipient)),
            10
        )
        .await,
        10
    );

    // 6. Installed native wrap, completed only after the finalized WMON mint is verified.
    // The first broadcast response is dropped: the operation must stay unknown, then be
    // completed by observation with exactly one onchain execution.
    let wmon = deployment["wmon"].as_str().unwrap().to_owned();
    let marked = client
        .post(format!("{proxy_url}/drop-next"))
        .send()
        .await
        .unwrap();
    assert!(marked.status().is_success());
    let lost = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "wrap",
            json!({"amount":"5","operation_ref":"a1".repeat(32)}),
        ),
    ));
    assert_eq!(
        lost.2, 2,
        "a lost response must be pending/unknown: {lost:?}"
    );
    assert_eq!(lost.1["status"], "pending");
    let lost_sync = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        operation_completed(value, "wrap")
    })
    .await;
    assert!(
        lost_sync.0,
        "lost-response wrap never completed: {lost_sync:?}"
    );
    assert_eq!(
        wait_for_balance(&client, &url, &wmon, &signer_hex, 5).await,
        5
    );
    let wrapped = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "wrap",
            json!({"amount":"5","operation_ref":"a2".repeat(32)}),
        ),
    ));
    assert!(
        wrapped.0,
        "second wrap was refused: {} (exit {})",
        wrapped.1, wrapped.2
    );
    assert_eq!(
        wait_for_balance(&client, &url, &wmon, &signer_hex, 10).await,
        10
    );

    // 6b. A same-block unrelated transfer must not defeat completion: the wrapper mint event
    // authenticates the call, while the parent/including balance delta is only supplementary.
    let wmon_address: [u8; 20] = hex::decode(wmon.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();
    let transfer_recipient: [u8; 20] = [0x5a; 20];
    // Release any finalized signer-lane claim before the mempool-parked wrap.
    let released = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        value["result"]["nonce_released"] == true
    })
    .await;
    assert!(released.0, "signer lane was not released: {released:?}");
    rpc(&client, &url, "anvil_setAutomine", json!([false])).await;
    rpc(&client, &url, "anvil_setIntervalMining", json!([0])).await;
    let same_block = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "wrap",
            json!({"amount":"5","operation_ref":"f1".repeat(32)}),
        ),
    ));
    assert!(
        same_block.0,
        "same-block wrap was not submitted: {} (exit {})",
        same_block.1, same_block.2
    );
    // Sign an unrelated WMON transfer with the next nonce while the wrap sits in the mempool.
    let pending_nonce = rpc(
        &client,
        &url,
        "eth_getTransactionCount",
        json!([signer_hex, "pending"]),
    )
    .await["result"]
        .as_str()
        .unwrap()
        .trim_start_matches("0x")
        .to_owned();
    let nonce = u64::from_str_radix(&pending_nonce, 16).unwrap();
    let key = TransactionKey::from_bytes(&transaction_key).unwrap();
    let mut transfer_data = vec![0xa9, 0x05, 0x9c, 0xbb];
    transfer_data.extend_from_slice(&word(&transfer_recipient));
    transfer_data.extend_from_slice(&word(&5u128.to_be_bytes()));
    let plan = erebus_evm::chain::SigningPlan::new(
        signer,
        erebus_evm::chain::TransactionParams {
            nonce,
            fees: erebus_evm::chain::Eip1559Fees::new(1_000_000_000, 100_000_000).unwrap(),
            gas_limit: 200_000,
            value: 0,
        },
    )
    .unwrap();
    let transfer_raw = plan
        .sign_call(31337, wmon_address, transfer_data.clone(), &key)
        .unwrap()
        .raw()
        .to_vec();
    let sent = rpc(
        &client,
        &url,
        "eth_sendRawTransaction",
        json!([format!("0x{}", hex::encode(&transfer_raw))]),
    )
    .await;
    assert!(
        sent["result"].is_string(),
        "unrelated transfer failed: {sent}"
    );
    rpc(&client, &url, "anvil_mine", json!(["0x1"])).await;
    rpc(&client, &url, "anvil_setAutomine", json!([true])).await;
    rpc(&client, &url, "anvil_setIntervalMining", json!([1])).await;
    let same_block_identity = same_block.1["result"]["identity"]
        .as_str()
        .unwrap()
        .to_owned();
    let same_block_sync = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        operation_identity_completed(value, &same_block_identity)
    })
    .await;
    assert!(
        same_block_sync.0,
        "same-block wrap never completed: {same_block_sync:?}"
    );
    assert_eq!(
        wait_for_balance(&client, &url, &wmon, &signer_hex, 10).await,
        10
    );
    assert_eq!(
        wait_for_balance(
            &client,
            &url,
            &wmon,
            &format!("0x{}", hex::encode(transfer_recipient)),
            5
        )
        .await,
        5
    );

    // 7. A wrapper that fails the native payout reverts the whole unwrap; the burn rolls back
    // and only the finalized receipt may return the operation to a retryable plan.
    let enable = encode_call(&FAIL_PAYOUT_SELECTOR, &[word(&1u128.to_be_bytes())]);
    let fee = rpc(
        &client,
        &url,
        "eth_sendTransaction",
        json!([{"from": ANVIL_ACCOUNT, "to": wmon, "data": enable, "gas": "0x100000"}]),
    )
    .await;
    assert!(fee["result"].is_string(), "setFailPayout failed: {fee}");
    mine(&client, &url).await;
    let failed_unwrap = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "unwrap",
            json!({"amount":"5","operation_ref":"b1".repeat(32)}),
        ),
    ));
    assert!(
        failed_unwrap.0,
        "failed-payout unwrap was not submitted: {} (exit {})",
        failed_unwrap.1, failed_unwrap.2
    );
    let failed_sync = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        operation_reverted(value, "unwrap")
    })
    .await;
    assert!(
        failed_sync.0,
        "payout revert never reconciled: {failed_sync:?}"
    );
    assert_eq!(
        token_balance(&client, &url, &wmon, &signer_hex).await,
        10,
        "a reverted payout must leave the WMON balance unchanged"
    );

    // 8. Clear the payout failure; the retry completes the same operation and burns the WMON.
    let disable = encode_call(&FAIL_PAYOUT_SELECTOR, &[word(&0u128.to_be_bytes())]);
    let fee = rpc(
        &client,
        &url,
        "eth_sendTransaction",
        json!([{"from": ANVIL_ACCOUNT, "to": wmon, "data": disable, "gas": "0x100000"}]),
    )
    .await;
    assert!(fee["result"].is_string(), "setFailPayout failed: {fee}");
    mine(&client, &url).await;
    let unwrapped = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "unwrap",
            json!({"amount":"5","operation_ref":"b1".repeat(32)}),
        ),
    ));
    assert!(
        unwrapped.0,
        "unwrap retry failed: {} (exit {})",
        unwrapped.1, unwrapped.2
    );
    let unwrapped_done = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        operation_completed(value, "unwrap")
    })
    .await;
    assert!(
        unwrapped_done.0,
        "unwrap never completed: {unwrapped_done:?}"
    );
    assert_eq!(token_balance(&client, &url, &wmon, &signer_hex).await, 5);

    // 9. Unwrapping more than the wrapper balance reverts and stays retryable.
    let too_much = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "unwrap",
            json!({"amount":"6","operation_ref":"c1".repeat(32)}),
        ),
    ));
    assert!(
        too_much.0,
        "over-unwrap was not submitted: {} (exit {})",
        too_much.1, too_much.2
    );
    let too_much_sync = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        operation_reverted(value, "unwrap")
    })
    .await;
    assert!(
        too_much_sync.0,
        "over-unwrap never reconciled: {too_much_sync:?}"
    );
    assert_eq!(token_balance(&client, &url, &wmon, &signer_hex).await, 5);

    // 10. A wrong wrapper target reverts on unknown calldata and never completes.
    let bad_config = root.path().join("wallet-wrong-wrapper.json");
    private(
        &bad_config,
        json!({
            "version": 1, "state_root": state_root, "namespace": "eip155:31337",
            "pool": pool, "verifier_version": 2, "asset": token, "wrapper": pool,
            "rpc_url": proxy_url, "peer_rpc_url": peer_url, "first_block": first_block, "first_hash": first_hash,
            "wallet_file": state_root.join("wallet/notes.enc"), "wallet_key_file": wallet_key_file,
            "agreement_key_file": agreement_key_file, "signer_address": signer_hex,
            "signer_journal_root": state_root.join("signer-journal"), "transaction_key_file": transaction_key_file,
            "artifact_manifest_file": manifest_path, "artifact_manifest_sha256": manifest_digest,
            "artifact_cache_root": state_root.join("artifacts"), "allow_test_artifacts": true,
            "allow_loopback_artifact_http": true, "gas_limit": 2_000_000u64,
            "max_fee_per_gas": "1000000000", "max_priority_fee_per_gas": "100000000",
            "timeout_seconds": 30, "max_scan_blocks": 1000, "artifact_timeout_seconds": 120,
        })
        .to_string()
        .as_bytes(),
    );
    let wrong = finish(spawn(
        &wallet_bin,
        root.path(),
        json!({"method":"wrap","config_file":bad_config,"amount":"1","operation_ref":"d1".repeat(32)}),
    ));
    assert!(
        wrong.0,
        "wrong-wrapper wrap was not submitted: {} (exit {})",
        wrong.1, wrong.2
    );
    let wrong_sync = sync_until(&wallet_bin, root.path(), &bad_config, |value| {
        operation_reverted(value, "wrap")
    })
    .await;
    assert!(
        wrong_sync.0,
        "wrong-wrapper wrap never reconciled: {wrong_sync:?}"
    );

    // 10b. A config pointing at a foreign pool fails closed before any operation is created.
    let foreign_config = root.path().join("wallet-foreign-pool.json");
    private(
        &foreign_config,
        json!({
            "version": 1, "state_root": state_root, "namespace": "eip155:31337",
            "pool": "0x00000000000000000000000000000000000000ff", "verifier_version": 2,
            "asset": token, "wrapper": wmon,
            "rpc_url": proxy_url, "peer_rpc_url": peer_url, "first_block": first_block, "first_hash": first_hash,
            "wallet_file": state_root.join("wallet/notes.enc"), "wallet_key_file": wallet_key_file,
            "agreement_key_file": agreement_key_file, "signer_address": signer_hex,
            "signer_journal_root": state_root.join("signer-journal"), "transaction_key_file": transaction_key_file,
            "artifact_manifest_file": manifest_path, "artifact_manifest_sha256": manifest_digest,
            "artifact_cache_root": state_root.join("artifacts"), "allow_test_artifacts": true,
            "allow_loopback_artifact_http": true, "gas_limit": 2_000_000u64,
            "max_fee_per_gas": "1000000000", "max_priority_fee_per_gas": "100000000",
            "timeout_seconds": 30, "max_scan_blocks": 1000, "artifact_timeout_seconds": 120,
        })
        .to_string()
        .as_bytes(),
    );
    let foreign = finish(spawn(
        &wallet_bin,
        root.path(),
        json!({"method":"wrap","config_file":foreign_config,"amount":"1","operation_ref":"aa".repeat(32)}),
    ));
    assert!(
        !foreign.0 && foreign.2 == 1,
        "foreign pool was not refused: {foreign:?}"
    );

    // 10c. A corrupted public index fails closed and recovers by rebuilding from the chain.
    let primary_index = state_root.join("wallet-index/primary.json");
    let saved_index = fs::read(&primary_index).unwrap();
    fs::write(&primary_index, b"not-json").unwrap();
    let corrupt = finish(spawn(
        &wallet_bin,
        root.path(),
        json!({"method":"sync","config_file":config_path}),
    ));
    assert!(
        !corrupt.0 && corrupt.2 == 1,
        "corrupt index was not refused: {corrupt:?}"
    );
    fs::write(&primary_index, &saved_index).unwrap();
    let recovered = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        value["result"]["notes"].as_array().is_some()
    })
    .await;
    assert!(recovered.0, "index recovery failed: {recovered:?}");

    // 10d. Restore stale encrypted state with new RPC endpoints and no retained cache.
    // Recovery must rediscover consumption, not resurrect the withdrawn note or send funds.
    let retained_wallet = state_root.join("wallet/notes-retained.enc");
    fs::rename(state_root.join("wallet/notes.enc"), &retained_wallet).unwrap();
    private(&state_root.join("wallet/notes.enc"), &wallet_backup);
    fs::rename(
        state_root.join("wallet-index"),
        state_root.join("wallet-index-retained"),
    )
    .unwrap();
    let switched_config = root.path().join("wallet-restored.json");
    let mut recovery_config: Value =
        serde_json::from_slice(&fs::read(&config_path).unwrap()).unwrap();
    let (restored_rpc, primary_sends) = lossy_proxy_with_send_count(url.clone()).await;
    let (restored_peer_rpc, peer_sends) = lossy_proxy_with_send_count(url.clone()).await;
    recovery_config["rpc_url"] = json!(restored_rpc);
    recovery_config["peer_rpc_url"] = json!(restored_peer_rpc);
    recovery_config["transaction_key_file"] = Value::Null;
    recovery_config["artifact_manifest_file"] = json!(root.path().join("absent-manifest.json"));
    recovery_config["artifact_cache_root"] = json!(root.path().join("absent-artifacts"));
    private(
        &switched_config,
        &serde_json::to_vec(&recovery_config).unwrap(),
    );
    let before_recovery = rpc(
        &client,
        &url,
        "eth_getTransactionCount",
        json!([signer_hex, "pending"]),
    )
    .await;
    let restored = sync_until(&wallet_bin, root.path(), &switched_config, |value| {
        value["result"]["notes"].as_array().is_some_and(|notes| {
            notes.len() == 2
                && notes.iter().any(|note| {
                    note["commitment"] == first_commitment
                        && note["included"] == true
                        && note["consumed"] == true
                })
                && notes.iter().any(|note| {
                    note["commitment"] == second_commitment
                        && note["included"] == true
                        && note["consumed"] == false
                        && note["reserved"].is_null()
                })
        })
    })
    .await;
    assert!(restored.0, "stale backup recovery failed: {restored:?}");
    assert!(state_root.join("wallet-index/primary.json").is_file());
    assert!(state_root.join("wallet-index/peer.json").is_file());
    assert!(!root.path().join("absent-artifacts").exists());
    let after_recovery = rpc(
        &client,
        &url,
        "eth_getTransactionCount",
        json!([signer_hex, "pending"]),
    )
    .await;
    assert_eq!(before_recovery["result"], after_recovery["result"]);
    assert_eq!(primary_sends.load(Ordering::SeqCst), 0);
    assert_eq!(peer_sends.load(Ordering::SeqCst), 0);
    assert_eq!(token_balance(&client, &url, &token, &signer_hex).await, 80);
    assert_eq!(
        token_balance(
            &client,
            &url,
            &token,
            &format!("0x{}", hex::encode(recipient))
        )
        .await,
        10
    );

    // 11. Insufficient native funds is refused before any transaction is built.
    let poor = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "wrap",
            json!({"amount":"1000000000000000000000000000","operation_ref":"e1".repeat(32)}),
        ),
    ));
    assert!(
        !poor.0 && poor.2 == 1,
        "insufficient funds was not refused: {poor:?}"
    );
    drop(anvil);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires generated M5 prototype artifacts and Anvil; not live WMON qualification"]
async fn installed_wallet_wraps_deposits_withdraws_and_unwraps_native_funds() {
    const WRAPPED: u128 = 20_000_000_000_000_000;
    const PRIVATE_AMOUNT: u128 = WRAPPED / 2;
    const INITIAL_NATIVE: u128 = 100_000_000_000_000_000_000;
    let port = free_port();
    let _anvil = ServiceProcess(
        Command::new("anvil")
            .args([
                "--port",
                &port.to_string(),
                "--chain-id",
                "31337",
                "--slots-in-an-epoch",
                "1",
                "--block-time",
                "1",
                "--silent",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let url = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    wait_for_chain(&client, &url).await;
    let fixture_url = url.clone();
    let deployment: Value = tokio::task::spawn_blocking(move || {
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../circuits/m5/scripts/native-fixture.mjs");
        let output = Command::new("node")
            .arg(script)
            .arg(fixture_url)
            .arg("wmon")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "WMON fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    })
    .await
    .unwrap();
    assert_eq!(deployment["asset_mode"], "wmon");
    assert_eq!(deployment["token"], deployment["wmon"]);
    let token = deployment["token"].as_str().unwrap();
    let pool = deployment["pool"].as_str().unwrap();
    let transaction_key = random_bytes::<32>();
    let signer = TransactionKey::from_bytes(&transaction_key)
        .unwrap()
        .address();
    let signer_hex = format!("0x{}", hex::encode(signer));
    let funded = rpc(
        &client,
        &url,
        "anvil_setBalance",
        json!([signer_hex, format!("0x{INITIAL_NATIVE:x}")]),
    )
    .await;
    assert!(funded.get("error").is_none());
    let root = tempfile::tempdir().unwrap();
    let state_root = root.path().join("state");
    fs::create_dir(&state_root).unwrap();
    let transaction_key_file = root.path().join("transaction.key");
    let wallet_key_file = root.path().join("wallet.key");
    let agreement_key_file = root.path().join("agreement.key");
    private(&transaction_key_file, &transaction_key);
    private(&wallet_key_file, &random_bytes::<32>());
    private(
        &agreement_key_file,
        &derive_key(&random_bytes::<32>()).unwrap(),
    );
    let manifest = serde_json::to_vec(&fixture_manifest(&deployment).await).unwrap();
    let manifest_path = root.path().join("manifest.json");
    private(&manifest_path, &manifest);
    let (proxy_url, sends) = lossy_proxy_with_send_count(url.clone()).await;
    let config_path = root.path().join("wallet.json");
    private(&config_path, &serde_json::to_vec(&json!({
        "version": 1, "state_root": state_root, "namespace": "eip155:31337",
        "pool": pool, "verifier_version": 2, "asset": token, "wrapper": token,
        "rpc_url": proxy_url, "peer_rpc_url": url,
        "first_block": deployment["first_block"], "first_hash": deployment["first_hash"],
        "wallet_file": state_root.join("wallet/notes.enc"),
        "wallet_key_file": wallet_key_file, "agreement_key_file": agreement_key_file,
        "signer_address": signer_hex, "signer_journal_root": state_root.join("signer-journal"),
        "transaction_key_file": transaction_key_file,
        "artifact_manifest_file": manifest_path, "artifact_manifest_sha256": hex::encode(Sha256::digest(&manifest)),
        "artifact_cache_root": state_root.join("artifacts"), "allow_test_artifacts": true,
        "allow_loopback_artifact_http": true, "gas_limit": 2_000_000u64,
        "max_fee_per_gas": "1000000000", "max_priority_fee_per_gas": "100000000",
        "timeout_seconds": 30, "max_scan_blocks": 1000, "artifact_timeout_seconds": 120,
    })).unwrap());
    let wallet_bin = native_binary("erebus-wallet", env!("CARGO_BIN_EXE_erebus-wallet"));
    let request = |method: &str, extra: Value| {
        let mut value = json!({"method": method, "config_file": config_path});
        for (key, entry) in extra.as_object().unwrap() {
            value_insert(&mut value, key, entry.clone());
        }
        value
    };
    assert_eq!(token_balance(&client, &url, token, &signer_hex).await, 0);
    assert_eq!(token_balance(&client, &url, token, pool).await, 150);
    let wrapped = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "wrap",
            json!({
                "amount": WRAPPED.to_string(), "operation_ref": "71".repeat(32),
            }),
        ),
    ));
    assert!(wrapped.0, "wrap failed: {wrapped:?}");
    let observed = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        operation_completed(value, "wrap")
    })
    .await;
    assert!(observed.0, "wrap did not finalize: {observed:?}");
    assert_eq!(
        token_balance(&client, &url, token, &signer_hex).await,
        WRAPPED
    );

    let approved = finish(spawn(
        &wallet_bin,
        root.path(),
        request("approve", json!({"amount": PRIVATE_AMOUNT.to_string()})),
    ));
    assert!(approved.0, "approve failed: {approved:?}");
    let observed = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        operation_completed(value, "approve")
    })
    .await;
    assert!(observed.0, "approval did not finalize: {observed:?}");
    let deposited = finish(spawn(
        &wallet_bin,
        root.path(),
        request("deposit", json!({"amount": PRIVATE_AMOUNT.to_string()})),
    ));
    assert!(deposited.0, "WMON deposit failed: {deposited:?}");
    let commitment = deposited.1["result"]["commitment"]
        .as_str()
        .unwrap()
        .to_owned();
    let observed = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        value["result"]["notes"].as_array().is_some_and(|notes| {
            notes.len() == 1
                && notes[0]["commitment"] == commitment
                && notes[0]["included"] == true
                && notes[0]["consumed"] == false
        })
    })
    .await;
    assert!(observed.0, "WMON note recovery failed: {observed:?}");
    assert_eq!(
        token_balance(&client, &url, token, &signer_hex).await,
        WRAPPED - PRIVATE_AMOUNT
    );
    assert_eq!(
        token_balance(&client, &url, token, pool).await,
        150 + PRIVATE_AMOUNT
    );

    let withdrawn = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "withdraw",
            json!({
                "commitment": commitment, "recipient": signer_hex,
            }),
        ),
    ));
    assert!(withdrawn.0, "WMON withdrawal failed: {withdrawn:?}");
    let observed = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        value["result"]["notes"].as_array().is_some_and(|notes| {
            notes.len() == 1 && notes[0]["commitment"] == commitment && notes[0]["consumed"] == true
        })
    })
    .await;
    assert!(observed.0, "WMON consumption not recovered: {observed:?}");
    assert_eq!(
        token_balance(&client, &url, token, &signer_hex).await,
        WRAPPED
    );
    assert_eq!(token_balance(&client, &url, token, pool).await, 150);
    let unwrapped = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "unwrap",
            json!({
                "amount": WRAPPED.to_string(), "operation_ref": "72".repeat(32),
            }),
        ),
    ));
    assert!(unwrapped.0, "unwrap failed: {unwrapped:?}");
    let observed = sync_until(&wallet_bin, root.path(), &config_path, |value| {
        operation_completed(value, "unwrap")
    })
    .await;
    assert!(observed.0, "unwrap did not finalize: {observed:?}");
    assert_eq!(token_balance(&client, &url, token, &signer_hex).await, 0);
    assert_eq!(sends.load(Ordering::SeqCst), 5);

    // The wrapped native amount returns exactly; only the five transaction fees remain spent.
    let operations = observed.1["result"]["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 5);
    let mut fees = 0u128;
    for operation in operations {
        assert_eq!(operation["state"], "completed");
        let attempts = operation["attempts"].as_array().unwrap();
        let hash = attempts[0]["tx_hash"].as_str().unwrap();
        // Some operations retain both unknown and completed observations of one hash.
        assert!(attempts.iter().all(|attempt| attempt["tx_hash"] == hash));
        let tx = format!("0x{hash}");
        let receipt = rpc(&client, &url, "eth_getTransactionReceipt", json!([tx])).await;
        assert_eq!(receipt["result"]["status"], "0x1");
        let parse = |field: &str| {
            u128::from_str_radix(
                receipt["result"][field]
                    .as_str()
                    .unwrap()
                    .trim_start_matches("0x"),
                16,
            )
            .unwrap()
        };
        fees += parse("gasUsed") * parse("effectiveGasPrice");
    }
    let balance = rpc(
        &client,
        &url,
        "eth_getBalance",
        json!([signer_hex, "latest"]),
    )
    .await;
    let balance = u128::from_str_radix(
        balance["result"].as_str().unwrap().trim_start_matches("0x"),
        16,
    )
    .unwrap();
    assert_eq!(balance + fees, INITIAL_NATIVE);
    let retried = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "wrap",
            json!({
                "amount": WRAPPED.to_string(), "operation_ref": "71".repeat(32),
            }),
        ),
    ));
    assert!(retried.0, "completed wrap retry failed: {retried:?}");
    let retried = finish(spawn(
        &wallet_bin,
        root.path(),
        request(
            "unwrap",
            json!({
                "amount": WRAPPED.to_string(), "operation_ref": "72".repeat(32),
            }),
        ),
    ));
    assert!(retried.0, "completed unwrap retry failed: {retried:?}");
    assert_eq!(sends.load(Ordering::SeqCst), 5);
    assert_eq!(token_balance(&client, &url, token, &signer_hex).await, 0);
}

fn operation_identity_completed(value: &Value, identity: &str) -> bool {
    value["result"]["operations"]
        .as_array()
        .is_some_and(|operations| {
            operations.iter().any(|operation| {
                operation["identity"] == identity && operation["state"] == "completed"
            })
        })
}

fn operation_completed(value: &Value, kind: &str) -> bool {
    value["result"]["operations"]
        .as_array()
        .is_some_and(|operations| {
            operations
                .iter()
                .any(|operation| operation["kind"] == kind && operation["state"] == "completed")
        })
}

fn operation_reverted(value: &Value, kind: &str) -> bool {
    value["result"]["operations"]
        .as_array()
        .is_some_and(|operations| {
            operations.iter().any(|operation| {
                operation["kind"] == kind
                    && operation["state"] == "planned"
                    && operation["retained"] == false
                    && operation["attempts"].as_array().is_some_and(|attempts| {
                        attempts
                            .last()
                            .is_some_and(|last| last["outcome"] == "reverted")
                    })
            })
        })
}

fn value_insert(target: &mut Value, key: &str, value: Value) {
    if let Some(object) = target.as_object_mut() {
        object.insert(key.to_owned(), value);
    }
}

/// Runs sync until `predicate` holds or the deadline passes; a failing exit is returned.
async fn sync_until(
    binary: &Path,
    cwd: &Path,
    config: &Path,
    predicate: impl Fn(&Value) -> bool,
) -> (bool, Value, i32) {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let binary = binary.to_path_buf();
        let cwd = cwd.to_path_buf();
        let request = json!({"method":"sync","config_file":config});
        let result = tokio::task::spawn_blocking(move || finish(spawn(&binary, &cwd, request)))
            .await
            .unwrap();
        if result.0 && predicate(&result.1) {
            return result;
        }
        if Instant::now() >= deadline {
            // The predicate never held; never report this as a satisfied sync.
            return (false, result.1, result.2);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
