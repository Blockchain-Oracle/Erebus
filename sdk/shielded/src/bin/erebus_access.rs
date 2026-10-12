//! Path-only local buyer authentication: snapshot retrieval and prepaid metered calls.

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use erebus_shielded_prover::access::{
    client::{AccessClient, RetrievalError, RetrievalReceipt},
    metered::{MeteredCallReceipt, MeteredClient, MeteredClientError, MeteredStatusReceipt},
};
use erebus_transport::disclosure::{SelectedAgreement, MAX_DISCLOSURE_BYTES};
use serde::Deserialize;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    method: String,
    evidence_file: PathBuf,
    buyer_key_file: PathBuf,
    service_url: String,
    service_id: String,
    cache_root: PathBuf,
    #[serde(default)]
    allow_loopback_http: bool,
    #[serde(default)]
    x402_exact: bool,
    #[serde(default)]
    call_index: Option<u64>,
}

#[tokio::main]
async fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && args[0] == "--version" {
        println!(
            "{}",
            serde_json::json!({
                "status": "ok",
                "protocol": 2,
                "methods": ["retrieve", "metered_call", "metered_status"],
            })
        );
        exit(0);
    }
    if args.len() == 1 && args[0] == "--help" {
        println!("erebus-access: authenticate with the buyer agreement key and retrieve one signed snapshot, or consume one prepaid metered call.\nOne JSON request: method=retrieve, metered_call, or metered_status, evidence_file, buyer_key_file, service_url, service_id, cache_root.\nThe endpoint must be HTTPS ending in /v1/access for retrieve and /v1/metered for metered methods; loopback HTTP requires explicit development opt-in.\nKeys remain in owner-only local files. Payloads are saved locally, not printed.\nretrieve (default mode) retrieves already paid resources. Explicit x402_exact=true signs and persists one Permit2 authorization before HTTP; the seller submits payment. Retries reuse that authorization.\nmetered_call consumes the next prepaid call, finishes the call in flight, or returns an earlier call from cache. An optional call_index must not exceed the next unconsumed index. A timeout, pending payment, or lost reply keeps the same index in flight; retry it, never another payment.\nmetered_status reports the seller's allowance and reconciles the local cursor. It consumes nothing.\nExit 2 means pending access. A content receipt is not independent payment or delivery evidence.");
        exit(0);
    }
    if !args.is_empty() {
        finish(Err(RetrievalError::Configuration));
    }
    let mut bytes = Zeroizing::new(Vec::new());
    if std::io::stdin()
        .take(16385)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() > 16384
    {
        finish(Err(RetrievalError::Configuration));
    }
    let request: Request = match serde_json::from_slice(&bytes) {
        Ok(request) => request,
        Err(_) => finish(Err(RetrievalError::Configuration)),
    };
    match request.method.as_str() {
        "retrieve" => finish(retrieve(request).await),
        "metered_call" => finish_metered_call(metered_call(request).await),
        "metered_status" => finish_metered_status(metered_status(request).await),
        _ => finish(Err(RetrievalError::Configuration)),
    }
}

fn parse_service_id(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    hex::decode(value).ok()?.try_into().ok()
}

/// A 32-byte seed, raw or as trimmed hex with an optional `0x` prefix.
fn seed_bytes(bytes: &[u8]) -> Option<Zeroizing<[u8; 32]>> {
    if bytes.len() == 32 {
        return Some(Zeroizing::new(bytes.try_into().ok()?));
    }
    let text = std::str::from_utf8(bytes).ok()?.trim();
    let decoded = Zeroizing::new(hex::decode(text.strip_prefix("0x").unwrap_or(text)).ok()?);
    Some(Zeroizing::new(decoded.as_slice().try_into().ok()?))
}

fn unix_now() -> Result<u64, RetrievalError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_secs())
        .map_err(|_| RetrievalError::Authentication)
}

