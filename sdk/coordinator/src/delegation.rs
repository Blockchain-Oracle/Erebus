//! Buyer-side durable delegation. No transport or delegate signature lives here.

use erebus_core::{suite::keccak256, terms::SettlementMode};
use serde::{Deserialize, Serialize};

use crate::{record::Accounting, Coordinator, Error};

/// Immutable delegation of one prepared shielded call to an operator-pinned relay.
///
/// This is a local submission fence, not evidence of acceptance, broadcast or payment.
/// No clearing or automatic direct-submission fallback is supported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmissionDelegation {
    /// Operator-pinned service identity digest, never supplied by the remote response.
    pub relay_identity: [u8; 32],
    /// Digest derived from the exact locally retained prepared calldata.
    pub calldata_keccak256: [u8; 32],
}

impl Coordinator {
    /// Persists the delegation before the first outbound request can be made.
    ///
    /// The caller authenticates the relay identity. This method does no network I/O.
    /// Retrying the same delegation is idempotent, including after local expiry.
    /// A changed relay conflicts; an already-started direct signing plan cannot delegate.
    /// An uncertain storage failure must be resolved by reloading, never by direct signing.
    pub fn delegate_submission(
        &self,
        operation_ref: [u8; 32],
        relay_identity: [u8; 32],
        now: u64,
    ) -> Result<SubmissionDelegation, Error> {
        if relay_identity == [0; 32] {
            return Err(Error::Context);
        }
        let _lock = self.store.lock_identity().map_err(|_| Error::Storage)?;
        let mut state = self.load()?;
        let index = state.index(operation_ref)?;
        let intent = &state.intents[index];
        let (terms, _) = intent.opening()?;
        self.check_terms(&terms)?;
        let prepared = intent.prepared.as_ref().ok_or(Error::Stage)?;
        let delegation = SubmissionDelegation {
            relay_identity,
            calldata_keccak256: keccak256(&[prepared]),
        };
        if let Some(existing) = intent.delegation {
            if existing != delegation {
                return Err(Error::Conflict);
            }
            // A previous error may have followed rename but preceded directory sync.
            self.store.sync().map_err(|_| Error::Storage)?;
            return Ok(existing);
        }
        if terms.settlement_mode != SettlementMode::Shielded {
            return Err(Error::Context);
        }
        if intent.accounting != Accounting::Reserved
            || intent.transaction_plan.is_some()
            || intent.signed_transaction.is_some()
            || !intent.broadcast_attempts.is_empty()
        {
            return Err(Error::Stage);
        }
        if now >= terms.expiry {
            return Err(Error::Expired);
        }
        state.intents[index].delegation = Some(delegation);
        self.save(&state)?;
        Ok(delegation)
    }

    /// Reads the durable fence without creating a network request or releasing capacity.
    pub fn submission_delegation(
        &self,
        operation_ref: [u8; 32],
    ) -> Result<Option<SubmissionDelegation>, Error> {
        let _lock = self.store.lock_identity().map_err(|_| Error::Storage)?;
        let state = self.load()?;
        Ok(state.intents[state.index(operation_ref)?].delegation)
    }
}
