//! Loopback-only, authenticated operator-sponsored shielded relay.

use std::{
    io::Read,
    net::{Ipv4Addr, SocketAddrV4},
    path::{Path, PathBuf},
    time::Duration,
};

use erebus_core::{
    domain::DeploymentDomain,
    ids::{AddressBytes, AssetId, ChainNamespace},
    settlement::SettlementContext,
    shielded::SHIELDED_GUARANTEES,
    suite::SHIELDED_POSEIDON_EDDSA_SUITE_ID,
    terms::{GuaranteeSet, SettlementMode},
};
use erebus_evm::chain::Eip1559Fees;
use erebus_shielded_prover::{
    index_store::IndexDomain,
    operations::{parse_address, parse_digest32},
    relay_admission::{RelayAdmissionPolicy, TransferVerifier},
    relay_http::{RelayHttpPolicy, RelayHttpService},
    relay_service::{SponsorConfig, SponsoredRelay},
    rpc::PoolRuntimePins,
};
use serde::Deserialize;
use serde_json::json;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u16,
    chain_id: u64,
    pool: String,
    asset: String,
    verifier_version: u32,
    first_block: u64,
    first_hash: String,
    runtime_pins: Pins,
    transfer_zkey_file: PathBuf,
    transfer_zkey_sha256: String,
    transaction_key_file: PathBuf,
    authentication_key_file: PathBuf,
    sponsor: String,
    state_root: PathBuf,
    signer_journal_root: PathBuf,
    rpc_url: String,
    peer_rpc_url: String,
    port: u16,
    gas_limit: u64,
    max_fee_per_gas: String,
    max_priority_fee_per_gas: String,
    #[serde(default)]
    replacement_fees: Option<[String; 2]>,
    maximum_gas_budget: String,
    maximum_operations: u32,
    requests_per_window: u32,
    window_seconds: u64,
    timeout_seconds: u64,
    /// Required acknowledgement for the current single-operator testnet setup.
    allow_test_artifacts: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pins {
    pool: String,
    poseidon: String,
    deposit: String,
    transfer: String,
    withdraw: String,
}

fn input(path: &Path, maximum: u64, private: bool) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if !path.is_absolute() {
        return Err("absolute local inputs required");
    }
    let meta = std::fs::symlink_metadata(path).map_err(|_| "local input unavailable")?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > maximum {
        return Err("invalid local input");
    }
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            return Err("private inputs must be owner-only");
        }
    }
    let mut bytes = Zeroizing::new(Vec::new());
    std::fs::File::open(path)
        .map_err(|_| "local input unavailable")?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "local input unavailable")?;
    if bytes.len() as u64 > maximum {
        return Err("invalid local input");
    }
    Ok(bytes)
}

fn digest(value: &str) -> Result<[u8; 32], &'static str> {
    let hash = parse_digest32(value).map_err(|_| "invalid runtime pin")?;
    if hash == [0; 32] {
        return Err("zero runtime pin");
    }
    Ok(hash)
}

