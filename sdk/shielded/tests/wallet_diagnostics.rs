//! Read-only account diagnostics: fixed addresses, shared finality and fail-closed evidence.

use axum::{extract::State, routing::post, Json, Router};
use erebus_shielded_prover::{
    index_store::IndexDomain,
    rpc::{PoolIdentity, PoolRpc, PoolRuntimePins},
};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Honest,
    Lagging,
    BalanceConflict,
    InvalidNative,
    InvalidToken,
    EmptyCode,
    GetterFailure,
    AnchorChange,
    FinalityRegression,
}
struct Source {
    mode: Mode,
    account_read: AtomicBool,
    calls: Mutex<Vec<Value>>,
}
struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn word(value: &[u8]) -> String {
    let mut result = [0; 32];
    result[32 - value.len()..].copy_from_slice(value);
    format!("0x{}", hex::encode(result))
}
fn header(number: u64) -> Value {
    json!({"number":format!("0x{number:x}"), "hash":format!("0x{}",hex::encode([number as u8;32])),
           "parentHash":format!("0x{}",hex::encode([number.saturating_sub(1) as u8;32])),
           "timestamp":format!("0x{:x}",1000 + number)})
}
async fn handle(State(source): State<Arc<Source>>, Json(request): Json<Value>) -> Json<Value> {
    source.calls.lock().unwrap().push(request.clone());
    let method = request["method"].as_str().unwrap();
    let result = match method {
        "eth_chainId" => json!("0x279f"),
        "eth_blockNumber" => json!("0xa"),
        "eth_getBlockByNumber" => {
            let tag = request["params"][0].as_str().unwrap();
            let number = if tag == "finalized" {
                if source.mode == Mode::FinalityRegression
                    && source.account_read.load(Ordering::SeqCst)
                {
                    8
                } else if source.mode == Mode::Lagging {
                    10
                } else {
                    9
                }
            } else {
                u64::from_str_radix(&tag[2..], 16).unwrap()
            };
            let mut result = header(number);
            if number == 9
                && source.mode == Mode::AnchorChange
                && source.account_read.load(Ordering::SeqCst)
            {
                result["hash"] = header(8)["hash"].clone();
            }
            result
        }
        "eth_getCode" => {
            assert_eq!(request["params"][1]["requireCanonical"], true);
            json!(if source.mode == Mode::EmptyCode {
                "0x"
            } else {
                "0x6000"
            })
        }
        "eth_getBalance" => {
            assert_eq!(request["params"][0], format!("0x{}", hex::encode([4; 20])));
            assert_eq!(
                request["params"][1],
                json!({"blockHash":format!("0x{}",hex::encode([9;32])),"requireCanonical":true})
            );
            source.account_read.store(true, Ordering::SeqCst);
            json!(if source.mode == Mode::InvalidNative {
                "0x00"
            } else {
                "0x64"
            })
        }
        "eth_call" => {
            assert_eq!(request["params"][1]["requireCanonical"], true);
            let data = request["params"][0]["data"].as_str().unwrap();
            let selector = |signature: &str| {
                format!(
                    "0x{}",
                    hex::encode(&Keccak256::digest(signature.as_bytes())[..4])
                )
            };
            if data == selector("asset()") {
                json!(word(&[2; 20]))
            } else if data == selector("verifierVersion()") {
                json!(word(&[2]))
            } else if data.starts_with(&selector("balanceOf(address)")) {
                assert_eq!(
                    request["params"][0]["to"],
                    format!("0x{}", hex::encode([2; 20]))
                );
                assert_eq!(&data[10..], &word(&[4; 20])[2..]);
                if source.mode == Mode::InvalidToken {
                    json!("0x01")
                } else {
                    json!(word(&[if source.mode == Mode::BalanceConflict {
                        126
                    } else {
                        125
                    }]))
                }
            } else if data.starts_with(&selector("allowance(address,address)")) {
                assert_eq!(
                    data,
                    format!(
                        "{}{}{}",
                        selector("allowance(address,address)"),
                        &word(&[4; 20])[2..],
                        &word(&[1; 20])[2..]
                    )
                );
                json!(word(&[250]))
            } else if source.mode == Mode::GetterFailure {
                return Json(
                    json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":-32000,"message":"getter unavailable"}}),
                );
            } else {
                json!(word(&[3; 20]))
            }
        }
        _ => panic!("diagnostics must never request signing, accounts, nonces or writes: {method}"),
    };
    Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
}
async fn source(mode: Mode) -> (PoolRpc, Arc<Source>, Server) {
    let source = Arc::new(Source {
        mode,
        account_read: AtomicBool::new(false),
        calls: Mutex::new(vec![]),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rpc = PoolRpc::new(
        &format!("http://{}", listener.local_addr().unwrap()),
        10143,
        [1; 20],
    )
    .unwrap();
    let router = Router::new()
        .route("/", post(handle))
        .with_state(source.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (rpc, source, Server(task))
}
fn domain() -> IndexDomain {
    IndexDomain {
        chain_id: 10143,
        pool: [1; 20],
        first_block: 1,
        first_hash: [1; 32],
    }
}
fn identity() -> PoolIdentity {
    PoolIdentity {
        asset: [2; 20],
        verifier_version: 2,
    }
}
fn pins() -> PoolRuntimePins {
    let hash = Keccak256::digest([0x60, 0x00]).into();
    PoolRuntimePins {
        pool: hash,
        poseidon: hash,
        deposit: hash,
        transfer: hash,
        withdraw: hash,
    }
}

#[tokio::test]
async fn account_diagnostics_use_one_shared_finalized_hash_with_honest_provider_lag() {
    let (first, a, _a) = source(Mode::Honest).await;
    let (peer, b, _b) = source(Mode::Lagging).await;
    let report = first
        .account_state_agreed(&peer, domain(), identity(), pins(), [4; 20])
        .await
        .unwrap();
    assert_eq!(report.anchor.number, 9);
    assert_eq!(report.native_balance[31], 100);
    assert_eq!(report.asset_balance[31], 125);
    assert_eq!(report.allowance[31], 250);
    for state in [a, b] {
        let calls = state.calls.lock().unwrap();
        assert_eq!(
            calls
                .iter()
                .filter(|r| r["method"] == "eth_getBalance")
                .count(),
            1
        );
        assert!(!calls
            .iter()
            .any(|r| r["method"].as_str().unwrap().starts_with("eth_send")));
        for call in calls.iter().filter(|r| {
            r["method"] == "eth_call"
                && r["params"][0]["to"] == format!("0x{}", hex::encode([2; 20]))
        }) {
            assert_eq!(
                call["params"][1]["blockHash"],
                format!("0x{}", hex::encode([9; 32]))
            );
        }
    }
}

#[tokio::test]
async fn account_diagnostics_refuse_conflicts_malformed_words_and_changing_anchors_without_writes()
{
    for mode in [
        Mode::BalanceConflict,
        Mode::InvalidNative,
        Mode::InvalidToken,
        Mode::EmptyCode,
        Mode::GetterFailure,
        Mode::AnchorChange,
        Mode::FinalityRegression,
    ] {
        let (first, _, _a) = source(Mode::Honest).await;
        let (peer, _, _b) = source(mode).await;
        assert!(first
            .account_state_agreed(&peer, domain(), identity(), pins(), [4; 20])
            .await
            .is_err());
    }
    let (first, _, _a) = source(Mode::Honest).await;
    let (peer, _, _b) = source(Mode::Honest).await;
    let mut wrong = pins();
    wrong.pool = [0; 32];
    assert!(first
        .account_state_agreed(&peer, domain(), identity(), wrong, [4; 20])
        .await
        .is_err());
    assert!(first
        .account_state_agreed(&first, domain(), identity(), pins(), [4; 20])
        .await
        .is_err());
    assert!(first
        .account_state_agreed(
            &peer,
            domain(),
            PoolIdentity {
                asset: [5; 20],
                verifier_version: 2
            },
            pins(),
            [4; 20]
        )
        .await
        .is_err());
}
