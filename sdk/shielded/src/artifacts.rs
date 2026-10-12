//! Authenticated public artifact installation, separate from private witness generation.
//!
//! The operator supplies a manifest digest obtained independently of the download server.
//! A digest authenticates the supplied bytes, not the ceremony or the deployed verifier.
//! Download requests carry no wallet data. The operator must control the cache and its parents.

use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::{LocalProof, ProverError, ProvingArtifacts};

/// Maximum authenticated manifest length.
pub const MAX_MANIFEST_BYTES: usize = 16 * 1024;
/// Maximum size of one downloaded artifact. Streaming never trusts Content-Length alone.
pub const MAX_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;

/// One circuit supported by the pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CircuitKind {
    /// Shielding an externally visible deposit.
    Deposit,
    /// Private agreement-bound transfer.
    Transfer,
    /// Unshielding to an externally visible recipient.
    Withdraw,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArtifactFile {
    url: String,
    sha256: String,
    bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Circuit {
    kind: CircuitKind,
    wasm: ArtifactFile,
    r1cs: ArtifactFile,
    zkey: ArtifactFile,
    #[serde(default)]
    identity: Option<CircuitIdentity>,
}

/// Publisher-attested identities for a reviewed circuit and its deployed verifier.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CircuitIdentity {
    /// Hash of the release's canonical source inventory, including transitive includes.
    source_inventory_sha256: String,
    witness_schema_version: u16,
    verification_key_sha256: String,
    /// Keccak-256 of the complete deployed runtime, not creation bytecode.
    verifier_runtime_keccak256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u16,
    chain_id: u64,
    settlement_contract: String,
    verifier_version: u32,
    test_only: bool,
    circuits: Vec<Circuit>,
}

/// A manifest whose complete bytes matched an independently trusted digest.
/// Fields are private so URLs, sizes, and digests cannot be changed after authentication.
pub struct ArtifactRelease {
    manifest: Manifest,
    publisher_authenticated: bool,
}

/// Detached publisher signature over the exact manifest bytes, with a protocol-specific domain.
/// The publisher address must be pinned independently in operator configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestSignature {
    /// Signature-envelope version, currently 1.
    pub version: u16,
    /// SHA-256 of the exact manifest file, without JSON reserialization.
    pub manifest_sha256: String,
    /// Canonical suite-1 signature: lowercase hex `r || s || recovery_id`.
    pub signature_hex: String,
}

/// Operator-pinned publisher and detached signature. Never load the trusted publisher from
/// the manifest or the artifact server; the signature alone does not establish trust.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestAuthorization {
    /// Independently trusted lowercase Ethereum-style publisher address.
    pub publisher: String,
    /// Detached signature supplied with the release.
    pub signature: ManifestSignature,
}

fn manifest_signature_digest(sha256: &[u8; 32]) -> [u8; 32] {
    erebus_core::suite::keccak256(&[b"EREBUS_ARTIFACT_MANIFEST_SIGNATURE_V1", sha256])
}