async fn retrieve(request: Request) -> Result<RetrievalReceipt, RetrievalError> {
    if request.call_index.is_some() {
        return Err(RetrievalError::Configuration);
    }
    let service_id = parse_service_id(&request.service_id).ok_or(RetrievalError::Configuration)?;
    let evidence_file = request.evidence_file;
    let key_file = request.buyer_key_file;
    let prepared = tokio::task::spawn_blocking(move || {
        let evidence =
            SelectedAgreement::decode(&private_read(&evidence_file, MAX_DISCLOSURE_BYTES)?)
                .map_err(|_| RetrievalError::Agreement)?;
        let client = AccessClient::open(
            &request.service_url,
            service_id,
            request.cache_root,
            request.allow_loopback_http,
        )?;
        if let Some(receipt) = client.cached(&evidence)? {
            return Ok((client, evidence, None, Some(receipt)));
        }
        let bytes = private_read(&key_file, 128)?;
        let seed = seed_bytes(&bytes).ok_or(RetrievalError::Authentication)?;
        Ok::<_, RetrievalError>((client, evidence, Some(seed), None))
    })
    .await
    .map_err(|_| RetrievalError::Storage)??;
    let (client, evidence, seed, cached) = prepared;
    if let Some(cached) = cached {
        return Ok(cached);
    }
    let now = unix_now()?;
    let seed = seed.ok_or(RetrievalError::Authentication)?;
    if request.x402_exact {
        client.retrieve_x402(&evidence, &seed, now).await
    } else {
        client.retrieve(&evidence, &seed, now).await
    }
}

/// Opens the durable cursor, the verified agreement, and the buyer seed for one metered call.
async fn open_metered(
    request: &Request,
) -> Result<(MeteredClient, SelectedAgreement, Zeroizing<[u8; 32]>), MeteredClientError> {
    if request.x402_exact {
        return Err(MeteredClientError::Configuration);
    }
    let service_id =
        parse_service_id(&request.service_id).ok_or(MeteredClientError::Configuration)?;
    let evidence_file = request.evidence_file.clone();
    let key_file = request.buyer_key_file.clone();
    let service_url = request.service_url.clone();
    let cache_root = request.cache_root.clone();
    let allow_loopback_http = request.allow_loopback_http;
    tokio::task::spawn_blocking(move || {
        let evidence = SelectedAgreement::decode(
            &private_read(&evidence_file, MAX_DISCLOSURE_BYTES)
                .map_err(|_| MeteredClientError::Storage)?,
        )
        .map_err(|_| MeteredClientError::Agreement)?;
        let client =
            MeteredClient::open(&service_url, service_id, cache_root, allow_loopback_http)?;
        let bytes = private_read(&key_file, 128).map_err(|_| MeteredClientError::Storage)?;
        let seed = seed_bytes(&bytes).ok_or(MeteredClientError::Authentication)?;
        Ok((client, evidence, seed))
    })
    .await
    .map_err(|_| MeteredClientError::Storage)?
}

async fn metered_call(request: Request) -> Result<MeteredCallReceipt, MeteredClientError> {
    let call_index = request.call_index;
    let (client, evidence, seed) = open_metered(&request).await?;
    let now = unix_now().map_err(|_| MeteredClientError::Authentication)?;
    client.call(&evidence, &seed, call_index, now).await
}

async fn metered_status(request: Request) -> Result<MeteredStatusReceipt, MeteredClientError> {
    if request.call_index.is_some() {
        return Err(MeteredClientError::Configuration);
    }
    let (client, evidence, seed) = open_metered(&request).await?;
    let now = unix_now().map_err(|_| MeteredClientError::Authentication)?;
    client.status(&evidence, &seed, now).await
}

fn private_read(path: &Path, limit: usize) -> Result<Zeroizing<Vec<u8>>, RetrievalError> {
    let info = std::fs::symlink_metadata(path).map_err(|_| RetrievalError::Storage)?;
    if !info.is_file() || info.len() > limit as u64 {
        return Err(RetrievalError::Storage);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if info.permissions().mode() & 0o077 != 0 {
            return Err(RetrievalError::Storage);
        }
    }
    #[cfg(not(unix))]
    return Err(RetrievalError::Configuration);
    let mut bytes = Zeroizing::new(Vec::new());
    std::fs::File::open(path)
        .map_err(|_| RetrievalError::Storage)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| RetrievalError::Storage)?;
    if bytes.len() > limit {
        return Err(RetrievalError::Storage);
    }
    Ok(bytes)
}

