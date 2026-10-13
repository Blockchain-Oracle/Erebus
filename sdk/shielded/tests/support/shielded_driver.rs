//! Actual negotiated suite-2 payment through copied native commands, with prototype keys.

use super::*;
use axum::{extract::Path as RoutePath, routing::get};
use erebus_core::shielded_auth::derive_key;
use erebus_evm::chain::{SignedTransaction, TransactionKey};
use erebus_shielded_prover::wallet::{OwnedNote, WalletDomain, WalletStore};

#[derive(Default)]
struct RpcAudit {
    requests: std::sync::Mutex<Vec<Value>>,
    overflow: AtomicBool,
}

async fn audited_forward(
    State((proxy, audit)): State<(Proxy, Arc<RpcAudit>)>,
    Json(request): Json<Value>,
) -> Json<Value> {
    {
        let mut requests = audit.requests.lock().unwrap();
        if requests.len() >= 8192 || serde_json::to_vec(&request).unwrap().len() > 4096 {
            audit.overflow.store(true, Ordering::SeqCst);
        } else {
            requests.push(request.clone());
        }
    }
    forward(State(proxy), Json(request)).await
}

fn assert_public_query(value: &Value) {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
        Value::String(text) => assert!(
            ["latest", "pending", "safe", "finalized", "earliest"].contains(&text.as_str())
                || text.strip_prefix("0x").is_some_and(|hex| {
                    hex.len() <= 128 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
                }),
            "RPC query contains an unexpected text field"
        ),
        Value::Array(values) => values.iter().for_each(assert_public_query),
        Value::Object(fields) => {
            for (key, value) in fields {
                assert!(
                    [
                        "to",
                        "from",
                        "data",
                        "input",
                        "blockHash",
                        "blockNumber",
                        "requireCanonical",
                        "address",
                        "fromBlock",
                        "toBlock",
                        "topics"
                    ]
                    .contains(&key.as_str()),
                    "RPC query contains an unexpected object field"
                );
                assert_public_query(value);
            }
        }
    }
}

fn check_rpc_audit(audit: &RpcAudit, config: &Value, calldata: &[u8]) {
    assert!(
        !audit.overflow.load(Ordering::SeqCst),
        "RPC audit was truncated"
    );
    let requests = audit.requests.lock().unwrap();
    assert!(!requests.is_empty());
    let mut broadcasts = 0;
    for request in requests.iter() {
        let envelope = request.as_object().expect("RPC envelope");
        assert!(envelope
            .keys()
            .all(|key| ["jsonrpc", "id", "method", "params"].contains(&key.as_str())));
        assert_eq!(request["jsonrpc"], "2.0");
        assert!(
            request["id"].is_u64(),
            "RPC identifier is not a public numeric counter"
        );
        let empty = Vec::new();
        let params = request.get("params").map_or(&empty, |params| {
            params.as_array().expect("RPC parameters must be an array")
        });
        match request["method"].as_str().expect("RPC method") {
            "eth_sendRawTransaction" => {
                broadcasts += 1;
                assert_eq!(params.len(), 1);
                let raw =
                    hex::decode(params[0].as_str().unwrap().trim_start_matches("0x")).unwrap();
                let signed = SignedTransaction::from_raw(&raw).unwrap();
                assert_eq!(signed.chain_id(), 31337);
                assert_eq!(format!("0x{}", hex::encode(signed.to())), config["pool"]);
                assert_eq!(
                    format!("0x{}", hex::encode(signed.sender())),
                    config["signer_address"]
                );
                assert_eq!(signed.params().value, 0);
                assert!(
                    signed.input() == calldata,
                    "RPC broadcast contains unexpected calldata"
                );
            }
            "eth_estimateGas" => {
                assert!((1..=2).contains(&params.len()));
                let call = params[0].as_object().expect("gas estimate call");
                assert!(call
                    .keys()
                    .all(|key| ["from", "to", "data", "input"].contains(&key.as_str())));
                assert_eq!(params[0]["to"], config["pool"]);
                assert_eq!(params[0]["from"], config["signer_address"]);
                let data = params[0]
                    .get("data")
                    .or_else(|| params[0].get("input"))
                    .unwrap();
                let data = hex::decode(data.as_str().unwrap().trim_start_matches("0x")).unwrap();
                assert!(
                    data == calldata,
                    "gas estimate contains unexpected calldata"
                );
                if params.len() == 2 {
                    assert_public_query(&params[1]);
                }
            }
            "eth_call" => {
                assert_eq!(params.len(), 2);
                assert_public_query(&request["params"]);
                let encoded = params[0]
                    .get("data")
                    .or_else(|| params[0].get("input"))
                    .unwrap();
                let data = hex::decode(encoded.as_str().unwrap().trim_start_matches("0x")).unwrap();
                let allowed = [
                    ("asset()", 4),
                    ("verifierVersion()", 4),
                    ("currentRoot()", 4),
                    ("nextLeafIndex()", 4),
                    ("poseidonTwo()", 4),
                    ("depositVerifier()", 4),
                    ("transferVerifier()", 4),
                    ("withdrawVerifier()", 4),
                    ("consumedDeals(uint256)", 36),
                    ("consumedNotes(uint256)", 36),
                    ("knownRoots(uint256)", 36),
                    ("insertedCommitments(uint256)", 36),
                    ("balanceOf(address)", 36),
                ];
                assert!(
                    allowed.iter().any(|(name, bytes)| {
                        data.len() == *bytes
                            && data[..4] == erebus_core::suite::keccak256(&[name.as_bytes()])[..4]
                    }),
                    "RPC eth_call contains an unexpected calldata schema"
                );
            }
            "eth_chainId" | "eth_blockNumber" => assert!(params.is_empty()),
            "eth_getLogs" | "eth_getTransactionReceipt" | "eth_getTransactionByHash" => {
                assert_eq!(params.len(), 1);
                assert_public_query(&request["params"]);
            }
            "eth_getCode"
            | "eth_getBalance"
            | "eth_getTransactionCount"
            | "eth_getBlockByNumber"
            | "eth_getBlockByHash" => {
                assert_eq!(params.len(), 2);
                assert_public_query(&request["params"]);
            }
            method => {
                assert!(
                    method.len() <= 64
                        && method
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                );
                panic!("shielded operation used unexpected RPC method {method}");
            }
        }
    }
    assert_eq!(broadcasts, 1);
    eprintln!(
        "shielded RPC schema audit: {} bounded requests, one proof-only broadcast",
        requests.len()
    );
}

#[test]
fn rpc_query_audit_rejects_private_payload_shapes_and_truncated_capture() {
    assert_public_query(&json!([
        {"to":format!("0x{}", "11".repeat(20)), "data":"0x12345678"},
        {"blockHash":format!("0x{}", "22".repeat(32)), "requireCanonical":true}
    ]));
    for query in [
        json!("PRIVATE_SPENDING_SECRET"),
        json!({"witness": {"amount":"70", "spend_secret":"PRIVATE_SPENDING_SECRET"}}),
        json!({"data":format!("0x{}", "33".repeat(200))}),
        json!({"blockHash":"/private/operator/wallet.key"}),
    ] {
        assert!(std::panic::catch_unwind(|| assert_public_query(&query)).is_err());
    }
    let audit = RpcAudit::default();
    audit.overflow.store(true, Ordering::SeqCst);
    assert!(std::panic::catch_unwind(|| check_rpc_audit(&audit, &Value::Null, &[])).is_err());
}