fn valid_identity_hash(value: &str) -> bool {
    value.len() == 64
        && value != "0".repeat(64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl ManifestSignature {
    /// Signs exact public manifest bytes with a separate release-publisher identity.
    /// Signing does not certify the circuit, ceremony, or deployed verifier.
    pub fn sign(
        bytes: &[u8],
        publisher: &erebus_transport::identity::AuthorizationIdentity,
    ) -> Result<Self, InstallError> {
        if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
            return Err(InstallError::Manifest);
        }
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        Ok(Self {
            version: 1,
            manifest_sha256: hex::encode(digest),
            signature_hex: hex::encode(publisher.sign_digest(&manifest_signature_digest(&digest))),
        })
    }
}

/// Local cache and explicit development exceptions. There are no private proving inputs here.
pub struct InstallOptions {
    /// Private cache directory, with parent directories controlled by the operator.
    pub cache_root: PathBuf,
    /// Whole-download timeout, including the response body, between 1 and 600 seconds.
    pub timeout: Duration,
    /// Explicit opt-in for insecure prototype artifacts. False for a developer release.
    pub allow_test_artifacts: bool,
    /// Allow HTTP only to literal loopback or localhost for development tests.
    pub allow_loopback_http: bool,
}

/// Verified paths ready for the existing local prover. This is not proof generation.
#[derive(Debug)]
pub struct InstalledArtifacts {
    /// Hash-pinned public files.
    pub artifacts: ProvingArtifacts,
    /// Actual response bytes downloaded this time.
    pub downloaded_bytes: u64,
    /// Number of files verified and reused from the cache.
    pub cache_hits: u8,
}

/// A locally verified proof and separate installation/proving measurements.
#[derive(Debug)]
pub struct InstalledLocalProof {
    /// Verified local proof; never contains the private witness.
    pub proof: LocalProof,
    /// Actual bytes fetched from the public artifact server.
    pub downloaded_bytes: u64,
    /// Files reused after checking their hashes again.
    pub cache_hits: u8,
    /// Installation wall time, including cache checks.
    pub installation_ms: u128,
    /// Local proving wall time, including witness generation and local verification.
    pub proving_ms: u128,
}

/// Public installation and local proving have separate failure boundaries.
#[derive(Debug, thiserror::Error)]
pub enum InstalledProvingError {
    /// No proof was generated because public artifact installation failed.
    #[error(transparent)]
    Installation(#[from] InstallError),
    /// Local witness, expected transition, or proof was rejected.
    #[error(transparent)]
    Proving(#[from] ProverError),
    /// Local proving worker failed without returning a proof.
    #[error("local proving worker failed")]
    Worker,
}

struct PrivateWitness(serde_json::Value);

impl Drop for PrivateWitness {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        fn clear(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::String(text) => text.zeroize(),
                serde_json::Value::Array(values) => values.iter_mut().for_each(clear),
                serde_json::Value::Object(values) => values.values_mut().for_each(clear),
                _ => *value = serde_json::Value::Null,
            }
        }
        clear(&mut self.0);
    }
}

/// Installation failed. Provider URLs and error bodies are never included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InstallError {
    /// Manifest bytes, schema, hashes, sizes, or deployment metadata are invalid.
    #[error("artifact manifest is invalid or does not match its trusted digest")]
    Manifest,
    /// Detached signature is absent, malformed, or not made by the pinned publisher.
    #[error("artifact publisher signature is missing or invalid")]
    Publisher,
    /// A prototype release was not explicitly allowed.
    #[error("test-only proving artifacts require explicit development opt-in")]
    TestOnly,
    /// URL or timeout is not allowed.
    #[error("artifact download configuration is invalid")]
    Configuration,
    /// Private cache, lock, temporary file, or durable publication failed.
    #[error("artifact cache is unavailable or unsafe")]
    Cache,
    /// Bounded public download failed. No witness was sent.
    #[error("public artifact download failed; retry installation")]
    Download,
    /// Actual length or SHA-256 does not match the authenticated manifest.
    #[error("downloaded artifact does not match its pinned length and SHA-256")]
    Integrity,
}

impl ArtifactRelease {
    /// Whether this manifest explicitly identifies insecure local test material.
    pub fn is_test_only(&self) -> bool {
        self.manifest.test_only
    }

