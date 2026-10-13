//! External proof admission against local, known-entropy M5 artifacts only.

use std::{fs, path::PathBuf};

use ark_bn254::{Fq, Fr};
use ark_ff::{BigInteger, PrimeField};
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
    relay_admission::{RelayAdmissionPolicy, RelayProofError, TransferVerifier},
    rpc::{PoolRpc, PoolRuntimePins},
};
use sha2::{Digest, Sha256};

fn signal(data: &[u8], index: usize) -> [u8; 32] {
    data[4 + (8 + index) * 32..4 + (9 + index) * 32]
        .try_into()
        .unwrap()
}

mod funded {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::{json, Value};
    use std::{
        net::TcpListener,
        process::{Child, Command, Stdio},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        time::Duration,
    };

    struct Anvil(Child);
    impl Drop for Anvil {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    struct Proxy {
        upstream: String,
        client: reqwest::Client,
        simulations: AtomicUsize,
        writes: AtomicUsize,
    }

    struct LostBroadcast {
        upstream: String,
        client: reqwest::Client,
        requests: std::sync::Mutex<Vec<String>>,
        drop_count: usize,
    }

    async fn drop_first_broadcast(
        State(state): State<Arc<LostBroadcast>>,
        Json(request): Json<Value>,
    ) -> Json<Value> {
        if request["method"] == "eth_sendRawTransaction" {
            let first = {
                let mut requests = state.requests.lock().unwrap();
                requests.push(request["params"][0].as_str().unwrap().to_owned());
                requests.len() <= state.drop_count
            };
            if first {
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

    async fn forward(State(state): State<Arc<Proxy>>, Json(request): Json<Value>) -> Json<Value> {
        let method = request["method"].as_str().unwrap();
        if ![
            "eth_chainId",
            "eth_blockNumber",
            "eth_getBlockByNumber",
            "eth_getCode",
            "eth_call",
            "eth_getTransactionReceipt",
            "eth_getTransactionByHash",
            "eth_getTransactionCount",
            "eth_getBalance",
        ]
        .contains(&method)
        {
            state.writes.fetch_add(1, Ordering::SeqCst);
            return Json(json!({"jsonrpc":"2.0", "id":request["id"],
                "error":{"code":-32601, "message":"admission proxy is read-only"}}));
        }
        if method == "eth_call" && request["params"][0]["data"].as_str().unwrap().len() > 10 {
            state.simulations.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request["params"][1]["requireCanonical"], true);
            assert_eq!(request["params"][0]["value"], "0x0");
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

    fn bytes<const N: usize>(value: &Value) -> [u8; N] {
        hex::decode(value.as_str().unwrap().strip_prefix("0x").unwrap())
            .unwrap()
            .try_into()
            .unwrap()
    }

    async fn refuses(
        client: &reqwest::Client,
        url: &str,
        fixture: &Value,
        name: &str,
        reason: &str,
    ) {
        let response: Value = client
            .post(url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"eth_call",
            "params":[{"from":fixture["sponsor"],"to":fixture["pool"],"data":fixture[name],
                "gas":"0x2dc6c0","value":"0x0"}, "latest"]}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert!(
            response["error"]["message"]
                .as_str()
                .unwrap()
                .contains(reason),
            "{name}: {response}"
        );
    }

    #[tokio::test]
    #[ignore = "requires Anvil, node modules and current M5 local proving artifacts"]
    async fn funded_pool_admission_checks_real_roots_consumption_and_output_reuse_without_sending()
    {
        exercise_funded_pool(false, 0).await;
    }

    #[tokio::test]
    #[ignore = "requires Anvil, node modules and current M5 local proving artifacts"]
    async fn funded_sponsor_submits_once_and_recovers_own_finalized_receipt_after_restart() {
        exercise_funded_pool(true, 0).await;
    }

    #[tokio::test]
    #[ignore = "requires Anvil, node modules and current M5 local proving artifacts"]
    async fn explicit_owner_resume_recovers_a_dropped_broadcast_with_identical_bytes() {
        exercise_funded_pool(true, 1).await;
    }

    #[tokio::test]
    #[ignore = "requires Anvil, node modules and current M5 local proving artifacts"]
    async fn explicit_fee_replacement_is_same_nonce_and_finalizes_once() {
        exercise_funded_pool(true, 2).await;
    }

    #[tokio::test]
    #[ignore = "requires Anvil, node modules and current M5 local proving artifacts"]
    async fn older_sponsor_attempt_can_win_after_a_replacement_is_retained() {
        exercise_funded_pool(true, 3).await;
    }

    async fn exercise_funded_pool(use_service: bool, recovery: u8) {
        let drop_broadcast = recovery != 0;
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let _anvil = Anvil(
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
            context,
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
        let key_bytes = key("transfer");
        let verifier = TransferVerifier::from_zkey_bytes(
            &key_bytes,
            &format!("{:x}", Sha256::digest(&key_bytes)),
        )
        .unwrap();
        let state = Arc::new(Proxy {
            upstream: url.clone(),
            client: client.clone(),
            simulations: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let peer_url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/", post(forward))
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        // Abort the proxy even if a later assertion panics.
        struct Server(tokio::task::JoinHandle<()>);
        impl Drop for Server {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _server = Server(server);
        let primary = PoolRpc::new(&url, 31337, pool).unwrap();
        let peer = PoolRpc::new(&peer_url, 31337, pool).unwrap();
        // Anvil's explicit finalized tag lags its head; finalize the creation first.
        rpc(&client, &url, "anvil_mine", json!(["0x40"])).await;
        let sponsor = fixture["sponsor"].as_str().unwrap();
        let nonce = rpc(
            &client,
            &url,
            "eth_getTransactionCount",
            json!([sponsor, "latest"]),
        )
        .await;
        verifier
            .admit(&policy, &prepared, &primary, &peer)
            .await
            .unwrap();
        assert_eq!(state.simulations.load(Ordering::SeqCst), 1);
        assert_eq!(
            rpc(
                &client,
                &url,
                "eth_getTransactionCount",
                json!([sponsor, "latest"])
            )
            .await,
            nonce
        );
        assert_eq!(state.writes.load(Ordering::SeqCst), 0);
        let (_, unknown) = context_and_prepared(data("unknown_root"));
        verifier.verify(&policy.context, &unknown, 0).unwrap();
        assert!(verifier
            .admit(&policy, &unknown, &primary, &peer)
            .await
            .is_err());
        refuses(&client, &url, &fixture, "unknown_root", "unknown root").await;
        assert_eq!(
            rpc(
                &client,
                &url,
                "eth_getTransactionCount",
                json!([sponsor, "latest"])
            )
            .await,
            nonce
        );
        // The original admission gate sends from the test; the separate service gate
        // uses the production journal, nonce lane, signer and raw broadcast adapter.
        let root = tempfile::tempdir().unwrap();
        let sponsor_key: [u8; 32] =
            hex::decode("59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d")
                .unwrap()
                .try_into()
                .unwrap();
        let lost = Arc::new(LostBroadcast {
            upstream: url.clone(),
            client: client.clone(),
            requests: std::sync::Mutex::new(Vec::new()),
            drop_count: if recovery == 3 { 2 } else { 1 },
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let broadcast_url = format!("http://{}", listener.local_addr().unwrap());
        let lost_app = Router::new()
            .route("/", post(drop_first_broadcast))
            .with_state(lost.clone());
        let _lost_server = Server(tokio::spawn(async move {
            axum::serve(listener, lost_app).await.unwrap();
        }));
        let config = || erebus_shielded_prover::relay_service::SponsorConfig {
            root: root.path().join("service"),
            signer_root: root.path().join("nonces"),
            admission: policy.clone(),
            fees: erebus_evm::chain::Eip1559Fees::new(3_000_000_000, 1_000_000_000).unwrap(),
            replacement_fees: (recovery >= 2).then(|| {
                erebus_evm::chain::Eip1559Fees::new(6_000_000_000, 2_000_000_000).unwrap()
            }),
            rpc_url: if drop_broadcast {
                broadcast_url.clone()
            } else {
                url.clone()
            },
            peer_rpc_url: peer_url.clone(),
            timeout: Duration::from_secs(10),
        };
        let service_request = erebus_shielded_prover::relay_request::RelayRequest::from_prepared(
            &policy.context,
            &prepared,
        )
        .unwrap()
        .encode()
        .unwrap();
        let mut relay_server = None;
        let mut relay_endpoint = None;
        let mut tx = if use_service {
            let service = erebus_shielded_prover::relay_service::SponsoredRelay::connect(
                config(),
                TransferVerifier::from_zkey_bytes(
                    &key_bytes,
                    &format!("{:x}", Sha256::digest(&key_bytes)),
                )
                .unwrap(),
                &sponsor_key,
            )
            .await
            .unwrap();
            let balance = rpc(&client, &url, "eth_getBalance", json!([sponsor, "latest"])).await;
            rpc(&client, &url, "anvil_setBalance", json!([sponsor, "0x0"])).await;
            assert!(matches!(
                service.relay(&service_request).await,
                Err(erebus_shielded_prover::relay_service::SponsorError::Funding)
            ));
            assert_eq!(
                erebus_evm::chain::SignerJournal::open(
                    root.path().join("nonces"),
                    31337,
                    policy.sponsor
                )
                .unwrap()
                .claim_nonce()
                .unwrap(),
                None
            );
            assert_eq!(
                rpc(
                    &client,
                    &url,
                    "eth_getTransactionCount",
                    json!([sponsor, "latest"])
                )
                .await,
                nonce
            );
            rpc(&client, &url, "anvil_setBalance", json!([sponsor, balance])).await;
            let transport = erebus_shielded_prover::relay_http::RelayHttpService::new(
                service,
                [7; 32],
                &root.path().join("http-quota"),
                erebus_shielded_prover::relay_http::RelayHttpPolicy {
                    requests_per_window: 50,
                    window_seconds: 60,
                    maximum_gas_budget: if recovery >= 2 {
                        18_000_000_000_000_000
                    } else {
                        9_000_000_000_000_000
                    },
                    maximum_operations: 1,
                    timeout_seconds: 60,
                },
            )
            .unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            relay_server = Some(tokio::spawn(async move {
                axum::serve(listener, transport.router()).await.unwrap();
            }));
            let refused = client
                .post(format!("{endpoint}/v1/relay"))
                .body(service_request.clone())
                .send()
                .await
                .unwrap();
            assert_eq!(refused.status().as_u16(), 401);
            let oversized = client
                .post(format!("{endpoint}/v1/relay"))
                .bearer_auth(hex::encode([7; 32]))
                .body(vec![b' '; 2049])
                .send()
                .await
                .unwrap();
            assert_eq!(oversized.status().as_u16(), 413);
            assert_eq!(
                rpc(
                    &client,
                    &url,
                    "eth_getTransactionCount",
                    json!([sponsor, "latest"])
                )
                .await,
                nonce
            );
            let response = client
                .post(format!("{endpoint}/v1/relay"))
                .bearer_auth(hex::encode([7; 32]))
                .body(service_request.clone())
                .send()
                .await
                .unwrap();
            assert!(response.status().is_success());
            let status: Value = response.json().await.unwrap();
            assert_eq!(status["broadcast_started"], true);
            assert_eq!(status["status"], "pending");
            assert!(status["finalized"].is_null());
            relay_endpoint = Some(endpoint);
            status["transaction_hash"].clone()
        } else {
            rpc(&client, &url, "eth_sendTransaction", json!([{
            "from":sponsor,"to":fixture["pool"],"data":fixture["valid"],"value":"0x0","gas":"0x2dc6c0"
        }])).await
        };
        if drop_broadcast {
            assert_eq!(lost.requests.lock().unwrap().len(), 1);
            let restored = erebus_shielded_prover::relay_service::SponsoredRelay::connect(
                config(),
                TransferVerifier::from_zkey_bytes(
                    &key_bytes,
                    &format!("{:x}", Sha256::digest(&key_bytes)),
                )
                .unwrap(),
                &sponsor_key,
            )
            .await
            .unwrap();
            let held = restored.relay(&service_request).await.unwrap();
            assert!(held.finalized.is_none());
            assert_eq!(
                lost.requests.lock().unwrap().len(),
                1,
                "a normal duplicate request must not resend"
            );
            if recovery == 1 {
                let resumed = restored.resume(&service_request).await.unwrap();
                assert_eq!(
                    format!("0x{}", hex::encode(resumed.transaction_hash.unwrap())),
                    tx
                );
                let requests = lost.requests.lock().unwrap();
                assert_eq!(requests.len(), 2);
                assert_eq!(
                    requests[0], requests[1],
                    "explicit resume must not change the signed bytes"
                );
            } else {
                let replaced = restored.replace(&service_request).await.unwrap();
                let requests = lost.requests.lock().unwrap().clone();
                assert_eq!(requests.len(), 2);
                let old = erebus_evm::chain::SignedTransaction::from_raw(
                    &hex::decode(requests[0].trim_start_matches("0x")).unwrap(),
                )
                .unwrap();
                let new = erebus_evm::chain::SignedTransaction::from_raw(
                    &hex::decode(requests[1].trim_start_matches("0x")).unwrap(),
                )
                .unwrap();
                new.validate_replacement(&old).unwrap();
                assert_eq!(new.params().nonce, old.params().nonce);
                assert_eq!(replaced.transaction_hash, Some(new.hash()));
                let duplicate = restored.replace(&service_request).await.unwrap();
                assert_eq!(duplicate.transaction_hash, Some(new.hash()));
                assert_eq!(
                    lost.requests.lock().unwrap().len(),
                    2,
                    "duplicate replacement cannot resend"
                );
                if recovery == 3 {
                    // A delayed older attempt wins even though a replacement was durably retained.
                    assert_eq!(
                        rpc(
                            &client,
                            &url,
                            "eth_sendRawTransaction",
                            json!([requests[0]])
                        )
                        .await,
                        tx
                    );
                } else {
                    tx = json!(format!("0x{}", hex::encode(new.hash())));
                }
            }
        }
        let mut receipt = Value::Null;
        for _ in 0..100 {
            receipt = rpc(&client, &url, "eth_getTransactionReceipt", json!([tx])).await;
            if !receipt.is_null() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(receipt["status"], "0x1");
        assert_eq!(receipt["from"], sponsor);
        if use_service {
            rpc(&client, &url, "anvil_mine", json!(["0x40"])).await;
            let restored = erebus_shielded_prover::relay_service::SponsoredRelay::connect(
                config(),
                TransferVerifier::from_zkey_bytes(
                    &key_bytes,
                    &format!("{:x}", Sha256::digest(&key_bytes)),
                )
                .unwrap(),
                &sponsor_key,
            )
            .await
            .unwrap();
            let observed = restored.observe(&service_request).await.unwrap();
            assert!(observed.finalized.unwrap().success);
            assert_eq!(
                observed.finalized.unwrap().block_number,
                u64::from_str_radix(
                    receipt["blockNumber"]
                        .as_str()
                        .unwrap()
                        .trim_start_matches("0x"),
                    16
                )
                .unwrap()
            );
            assert_eq!(restored.relay(&service_request).await.unwrap(), observed);
            assert_eq!(restored.resume(&service_request).await.unwrap(), observed);
            if recovery >= 2 {
                assert_eq!(restored.replace(&service_request).await.unwrap(), observed);
                assert_eq!(lost.requests.lock().unwrap().len(), 2);
            }
            let response = client
                .post(format!("{}/v1/observe", relay_endpoint.as_ref().unwrap()))
                .bearer_auth(hex::encode([7; 32]))
                .body(service_request.clone())
                .send()
                .await
                .unwrap();
            assert!(response.status().is_success());
            let status: Value = response.json().await.unwrap();
            assert_eq!(status["status"], "finalized");
            assert_eq!(status["transaction_hash"], tx);
            assert_eq!(status["retry_without_new_payment"], true);
            assert!(!status.to_string().contains("calldata"));
            assert_eq!(
                erebus_evm::chain::SignerJournal::open(
                    root.path().join("nonces"),
                    31337,
                    policy.sponsor
                )
                .unwrap()
                .claim_nonce()
                .unwrap(),
                None
            );
        }
        let mined_nonce = rpc(
            &client,
            &url,
            "eth_getTransactionCount",
            json!([sponsor, "latest"]),
        )
        .await;
        assert_eq!(
            u64::from_str_radix(mined_nonce.as_str().unwrap().trim_start_matches("0x"), 16)
                .unwrap(),
            u64::from_str_radix(nonce.as_str().unwrap().trim_start_matches("0x"), 16).unwrap() + 1
        );
        for name in ["valid", "spent", "reused"] {
            let (_, candidate) = context_and_prepared(data(name));
            verifier.verify(&policy.context, &candidate, 0).unwrap();
            assert!(
                verifier
                    .admit(&policy, &candidate, &primary, &peer)
                    .await
                    .is_err(),
                "{name}"
            );
            assert_eq!(
                rpc(
                    &client,
                    &url,
                    "eth_getTransactionCount",
                    json!([sponsor, "latest"])
                )
                .await,
                mined_nonce
            );
            refuses(
                &client,
                &url,
                &fixture,
                name,
                if name == "reused" {
                    "duplicate note"
                } else {
                    "already consumed"
                },
            )
            .await;
        }
        if let Some(server) = relay_server {
            server.abort();
        }
        assert_eq!(state.writes.load(Ordering::SeqCst), 0);
    }
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

fn transfer_data() -> Vec<u8> {
    let encoded: String = serde_json::from_slice(
        &fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../circuits/m5/build/rust-prepared-transfer-calldata.json"),
        )
        .unwrap(),
    )
    .unwrap();
    hex::decode(encoded.strip_prefix("0x").unwrap()).unwrap()
}

#[test]
#[ignore = "requires the M5 coordinated prototype and its retained transfer calldata"]
fn external_transfer_proof_is_verified_without_witness_or_spending_material() {
    let data = transfer_data();
    let bytes = key("transfer");
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let verifier = TransferVerifier::from_zkey_bytes(&bytes, &digest).unwrap();
    let (context, prepared) = context_and_prepared(data.clone());
    verifier.verify(&context, &prepared, 0).unwrap();

    let expiry = u64::from_be_bytes(signal(&data, 10)[24..].try_into().unwrap());
    verifier.verify(&context, &prepared, expiry - 1).unwrap();
    assert_eq!(
        verifier.verify(&context, &prepared, expiry),
        Err(RelayProofError::Expired)
    );

    for index in 0..11 {
        let mut changed = data.clone();
        changed[4 + (9 + index) * 32 - 1] ^= 1;
        let (context, prepared) = context_and_prepared(changed);
        assert!(
            verifier.verify(&context, &prepared, 0).is_err(),
            "public input {index}"
        );
    }
    for offset in [4, 4 + 2 * 32, 4 + 6 * 32] {
        let mut changed = prepared.clone();
        changed.backend_evidence[offset..offset + 64].fill(0);
        assert!(verifier.verify(&context, &changed, 0).is_err());
    }
    for (offset, modulus) in [
        (4, Fq::MODULUS.to_bytes_be()),
        (4 + 14 * 32, Fr::MODULUS.to_bytes_be()),
    ] {
        let mut changed = prepared.clone();
        changed.backend_evidence[offset..offset + 32].copy_from_slice(&modulus);
        assert_eq!(
            verifier.verify(&context, &changed, 0),
            Err(RelayProofError::Encoding)
        );
    }
    let mut changed = prepared.clone();
    changed.backend_evidence[0] ^= 1;
    assert_eq!(
        verifier.verify(&context, &changed, 0),
        Err(RelayProofError::Context)
    );
    let mut changed = prepared.clone();
    changed.operation_ref = [0; 32];
    assert_eq!(
        verifier.verify(&context, &changed, 0),
        Err(RelayProofError::Context)
    );
    let mut changed = prepared.clone();
    changed.backend_evidence.push(0);
    assert_eq!(
        verifier.verify(&context, &changed, 0),
        Err(RelayProofError::Context)
    );

    assert!(matches!(
        TransferVerifier::from_zkey_bytes(&bytes, &"0".repeat(64)),
        Err(RelayProofError::Artifact)
    ));
    let truncated = &bytes[..16];
    assert!(matches!(
        TransferVerifier::from_zkey_bytes(truncated, &format!("{:x}", Sha256::digest(truncated))),
        Err(RelayProofError::Key)
    ));
    let bytes = key("withdraw");
    assert!(matches!(
        TransferVerifier::from_zkey_bytes(&bytes, &format!("{:x}", Sha256::digest(&bytes))),
        Err(RelayProofError::Key)
    ));
}

// These providers simulate RPC evidence, not contract execution or consensus finality.
mod paired {
    use super::*;
    use axum::{extract::State, routing::post, Json, Router};
    use serde_json::{json, Value};
    use sha3::Keccak256;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[derive(Clone, Copy)]
    enum Fault {
        None,
        HeaderConflict,
        WrongRuntime,
        WrongAsset,
        Revert,
        ReturnData,
        ReorgAfterSimulation,
        FinalityRegression,
        WrongChain,
        MissingFinality,
    }

    struct Provider {
        chain: u64,
        pool: [u8; 20],
        asset: [u8; 20],
        version: u32,
        head: u64,
        finalized: u64,
        data: Vec<u8>,
        fault: Fault,
        requests: AtomicUsize,
        simulations: AtomicUsize,
        writes: AtomicUsize,
    }

    fn hash(number: u64) -> String {
        let mut hash = [0; 32];
        hash[24..].copy_from_slice(&number.to_be_bytes());
        format!("0x{}", hex::encode(hash))
    }

    async fn handle(State(state): State<Arc<Provider>>, Json(request): Json<Value>) -> Json<Value> {
        state.requests.fetch_add(1, Ordering::SeqCst);
        let params = &request["params"];
        let result = match request["method"].as_str().unwrap() {
            "eth_chainId" => json!(format!(
                "0x{:x}",
                state.chain + u64::from(matches!(state.fault, Fault::WrongChain))
            )),
            "eth_blockNumber" => json!(format!("0x{:x}", state.head)),
            "eth_getBlockByNumber" => {
                if params[0] == "finalized" && matches!(state.fault, Fault::MissingFinality) {
                    return Json(json!({"jsonrpc":"2.0", "id":request["id"], "result":null}));
                }
                let number = if params[0] == "finalized" {
                    if matches!(state.fault, Fault::FinalityRegression)
                        && state.simulations.load(Ordering::SeqCst) > 0
                    {
                        80
                    } else {
                        state.finalized
                    }
                } else {
                    u64::from_str_radix(params[0].as_str().unwrap().strip_prefix("0x").unwrap(), 16)
                        .unwrap()
                };
                let changed = matches!(state.fault, Fault::HeaderConflict)
                    || (matches!(state.fault, Fault::ReorgAfterSimulation)
                        && state.simulations.load(Ordering::SeqCst) > 0);
                json!({"number":format!("0x{number:x}"),
                    "hash":if changed && number == 95 {hash(999)} else {hash(number)},
                    "parentHash":hash(number.saturating_sub(1)), "timestamp":"0x1"})
            }
            "eth_getCode" => {
                assert_eq!(params[1]["requireCanonical"], true);
                assert!([hash(1), hash(95)]
                    .contains(&params[1]["blockHash"].as_str().unwrap().to_string()));
                json!(if matches!(state.fault, Fault::WrongRuntime) {
                    "0x6002"
                } else {
                    "0x6001"
                })
            }
            "eth_call" => {
                assert_eq!(params[0]["to"], format!("0x{}", hex::encode(state.pool)));
                assert_eq!(params[1]["requireCanonical"], true);
                let data = hex::decode(
                    params[0]["data"]
                        .as_str()
                        .unwrap()
                        .strip_prefix("0x")
                        .unwrap(),
                )
                .unwrap();
                if data.len() > 4 {
                    assert_eq!(data, state.data);
                    assert_eq!(params[0]["from"], format!("0x{}", hex::encode([7; 20])));
                    assert_eq!(params[0]["gas"], "0x2dc6c0");
                    assert_eq!(params[0]["value"], "0x0");
                    assert_eq!(params[1]["blockHash"], hash(95));
                    state.simulations.fetch_add(1, Ordering::SeqCst);
                    if matches!(state.fault, Fault::Revert) {
                        return Json(json!({"jsonrpc":"2.0", "id":request["id"],
                            "error":{"code":3,"message":"execution reverted"}}));
                    }
                    json!(if matches!(state.fault, Fault::ReturnData) {
                        "0x00"
                    } else {
                        "0x"
                    })
                } else {
                    let mut word = [0; 32];
                    let selector =
                        |signature: &str| Keccak256::digest(signature.as_bytes())[..4].to_vec();
                    if data == selector("asset()") {
                        word[12..].copy_from_slice(&if matches!(state.fault, Fault::WrongAsset) {
                            [9; 20]
                        } else {
                            state.asset
                        });
                    } else if data == selector("verifierVersion()") {
                        word[28..].copy_from_slice(&state.version.to_be_bytes());
                    } else {
                        assert!([
                            "poseidonTwo()",
                            "depositVerifier()",
                            "transferVerifier()",
                            "withdrawVerifier()"
                        ]
                        .iter()
                        .any(|signature| data == selector(signature)));
                        word[12..].fill(2);
                    }
                    json!(format!("0x{}", hex::encode(word)))
                }
            }
            _ => {
                state.writes.fetch_add(1, Ordering::SeqCst);
                return Json(json!({"jsonrpc":"2.0", "id":request["id"],
                    "error":{"code":-32601,"message":"unexpected method"}}));
            }
        };
        Json(json!({"jsonrpc":"2.0", "id":request["id"], "result":result}))
    }

    struct Server {
        rpc: PoolRpc,
        state: Arc<Provider>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn server(
        policy: &RelayAdmissionPolicy,
        data: &[u8],
        peer: bool,
        fault: Fault,
    ) -> Server {
        let state = Arc::new(Provider {
            chain: policy.deployment.chain_id,
            pool: policy.deployment.pool,
            asset: signal(data, 3)[12..].try_into().unwrap(),
            version: policy.context.domain.verifier_version,
            head: if peer { 95 } else { 100 },
            finalized: if peer { 85 } else { 90 },
            data: data.to_vec(),
            fault,
            requests: AtomicUsize::new(0),
            simulations: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/", post(handle))
            .with_state(state.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Server {
            rpc: PoolRpc::new(&url, policy.deployment.chain_id, policy.deployment.pool).unwrap(),
            state,
            task,
        }
    }

    #[tokio::test]
    #[ignore = "requires the retained M5 transfer proof and known-entropy key"]
    async fn paired_admission_pins_simulations_and_refuses_inconsistent_evidence_without_sends() {
        let data = transfer_data();
        let bytes = key("transfer");
        let verifier =
            TransferVerifier::from_zkey_bytes(&bytes, &format!("{:x}", Sha256::digest(&bytes)))
                .unwrap();
        let (context, prepared) = context_and_prepared(data.clone());
        let pin = Keccak256::digest([0x60, 0x01]).into();
        let policy = RelayAdmissionPolicy {
            deployment: IndexDomain {
                chain_id: context.domain.namespace.reference().parse().unwrap(),
                pool: signal(&data, 1)[12..].try_into().unwrap(),
                first_block: 1,
                first_hash: hex::decode(&hash(1)[2..]).unwrap().try_into().unwrap(),
            },
            context,
            runtime_pins: PoolRuntimePins {
                pool: pin,
                poseidon: pin,
                deposit: pin,
                transfer: pin,
                withdraw: pin,
            },
            sponsor: [7; 20],
            gas_limit: 3_000_000,
        };
        let primary = server(&policy, &data, false, Fault::None).await;
        let peer = server(&policy, &data, true, Fault::None).await;
        let admitted = verifier
            .admit(&policy, &prepared, &primary.rpc, &peer.rpc)
            .await
            .unwrap();
        assert_eq!(admitted.head_number, 95);
        assert_eq!(admitted.finalized.number, 85);
        assert_eq!(
            admitted.calldata_sha256,
            <[u8; 32]>::from(Sha256::digest(&data))
        );
        assert_eq!(primary.state.simulations.load(Ordering::SeqCst), 1);
        assert_eq!(peer.state.simulations.load(Ordering::SeqCst), 1);
        assert!(verifier
            .admit(&policy, &prepared, &primary.rpc, &primary.rpc)
            .await
            .is_err());
        for fault in [
            Fault::HeaderConflict,
            Fault::WrongRuntime,
            Fault::WrongAsset,
            Fault::Revert,
            Fault::ReturnData,
            Fault::ReorgAfterSimulation,
            Fault::FinalityRegression,
            Fault::WrongChain,
            Fault::MissingFinality,
        ] {
            let primary = server(&policy, &data, false, Fault::None).await;
            let peer = server(&policy, &data, true, fault).await;
            assert!(verifier
                .admit(&policy, &prepared, &primary.rpc, &peer.rpc)
                .await
                .is_err());
            assert_eq!(primary.state.writes.load(Ordering::SeqCst), 0);
            assert_eq!(peer.state.writes.load(Ordering::SeqCst), 0);
        }
        let primary = server(&policy, &data, false, Fault::None).await;
        let peer = server(&policy, &data, true, Fault::None).await;
        let mut invalid = prepared.clone();
        invalid.backend_evidence[4] ^= 1;
        assert!(verifier
            .admit(&policy, &invalid, &primary.rpc, &peer.rpc)
            .await
            .is_err());
        assert_eq!(primary.state.requests.load(Ordering::SeqCst), 0);
        assert_eq!(peer.state.requests.load(Ordering::SeqCst), 0);
    }
}
