//! Chain-level durable-boundary fault sweeps for explicit wallet operations.
//!
//! In-process stores with a deterministic fault hook stop the operation at each operation-
//! journal write boundary. The test then reopens the stores (restart), recovers by observation
//! or explicit resume, and requires exactly one successful onchain execution per operation.

use std::{
    collections::HashMap,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
    time::Duration,
};

use axum::{extract::State, Router};
use erebus_core::ids::{AddressBytes, ChainNamespace};
use erebus_evm::{
    chain::{Eip1559Fees, EvmChain, SignerJournal, TransactionKey},
    deployment::EvmDeployment,
};
use erebus_journal::{Boundary, FaultHook};
use erebus_shielded_prover::{
    artifacts::{ArtifactRelease, InstallOptions},
    index_store::{IndexDomain, IndexStore},
    operations::{
        prepare_approve, prepare_deposit, prepare_unwrap, prepare_withdraw, prepare_wrap,
        reconcile_finalized_operations, reconcile_operations, replace_wallet_call,
        resume_wallet_call, submit_wallet_call, OperationContext, OperationError, OperationStore,
        WalletOperationKind, WalletOperationState,
    },
    recovery::recover_finalized_wallet,
    rpc::PoolRpc,
    wallet::{WalletDomain, WalletStore},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const ANVIL_ACCOUNT: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";
const MINT_SELECTOR: [u8; 4] = [0x40, 0xc1, 0x0f, 0x19];
const ALLOWANCE_SELECTOR: [u8; 4] = [0xdd, 0x62, 0xed, 0x3e];
const APPROVE_SELECTOR: [u8; 4] = [0x09, 0x5e, 0xa7, 0xb3];
const BALANCE_SELECTOR: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];

struct Service(Child);
impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Stops the caller at one operation-journal write boundary.
struct FaultOnce {
    fail_at: usize,
    count: AtomicUsize,
}

impl FaultHook for FaultOnce {
    fn after(&self, _boundary: Boundary<'_>) -> std::io::Result<()> {
        if self.count.fetch_add(1, Ordering::SeqCst) + 1 == self.fail_at {
            Err(std::io::Error::other("injected durable boundary failure"))
        } else {
            Ok(())
        }
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
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

async fn call_u128(client: &reqwest::Client, url: &str, to: &str, data: String) -> u128 {
    let response = rpc(
        client,
        url,
        "eth_call",
        json!([{"to": to, "data": data}, "latest"]),
    )
    .await;
    let text = response["result"]
        .as_str()
        .unwrap()
        .trim_start_matches("0x");
    let bytes = hex::decode(text).unwrap();
    u128::from_be_bytes(bytes[16..].try_into().unwrap())
}

async fn artifact_server(
    files: HashMap<String, Vec<u8>>,
) -> (String, Arc<HashMap<String, Vec<u8>>>) {
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
    (format!("http://{address}"), bodies)
}

/// A JSON-RPC forwarder that can pin the reported `finalized` block behind the tip.
///
/// Anvil reaches immediate finality, so a real unfinalized tip cannot be produced. This
/// harness controls only the RPC's finality view; it does not reverse finalized consensus
/// state. The chain itself still performs a real tip reorg via `anvil_reorg`.
#[derive(Clone)]
struct FinalityState {
    upstream: String,
    client: reqwest::Client,
    pinned: Arc<std::sync::Mutex<Option<Value>>>,
    hidden: Arc<std::sync::Mutex<Option<String>>>,
    no_finalized: Arc<std::sync::atomic::AtomicBool>,
    drop_next_send: Arc<std::sync::atomic::AtomicBool>,
    lose_next_send: Arc<std::sync::atomic::AtomicBool>,
}

async fn finality_proxy(upstream: String) -> String {
    use axum::{extract::State as AxumState, response::IntoResponse, routing::post as axum_post};
    async fn forward(
        AxumState(state): AxumState<FinalityState>,
        body: String,
    ) -> axum::response::Response {
        let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        if parsed.is_object() {
            let response = forward_one(&state, parsed).await;
            return axum::Json(response).into_response();
        }
        let mut responses = Vec::new();
        for request in parsed.as_array().cloned().unwrap_or_default() {
            responses.push(forward_one(&state, request).await);
        }
        axum::Json(Value::Array(responses)).into_response()
    }
    async fn pin(AxumState(state): AxumState<FinalityState>) -> &'static str {
        // Pin the parent of the latest block, which is necessarily behind the tip.
        let parent = match upstream_block(&state, "latest").await {
            Some(block) => {
                let hash = block["parentHash"].clone();
                let body = json!({"jsonrpc":"2.0","id":1,"method":"eth_getBlockByHash","params":[hash, false]});
                let mut result = None;
                if let Ok(response) = state
                    .client
                    .post(&state.upstream)
                    .header("content-type", "application/json")
                    .body(body.to_string())
                    .send()
                    .await
                {
                    if let Ok(text) = response.text().await {
                        if let Ok(value) = serde_json::from_str::<Value>(&text) {
                            result = value.get("result").cloned();
                        }
                    }
                }
                result
            }
            None => None,
        };
        *state.pinned.lock().unwrap() = parent;
        "ok"
    }

    async fn unpin(AxumState(state): AxumState<FinalityState>) -> &'static str {
        *state.pinned.lock().unwrap() = None;
        "ok"
    }
    async fn hide(
        AxumState(state): AxumState<FinalityState>,
        axum::Json(body): axum::Json<Value>,
    ) -> &'static str {
        *state.hidden.lock().unwrap() = body["hash"].as_str().map(|hash| hash.to_ascii_lowercase());
        "ok"
    }
    async fn show(AxumState(state): AxumState<FinalityState>) -> &'static str {
        *state.hidden.lock().unwrap() = None;
        "ok"
    }
    async fn disable_finalized(AxumState(state): AxumState<FinalityState>) -> &'static str {
        state
            .no_finalized
            .store(true, std::sync::atomic::Ordering::SeqCst);
        "ok"
    }
    async fn enable_finalized(AxumState(state): AxumState<FinalityState>) -> &'static str {
        state
            .no_finalized
            .store(false, std::sync::atomic::Ordering::SeqCst);
        "ok"
    }
    async fn drop_next_send(AxumState(state): AxumState<FinalityState>) -> &'static str {
        state
            .drop_next_send
            .store(true, std::sync::atomic::Ordering::SeqCst);
        "ok"
    }
    async fn lose_next_send(AxumState(state): AxumState<FinalityState>) -> &'static str {
        state
            .lose_next_send
            .store(true, std::sync::atomic::Ordering::SeqCst);
        "ok"
    }
    async fn upstream_block(state: &FinalityState, tag: &str) -> Option<Value> {
        let body =
            json!({"jsonrpc":"2.0","id":1,"method":"eth_getBlockByNumber","params":[tag, false]});
        let text = state
            .client
            .post(&state.upstream)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            .ok()?
            .text()
            .await
            .ok()?;
        serde_json::from_str::<Value>(&text)
            .ok()?
            .get("result")
            .cloned()
    }
    async fn forward_one(state: &FinalityState, request: Value) -> Value {
        let method = request["method"].as_str().unwrap_or_default().to_owned();
        if method == "eth_getBlockByNumber" && request["params"][0] == "finalized" {
            if state.no_finalized.load(std::sync::atomic::Ordering::SeqCst) {
                // An RPC without finalized-tag support: explicit null, never a fallback.
                return json!({"jsonrpc":"2.0","id":request["id"].clone(),"result":null});
            }
            if let Some(block) = state.pinned.lock().unwrap().clone() {
                return json!({"jsonrpc":"2.0","id":request["id"].clone(),"result":block});
            }
        }
        if method == "eth_sendRawTransaction"
            && state
                .drop_next_send
                .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            // The request never reaches the node: it is attempted but cannot be included.
            return json!({"jsonrpc":"2.0","id":request["id"].clone(),
                "error":{"code":-32603,"message":"injected send failure"}});
        }
        let lose_response = method == "eth_sendRawTransaction"
            && state
                .lose_next_send
                .swap(false, std::sync::atomic::Ordering::SeqCst);
        let mut response: Value = match state
            .client
            .post(&state.upstream)
            .header("content-type", "application/json")
            .body(request.to_string())
            .send()
            .await
        {
            Ok(upstream) => upstream.json::<Value>().await.unwrap_or_else(|_| {
                json!({"jsonrpc":"2.0","id":request["id"].clone(),"error":{"code":-32000,"message":"proxy"}})
            }),
            Err(_) => json!({"jsonrpc":"2.0","id":request["id"].clone(),"error":{"code":-32000,"message":"proxy"}}),
        };
        if lose_response {
            // The transaction reached the node while its response was lost: the attempt is
            // durable and may still land.
            return json!({"jsonrpc":"2.0","id":request["id"].clone(),
                "error":{"code":-32603,"message":"lost broadcast response"}});
        }
        let hidden = state.hidden.lock().unwrap().clone();
        if let Some(hidden) = hidden {
            let target = request["params"][0]
                .as_str()
                .map(|hash| hash.to_ascii_lowercase());
            if target.as_deref() == Some(hidden.as_str()) {
                if method == "eth_getTransactionReceipt" {
                    response["result"] = Value::Null;
                } else if method == "eth_getTransactionByHash" {
                    if let Some(result) = response.get_mut("result") {
                        if let Some(object) = result.as_object_mut() {
                            object.remove("blockHash");
                            object.remove("blockNumber");
                            object.remove("transactionIndex");
                        }
                    }
                }
            }
        }
        response
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let state = FinalityState {
        upstream,
        client: reqwest::Client::new(),
        pinned: Arc::new(std::sync::Mutex::new(None)),
        hidden: Arc::new(std::sync::Mutex::new(None)),
        no_finalized: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        drop_next_send: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        lose_next_send: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/", axum_post(forward))
                .route("/pin-earlier", axum_post(pin))
                .route("/unpin", axum_post(unpin))
                .route("/hide", axum_post(hide))
                .route("/show", axum_post(show))
                .route("/disable-finalized", axum_post(disable_finalized))
                .route("/enable-finalized", axum_post(enable_finalized))
                .route("/drop-next-send", axum_post(drop_next_send))
                .route("/lose-next-send", axum_post(lose_next_send))
                .with_state(state),
        )
        .await
        .unwrap();
    });
    format!("http://{address}")
}

struct Env {
    url: String,
    client: reqwest::Client,
    pool: [u8; 20],
    token: [u8; 20],
    token_hex: String,
    wmon: [u8; 20],
    wmon_hex: String,
    first_block: u64,
    first_hash: [u8; 32],
    chain: EvmChain,
    key: TransactionKey,
    signer: [u8; 20],
    fees: Eip1559Fees,
    release: ArtifactRelease,
    options: InstallOptions,
    _anvil: Service,
}

