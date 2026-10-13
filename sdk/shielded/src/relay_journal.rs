//! Sponsor-owned submission journal. It never receives agreement openings or witnesses.
//!
//! Proof admission, funding, the account-wide SignerJournal and finalized reconciliation
//! remain service responsibilities. This journal fences bytes, not payment completion.

use std::{fmt, path::Path, sync::Arc};

use erebus_core::settlement::{PreparedSettlement, SettlementContext};
use erebus_evm::chain::{Eip1559Fees, FinalizedNonce, Inclusion, SignedTransaction, SigningPlan};
use erebus_journal::{FaultHook, JournalRecord, NoFaults, RecordId, Store};
use serde::{Deserialize, Serialize};

use crate::relay_request::RelayRequest;

/// Redacted storage and intent errors. None establishes nonpayment.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RelayJournalError {
    /// Corruption or an uncertain write requires retained-state recovery.
    #[error("shielded relay journal unavailable; retain the operation")]
    Storage,
    /// The deterministic payment identity already names different call bytes or a plan.
    #[error("shielded relay request conflicts with retained intent")]
    Conflict,
    /// A transaction does not match the durable zero-value sponsor call.
    #[error("shielded relay transaction does not match retained intent")]
    Transaction,
    /// Invalid operator-owned configuration, never a client fee policy.
    #[error("invalid shielded relay journal policy")]
    Policy,
}