async fn run(recovery: Option<&str>) -> Result<(), &'static str> {
    let path = PathBuf::from(
        std::env::var_os("EREBUS_SHIELDED_RELAY_CONFIG")
            .ok_or("configure EREBUS_SHIELDED_RELAY_CONFIG")?,
    );
    let config: Config = serde_json::from_slice(&input(&path, 32_768, true)?)
        .map_err(|_| "invalid relay configuration")?;
    if config.version != 1
        || config.chain_id != 10143
        || config.verifier_version == 0
        || !config.allow_test_artifacts
        || config.port == 0
        || !(1..=900).contains(&config.timeout_seconds)
    {
        return Err("relay requires explicit Monad testnet policy");
    }
    let pool = parse_address(&config.pool).map_err(|_| "invalid pool")?;
    let asset = parse_address(&config.asset).map_err(|_| "invalid asset")?;
    let sponsor = parse_address(&config.sponsor).map_err(|_| "invalid sponsor")?;
    if [pool, asset, sponsor].contains(&[0; 20]) {
        return Err("zero relay address");
    }
    let address = AddressBytes::new(pool.to_vec()).map_err(|_| "invalid pool")?;
    let namespace = ChainNamespace::parse("eip155:10143").map_err(|_| "invalid namespace")?;
    let context = SettlementContext {
        require_local_proving: true,
        mode: SettlementMode::Shielded,
        domain: DeploymentDomain {
            namespace,
            settlement_contract: Some(address.clone()),
            pool: Some(address),
            verifier_version: config.verifier_version,
        },
        suite_id: SHIELDED_POSEIDON_EDDSA_SUITE_ID,
        asset: AssetId::parse(&format!("eip155:10143/erc20:0x{}", hex::encode(asset)))
            .map_err(|_| "invalid asset")?,
        required_guarantees: GuaranteeSet::from_bits(SHIELDED_GUARANTEES)
            .map_err(|_| "invalid guarantees")?,
    };
    let policy = RelayAdmissionPolicy {
        context,
        deployment: IndexDomain {
            chain_id: config.chain_id,
            pool,
            first_block: config.first_block,
            first_hash: digest(&config.first_hash)?,
        },
        runtime_pins: PoolRuntimePins {
            pool: digest(&config.runtime_pins.pool)?,
            poseidon: digest(&config.runtime_pins.poseidon)?,
            deposit: digest(&config.runtime_pins.deposit)?,
            transfer: digest(&config.runtime_pins.transfer)?,
            withdraw: digest(&config.runtime_pins.withdraw)?,
        },
        sponsor,
        gas_limit: config.gas_limit,
    };
    let verifier = TransferVerifier::from_zkey_bytes(
        &input(&config.transfer_zkey_file, 128 << 20, false)?,
        &config.transfer_zkey_sha256,
    )
    .map_err(|_| "transfer key authentication failed")?;
    let key = input(&config.transaction_key_file, 32, true)?;
    let token = input(&config.authentication_key_file, 32, true)?;
    let key: &[u8; 32] = key
        .as_slice()
        .try_into()
        .map_err(|_| "gas key must be 32 raw bytes")?;
    let token: [u8; 32] = token
        .as_slice()
        .try_into()
        .map_err(|_| "authentication key must be 32 raw bytes")?;
    let fees = Eip1559Fees::new(
        config
            .max_fee_per_gas
            .parse()
            .map_err(|_| "invalid fee cap")?,
        config
            .max_priority_fee_per_gas
            .parse()
            .map_err(|_| "invalid fee cap")?,
    )
    .map_err(|_| "invalid fee cap")?;
    let replacement_fees = config
        .replacement_fees
        .map(|[max, priority]| {
            Eip1559Fees::new(
                max.parse().map_err(|_| "invalid replacement fee cap")?,
                priority
                    .parse()
                    .map_err(|_| "invalid replacement fee cap")?,
            )
            .map_err(|_| "invalid replacement fee cap")
        })
        .transpose()?;
    let root = config.state_root.clone();
    let engine = SponsoredRelay::connect(
        SponsorConfig {
            root: config.state_root,
            signer_root: config.signer_journal_root,
            admission: policy,
            fees,
            replacement_fees,
            rpc_url: config.rpc_url,
            peer_rpc_url: config.peer_rpc_url,
            timeout: Duration::from_secs(config.timeout_seconds),
        },
        verifier,
        key,
    )
    .await
    .map_err(|_| "relay backend unavailable")?;
    if let Some(recovery) = recovery {
        let mut request = Vec::new();
        std::io::stdin()
            .take(2049)
            .read_to_end(&mut request)
            .map_err(|_| "relay recovery request unavailable")?;
        if request.len() > 2048 {
            return Err("relay recovery request too large");
        }
        let status = if recovery == "--replace" {
            engine.replace(&request).await
        } else {
            engine.resume(&request).await
        }
        .map_err(|_| "relay recovery unavailable; retain original operation")?;
        println!(
            "{}",
            json!({"protocol_version":1,"explicit_resume":recovery == "--resume",
            "explicit_replacement":recovery == "--replace",
            "status": match status.finalized { Some(finalized) if finalized.success => "finalized",
                Some(_) => "reverted", None => "pending" },
            "operation_ref":hex::encode(status.operation_ref),
            "transaction_hash":status.transaction_hash.map(|hash| format!("0x{}", hex::encode(hash))),
            "retry_without_new_payment":true,"payment_verified":false,"automatic_rebroadcast":false})
        );
        return Ok(());
    }
    let service = RelayHttpService::new(
        engine,
        token,
        &root.join("quota"),
        RelayHttpPolicy {
            requests_per_window: config.requests_per_window,
            window_seconds: config.window_seconds,
            maximum_gas_budget: config
                .maximum_gas_budget
                .parse()
                .map_err(|_| "invalid sponsorship budget")?,
            maximum_operations: config.maximum_operations,
            timeout_seconds: config.timeout_seconds,
        },
    )?;
    let listener =
        tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, config.port))
            .await
            .map_err(|_| "relay listen unavailable")?;
    axum::serve(listener, service.router())
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| "relay serve unavailable")
}

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!(
            "{}",
            json!({"service":"erebus-shielded-relayer","protocol_version":1,
            "user_fee":0,"automatic_rebroadcast":false,"testnet_only":true,
            "explicit_resume":true,"explicit_fee_replacement":true})
        );
        return;
    }
    if args == ["--help"] {
        println!("erebus-shielded-relayer: configure an owner-only EREBUS_SHIELDED_RELAY_CONFIG.\nLoopback HTTP: GET /healthz, POST /v1/relay, POST /v1/observe.\nUse an authenticated TLS gateway for remote clients. Bearer token is a hex-encoded 32-byte operator key.\n--resume reads one original proof-only request from stdin and explicitly recovers retained bytes; never use a new payment ID.\n--replace uses the original request and owner-configured replacement_fees [max fee, priority fee]; configure the ceiling before first use.\nReplacement changes only fee caps, never nonce, gas, value or calldata. Budgets reserve the ceiling before admission.\nAccepts proofs/public inputs only; never witnesses, keys or agreement openings.\nSponsorship budgets persist and never reset automatically. Testnet only, single-operator setup, unaudited.");
        return;
    }
    let recovery = args
        .first()
        .map(String::as_str)
        .filter(|arg| *arg == "--resume" || *arg == "--replace");
    if !args.is_empty() && (args.len() != 1 || recovery.is_none()) {
        eprintln!("invalid relay arguments");
        std::process::exit(1);
    }
    if let Err(error) = run(recovery).await {
        eprintln!("{}", json!({"status":"error","error":error}));
        std::process::exit(1);
    }
}