async fn env(root: &Path) -> Env {
    let port = free_port();
    let anvil = Service(
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
    for _ in 0..200 {
        if client
            .post(&url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}))
            .send()
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
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
        "fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let deployment: Value = serde_json::from_slice(&output.stdout).unwrap();
    let pool_hex = deployment["pool"].as_str().unwrap().to_owned();
    let token_hex = deployment["token"].as_str().unwrap().to_owned();
    let wmon_hex = deployment["wmon"].as_str().unwrap().to_owned();
    let pool: [u8; 20] = hex::decode(pool_hex.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();
    let token: [u8; 20] = hex::decode(token_hex.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();
    let wmon: [u8; 20] = hex::decode(wmon_hex.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();
    let first_block = deployment["first_block"].as_u64().unwrap();
    let first_hash: [u8; 32] = hex::decode(
        deployment["first_hash"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
    )
    .unwrap()
    .try_into()
    .unwrap();

    let key = TransactionKey::from_bytes(&[0x33; 32]).unwrap();
    let signer = key.address();
    let signer_hex = format!("0x{}", hex::encode(signer));
    rpc(
        &client,
        &url,
        "anvil_setBalance",
        json!([signer_hex, "0x56bc75e2d63100000"]),
    )
    .await;
    // Fund the token signer and grant the pool allowance for deposit sweeps.
    let mint = encode_call(
        &MINT_SELECTOR,
        &[word(&signer), word(&1_000u128.to_be_bytes())],
    );
    let sent = rpc(
        &client,
        &url,
        "eth_sendTransaction",
        json!([{"from": ANVIL_ACCOUNT, "to": token_hex, "data": mint, "gas": "0x100000"}]),
    )
    .await;
    assert!(sent["result"].is_string());
    let approve = encode_call(
        &APPROVE_SELECTOR,
        &[word(&pool), word(&1_000u128.to_be_bytes())],
    );
    rpc(
        &client,
        &url,
        "anvil_impersonateAccount",
        json!([signer_hex]),
    )
    .await;
    let sent = rpc(
        &client,
        &url,
        "eth_sendTransaction",
        json!([{"from": signer_hex, "to": token_hex, "data": approve, "gas": "0x100000"}]),
    )
    .await;
    assert!(sent["result"].is_string());
    rpc(&client, &url, "anvil_mine", json!(["0x2"])).await;

    // Serve generated local artifacts and authenticate their manifest for the pool.
    let build = PathBuf::from(deployment["artifact_build"].as_str().unwrap());
    let mut files = HashMap::new();
    for kind in ["deposit", "transfer", "withdraw"] {
        for suffix in ["wasm", "r1cs", "zkey"] {
            let path = if suffix == "wasm" {
                build.join(format!("{kind}_js/{kind}.wasm"))
            } else {
                build.join(format!("{kind}.{suffix}"))
            };
            files.insert(format!("{kind}.{suffix}"), std::fs::read(path).unwrap());
        }
    }
    let (artifact_url, bodies) = artifact_server(files).await;
    let circuits: Vec<Value> = ["deposit", "transfer", "withdraw"]
        .into_iter()
        .map(|kind| {
            json!({
                "kind": kind,
                "wasm": {"url":"","sha256":"","bytes":0},
                "r1cs": {"url":"","sha256":"","bytes":0},
                "zkey": {"url":"","sha256":"","bytes":0},
            })
        })
        .collect();
    let mut manifest = json!({
        "version": 1,
        "chain_id": 31337,
        "settlement_contract": pool_hex,
        "verifier_version": 2,
        "test_only": true,
        "circuits": circuits,
    });
    for circuit in manifest["circuits"].as_array_mut().unwrap() {
        let kind = circuit["kind"].as_str().unwrap().to_owned();
        for suffix in ["wasm", "r1cs", "zkey"] {
            let name = format!("{kind}.{suffix}");
            let body = &bodies[&name];
            circuit[suffix] = json!({
                "url": format!("{artifact_url}/{name}"),
                "sha256": hex::encode(Sha256::digest(body)),
                "bytes": body.len(),
            });
        }
    }
    let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
    let manifest_path = root.join("manifest.json");
    std::fs::write(&manifest_path, &manifest_bytes).unwrap();
    let digest: [u8; 32] = Sha256::digest(&manifest_bytes).into();
    let release = ArtifactRelease::authenticate(&manifest_bytes, digest).unwrap();

    let deployment = EvmDeployment::new(
        ChainNamespace::parse("eip155:31337").unwrap(),
        pool,
        2,
        url.clone(),
    )
    .unwrap();
    let chain = EvmChain::connect(deployment, Duration::from_secs(30))
        .await
        .unwrap();
    Env {
        url,
        client,
        pool,
        token,
        token_hex,
        wmon,
        wmon_hex,
        first_block,
        first_hash,
        chain,
        key,
        signer,
        fees: Eip1559Fees::new(1_000_000_000, 100_000_000).unwrap(),
        release,
        options: InstallOptions {
            cache_root: root.join("artifacts"),
            timeout: Duration::from_secs(120),
            allow_test_artifacts: true,
            allow_loopback_http: true,
        },
        _anvil: anvil,
    }
}

struct Stores {
    operations: OperationStore,
    wallet: WalletStore,
    journal: SignerJournal,
}

fn stores(env: &Env, root: &Path, faults: Option<Arc<dyn FaultHook>>) -> Result<Stores, String> {
    let wallet_key = [0x11; 32];
    let wallet = match faults.as_ref() {
        Some(faults) => WalletStore::with_faults(
            root.join("wallet/notes.enc"),
            WalletDomain {
                chain_id: 31337,
                pool: env.pool,
                verifier_version: 2,
            },
            wallet_key,
            Arc::clone(faults),
        ),
        None => WalletStore::new(
            root.join("wallet/notes.enc"),
            WalletDomain {
                chain_id: 31337,
                pool: env.pool,
                verifier_version: 2,
            },
            wallet_key,
        ),
    }
    .map_err(|error| format!("wallet store at {root:?}: {error:?}"))?;
    let operations = match faults.as_ref() {
        Some(faults) => OperationStore::with_faults(root.join("operations"), Arc::clone(faults))
            .map_err(|_| "operation store".to_owned())?,
        None => OperationStore::open(root.join("operations"))
            .map_err(|_| "operation store".to_owned())?,
    };
    let journal = SignerJournal::open(root.join("journal"), 31337, env.signer)
        .map_err(|_| "signer journal".to_owned())?;
    Ok(Stores {
        operations,
        wallet,
        journal,
    })
}

fn context(env: &Env) -> OperationContext {
    OperationContext {
        chain_id: 31337,
        pool: env.pool,
        verifier_version: 2,
        asset: env.token,
    }
}

fn domain(env: &Env) -> erebus_core::domain::DeploymentDomain {
    let address = AddressBytes::new(env.pool.to_vec()).unwrap();
    erebus_core::domain::DeploymentDomain {
        namespace: ChainNamespace::parse("eip155:31337").unwrap(),
        settlement_contract: Some(address.clone()),
        pool: Some(address),
        verifier_version: 2,
    }
}

async fn recover_wallet(env: &Env, root: &Path, stores: &Stores) {
    let rpc = PoolRpc::new(&env.url, 31337, env.pool).unwrap();
    let index = IndexStore::new(
        root.join("index.json"),
        IndexDomain {
            chain_id: 31337,
            pool: env.pool,
            first_block: env.first_block,
            first_hash: env.first_hash,
        },
    )
    .unwrap();
    let _ = recover_finalized_wallet(&rpc, &index, &stores.wallet).await;
}

async fn sync_once(env: &Env, root: &Path, stores: &Stores) -> Result<(), OperationError> {
    recover_wallet(env, root, stores).await;
    let _ = reconcile_finalized_operations(
        &env.chain,
        &context(env),
        &stores.wallet,
        &stores.operations,
        now(),
    )
    .await?;
    let _ = reconcile_operations(&stores.wallet, &stores.operations, now())?;
    if let Ok(nonce) = env.chain.verified_finalized_nonce(env.signer).await {
        let _ = stores.journal.release(&nonce);
    }
    Ok(())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn wait_completed(env: &Env, root: &Path, stores: &Stores, identity: [u8; 32]) -> bool {
    for _ in 0..30 {
        let _ = sync_once(env, root, stores).await;
        if stores
            .operations
            .get(&identity)
            .ok()
            .flatten()
            .is_some_and(|operation| operation.state() == WalletOperationState::Completed)
        {
            return true;
        }
        let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

/// Runs one operation kind to completion with a fault injected at `fail_at`.
/// Grants a generous pool allowance from the local node account. Allowance is an explicit
/// operator precondition for deposits; the sweep only relies on it being sufficient.
async fn ensure_allowance(env: &Env) {
    let signer_hex = format!("0x{}", hex::encode(env.signer));
    let approve = encode_call(
        &APPROVE_SELECTOR,
        &[word(&env.pool), word(&1_000_000u128.to_be_bytes())],
    );
    let _ = rpc(
        &env.client,
        &env.url,
        "anvil_impersonateAccount",
        json!([signer_hex]),
    )
    .await;
    let sent = rpc(
        &env.client,
        &env.url,
        "eth_sendTransaction",
        json!([{"from": signer_hex, "to": env.token_hex, "data": approve, "gas": "0x100000"}]),
    )
    .await;
    assert!(
        sent["result"].is_string(),
        "allowance top-up failed: {sent}"
    );
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
    let allowance = call_u128(
        &env.client,
        &env.url,
        &env.token_hex,
        encode_call(&ALLOWANCE_SELECTOR, &[word(&env.signer), word(&env.pool)]),
    )
    .await;
    assert_eq!(allowance, 1_000_000, "allowance top-up not visible");
}

async fn sweep(env: &Env, kind: WalletOperationKind, fail_at: usize) -> Result<(), String> {
    let root = tempfile::tempdir().map_err(|_| "tempdir".to_owned())?;
    if matches!(
        kind,
        WalletOperationKind::Deposit | WalletOperationKind::Withdraw
    ) {
        ensure_allowance(env).await;
    }

    // Prerequisites: a confirmed deposit before a withdrawal, and wrapped balance before an
    // unwrap. These run without faults and complete before the faulted iteration starts.
    if kind == WalletOperationKind::Withdraw || kind == WalletOperationKind::Unwrap {
        let seed_kind = if kind == WalletOperationKind::Withdraw {
            WalletOperationKind::Deposit
        } else {
            WalletOperationKind::Wrap
        };
        let seed_stores = stores(env, root.path(), None)?;
        let seed = run_operation(env, root.path(), &seed_stores, seed_kind, [9; 32], false)
            .await
            .map_err(|error| format!("seed {seed_kind:?}: {error}"))?;
        let _ = sync_once(env, root.path(), &seed_stores).await;
        if !wait_completed(env, root.path(), &seed_stores, seed.identity).await {
            return Err(format!("seed {seed_kind:?} did not complete"));
        }
        drop(seed_stores);
    }

    let before = measure(env, kind).await;
    let hook = Arc::new(FaultOnce {
        fail_at,
        count: AtomicUsize::new(0),
    });
    let faults: Arc<dyn FaultHook> = hook;
    let faulted = stores(env, root.path(), Some(Arc::clone(&faults)))?;
    let op_ref = [kind_byte(kind); 32];

    // Step one: prepare and submit with the fault hook active.
    let prepared = run_operation(env, root.path(), &faulted, kind, op_ref, false).await;
    drop(faulted);

    // Restart without faults and recover.
    let stores = stores(env, root.path(), None)?;
    let identity = match prepared {
        Ok(call) => call.identity,
        Err(_) => {
            // The fault may have stopped the operation before any record existed; a rerun
            // prepares a fresh, valid operation (pending records are reused where present).
            let call = run_operation(env, root.path(), &stores, kind, op_ref, false)
                .await
                .map_err(|error| format!("restart prepare failed: {error}"))?;
            call.identity
        }
    };
    let _ = sync_once(env, root.path(), &stores).await;
    if stores
        .operations
        .get(&identity)
        .ok()
        .flatten()
        .is_some_and(|operation| operation.retained())
    {
        let _ = resume_wallet_call(&env.chain, &stores.operations, &identity, now()).await;
    }
    if !wait_completed(env, root.path(), &stores, identity).await {
        let operation = stores.operations.get(&identity).ok().flatten();
        let notes = stores
            .wallet
            .snapshot()
            .map(|snapshot| {
                snapshot
                    .notes()
                    .iter()
                    .map(|note| {
                        (
                            hex::encode(note.commitment()),
                            note.inclusion().is_some(),
                            note.consumption().is_some(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let attempts = operation
            .as_ref()
            .map(|operation| {
                operation
                    .attempts()
                    .iter()
                    .map(|attempt| {
                        format!(
                            "{:?}@{}:{}",
                            attempt.outcome,
                            attempt.block.unwrap_or(0),
                            hex::encode(attempt.tx_hash)
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        return Err(format!(
            "{kind:?} did not complete after fault {fail_at}: state={:?} retained={:?} attempts={attempts:?} notes={notes:?}",
            operation.as_ref().map(|operation| operation.state()),
            operation.as_ref().map(|operation| operation.retained()),
        ));
    }
    if !verify_one_effect(env, &stores, kind, before).await {
        return Err(format!(
            "{kind:?} did not execute exactly once after fault {fail_at}"
        ));
    }
    let operation = stores
        .operations
        .get(&identity)
        .map_err(|_| "journal".to_owned())?
        .ok_or("missing operation")?;
    let completions = operation
        .attempts()
        .iter()
        .filter(|attempt| {
            matches!(
                attempt.outcome,
                erebus_shielded_prover::operations::WalletAttemptOutcome::Completed
            )
        })
        .count();
    if completions != 1 {
        return Err(format!("{kind:?} completion attempts {completions}"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Before {
    Allowance,
    Token(u128),
    Wmon(u128),
    Recipient(u128),
}

async fn measure(env: &Env, kind: WalletOperationKind) -> Before {
    match kind {
        WalletOperationKind::Approve => Before::Allowance,
        WalletOperationKind::Wrap => {
            Before::Wmon(balance_of(env, &env.wmon_hex, &env.signer).await)
        }
        WalletOperationKind::Unwrap => {
            Before::Wmon(balance_of(env, &env.wmon_hex, &env.signer).await)
        }
        WalletOperationKind::Deposit => {
            Before::Token(balance_of(env, &env.token_hex, &env.signer).await)
        }
        WalletOperationKind::Withdraw => {
            Before::Recipient(balance_of(env, &env.token_hex, &[0x77; 20]).await)
        }
    }
}

async fn balance_of(env: &Env, token: &str, who: &[u8; 20]) -> u128 {
    call_u128(
        &env.client,
        &env.url,
        token,
        encode_call(&BALANCE_SELECTOR, &[word(who)]),
    )
    .await
}

async fn verify_one_effect(
    env: &Env,
    stores: &Stores,
    kind: WalletOperationKind,
    before: Before,
) -> bool {
    match kind {
        WalletOperationKind::Approve => {
            let allowance = call_u128(
                &env.client,
                &env.url,
                &env.token_hex,
                encode_call(&ALLOWANCE_SELECTOR, &[word(&env.signer), word(&env.pool)]),
            )
            .await;
            allowance == 7
        }
        WalletOperationKind::Wrap => match before {
            Before::Wmon(before) => {
                balance_of(env, &env.wmon_hex, &env.signer)
                    .await
                    .checked_sub(before)
                    == Some(7)
            }
            _ => false,
        },
        WalletOperationKind::Unwrap => match before {
            Before::Wmon(before) => {
                before.checked_sub(balance_of(env, &env.wmon_hex, &env.signer).await) == Some(7)
            }
            _ => false,
        },
        WalletOperationKind::Deposit => match before {
            Before::Token(before) => {
                let spent = before.checked_sub(balance_of(env, &env.token_hex, &env.signer).await)
                    == Some(7);
                let included = stores
                    .wallet
                    .snapshot()
                    .map(|snapshot| {
                        snapshot
                            .notes()
                            .iter()
                            .filter(|note| note.inclusion().is_some())
                            .count()
                    })
                    .unwrap_or(0);
                spent && included == 1
            }
            _ => false,
        },
        WalletOperationKind::Withdraw => match before {
            Before::Recipient(before) => {
                balance_of(env, &env.token_hex, &[0x77; 20])
                    .await
                    .checked_sub(before)
                    == Some(7)
            }
            _ => false,
        },
    }
}

fn kind_byte(kind: WalletOperationKind) -> u8 {
    match kind {
        WalletOperationKind::Deposit => 1,
        WalletOperationKind::Withdraw => 2,
        WalletOperationKind::Approve => 3,
        WalletOperationKind::Wrap => 4,
        WalletOperationKind::Unwrap => 5,
    }
}

async fn run_operation(
    env: &Env,
    root: &Path,
    stores: &Stores,
    kind: WalletOperationKind,
    op_ref: [u8; 32],
    _resume: bool,
) -> Result<erebus_shielded_prover::operations::PreparedWalletCall, OperationError> {
    let amount = 7u128;
    let prepared = match kind {
        WalletOperationKind::Approve => prepare_approve(
            &stores.operations,
            31337,
            env.pool,
            env.token,
            amount,
            now(),
        )?,
        WalletOperationKind::Wrap => {
            prepare_wrap(&stores.operations, 31337, env.wmon, amount, op_ref, now())?
        }
        WalletOperationKind::Unwrap => {
            prepare_unwrap(&stores.operations, 31337, env.wmon, amount, op_ref, now())?
        }
        WalletOperationKind::Deposit => {
            prepare_deposit(
                &env.release,
                &env.options,
                &stores.wallet,
                &stores.operations,
                WalletDomain {
                    chain_id: 31337,
                    pool: env.pool,
                    verifier_version: 2,
                },
                env.token,
                derive_key_fixture(),
                2,
                amount,
                now(),
            )
            .await?
        }
        WalletOperationKind::Withdraw => {
            let rpc = PoolRpc::new(&env.url, 31337, env.pool).unwrap();
            let index = IndexStore::new(
                root.join("withdraw-index.json"),
                IndexDomain {
                    chain_id: 31337,
                    pool: env.pool,
                    first_block: env.first_block,
                    first_hash: env.first_hash,
                },
            )
            .map_err(|_| OperationError::Journal)?;
            let _ = recover_finalized_wallet(&rpc, &index, &stores.wallet).await;
            let pool_index = index.load().map_err(|_| OperationError::Journal)?;
            let commitment = stores
                .wallet
                .snapshot()
                .map_err(|_| OperationError::Journal)?
                .notes()
                .iter()
                .find(|note| note.inclusion().is_some() && note.consumption().is_none())
                .map(|note| note.commitment())
                .ok_or(OperationError::Shape("no confirmed note"))?;
            prepare_withdraw(
                &env.release,
                &env.options,
                &stores.wallet,
                &stores.operations,
                &pool_index,
                WalletDomain {
                    chain_id: 31337,
                    pool: env.pool,
                    verifier_version: 2,
                },
                2,
                commitment,
                [0x77; 20],
                now(),
            )
            .await?
        }
    };
    submit_wallet_call(
        &env.chain,
        &stores.journal,
        &stores.operations,
        &domain(env),
        &prepared,
        &env.key,
        env.fees,
        2_000_000,
        now(),
        false,
    )
    .await?;
    Ok(prepared)
}

fn derive_key_fixture() -> [u8; 64] {
    erebus_core::shielded_auth::derive_key(&[0x22; 32]).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil and generated M5 prototype artifacts; local prototype setup"]
async fn durable_boundary_faults_recover_each_wallet_operation_exactly_once() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    for kind in [
        WalletOperationKind::Approve,
        WalletOperationKind::Wrap,
        WalletOperationKind::Unwrap,
        WalletOperationKind::Deposit,
        WalletOperationKind::Withdraw,
    ] {
        for fail_at in 1..=3 {
            sweep(&env, kind, fail_at)
                .await
                .unwrap_or_else(|error| panic!("{kind:?} fault {fail_at}: {error}"));
            thread::sleep(Duration::from_millis(200));
        }
    }
}

/// A controlled-finality reorg gate.
///
/// Anvil reaches immediate finality, so the harness pins the RPC's reported finalized block
/// behind the tip. The chain itself performs a real tip reorg via `anvil_reorg`; this is a
/// controlled finality-view lag, not a reversal of finalized consensus state. The operation
/// must stay held while included-but-unfinal and while the receipt disappears, must keep its
/// note reservation and signer claim, and must execute exactly once after explicit recovery.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil and generated M5 prototype artifacts; controlled finality harness"]
async fn included_but_unfinal_then_reorged_withdrawal_stays_held_and_executes_once() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    let proxy_url = finality_proxy(env.url.clone()).await;
    let deployment = EvmDeployment::new(
        ChainNamespace::parse("eip155:31337").unwrap(),
        env.pool,
        2,
        proxy_url.clone(),
    )
    .unwrap();
    let proxy_chain = EvmChain::connect(deployment, Duration::from_secs(30))
        .await
        .unwrap();
    let context = context(&env);

    // Seed a confirmed note without any finality manipulation.
    let base = stores(&env, root.path(), None).unwrap();
    let seed = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Deposit,
        [9; 32],
        false,
    )
    .await
    .unwrap();
    let _ = sync_once(&env, root.path(), &base).await;
    assert!(wait_completed(&env, root.path(), &base, seed.identity).await);

    // Start the withdrawal and observe the included transaction.
    let withdraw = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Withdraw,
        [5; 32],
        false,
    )
    .await
    .unwrap();
    let binding = {
        use erebus_core::suite::keccak256;
        let mut value = [0u8; 32];
        value.copy_from_slice(&keccak256(&[
            b"erebus/shielded/wallet-withdraw/v1",
            &withdraw.identity,
            &withdraw.commitment,
        ]));
        value
    };
    let pinned = env
        .client
        .post(format!("{proxy_url}/pin-earlier"))
        .send()
        .await
        .unwrap();
    assert!(pinned.status().is_success());
    let finalized = proxy_chain.finalized_block().await.unwrap();
    let status = env
        .chain
        .observe(withdraw_tx(&base, &withdraw.identity))
        .await;
    let _ = status;

    // Included but reported unfinal: no completion, reservation and claim held.
    let _ = reconcile_finalized_operations(
        &proxy_chain,
        &context,
        &base.wallet,
        &base.operations,
        now(),
    )
    .await;
    let _ = reconcile_operations(&base.wallet, &base.operations, now());
    let operation = base.operations.get(&withdraw.identity).unwrap().unwrap();
    assert_ne!(operation.state(), WalletOperationState::Completed);
    assert!(base.journal.claim_nonce().unwrap().is_some());
    let snapshot = base.wallet.snapshot().unwrap();
    assert!(snapshot.spendable(&withdraw.commitment).is_none());
    assert!(snapshot
        .spendable_for(&withdraw.commitment, binding)
        .is_some());
    let _ = finalized;

    // Controlled RPC-view reorg: the included transaction disappears from the canonical view
    // (receipt null, transaction unmined), then reappears. This is a finality/reorg visibility
    // harness, not a consensus reorg of finalized state.
    let hide = env
        .client
        .post(format!("{proxy_url}/hide"))
        .json(
            &json!({"hash": format!("0x{}", hex::encode(withdraw_tx(&base, &withdraw.identity)))}),
        )
        .send()
        .await
        .unwrap();
    assert!(hide.status().is_success());
    let mut gone = false;
    for _ in 0..20 {
        if !matches!(
            proxy_chain
                .observe(withdraw_tx(&base, &withdraw.identity))
                .await,
            Ok(erebus_evm::chain::TxStatus::Included(_))
        ) {
            gone = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(gone, "hidden transaction still observed as included");

    // Still held while ambiguous.
    let _ = reconcile_finalized_operations(
        &proxy_chain,
        &context,
        &base.wallet,
        &base.operations,
        now(),
    )
    .await;
    let operation = base.operations.get(&withdraw.identity).unwrap().unwrap();
    assert_ne!(operation.state(), WalletOperationState::Completed);
    assert!(base.journal.claim_nonce().unwrap().is_some());

    // The transaction reappears; recovery is observation-only and must complete it once.
    let show = env
        .client
        .post(format!("{proxy_url}/show"))
        .send()
        .await
        .unwrap();
    assert!(show.status().is_success());
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x2"])).await;
    let unpin = env
        .client
        .post(format!("{proxy_url}/unpin"))
        .send()
        .await
        .unwrap();
    assert!(unpin.status().is_success());
    let mut completed = false;
    for _ in 0..30 {
        let _ = recover_wallet(&env, root.path(), &base).await;
        let _ = proxy_chain
            .verified_finalized_nonce(env.signer)
            .await
            .map(|nonce| {
                let _ = base.journal.release(&nonce);
            });
        let _ = reconcile_finalized_operations(
            &proxy_chain,
            &context,
            &base.wallet,
            &base.operations,
            now(),
        )
        .await;
        let _ = reconcile_operations(&base.wallet, &base.operations, now());
        if base
            .operations
            .get(&withdraw.identity)
            .unwrap()
            .is_some_and(|operation| operation.state() == WalletOperationState::Completed)
        {
            completed = true;
            break;
        }
        let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    if !completed {
        let operation = base.operations.get(&withdraw.identity).unwrap().unwrap();
        let notes = base
            .wallet
            .snapshot()
            .map(|snapshot| {
                snapshot
                    .notes()
                    .iter()
                    .map(|note| {
                        (
                            hex::encode(note.commitment()),
                            note.inclusion().is_some(),
                            note.consumption().is_some(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let attempts = operation
            .attempts()
            .iter()
            .map(|attempt| {
                format!(
                    "{:?}@{}:{}",
                    attempt.outcome,
                    attempt.block.unwrap_or(0),
                    hex::encode(attempt.tx_hash)
                )
            })
            .collect::<Vec<_>>();
        panic!(
            "recovered withdrawal never completed: state={:?} attempts={attempts:?} notes={notes:?}",
            operation.state()
        );
    }
    let snapshot = base.wallet.snapshot().unwrap();
    let consumed = snapshot
        .notes()
        .iter()
        .filter(|note| note.consumption().is_some())
        .count();
    assert_eq!(consumed, 1, "exactly one consumption expected");
    assert!(base.journal.claim_nonce().unwrap().is_none());
    let recipient = call_u128(
        &env.client,
        &env.url,
        &env.token_hex,
        encode_call(&BALANCE_SELECTOR, &[word(&[0x77; 20])]),
    )
    .await;
    assert_eq!(recipient, 7, "exactly one payout expected");
}

fn withdraw_tx(stores: &Stores, identity: &[u8; 32]) -> [u8; 32] {
    stores
        .operations
        .get(identity)
        .ok()
        .flatten()
        .and_then(|operation| operation.attempts().last().map(|attempt| attempt.tx_hash))
        .unwrap_or([0; 32])
}

/// Roll back actual Anvil state, not just receipt visibility. The reported finality stays
/// behind the discarded withdrawal; this is not a reversal of finalized consensus state.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil and generated M5 artifacts; actual local-chain rollback"]
async fn rolled_back_withdrawal_recovers_by_same_intent_replacement_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    let base = stores(&env, root.path(), None).unwrap();
    let deposit = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Deposit,
        [9; 32],
        false,
    )
    .await
    .unwrap();
    assert!(wait_completed(&env, root.path(), &base, deposit.identity).await);
    set_mining(&env, false).await;
    let checkpoint = rpc(&env.client, &env.url, "evm_snapshot", json!([])).await["result"].clone();
    assert!(checkpoint.is_string());
    let withdraw = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Withdraw,
        [5; 32],
        false,
    )
    .await
    .unwrap();
    let original = withdraw_tx(&base, &withdraw.identity);
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
    let proxy_url = finality_proxy(env.url.clone()).await;
    assert!(env
        .client
        .post(format!("{proxy_url}/pin-earlier"))
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    let proxy_chain = EvmChain::connect(
        EvmDeployment::new(
            ChainNamespace::parse("eip155:31337").unwrap(),
            env.pool,
            2,
            proxy_url.clone(),
        )
        .unwrap(),
        Duration::from_secs(30),
    )
    .await
    .unwrap();
    reconcile_finalized_operations(
        &proxy_chain,
        &context(&env),
        &base.wallet,
        &base.operations,
        now(),
    )
    .await
    .unwrap();
    assert_ne!(
        base.operations
            .get(&withdraw.identity)
            .unwrap()
            .unwrap()
            .state(),
        WalletOperationState::Completed
    );
    let recipient_call = encode_call(&BALANCE_SELECTOR, &[word(&[0x77; 20])]);
    assert_eq!(
        call_u128(
            &env.client,
            &env.url,
            &env.token_hex,
            recipient_call.clone()
        )
        .await,
        7
    );

    assert_eq!(
        rpc(&env.client, &env.url, "evm_revert", json!([checkpoint])).await["result"],
        json!(true)
    );
    assert_eq!(
        call_u128(
            &env.client,
            &env.url,
            &env.token_hex,
            recipient_call.clone()
        )
        .await,
        0
    );
    assert!(rpc(
        &env.client,
        &env.url,
        "eth_getTransactionReceipt",
        json!([format!("0x{}", hex::encode(original))])
    )
    .await["result"]
        .is_null());
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x2"])).await;
    reconcile_finalized_operations(
        &proxy_chain,
        &context(&env),
        &base.wallet,
        &base.operations,
        now(),
    )
    .await
    .unwrap();
    assert!(base.journal.claim_nonce().unwrap().is_some());
    assert!(base
        .wallet
        .snapshot()
        .unwrap()
        .spendable(&withdraw.commitment)
        .is_none());
    drop(base);

    let reopened = stores(&env, root.path(), None).unwrap();
    let replacement = replace_wallet_call(
        &env.chain,
        &reopened.operations,
        &withdraw.identity,
        &env.key,
        None,
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        replacement.state(),
        erebus_shielded_prover::operations::WalletSubmissionState::Submitted
    );
    assert_eq!(
        attempt_hashes(&reopened.operations, &withdraw.identity).len(),
        2
    );
    assert_ne!(withdraw_tx(&reopened, &withdraw.identity), original);
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
    assert!(env
        .client
        .post(format!("{proxy_url}/unpin"))
        .send()
        .await
        .unwrap()
        .status()
        .is_success());
    assert!(wait_completed(&env, root.path(), &reopened, withdraw.identity).await);
    assert!(reopened.journal.claim_nonce().unwrap().is_none());
    assert_eq!(
        reopened
            .wallet
            .snapshot()
            .unwrap()
            .notes()
            .iter()
            .filter(|note| note.consumption().is_some())
            .count(),
        1
    );
    assert_eq!(
        call_u128(&env.client, &env.url, &env.token_hex, recipient_call).await,
        7
    );
    assert_eq!(
        reopened
            .operations
            .get(&withdraw.identity)
            .unwrap()
            .unwrap()
            .attempts()
            .iter()
            .filter(|attempt| matches!(
                attempt.outcome,
                erebus_shielded_prover::operations::WalletAttemptOutcome::Completed
            ))
            .count(),
        1
    );
    // Resume refuses a completed identity; replacement reports completion without a send.
    let completed_history = attempt_hashes(&reopened.operations, &withdraw.identity);
    assert_eq!(
        completed_history
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        2
    );
    assert!(matches!(
        resume_wallet_call(&env.chain, &reopened.operations, &withdraw.identity, now()).await,
        Err(OperationError::Conflict)
    ));
    let resumed = replace_wallet_call(
        &env.chain,
        &reopened.operations,
        &withdraw.identity,
        &env.key,
        None,
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        resumed.state(),
        erebus_shielded_prover::operations::WalletSubmissionState::Completed
    );
    assert_eq!(
        attempt_hashes(&reopened.operations, &withdraw.identity),
        completed_history
    );
}

/// An RPC without finalized-tag support must fail closed, hold the operation and its claim,
/// and leave the wallet untouched; enabling the tag again recovers without a second execution.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil; controlled finality harness"]
async fn unsupported_finalized_tag_fails_closed_and_recovers_once() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    let proxy_url = finality_proxy(env.url.clone()).await;
    let deployment = EvmDeployment::new(
        ChainNamespace::parse("eip155:31337").unwrap(),
        env.pool,
        2,
        proxy_url.clone(),
    )
    .unwrap();
    let proxy_chain = EvmChain::connect(deployment, Duration::from_secs(30))
        .await
        .unwrap();
    let proxy_rpc = PoolRpc::new(&proxy_url, 31337, env.pool).unwrap();
    let base = stores(&env, root.path(), None).unwrap();

    let wrap = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Wrap,
        [7; 32],
        false,
    )
    .await
    .unwrap();
    let disabled = env
        .client
        .post(format!("{proxy_url}/disable-finalized"))
        .send()
        .await
        .unwrap();
    assert!(disabled.status().is_success());

    // The explicit finalized read fails; no fallback to head or confirmation counts.
    assert!(proxy_chain.finalized_block().await.is_err());
    assert!(reconcile_finalized_operations(
        &proxy_chain,
        &context(&env),
        &base.wallet,
        &base.operations,
        now(),
    )
    .await
    .is_err());
    let operation = base.operations.get(&wrap.identity).unwrap().unwrap();
    assert_ne!(operation.state(), WalletOperationState::Completed);
    assert!(base.journal.claim_nonce().unwrap().is_some());
    // Wallet recovery over the same RPC fails before writing any observation.
    let index = IndexStore::new(
        root.path().join("no-finality-index.json"),
        IndexDomain {
            chain_id: 31337,
            pool: env.pool,
            first_block: env.first_block,
            first_hash: env.first_hash,
        },
    )
    .unwrap();
    assert!(recover_finalized_wallet(&proxy_rpc, &index, &base.wallet)
        .await
        .is_err());

    // Enabling finalized evidence recovers and executes exactly once.
    let enabled = env
        .client
        .post(format!("{proxy_url}/enable-finalized"))
        .send()
        .await
        .unwrap();
    assert!(enabled.status().is_success());
    let mut completed = false;
    for _ in 0..30 {
        let _ = proxy_chain
            .verified_finalized_nonce(env.signer)
            .await
            .map(|nonce| {
                let _ = base.journal.release(&nonce);
            });
        let _ = reconcile_finalized_operations(
            &proxy_chain,
            &context(&env),
            &base.wallet,
            &base.operations,
            now(),
        )
        .await;
        if base
            .operations
            .get(&wrap.identity)
            .unwrap()
            .is_some_and(|operation| operation.state() == WalletOperationState::Completed)
        {
            completed = true;
            break;
        }
        let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        completed,
        "wrap never completed after finality support returned"
    );
    let balance = call_u128(
        &env.client,
        &env.url,
        &env.wmon_hex,
        encode_call(&BALANCE_SELECTOR, &[word(&env.signer)]),
    )
    .await;
    assert_eq!(balance, 7, "exactly one wrap execution expected");
}

fn attempt_hashes(operations: &OperationStore, identity: &[u8; 32]) -> Vec<String> {
    operations
        .get(identity)
        .ok()
        .flatten()
        .map(|operation| {
            operation
                .attempts()
                .iter()
                .map(|attempt| hex::encode(attempt.tx_hash))
                .collect()
        })
        .unwrap_or_default()
}

async fn set_mining(env: &Env, enabled: bool) {
    let _ = rpc(&env.client, &env.url, "anvil_setAutomine", json!([enabled])).await;
    let _ = rpc(
        &env.client,
        &env.url,
        "anvil_setIntervalMining",
        json!([if enabled { 1 } else { 0 }]),
    )
    .await;
}

/// A replacement is persisted before broadcast; after a restart the active replacement is
/// resumed, the older attempt never wins because it never reached the node, and exactly one
/// execution is observed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil; controlled replacement harness"]
async fn replacement_persists_before_broadcast_and_recovers_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    let proxy_url = finality_proxy(env.url.clone()).await;
    let deployment = EvmDeployment::new(
        ChainNamespace::parse("eip155:31337").unwrap(),
        env.pool,
        2,
        proxy_url.clone(),
    )
    .unwrap();
    let proxy_chain = EvmChain::connect(deployment, Duration::from_secs(30))
        .await
        .unwrap();
    let base = stores(&env, root.path(), None).unwrap();
    set_mining(&env, false).await;

    let wrap = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Wrap,
        [7; 32],
        false,
    )
    .await
    .unwrap();
    let first = attempt_hashes(&base.operations, &wrap.identity);
    assert_eq!(first.len(), 1);
    let dropped = env
        .client
        .post(format!("{proxy_url}/drop-next-send"))
        .send()
        .await
        .unwrap();
    assert!(dropped.status().is_success());
    let replacement = replace_wallet_call(
        &proxy_chain,
        &base.operations,
        &wrap.identity,
        &env.key,
        None,
        now(),
    )
    .await
    .unwrap();
    assert_eq!(
        replacement.state(),
        erebus_shielded_prover::operations::WalletSubmissionState::Rejected
    );
    let both = attempt_hashes(&base.operations, &wrap.identity);
    assert_eq!(both.len(), 2);
    assert_ne!(both[0], both[1]);
    drop(base);

    // Restart, then resume the active replacement; the dropped older attempt never lands.
    let reopened = stores(&env, root.path(), None).unwrap();
    let resumed = resume_wallet_call(&env.chain, &reopened.operations, &wrap.identity, now())
        .await
        .unwrap();
    assert_eq!(
        resumed.state(),
        erebus_shielded_prover::operations::WalletSubmissionState::Submitted
    );
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
    set_mining(&env, true).await;
    assert!(wait_completed(&env, root.path(), &reopened, wrap.identity).await);
    let completed = reopened
        .operations
        .get(&wrap.identity)
        .unwrap()
        .unwrap()
        .attempts()
        .iter()
        .filter(|attempt| {
            matches!(
                attempt.outcome,
                erebus_shielded_prover::operations::WalletAttemptOutcome::Completed
            )
        })
        .count();
    assert_eq!(completed, 1);
    let balance = call_u128(
        &env.client,
        &env.url,
        &env.wmon_hex,
        encode_call(&BALANCE_SELECTOR, &[word(&env.signer)]),
    )
    .await;
    assert_eq!(balance, 7, "exactly one wrap execution expected");
}

/// An older attempt that reached the chain completes the operation even after a replacement
/// was persisted but never sent, and no duplicate execution occurs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil; controlled replacement harness"]
async fn an_older_attempt_receipt_completes_without_duplicate_execution() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    let proxy_url = finality_proxy(env.url.clone()).await;
    let deployment = EvmDeployment::new(
        ChainNamespace::parse("eip155:31337").unwrap(),
        env.pool,
        2,
        proxy_url.clone(),
    )
    .unwrap();
    let proxy_chain = EvmChain::connect(deployment, Duration::from_secs(30))
        .await
        .unwrap();
    let base = stores(&env, root.path(), None).unwrap();
    set_mining(&env, false).await;

    let wrap = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Wrap,
        [7; 32],
        false,
    )
    .await
    .unwrap();
    let first = attempt_hashes(&base.operations, &wrap.identity)[0].clone();
    let dropped = env
        .client
        .post(format!("{proxy_url}/drop-next-send"))
        .send()
        .await
        .unwrap();
    assert!(dropped.status().is_success());
    let _ = replace_wallet_call(
        &proxy_chain,
        &base.operations,
        &wrap.identity,
        &env.key,
        None,
        now(),
    )
    .await
    .unwrap();
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
    set_mining(&env, true).await;
    assert!(wait_completed(&env, root.path(), &base, wrap.identity).await);
    let operation = base.operations.get(&wrap.identity).unwrap().unwrap();
    let completed: Vec<String> = operation
        .attempts()
        .iter()
        .filter(|attempt| {
            matches!(
                attempt.outcome,
                erebus_shielded_prover::operations::WalletAttemptOutcome::Completed
            )
        })
        .map(|attempt| hex::encode(attempt.tx_hash))
        .collect();
    assert_eq!(
        completed,
        vec![first],
        "the older included attempt must be the completion"
    );
    let balance = call_u128(
        &env.client,
        &env.url,
        &env.wmon_hex,
        encode_call(&BALANCE_SELECTOR, &[word(&env.signer)]),
    )
    .await;
    assert_eq!(balance, 7, "exactly one wrap execution expected");
}

/// Replacement rejects unchanged fees before persisting or broadcasting anything.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil; controlled replacement harness"]
async fn replacement_requires_a_fee_bump_and_preserves_intent() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    let base = stores(&env, root.path(), None).unwrap();
    set_mining(&env, false).await;
    let wrap = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Wrap,
        [7; 32],
        false,
    )
    .await
    .unwrap();
    let before = attempt_hashes(&base.operations, &wrap.identity);
    let unchanged = replace_wallet_call(
        &env.chain,
        &base.operations,
        &wrap.identity,
        &env.key,
        Some(env.fees),
        now(),
    )
    .await;
    assert!(unchanged.is_err(), "identical fees must not replace");
    assert_eq!(attempt_hashes(&base.operations, &wrap.identity), before);
}

fn completed_hashes(operations: &OperationStore, identity: &[u8; 32]) -> Vec<String> {
    operations
        .get(identity)
        .ok()
        .flatten()
        .map(|operation| {
            operation
                .attempts()
                .iter()
                .filter(|attempt| {
                    matches!(
                        attempt.outcome,
                        erebus_shielded_prover::operations::WalletAttemptOutcome::Completed
                    )
                })
                .map(|attempt| hex::encode(attempt.tx_hash))
                .collect()
        })
        .unwrap_or_default()
}

fn snapshot_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                stack.push(entry.path());
            } else if metadata.is_file() {
                files.push((entry.path(), std::fs::read(entry.path()).unwrap()));
            }
        }
    }
    files
}

fn wallet_file(root: &Path) -> PathBuf {
    root.join("wallet/notes.enc")
}

async fn prepare_withdrawal(
    env: &Env,
    root: &Path,
    stores: &Stores,
) -> Result<erebus_shielded_prover::operations::PreparedWalletCall, OperationError> {
    let rpc = PoolRpc::new(&env.url, 31337, env.pool).unwrap();
    let index = IndexStore::new(
        root.join("withdraw-index.json"),
        IndexDomain {
            chain_id: 31337,
            pool: env.pool,
            first_block: env.first_block,
            first_hash: env.first_hash,
        },
    )
    .map_err(|_| OperationError::Journal)?;
    let _ = recover_finalized_wallet(&rpc, &index, &stores.wallet).await;
    let pool_index = index.load().map_err(|_| OperationError::Journal)?;
    let commitment = stores
        .wallet
        .snapshot()
        .map_err(|_| OperationError::Journal)?
        .notes()
        .iter()
        .find(|note| note.inclusion().is_some() && note.consumption().is_none())
        .map(|note| note.commitment())
        .ok_or(OperationError::Shape("no confirmed note"))?;
    prepare_withdraw(
        &env.release,
        &env.options,
        &stores.wallet,
        &stores.operations,
        &pool_index,
        WalletDomain {
            chain_id: 31337,
            pool: env.pool,
            verifier_version: 2,
        },
        2,
        commitment,
        [0x77; 20],
        now(),
    )
    .await
}

async fn prepare_withdrawal_from_stale_index(
    env: &Env,
    stores: &Stores,
    index_path: &Path,
    commitment: [u8; 32],
) -> Result<erebus_shielded_prover::operations::PreparedWalletCall, OperationError> {
    let index = IndexStore::new(
        index_path,
        IndexDomain {
            chain_id: 31337,
            pool: env.pool,
            first_block: env.first_block,
            first_hash: env.first_hash,
        },
    )
    .map_err(|_| OperationError::Journal)?;
    let pool_index = index.load().map_err(|_| OperationError::Journal)?;
    prepare_withdraw(
        &env.release,
        &env.options,
        &stores.wallet,
        &stores.operations,
        &pool_index,
        WalletDomain {
            chain_id: 31337,
            pool: env.pool,
            verifier_version: 2,
        },
        2,
        commitment,
        [0x77; 20],
        now(),
    )
    .await
}

/// A stale encrypted backup restored after a withdrawal can look spendable, but it can never
/// authorize a second payout: the withdrawal path re-observes finality first, and a direct
/// stale build is rejected by the chain's consumed nullifier.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil and generated M5 prototype artifacts; stale backup after withdrawal"]
async fn stale_backup_after_withdrawal_cannot_spend_or_reauthorize() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    ensure_allowance(&env).await;
    let base = stores(&env, root.path(), None).unwrap();
    let deposit = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Deposit,
        [0x11; 32],
        false,
    )
    .await
    .unwrap();
    assert!(wait_completed(&env, root.path(), &base, deposit.identity).await);
    let backup = std::fs::read(wallet_file(root.path())).unwrap();

    // Snapshot the pre-withdrawal pool history prefix for the direct stale attempt.
    let stale_index = root.path().join("stale-index.json");
    {
        let rpc = PoolRpc::new(&env.url, 31337, env.pool).unwrap();
        let index = IndexStore::new(
            &stale_index,
            IndexDomain {
                chain_id: 31337,
                pool: env.pool,
                first_block: env.first_block,
                first_hash: env.first_hash,
            },
        )
        .unwrap();
        recover_finalized_wallet(&rpc, &index, &base.wallet)
            .await
            .unwrap();
    }

    let withdraw = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Withdraw,
        [0x22; 32],
        false,
    )
    .await
    .unwrap();
    assert!(wait_completed(&env, root.path(), &base, withdraw.identity).await);
    let commitment = base
        .wallet
        .snapshot()
        .unwrap()
        .notes()
        .iter()
        .find(|note| note.consumption().is_some())
        .expect("withdrawn note")
        .commitment();
    assert_eq!(
        balance_of(&env, &env.token_hex, &[0x77; 20]).await,
        7,
        "exactly one withdrawal payout before restoring the backup"
    );

    // Restore the stale backup and sync: consumption is re-observed, never forgotten.
    std::fs::write(wallet_file(root.path()), &backup).unwrap();
    let restored = stores(&env, root.path(), None).unwrap();
    let _ = sync_once(&env, root.path(), &restored).await;
    let snapshot = restored.wallet.snapshot().unwrap();
    let note = snapshot
        .notes()
        .iter()
        .find(|note| note.commitment() == commitment)
        .expect("restored note");
    assert!(
        note.consumption().is_some(),
        "the restored backup must re-observe the withdrawal"
    );
    assert!(snapshot.spendable(&commitment).is_none());
    assert!(snapshot.select(&env.token, 7).is_none());
    drop(snapshot);

    // The normal withdrawal path refuses before building a second payout.
    let refused = run_operation(
        &env,
        root.path(),
        &restored,
        WalletOperationKind::Withdraw,
        [0x33; 32],
        false,
    )
    .await;
    assert!(refused.is_err(), "a stale note built a second withdrawal");

    // The direct stale attempt may build and broadcast, but the consumed nullifier cannot
    // execute a second payout and the operation never completes.
    std::fs::write(wallet_file(root.path()), &backup).unwrap();
    let mut stale_stores = stores(&env, root.path(), None).unwrap();
    // Model restoring a backup that did not retain the original withdrawal operation.
    // Reusing its completed journal would only exercise legitimate idempotent completion.
    stale_stores.operations = OperationStore::open(root.path().join("stale-operations")).unwrap();
    let direct =
        prepare_withdrawal_from_stale_index(&env, &stale_stores, &stale_index, commitment).await;
    if let Ok(prepared) = direct {
        let submitted = submit_wallet_call(
            &env.chain,
            &stale_stores.journal,
            &stale_stores.operations,
            &domain(&env),
            &prepared,
            &env.key,
            env.fees,
            2_000_000,
            now(),
            false,
        )
        .await;
        let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x2"])).await;
        let _ = sync_once(&env, root.path(), &stale_stores).await;
        let record = stale_stores
            .operations
            .get(&prepared.identity)
            .ok()
            .flatten();
        let state = record.as_ref().map(|operation| operation.state());
        let attempts: Vec<String> = record
            .as_ref()
            .map(|operation| {
                operation
                    .attempts()
                    .iter()
                    .map(|attempt| {
                        format!(
                            "{:?}@{}:{}",
                            attempt.outcome,
                            attempt.block.unwrap_or(0),
                            hex::encode(attempt.tx_hash)
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let payout = balance_of(&env, &env.token_hex, &[0x77; 20]).await;
        eprintln!(
            "{}",
            json!({
                "status": "stale-direct-attempt",
                "submit": format!("{submitted:?}"),
                "state": format!("{state:?}"),
                "attempts": attempts,
                "payout": payout,
            })
        );
        assert_eq!(
            payout, 7,
            "a stale backup must never pay twice: {submitted:?}"
        );
        assert_ne!(
            state,
            Some(WalletOperationState::Completed),
            "a reverted stale attempt must not be reported Completed: {record:?}"
        );
    }
    assert_eq!(
        balance_of(&env, &env.token_hex, &[0x77; 20]).await,
        7,
        "a stale backup must never pay twice"
    );
    assert_eq!(
        completed_hashes(&base.operations, &withdraw.identity).len(),
        1,
        "the original withdrawal must remain the only completion"
    );
    eprintln!(
        "{}",
        json!({
            "status": "stale-backup-after-withdrawal",
            "payout": balance_of(&env, &env.token_hex, &[0x77; 20]).await,
            "original_completions": completed_hashes(&base.operations, &withdraw.identity).len(),
            "restored_note_consumed": true,
            "second_payout_refused": true,
        })
    );
}

/// A lost broadcast response plus an older attempt that wins over a fee-bumped replacement
/// must still produce exactly one payout or burn and one completed operation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil; controlled replacement harness"]
async fn lost_response_with_older_attempt_winning_executes_once() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    let proxy_url = finality_proxy(env.url.clone()).await;
    let deployment = EvmDeployment::new(
        ChainNamespace::parse("eip155:31337").unwrap(),
        env.pool,
        2,
        proxy_url.clone(),
    )
    .unwrap();
    let proxy_chain = EvmChain::connect(deployment, Duration::from_secs(30))
        .await
        .unwrap();
    ensure_allowance(&env).await;
    let base = stores(&env, root.path(), None).unwrap();
    set_mining(&env, false).await;

    // Withdrawal: the older attempt's response is lost while the transaction reaches the
    // mempool, the older attempt then mines, and a late replacement can never execute.
    let deposit = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Deposit,
        [0x51; 32],
        false,
    )
    .await
    .unwrap();
    assert!(wait_completed(&env, root.path(), &base, deposit.identity).await);
    let withdrawal = prepare_withdrawal(&env, root.path(), &base).await.unwrap();
    let lost = env
        .client
        .post(format!("{proxy_url}/lose-next-send"))
        .send()
        .await
        .unwrap();
    assert!(lost.status().is_success());
    let submitted = submit_wallet_call(
        &proxy_chain,
        &base.journal,
        &base.operations,
        &domain(&env),
        &withdrawal,
        &env.key,
        env.fees,
        2_000_000,
        now(),
        false,
    )
    .await;
    if let Ok(submission) = &submitted {
        assert_ne!(
            submission.state(),
            erebus_shielded_prover::operations::WalletSubmissionState::Completed,
            "a lost response must never complete: {submitted:?}"
        );
    }
    let attempts = attempt_hashes(&base.operations, &withdrawal.identity);
    assert_eq!(
        attempts.len(),
        1,
        "the attempt must be durable: {attempts:?}"
    );
    let older = attempts[0].clone();
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
    let _ = replace_wallet_call(
        &proxy_chain,
        &base.operations,
        &withdrawal.identity,
        &env.key,
        None,
        now(),
    )
    .await;
    set_mining(&env, true).await;
    assert!(
        wait_completed(&env, root.path(), &base, withdrawal.identity).await,
        "the older winning attempt must complete the operation"
    );
    let all_hashes = attempt_hashes(&base.operations, &withdrawal.identity);
    let completed = completed_hashes(&base.operations, &withdrawal.identity);
    let outcomes: Vec<String> = base
        .operations
        .get(&withdrawal.identity)
        .ok()
        .flatten()
        .map(|operation| {
            operation
                .attempts()
                .iter()
                .map(|attempt| {
                    format!(
                        "{:?}@{}:{}",
                        attempt.outcome,
                        attempt.block.unwrap_or(0),
                        hex::encode(attempt.tx_hash)
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    eprintln!(
        "{}",
        json!({
            "status": "withdraw-older-wins",
            "older": older,
            "attempts": all_hashes,
            "outcomes": outcomes,
            "completed": completed,
        })
    );
    assert_eq!(
        completed.len(),
        1,
        "exactly one completed attempt expected: {completed:?}"
    );
    assert_eq!(
        completed,
        vec![older.clone()],
        "completion must name the older finalized winner, not its rejected replacement"
    );
    let record = base.operations.get(&withdrawal.identity).unwrap().unwrap();
    assert!(record.attempts().iter().any(|attempt| {
        hex::encode(attempt.tx_hash) == older
            && attempt.outcome
                == erebus_shielded_prover::operations::WalletAttemptOutcome::Completed
            && attempt.block.is_some_and(|block| block > 0)
    }));
    assert_eq!(
        balance_of(&env, &env.token_hex, &[0x77; 20]).await,
        7,
        "exactly one withdrawal payout expected"
    );

    // Unwrap: same lost response and older-wins race, exactly one burn.
    let wrap = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Wrap,
        [0x53; 32],
        false,
    )
    .await
    .unwrap();
    assert!(wait_completed(&env, root.path(), &base, wrap.identity).await);
    assert_eq!(balance_of(&env, &env.wmon_hex, &env.signer).await, 7);
    set_mining(&env, false).await;
    let unwrap = prepare_unwrap(&base.operations, 31337, env.wmon, 7, [0x54; 32], now()).unwrap();
    let lost = env
        .client
        .post(format!("{proxy_url}/lose-next-send"))
        .send()
        .await
        .unwrap();
    assert!(lost.status().is_success());
    let submitted = submit_wallet_call(
        &proxy_chain,
        &base.journal,
        &base.operations,
        &domain(&env),
        &unwrap,
        &env.key,
        env.fees,
        2_000_000,
        now(),
        false,
    )
    .await;
    if let Ok(submission) = &submitted {
        assert_ne!(
            submission.state(),
            erebus_shielded_prover::operations::WalletSubmissionState::Completed,
            "a lost unwind response must never complete: {submitted:?}"
        );
    }
    let attempts = attempt_hashes(&base.operations, &unwrap.identity);
    assert_eq!(
        attempts.len(),
        1,
        "the attempt must be durable: {attempts:?}"
    );
    let older = attempts[0].clone();
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
    let _ = replace_wallet_call(
        &proxy_chain,
        &base.operations,
        &unwrap.identity,
        &env.key,
        None,
        now(),
    )
    .await;
    set_mining(&env, true).await;
    assert!(
        wait_completed(&env, root.path(), &base, unwrap.identity).await,
        "the older winning unwrap must complete the operation"
    );
    let completed_unwrap = completed_hashes(&base.operations, &unwrap.identity);
    eprintln!(
        "{}",
        json!({
            "status": "unwrap-older-wins",
            "older": older,
            "attempts": attempt_hashes(&base.operations, &unwrap.identity),
            "completed": completed_unwrap,
        })
    );
    assert_eq!(
        completed_unwrap.len(),
        1,
        "exactly one completed unwrap attempt expected: {completed_unwrap:?}"
    );
    assert_eq!(
        balance_of(&env, &env.wmon_hex, &env.signer).await,
        0,
        "exactly one unwrap burn expected"
    );
    eprintln!(
        "{}",
        json!({
            "status": "lost-response-older-wins",
            "withdraw_payout": 7,
            "withdraw_completed_attempts": 1,
            "unwrap_burn": "7 -> 0",
            "unwrap_completed_attempts": 1,
        })
    );
}

/// Truncated operation completion evidence reads as corruption, never absence: sync and
/// recovery fail closed without another broadcast, and restoring the bytes completes the
/// original intent exactly once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil and generated M5 prototype artifacts; truncated completion evidence"]
async fn truncated_operation_completion_evidence_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    ensure_allowance(&env).await;
    let base = stores(&env, root.path(), None).unwrap();
    let deposit = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Deposit,
        [0x61; 32],
        false,
    )
    .await
    .unwrap();
    assert!(wait_completed(&env, root.path(), &base, deposit.identity).await);
    set_mining(&env, false).await;

    let withdrawal = prepare_withdrawal(&env, root.path(), &base).await.unwrap();
    submit_wallet_call(
        &env.chain,
        &base.journal,
        &base.operations,
        &domain(&env),
        &withdrawal,
        &env.key,
        env.fees,
        2_000_000,
        now(),
        false,
    )
    .await
    .unwrap();
    let signer_hex = format!("0x{}", hex::encode(env.signer));
    let nonce_before: u64 = {
        let value = rpc(
            &env.client,
            &env.url,
            "eth_getTransactionCount",
            json!([signer_hex, "latest"]),
        )
        .await;
        u64::from_str_radix(
            value["result"].as_str().unwrap().trim_start_matches("0x"),
            16,
        )
        .unwrap()
    };

    // Truncate every file under the operation store: records may not read as absence.
    let operations_root = root.path().join("operations");
    let files = snapshot_tree(&operations_root);
    assert!(!files.is_empty());
    for (path, _) in &files {
        std::fs::write(path, b"").unwrap();
    }
    let corrupt = stores(&env, root.path(), None).unwrap();
    assert!(
        corrupt.operations.get(&withdrawal.identity).is_err(),
        "a truncated completion record must be corruption, not absence"
    );
    let sync = sync_once(&env, root.path(), &corrupt).await;
    assert!(sync.is_err(), "sync must fail closed on corrupt evidence");
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x2"])).await;
    let sync = sync_once(&env, root.path(), &corrupt).await;
    assert!(sync.is_err(), "sync must stay failed closed while corrupt");

    // Restore the exact bytes: the original intent reconciles exactly once.
    for (path, bytes) in files {
        std::fs::write(path, bytes).unwrap();
    }
    set_mining(&env, true).await;
    let reopened = stores(&env, root.path(), None).unwrap();
    assert!(wait_completed(&env, root.path(), &reopened, withdrawal.identity).await);
    assert_eq!(
        completed_hashes(&reopened.operations, &withdrawal.identity).len(),
        1,
        "exactly one completed attempt after restoration"
    );
    assert_eq!(
        balance_of(&env, &env.token_hex, &[0x77; 20]).await,
        7,
        "exactly one payout after restoration"
    );
    let nonce_after: u64 = {
        let value = rpc(
            &env.client,
            &env.url,
            "eth_getTransactionCount",
            json!([signer_hex, "latest"]),
        )
        .await;
        u64::from_str_radix(
            value["result"].as_str().unwrap().trim_start_matches("0x"),
            16,
        )
        .unwrap()
    };
    assert_eq!(
        nonce_after,
        nonce_before + 1,
        "no replacement broadcast may escape the corrupt store"
    );
    eprintln!(
        "{}",
        json!({
            "status": "truncated-completion-evidence",
            "corruption_observed": true,
            "sync_failed_closed": true,
            "completed_attempts": 1,
            "payout": 7,
            "nonce_delta": nonce_after - nonce_before,
        })
    );
}

async fn prepare_withdrawal_for(
    env: &Env,
    root: &Path,
    stores: &Stores,
    commitment: [u8; 32],
) -> Result<erebus_shielded_prover::operations::PreparedWalletCall, OperationError> {
    let rpc = PoolRpc::new(&env.url, 31337, env.pool).unwrap();
    let index = IndexStore::new(
        root.join("withdraw-index.json"),
        IndexDomain {
            chain_id: 31337,
            pool: env.pool,
            first_block: env.first_block,
            first_hash: env.first_hash,
        },
    )
    .map_err(|_| OperationError::Journal)?;
    let _ = recover_finalized_wallet(&rpc, &index, &stores.wallet).await;
    let pool_index = index.load().map_err(|_| OperationError::Journal)?;
    prepare_withdraw(
        &env.release,
        &env.options,
        &stores.wallet,
        &stores.operations,
        &pool_index,
        WalletDomain {
            chain_id: 31337,
            pool: env.pool,
            verifier_version: 2,
        },
        2,
        commitment,
        [0x77; 20],
        now(),
    )
    .await
}

fn note_view(stores: &Stores, commitment: [u8; 32]) -> (bool, bool, Option<[u8; 32]>) {
    let snapshot = stores.wallet.snapshot().unwrap();
    let note = snapshot
        .notes()
        .iter()
        .find(|note| note.commitment() == commitment)
        .expect("note");
    (
        note.inclusion().is_some(),
        note.consumption().is_some(),
        note.reserved(),
    )
}

/// A crash after the matching reservation cleanup but before operation completion persists
/// must complete from the same finalized receipt on restart, with no second send.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil and generated M5 prototype artifacts; reservation cleanup crash boundary"]
async fn crash_after_reservation_cleanup_completes_from_same_receipt() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    ensure_allowance(&env).await;
    let base = stores(&env, root.path(), None).unwrap();
    let deposit = run_operation(
        &env,
        root.path(),
        &base,
        WalletOperationKind::Deposit,
        [0x81; 32],
        false,
    )
    .await
    .unwrap();
    assert!(wait_completed(&env, root.path(), &base, deposit.identity).await);

    let withdrawal = prepare_withdrawal(&env, root.path(), &base).await.unwrap();
    submit_wallet_call(
        &env.chain,
        &base.journal,
        &base.operations,
        &domain(&env),
        &withdrawal,
        &env.key,
        env.fees,
        2_000_000,
        now(),
        false,
    )
    .await
    .unwrap();
    let attempt_hash = attempt_hashes(&base.operations, &withdrawal.identity)
        .into_iter()
        .next()
        .expect("retained attempt");
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
    let inclusion = match env
        .chain
        .observe(hex::decode(&attempt_hash).unwrap().try_into().unwrap())
        .await
        .unwrap()
    {
        erebus_evm::chain::TxStatus::Included(inclusion) => inclusion,
        other => panic!("expected an included withdrawal: {other:?}"),
    };
    // Observe the consumption first, then clear the reservation exactly as reconciliation
    // does, and stop before the operation completion write: that is the crash state.
    let mut consumed = false;
    for _ in 0..30 {
        recover_wallet(&env, root.path(), &base).await;
        let (_, seen, _) = note_view(&base, withdrawal.commitment);
        if seen {
            consumed = true;
            break;
        }
        let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(consumed, "consumption must be observed before cleanup");
    let (_, consumed, binding) = note_view(&base, withdrawal.commitment);
    assert!(consumed, "consumption must be observed before cleanup");
    let binding = binding.expect("reservation binding");
    base.wallet
        .update(|wallet| wallet.conclude_reservation(&withdrawal.commitment, binding))
        .unwrap();
    let (_, consumed, reserved) = note_view(&base, withdrawal.commitment);
    assert!(consumed && reserved.is_none(), "crash state not reached");
    assert_ne!(
        base.operations
            .get(&withdrawal.identity)
            .unwrap()
            .unwrap()
            .state(),
        WalletOperationState::Completed,
        "the completion must not be persisted before the simulated crash"
    );

    // Restart: the same receipt must complete the operation without another send.
    let signer_hex = format!("0x{}", hex::encode(env.signer));
    let nonce_at_crash: u64 = {
        let value = rpc(
            &env.client,
            &env.url,
            "eth_getTransactionCount",
            json!([signer_hex, "latest"]),
        )
        .await;
        u64::from_str_radix(
            value["result"].as_str().unwrap().trim_start_matches("0x"),
            16,
        )
        .unwrap()
    };
    set_mining(&env, true).await;
    let reopened = stores(&env, root.path(), None).unwrap();
    let report = reconcile_finalized_operations(
        &env.chain,
        &context(&env),
        &reopened.wallet,
        &reopened.operations,
        now(),
    )
    .await
    .unwrap();
    assert!(
        report
            .changed
            .iter()
            .any(|operation| operation.identity() == withdrawal.identity),
        "the same receipt must complete the operation: {report:?}"
    );
    let _ = reconcile_operations(&reopened.wallet, &reopened.operations, now()).unwrap();
    let operation = reopened
        .operations
        .get(&withdrawal.identity)
        .unwrap()
        .unwrap();
    assert_eq!(operation.state(), WalletOperationState::Completed);
    let completed: Vec<_> = operation
        .attempts()
        .iter()
        .filter(|attempt| {
            matches!(
                attempt.outcome,
                erebus_shielded_prover::operations::WalletAttemptOutcome::Completed
            )
        })
        .collect();
    assert_eq!(completed.len(), 1);
    assert_eq!(
        completed[0].block,
        Some(inclusion.block.number),
        "the winning attempt must keep its real finalized block"
    );
    assert_eq!(
        balance_of(&env, &env.token_hex, &[0x77; 20]).await,
        7,
        "exactly one payout expected"
    );
    let nonce: u64 = {
        let value = rpc(
            &env.client,
            &env.url,
            "eth_getTransactionCount",
            json!([signer_hex, "latest"]),
        )
        .await;
        u64::from_str_radix(
            value["result"].as_str().unwrap().trim_start_matches("0x"),
            16,
        )
        .unwrap()
    };
    assert_eq!(
        nonce, nonce_at_crash,
        "no send may escape the crash-boundary restart"
    );
    eprintln!(
        "{}",
        json!({
            "status": "reservation-cleanup-crash",
            "completed_from_same_receipt": true,
            "payout": 7,
            "nonce": nonce,
            "completed_block": completed[0].block,
        })
    );
}

/// Consumption evidence that does not match this operation's own successful receipt (wrong
/// transaction hash, block number or block hash) must never complete the withdrawal or clear
/// its reservation.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil and generated M5 prototype artifacts; mismatched consumption evidence"]
async fn mismatched_consumption_never_completes_or_clears_the_reservation() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    let base = stores(&env, root.path(), None).unwrap();
    ensure_allowance(&env).await;
    for op_ref in [[0x91u8; 32], [0x92; 32], [0x93; 32]] {
        let deposit = run_operation(
            &env,
            root.path(),
            &base,
            WalletOperationKind::Deposit,
            op_ref,
            false,
        )
        .await
        .unwrap();
        assert!(wait_completed(&env, root.path(), &base, deposit.identity).await);
    }
    let notes: Vec<[u8; 32]> = base
        .wallet
        .snapshot()
        .unwrap()
        .notes()
        .iter()
        .filter(|note| note.inclusion().is_some() && note.consumption().is_none())
        .map(|note| note.commitment())
        .collect();
    assert_eq!(notes.len(), 3, "three confirmed notes expected");

    for (phase, commitment) in notes.into_iter().enumerate() {
        let withdrawal = prepare_withdrawal_for(&env, root.path(), &base, commitment)
            .await
            .unwrap();
        let binding = note_view(&base, commitment).2.expect("reservation");
        submit_wallet_call(
            &env.chain,
            &base.journal,
            &base.operations,
            &domain(&env),
            &withdrawal,
            &env.key,
            env.fees,
            2_000_000,
            now(),
            false,
        )
        .await
        .unwrap();
        let tx_hash: [u8; 32] = hex::decode(
            attempt_hashes(&base.operations, &withdrawal.identity)
                .into_iter()
                .next()
                .expect("attempt"),
        )
        .unwrap()
        .try_into()
        .unwrap();
        let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
        let inclusion = match env.chain.observe(tx_hash).await.unwrap() {
            erebus_evm::chain::TxStatus::Included(inclusion) => inclusion,
            other => panic!("expected inclusion: {other:?}"),
        };
        let nullifier = base
            .wallet
            .snapshot()
            .unwrap()
            .notes()
            .iter()
            .find(|note| note.commitment() == commitment)
            .unwrap()
            .nullifier();
        let field = ["tx_hash", "block_number", "block_hash"][phase];
        let spent = match phase {
            0 => erebus_shielded_prover::wallet::NoteConsumption {
                block_number: inclusion.block.number,
                block_hash: inclusion.block.hash,
                tx_hash: [0xEE; 32],
            },
            1 => erebus_shielded_prover::wallet::NoteConsumption {
                block_number: inclusion.block.number + 1,
                block_hash: inclusion.block.hash,
                tx_hash,
            },
            _ => erebus_shielded_prover::wallet::NoteConsumption {
                block_number: inclusion.block.number,
                block_hash: [0xDD; 32],
                tx_hash,
            },
        };
        base.wallet
            .update(|wallet| wallet.observe_consumption(&nullifier, spent))
            .unwrap();
        let report = reconcile_finalized_operations(
            &env.chain,
            &context(&env),
            &base.wallet,
            &base.operations,
            now(),
        )
        .await
        .unwrap();
        assert!(
            !report
                .changed
                .iter()
                .any(|operation| operation.identity() == withdrawal.identity),
            "phase {phase}: a mismatched consumption completed the withdrawal: {report:?}"
        );
        let _ = reconcile_operations(&base.wallet, &base.operations, now()).unwrap();
        let operation = base.operations.get(&withdrawal.identity).unwrap().unwrap();
        assert_ne!(
            operation.state(),
            WalletOperationState::Completed,
            "phase {phase}: mismatched consumption completed the operation"
        );
        let (_, consumed, reserved) = note_view(&base, commitment);
        assert!(consumed, "phase {phase}: fabricated consumption missing");
        assert_eq!(
            reserved,
            Some(binding),
            "phase {phase}: a mismatched consumption cleared the reservation"
        );
        // Recovery is authoritative from the finalized prefix: it repairs the fabricated
        // consumption to the real finalized spend. The refusal above happened before this.
        let mut spent = None;
        for _ in 0..30 {
            recover_wallet(&env, root.path(), &base).await;
            let snapshot = base.wallet.snapshot().unwrap();
            let note = snapshot
                .notes()
                .iter()
                .find(|note| note.commitment() == commitment)
                .unwrap();
            if let Some(consumption) = note.consumption() {
                spent = Some(consumption);
                break;
            }
            let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let spent = spent.expect("repaired consumption");
        assert_eq!(spent.tx_hash, tx_hash, "phase {phase}: wrong repaired tx");
        assert_eq!(
            spent.block_number, inclusion.block.number,
            "phase {phase}: wrong repaired block number"
        );
        assert_eq!(
            spent.block_hash, inclusion.block.hash,
            "phase {phase}: wrong repaired block hash"
        );
        // Free the finalized signer nonce for the next phase without touching the operation.
        if let Ok(nonce) = env.chain.verified_finalized_nonce(env.signer).await {
            let _ = base.journal.release(&nonce);
        }
        eprintln!(
            "{}",
            json!({
                "status": "mismatched-consumption",
                "phase": phase,
                "field": field,
                "operation_state": format!("{:?}", operation.state()),
                "reservation_retained": true,
                "payout": balance_of(&env, &env.token_hex, &[0x77; 20]).await,
            })
        );
    }
    assert_eq!(
        balance_of(&env, &env.token_hex, &[0x77; 20]).await,
        21,
        "each of the three withdrawals paid exactly once"
    );
}

async fn observe_consumption_until(
    env: &Env,
    root: &Path,
    stores: &Stores,
    commitment: [u8; 32],
) -> bool {
    for _ in 0..30 {
        recover_wallet(env, root, stores).await;
        if note_view(stores, commitment).1 {
            return true;
        }
        let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

/// Reconciling one operation's finalized receipt must not touch another pending operation's
/// reservation or execute it early.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil and generated M5 prototype artifacts; foreign reservation isolation"]
async fn another_operations_reservation_stays_untouched() {
    let root = tempfile::tempdir().unwrap();
    let env = env(root.path()).await;
    ensure_allowance(&env).await;
    let base = stores(&env, root.path(), None).unwrap();
    for op_ref in [[0xa1u8; 32], [0xa2; 32]] {
        let deposit = run_operation(
            &env,
            root.path(),
            &base,
            WalletOperationKind::Deposit,
            op_ref,
            false,
        )
        .await
        .unwrap();
        assert!(wait_completed(&env, root.path(), &base, deposit.identity).await);
    }
    let notes: Vec<[u8; 32]> = base
        .wallet
        .snapshot()
        .unwrap()
        .notes()
        .iter()
        .filter(|note| note.inclusion().is_some() && note.consumption().is_none())
        .map(|note| note.commitment())
        .collect();
    assert_eq!(notes.len(), 2);

    let first = prepare_withdrawal_for(&env, root.path(), &base, notes[0])
        .await
        .unwrap();
    let second = prepare_withdrawal_for(&env, root.path(), &base, notes[1])
        .await
        .unwrap();
    let second_binding = note_view(&base, notes[1]).2.expect("second reservation");
    submit_wallet_call(
        &env.chain,
        &base.journal,
        &base.operations,
        &domain(&env),
        &first,
        &env.key,
        env.fees,
        2_000_000,
        now(),
        false,
    )
    .await
    .unwrap();
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
    assert!(
        observe_consumption_until(&env, root.path(), &base, notes[0]).await,
        "the first withdrawal consumption must be observed"
    );
    let report = reconcile_finalized_operations(
        &env.chain,
        &context(&env),
        &base.wallet,
        &base.operations,
        now(),
    )
    .await
    .unwrap();
    let _ = reconcile_operations(&base.wallet, &base.operations, now()).unwrap();
    assert!(report
        .changed
        .iter()
        .any(|operation| operation.identity() == first.identity));
    assert_eq!(
        base.operations
            .get(&first.identity)
            .unwrap()
            .unwrap()
            .state(),
        WalletOperationState::Completed
    );
    let second_operation = base.operations.get(&second.identity).unwrap().unwrap();
    assert_ne!(
        second_operation.state(),
        WalletOperationState::Completed,
        "the untouched operation must stay pending"
    );
    let (second_included, second_consumed, second_reserved) = note_view(&base, notes[1]);
    assert!(second_included);
    assert!(!second_consumed, "the second note must stay unspent");
    assert_eq!(
        second_reserved,
        Some(second_binding),
        "another operation's reservation was altered"
    );
    assert_eq!(
        balance_of(&env, &env.token_hex, &[0x77; 20]).await,
        7,
        "only the first withdrawal may pay before the second is submitted"
    );

    // The untouched operation can still execute normally.
    if let Ok(nonce) = env.chain.verified_finalized_nonce(env.signer).await {
        let _ = base.journal.release(&nonce);
    }
    submit_wallet_call(
        &env.chain,
        &base.journal,
        &base.operations,
        &domain(&env),
        &second,
        &env.key,
        env.fees,
        2_000_000,
        now(),
        false,
    )
    .await
    .unwrap();
    let _ = rpc(&env.client, &env.url, "anvil_mine", json!(["0x1"])).await;
    assert!(
        observe_consumption_until(&env, root.path(), &base, notes[1]).await,
        "the second withdrawal consumption must be observed"
    );
    let _ = reconcile_finalized_operations(
        &env.chain,
        &context(&env),
        &base.wallet,
        &base.operations,
        now(),
    )
    .await
    .unwrap();
    let _ = reconcile_operations(&base.wallet, &base.operations, now()).unwrap();
    assert_eq!(
        base.operations
            .get(&second.identity)
            .unwrap()
            .unwrap()
            .state(),
        WalletOperationState::Completed
    );
    assert_eq!(
        balance_of(&env, &env.token_hex, &[0x77; 20]).await,
        14,
        "each withdrawal pays exactly once"
    );
    eprintln!(
        "{}",
        json!({
            "status": "foreign-reservation-isolation",
            "first_completed": true,
            "second_pending_untouched": true,
            "second_reservation_retained": true,
            "total_payout": 14,
        })
    );
}
