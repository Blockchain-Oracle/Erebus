//! Canonical local signing parameters. The agreement supplies chain, target, and calldata.

use super::{
    build_signed, build_signed_call, Eip1559Fees, SignedTransaction, TransactionKey,
    TransactionParams,
};
use crate::{deployment::EvmDeployment, error::EvmError};
use erebus_core::{
    encoding::{Reader, Writer},
    settlement::PreparedSettlement,
};

/// Immutable local transaction intent persisted before signing.
/// This does not allocate a nonce or establish ownership of a nonce reservation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SigningPlan {
    sender: [u8; 20],
    params: TransactionParams,
}

impl SigningPlan {
    /// The gas-paying sender bound by this plan.
    #[must_use]
    pub const fn sender(&self) -> [u8; 20] {
        self.sender
    }

    /// The immutable nonce, fee caps, and gas limit.
    #[must_use]
    pub const fn params(&self) -> TransactionParams {
        self.params
    }

    /// Creates a plan using an explicitly selected sender and transaction parameters.
    pub fn new(sender: [u8; 20], params: TransactionParams) -> Result<Self, EvmError> {
        if params.gas_limit == 0 {
            return Err(EvmError::TransactionEncoding("gas limit is zero".into()));
        }
        Ok(Self { sender, params })
    }

    /// Encodes version 2: version, sender, nonce, fees, gas limit, and native value.
    ///
    /// Version 1 carried no value field. Version 1 plans can only have been produced before
    /// value support existed, so decoding one means zero value unambiguously.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.u8(2);
        w.fixed(&self.sender);
        w.u64(self.params.nonce);
        w.u128(self.params.fees.max_fee_per_gas());
        w.u128(self.params.fees.max_priority_fee_per_gas());
        w.u64(self.params.gas_limit);
        w.u128(self.params.value);
        w.finish()
    }

    /// Decodes canonical local metadata; unknown versions and trailing bytes fail closed.
    ///
    /// Version 1 is still readable and means zero value. It is never re-encoded as version 1.
    pub fn decode(bytes: &[u8]) -> Result<Self, EvmError> {
        let mut r = Reader::new(bytes);
        let version = r.u8("plan_version")?;
        if version != 1 && version != 2 {
            return Err(EvmError::TransactionEncoding(
                "unknown signing plan version".into(),
            ));
        }
        let sender = r.fixed("sender")?;
        let nonce = r.u64("nonce")?;
        let fees = Eip1559Fees::new(r.u128("max_fee")?, r.u128("priority_fee")?)?;
        let gas_limit = r.u64("gas_limit")?;
        let value = if version >= 2 { r.u128("value")? } else { 0 };
        r.finish()?;
        Self::new(
            sender,
            TransactionParams {
                nonce,
                fees,
                gas_limit,
                value,
            },
        )
    }

    /// Signs only with the planned gas payer. Does not perform network I/O.
    pub fn sign(
        &self,
        deployment: &EvmDeployment,
        prepared: &PreparedSettlement,
        key: &TransactionKey,
    ) -> Result<SignedTransaction, EvmError> {
        if key.address() != self.sender {
            return Err(EvmError::SignedIntentMismatch);
        }
        build_signed(deployment, prepared, key, self.params)
    }

    /// Signs a backend-validated call with the journaled sender and parameters.
    pub fn sign_call(
        &self,
        chain_id: u64,
        to: [u8; 20],
        input: Vec<u8>,
        key: &TransactionKey,
    ) -> Result<SignedTransaction, EvmError> {
        if key.address() != self.sender {
            return Err(EvmError::SignedIntentMismatch);
        }
        build_signed_call(key, chain_id, to, input, self.params)
    }

    /// Checks restored bytes against both the durable plan and the accepted agreement.
    pub fn validate(
        &self,
        deployment: &EvmDeployment,
        prepared: &PreparedSettlement,
        raw: &[u8],
    ) -> Result<(), EvmError> {
        SignedTransaction::from_raw(raw)?.validate_against(
            deployment,
            prepared,
            self.sender,
            self.params,
        )
    }

    /// Checks restored bytes against backend-validated call data and this stored plan.
    pub fn validate_call(
        &self,
        chain_id: u64,
        to: [u8; 20],
        input: &[u8],
        raw: &[u8],
    ) -> Result<(), EvmError> {
        SignedTransaction::from_raw(raw)?.validate_call(
            chain_id,
            to,
            input,
            self.sender,
            self.params,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_encoding_is_strict_and_round_trips() {
        let plan = SigningPlan::new(
            [1; 20],
            TransactionParams {
                nonce: 7,
                fees: Eip1559Fees::new(100, 10).unwrap(),
                gas_limit: 300_000,
                value: 0,
            },
        )
        .unwrap();
        let bytes = plan.encode();
        assert_eq!(bytes.len(), 85);
        assert_eq!(SigningPlan::decode(&bytes).unwrap(), plan);
        for end in 0..bytes.len() {
            assert!(SigningPlan::decode(&bytes[..end]).is_err());
        }
        let mut changed = bytes.clone();
        changed.push(0);
        assert!(SigningPlan::decode(&changed).is_err());
        let mut changed = bytes.clone();
        changed[0] = 3;
        assert!(SigningPlan::decode(&changed).is_err());
        let mut changed = bytes.clone();
        changed[61..].fill(0);
        assert!(SigningPlan::decode(&changed).is_err());
        let mut changed = bytes;
        changed[29..45].fill(0);
        assert!(SigningPlan::decode(&changed).is_err());
    }

    #[test]
    fn legacy_version_one_plans_decode_as_zero_value() {
        let mut w = Writer::new();
        w.u8(1);
        w.fixed(&[1; 20]);
        w.u64(7);
        w.u128(100);
        w.u128(10);
        w.u64(300_000);
        let legacy = w.finish();
        assert_eq!(legacy.len(), 69);
        let decoded = SigningPlan::decode(&legacy).unwrap();
        assert_eq!(decoded.params().value, 0);
        // Re-encoding a legacy plan writes the version-2 form.
        assert_eq!(decoded.encode().len(), 85);
        assert_eq!(SigningPlan::decode(&decoded.encode()).unwrap(), decoded);
    }

    #[test]
    fn nonzero_values_round_trip_and_are_bounded_by_the_reader() {
        let plan = SigningPlan::new(
            [1; 20],
            TransactionParams {
                nonce: 7,
                fees: Eip1559Fees::new(100, 10).unwrap(),
                gas_limit: 300_000,
                value: 123_456,
            },
        )
        .unwrap();
        let bytes = plan.encode();
        assert_eq!(SigningPlan::decode(&bytes).unwrap(), plan);
        for end in 0..bytes.len() {
            assert!(SigningPlan::decode(&bytes[..end]).is_err());
        }
        let mut changed = bytes;
        changed.push(0);
        assert!(SigningPlan::decode(&changed).is_err());
    }
}
