//! Read-only public-history diagnostic. Does not open a wallet or sign a transaction.

use std::{env, error::Error, path::PathBuf};

use erebus_shielded_prover::{
    index_store::{IndexDomain, IndexStore},
    recovery::sync_public_index_step,
    rpc::PoolRpc,
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 3 {
        return Err("usage: diagnose_pool_history wallet-config new-public-cache-directory".into());
    }
    let config: Value = serde_json::from_slice(&std::fs::read(&args[1])?)?;
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
    for (role, field) in [("primary", "rpc_url"), ("peer", "peer_rpc_url")] {
        let rpc = PoolRpc::new(
            config[field].as_str().ok_or("missing RPC")?,
            domain.chain_id,
            domain.pool,
        )?;
        let store = IndexStore::new(directory.join(format!("{role}.json")), domain)?;
        let result = match rpc.finalized_head().await {
            Ok(anchor) => sync_public_index_step(&rpc, &store, anchor.number, 1_000).await,
            Err(error) => Err(error.into()),
        };
        // Errors contain static method/evidence labels, not endpoint credentials or wallet data.
        match result {
            Ok((_, report)) => println!(
                "{}",
                json!({"role":role,"through":report.through,"complete":true})
            ),
            Err(error) => println!(
                "{}",
                json!({"role":role,"error":error.to_string(),"complete":false})
            ),
        }
    }
    Ok(())
}
