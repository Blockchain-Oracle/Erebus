//! Adversarial admission regressions for the shielded sponsored relayer.
//!
//! These providers simulate RPC evidence, not contract execution or consensus finality.
//! Every case asserts that admission performed no write, no nonce read and no send.

use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

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
    relay_admission::{
        RelayAdmissionError, RelayAdmissionPolicy, RelayProofError, TransferVerifier,
    },
    rpc::{PoolRpc, PoolRuntimePins},
};
use sha2::{Digest, Sha256};

use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};
use sha3::Keccak256;

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

fn verifier() -> TransferVerifier {
    let bytes = key("transfer");
    TransferVerifier::from_zkey_bytes(&bytes, &format!("{:x}", Sha256::digest(&bytes))).unwrap()
}

fn hash(number: u64) -> String {
    let mut hash = [0; 32];
    hash[24..].copy_from_slice(&number.to_be_bytes());
    format!("0x{}", hex::encode(hash))
}

fn hash_bytes(number: u64) -> [u8; 32] {
    hex::decode(&hash(number)[2..]).unwrap().try_into().unwrap()
}

fn selector(signature: &str) -> Vec<u8> {
    Keccak256::digest(signature.as_bytes())[..4].to_vec()
}

fn code_pin(code: &[u8]) -> [u8; 32] {
    Keccak256::digest(code).into()
}