#[test]
fn rpc_audit_checks_estimation_and_broadcast_against_one_public_call() {
    use erebus_evm::chain::{build_signed_call, Eip1559Fees, TransactionParams};
    let key = TransactionKey::from_bytes(&[24; 32]).unwrap();
    let pool = [8; 20];
    // ABI-shaped dummy proof: this unit test checks the auditor, not proof validity.
    let mut calldata = vec![0; 4 + 19 * 32];
    calldata[..4].copy_from_slice(
        &erebus_core::suite::keccak256(&[
            b"transferPrivate(uint256[2],uint256[2][2],uint256[2],uint256[11])",
        ])[..4],
    );
    let signed = build_signed_call(
        &key,
        31337,
        pool,
        calldata.clone(),
        TransactionParams {
            nonce: 0,
            gas_limit: 3_000_000,
            value: 0,
            fees: Eip1559Fees::new(2_000_000_000, 1_000_000_000).unwrap(),
        },
    )
    .unwrap();
    let config = json!({"pool":format!("0x{}", hex::encode(pool)),
        "signer_address":format!("0x{}", hex::encode(key.address()))});
    let requests = vec![
        json!({"jsonrpc":"2.0", "id":1, "method":"eth_chainId"}),
        json!({"jsonrpc":"2.0", "id":2, "method":"eth_estimateGas", "params":[{
            "from":config["signer_address"], "to":config["pool"], "input":format!("0x{}", hex::encode(&calldata))}]}),
        json!({"jsonrpc":"2.0", "id":3, "method":"eth_sendRawTransaction", "params":[format!("0x{}",hex::encode(signed.raw()))]}),
    ];
    let audit = RpcAudit {
        requests: std::sync::Mutex::new(requests.clone()),
        overflow: AtomicBool::new(false),
    };
    check_rpc_audit(&audit, &config, &calldata);
    for mutation in 0..4 {
        let mut requests = requests.clone();
        match mutation {
            0 => requests[1]["params"][0]["witness"] = json!("PRIVATE_INPUT"),
            1 => requests[1]["params"][0]["input"] = json!("0x12345678"),
            2 => requests[0]["id"] = json!("PRIVATE_IDENTIFIER"),
            _ => requests[0]["params"] = json!(["0x11"]),
        }
        let audit = RpcAudit {
            requests: std::sync::Mutex::new(requests),
            overflow: AtomicBool::new(false),
        };
        assert!(std::panic::catch_unwind(|| check_rpc_audit(&audit, &config, &calldata)).is_err());
    }
}

/// A funded suite-2 buyer, a seller, and the buyer's shielded payment configuration on a fresh
/// Anvil. Nothing is negotiated yet. `block_time` lets finality advance without test-driven mining.
pub(super) struct Deployed {
    pub(super) fixture: Fixture,
    pub(super) config: Value,
    pub(super) config_path: PathBuf,
    pub(super) buyer_root: PathBuf,
    pub(super) url: String,
    pub(super) client: reqwest::Client,
    pub(super) wallet: WalletStore,
    pub(super) token_bytes: [u8; 20],
    pub(super) sends: Arc<AtomicUsize>,
    pub(super) corrupt_consumed: Arc<AtomicBool>,
    rpc_audit: Arc<RpcAudit>,
    _anvil: ServiceProcess,
    tasks: [tokio::task::JoinHandle<()>; 3],
}

impl Drop for Deployed {
    fn drop(&mut self) {
        self.tasks.iter().for_each(|task| task.abort());
    }
}

impl Deployed {
    pub(super) fn stop_chain(&mut self) {
        let _ = self._anvil.0.kill();
        let _ = self._anvil.0.wait();
    }
}

pub(super) async fn deploy(lose_broadcast: bool, block_time: Option<u64>) -> Deployed {
    deploy_profile(lose_broadcast, block_time, |_| {}).await
}

