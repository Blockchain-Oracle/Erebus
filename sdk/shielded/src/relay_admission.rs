//! Verification of externally supplied shielded transfer proofs before sponsorship.
//!
//! This boundary never loads a witness, signs or allocates a nonce. Paired admission
//! authenticates deployment code and simulates the call without changing chain state.
//! Funding checks and durable submission/delegation fences remain caller-owned.

use std::io::Cursor;

use ark_bn254::{Bn254, Fq, Fq2, Fr, G1Affine, G2Affine};
use ark_crypto_primitives::snark::SNARK;
use ark_ff::{BigInteger, PrimeField};
use ark_groth16::{Groth16, PreparedVerifyingKey, Proof};
use erebus_core::settlement::{PreparedSettlement, SettlementContext};
use sha2::{Digest, Sha256};

use crate::preparation::validate_prepared;
use crate::{
    index_store::IndexDomain,
    rpc::{PoolFinalizedBlock, PoolRpc, PoolRuntimePins, RpcError},
};

const TRANSFER_CALL_BYTES: usize = 4 + 19 * 32;

/// A supplied proof or pinned verification key failed validation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RelayProofError {
    /// The key bytes do not match the independently authenticated artifact pin.
    #[error("shielded relay verification key is not authenticated")]
    Artifact,
    /// The authenticated key is unreadable or is not an eleven-input transfer key.
    #[error("shielded relay transfer verification key is invalid")]
    Key,
    /// The configured domain, prepared identity or calldata shape differs.
    #[error("shielded relay transfer context does not match")]
    Context,
    /// A field is not canonical or a proof point is invalid.
    #[error("shielded relay transfer proof encoding is invalid")]
    Encoding,
    /// The supplied timestamp has reached the public payment expiry.
    #[error("shielded relay transfer has expired")]
    Expired,
    /// The pairing equation does not verify against the pinned key and inputs.
    #[error("shielded relay transfer proof does not verify")]
    Proof,
}

/// An authenticated transfer verification key, with no witness or spending material.
pub struct TransferVerifier {
    key: PreparedVerifyingKey<Bn254>,
}

/// Operator-owned sponsorship policy, never inferred from the submitted proof.
#[derive(Clone)]
pub struct RelayAdmissionPolicy {
    /// Accepted suite, guarantees, deployment and asset.
    pub context: SettlementContext,
    /// Authenticated deployment creation block.
    pub deployment: IndexDomain,
    /// Independently pinned pool and dependency runtimes.
    pub runtime_pins: PoolRuntimePins,
    /// Address that would pay gas; this does not bind the proof to that address.
    pub sponsor: [u8; 20],
    /// Gas allowance used by both simulations.
    pub gas_limit: u64,
}

/// Read-only admission observations, not authorization to submit or evidence of payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelayAdmission {
    /// Shared explicit RPC-finalized anchor.
    pub finalized: PoolFinalizedBlock,
    /// Shared canonical head used for both simulations.
    pub head_number: u64,
    /// Hash of that simulated head.
    pub head_hash: [u8; 32],
    /// Digest of the exact simulated calldata.
    pub calldata_sha256: [u8; 32],
}