    /// Authenticates the entire manifest before interpreting it or making any request.
    pub fn authenticate(bytes: &[u8], expected_sha256: [u8; 32]) -> Result<Self, InstallError> {
        if bytes.len() > MAX_MANIFEST_BYTES
            || <[u8; 32]>::from(Sha256::digest(bytes)) != expected_sha256
        {
            return Err(InstallError::Manifest);
        }
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|_| InstallError::Manifest)?;
        if !matches!(manifest.version, 1 | 2)
            || (!manifest.test_only && manifest.version != 2)
            || manifest.chain_id == 0
            || manifest.verifier_version == 0
            || erebus_evm::deployment::parse_lowercase_address(&manifest.settlement_contract)
                .is_none_or(|address| address == [0; 20])
            || manifest.circuits.len() != 3
        {
            return Err(InstallError::Manifest);
        }
        for kind in [
            CircuitKind::Deposit,
            CircuitKind::Transfer,
            CircuitKind::Withdraw,
        ] {
            if manifest
                .circuits
                .iter()
                .filter(|circuit| circuit.kind == kind)
                .count()
                != 1
            {
                return Err(InstallError::Manifest);
            }
        }
        for circuit in &manifest.circuits {
            match (&circuit.identity, manifest.version) {
                (Some(identity), 2)
                    if identity.witness_schema_version == 2
                        && [
                            &identity.source_inventory_sha256,
                            &identity.verification_key_sha256,
                            &identity.verifier_runtime_keccak256,
                        ]
                        .into_iter()
                        .all(|hash| valid_identity_hash(hash)) => {}
                (None, 1) => {}
                _ => return Err(InstallError::Manifest),
            }
            for file in [&circuit.wasm, &circuit.r1cs, &circuit.zkey] {
                if file.bytes == 0
                    || file.bytes > MAX_ARTIFACT_BYTES
                    || file.sha256.len() != 64
                    || !file
                        .sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                    || file.url.len() > 2_048
                {
                    return Err(InstallError::Manifest);
                }
            }
        }
        Ok(Self {
            manifest,
            publisher_authenticated: false,
        })
    }

    /// Authenticates exact manifest bytes and optionally verifies a detached signature against
    /// an independently pinned publisher. Non-test installation requires this signature.
    pub fn authenticate_authorized(
        bytes: &[u8],
        expected_sha256: [u8; 32],
        authorization: Option<&ManifestAuthorization>,
    ) -> Result<Self, InstallError> {
        let mut release = Self::authenticate(bytes, expected_sha256)?;
        if let Some(authorization) = authorization {
            let signer = erebus_evm::deployment::parse_lowercase_address(&authorization.publisher)
                .filter(|address| address != &[0; 20])
                .ok_or(InstallError::Publisher)?;
            let envelope = &authorization.signature;
            if envelope.version != 1
                || envelope.manifest_sha256 != hex::encode(expected_sha256)
                || envelope.signature_hex.len() != 130
                || !envelope
                    .signature_hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(InstallError::Publisher);
            }
            let signature =
                hex::decode(&envelope.signature_hex).map_err(|_| InstallError::Publisher)?;
            erebus_core::suite::suite(1)
                .map_err(|_| InstallError::Publisher)?
                .verify_authorization(
                    &signer,
                    &manifest_signature_digest(&expected_sha256),
                    &signature,
                )
                .map_err(|_| InstallError::Publisher)?;
            release.publisher_authenticated = true;
        }
        if !release.manifest.test_only && !release.publisher_authenticated {
            return Err(InstallError::Publisher);
        }
        Ok(release)
    }

    /// Requires the caller's independently trusted deployment to match this artifact release.
    pub fn matches_deployment(&self, chain_id: u64, contract: [u8; 20], version: u32) -> bool {
        self.manifest.chain_id == chain_id
            && self.manifest.verifier_version == version
            && erebus_evm::deployment::parse_lowercase_address(&self.manifest.settlement_contract)
                == Some(contract)
    }

    /// Compares signed verifier identities with independently configured live-code pins.
    /// Legacy test manifests have no identities; only their explicit test opt-in permits use.
    pub fn matches_verifier_pins(&self, pins: &crate::rpc::PoolDependencyPins) -> bool {
        if self.manifest.version == 1 {
            return self.manifest.test_only;
        }
        self.manifest.circuits.iter().all(|circuit| {
            let expected = match circuit.kind {
                CircuitKind::Deposit => pins.deposit,
                CircuitKind::Transfer => pins.transfer,
                CircuitKind::Withdraw => pins.withdraw,
            };
            circuit.identity.as_ref().is_some_and(|identity| {
                identity.verifier_runtime_keccak256 == hex::encode(expected)
            })
        })
    }

    /// Installs public artifacts and proves locally without sending witness data to a server.
    /// The caller must derive expected public signals from its authorized transition.
    /// The first three signals must match the manifest's chain, pool, and verifier version.
    /// This checks metadata consistency, not a trusted setup or a live contract's identity.
    pub async fn prove(
        &self,
        kind: CircuitKind,
        options: &InstallOptions,
        input: serde_json::Value,
        expected_public: Vec<String>,
    ) -> Result<InstalledLocalProof, InstalledProvingError> {
        let input = PrivateWitness(input);
        let count = match kind {
            CircuitKind::Deposit => 6,
            CircuitKind::Transfer => 11,
            CircuitKind::Withdraw => 8,
        };
        let contract =
            erebus_evm::deployment::parse_lowercase_address(&self.manifest.settlement_contract)
                .ok_or(InstallError::Manifest)?;
        if expected_public.len() != count
            || expected_public[0] != self.manifest.chain_id.to_string()
            || expected_public[1] != num_bigint::BigUint::from_bytes_be(&contract).to_string()
            || expected_public[2] != self.manifest.verifier_version.to_string()
        {
            return Err(ProverError::PublicInputMismatch.into());
        }
        let started = std::time::Instant::now();
        let installed = self.install(kind, options).await?;
        let installation_ms = started.elapsed().as_millis();
        let started = std::time::Instant::now();
        let proof = tokio::task::spawn_blocking(move || {
            let expected: Vec<_> = expected_public.iter().map(String::as_str).collect();
            installed.artifacts.prove(&input.0, &expected)
        })
        .await
        .map_err(|_| InstalledProvingError::Worker)??;
        Ok(InstalledLocalProof {
            proof,
            downloaded_bytes: installed.downloaded_bytes,
            cache_hits: installed.cache_hits,
            installation_ms,
            proving_ms: started.elapsed().as_millis(),
        })
    }

    /// Installs one circuit, streaming bounded public files into an atomically published cache.
    /// Concurrent processes serialize per content digest; a partial download is never reused.
    pub async fn install(
        &self,
        kind: CircuitKind,
        options: &InstallOptions,
    ) -> Result<InstalledArtifacts, InstallError> {
        if !self.manifest.test_only && !self.publisher_authenticated {
            return Err(InstallError::Publisher);
        }
        if self.manifest.test_only && !options.allow_test_artifacts {
            return Err(InstallError::TestOnly);
        }
        if options.timeout < Duration::from_secs(1) || options.timeout > Duration::from_secs(600) {
            return Err(InstallError::Configuration);
        }
        let circuit = self
            .manifest
            .circuits
            .iter()
            .find(|circuit| circuit.kind == kind)
            .ok_or(InstallError::Manifest)?;
        let allow_http = options.allow_loopback_http;
        for file in [&circuit.wasm, &circuit.r1cs, &circuit.zkey] {
            let url = reqwest::Url::parse(&file.url).map_err(|_| InstallError::Configuration)?;
            if !allowed_url(&url, allow_http) {
                return Err(InstallError::Configuration);
            }
        }
        let client = reqwest::Client::builder()
            .timeout(options.timeout)
            .referer(false)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= 5 || !allowed_url(attempt.url(), allow_http) {
                    attempt.error("artifact redirect is not allowed")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|_| InstallError::Configuration)?;
        private_directory(&options.cache_root)?;
        let mut paths = Vec::new();
        let mut downloaded_bytes = 0;
        let mut cache_hits = 0;
        for file in [&circuit.wasm, &circuit.r1cs, &circuit.zkey] {
            let (path, downloaded) = install_file(file, &options.cache_root, &client).await?;
            paths.push(path);
            if downloaded == 0 {
                cache_hits += 1;
            } else {
                downloaded_bytes += downloaded;
            }
        }
        let artifacts = ProvingArtifacts {
            wasm: paths[0].clone(),
            r1cs: paths[1].clone(),
            zkey: paths[2].clone(),
            wasm_sha256: circuit.wasm.sha256.clone(),
            r1cs_sha256: circuit.r1cs.sha256.clone(),
            zkey_sha256: circuit.zkey.sha256.clone(),
        };
        artifacts.verify().map_err(|_| InstallError::Integrity)?;
        Ok(InstalledArtifacts {
            artifacts,
            downloaded_bytes,
            cache_hits,
        })
    }
}