fn honest_pins() -> PoolRuntimePins {
    let pin = code_pin(&[0x60, 0x01]);
    PoolRuntimePins {
        pool: pin,
        poseidon: pin,
        deposit: pin,
        transfer: pin,
        withdraw: pin,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fault {
    None,
    /// Every read of the deployment creation block reports a different hash.
    WrongCreationHash,
    /// The creation block hash differs only after its first read.
    CreationHashChangedAfterFirstRead,
    /// Runtime code differs only at the head anchor.
    WrongRuntimeAtHead,
    /// Raw code bytes are not valid hex.
    UnparseableRuntimeCode,
    /// The head block hash changes once the first runtime read has happened.
    HeadHashChangedAfterSnapshot,
    /// The finalized block hash changes after the simulations.
    FinalizedHashChangedAfterSnapshot,
    /// Every block timestamp is one second past the proof expiry.
    LateHeadTimestamp,
    /// The simulation returns JSON null.
    SimulationNull,
    /// The simulation returns a number instead of a hex string.
    SimulationNumber,
    /// Identity getters fail with a JSON-RPC error.
    GetterError,
    /// A block header omits the timestamp field.
    BlockMissingTimestamp,
}

struct Provider {
    chain: u64,
    pool: [u8; 20],
    asset: [u8; 20],
    version: u32,
    head: u64,
    finalized: u64,
    first_block: u64,
    sponsor: [u8; 20],
    gas: u64,
    expiry: u64,
    data: Vec<u8>,
    fault: Fault,
    reads_of: Mutex<std::collections::HashMap<u64, usize>>,
    getters: AtomicUsize,
    simulations: AtomicUsize,
    writes: AtomicUsize,
}

impl Provider {
    fn block_hash(&self, number: u64) -> String {
        if number == self.first_block {
            match self.fault {
                Fault::WrongCreationHash => return hash(999_999),
                Fault::CreationHashChangedAfterFirstRead => {
                    let mut reads = self.reads_of.lock().unwrap();
                    let count = reads.entry(number).or_insert(0);
                    *count += 1;
                    if *count > 1 {
                        return hash(999_998);
                    }
                }
                _ => {}
            }
        }
        if number == self.head
            && self.fault == Fault::HeadHashChangedAfterSnapshot
            && self.getters.load(Ordering::SeqCst) > 0
        {
            return hash(999_997);
        }
        if number == self.finalized
            && self.fault == Fault::FinalizedHashChangedAfterSnapshot
            && self.simulations.load(Ordering::SeqCst) > 0
        {
            return hash(999_996);
        }
        hash(number)
    }

    fn timestamp(&self) -> String {
        if self.fault == Fault::LateHeadTimestamp {
            format!("0x{:x}", self.expiry + 1)
        } else {
            "0x1".to_owned()
        }
    }
}

async fn handle(State(state): State<Arc<Provider>>, Json(request): Json<Value>) -> Json<Value> {
    let params = &request["params"];
    let reply = |value: Value| Json(json!({"jsonrpc":"2.0", "id":request["id"], "result":value}));
    let reject = |message: &str| {
        state.writes.fetch_add(1, Ordering::SeqCst);
        Json(json!({"jsonrpc":"2.0", "id":request["id"],
            "error":{"code":-32601,"message":message}}))
    };
    match request["method"].as_str().unwrap() {
        "eth_chainId" => reply(json!(format!("0x{:x}", state.chain))),
        "eth_blockNumber" => reply(json!(format!("0x{:x}", state.head))),
        "eth_getBlockByNumber" => {
            let number = if params[0] == "finalized" {
                state.finalized
            } else {
                u64::from_str_radix(params[0].as_str().unwrap().strip_prefix("0x").unwrap(), 16)
                    .unwrap()
            };
            let mut result = json!({"number":format!("0x{number:x}"),
                "hash":state.block_hash(number),
                "parentHash":hash(number.saturating_sub(1)),
                "timestamp":state.timestamp()});
            if state.fault == Fault::BlockMissingTimestamp
                && (params[0] == "finalized" || number == state.head)
            {
                result.as_object_mut().unwrap().remove("timestamp");
            }
            reply(result)
        }
        "eth_getCode" => {
            state.getters.fetch_add(1, Ordering::SeqCst);
            let block_hash = params[1]["blockHash"].as_str().unwrap_or_default();
            if state.fault == Fault::UnparseableRuntimeCode {
                return reply(json!("0xzz"));
            }
            if state.fault == Fault::WrongRuntimeAtHead && block_hash == hash(state.head) {
                return reply(json!("0x6002"));
            }
            reply(json!("0x6001"))
        }
        "eth_call" => {
            let data = hex::decode(
                params[0]["data"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("0x")
                    .unwrap(),
            )
            .unwrap();
            if data.len() > 4 {
                // Simulation: assert the exact operator policy is used, never substituted.
                assert_eq!(data, state.data, "simulated calldata differs");
                assert_eq!(
                    params[0]["to"],
                    format!("0x{}", hex::encode(state.pool)),
                    "simulation target differs"
                );
                assert_eq!(
                    params[0]["from"],
                    format!("0x{}", hex::encode(state.sponsor)),
                    "simulation sponsor differs"
                );
                assert_eq!(
                    params[0]["gas"],
                    format!("0x{:x}", state.gas),
                    "simulation gas differs"
                );
                assert_eq!(params[0]["value"], "0x0", "simulation value differs");
                assert_eq!(params[1]["requireCanonical"], true);
                assert_eq!(params[1]["blockHash"], hash(state.head));
                state.simulations.fetch_add(1, Ordering::SeqCst);
                return match state.fault {
                    Fault::SimulationNull => reply(json!(null)),
                    Fault::SimulationNumber => reply(json!(42)),
                    _ => reply(json!("0x")),
                };
            }
            if state.fault == Fault::GetterError {
                return Json(json!({"jsonrpc":"2.0", "id":request["id"],
                    "error":{"code":3,"message":"getter reverted"}}));
            }
            let mut word = [0; 32];
            if data == selector("asset()") {
                word[12..].copy_from_slice(&state.asset);
            } else if data == selector("verifierVersion()") {
                word[28..].copy_from_slice(&state.version.to_be_bytes());
            } else {
                assert!(
                    [
                        "poseidonTwo()",
                        "depositVerifier()",
                        "transferVerifier()",
                        "withdrawVerifier()"
                    ]
                    .iter()
                    .any(|signature| data == selector(signature)),
                    "unexpected getter"
                );
                word[12..].fill(2);
            }
            reply(json!(format!("0x{}", hex::encode(word))))
        }
        "eth_sendRawTransaction"
        | "eth_sendTransaction"
        | "eth_signTransaction"
        | "eth_sign"
        | "eth_accounts" => reject("admission must never sign or send"),
        "eth_getTransactionCount" => reject("admission must not read the nonce"),
        other => reject(&format!("unexpected method {other}")),
    }
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

async fn provider(
    data: &[u8],
    chain_id: u64,
    pool: [u8; 20],
    version: u32,
    first_block: u64,
    sponsor: [u8; 20],
    gas: u64,
    fault: Fault,
) -> Server {
    let state = Arc::new(Provider {
        chain: chain_id,
        pool,
        asset: signal(data, 3)[12..].try_into().unwrap(),
        version,
        head: 95,
        finalized: 85,
        first_block,
        sponsor,
        gas,
        expiry: u64::from_be_bytes(signal(data, 10)[24..].try_into().unwrap()),
        data: data.to_vec(),
        fault,
        reads_of: Mutex::new(std::collections::HashMap::new()),
        getters: AtomicUsize::new(0),
        simulations: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn({
        let state = state.clone();
        async move {
            axum::serve(
                listener,
                Router::new().route("/", post(handle)).with_state(state),
            )
            .await
            .unwrap();
        }
    });
    Server {
        rpc: PoolRpc::new(&url, chain_id, pool).unwrap(),
        state,
        task,
    }
}

fn policy_for(
    context: &SettlementContext,
    data: &[u8],
    first_block: u64,
    sponsor: [u8; 20],
    gas_limit: u64,
    pins: PoolRuntimePins,
) -> RelayAdmissionPolicy {
    RelayAdmissionPolicy {
        deployment: IndexDomain {
            chain_id: u64::from_be_bytes(signal(data, 0)[24..].try_into().unwrap()),
            pool: signal(data, 1)[12..].try_into().unwrap(),
            first_block,
            first_hash: hash_bytes(first_block),
        },
        context: context.clone(),
        runtime_pins: pins,
        sponsor,
        gas_limit,
    }
}

fn assert_quiet(primary: &Server, peer: &Server) {
    for server in [primary, peer] {
        assert_eq!(
            server.state.writes.load(Ordering::SeqCst),
            0,
            "admission attempted a write, a nonce read or another method"
        );
    }
}

async fn both(data: &[u8], policy: &RelayAdmissionPolicy, fault: Fault) -> (Server, Server) {
    let primary = provider(
        data,
        policy.deployment.chain_id,
        policy.deployment.pool,
        policy.context.domain.verifier_version,
        policy.deployment.first_block,
        policy.sponsor,
        policy.gas_limit,
        Fault::None,
    )
    .await;
    let peer = provider(
        data,
        policy.deployment.chain_id,
        policy.deployment.pool,
        policy.context.domain.verifier_version,
        policy.deployment.first_block,
        policy.sponsor,
        policy.gas_limit,
        fault,
    )
    .await;
    (primary, peer)
}

#[tokio::test]
#[ignore = "requires the retained M5 transfer proof and known-entropy key"]
async fn admission_refuses_changed_creation_anchors_before_simulating() {
    let data = transfer_data();
    let (context, prepared) = context_and_prepared(data.clone());
    let verifier = verifier();
    for fault in [
        Fault::WrongCreationHash,
        Fault::CreationHashChangedAfterFirstRead,
    ] {
        let policy = policy_for(&context, &data, 1, [9; 20], 3_000_000, honest_pins());
        let (primary, peer) = both(&data, &policy, fault).await;
        let outcome = verifier
            .admit(&policy, &prepared, &primary.rpc, &peer.rpc)
            .await;
        assert!(
            outcome.is_err(),
            "creation anchor fault must refuse admission"
        );
        assert_eq!(
            peer.state.simulations.load(Ordering::SeqCst),
            usize::from(fault == Fault::CreationHashChangedAfterFirstRead),
            "a changed creation anchor must refuse before simulating"
        );
        assert_quiet(&primary, &peer);
    }
}

#[tokio::test]
#[ignore = "requires the retained M5 transfer proof and known-entropy key"]
async fn admission_refuses_runtime_substitution_and_identity_getter_faults() {
    let data = transfer_data();
    let (context, prepared) = context_and_prepared(data.clone());
    let verifier = verifier();
    for fault in [
        Fault::WrongRuntimeAtHead,
        Fault::UnparseableRuntimeCode,
        Fault::GetterError,
    ] {
        let policy = policy_for(&context, &data, 1, [9; 20], 3_000_000, honest_pins());
        let (primary, peer) = both(&data, &policy, fault).await;
        let outcome = verifier
            .admit(&policy, &prepared, &primary.rpc, &peer.rpc)
            .await;
        assert!(
            outcome.is_err(),
            "runtime or getter fault must refuse admission"
        );
        assert_eq!(
            peer.state.simulations.load(Ordering::SeqCst),
            0,
            "code or identity faults must refuse before simulating"
        );
        assert_eq!(primary.state.writes.load(Ordering::SeqCst), 0);
        assert_eq!(peer.state.writes.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
#[ignore = "requires the retained M5 transfer proof and known-entropy key"]
async fn admission_refuses_zero_and_mismatched_pins_before_any_rpc() {
    let data = transfer_data();
    let (context, prepared) = context_and_prepared(data.clone());
    let verifier = verifier();
    let zero = policy_for(
        &context,
        &data,
        1,
        [9; 20],
        3_000_000,
        PoolRuntimePins {
            pool: [0; 32],
            poseidon: [0; 32],
            deposit: [0; 32],
            transfer: [0; 32],
            withdraw: [0; 32],
        },
    );
    let (primary, peer) = both(&data, &zero, Fault::None).await;
    assert!(matches!(
        verifier
            .admit(&zero, &prepared, &primary.rpc, &peer.rpc)
            .await,
        Err(RelayAdmissionError::Policy)
    ));
    assert_eq!(primary.state.simulations.load(Ordering::SeqCst), 0);
    assert_quiet(&primary, &peer);

    let mismatch = policy_for(
        &context,
        &data,
        1,
        [9; 20],
        3_000_000,
        PoolRuntimePins {
            pool: code_pin(&[0x60, 0x02]),
            ..honest_pins()
        },
    );
    let (primary, peer) = both(&data, &mismatch, Fault::None).await;
    assert!(verifier
        .admit(&mismatch, &prepared, &primary.rpc, &peer.rpc)
        .await
        .is_err());
    assert_eq!(peer.state.simulations.load(Ordering::SeqCst), 0);
    assert_quiet(&primary, &peer);
}

#[tokio::test]
#[ignore = "requires the retained M5 transfer proof and known-entropy key"]
async fn admission_refuses_head_and_finality_changes_after_reads() {
    let data = transfer_data();
    let (context, prepared) = context_and_prepared(data.clone());
    let verifier = verifier();
    for fault in [
        Fault::HeadHashChangedAfterSnapshot,
        Fault::FinalizedHashChangedAfterSnapshot,
    ] {
        let policy = policy_for(&context, &data, 1, [9; 20], 3_000_000, honest_pins());
        let (primary, peer) = both(&data, &policy, fault).await;
        assert!(
            verifier
                .admit(&policy, &prepared, &primary.rpc, &peer.rpc)
                .await
                .is_err(),
            "a post-read anchor change must refuse admission"
        );
        assert!(
            peer.state.simulations.load(Ordering::SeqCst) <= 1,
            "a changed anchor must not be simulated repeatedly"
        );
        assert_quiet(&primary, &peer);
    }
}

#[tokio::test]
#[ignore = "requires the retained M5 transfer proof and known-entropy key"]
async fn admission_enforces_expiry_against_the_paired_head_timestamp() {
    let data = transfer_data();
    let (context, prepared) = context_and_prepared(data.clone());
    let verifier = verifier();
    let policy = policy_for(&context, &data, 1, [9; 20], 3_000_000, honest_pins());
    let primary = provider(
        &data,
        policy.deployment.chain_id,
        policy.deployment.pool,
        policy.context.domain.verifier_version,
        policy.deployment.first_block,
        policy.sponsor,
        policy.gas_limit,
        Fault::LateHeadTimestamp,
    )
    .await;
    let peer = provider(
        &data,
        policy.deployment.chain_id,
        policy.deployment.pool,
        policy.context.domain.verifier_version,
        policy.deployment.first_block,
        policy.sponsor,
        policy.gas_limit,
        Fault::LateHeadTimestamp,
    )
    .await;
    let outcome = verifier
        .admit(&policy, &prepared, &primary.rpc, &peer.rpc)
        .await;
    assert!(matches!(
        outcome,
        Err(RelayAdmissionError::Proof(RelayProofError::Expired))
    ));
    assert_eq!(primary.state.simulations.load(Ordering::SeqCst), 0);
    assert_eq!(peer.state.simulations.load(Ordering::SeqCst), 0);
    assert_quiet(&primary, &peer);
}

#[tokio::test]
#[ignore = "requires the retained M5 transfer proof and known-entropy key"]
async fn admission_refuses_malformed_simulation_and_block_responses() {
    let data = transfer_data();
    let (context, prepared) = context_and_prepared(data.clone());
    let verifier = verifier();
    for fault in [
        Fault::SimulationNull,
        Fault::SimulationNumber,
        Fault::BlockMissingTimestamp,
    ] {
        let policy = policy_for(&context, &data, 1, [9; 20], 3_000_000, honest_pins());
        let (primary, peer) = both(&data, &policy, fault).await;
        assert!(
            verifier
                .admit(&policy, &prepared, &primary.rpc, &peer.rpc)
                .await
                .is_err(),
            "a malformed response must refuse admission"
        );
        assert_quiet(&primary, &peer);
    }
}

/// The happy path pins the exact operator sponsor, gas cap and zero value into both
/// simulations; a distinct policy identity is passed through and never substituted.
#[tokio::test]
#[ignore = "requires the retained M5 transfer proof and known-entropy key"]
async fn admission_simulates_with_the_exact_operator_sponsor_gas_and_value() {
    let data = transfer_data();
    let (context, prepared) = context_and_prepared(data.clone());
    let verifier = verifier();
    let policy = policy_for(&context, &data, 1, [9; 20], 0x2f_0000, honest_pins());
    let (primary, peer) = both(&data, &policy, Fault::None).await;
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
    assert_quiet(&primary, &peer);
}

/// Observer checks: the outer sender is the operator's sponsor while the inner calldata is
/// unchanged public proof data that contains no participant secrets; errors and statuses are
/// redacted.
#[tokio::test]
#[ignore = "requires the retained M5 transfer proof and known-entropy key"]
async fn sponsor_is_the_public_sender_and_public_calldata_carries_no_participant_keys() {
    use erebus_core::shielded_auth::derive_key;
    use erebus_evm::chain::{
        Eip1559Fees, SignedTransaction, SigningPlan, TransactionKey, TransactionParams,
    };

    let data = transfer_data();
    let (context, prepared) = context_and_prepared(data.clone());
    // The transport message is exactly the bounded public proof envelope.
    let request =
        erebus_shielded_prover::relay_request::RelayRequest::from_prepared(&context, &prepared)
            .unwrap();
    let encoded: Value = serde_json::from_slice(&request.encode().unwrap()).unwrap();
    assert_eq!(encoded.as_object().unwrap().len(), 2);
    assert_eq!(encoded["calldata"], format!("0x{}", hex::encode(&data)));

    // Private participant material must not appear anywhere in the public calldata.
    let mut private_material: Vec<[u8; 32]> = vec![
        [0x06; 32], // fixture spend secret
        [0x07; 32], // fixture salt
    ];
    for label in [
        b"EREBUS_M5_BUYER_TEST_ONLY".as_slice(),
        b"EREBUS_M5_SELLER_TEST_ONLY",
    ] {
        let seed: [u8; 32] = Sha256::digest(label).into();
        private_material.push(seed);
        let key = derive_key(&seed).unwrap();
        private_material.push(key[..32].try_into().unwrap());
        private_material.push(key[32..].try_into().unwrap());
    }
    for secret in private_material {
        'outer: for offset in 0..data.len().saturating_sub(31) {
            if data[offset..offset + 32] == secret {
                panic!("private participant material appears in public calldata at byte {offset}");
            }
            continue 'outer;
        }
    }

    // The outer transaction is signed by the sponsor gas account and carries the unchanged
    // pool call: the public sender is the relayer, not either participant.
    let key = TransactionKey::from_bytes(&[0x77; 32]).unwrap();
    let plan = SigningPlan::new(
        key.address(),
        TransactionParams {
            nonce: 0,
            gas_limit: 3_000_000,
            fees: Eip1559Fees::new(300, 2).unwrap(),
            value: 0,
        },
    )
    .unwrap();
    let pool: [u8; 20] = signal(&data, 1)[12..].try_into().unwrap();
    let signed = plan.sign_call(10143, pool, data.clone(), &key).unwrap();
    let decoded = SignedTransaction::from_raw(signed.raw()).unwrap();
    assert_eq!(decoded.sender(), key.address());
    assert_eq!(decoded.to(), pool);
    assert_eq!(decoded.params().value, 0);
    assert_eq!(decoded.input(), &data[..]);

    // Admission error messages are fixed, redacted strings; admission results expose only
    // public anchors and a calldata digest.
    let verifier = verifier();
    let zero = policy_for(
        &context,
        &data,
        1,
        [9; 20],
        3_000_000,
        PoolRuntimePins {
            pool: [0; 32],
            poseidon: [0; 32],
            deposit: [0; 32],
            transfer: [0; 32],
            withdraw: [0; 32],
        },
    );
    let (primary, peer) = both(&data, &zero, Fault::None).await;
    let error = verifier
        .admit(&zero, &prepared, &primary.rpc, &peer.rpc)
        .await
        .unwrap_err();
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(&hex::encode(&data[4..68])));
    assert!(!rendered.contains(&hex::encode(signal(&data, 4))));
    assert!(!rendered.contains(&hex::encode(zero.sponsor)));
    let mut invalid = prepared.clone();
    invalid.backend_evidence[4] ^= 1;
    let error = verifier.verify(&context, &invalid, 0).unwrap_err();
    let rendered = format!("{error:?} {error}");
    assert!(!rendered.contains(&hex::encode(&invalid.backend_evidence[4..68])));
}