/// Proof, configuration or paired chain-state admission failed.
#[derive(Debug, thiserror::Error)]
pub enum RelayAdmissionError {
    /// The operator policy differs from the RPC or prepared deployment.
    #[error("invalid shielded relay admission policy")]
    Policy,
    /// External proof verification failed.
    #[error(transparent)]
    Proof(#[from] RelayProofError),
    /// Deployment, simulation or anchor evidence could not be accepted.
    #[error(transparent)]
    Observation(#[from] RpcError),
}

impl TransferVerifier {
    /// Authenticates both providers and simulates an external proof before sponsorship.
    ///
    /// The pinned pool's simulation checks roots, nullifiers, capacity and output reuse.
    /// No unspent or absence claim survives this snapshot: admission must be refreshed
    /// before signing, and a durable delegation fence must prevent buyer fallback.
    pub async fn admit(
        &self,
        policy: &RelayAdmissionPolicy,
        prepared: &PreparedSettlement,
        primary: &PoolRpc,
        peer: &PoolRpc,
    ) -> Result<RelayAdmission, RelayAdmissionError> {
        let deployment = &policy.deployment;
        if deployment.chain_id == 0
            || deployment.pool == [0; 20]
            || deployment.first_hash == [0; 32]
            || policy.sponsor == [0; 20]
            || policy.gas_limit == 0
            || [
                policy.runtime_pins.pool,
                policy.runtime_pins.poseidon,
                policy.runtime_pins.deposit,
                policy.runtime_pins.transfer,
                policy.runtime_pins.withdraw,
            ]
            .contains(&[0; 32])
            || policy
                .context
                .domain
                .settlement_contract
                .as_ref()
                .map(|address| address.as_bytes())
                != Some(deployment.pool.as_slice())
            || primary.shares_endpoint(peer)
            || [primary, peer]
                .iter()
                .any(|rpc| rpc.chain_id() != deployment.chain_id || rpc.pool() != deployment.pool)
        {
            return Err(RelayAdmissionError::Policy);
        }
        // Reject malformed or invalid proofs before spending the operator's RPC budget.
        self.verify(&policy.context, prepared, 0)?;
        let data = &prepared.backend_evidence;
        let signals = &data[4 + 8 * 32..];
        let chain = u64::from_be_bytes(signals[24..32].try_into().expect("validated calldata"));
        if chain != deployment.chain_id || signals[32 + 12..64] != deployment.pool {
            return Err(RelayAdmissionError::Policy);
        }
        let snapshot = primary.shared_snapshot(peer).await?;
        if deployment.first_block > snapshot.finalized.number {
            return Err(RelayAdmissionError::Policy);
        }
        self.verify(&policy.context, prepared, snapshot.head_timestamp)?;
        for rpc in [primary, peer] {
            if rpc.canonical_block_hash(deployment.first_block).await? != deployment.first_hash {
                return Err(RpcError::Evidence("relay deployment creation anchor differs").into());
            }
            for hash in [deployment.first_hash, snapshot.head_hash] {
                rpc.authenticate_runtime_at(hash, policy.runtime_pins)
                    .await?;
            }
            let identity = rpc.pool_identity_at(snapshot.head_hash).await?;
            if identity.asset != signals[3 * 32 + 12..4 * 32]
                || identity.verifier_version != policy.context.domain.verifier_version
            {
                return Err(RpcError::Evidence("relay pool identity differs").into());
            }
            rpc.simulate_transfer_at(data, policy.sponsor, policy.gas_limit, snapshot.head_hash)
                .await?;
        }
        for rpc in [primary, peer] {
            for (number, hash) in [
                (deployment.first_block, deployment.first_hash),
                (snapshot.finalized.number, snapshot.finalized.hash),
                (snapshot.head_number, snapshot.head_hash),
            ] {
                if rpc.canonical_block_hash(number).await? != hash {
                    return Err(RpcError::Evidence("relay admission anchor changed").into());
                }
            }
            if rpc.finalized_head().await?.number < snapshot.finalized.number {
                return Err(RpcError::Evidence("relay admission finality regressed").into());
            }
            rpc.check_chain().await?;
        }
        Ok(RelayAdmission {
            finalized: snapshot.finalized,
            head_number: snapshot.head_number,
            head_hash: snapshot.head_hash,
            calldata_sha256: Sha256::digest(data).into(),
        })
    }

    /// Loads the verification part of an operator's hash-pinned snarkjs zkey.
    ///
    /// The expected digest must come from an authenticated manifest, never the request.
    /// The exact same bytes are hashed and parsed; a path replacement cannot change them.
    /// This does not establish setup trust or authenticate the deployed verifier runtime.
    pub fn from_zkey_bytes(bytes: &[u8], expected_sha256: &str) -> Result<Self, RelayProofError> {
        if bytes.is_empty()
            || bytes.len() as u64 > crate::artifacts::MAX_ARTIFACT_BYTES
            || expected_sha256.len() != 64
            || !expected_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || format!("{:x}", Sha256::digest(bytes)) != expected_sha256
        {
            return Err(RelayProofError::Artifact);
        }
        let (key, _) =
            ark_circom::read_zkey(&mut Cursor::new(bytes)).map_err(|_| RelayProofError::Key)?;
        if key.vk.gamma_abc_g1.len() != 12 {
            return Err(RelayProofError::Key);
        }
        let key = Groth16::<Bn254>::process_vk(&key.vk).map_err(|_| RelayProofError::Key)?;
        Ok(Self { key })
    }

    /// Checks an external proof against its configured deployment and prepared identity.
    ///
    /// `timestamp` is supplied by the caller's paired chain observation. A successful
    /// result does not assert a known root, unused nullifiers, funding or finality;
    /// those checks and their anchors must be completed before sponsored submission.
    pub fn verify(
        &self,
        context: &SettlementContext,
        prepared: &PreparedSettlement,
        timestamp: u64,
    ) -> Result<(), RelayProofError> {
        validate_prepared(context, prepared).map_err(|_| RelayProofError::Context)?;
        let (proof, public) = decode(&prepared.backend_evidence)?;
        let expiry = &prepared.backend_evidence[4 + 18 * 32..];
        if expiry[..24] != [0; 24] {
            return Err(RelayProofError::Encoding);
        }
        let expiry = u64::from_be_bytes(expiry[24..].try_into().expect("fixed calldata width"));
        if timestamp >= expiry {
            return Err(RelayProofError::Expired);
        }
        match Groth16::<Bn254>::verify_with_processed_vk(&self.key, &public, &proof) {
            Ok(true) => Ok(()),
            _ => Err(RelayProofError::Proof),
        }
    }
}

fn canonical_field<F: PrimeField>(word: &[u8]) -> Result<F, RelayProofError> {
    if word.len() != 32 {
        return Err(RelayProofError::Encoding);
    }
    let field = F::from_be_bytes_mod_order(word);
    let bytes = field.into_bigint().to_bytes_be();
    let mut encoded = [0; 32];
    encoded[32 - bytes.len()..].copy_from_slice(&bytes);
    if encoded != word {
        return Err(RelayProofError::Encoding);
    }
    Ok(field)
}

fn g1(x: Fq, y: Fq) -> Result<G1Affine, RelayProofError> {
    let point = G1Affine::new_unchecked(x, y);
    if !point.is_on_curve() || !point.is_in_correct_subgroup_assuming_on_curve() {
        return Err(RelayProofError::Encoding);
    }
    Ok(point)
}

fn decode(data: &[u8]) -> Result<(Proof<Bn254>, [Fr; 11]), RelayProofError> {
    if data.len() != TRANSFER_CALL_BYTES {
        return Err(RelayProofError::Encoding);
    }
    let words: Vec<&[u8]> = data[4..].chunks_exact(32).collect();
    let coordinates = words[..8]
        .iter()
        .map(|word| canonical_field::<Fq>(word))
        .collect::<Result<Vec<_>, _>>()?;
    // Solidity's pairing input reverses the two Fq2 components of each coordinate.
    let b = G2Affine::new_unchecked(
        Fq2::new(coordinates[3], coordinates[2]),
        Fq2::new(coordinates[5], coordinates[4]),
    );
    if !b.is_on_curve() || !b.is_in_correct_subgroup_assuming_on_curve() {
        return Err(RelayProofError::Encoding);
    }
    let proof = Proof {
        a: g1(coordinates[0], coordinates[1])?,
        b,
        c: g1(coordinates[6], coordinates[7])?,
    };
    let mut public = [Fr::from(0u64); 11];
    for (field, word) in public.iter_mut().zip(&words[8..]) {
        *field = canonical_field(word)?;
    }
    Ok((proof, public))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calldata_fields_never_reduce_noncanonical_words() {
        assert!(canonical_field::<Fr>(&[0; 32]).is_ok());
        for modulus in [Fr::MODULUS.to_bytes_be(), Fq::MODULUS.to_bytes_be()] {
            assert_eq!(
                canonical_field::<Fr>(&modulus),
                Err(RelayProofError::Encoding)
            );
        }
        assert_eq!(
            canonical_field::<Fq>(&Fq::MODULUS.to_bytes_be()),
            Err(RelayProofError::Encoding)
        );
        assert_eq!(
            canonical_field::<Fr>(&[0; 31]),
            Err(RelayProofError::Encoding)
        );
    }

    #[test]
    fn malformed_proof_points_and_shapes_are_rejected() {
        assert!(g1(Fq::from(1u64), Fq::from(2u64)).is_ok());
        assert_eq!(
            g1(Fq::from(1u64), Fq::from(1u64)),
            Err(RelayProofError::Encoding)
        );
        // The EVM precompile represents infinity as (0, 0); the pairing check still applies.
        assert!(g1(Fq::from(0u64), Fq::from(0u64)).is_ok());
        for size in [0, TRANSFER_CALL_BYTES - 1, TRANSFER_CALL_BYTES + 1] {
            assert!(decode(&vec![0; size]).is_err());
        }
    }

    #[test]
    fn unauthenticated_key_bytes_are_refused_before_parsing() {
        assert!(matches!(
            TransferVerifier::from_zkey_bytes(b"not a key", &"0".repeat(64)),
            Err(RelayProofError::Artifact)
        ));
        assert!(matches!(
            TransferVerifier::from_zkey_bytes(&[], &format!("{:x}", Sha256::digest([]))),
            Err(RelayProofError::Artifact)
        ));
    }

    #[test]
    fn on_curve_g2_points_outside_the_subgroup_are_rejected() {
        let point = (0..100u64)
            .filter_map(|x| {
                G2Affine::get_point_from_x_unchecked(Fq2::new(Fq::from(x), Fq::from(1u64)), false)
            })
            .find(|point| !point.is_in_correct_subgroup_assuming_on_curve())
            .expect("deterministic point outside the prime-order subgroup");
        assert!(point.is_on_curve());
        let mut data = vec![0; TRANSFER_CALL_BYTES];
        for (index, value) in [
            Fq::from(1u64),
            Fq::from(2u64),
            point.x.c1,
            point.x.c0,
            point.y.c1,
            point.y.c0,
            Fq::from(1u64),
            Fq::from(2u64),
        ]
        .into_iter()
        .enumerate()
        {
            let bytes = value.into_bigint().to_bytes_be();
            let end = 4 + (index + 1) * 32;
            data[end - bytes.len()..end].copy_from_slice(&bytes);
        }
        assert!(matches!(decode(&data), Err(RelayProofError::Encoding)));
    }
}
