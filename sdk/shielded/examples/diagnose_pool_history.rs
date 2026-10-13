//! Read-only public-history diagnostic. Does not open a wallet or sign a transaction.

use std::{env, error::Error, path::PathBuf};

use erebus_shielded_prover::{
    index_store::{IndexDomain, IndexStore},
    recovery::sync_public_index_step,
    rpc::{PoolRpc, DEFAULT_MAX_REQUESTS_PER_SECOND},
};
use serde_json::{json, Value};

fn fixed<const N: usize>(value: &Value) -> Result<[u8; N], Box<dyn Error>> {
    Ok(hex::decode(
        value
            .as_str()
            .ok_or("missing public hex field")?
            .trim_start_matches("0x"),
    )?
    .try_into()
    .map_err(|_| "wrong public hex width")?)
}

fn bounded(config: &Value, field: &str, default: u64, maximum: u64) -> Result<u64, Box<dyn Error>> {
    let value = match config.get(field) {
        None | Some(Value::Null) => default,
        Some(value) => value.as_u64().ok_or("invalid diagnostic read budget")?,
    };
    if value == 0 || value > maximum {
        return Err("invalid diagnostic read budget".into());
    }
    Ok(value)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 {
        return Err("usage: diagnose_pool_history wallet-config new-public-cache-directory".into());
    }
    let config: Value = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let limit = bounded(&config, "max_scan_blocks", 1_000, 1_000)?;
    let primary_rate = bounded(
        &config,
        "rpc_requests_per_second",
        DEFAULT_MAX_REQUESTS_PER_SECOND as u64,
        40,
    )? as u32;
    let peer_rate = bounded(
        &config,
        "peer_rpc_requests_per_second",
        DEFAULT_MAX_REQUESTS_PER_SECOND as u64,
        40,
    )? as u32;
    let domain = IndexDomain {
        chain_id: config["namespace"]
            .as_str()
            .ok_or("missing namespace")?
            .strip_prefix("eip155:")
            .ok_or("not EVM")?
            .parse()?,
        pool: fixed(&config["pool"])?,
        first_block: config["first_block"]
            .as_u64()
            .ok_or("missing first block")?,
        first_hash: fixed(&config["first_hash"])?,
    };
    let directory = PathBuf::from(&args[2]);
    std::fs::create_dir(&directory)?;
    for (role, field, rate) in [
        ("primary", "rpc_url", primary_rate),
        ("peer", "peer_rpc_url", peer_rate),
    ] {
        let rpc = PoolRpc::new(
            config[field].as_str().ok_or("missing RPC")?,
            domain.chain_id,
            domain.pool,
        )?
        .with_max_requests_per_second(rate)?;
        let store = IndexStore::new(directory.join(format!("{role}.json")), domain)?;
        let result = match rpc.finalized_head().await {
            Ok(anchor) => sync_public_index_step(&rpc, &store, anchor.number, limit).await,
            Err(error) => Err(error.into()),
        };
        // Errors contain static method/evidence labels, not endpoint credentials or wallet data.
        match result {
            Ok((_, report)) => println!(
                "{}",
                json!({"role":role,"through":report.through,"complete":true,"requests_per_second":rate})
            ),
            Err(error) => println!(
                "{}",
                json!({"role":role,"error":error.to_string(),"complete":false,"requests_per_second":rate,
                    "checkpoint":store.load()?.tip().map(|tip|tip.number)})
            ),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_default_independently_and_refuse_invalid_values() {
        assert_eq!(
            bounded(
                &json!({}),
                "rate",
                DEFAULT_MAX_REQUESTS_PER_SECOND as u64,
                40
            )
            .unwrap(),
            40
        );
        assert_eq!(bounded(&json!({"rate":2}), "rate", 40, 40).unwrap(), 2);
        assert_eq!(bounded(&json!({"rate":null}), "rate", 40, 40).unwrap(), 40);
        for value in [
            json!(0),
            json!(41),
            json!(-1),
            json!(true),
            json!("SECRET"),
            json!(1.5),
        ] {
            let error = bounded(&json!({"rate":value}), "rate", 40, 40).unwrap_err();
            assert_eq!(error.to_string(), "invalid diagnostic read budget");
        }
        assert!(bounded(
            &json!({"max_scan_blocks":1001}),
            "max_scan_blocks",
            1000,
            1000
        )
        .is_err());
    }
}