#[cfg(test)]
mod tests {
    use super::*;
    use erebus_evm::chain::{TransactionKey, TransactionParams};
    use erebus_journal::Boundary;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Barrier,
    };

    fn journal(root: &Path) -> RelayJournal {
        let (context, _) = crate::relay_request::tests::fixture();
        let key = TransactionKey::from_bytes(&[9; 32]).unwrap();
        RelayJournal::open(
            root,
            context,
            [1; 32],
            key.address(),
            3_000_000,
            Eip1559Fees::new(300, 2).unwrap(),
        )
        .unwrap()
    }
    fn request() -> RelayRequest {
        let (context, prepared) = crate::relay_request::tests::fixture();
        RelayRequest::from_prepared(&context, &prepared).unwrap()
    }

    #[test]
    fn finalized_success_requires_exact_untruncated_pool_effects() {
        use erebus_core::suite::keccak256;
        use erebus_evm::chain::{BlockRef, InclusionLog};
        let (_, mut prepared) = crate::relay_request::tests::fixture();
        let signals = &mut prepared.backend_evidence[4 + 8 * 32..];
        signals[8 * 32..9 * 32].fill(5);
        signals[9 * 32..10 * 32].fill(6);
        let word = |index: usize| -> [u8; 32] {
            signals[index * 32..(index + 1) * 32].try_into().unwrap()
        };
        let note_topic = keccak256(&[b"NoteInserted(uint256,uint256,uint256)"]);
        let mut receipt = Inclusion {
            block: BlockRef {
                number: 1,
                hash: [1; 32],
                timestamp: 1,
            },
            success: true,
            from: [2; 20],
            to: Some([1; 20]),
            nonce: 0,
            value: 0,
            logs_truncated: false,
            transaction_index: 0,
            logs: vec![
                InclusionLog {
                    address: [1; 20],
                    topics: vec![note_topic, [0; 32], word(8)],
                    data: vec![0; 32],
                    log_index: 0,
                },
                InclusionLog {
                    address: [1; 20],
                    topics: vec![note_topic, [1; 32], word(9)],
                    data: vec![0; 32],
                    log_index: 1,
                },
                InclusionLog {
                    address: [1; 20],
                    topics: vec![
                        keccak256(&[b"DealTransferred(uint256,uint256,uint256)"]),
                        word(4),
                        word(5),
                        word(7),
                    ],
                    data: vec![],
                    log_index: 2,
                },
            ],
        };
        assert!(validate_effects([1; 20], &prepared.backend_evidence, &receipt).is_ok());
        receipt.logs_truncated = true;
        assert!(validate_effects([1; 20], &prepared.backend_evidence, &receipt).is_err());
        receipt.logs_truncated = false;
        receipt.logs[2].topics[2] = [9; 32];
        assert!(validate_effects([1; 20], &prepared.backend_evidence, &receipt).is_err());
        receipt.logs[2].topics[2] = [4; 32];
        receipt.logs[0].address = [2; 20];
        assert!(validate_effects([1; 20], &prepared.backend_evidence, &receipt).is_err());
    }
    fn plan(nonce: u64) -> SigningPlan {
        SigningPlan::new(
            TransactionKey::from_bytes(&[9; 32]).unwrap().address(),
            TransactionParams {
                nonce,
                gas_limit: 3_000_000,
                value: 0,
                fees: Eip1559Fees::new(300, 2).unwrap(),
            },
        )
        .unwrap()
    }
    fn sign(plan: &SigningPlan) -> Result<SignedTransaction, RelayJournalError> {
        let (_, prepared) = crate::relay_request::tests::fixture();
        plan.sign_call(
            31337,
            [1; 20],
            prepared.backend_evidence,
            &TransactionKey::from_bytes(&[9; 32]).unwrap(),
        )
        .map_err(|_| RelayJournalError::Transaction)
    }

    #[test]
    fn sponsor_bytes_and_initial_broadcast_are_fenced_across_restart() {
        let root = tempfile::tempdir().unwrap();
        let store = journal(root.path());
        let id = store.retain(&request()).unwrap();
        assert_eq!(store.status(id).unwrap().transaction_hash, None);
        let signed = store.sign_once(id, &plan(7), sign).unwrap();
        let reopened = journal(root.path());
        let restored = reopened
            .sign_once(id, &plan(7), |_| panic!("must not sign again"))
            .unwrap();
        assert_eq!(signed.raw(), restored.raw());
        assert_eq!(
            reopened
                .begin_initial_broadcast(id)
                .unwrap()
                .unwrap()
                .hash(),
            signed.hash()
        );
        assert!(journal(root.path())
            .begin_initial_broadcast(id)
            .unwrap()
            .is_none());
        assert!(journal(root.path()).status(id).unwrap().broadcast_started);
        assert!(matches!(
            reopened.sign_once(id, &plan(8), sign),
            Err(RelayJournalError::Conflict)
        ));
    }

    #[test]
    fn explicit_owner_resumes_retain_identical_bytes_and_are_bounded_across_restart() {
        let root = tempfile::tempdir().unwrap();
        let store = journal(root.path());
        let id = store.retain(&request()).unwrap();
        assert!(store.begin_explicit_resume(id).is_err());
        let original = store.sign_once(id, &plan(7), sign).unwrap();
        for _ in 0..16 {
            let resumed = journal(root.path())
                .begin_explicit_resume(id)
                .unwrap()
                .unwrap();
            assert_eq!(resumed.raw(), original.raw());
            assert_eq!(resumed.params().nonce, 7);
        }
        assert!(matches!(
            journal(root.path()).begin_explicit_resume(id),
            Err(RelayJournalError::Policy)
        ));
        let record_id = RelayId(hex::encode(id));
        let mut record = store.load(&record_id).unwrap();
        record.finalized = Some(RelayFinalized {
            block_number: 1,
            block_hash: [1; 32],
            success: true,
        });
        store.store.write(&record).unwrap();
        assert!(journal(root.path())
            .begin_explicit_resume(id)
            .unwrap()
            .is_none());
    }

    #[test]
    fn fee_replacement_preserves_intent_history_and_durable_send_fences() {
        let root = tempfile::tempdir().unwrap();
        let ceiling = Eip1559Fees::new(600, 6).unwrap();
        let open = || {
            journal(root.path())
                .with_replacement_ceiling(ceiling)
                .unwrap()
        };
        let store = open();
        let id = store.retain(&request()).unwrap();
        let original = store.sign_once(id, &plan(7), sign).unwrap();
        assert!(store.sign_replacement(id, ceiling, sign).is_err());
        store.begin_initial_broadcast(id).unwrap().unwrap();
        assert!(
            journal(root.path()).status(id).is_err(),
            "policy cannot silently change on restart"
        );
        assert!(store
            .sign_replacement(id, Eip1559Fees::new(700, 7).unwrap(), sign)
            .is_err());
        assert!(store
            .sign_replacement(id, Eip1559Fees::new(301, 3).unwrap(), sign)
            .is_err());
        assert!(
            store
                .sign_replacement(id, ceiling, |p| {
                    let mut params = p.params();
                    params.value = 1;
                    sign(&SigningPlan::new(p.sender(), params).unwrap())
                })
                .is_err(),
            "fee replacement must never change native value"
        );
        let replacement = store.sign_replacement(id, ceiling, sign).unwrap();
        replacement.validate_replacement(&original).unwrap();
        assert_eq!(replacement.params().nonce, 7);
        assert_eq!(
            open().attempts(id).unwrap(),
            [original.hash(), replacement.hash()]
        );
        let restored = open()
            .sign_replacement(id, ceiling, |_| {
                panic!("retained bytes must not be signed twice")
            })
            .unwrap();
        assert_eq!(restored.raw(), replacement.raw());
        assert!(open()
            .begin_replacement_broadcast(id, original.hash())
            .is_err());
        assert_eq!(
            open()
                .begin_replacement_broadcast(id, replacement.hash())
                .unwrap()
                .unwrap()
                .raw(),
            replacement.raw()
        );
        assert!(open()
            .begin_replacement_broadcast(id, replacement.hash())
            .unwrap()
            .is_none());
        assert_eq!(
            open().begin_explicit_resume(id).unwrap().unwrap().raw(),
            replacement.raw()
        );
        let record_id = RelayId(hex::encode(id));
        let mut record = store.load(&record_id).unwrap();
        record.finalized = Some(RelayFinalized {
            block_number: 1,
            block_hash: [1; 32],
            success: true,
        });
        record.finalized_hash = Some(original.hash());
        store.store.write(&record).unwrap();
        assert_eq!(
            open().status(id).unwrap().transaction_hash,
            Some(original.hash())
        );
        assert!(open().begin_explicit_resume(id).unwrap().is_none());
        assert!(open()
            .begin_replacement_broadcast(id, replacement.hash())
            .unwrap()
            .is_none());
        assert!(open().sign_replacement(id, ceiling, sign).is_err());
        record.replacements[0].raw[5] ^= 1;
        store.store.write(&record).unwrap();
        assert!(
            open().status(id).is_err(),
            "corrupt retained history is not absence"
        );
    }

    #[test]
    fn copied_proof_conflicts_and_invalid_signed_intent_never_becomes_sendable() {
        let root = tempfile::tempdir().unwrap();
        let store = journal(root.path());
        let id = store.retain(&request()).unwrap();
        let (context, mut other) = crate::relay_request::tests::fixture();
        other.backend_evidence[4] = 1;
        assert!(matches!(
            store.retain(&RelayRequest::from_prepared(&context, &other).unwrap()),
            Err(RelayJournalError::Conflict)
        ));
        assert!(matches!(
            store.sign_once(id, &plan(7), |p| {
                p.sign_call(
                    31337,
                    [2; 20],
                    other.backend_evidence,
                    &TransactionKey::from_bytes(&[9; 32]).unwrap(),
                )
                .map_err(|_| RelayJournalError::Transaction)
            }),
            Err(RelayJournalError::Transaction)
        ));
        assert!(store.begin_initial_broadcast(id).is_err());
        assert_eq!(store.status(id).unwrap().transaction_hash, None);
        store.sign_once(id, &plan(7), sign).unwrap();
        let path = store.store.record_path(&RelayId(hex::encode(id)));
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(
            store.retain(&request()),
            Err(RelayJournalError::Storage)
        ));
    }

    #[test]
    fn concurrent_sponsor_callers_run_one_signer_and_authorize_one_initial_send() {
        let root = tempfile::tempdir().unwrap();
        let id = journal(root.path()).retain(&request()).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let calls = Arc::new(AtomicUsize::new(0));
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let store = journal(root.path());
                let barrier = barrier.clone();
                let calls = calls.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store
                        .sign_once(id, &plan(7), |p| {
                            calls.fetch_add(1, Ordering::SeqCst);
                            sign(p)
                        })
                        .unwrap();
                    store.begin_initial_broadcast(id).unwrap().is_some()
                })
            })
            .collect();
        let sends = handles
            .into_iter()
            .map(|h| usize::from(h.join().unwrap()))
            .sum::<usize>();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(sends, 1);
    }

    struct FailAt {
        at: usize,
        count: AtomicUsize,
    }
    impl FaultHook for FailAt {
        fn after(&self, _: Boundary<'_>) -> std::io::Result<()> {
            if self.count.fetch_add(1, Ordering::SeqCst) + 1 == self.at {
                Err(std::io::Error::other("injected write failure"))
            } else {
                Ok(())
            }
        }
    }
    fn faulty(root: &Path, faults: Arc<FailAt>) -> RelayJournal {
        let (context, _) = crate::relay_request::tests::fixture();
        RelayJournal::open_with_faults(
            root,
            context,
            [1; 32],
            TransactionKey::from_bytes(&[9; 32]).unwrap().address(),
            3_000_000,
            Eip1559Fees::new(300, 2).unwrap(),
            faults,
        )
        .unwrap()
    }

    #[test]
    fn replacement_write_faults_never_authorize_unretained_bytes_or_a_second_initial_send() {
        let ceiling = Eip1559Fees::new(600, 6).unwrap();
        let prepare = |root: &Path| {
            let store = journal(root).with_replacement_ceiling(ceiling).unwrap();
            let id = store.retain(&request()).unwrap();
            store.sign_once(id, &plan(7), sign).unwrap();
            store.begin_initial_broadcast(id).unwrap().unwrap();
            id
        };
        let measured = tempfile::tempdir().unwrap();
        let id = prepare(measured.path());
        let hook = Arc::new(FailAt {
            at: usize::MAX,
            count: AtomicUsize::new(0),
        });
        let store = faulty(measured.path(), hook.clone())
            .with_replacement_ceiling(ceiling)
            .unwrap();
        let replacement = store.sign_replacement(id, ceiling, sign).unwrap();
        store
            .begin_replacement_broadcast(id, replacement.hash())
            .unwrap()
            .unwrap();
        let boundaries = hook.count.load(Ordering::SeqCst);
        assert!(boundaries > 0);
        for at in 1..=boundaries {
            let root = tempfile::tempdir().unwrap();
            let id = prepare(root.path());
            let store = faulty(
                root.path(),
                Arc::new(FailAt {
                    at,
                    count: AtomicUsize::new(0),
                }),
            )
            .with_replacement_ceiling(ceiling)
            .unwrap();
            let first = store
                .sign_replacement(id, ceiling, sign)
                .and_then(|tx| store.begin_replacement_broadcast(id, tx.hash()));
            assert!(matches!(first, Err(RelayJournalError::Storage)));
            let reopened = journal(root.path())
                .with_replacement_ceiling(ceiling)
                .unwrap();
            let started = reopened.replacement_broadcast_started(id).unwrap();
            let tx = reopened.sign_replacement(id, ceiling, sign).unwrap();
            let sendable = reopened.begin_replacement_broadcast(id, tx.hash()).unwrap();
            assert_eq!(sendable.is_none(), started);
            assert!(reopened
                .begin_replacement_broadcast(id, tx.hash())
                .unwrap()
                .is_none());
            assert!(reopened.begin_initial_broadcast(id).unwrap().is_none());
            assert_eq!(reopened.attempts(id).unwrap().len(), 2);
            assert_eq!(tx.params().nonce, 7);
        }
    }

    #[test]
    fn sponsor_write_faults_never_authorize_an_unretained_or_second_initial_send() {
        let measured = tempfile::tempdir().unwrap();
        let id = journal(measured.path()).retain(&request()).unwrap();
        let hook = Arc::new(FailAt {
            at: usize::MAX,
            count: AtomicUsize::new(0),
        });
        let store = faulty(measured.path(), hook.clone());
        store.sign_once(id, &plan(7), sign).unwrap();
        store.begin_initial_broadcast(id).unwrap();
        let boundaries = hook.count.load(Ordering::SeqCst);
        assert!(boundaries > 0);
        for at in 1..=boundaries {
            let root = tempfile::tempdir().unwrap();
            let id = journal(root.path()).retain(&request()).unwrap();
            let hook = Arc::new(FailAt {
                at,
                count: AtomicUsize::new(0),
            });
            let store = faulty(root.path(), hook);
            let first = store
                .sign_once(id, &plan(7), sign)
                .and_then(|_| store.begin_initial_broadcast(id));
            assert!(matches!(first, Err(RelayJournalError::Storage)));
            let reopened = journal(root.path());
            let before = reopened.status(id).unwrap();
            reopened.sign_once(id, &plan(7), sign).unwrap();
            let outbound = reopened.begin_initial_broadcast(id).unwrap();
            assert_eq!(outbound.is_none(), before.broadcast_started);
            assert!(reopened.begin_initial_broadcast(id).unwrap().is_none());
            assert!(reopened.status(id).unwrap().transaction_hash.is_some());
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RelayId(String);
impl fmt::Display for RelayId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl RecordId for RelayId {
    fn as_file_stem(&self) -> &str {
        &self.0
    }
    fn from_file_stem(stem: &str) -> Option<Self> {
        (stem.len() == 64
            && stem
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| Self(stem.to_owned()))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelayRecord {
    version: u32,
    id: RelayId,
    policy_pin: [u8; 32],
    request: Vec<u8>,
    plan: Option<Vec<u8>>,
    signed: Option<Vec<u8>>,
    broadcast_started: bool,
    #[serde(default)]
    explicit_resumes: u32,
    #[serde(default)]
    replacements: Vec<RelayReplacement>,
    #[serde(default)]
    finalized_hash: Option<[u8; 32]>,
    #[serde(default)]
    finalized: Option<RelayFinalized>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RelayReplacement {
    raw: Vec<u8>,
    broadcast_started: bool,
}
impl JournalRecord for RelayRecord {
    type Id = RelayId;
    const CURRENT_VERSION: u32 = 1;
    const OLDEST_READABLE_VERSION: u32 = 1;
    fn version(&self) -> u32 {
        self.version
    }
    fn record_id(&self) -> &RelayId {
        &self.id
    }
    fn attempt_count(&self) -> usize {
        1
    }
}

/// Redacted durable progress. A hash or broadcast fence does not prove payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayJournalStatus {
    /// Service identity derived from chain, pool and deal nullifier.
    pub operation_ref: [u8; 32],
    /// Hash of retained signed bytes, not evidence of inclusion or finality.
    pub transaction_hash: Option<[u8; 32]>,
    /// The initial send was authorized durably; its actual outcome may be unknown.
    pub broadcast_started: bool,
    /// Verified own-receipt outcome, never inferred from consumed nonce alone.
    pub finalized: Option<RelayFinalized>,
}

/// Persisted own-attempt outcome after paired canonical finalized verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayFinalized {
    /// Canonical including height of the retained transaction.
    pub block_number: u64,
    /// Including block hash authenticated by both providers.
    pub block_hash: [u8; 32],
    /// True only for a successful receipt with matching pool effects.
    pub success: bool,
}

/// Exact call and signer policy for one independently pinned service configuration.
/// All processes using this service must share this root and the account nonce journal.
pub struct RelayJournal {
    store: Store<RelayRecord>,
    context: SettlementContext,
    policy_pin: [u8; 32],
    sponsor: [u8; 20],
    gas_limit: u64,
    fees: Eip1559Fees,
    replacement_ceiling: Option<Eip1559Fees>,
    chain_id: u64,
    pool: [u8; 20],
}

impl RelayJournal {
    pub(crate) fn try_execution_lock(
        root: &Path,
    ) -> Result<Option<erebus_journal::IdentityLock>, RelayJournalError> {
        Store::<RelayRecord>::open(root.to_path_buf())
            .map_err(|_| RelayJournalError::Storage)?
            .try_lock_identity()
            .map_err(|_| RelayJournalError::Storage)
    }
    /// Opens owner-only storage with fixed sender, gas and fee policy.
    pub fn open(
        root: impl AsRef<Path>,
        context: SettlementContext,
        policy_pin: [u8; 32],
        sponsor: [u8; 20],
        gas_limit: u64,
        fees: Eip1559Fees,
    ) -> Result<Self, RelayJournalError> {
        Self::open_with_faults(
            root,
            context,
            policy_pin,
            sponsor,
            gas_limit,
            fees,
            Arc::new(NoFaults),
        )
    }

    /// Opens the same storage with deterministic durable-boundary fault injection.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_faults(
        root: impl AsRef<Path>,
        context: SettlementContext,
        policy_pin: [u8; 32],
        sponsor: [u8; 20],
        gas_limit: u64,
        fees: Eip1559Fees,
        faults: Arc<dyn FaultHook>,
    ) -> Result<Self, RelayJournalError> {
        let chain_id = context
            .domain
            .namespace
            .reference()
            .parse::<u64>()
            .map_err(|_| RelayJournalError::Policy)?;
        let pool: [u8; 20] = context
            .domain
            .pool
            .as_ref()
            .ok_or(RelayJournalError::Policy)?
            .as_bytes()
            .try_into()
            .map_err(|_| RelayJournalError::Policy)?;
        if !root.as_ref().is_absolute()
            || chain_id == 0
            || pool == [0; 20]
            || sponsor == [0; 20]
            || policy_pin == [0; 32]
            || !(21_000..=30_000_000).contains(&gas_limit)
        {
            return Err(RelayJournalError::Policy);
        }
        Ok(Self {
            store: Store::open_with_fault_hook(root.as_ref().to_path_buf(), faults)
                .map_err(|_| RelayJournalError::Storage)?,
            context,
            policy_pin,
            sponsor,
            gas_limit,
            fees,
            replacement_ceiling: None,
            chain_id,
            pool,
        })
    }

    /// Must be fixed before the first request; changing it invalidates retained policy pins.
    pub fn with_replacement_ceiling(
        mut self,
        ceiling: Eip1559Fees,
    ) -> Result<Self, RelayJournalError> {
        if !ceiling.can_replace(&self.fees) {
            return Err(RelayJournalError::Policy);
        }
        self.policy_pin = erebus_core::suite::keccak256(&[
            b"erebus-shielded-sponsor-replacement-v1",
            &self.policy_pin,
            &ceiling.max_fee_per_gas().to_be_bytes(),
            &ceiling.max_priority_fee_per_gas().to_be_bytes(),
        ]);
        self.replacement_ceiling = Some(ceiling);
        Ok(self)
    }

    fn prepared(&self, record: &RelayRecord) -> Result<PreparedSettlement, RelayJournalError> {
        let prepared = RelayRequest::decode(&record.request)
            .and_then(|request| request.prepared(&self.context))
            .map_err(|_| RelayJournalError::Storage)?;
        if record.policy_pin != self.policy_pin
            || record.id.0 != hex::encode(prepared.operation_ref)
            || (record.broadcast_started && record.signed.is_none())
            || (record.finalized.is_some() && record.signed.is_none())
            || record.explicit_resumes > 16
            || (record.explicit_resumes != 0 && !record.broadcast_started)
            || record.replacements.len() > 16
            || (!record.replacements.is_empty() && !record.broadcast_started)
            || (record.finalized_hash.is_some() && record.finalized.is_none())
        {
            return Err(RelayJournalError::Storage);
        }
        if let Some(bytes) = &record.plan {
            let plan = SigningPlan::decode(bytes).map_err(|_| RelayJournalError::Storage)?;
            if plan.sender() != self.sponsor
                || plan.params().value != 0
                || plan.params().gas_limit != self.gas_limit
                || plan.params().fees != self.fees
            {
                return Err(RelayJournalError::Storage);
            }
            if let Some(raw) = &record.signed {
                plan.validate_call(self.chain_id, self.pool, &prepared.backend_evidence, raw)
                    .map_err(|_| RelayJournalError::Storage)?;
            }
        } else if record.signed.is_some() {
            return Err(RelayJournalError::Storage);
        }
        let mut previous = record
            .signed
            .as_ref()
            .map(|raw| SignedTransaction::from_raw(raw).map_err(|_| RelayJournalError::Storage))
            .transpose()?;
        let mut hashes = previous
            .as_ref()
            .map(|tx| vec![tx.hash()])
            .unwrap_or_default();
        for replacement in &record.replacements {
            let signed = SignedTransaction::from_raw(&replacement.raw)
                .map_err(|_| RelayJournalError::Storage)?;
            signed
                .validate_replacement(previous.as_ref().ok_or(RelayJournalError::Storage)?)
                .map_err(|_| RelayJournalError::Storage)?;
            let ceiling = self.replacement_ceiling.ok_or(RelayJournalError::Storage)?;
            if signed.params().fees.max_fee_per_gas() > ceiling.max_fee_per_gas()
                || signed.params().fees.max_priority_fee_per_gas()
                    > ceiling.max_priority_fee_per_gas()
                || previous.as_ref().is_some_and(|tx| {
                    record
                        .replacements
                        .iter()
                        .any(|r| r.raw == tx.raw() && !r.broadcast_started)
                })
            {
                return Err(RelayJournalError::Storage);
            }
            hashes.push(signed.hash());
            previous = Some(signed);
        }
        if record
            .finalized_hash
            .is_some_and(|hash| !hashes.contains(&hash))
        {
            return Err(RelayJournalError::Storage);
        }
        Ok(prepared)
    }

    fn load(&self, id: &RelayId) -> Result<RelayRecord, RelayJournalError> {
        let record = self
            .store
            .read(id)
            .map_err(|_| RelayJournalError::Storage)?
            .ok_or(RelayJournalError::Storage)?;
        self.prepared(&record)?;
        if self
            .store
            .read_blob(id, 0)
            .map_err(|_| RelayJournalError::Storage)?
            != record.request
        {
            return Err(RelayJournalError::Storage);
        }
        Ok(record)
    }

    /// Retains exact request bytes before allocating a nonce. Another proof under the same
    /// nullifier conflicts rather than overwriting the first call, even before signing.
    pub fn retain(&self, request: &RelayRequest) -> Result<[u8; 32], RelayJournalError> {
        let prepared = request
            .prepared(&self.context)
            .map_err(|_| RelayJournalError::Conflict)?;
        let bytes = request.encode().map_err(|_| RelayJournalError::Conflict)?;
        let id = RelayId(hex::encode(prepared.operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        if let Some(record) = self
            .store
            .read(&id)
            .map_err(|_| RelayJournalError::Storage)?
        {
            self.prepared(&record)?;
            if self
                .store
                .read_blob(&id, 0)
                .map_err(|_| RelayJournalError::Storage)?
                != record.request
            {
                return Err(RelayJournalError::Storage);
            }
            if record.request != bytes {
                return Err(RelayJournalError::Conflict);
            }
            self.store.sync().map_err(|_| RelayJournalError::Storage)?;
        } else {
            if self
                .store
                .blob_path(&id, 0)
                .try_exists()
                .map_err(|_| RelayJournalError::Storage)?
            {
                return Err(RelayJournalError::Storage);
            }
            let mut record = RelayRecord {
                version: 1,
                id,
                policy_pin: self.policy_pin,
                request: bytes.clone(),
                plan: None,
                signed: None,
                broadcast_started: false,
                explicit_resumes: 0,
                replacements: Vec::new(),
                finalized_hash: None,
                finalized: None,
            };
            // Marker deletion or an orphan marker is corruption, never a fresh operation.
            self.store
                .write_blob_then_record::<erebus_journal::StoreError<RelayId>>(
                    &mut record,
                    0,
                    &bytes,
                    |_| Ok(()),
                )
                .map_err(|_| RelayJournalError::Storage)?;
        }
        Ok(prepared.operation_ref)
    }

    /// Saves the account-journaled plan before invoking the signer. Retained bytes are
    /// returned on restart without running the callback again.
    pub fn sign_once<F>(
        &self,
        operation_ref: [u8; 32],
        plan: &SigningPlan,
        sign: F,
    ) -> Result<SignedTransaction, RelayJournalError>
    where
        F: FnOnce(&SigningPlan) -> Result<SignedTransaction, RelayJournalError>,
    {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        let mut record = self.load(&id)?;
        if plan.sender() != self.sponsor
            || plan.params().value != 0
            || plan.params().gas_limit != self.gas_limit
            || plan.params().fees != self.fees
        {
            return Err(RelayJournalError::Transaction);
        }
        let encoded = plan.encode();
        if record.plan.as_ref().is_some_and(|old| old != &encoded) {
            return Err(RelayJournalError::Conflict);
        }
        if let Some(raw) = &record.signed {
            self.store.sync().map_err(|_| RelayJournalError::Storage)?;
            return SignedTransaction::from_raw(raw).map_err(|_| RelayJournalError::Storage);
        }
        if record.plan.is_none() {
            record.plan = Some(encoded);
            self.store
                .write(&record)
                .map_err(|_| RelayJournalError::Storage)?;
        }
        let prepared = self.prepared(&record)?;
        let signed = sign(plan)?;
        plan.validate_call(
            self.chain_id,
            self.pool,
            &prepared.backend_evidence,
            signed.raw(),
        )
        .map_err(|_| RelayJournalError::Transaction)?;
        record.signed = Some(signed.raw().to_vec());
        self.store
            .write(&record)
            .map_err(|_| RelayJournalError::Storage)?;
        Ok(signed)
    }

    /// Atomic first-send authorization. An already-started call returns None, even if its
    /// response was lost. No caller may infer absence or sign another nonce from this.
    pub fn begin_initial_broadcast(
        &self,
        operation_ref: [u8; 32],
    ) -> Result<Option<SignedTransaction>, RelayJournalError> {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        let mut record = self.load(&id)?;
        if record.broadcast_started {
            return Ok(None);
        }
        if record.finalized.is_some() {
            return Ok(None);
        }
        let raw = record
            .signed
            .as_ref()
            .ok_or(RelayJournalError::Transaction)?;
        let signed = SignedTransaction::from_raw(raw).map_err(|_| RelayJournalError::Storage)?;
        record.broadcast_started = true;
        self.store
            .write(&record)
            .map_err(|_| RelayJournalError::Storage)?;
        Ok(Some(signed))
    }

    /// Explicit owner retry of the same signed bytes. This never signs or allocates a nonce.
    /// The retry count is durable before sending; terminal operations cannot be resumed.
    pub(crate) fn begin_explicit_resume(
        &self,
        operation_ref: [u8; 32],
    ) -> Result<Option<SignedTransaction>, RelayJournalError> {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        let mut record = self.load(&id)?;
        if record.finalized.is_some() {
            return Ok(None);
        }
        if record.explicit_resumes >= 16 {
            return Err(RelayJournalError::Policy);
        }
        let signed = Self::active_signed(&record)?;
        record.explicit_resumes += 1;
        record.broadcast_started = true;
        if let Some(replacement) = record.replacements.last_mut() {
            replacement.broadcast_started = true;
        }
        self.store
            .write(&record)
            .map_err(|_| RelayJournalError::Storage)?;
        Ok(Some(signed))
    }

    fn active_signed(record: &RelayRecord) -> Result<SignedTransaction, RelayJournalError> {
        let raw = record
            .replacements
            .last()
            .map(|r| r.raw.as_slice())
            .or(record.signed.as_deref())
            .ok_or(RelayJournalError::Transaction)?;
        SignedTransaction::from_raw(raw).map_err(|_| RelayJournalError::Storage)
    }

    fn transaction_hash(record: &RelayRecord) -> Result<Option<[u8; 32]>, RelayJournalError> {
        if record.signed.is_none() {
            return Ok(None);
        }
        Ok(Some(
            record
                .finalized_hash
                .unwrap_or(Self::active_signed(record)?.hash()),
        ))
    }

    pub(crate) fn replacement_broadcast_started(
        &self,
        operation_ref: [u8; 32],
    ) -> Result<bool, RelayJournalError> {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        Ok(self
            .load(&id)?
            .replacements
            .last()
            .is_some_and(|r| r.broadcast_started))
    }

    /// All retained same-nonce attempts remain observable, including older potential winners.
    pub(crate) fn attempts(
        &self,
        operation_ref: [u8; 32],
    ) -> Result<Vec<[u8; 32]>, RelayJournalError> {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        let record = self.load(&id)?;
        record
            .signed
            .iter()
            .map(|r| r.as_slice())
            .chain(record.replacements.iter().map(|r| r.raw.as_slice()))
            .map(|raw| {
                SignedTransaction::from_raw(raw)
                    .map(|tx| tx.hash())
                    .map_err(|_| RelayJournalError::Storage)
            })
            .collect()
    }

    /// Persists a fee-only replacement before authorizing its first broadcast.
    pub(crate) fn sign_replacement<F>(
        &self,
        operation_ref: [u8; 32],
        fees: Eip1559Fees,
        sign: F,
    ) -> Result<SignedTransaction, RelayJournalError>
    where
        F: FnOnce(&SigningPlan) -> Result<SignedTransaction, RelayJournalError>,
    {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        let mut record = self.load(&id)?;
        let ceiling = self.replacement_ceiling.ok_or(RelayJournalError::Policy)?;
        if record.finalized.is_some()
            || !record.broadcast_started
            || fees.max_fee_per_gas() > ceiling.max_fee_per_gas()
            || fees.max_priority_fee_per_gas() > ceiling.max_priority_fee_per_gas()
        {
            return Err(RelayJournalError::Policy);
        }
        let previous = Self::active_signed(&record)?;
        if previous.params().fees == fees && !record.replacements.is_empty() {
            self.store.sync().map_err(|_| RelayJournalError::Storage)?;
            return Ok(previous);
        }
        if record.replacements.len() >= 16
            || !fees.can_replace(&previous.params().fees)
            || record
                .replacements
                .last()
                .is_some_and(|r| !r.broadcast_started)
        {
            return Err(RelayJournalError::Policy);
        }
        let plan = SigningPlan::new(
            self.sponsor,
            erebus_evm::chain::TransactionParams {
                fees,
                ..previous.params()
            },
        )
        .map_err(|_| RelayJournalError::Transaction)?;
        let signed = sign(&plan)?;
        signed
            .validate_replacement(&previous)
            .map_err(|_| RelayJournalError::Transaction)?;
        record.replacements.push(RelayReplacement {
            raw: signed.raw().to_vec(),
            broadcast_started: false,
        });
        self.store
            .write(&record)
            .map_err(|_| RelayJournalError::Storage)?;
        Ok(signed)
    }

    pub(crate) fn begin_replacement_broadcast(
        &self,
        operation_ref: [u8; 32],
        hash: [u8; 32],
    ) -> Result<Option<SignedTransaction>, RelayJournalError> {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        let mut record = self.load(&id)?;
        if record.finalized.is_some() {
            return Ok(None);
        }
        let signed = Self::active_signed(&record)?;
        if signed.hash() != hash {
            return Err(RelayJournalError::Conflict);
        }
        let replacement = record
            .replacements
            .last_mut()
            .ok_or(RelayJournalError::Transaction)?;
        if replacement.broadcast_started {
            return Ok(None);
        }
        replacement.broadcast_started = true;
        self.store
            .write(&record)
            .map_err(|_| RelayJournalError::Storage)?;
        Ok(Some(signed))
    }

    /// Loads and validates retained intent, plan and bytes without network or signing.
    pub fn status(&self, operation_ref: [u8; 32]) -> Result<RelayJournalStatus, RelayJournalError> {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        let record = self.load(&id)?;
        let transaction_hash = Self::transaction_hash(&record)?;
        Ok(RelayJournalStatus {
            operation_ref,
            transaction_hash,
            broadcast_started: record.broadcast_started,
            finalized: record.finalized,
        })
    }

    /// Checks absence without treating a missing marked record as a fresh operation.
    pub fn lookup(
        &self,
        operation_ref: [u8; 32],
    ) -> Result<Option<RelayJournalStatus>, RelayJournalError> {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        match self
            .store
            .read(&id)
            .map_err(|_| RelayJournalError::Storage)?
        {
            Some(_) => {
                let record = self.load(&id)?;
                Ok(Some(RelayJournalStatus {
                    operation_ref,
                    transaction_hash: Self::transaction_hash(&record)?,
                    broadcast_started: record.broadcast_started,
                    finalized: record.finalized,
                }))
            }
            None => {
                if self
                    .store
                    .blob_path(&id, 0)
                    .try_exists()
                    .map_err(|_| RelayJournalError::Storage)?
                {
                    Err(RelayJournalError::Storage)
                } else {
                    Ok(None)
                }
            }
        }
    }

    pub(crate) fn retained_plan(
        &self,
        operation_ref: [u8; 32],
    ) -> Result<Option<SigningPlan>, RelayJournalError> {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        self.load(&id)?
            .plan
            .map(|bytes| SigningPlan::decode(&bytes).map_err(|_| RelayJournalError::Storage))
            .transpose()
    }

    pub(crate) fn record_finalized(
        &self,
        operation_ref: [u8; 32],
        transaction_hash: [u8; 32],
        receipt: &Inclusion,
        nonce: &FinalizedNonce,
    ) -> Result<(), RelayJournalError> {
        let id = RelayId(hex::encode(operation_ref));
        let _identity = self
            .store
            .lock_identity()
            .map_err(|_| RelayJournalError::Storage)?;
        let _record = self
            .store
            .lock_record(&id)
            .map_err(|_| RelayJournalError::Storage)?;
        let mut record = self.load(&id)?;
        let signed = record
            .signed
            .iter()
            .map(|r| r.as_slice())
            .chain(record.replacements.iter().map(|r| r.raw.as_slice()))
            .map(SignedTransaction::from_raw)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| RelayJournalError::Storage)?
            .into_iter()
            .find(|tx| tx.hash() == transaction_hash)
            .ok_or(RelayJournalError::Transaction)?;
        if !receipt.success && Self::active_signed(&record)?.hash() != transaction_hash {
            return Err(RelayJournalError::Transaction);
        }
        if receipt.from != self.sponsor
            || receipt.to != Some(self.pool)
            || receipt.nonce != signed.params().nonce
            || receipt.value != 0
            || nonce.chain_id() != self.chain_id
            || nonce.sender() != self.sponsor
            || nonce.anchor().number < receipt.block.number
            || nonce.nonce() <= receipt.nonce
        {
            return Err(RelayJournalError::Transaction);
        }
        if receipt.success {
            validate_effects(self.pool, signed.input(), receipt)?;
        }
        let finalized = RelayFinalized {
            block_number: receipt.block.number,
            block_hash: receipt.block.hash,
            success: receipt.success,
        };
        if record.finalized.is_some_and(|old| old != finalized)
            || record
                .finalized_hash
                .is_some_and(|old| old != transaction_hash)
        {
            return Err(RelayJournalError::Conflict);
        }
        record.finalized = Some(finalized);
        record.finalized_hash = Some(transaction_hash);
        self.store
            .write(&record)
            .map_err(|_| RelayJournalError::Storage)
    }
}

fn validate_effects(
    pool: [u8; 20],
    calldata: &[u8],
    receipt: &Inclusion,
) -> Result<(), RelayJournalError> {
    use erebus_core::suite::keccak256;
    let signals = &calldata[4 + 8 * 32..];
    let expected_deal = vec![
        keccak256(&[b"DealTransferred(uint256,uint256,uint256)"]),
        signals[4 * 32..5 * 32].try_into().expect("validated call"),
        signals[5 * 32..6 * 32].try_into().expect("validated call"),
        signals[7 * 32..8 * 32].try_into().expect("validated call"),
    ];
    let note_topic = keccak256(&[b"NoteInserted(uint256,uint256,uint256)"]);
    let logs: Vec<_> = receipt
        .logs
        .iter()
        .filter(|log| log.address == pool)
        .collect();
    if receipt.logs_truncated
        || logs.len() != 3
        || logs
            .iter()
            .filter(|log| log.topics == expected_deal && log.data.is_empty())
            .count()
            != 1
    {
        return Err(RelayJournalError::Transaction);
    }
    for output in [8, 9] {
        if logs
            .iter()
            .filter(|log| {
                log.topics.len() == 3
                    && log.topics[0] == note_topic
                    && log.topics[2].as_slice() == &signals[output * 32..(output + 1) * 32]
                    && log.data.len() == 32
            })
            .count()
            != 1
        {
            return Err(RelayJournalError::Transaction);
        }
    }
    Ok(())
}