fn allowed_url(url: &reqwest::Url, allow_http: bool) -> bool {
    url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && (url.scheme() == "https"
            || (allow_http
                && url.scheme() == "http"
                && matches!(
                    url.host_str(),
                    Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
                )))
}

fn private_directory(root: &Path) -> Result<(), InstallError> {
    if root.exists()
        && !fs::symlink_metadata(root)
            .map_err(|_| InstallError::Cache)?
            .is_dir()
    {
        return Err(InstallError::Cache);
    }
    fs::create_dir_all(root).map_err(|_| InstallError::Cache)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))
            .map_err(|_| InstallError::Cache)?;
    }
    Ok(())
}

fn cached(path: &Path, spec: &ArtifactFile) -> Result<bool, InstallError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(InstallError::Cache),
    };
    if !metadata.is_file() {
        return Err(InstallError::Cache);
    }
    if metadata.len() != spec.bytes {
        return Ok(false);
    }
    let mut file = fs::File::open(path)
        .map_err(|_| InstallError::Cache)?
        .take(spec.bytes + 1);
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut length = 0u64;
    loop {
        let count = file.read(&mut buffer).map_err(|_| InstallError::Cache)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        length += count as u64;
    }
    Ok(length == spec.bytes && hex::encode(hash.finalize()) == spec.sha256)
}

