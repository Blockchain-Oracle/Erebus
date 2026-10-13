//! Bounded public-proof transport. Decoding is not proof verification or admission.

use erebus_core::{
    commitment::{DealCommitment, DealNullifier},
    settlement::{PreparedSettlement, SettlementContext},
    suite::keccak256,
};
use serde::{Deserialize, Serialize};

use crate::preparation::validate_prepared;

const CALL_BYTES: usize = 4 + 19 * 32;
/// Enforced before JSON parsing; operators must also bound HTTP bodies before buffering.
pub const MAX_RELAY_REQUEST_BYTES: usize = 2048;

/// A noncanonical, oversized or foreign-domain request.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid shielded relay request")]
pub struct RelayRequestError;

/// No client operation identifier, spending material or agreement opening is accepted.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelayRequest {
    version: u16,
    calldata: String,
}

impl RelayRequest {
    /// Constructs the outbound public message from locally retained prepared evidence.
    pub fn from_prepared(
        context: &SettlementContext,
        prepared: &PreparedSettlement,
    ) -> Result<Self, RelayRequestError> {
        validate_prepared(context, prepared).map_err(|_| RelayRequestError)?;
        Ok(Self {
            version: 1,
            calldata: format!("0x{}", hex::encode(&prepared.backend_evidence)),
        })
    }

    /// Parses the fixed transport schema. Always call `prepared` and proof admission next.
    pub fn decode(bytes: &[u8]) -> Result<Self, RelayRequestError> {
        if bytes.len() > MAX_RELAY_REQUEST_BYTES {
            return Err(RelayRequestError);
        }
        let request: Self = serde_json::from_slice(bytes).map_err(|_| RelayRequestError)?;
        request.call_bytes()?;
        Ok(request)
    }

    /// Encodes only the two fixed public transport fields.
    pub fn encode(&self) -> Result<Vec<u8>, RelayRequestError> {
        self.call_bytes()?;
        serde_json::to_vec(self).map_err(|_| RelayRequestError)
    }

    fn call_bytes(&self) -> Result<Vec<u8>, RelayRequestError> {
        if self.version != 1
            || self.calldata.len() != 2 + 2 * CALL_BYTES
            || !self.calldata.starts_with("0x")
            || !self.calldata[2..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(RelayRequestError);
        }
        hex::decode(&self.calldata[2..]).map_err(|_| RelayRequestError)
    }