fn finish(result: Result<RetrievalReceipt, RetrievalError>) -> ! {
    match result {
        Ok(receipt) => emit(
            0,
            serde_json::json!({"status":"retrieved","result":receipt}),
        ),
        Err(RetrievalError::Pending) => emit(
            2,
            serde_json::json!({"status":"pending","retry_without_payment":true,"payment_verified":false,"resource_verified":false,"delivery_verified":false}),
        ),
        Err(RetrievalError::SellerReportedUndelivered) => emit(
            2,
            serde_json::json!({"status":"paid_but_undelivered","seller_reported_payment_finalized":true,"retry_without_payment":true,"payment_verified":false,"resource_verified":false,"delivery_verified":false}),
        ),
        Err(error) => emit(
            1,
            serde_json::json!({"status":"error","error":error.to_string(),"retry_without_payment":true}),
        ),
    }
}

fn finish_metered_call(result: Result<MeteredCallReceipt, MeteredClientError>) -> ! {
    match result {
        Ok(receipt) => emit(0, serde_json::json!({"status":"issued","result":receipt})),
        Err(MeteredClientError::Pending) => emit(
            2,
            serde_json::json!({"status":"pending","retry_without_payment":true,"payment_verified":false,"resource_verified":false,"delivery_verified":false}),
        ),
        Err(error) => emit(
            1,
            serde_json::json!({"status":"error","error":error.to_string(),"error_code":error.code(),"retry_without_payment":true}),
        ),
    }
}

fn finish_metered_status(result: Result<MeteredStatusReceipt, MeteredClientError>) -> ! {
    match result {
        Ok(receipt) => emit(
            0,
            serde_json::json!({"status":"allowance","result":receipt}),
        ),
        Err(MeteredClientError::Pending) => emit(
            2,
            serde_json::json!({"status":"pending","retry_without_payment":true,"payment_verified":false,"resource_verified":false,"delivery_verified":false}),
        ),
        Err(error) => emit(
            1,
            serde_json::json!({"status":"error","error":error.to_string(),"error_code":error.code(),"retry_without_payment":true}),
        ),
    }
}

fn emit(code: i32, output: serde_json::Value) -> ! {
    println!("{output}");
    exit(code);
}

fn exit(code: i32) -> ! {
    let flushed = std::io::stdout().flush().is_ok();
    std::process::exit(if flushed { code } else { 1 });
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn private_inputs_require_bounded_regular_owner_only_files() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("input");
        std::fs::write(&file, [7; 32]).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(private_read(&file, 32).unwrap().len(), 32);
        assert!(matches!(
            private_read(&file, 31),
            Err(RetrievalError::Storage)
        ));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(matches!(
            private_read(&file, 128),
            Err(RetrievalError::Storage)
        ));
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(matches!(
            private_read(&link, 128),
            Err(RetrievalError::Storage)
        ));
        assert!(matches!(
            private_read(root.path(), 128),
            Err(RetrievalError::Storage)
        ));
    }

    #[test]
    fn seeds_accept_raw_and_hex_forms() {
        assert_eq!(*seed_bytes(&[7; 32]).unwrap(), [7; 32]);
        let hex = "07".repeat(32);
        assert_eq!(*seed_bytes(hex.as_bytes()).unwrap(), [7; 32]);
        assert_eq!(*seed_bytes(format!("0x{hex}").as_bytes()).unwrap(), [7; 32]);
        assert_eq!(
            *seed_bytes(format!(" {hex}\n").as_bytes()).unwrap(),
            [7; 32]
        );
        assert!(seed_bytes(b"not a seed").is_none());
    }
}