async fn install_file(
    spec: &ArtifactFile,
    root: &Path,
    client: &reqwest::Client,
) -> Result<(PathBuf, u64), InstallError> {
    let path = root.join(format!("{}.artifact", spec.sha256));
    let lock_path = root.join(format!("{}.lock", spec.sha256));
    // Waiting for another process's download must not block a Tokio executor thread.
    let _lock = tokio::task::spawn_blocking(move || {
        if let Ok(metadata) = fs::symlink_metadata(&lock_path) {
            if !metadata.is_file() {
                return Err(InstallError::Cache);
            }
        }
        let mut options = fs::OpenOptions::new();
        options.create(true).truncate(false).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(lock_path).map_err(|_| InstallError::Cache)?;
        file.lock_exclusive().map_err(|_| InstallError::Cache)?;
        Ok(file)
    })
    .await
    .map_err(|_| InstallError::Cache)??;
    if cached(&path, spec)? {
        return Ok((path, 0));
    }
    let mut response = client
        .get(&spec.url)
        .send()
        .await
        .map_err(|_| InstallError::Download)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(InstallError::Download);
    }
    if response
        .content_length()
        .is_some_and(|length| length != spec.bytes)
    {
        return Err(InstallError::Integrity);
    }
    let mut download = ArtifactDownload::new(root, &path, spec)?;
    while let Some(chunk) = response.chunk().await.map_err(|_| InstallError::Download)? {
        download.write(&chunk).await?;
    }
    let count = download.finish().await?;
    if !cached(&path, spec)? {
        return Err(InstallError::Integrity);
    }
    Ok((path, count))
}

// Own the temporary file through every write and publication boundary. Dropping an
// interrupted download removes its partial file without publishing unchecked bytes.
struct ArtifactDownload<'a> {
    partial: tempfile::NamedTempFile,
    file: tokio::fs::File,
    root: &'a Path,
    path: &'a Path,
    spec: &'a ArtifactFile,
    hash: Sha256,
    count: u64,
    #[cfg(test)]
    fail_at: Option<&'static str>,
}

impl<'a> ArtifactDownload<'a> {
    fn new(root: &'a Path, path: &'a Path, spec: &'a ArtifactFile) -> Result<Self, InstallError> {
        let partial = tempfile::NamedTempFile::new_in(root).map_err(|_| InstallError::Cache)?;
        let file = tokio::fs::File::from_std(
            partial
                .as_file()
                .try_clone()
                .map_err(|_| InstallError::Cache)?,
        );
        Ok(Self {
            partial,
            file,
            root,
            path,
            spec,
            hash: Sha256::new(),
            count: 0,
            #[cfg(test)]
            fail_at: None,
        })
    }

    fn boundary(&self, _stage: &'static str) -> Result<(), InstallError> {
        #[cfg(test)]
        if self.fail_at == Some(_stage) {
            return Err(std::io::Error::from_raw_os_error(28)).map_err(|_| InstallError::Cache);
        }
        Ok(())
    }