    /// Derives metadata against operator configuration, never against client policy.
    ///
    /// Identity excludes proof bytes: another proof for this nullifier has the same ID.
    /// A durable service must reject changed calldata under an existing ID, not replace it.
    /// This validates shape/binding only; Groth16 verification and paired admission follow.
    pub fn prepared(
        &self,
        context: &SettlementContext,
    ) -> Result<PreparedSettlement, RelayRequestError> {
        let data = self.call_bytes()?;
        let signals = &data[4 + 8 * 32..];
        let commitment: [u8; 32] = signals[4 * 32..5 * 32]
            .try_into()
            .expect("fixed call shape");
        let nullifier: [u8; 32] = signals[5 * 32..6 * 32]
            .try_into()
            .expect("fixed call shape");
        let operation_ref = keccak256(&[b"erebus-shielded-relay-v1", &signals[..64], &nullifier]);
        let prepared = PreparedSettlement {
            operation_ref,
            deal_commitment: DealCommitment::from_bytes(commitment),
            deal_nullifier: DealNullifier::from_bytes(nullifier),
            domain: context.domain.clone(),
            mode: context.mode,
            required_guarantees: context.required_guarantees,
            backend_evidence: data,
        };
        validate_prepared(context, &prepared).map_err(|_| RelayRequestError)?;
        Ok(prepared)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use erebus_core::{
        domain::DeploymentDomain,
        ids::{AddressBytes, AssetId, ChainNamespace},
        shielded::SHIELDED_GUARANTEES,
        terms::{GuaranteeSet, SettlementMode},
    };

    // Shape fixture only: its zero proof must still be rejected by proof admission.
    pub(crate) fn fixture() -> (SettlementContext, PreparedSettlement) {
        let pool = AddressBytes::new(vec![1; 20]).unwrap();
        let domain = DeploymentDomain {
            namespace: ChainNamespace::parse("eip155:31337").unwrap(),
            settlement_contract: Some(pool.clone()),
            pool: Some(pool),
            verifier_version: 2,
        };
        let guarantees = GuaranteeSet::from_bits(SHIELDED_GUARANTEES).unwrap();
        let context = SettlementContext {
            domain: domain.clone(),
            suite_id: 2,
            mode: SettlementMode::Shielded,
            asset: AssetId::parse("eip155:31337/erc20:0x0202020202020202020202020202020202020202")
                .unwrap(),
            required_guarantees: guarantees,
            require_local_proving: true,
        };
        let mut data = vec![0; CALL_BYTES];
        data[..4].copy_from_slice(
            &keccak256(&[b"transferPrivate(uint256[2],uint256[2][2],uint256[2],uint256[11])"])[..4],
        );
        let signals = &mut data[4 + 8 * 32..];
        signals[24..32].copy_from_slice(&31337u64.to_be_bytes());
        signals[44..64].fill(1);
        signals[92..96].copy_from_slice(&2u32.to_be_bytes());
        signals[108..128].fill(2);
        signals[128..160].fill(3);
        signals[160..192].fill(4);
        let prepared = PreparedSettlement {
            operation_ref: [9; 32],
            deal_commitment: DealCommitment::from_bytes([3; 32]),
            deal_nullifier: DealNullifier::from_bytes([4; 32]),
            domain,
            mode: SettlementMode::Shielded,
            required_guarantees: guarantees,
            backend_evidence: data,
        };
        (context, prepared)
    }

    #[test]
    fn relay_transport_derives_identity_and_contains_only_public_calldata() {
        let (context, prepared) = fixture();
        let request = RelayRequest::from_prepared(&context, &prepared).unwrap();
        let bytes = request.encode().unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json.as_object().unwrap().len(), 2);
        let decoded = RelayRequest::decode(&bytes)
            .unwrap()
            .prepared(&context)
            .unwrap();
        assert_eq!(decoded.backend_evidence, prepared.backend_evidence);
        assert_ne!(decoded.operation_ref, prepared.operation_ref);
        let mut reproved = prepared.clone();
        reproved.backend_evidence[4] = 1;
        let other = RelayRequest::from_prepared(&context, &reproved)
            .unwrap()
            .prepared(&context)
            .unwrap();
        assert_eq!(decoded.operation_ref, other.operation_ref);
        assert_ne!(decoded.backend_evidence, other.backend_evidence);
        let mut foreign = context.clone();
        foreign.domain.verifier_version = 3;
        assert!(request.prepared(&foreign).is_err());
    }

    #[test]
    fn relay_transport_rejects_private_fields_duplicates_and_noncanonical_encoding() {
        let (context, prepared) = fixture();
        let bytes = RelayRequest::from_prepared(&context, &prepared)
            .unwrap()
            .encode()
            .unwrap();
        for field in [
            "witness",
            "spending_key",
            "agreement",
            "operation_ref",
            "fee",
            "gas_payer",
        ] {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            value[field] = serde_json::json!("forbidden");
            assert!(RelayRequest::decode(&serde_json::to_vec(&value).unwrap()).is_err());
        }
        assert!(RelayRequest::decode(&vec![b' '; MAX_RELAY_REQUEST_BYTES + 1]).is_err());
        assert!(RelayRequest::decode(br#"{"version":1,"version":1,"calldata":"0x"}"#).is_err());
        for calldata in [
            "0x00".to_string(),
            format!("0x{}", "AA".repeat(CALL_BYTES)),
            "00".repeat(CALL_BYTES),
        ] {
            let invalid = RelayRequest {
                version: 1,
                calldata,
            };
            assert!(invalid.encode().is_err());
        }
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value["version"] = serde_json::json!(2);
        assert!(RelayRequest::decode(&serde_json::to_vec(&value).unwrap()).is_err());
    }
}