pub(super) async fn deploy_profile(
    lose_broadcast: bool,
    block_time: Option<u64>,
    configure: impl FnOnce(&mut AgreementTerms),
) -> Deployed {
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let anvil = ServiceProcess(
        Command::new("anvil")
            .args([
                "--port",
                &port.to_string(),
                "--chain-id",
                "31337",
                "--slots-in-an-epoch",
                "1",
                "--silent",
            ])
            .args(
                block_time
                    .map(|seconds| ["--block-time".to_string(), seconds.to_string()])
                    .into_iter()
                    .flatten(),
            )
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
    for _ in 0..100 {
        if client
            .post(&url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}))
            .send()
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
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
    rpc(&client, &url, "anvil_mine", json!(["0x3"])).await;
    let pool = deployment["pool"].as_str().unwrap();
    let token = deployment["token"].as_str().unwrap();
    let pool_bytes: [u8; 20] = hex::decode(pool.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();
    let token_bytes: [u8; 20] = hex::decode(token.trim_start_matches("0x"))
        .unwrap()
        .try_into()
        .unwrap();
    let mut template = terms(2, 60);
    template.domain = DeploymentDomain {
        namespace: ChainNamespace::parse("eip155:31337").unwrap(),
        settlement_contract: Some(AddressBytes::new(pool_bytes.to_vec()).unwrap()),
        pool: Some(AddressBytes::new(pool_bytes.to_vec()).unwrap()),
        verifier_version: 2,
    };
    template.asset = AssetId::parse(&format!("eip155:31337/erc20:{token}")).unwrap();
    template.service.unit = "snapshot".into();
    template.service.quantity = BaseUnits::new(1);
    template.service.fulfillment_method = "http-access-v1".into();
    template.service.fulfillment_digest = Sha256::digest(native_product::PAYLOAD).into();
    configure(&mut template);
    let fixture = Fixture::with_terms(template);
    let buyer_root = fixture.root.path().join("buyer");
    private(&buyer_root.join("wallet.key"), &[9; 32]);
    let wallet_path = buyer_root.join("state/wallet/notes.enc");
    let wallet = WalletStore::new(
        &wallet_path,
        WalletDomain {
            chain_id: 31337,
            pool: pool_bytes,
            verifier_version: 2,
        },
        [9; 32],
    )
    .unwrap();
    let note_domain = wallet.domain().note_domain().unwrap();
    wallet
        .update(|wallet| {
            wallet.add(OwnedNote::new(
                token_bytes,
                150,
                derive_key(&[61; 32]).unwrap(),
                [6; 32],
                [7; 32],
                note_domain,
            )?)
        })
        .unwrap();
    let key = TransactionKey::from_bytes(&[24; 32]).unwrap();
    private(&buyer_root.join("transaction.key"), &[24; 32]);
    let signer = format!("0x{}", hex::encode(key.address()));
    rpc(
        &client,
        &url,
        "anvil_setBalance",
        json!([signer, "0x56bc75e2d63100000"]),
    )
    .await;
    rpc(&client, &url, "anvil_mine", json!(["0x3"])).await;
    let sends = Arc::new(AtomicUsize::new(0));
    let corrupt_consumed = Arc::new(AtomicBool::new(false));
    let proxy = Proxy {
        upstream: url.clone(),
        client: client.clone(),
        lose_broadcast: Arc::new(AtomicBool::new(lose_broadcast)),
        sends: sends.clone(),
        minimum_log_block: Arc::new(std::sync::Mutex::new(None)),
        corrupt_code: Arc::new(AtomicBool::new(false)),
        corrupt_consumed: corrupt_consumed.clone(),
        broadcast_hashes: Arc::new(std::sync::Mutex::new(Vec::new())),
    };
    let rpc_audit = Arc::new(RpcAudit::default());
    let mut honest_proxy = proxy.clone();
    honest_proxy.lose_broadcast = Arc::new(AtomicBool::new(false));
    honest_proxy.corrupt_code = Arc::new(AtomicBool::new(false));
    honest_proxy.corrupt_consumed = Arc::new(AtomicBool::new(false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let honest_url = format!("http://{}", listener.local_addr().unwrap());
    let honest_state = (honest_proxy, rpc_audit.clone());
    let honest_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/", post(audited_forward))
                .with_state(honest_state),
        )
        .await
        .unwrap();
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer_url = format!("http://{}", listener.local_addr().unwrap());
    let proxy_state = (proxy, rpc_audit.clone());
    let proxy_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/", post(audited_forward))
                .with_state(proxy_state),
        )
        .await
        .unwrap();
    });
    // The primary sends through the lossy proxy; the peer reads the honest upstream.
    let build = Path::new(deployment["artifact_build"].as_str().unwrap());
    let bodies: std::collections::HashMap<String, Vec<u8>> = [
        ("transfer.wasm", build.join("transfer_js/transfer.wasm")),
        ("transfer.r1cs", build.join("transfer.r1cs")),
        ("transfer.zkey", build.join("transfer.zkey")),
    ]
    .into_iter()
    .map(|(name, path)| (name.into(), fs::read(path).unwrap()))
    .collect();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let artifact_url = format!("http://{}", listener.local_addr().unwrap());
    let artifact_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route(
                    "/{name}",
                    get(
                        |State(bodies): State<Arc<std::collections::HashMap<String, Vec<u8>>>>,
                         RoutePath(name): RoutePath<String>| async move {
                            bodies.get(&name).cloned().unwrap_or_default()
                        },
                    ),
                )
                .with_state(Arc::new(bodies)),
        )
        .await
        .unwrap();
    });
    let circuits: Vec<Value> = ["deposit","transfer","withdraw"].into_iter().map(|kind| {
        let mut circuit = json!({"kind":kind});
        for (name,path) in [("wasm",build.join(format!("{kind}_js/{kind}.wasm"))),("r1cs",build.join(format!("{kind}.r1cs"))),("zkey",build.join(format!("{kind}.zkey")))] {
            let body = fs::read(path).unwrap();
            circuit[name] = json!({"url":format!("{artifact_url}/{kind}.{name}"),"bytes":body.len(),"sha256":hex::encode(Sha256::digest(&body))});
        }
        circuit
    }).collect();
    let manifest = serde_json::to_vec(&json!({"version":1,"chain_id":31337,"settlement_contract":pool,"verifier_version":2,"test_only":true,"circuits":circuits})).unwrap();
    private(&buyer_root.join("manifest.json"), &manifest);
    let mut config = deployment.clone();
    config.as_object_mut().unwrap().remove("artifact_build");
    config["version"] = json!(1);
    config["mode"] = json!("shielded");
    config.as_object_mut().unwrap().remove("token");
    config.as_object_mut().unwrap().remove("wmon");
    config.as_object_mut().unwrap().remove("asset_mode");
    config["state_root"] = json!(buyer_root.join("state"));
    config["namespace"] = json!("eip155:31337");
    config["verifier_version"] = json!(2);
    config["rpc_url"] = json!(peer_url);
    config["peer_rpc_url"] = json!(honest_url);
    // Both are fixed before negotiation: the buyer's suite-2 agreement key file holds [61; 32].
    config["buyer_key_hex"] = json!(hex::encode(derive_key(&[61; 32]).unwrap()));
    config["asset"] = json!(fixture.template.asset.to_string());
    config["signer_address"] = json!(signer);
    config["signer_journal_root"] = json!(fixture.root.path().join("shared-signer"));
    config["transaction_key_file"] = json!(buyer_root.join("transaction.key"));
    config["wallet_file"] = json!(wallet_path);
    config["wallet_key_file"] = json!(buyer_root.join("wallet.key"));
    config["manifest_file"] = json!(buyer_root.join("manifest.json"));
    config["manifest_sha256"] = json!(hex::encode(Sha256::digest(&manifest)));
    config["artifact_cache"] = json!(buyer_root.join("artifact-cache"));
    config["allow_test_artifacts"] = json!(true);
    config["allow_loopback_http"] = json!(true);
    config["maximum_price"] = json!("75");
    config["gas_limit"] = json!(3_000_000);
    config["max_fee_per_gas"] = json!("2000000000");
    config["max_priority_fee_per_gas"] = json!("1000000000");
    config["timeout_seconds"] = json!(10);
    config["max_scan_blocks"] = json!(1000);
    let config_path = buyer_root.join("payment.json");
    private(&config_path, &serde_json::to_vec(&config).unwrap());
    Deployed {
        fixture,
        config,
        config_path,
        buyer_root,
        url,
        client,
        wallet,
        token_bytes,
        sends,
        corrupt_consumed,
        rpc_audit,
        _anvil: anvil,
        tasks: [proxy_task, honest_task, artifact_task],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, Node dependencies and existing M5 prototype proving artifacts"]
async fn negotiated_sponsored_payment_delegates_before_transport_and_never_falls_back() {
    use erebus_core::settlement::SettlementContext;
    use erebus_shielded_prover::{
        index_store::IndexDomain,
        relay_admission::{RelayAdmissionPolicy, TransferVerifier},
        relay_http::{RelayHttpPolicy, RelayHttpService},
        relay_service::{SponsorConfig, SponsoredRelay},
        rpc::PoolRuntimePins,
    };
    let deployed = deploy(false, None).await;
    let (buyer, _) = deployed.fixture.pair(false);
    let selected =
        SelectedAgreement::decode(&fs::read(buyer["evidence_file"].as_str().unwrap()).unwrap())
            .unwrap();
    let context = SettlementContext {
        domain: selected.terms.domain.clone(),
        asset: selected.terms.asset.clone(),
        suite_id: 2,
        mode: selected.terms.settlement_mode,
        required_guarantees: selected.terms.required_guarantees,
        require_local_proving: true,
    };
    let mut config = deployed.config.clone();
    let address = |name: &str| -> [u8; 20] {
        hex::decode(config[name].as_str().unwrap().trim_start_matches("0x"))
            .unwrap()
            .try_into()
            .unwrap()
    };
    let pin = |name: &str| -> [u8; 32] {
        hex::decode(config[name].as_str().unwrap().trim_start_matches("0x"))
            .unwrap()
            .try_into()
            .unwrap()
    };
    let sponsor_key = [24; 32];
    let sponsor = TransactionKey::from_bytes(&sponsor_key).unwrap().address();
    let sponsor_hex = format!("0x{}", hex::encode(sponsor));
    rpc(
        &deployed.client,
        &deployed.url,
        "anvil_setBalance",
        json!([sponsor_hex, "0xde0b6b3a7640000"]),
    )
    .await;
    let policy = RelayAdmissionPolicy {
        context,
        deployment: IndexDomain {
            chain_id: 31337,
            pool: address("pool"),
            first_block: config["first_block"].as_u64().unwrap(),
            first_hash: pin("first_hash"),
        },
        runtime_pins: PoolRuntimePins {
            pool: pin("runtime_keccak256"),
            poseidon: pin("poseidon_keccak256"),
            deposit: pin("deposit_verifier_keccak256"),
            transfer: pin("transfer_verifier_keccak256"),
            withdraw: pin("withdraw_verifier_keccak256"),
        },
        sponsor,
        gas_limit: 3_000_000,
    };
    let zkey = fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../circuits/m5/build/transfer.zkey"),
    )
    .unwrap();
    let service = SponsoredRelay::connect(
        SponsorConfig {
            root: deployed.fixture.root.path().join("relay-state"),
            signer_root: deployed.fixture.root.path().join("relay-nonces"),
            admission: policy,
            fees: erebus_evm::chain::Eip1559Fees::new(2_000_000_000, 1_000_000_000).unwrap(),
            replacement_fees: None,
            rpc_url: config["rpc_url"].as_str().unwrap().into(),
            peer_rpc_url: config["peer_rpc_url"].as_str().unwrap().into(),
            timeout: Duration::from_secs(10),
        },
        TransferVerifier::from_zkey_bytes(&zkey, &hex::encode(Sha256::digest(&zkey))).unwrap(),
        &sponsor_key,
    )
    .await
    .unwrap();
    let transport = RelayHttpService::new(
        service,
        [7; 32],
        &deployed.fixture.root.path().join("relay-quota"),
        RelayHttpPolicy {
            requests_per_window: 50,
            window_seconds: 60,
            maximum_gas_budget: 6_000_000_000_000_000,
            maximum_operations: 1,
            timeout_seconds: 60,
        },
    )
    .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/relay", listener.local_addr().unwrap());
    let token = deployed.buyer_root.join("relay-token");
    private(&token, &[7; 32]);
    config["relay"] = json!({
        "url": endpoint, "identity": "01".repeat(32),
        "authentication_key_file": token,
        "sponsor_address": sponsor_hex, "timeout_seconds": 1,
        "allow_loopback_http": true,
    });
    config["transaction_key_file"] = Value::Null;
    let unavailable_config = deployed.buyer_root.join("sponsored-unavailable.json");
    private(&unavailable_config, &serde_json::to_vec(&config).unwrap());
    let binary = native_binary("erebus-payment", env!("CARGO_BIN_EXE_erebus-payment"));
    // No server is accepting requests yet. Delegation must already be durable.
    let (_, first) = run_payment(
        &binary,
        deployed.fixture.root.path(),
        &unavailable_config,
        "settle",
    )
    .await;
    assert_eq!(first["submission_delegated"], true, "{first}");
    assert_eq!(first["submitted_this_call"], false);
    assert_eq!(first["relay_status"], "unavailable");
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 0);
    let server = tokio::spawn(async move {
        axum::serve(listener, transport.router()).await.unwrap();
    });
    config["relay"]["timeout_seconds"] = json!(60);
    let sponsored_config = deployed.buyer_root.join("sponsored.json");
    private(&sponsored_config, &serde_json::to_vec(&config).unwrap());
    let (_, sent) = run_payment(
        &binary,
        deployed.fixture.root.path(),
        &sponsored_config,
        "settle",
    )
    .await;
    assert_eq!(sent["submission_delegated"], true, "{sent}");
    assert_eq!(sent["submitted_this_call"], false);
    assert_eq!(sent["payment_verified"], false);
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 1, "{sent}");
    let tx = sent["relay_transaction_hash"].as_str().unwrap();
    // Removing the relay and restoring a buyer key must not authorize direct signing.
    config.as_object_mut().unwrap().remove("relay");
    config["transaction_key_file"] = deployed.config["transaction_key_file"].clone();
    let direct_config = deployed.buyer_root.join("sponsored-no-fallback.json");
    private(&direct_config, &serde_json::to_vec(&config).unwrap());
    let (_, fenced) = run_payment(
        &binary,
        deployed.fixture.root.path(),
        &direct_config,
        "settle",
    )
    .await;
    assert_eq!(fenced["submitted_this_call"], false, "{fenced}");
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 1);
    rpc(
        &deployed.client,
        &deployed.url,
        "anvil_mine",
        json!(["0x4"]),
    )
    .await;
    let receipt = rpc(
        &deployed.client,
        &deployed.url,
        "eth_getTransactionReceipt",
        json!([tx]),
    )
    .await;
    assert_eq!(receipt["from"], sponsor_hex);
    assert_eq!(receipt["status"], "0x1");
    let (ok, recovered) = run_payment(
        &binary,
        deployed.fixture.root.path(),
        &direct_config,
        "observe",
    )
    .await;
    assert!(ok, "{recovered}");
    assert_eq!(recovered["payment_verified"], true);
    assert_eq!(recovered["submitted_this_call"], false);
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 1);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, Node dependencies and existing M5 prototype proving artifacts"]
async fn sponsored_payment_failures_keep_delegation_and_never_fall_back() {
    use erebus_core::settlement::SettlementContext;
    use erebus_shielded_prover::{
        index_store::IndexDomain,
        relay_admission::{RelayAdmissionPolicy, TransferVerifier},
        relay_http::{RelayHttpPolicy, RelayHttpService},
        relay_service::{SponsorConfig, SponsoredRelay},
        rpc::PoolRuntimePins,
    };
    let deployed = deploy(false, None).await;
    let (buyer, _) = deployed.fixture.pair(false);
    let selected =
        SelectedAgreement::decode(&fs::read(buyer["evidence_file"].as_str().unwrap()).unwrap())
            .unwrap();
    let context = SettlementContext {
        domain: selected.terms.domain.clone(),
        asset: selected.terms.asset.clone(),
        suite_id: 2,
        mode: selected.terms.settlement_mode,
        required_guarantees: selected.terms.required_guarantees,
        require_local_proving: true,
    };
    let mut config = deployed.config.clone();
    let address = |name: &str| -> [u8; 20] {
        hex::decode(config[name].as_str().unwrap().trim_start_matches("0x"))
            .unwrap()
            .try_into()
            .unwrap()
    };
    let pin = |name: &str| -> [u8; 32] {
        hex::decode(config[name].as_str().unwrap().trim_start_matches("0x"))
            .unwrap()
            .try_into()
            .unwrap()
    };
    let sponsor_key = [24; 32];
    let sponsor = TransactionKey::from_bytes(&sponsor_key).unwrap().address();
    let sponsor_hex = format!("0x{}", hex::encode(sponsor));
    rpc(
        &deployed.client,
        &deployed.url,
        "anvil_setBalance",
        json!([sponsor_hex, "0xde0b6b3a7640000"]),
    )
    .await;
    let policy = RelayAdmissionPolicy {
        context,
        deployment: IndexDomain {
            chain_id: 31337,
            pool: address("pool"),
            first_block: config["first_block"].as_u64().unwrap(),
            first_hash: pin("first_hash"),
        },
        runtime_pins: PoolRuntimePins {
            pool: pin("runtime_keccak256"),
            poseidon: pin("poseidon_keccak256"),
            deposit: pin("deposit_verifier_keccak256"),
            transfer: pin("transfer_verifier_keccak256"),
            withdraw: pin("withdraw_verifier_keccak256"),
        },
        sponsor,
        gas_limit: 3_000_000,
    };
    let zkey = fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../circuits/m5/build/transfer.zkey"),
    )
    .unwrap();
    let service = SponsoredRelay::connect(
        SponsorConfig {
            root: deployed.fixture.root.path().join("relay-state"),
            signer_root: deployed.fixture.root.path().join("relay-nonces"),
            admission: policy,
            fees: erebus_evm::chain::Eip1559Fees::new(2_000_000_000, 1_000_000_000).unwrap(),
            replacement_fees: None,
            rpc_url: config["rpc_url"].as_str().unwrap().into(),
            peer_rpc_url: config["peer_rpc_url"].as_str().unwrap().into(),
            timeout: Duration::from_secs(10),
        },
        TransferVerifier::from_zkey_bytes(&zkey, &hex::encode(Sha256::digest(&zkey))).unwrap(),
        &sponsor_key,
    )
    .await
    .unwrap();
    let transport = RelayHttpService::new(
        service,
        [7; 32],
        &deployed.fixture.root.path().join("relay-quota"),
        RelayHttpPolicy {
            requests_per_window: 50,
            window_seconds: 60,
            maximum_gas_budget: 6_000_000_000_000_000,
            maximum_operations: 1,
            timeout_seconds: 60,
        },
    )
    .unwrap();
    // One fixed port is reused for every server so the delegated destination never changes.
    let port = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let endpoint = format!("http://127.0.0.1:{port}/v1/relay");
    let token = deployed.buyer_root.join("relay-token");
    private(&token, &[7; 32]);
    config["relay"] = json!({
        "url": endpoint, "identity": "01".repeat(32),
        "authentication_key_file": token,
        "sponsor_address": sponsor_hex, "timeout_seconds": 1,
        "allow_loopback_http": true,
    });
    config["transaction_key_file"] = Value::Null;
    let binary = native_binary("erebus-payment", env!("CARGO_BIN_EXE_erebus-payment"));

    // 1. A service that accepts the connection and never answers: bounded timeout, no fallback.
    let hang = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    let hang_task = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = hang.accept().await {
            held.push(socket);
        }
    });
    let timeout_config = deployed.buyer_root.join("sponsored-timeout.json");
    private(&timeout_config, &serde_json::to_vec(&config).unwrap());
    let (_, timed) = run_payment(
        &binary,
        deployed.fixture.root.path(),
        &timeout_config,
        "settle",
    )
    .await;
    assert_eq!(timed["submission_delegated"], true, "{timed}");
    assert_eq!(timed["relay_status"], "unavailable", "{timed}");
    assert_eq!(timed["submitted_this_call"], false, "{timed}");
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 0);
    hang_task.abort();
    let _ = hang_task.await;

    // 2. A live service that refuses the request: still delegated, still no fallback.
    let refused = axum::Router::new().route(
        "/v1/relay",
        axum::routing::post(|| async { axum::http::StatusCode::SERVICE_UNAVAILABLE }),
    );
    let refused_listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    let refused_task = tokio::spawn(async move {
        axum::serve(refused_listener, refused).await.unwrap();
    });
    config["relay"]["timeout_seconds"] = json!(60);
    let refused_config = deployed.buyer_root.join("sponsored-refused.json");
    private(&refused_config, &serde_json::to_vec(&config).unwrap());
    let (_, refused_reply) = run_payment(
        &binary,
        deployed.fixture.root.path(),
        &refused_config,
        "settle",
    )
    .await;
    assert_eq!(
        refused_reply["submission_delegated"], true,
        "{refused_reply}"
    );
    assert_eq!(
        refused_reply["relay_status"], "unavailable",
        "{refused_reply}"
    );
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 0);
    refused_task.abort();
    let _ = refused_task.await;

    // 3. Missing relay credentials: the durable delegation exists before the token read.
    config["relay"]["authentication_key_file"] =
        json!(deployed.buyer_root.join("missing-relay-token"));
    let missing_config = deployed.buyer_root.join("sponsored-missing-key.json");
    private(&missing_config, &serde_json::to_vec(&config).unwrap());
    let (ok, missing) = run_payment(
        &binary,
        deployed.fixture.root.path(),
        &missing_config,
        "settle",
    )
    .await;
    assert!(!ok, "{missing}");
    assert_eq!(missing["status"], "error", "{missing}");
    assert!(!missing.to_string().contains("missing-relay-token"));
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 0);

    // 4. Restart with the relay removed and the buyer key restored: the delegation fences
    // direct signing, even though every sponsored attempt so far failed.
    config.as_object_mut().unwrap().remove("relay");
    config["transaction_key_file"] = deployed.config["transaction_key_file"].clone();
    let direct_config = deployed
        .buyer_root
        .join("sponsored-failure-no-fallback.json");
    private(&direct_config, &serde_json::to_vec(&config).unwrap());
    let (_, fenced) = run_payment(
        &binary,
        deployed.fixture.root.path(),
        &direct_config,
        "settle",
    )
    .await;
    assert_eq!(fenced["submitted_this_call"], false, "{fenced}");
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 0);

    // 5. The honest service on the same delegated destination submits exactly once.
    let honest = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .unwrap();
    let server = tokio::spawn(async move {
        axum::serve(honest, transport.router()).await.unwrap();
    });
    config["relay"] = json!({
        "url": endpoint, "identity": "01".repeat(32),
        "authentication_key_file": token,
        "sponsor_address": sponsor_hex, "timeout_seconds": 60,
        "allow_loopback_http": true,
    });
    config["transaction_key_file"] = Value::Null;
    let sponsored_config = deployed.buyer_root.join("sponsored-failure-recovery.json");
    private(&sponsored_config, &serde_json::to_vec(&config).unwrap());
    let (_, sent) = run_payment(
        &binary,
        deployed.fixture.root.path(),
        &sponsored_config,
        "settle",
    )
    .await;
    assert_eq!(sent["submission_delegated"], true, "{sent}");
    assert_eq!(sent["submitted_this_call"], false, "{sent}");
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 1, "{sent}");
    let tx = sent["relay_transaction_hash"].as_str().unwrap();
    rpc(
        &deployed.client,
        &deployed.url,
        "anvil_mine",
        json!(["0x4"]),
    )
    .await;
    let receipt = rpc(
        &deployed.client,
        &deployed.url,
        "eth_getTransactionReceipt",
        json!([tx]),
    )
    .await;
    assert_eq!(receipt["from"], sponsor_hex);
    assert_eq!(receipt["status"], "0x1");
    let (ok, recovered) = run_payment(
        &binary,
        deployed.fixture.root.path(),
        &direct_config,
        "observe",
    )
    .await;
    assert!(ok, "{recovered}");
    assert_eq!(recovered["payment_verified"], true);
    assert_eq!(recovered["submitted_this_call"], false);
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 1);
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, Node dependencies and existing M5 prototype proving artifacts"]
async fn negotiated_shielded_payment_proves_locally_and_recovers_without_a_second_send() {
    let deployed = deploy(true, None).await;
    let Deployed {
        fixture,
        config,
        config_path,
        buyer_root,
        url,
        client,
        wallet,
        token_bytes,
        sends,
        corrupt_consumed,
        ..
    } = &deployed;
    let (fixture, config, config_path, buyer_root, url, client, wallet, token_bytes) = (
        fixture,
        config,
        config_path,
        buyer_root,
        url,
        client,
        wallet,
        *token_bytes,
    );
    let (buyer, seller) = fixture.pair(false);
    let selected =
        SelectedAgreement::decode(&fs::read(buyer["evidence_file"].as_str().unwrap()).unwrap())
            .unwrap();
    assert_ne!(selected.terms.transcript_root, [0; 32]);
    assert_eq!(selected.terms.amount.get(), 70);
    assert_eq!(selected.messages.len(), 4);
    verify_selected_agreement(&selected).unwrap();
    // Seller consent must already imply durable recipient spending material.
    let seller_config: Value =
        serde_json::from_slice(&fs::read(&fixture.seller_config).unwrap()).unwrap();
    let seller_wallet_path = PathBuf::from(seller_config["seller_wallet_file"].as_str().unwrap());
    let seller_wallet_key: [u8; 32] =
        fs::read(seller_config["seller_wallet_key_file"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
    let seller_domain = WalletDomain {
        chain_id: 31337,
        pool: hex::decode(config["pool"].as_str().unwrap().trim_start_matches("0x"))
            .unwrap()
            .try_into()
            .unwrap(),
        verifier_version: 2,
    };
    let seller_wallet =
        WalletStore::new(&seller_wallet_path, seller_domain, seller_wallet_key).unwrap();
    let expected = seller_wallet.snapshot().unwrap();
    assert_eq!(expected.notes().len(), 1);
    assert_eq!(expected.notes()[0].amount(), 70);
    assert!(expected.notes()[0].inclusion().is_none());
    let payment_commitment = expected.notes()[0].commitment();
    let recipient_backup = seller_wallet_path.with_extension("backup.enc");
    fs::copy(&seller_wallet_path, &recipient_backup).unwrap();
    drop(expected);
    drop(seller_wallet);
    let binary = fixture.root.path().join("erebus-payment");
    fs::copy(
        native_binary("erebus-payment", env!("CARGO_BIN_EXE_erebus-payment")),
        &binary,
    )
    .unwrap();
    let (_, funding) = run_payment(&binary, fixture.root.path(), config_path, "funding").await;
    assert_eq!(funding["status"], "ready", "{funding}");
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    assert!(!buyer_root.join("artifact-cache").exists());
    let mut bad = config.clone();
    bad["transfer_verifier_keccak256"] = json!("01".repeat(32));
    let bad_path = buyer_root.join("bad-pins.json");
    private(&bad_path, &serde_json::to_vec(&bad).unwrap());
    let (ok, rejected) = run_payment(&binary, fixture.root.path(), &bad_path, "settle").await;
    assert!(!ok, "{rejected}");
    assert_eq!(rejected["payment_verified"], false);
    bad = config.clone();
    bad["allow_test_artifacts"] = json!(false);
    let bad_path = buyer_root.join("no-test-opt-in.json");
    private(&bad_path, &serde_json::to_vec(&bad).unwrap());
    let (ok, rejected) = run_payment(&binary, fixture.root.path(), &bad_path, "settle").await;
    assert!(!ok, "{rejected}");
    assert!(wallet
        .snapshot()
        .unwrap()
        .select(&token_bytes, 150)
        .is_some());
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    let (_, submitted) = run_payment(&binary, fixture.root.path(), config_path, "settle").await;
    assert_eq!(submitted["submitted_this_call"], true, "{submitted}");
    assert_eq!(submitted["payment_verified"], false);
    assert!(
        submitted["measurements_ms"]["proof_preparation"]
            .as_u64()
            .unwrap()
            > 0
    );
    eprintln!(
        "native shielded debug-mode timings (Anvil, test-only setup): {}",
        submitted["measurements_ms"]
    );
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    for field in [
        "amount",
        "signature",
        "blinding",
        "witness",
        "spend_secret",
        "rpc_url",
    ] {
        assert!(submitted.get(field).is_none());
    }
    fs::remove_file(buyer_root.join("transaction.key")).unwrap();
    fs::remove_file(buyer_root.join("manifest.json")).unwrap();
    fs::rename(
        buyer_root.join("state/transcripts"),
        buyer_root.join("transcripts-offline"),
    )
    .unwrap();
    fs::rename(
        buyer_root.join("artifact-cache"),
        buyer_root.join("artifacts-offline"),
    )
    .unwrap();
    rpc(client, url, "anvil_mine", json!(["0x4"])).await;
    corrupt_consumed.store(true, Ordering::SeqCst);
    let (ok, rejected) = run_payment(&binary, fixture.root.path(), config_path, "settle").await;
    assert!(!ok, "{rejected}");
    assert_eq!(rejected["payment_verified"], false);
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    corrupt_consumed.store(false, Ordering::SeqCst);
    let (ok, recovered) = run_payment(&binary, fixture.root.path(), config_path, "settle").await;
    assert!(ok, "{recovered}");
    assert_eq!(recovered["payment_verified"], true);
    assert_eq!(recovered["submitted_this_call"], false);
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    // Inspect chain-visible bytes, not merely the client's redacted response. The gas
    // payer, asset, expiry and commitment graph remain public; this is not anonymity.
    let tx_hash = submitted["transaction_hash"].as_str().unwrap();
    let transaction = rpc(client, url, "eth_getTransactionByHash", json!([tx_hash])).await;
    assert_eq!(transaction["to"], config["pool"]);
    let calldata = hex::decode(
        transaction["input"]
            .as_str()
            .unwrap()
            .trim_start_matches("0x"),
    )
    .unwrap();
    let selector = erebus_core::suite::keccak256(&[
        b"transferPrivate(uint256[2],uint256[2][2],uint256[2],uint256[11])",
    ]);
    assert_eq!(&calldata[..4], &selector[..4]);
    assert_eq!(calldata.len(), 4 + (8 + 11) * 32);
    let inputs: Vec<&[u8]> = calldata[4 + 8 * 32..].chunks_exact(32).collect();
    assert_eq!(&inputs[0][24..], &31337u64.to_be_bytes());
    assert_eq!(&inputs[1][12..], &seller_domain.pool);
    assert_eq!(&inputs[2][24..], &2u64.to_be_bytes());
    assert_eq!(&inputs[3][12..], &token_bytes);
    for (index, name) in [(4, "deal_commitment"), (5, "deal_nullifier")] {
        assert_eq!(
            inputs[index],
            hex::decode(submitted[name].as_str().unwrap().trim_start_matches("0x")).unwrap()
        );
    }
    let receipt = rpc(client, url, "eth_getTransactionReceipt", json!([tx_hash])).await;
    assert_eq!(receipt["status"], "0x1");
    let logs = receipt["logs"].as_array().unwrap();
    assert_eq!(logs.len(), 3);
    let topic = |signature: &[u8]| {
        format!(
            "0x{}",
            hex::encode(erebus_core::suite::keccak256(&[signature]))
        )
    };
    let inserted = topic(b"NoteInserted(uint256,uint256,uint256)");
    let transferred = topic(b"DealTransferred(uint256,uint256,uint256)");
    for (index, log) in logs.iter().enumerate() {
        assert_eq!(log["address"], config["pool"]);
        let topics = log["topics"].as_array().unwrap();
        if index < 2 {
            assert_eq!(topics.len(), 3);
            assert_eq!(topics[0], inserted);
            assert_eq!(log["data"].as_str().unwrap().len(), 66);
        } else {
            assert_eq!(topics.len(), 4);
            assert_eq!(topics[0], transferred);
            assert_eq!(log["data"], "0x");
            assert_eq!(
                topics[1].as_str().unwrap().trim_start_matches("0x"),
                hex::encode(inputs[4])
            );
            assert_eq!(
                topics[2].as_str().unwrap().trim_start_matches("0x"),
                hex::encode(inputs[5])
            );
            assert_eq!(
                topics[3].as_str().unwrap().trim_start_matches("0x"),
                hex::encode(inputs[7])
            );
        }
    }
    let notes = wallet.snapshot().unwrap();
    assert!(notes
        .notes()
        .iter()
        .any(|note| note.amount() == 80 && note.inclusion().is_some()));
    assert_eq!(notes.select(&token_bytes, 80).unwrap().amount(), 80);
    // Restart from a backup made before payment, with sender state unavailable.
    fs::remove_file(&seller_wallet_path).unwrap();
    fs::remove_file(seller_config["seller_spend_secret_file"].as_str().unwrap()).unwrap();
    let sender_offline = fixture.root.path().join("sender-offline");
    fs::rename(buyer_root, &sender_offline).unwrap();
    let restored = WalletStore::new(&recipient_backup, seller_domain, seller_wallet_key).unwrap();
    assert!(restored
        .snapshot()
        .unwrap()
        .select(&token_bytes, 70)
        .is_none());
    let index_domain = erebus_shielded_prover::index_store::IndexDomain {
        chain_id: 31337,
        pool: seller_domain.pool,
        first_block: config["first_block"].as_u64().unwrap(),
        first_hash: hex::decode(
            config["first_hash"]
                .as_str()
                .unwrap()
                .trim_start_matches("0x"),
        )
        .unwrap()
        .try_into()
        .unwrap(),
    };
    let index = erebus_shielded_prover::index_store::IndexStore::new(
        fixture.root.path().join("recipient-history.json"),
        index_domain,
    )
    .unwrap();
    let pool_rpc =
        erebus_shielded_prover::rpc::PoolRpc::new(url, 31337, seller_domain.pool).unwrap();
    erebus_shielded_prover::recovery::recover_finalized_wallet(&pool_rpc, &index, &restored)
        .await
        .unwrap();
    let recovered = restored.snapshot().unwrap();
    let received = recovered.select(&token_bytes, 70).unwrap();
    assert_eq!(received.commitment(), payment_commitment);
    assert_eq!(received.amount(), 70);
    assert!(received.inclusion().is_some());
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    fs::rename(&sender_offline, buyer_root).unwrap();
    native_product::finish(fixture, &buyer, &seller, config, client).await;
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    check_rpc_audit(&deployed.rpc_audit, config, &calldata);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, Node dependencies and existing M5 prototype proving artifacts"]
async fn recipient_spends_a_recovered_payment_in_a_second_negotiated_deal() {
    let deployed = deploy(false, None).await;
    let fixture = &deployed.fixture;
    let root = fixture.root.path();
    let binary = native_binary("erebus-payment", env!("CARGO_BIN_EXE_erebus-payment"));
    let (first_buyer, _) = fixture.pair(false);
    let (_, submitted) = run_payment(&binary, root, &deployed.config_path, "settle").await;
    assert_eq!(submitted["stage"], "Submitted", "{submitted}");
    assert_eq!(submitted["submitted_this_call"], true);
    rpc(
        &deployed.client,
        &deployed.url,
        "anvil_mine",
        json!(["0x4"]),
    )
    .await;
    let (ok, finalized) = run_payment(&binary, root, &deployed.config_path, "settle").await;
    assert!(ok, "{finalized}");
    assert_eq!(finalized["payment_verified"], true);
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 1);

    let original_buyer: Value =
        serde_json::from_slice(&fs::read(&fixture.buyer_config).unwrap()).unwrap();
    let original_seller: Value =
        serde_json::from_slice(&fs::read(&fixture.seller_config).unwrap()).unwrap();
    let mut buyer_config = original_seller.clone();
    buyer_config["role"] = json!("buyer");
    for field in [
        "seller_spend_secret_file",
        "seller_wallet_file",
        "seller_wallet_key_file",
    ] {
        buyer_config[field] = Value::Null;
    }
    let buyer_terms_path = root.join("seller/second-buyer.terms");
    let mut template = fixture.template.clone();
    template.amount = BaseUnits::new(60);
    private(&buyer_terms_path, &template.encode().unwrap());
    buyer_config["terms_template_file"] = json!(buyer_terms_path);
    let buyer_config_path = root.join("seller/second-buyer.json");
    private(
        &buyer_config_path,
        &serde_json::to_vec(&buyer_config).unwrap(),
    );

    let mut seller_config = original_buyer;
    seller_config["role"] = json!("seller");
    let seller_spend_secret = root.join("buyer/second-spend.key");
    private(&seller_spend_secret, &[8; 32]);
    seller_config["seller_spend_secret_file"] = json!(seller_spend_secret);
    seller_config["seller_wallet_file"] = deployed.config["wallet_file"].clone();
    seller_config["seller_wallet_key_file"] = deployed.config["wallet_key_file"].clone();
    let seller_terms_path = root.join("buyer/second-seller.terms");
    template.amount = BaseUnits::new(65);
    private(&seller_terms_path, &template.encode().unwrap());
    seller_config["terms_template_file"] = json!(seller_terms_path);
    let seller_config_path = root.join("buyer/second-seller.json");
    private(
        &seller_config_path,
        &serde_json::to_vec(&seller_config).unwrap(),
    );

    let second_operation = [78; 32];
    let request = |config: &Path| {
        json!({"method":"negotiate","config_file":config,
            "operation_ref":hex::encode(second_operation),"freeze_only":false})
    };
    let seller_process = spawn(&fixture.binary, root, request(&seller_config_path));
    let buyer_process = spawn(&fixture.binary, root, request(&buyer_config_path));
    let (buyer_ok, second_buyer) = finish(buyer_process);
    let (seller_ok, second_seller) = finish(seller_process);
    assert!(buyer_ok && seller_ok, "{second_buyer}; {second_seller}");
    assert_ne!(
        second_buyer["deal_commitment"],
        first_buyer["deal_commitment"]
    );
    assert_eq!(
        second_buyer["deal_commitment"],
        second_seller["deal_commitment"]
    );
    let agreement = SelectedAgreement::decode(
        &fs::read(second_buyer["evidence_file"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    verify_selected_agreement(&agreement).unwrap();
    assert_eq!(agreement.terms.amount.get(), 65);
    assert_eq!(
        agreement.terms.buyer_authorization_key.as_bytes(),
        derive_key(&[62; 32]).unwrap()
    );

    // The second buyer has no seeded deposit: its only opening is the first payment.
    let recipient_path = PathBuf::from(original_seller["seller_wallet_file"].as_str().unwrap());
    let domain = deployed.wallet.domain();
    let recipient = WalletStore::new(&recipient_path, domain, [5; 32]).unwrap();
    let before = recipient.snapshot().unwrap();
    assert_eq!(before.notes().len(), 1);
    assert_eq!(before.notes()[0].amount(), 70);
    assert!(before.notes()[0].inclusion().is_none());
    let received_commitment = before.notes()[0].commitment();
    drop(before);
    drop(recipient);
    let mut payment_config = deployed.config.clone();
    payment_config["state_root"] = original_seller["state_root"].clone();
    payment_config["buyer_key_hex"] = json!(hex::encode(derive_key(&[62; 32]).unwrap()));
    payment_config["wallet_file"] = json!(recipient_path);
    payment_config["wallet_key_file"] = original_seller["seller_wallet_key_file"].clone();
    // The gas payer and its single nonce journal are shared, not the spending identity.
    let payment_config_path = root.join("seller/second-payment.json");
    private(
        &payment_config_path,
        &serde_json::to_vec(&payment_config).unwrap(),
    );
    let (ok, funding) = run_payment_for(
        &binary,
        root,
        &payment_config_path,
        "funding",
        second_operation,
    )
    .await;
    assert!(ok, "{funding}");
    assert_eq!(funding["status"], "ready");
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 1);
    let (_, submitted) = run_payment_for(
        &binary,
        root,
        &payment_config_path,
        "settle",
        second_operation,
    )
    .await;
    assert_eq!(submitted["stage"], "Submitted", "{submitted}");
    assert_eq!(submitted["submitted_this_call"], true);
    rpc(
        &deployed.client,
        &deployed.url,
        "anvil_mine",
        json!(["0x4"]),
    )
    .await;
    for _ in 0..2 {
        let (ok, observed) = run_payment_for(
            &binary,
            root,
            &payment_config_path,
            "settle",
            second_operation,
        )
        .await;
        assert!(ok, "{observed}");
        assert_eq!(observed["payment_verified"], true);
        assert_eq!(observed["submitted_this_call"], false);
    }
    let recipient = WalletStore::new(&recipient_path, domain, [5; 32]).unwrap();
    let after = recipient.snapshot().unwrap();
    assert!(after.spendable(&received_commitment).is_none());
    assert!(after
        .notes()
        .iter()
        .find(|n| n.commitment() == received_commitment)
        .unwrap()
        .consumption()
        .is_some());
    assert_eq!(after.select(&deployed.token_bytes, 5).unwrap().amount(), 5);
    assert!(after.select(&deployed.token_bytes, 6).is_none());
    let (ok, first_replay) = run_payment(&binary, root, &deployed.config_path, "settle").await;
    assert!(ok, "{first_replay}");
    assert_eq!(first_replay["payment_verified"], true);
    assert_eq!(first_replay["submitted_this_call"], false);
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Anvil, Node, uv, and existing M5 prototype proving artifacts"]
async fn two_mcp_agents_negotiate_and_settle_once_through_a_lost_broadcast() {
    // Real block production, so the agents reach finality by observation alone.
    let deployed = deploy(true, Some(1)).await;
    let root = deployed.fixture.root.path();
    let payment = root.join("erebus-payment");
    fs::copy(
        native_binary("erebus-payment", env!("CARGO_BIN_EXE_erebus-payment")),
        &payment,
    )
    .unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut command = match (
        std::env::var_os("EREBUS_TEST_MCP_SERVER"),
        std::env::var_os("EREBUS_TEST_MCP_PYTHON"),
    ) {
        (Some(server), Some(python)) => {
            let mut command = Command::new(python);
            command
                .arg("-I")
                .arg(repository.join("agents/src/erebus_agents/metropolis_loop.py"))
                .arg("--server-command")
                .arg(server);
            command
        }
        (None, None) => {
            let mut command = Command::new("uv");
            command.args([
                "run",
                "--locked",
                "python",
                "-m",
                "erebus_agents.metropolis_loop",
            ]);
            command
        }
        _ => panic!("installed MCP qualification requires both server and Python paths"),
    };
    command
        .args(["--operation", &hex::encode(OPERATION)])
        .arg("--buyer-negotiation")
        .arg(&deployed.fixture.buyer_config)
        .arg("--buyer-payment")
        .arg(&deployed.config_path)
        .arg("--seller-negotiation")
        .arg(&deployed.fixture.seller_config)
        .arg("--negotiation-cli")
        .arg(&deployed.fixture.binary)
        .arg("--payment-cli")
        .arg(&payment)
        .current_dir(if std::env::var_os("EREBUS_TEST_MCP_SERVER").is_some() {
            root
        } else {
            &repository
        })
        .env_remove("VIRTUAL_ENV");
    let output = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "agents: {stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: Value = serde_json::from_str(stdout.trim().lines().last().unwrap()).unwrap();
    eprintln!("two-agent MCP record (Anvil, test-only setup): {record}");
    assert_eq!(record["payment_verified"], true);
    assert_eq!(record["stage"], "Finalized");
    assert_eq!(record["settle_calls"], 1);
    assert_eq!(record["winning_commitment"], record["deal_commitment"]);
    // The lost acknowledgement made the first result uncertain; recovery was by observation.
    assert_eq!(record["settlement"]["submitted_this_call"], true);
    assert_eq!(record["settlement"]["payment_verified"], false);
    assert!(record["recover_calls"].as_u64().unwrap() >= 1);
    assert_eq!(deployed.sends.load(Ordering::SeqCst), 1);
    let notes = deployed.wallet.snapshot().unwrap();
    assert!(notes
        .notes()
        .iter()
        .any(|note| note.amount() == 80 && note.inclusion().is_some()));
}

/// Keeps a funded two-participant environment alive for externally driven agents.
/// Run with `EREBUS_AGENT_ENV_DIR=<new dir>`; create `<dir>/stop` to tear down.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "manual: holds Anvil and prototype artifacts open for external MCP agents"]
async fn hold_metropolis_agent_environment() {
    let Some(directory) = std::env::var_os("EREBUS_AGENT_ENV_DIR").map(PathBuf::from) else {
        return;
    };
    let deployed = deploy(true, Some(1)).await;
    let root = deployed.fixture.root.path();
    // Agents start independently, so give each side time to find its peer.
    for path in [
        &deployed.fixture.buyer_config,
        &deployed.fixture.seller_config,
    ] {
        let mut config: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        config["timeout_seconds"] = json!(300);
        fs::write(path, config.to_string()).unwrap();
    }
    let payment = root.join("erebus-payment");
    fs::copy(env!("CARGO_BIN_EXE_erebus-payment"), &payment).unwrap();
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("environment.json"),
        json!({"operation_ref":hex::encode(OPERATION),"buyer_negotiation":deployed.fixture.buyer_config,
            "buyer_payment":deployed.config_path,"seller_negotiation":deployed.fixture.seller_config,
            "negotiation_cli":deployed.fixture.binary,"payment_cli":payment})
        .to_string(),
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(1800);
    while !directory.join("stop").exists() && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let change_included = deployed
        .wallet
        .snapshot()
        .unwrap()
        .notes()
        .iter()
        .any(|note| note.amount() == 80 && note.inclusion().is_some());
    fs::write(
        directory.join("result.json"),
        json!({"raw_transaction_sends":deployed.sends.load(Ordering::SeqCst),"buyer_change_note_included":change_included})
            .to_string(),
    )
    .unwrap();
}