    async fn write(&mut self, chunk: &[u8]) -> Result<(), InstallError> {
        self.count = self
            .count
            .checked_add(chunk.len() as u64)
            .ok_or(InstallError::Integrity)?;
        if self.count > self.spec.bytes {
            return Err(InstallError::Integrity);
        }
        self.boundary("write")?;
        self.hash.update(chunk);
        self.file
            .write_all(chunk)
            .await
            .map_err(|_| InstallError::Cache)
    }

    async fn finish(self) -> Result<u64, InstallError> {
        if self.count != self.spec.bytes
            || hex::encode(self.hash.clone().finalize()) != self.spec.sha256
        {
            return Err(InstallError::Integrity);
        }
        self.boundary("sync")?;
        self.file
            .sync_all()
            .await
            .map_err(|_| InstallError::Cache)?;
        self.boundary("persist")?;
        #[cfg(test)]
        let fail_directory_sync = self.fail_at == Some("directory_sync");
        drop(self.file);
        self.partial
            .persist(self.path)
            .map_err(|_| InstallError::Cache)?;
        #[cfg(test)]
        if fail_directory_sync {
            return Err(InstallError::Cache);
        }
        fs::File::open(self.root)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| InstallError::Cache)?;
        Ok(self.count)
    }
}

#[cfg(test)]
mod download_tests {
    use super::*;

    fn spec() -> ArtifactFile {
        ArtifactFile {
            url: "https://artifacts.invalid/file".into(),
            bytes: 6,
            sha256: hex::encode(Sha256::digest(b"abcdef")),
        }
    }

    #[tokio::test]
    async fn download_disk_pressure_never_publishes_partial_bytes_and_can_retry() {
        for stage in ["write", "sync", "persist", "directory_sync"] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("artifact");
            let spec = spec();
            let mut download = ArtifactDownload::new(root.path(), &path, &spec).unwrap();
            download.write(b"abc").await.unwrap();
            download.fail_at = Some(stage);
            let result = match download.write(b"def").await {
                Ok(()) => download.finish().await,
                Err(error) => {
                    drop(download);
                    Err(error)
                }
            };
            assert!(matches!(result, Err(InstallError::Cache)));
            if stage == "directory_sync" {
                // A failed directory fsync may leave the verified rename visible.
                // Recovery must hash it again rather than assume durable success.
                assert!(cached(&path, &spec).unwrap());
                assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
            } else {
                assert!(!path.exists());
                assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
            }
            let mut retry = ArtifactDownload::new(root.path(), &path, &spec).unwrap();
            retry.write(b"abc").await.unwrap();
            retry.write(b"def").await.unwrap();
            assert_eq!(retry.finish().await.unwrap(), 6);
            assert!(cached(&path, &spec).unwrap());
        }
    }

    #[tokio::test]
    async fn dropped_and_truncated_downloads_do_not_publish_or_replace_cached_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("artifact");
        let spec = spec();
        fs::write(&path, b"abcdef").unwrap();
        let mut download = ArtifactDownload::new(root.path(), &path, &spec).unwrap();
        download.write(b"abc").await.unwrap();
        drop(download);
        assert!(cached(&path, &spec).unwrap());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        let mut download = ArtifactDownload::new(root.path(), &path, &spec).unwrap();
        download.write(b"abc").await.unwrap();
        assert!(matches!(
            download.finish().await,
            Err(InstallError::Integrity)
        ));
        assert!(cached(&path, &spec).unwrap());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn streamed_hash_and_length_mismatches_never_publish() {
        for body in [
            b"abcdeg".as_slice(),
            b"abcdefg".as_slice(),
            b"abc".as_slice(),
        ] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("artifact");
            let spec = spec();
            let mut download = ArtifactDownload::new(root.path(), &path, &spec).unwrap();
            let result = match download.write(body).await {
                Ok(()) => download.finish().await,
                Err(error) => {
                    drop(download);
                    Err(error)
                }
            };
            assert!(matches!(result, Err(InstallError::Integrity)));
            assert!(!path.exists());
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
        }
    }
}
