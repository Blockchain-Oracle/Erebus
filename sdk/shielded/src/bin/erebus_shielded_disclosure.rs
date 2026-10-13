//! Local disclosure command with independent shielded and public-bound payment verification.

use std::path::PathBuf;

use erebus_core::{ids::ChainNamespace, terms::SettlementMode};
use erebus_evm::{
    deployment::{parse_lowercase_address, EvmDeployment},
    disclosure::cli::{self, PaymentRequest, PaymentVerification},
};
use erebus_shielded_prover::{
    disclosure::{verify_shielded_payment_certified, ShieldedDisclosureError},
    index_store::{IndexDomain, IndexStore},
    observation::ObservationError,
    recovery::RecoveryError,
    rpc::{PoolRpc, PoolRuntimePins},
};
use erebus_transport::disclosure::verify_selected_agreement;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Deployment {
    namespace: String,
    settlement_contract: String,
    verifier_version: u32,
    rpc_url: String,
    peer_rpc_url: String,
    first_block: u64,
    first_hash: String,
    cache_root: PathBuf,
    runtime_keccak256: String,
    poseidon_keccak256: String,
    deposit_verifier_keccak256: String,
    transfer_verifier_keccak256: String,
    withdraw_verifier_keccak256: String,
}

fn digest(value: &str) -> Result<[u8; 32], &'static str> {
    let digits = value.strip_prefix("0x").ok_or("invalid runtime pin")?;
    if digits.len() != 64
        || !digits
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid runtime pin");
    }
    let bytes: [u8; 32] = hex::decode(digits)
        .map_err(|_| "invalid runtime pin")?
        .try_into()
        .map_err(|_| "invalid runtime pin")?;
    if bytes == [0; 32] {
        return Err("invalid runtime pin");
    }
    Ok(bytes)
}

fn pending_progress(error: &ShieldedDisclosureError) -> Option<(Option<u64>, u64)> {
    match error {
        ShieldedDisclosureError::Observation(ObservationError::Recovery(
            RecoveryError::HistoryPending { next_block, .. },
        )) => Some((Some(*next_block), next_block.saturating_sub(1))),
        ShieldedDisclosureError::Observation(ObservationError::FinalityPending)
        | ShieldedDisclosureError::Payment => Some((None, 0)),
        _ => None,
    }
}

#[tokio::main]
async fn main() {
    cli::run(|request| async move {
        if cli::is_x402_request(&request.deployment) {
            return cli::verify_x402_payment(request).await;
        }
        if request.evidence.terms.settlement_mode == SettlementMode::PublicBound {
            return cli::verify_public_payment(request).await;
        }
        verify(request).await
    })
    .await;
}

async fn verify(request: PaymentRequest) -> Result<PaymentVerification, &'static str> {
    let deployment: Deployment =
        serde_json::from_value(request.deployment).map_err(|_| "invalid shielded deployment")?;
    let pins = PoolRuntimePins {
        pool: digest(&deployment.runtime_keccak256)?,
        poseidon: digest(&deployment.poseidon_keccak256)?,
        deposit: digest(&deployment.deposit_verifier_keccak256)?,
        transfer: digest(&deployment.transfer_verifier_keccak256)?,
        withdraw: digest(&deployment.withdraw_verifier_keccak256)?,
    };
    let namespace =
        ChainNamespace::parse(&deployment.namespace).map_err(|_| "invalid deployment")?;
    let pool =
        parse_lowercase_address(&deployment.settlement_contract).ok_or("invalid deployment")?;
    let configured = EvmDeployment::new(
        namespace,
        pool,
        deployment.verifier_version,
        &deployment.rpc_url,
    )
    .map_err(|_| "invalid deployment")?;
    configured
        .matches_domain(&request.evidence.terms.domain)
        .map_err(|_| "deployment does not match agreement")?;
    let digits = deployment
        .first_hash
        .strip_prefix("0x")
        .ok_or("invalid deployment anchor")?;
    if digits.len() != 64
        || !digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("invalid deployment anchor");
    }
    let first_hash = hex::decode(digits)
        .map_err(|_| "invalid deployment anchor")?
        .try_into()
        .map_err(|_| "invalid deployment anchor")?;
    let domain = IndexDomain {
        chain_id: configured.chain_id,
        pool,
        first_block: deployment.first_block,
        first_hash,
    };
    let rpc = PoolRpc::new(&deployment.rpc_url, configured.chain_id, pool)
        .map_err(|_| "invalid pool RPC configuration")?;
    let peer_rpc = PoolRpc::new(&deployment.peer_rpc_url, configured.chain_id, pool)
        .map_err(|_| "invalid peer pool RPC configuration")?;
    let index = IndexStore::new(deployment.cache_root.join("primary/index.json"), domain)
        .map_err(|_| "public cache unavailable")?;
    let peer_index = IndexStore::new(deployment.cache_root.join("peer/index.json"), domain)
        .map_err(|_| "peer public cache unavailable")?;
    let agreement = verify_selected_agreement(&request.evidence)
        .map_err(|_| "disclosure agreement not verified")?;
    match verify_shielded_payment_certified(
        request.evidence,
        &rpc,
        &index,
        &peer_rpc,
        &peer_index,
        pins,
    )
    .await
    {
        Ok(result) => Ok(PaymentVerification::Finalized(result.payment.agreement)),
        Err(error) => {
            if let Some((next_log_block, ancestry_block)) = pending_progress(&error) {
                Ok(PaymentVerification::Pending {
                    agreement,
                    next_log_block,
                    ancestry_block,
                })
            } else {
                Err("payment not independently verified; retain caches and retry")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_history_is_pending_but_conflicting_evidence_is_not() {
        let pending = ShieldedDisclosureError::Observation(ObservationError::Recovery(
            RecoveryError::HistoryPending {
                next_block: 101,
                through: 200,
            },
        ));
        assert_eq!(pending_progress(&pending), Some((Some(101), 100)));
        assert_eq!(
            pending_progress(&ShieldedDisclosureError::Observation(
                ObservationError::Inconsistent
            )),
            None
        );
        assert_eq!(
            pending_progress(&ShieldedDisclosureError::Observation(
                ObservationError::FinalityPending
            )),
            Some((None, 0))
        );
    }

    #[test]
    fn runtime_pins_are_nonzero_canonical_digests() {
        assert_eq!(digest(&format!("0x{}", "01".repeat(32))), Ok([1; 32]));
        for value in [
            "01".repeat(32),
            format!("0x{}", "00".repeat(32)),
            format!("0x{}", "AB".repeat(32)),
        ] {
            assert!(digest(&value).is_err());
        }
    }
}
