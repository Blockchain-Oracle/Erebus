//! Public artifact downloads never send private witnesses and publish only verified bytes.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use erebus_shielded_prover::artifacts::{
    ArtifactRelease, CircuitKind, InstallError, InstallOptions, ManifestAuthorization,
    ManifestSignature, MAX_ARTIFACT_BYTES,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

struct Files {
    bodies: HashMap<String, Vec<u8>>,
    calls: AtomicUsize,
    unexpected_request: AtomicBool,
    referrer_query: AtomicBool,
}

struct Server {
    url: String,
    state: Arc<Files>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn public_artifact_request(name: &str, request: axum::extract::Request) -> bool {
    let (parts, body) = request.into_parts();
    let public_name = ["wasm", "r1cs", "zkey"].contains(&name)
        || ["deposit", "transfer", "withdraw"].iter().any(|kind| {
            ["wasm", "r1cs", "zkey"]
                .iter()
                .any(|suffix| name == format!("{kind}.{suffix}"))
        });
    let public_headers = parts
        .headers
        .iter()
        .all(|(header, value)| match header.as_str() {
            "host" => value
                .to_str()
                .ok()
                .and_then(|value| value.parse::<std::net::SocketAddr>().ok())
                .is_some_and(|address| address.ip().is_loopback()),
            "accept" => value == "*/*",
            "content-length" => value == "0",
            _ => false,
        });
    let empty_body = axum::body::to_bytes(body, 1)
        .await
        .is_ok_and(|body| body.is_empty());
    public_name
        && parts.method == axum::http::Method::GET
        && parts.uri.scheme().is_none()
        && parts.uri.authority().is_none()
        && parts.uri.path() == format!("/{name}")
        && parts.uri.query().is_none()
        && public_headers
        && empty_body
}

async fn serve(
    Path(name): Path<String>,
    State(files): State<Arc<Files>>,
    request: axum::extract::Request,
) -> Response {
    files.calls.fetch_add(1, Ordering::SeqCst);
    if request
        .headers()
        .get(axum::http::header::REFERER)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("authorization=download-marker"))
    {
        files.referrer_query.store(true, Ordering::SeqCst);
    }
    if !public_artifact_request(&name, request).await {
        files.unexpected_request.store(true, Ordering::SeqCst);
    }
    let (status, body) = match files.bodies.get(&name) {
        Some(body) => (StatusCode::OK, body.clone()),
        None => (StatusCode::NOT_FOUND, b"provider-private-error".to_vec()),
    };
    // Exercise the streaming byte bound without a trustworthy Content-Length header.
    let chunks: Vec<Result<axum::body::Bytes, std::convert::Infallible>> = body
        .chunks(65536)
        .map(|chunk| Ok(axum::body::Bytes::copy_from_slice(chunk)))
        .collect();
    (
        status,
        axum::body::Body::from_stream(tokio_stream::iter(chunks)),
    )
        .into_response()
}

async fn server(bodies: HashMap<String, Vec<u8>>) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let state = Arc::new(Files {
        bodies,
        calls: AtomicUsize::new(0),
        unexpected_request: AtomicBool::new(false),
        referrer_query: AtomicBool::new(false),
    });
    let router = Router::new()
        .route("/{name}", get(serve))
        .with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Server { url, state, task }
}

fn bodies() -> HashMap<String, Vec<u8>> {
    ["wasm", "r1cs", "zkey"]
        .into_iter()
        .map(|name| (name.into(), format!("public-{name}").into_bytes()))
        .collect()
}

fn manifest(url: &str) -> Value {
    let files = bodies();
    let file = |name: &str| json!({"url":format!("{url}/{name}"), "bytes":files[name].len(), "sha256":hex::encode(Sha256::digest(&files[name]))});
    json!({
        "version":1, "chain_id":10143, "settlement_contract":format!("0x{}", "01".repeat(20)),
        "verifier_version":1, "test_only":true,
        "circuits":(["deposit", "transfer", "withdraw"].map(|kind| json!({"kind":kind,"wasm":file("wasm"),"r1cs":file("r1cs"),"zkey":file("zkey")})))
    })
}

fn authenticate(value: &Value) -> Result<ArtifactRelease, InstallError> {
    let bytes = serde_json::to_vec(value).unwrap();
    ArtifactRelease::authenticate(&bytes, Sha256::digest(&bytes).into())
}

fn release_manifest(url: &str) -> Value {
    let mut value = manifest(url);
    value["version"] = json!(2);
    value["test_only"] = json!(false);
    for circuit in value["circuits"].as_array_mut().unwrap() {
        circuit["identity"] = json!({
            "source_inventory_sha256": "11".repeat(32),
            "witness_schema_version": 2,
            "verification_key_sha256": "22".repeat(32),
            "verifier_runtime_keccak256": "33".repeat(32),
        });
    }
    value
}

#[test]
fn release_identity_metadata_is_required_and_matches_live_verifier_pins() {
    use erebus_shielded_prover::rpc::PoolDependencyPins;
    let value = release_manifest("https://artifacts.invalid");
    let release = authenticate(&value).unwrap();
    let mut pins = PoolDependencyPins {
        poseidon: [4; 32],
        deposit: [0x33; 32],
        transfer: [0x33; 32],
        withdraw: [0x33; 32],
    };
    assert!(release.matches_verifier_pins(&pins));
    pins.withdraw[0] ^= 1;
    assert!(!release.matches_verifier_pins(&pins));
    for field in [
        "source_inventory_sha256",
        "witness_schema_version",
        "verification_key_sha256",
        "verifier_runtime_keccak256",
    ] {
        let mut changed = value.clone();
        changed["circuits"][0]["identity"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(matches!(
            authenticate(&changed),
            Err(InstallError::Manifest)
        ));
    }
    for hash in ["00".repeat(32), "AA".repeat(32), "invalid".into()] {
        let mut changed = value.clone();
        changed["circuits"][0]["identity"]["verification_key_sha256"] = json!(hash);
        assert!(matches!(
            authenticate(&changed),
            Err(InstallError::Manifest)
        ));
    }
    let mut legacy = manifest("https://artifacts.invalid");
    assert!(authenticate(&legacy).is_ok());
    legacy["test_only"] = json!(false);
    assert!(matches!(authenticate(&legacy), Err(InstallError::Manifest)));
    let mut changed = value.clone();
    changed["circuits"][0]["identity"]["witness_schema_version"] = json!(1);
    assert!(matches!(
        authenticate(&changed),
        Err(InstallError::Manifest)
    ));
}

fn options(root: &std::path::Path) -> InstallOptions {
    InstallOptions {
        cache_root: root.join("cache"),
        timeout: Duration::from_secs(5),
        allow_test_artifacts: true,
        allow_loopback_http: true,
    }
}

#[test]
fn publisher_authentication_binds_exact_bytes_and_an_independently_pinned_key() {
    use erebus_transport::identity::AuthorizationIdentity;
    let publisher = AuthorizationIdentity::from_bytes(&[7; 32]).unwrap();
    let stranger = AuthorizationIdentity::from_bytes(&[8; 32]).unwrap();
    let value = release_manifest("https://artifacts.invalid");
    let bytes = serde_json::to_vec(&value).unwrap();
    let digest = Sha256::digest(&bytes).into();
    let authorization = ManifestAuthorization {
        publisher: format!("0x{}", hex::encode(publisher.address())),
        signature: ManifestSignature::sign(&bytes, &publisher).unwrap(),
    };
    assert!(ArtifactRelease::authenticate_authorized(&bytes, digest, Some(&authorization)).is_ok());
    assert!(matches!(
        ArtifactRelease::authenticate_authorized(&bytes, digest, None),
        Err(InstallError::Publisher)
    ));
    let mut wrong = authorization.clone();
    wrong.publisher = format!("0x{}", hex::encode(stranger.address()));
    assert!(matches!(
        ArtifactRelease::authenticate_authorized(&bytes, digest, Some(&wrong)),
        Err(InstallError::Publisher)
    ));
    for mutation in [0, 1, 2] {
        let mut wrong = authorization.clone();
        match mutation {
            0 => wrong.signature.version = 2,
            1 => wrong.signature.signature_hex = "00".repeat(65),
            _ => wrong.signature.manifest_sha256 = "00".repeat(32),
        }
        assert!(matches!(
            ArtifactRelease::authenticate_authorized(&bytes, digest, Some(&wrong)),
            Err(InstallError::Publisher)
        ));
    }
    // Even harmless JSON whitespace changes the signed file identity.
    let changed = serde_json::to_vec_pretty(&value).unwrap();
    assert!(matches!(
        ArtifactRelease::authenticate_authorized(
            &changed,
            Sha256::digest(&changed).into(),
            Some(&authorization)
        ),
        Err(InstallError::Publisher)
    ));
    // An ordinary signature over the manifest digest cannot cross the release domain.
    let mut wrong = authorization.clone();
    wrong.signature.signature_hex = hex::encode(publisher.sign_digest(&digest));
    assert!(matches!(
        ArtifactRelease::authenticate_authorized(&bytes, digest, Some(&wrong)),
        Err(InstallError::Publisher)
    ));
}

#[tokio::test]
async fn unsigned_non_test_artifacts_never_download_even_with_development_opt_in() {
    let value = release_manifest("http://127.0.0.1:9");
    let release = authenticate(&value).unwrap();
    let root = tempfile::tempdir().unwrap();
    assert!(matches!(
        release
            .install(CircuitKind::Deposit, &options(root.path()))
            .await,
        Err(InstallError::Publisher)
    ));
    assert!(!root.path().join("cache").exists());
}

#[tokio::test]
async fn signed_release_reuses_rechecked_cached_artifacts_without_network_or_test_opt_in() {
    use erebus_transport::identity::AuthorizationIdentity;
    let publisher = AuthorizationIdentity::from_bytes(&[7; 32]).unwrap();
    let value = release_manifest("https://artifacts.invalid");
    let bytes = serde_json::to_vec(&value).unwrap();
    let authorization = ManifestAuthorization {
        publisher: format!("0x{}", hex::encode(publisher.address())),
        signature: ManifestSignature::sign(&bytes, &publisher).unwrap(),
    };
    let release = ArtifactRelease::authenticate_authorized(
        &bytes,
        Sha256::digest(&bytes).into(),
        Some(&authorization),
    )
    .unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut config = options(root.path());
    config.allow_test_artifacts = false;
    config.allow_loopback_http = false;
    std::fs::create_dir(&config.cache_root).unwrap();
    for body in bodies().values() {
        let hash = hex::encode(Sha256::digest(body));
        std::fs::write(config.cache_root.join(format!("{hash}.artifact")), body).unwrap();
    }
    let installed = release
        .install(CircuitKind::Transfer, &config)
        .await
        .unwrap();
    assert_eq!((installed.cache_hits, installed.downloaded_bytes), (3, 0));
    installed.artifacts.verify().unwrap();
}

#[tokio::test]
async fn offline_cache_rejects_partial_tampered_and_wrong_size_files_without_publication() {
    let release = authenticate(&manifest("http://127.0.0.1:9")).unwrap();
    for corrupt in [
        b"partial".as_slice(),
        b"public-WASM".as_slice(),
        b"".as_slice(),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut config = options(root.path());
        config.timeout = Duration::from_millis(100);
        std::fs::create_dir(&config.cache_root).unwrap();
        let files = bodies();
        for body in files.values() {
            let hash = hex::encode(Sha256::digest(body));
            std::fs::write(config.cache_root.join(format!("{hash}.artifact")), body).unwrap();
        }
        let wasm_hash = hex::encode(Sha256::digest(&files["wasm"]));
        let wasm = config.cache_root.join(format!("{wasm_hash}.artifact"));
        std::fs::write(&wasm, corrupt).unwrap();
        let interrupted = config.cache_root.join("interrupted-download.tmp");
        std::fs::write(&interrupted, b"not-published").unwrap();
        assert!(release
            .install(CircuitKind::Transfer, &config)
            .await
            .is_err());
        assert_eq!(std::fs::read(&wasm).unwrap(), corrupt);
        assert_eq!(std::fs::read(&interrupted).unwrap(), b"not-published");
        for name in ["r1cs", "zkey"] {
            let hash = hex::encode(Sha256::digest(&files[name]));
            assert_eq!(
                std::fs::read(config.cache_root.join(format!("{hash}.artifact"))).unwrap(),
                files[name]
            );
        }
    }
}

#[tokio::test]
async fn offline_cache_filesystem_obstructions_fail_closed() {
    let release = authenticate(&manifest("https://artifacts.invalid")).unwrap();
    for extension in ["artifact", "lock"] {
        let root = tempfile::tempdir().unwrap();
        let config = options(root.path());
        std::fs::create_dir(&config.cache_root).unwrap();
        let hash = hex::encode(Sha256::digest(&bodies()["wasm"]));
        std::fs::create_dir(config.cache_root.join(format!("{hash}.{extension}"))).unwrap();
        assert!(matches!(
            release.install(CircuitKind::Transfer, &config).await,
            Err(InstallError::Cache)
        ));
    }
    let root = tempfile::tempdir().unwrap();
    let config = options(root.path());
    std::fs::write(&config.cache_root, b"not-a-directory").unwrap();
    assert!(matches!(
        release.install(CircuitKind::Transfer, &config).await,
        Err(InstallError::Cache)
    ));
}

#[test]
fn publisher_cli_signs_without_returning_key_material() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let root = tempfile::tempdir().unwrap();
    let manifest_path = root.path().join("manifest.json");
    let key_path = root.path().join("publisher.key");
    let bytes = serde_json::to_vec(&manifest("https://artifacts.invalid")).unwrap();
    std::fs::write(&manifest_path, &bytes).unwrap();
    std::fs::write(&key_path, [7; 32]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_erebus-artifacts"))
        .arg("--sign-manifest")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(
            json!({"manifest_file":manifest_path,"publisher_key_file":key_path})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result.as_object().unwrap().len(), 3);
    let authorization: ManifestAuthorization = serde_json::from_value(
        json!({"publisher":result["publisher"],"signature":result["signature"]}),
    )
    .unwrap();
    assert!(ArtifactRelease::authenticate_authorized(
        &bytes,
        Sha256::digest(&bytes).into(),
        Some(&authorization)
    )
    .is_ok());
}

#[test]
fn manifest_authentication_and_deployment_binding_fail_closed() {
    let value = manifest("https://artifacts.invalid");
    let bytes = serde_json::to_vec(&value).unwrap();
    assert!(matches!(
        ArtifactRelease::authenticate(&bytes, [0; 32]),
        Err(InstallError::Manifest)
    ));
    let release = authenticate(&value).unwrap();
    assert!(release.matches_deployment(10143, [1; 20], 1));
    assert!(!release.matches_deployment(1, [1; 20], 1));
    for (pointer, replacement) in [
        ("/version", json!(2)),
        ("/chain_id", json!(0)),
        ("/circuits/1/kind", json!("deposit")),
        ("/circuits/0/wasm/bytes", json!(0)),
        ("/circuits/0/wasm/bytes", json!(MAX_ARTIFACT_BYTES + 1)),
        ("/circuits/0/wasm/sha256", json!("AB".repeat(32))),
        (
            "/settlement_contract",
            json!(format!("0x{}", "00".repeat(20))),
        ),
    ] {
        let mut bad = value.clone();
        *bad.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            matches!(authenticate(&bad), Err(InstallError::Manifest)),
            "{pointer}"
        );
    }
}

#[tokio::test]
async fn downloaded_files_are_rechecked_cached_and_repaired() {
    let server = server(bodies()).await;
    let release = authenticate(&manifest(&server.url)).unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = options(root.path());
    let first = release
        .install(CircuitKind::Transfer, &config)
        .await
        .unwrap();
    assert_eq!(first.cache_hits, 0);
    assert_eq!(
        first.downloaded_bytes,
        bodies().values().map(|body| body.len() as u64).sum::<u64>()
    );
    first.artifacts.verify().unwrap();
    let second = release
        .install(CircuitKind::Transfer, &config)
        .await
        .unwrap();
    assert_eq!((second.cache_hits, second.downloaded_bytes), (3, 0));
    assert_eq!(server.state.calls.load(Ordering::SeqCst), 3);
    std::fs::write(&second.artifacts.wasm, b"corrupt").unwrap();
    let repaired = release
        .install(CircuitKind::Transfer, &config)
        .await
        .unwrap();
    assert_eq!(repaired.cache_hits, 2);
    assert_eq!(server.state.calls.load(Ordering::SeqCst), 4);
    repaired.artifacts.verify().unwrap();
}

#[tokio::test]
async fn development_exceptions_are_explicit_and_never_contact_disallowed_urls() {
    let root = tempfile::tempdir().unwrap();
    let mut config = options(root.path());
    let release = authenticate(&manifest("http://127.0.0.1:9")).unwrap();
    config.allow_test_artifacts = false;
    assert!(matches!(
        release.install(CircuitKind::Transfer, &config).await,
        Err(InstallError::TestOnly)
    ));
    config.allow_test_artifacts = true;
    config.allow_loopback_http = false;
    assert!(matches!(
        release.install(CircuitKind::Transfer, &config).await,
        Err(InstallError::Configuration)
    ));
    config.allow_loopback_http = true;
    for url in [
        "http://example.invalid",
        "https://user:secret@example.invalid",
        "https://example.invalid/#secret",
    ] {
        let release = authenticate(&manifest(url)).unwrap();
        assert!(matches!(
            release.install(CircuitKind::Transfer, &config).await,
            Err(InstallError::Configuration)
        ));
    }
    assert!(!config.cache_root.exists());
}

#[tokio::test]
async fn artifact_redirect_does_not_forward_the_source_query_as_a_referrer() {
    let destination = server(bodies()).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = Router::new()
        .route(
            "/{name}",
            get(
                |Path(name): Path<String>, State(url): State<String>| async move {
                    axum::response::Redirect::temporary(&format!("{url}/{name}"))
                },
            ),
        )
        .with_state(destination.url.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let redirect = Server {
        url,
        state: destination.state.clone(),
        task,
    };
    let mut value = manifest(&redirect.url);
    for circuit in value["circuits"].as_array_mut().unwrap() {
        for kind in ["wasm", "r1cs", "zkey"] {
            circuit[kind]["url"] = json!(format!(
                "{}/{kind}?authorization=download-marker",
                redirect.url
            ));
        }
    }
    let release = authenticate(&value).unwrap();
    let root = tempfile::tempdir().unwrap();
    release
        .install(CircuitKind::Transfer, &options(root.path()))
        .await
        .unwrap();
    assert_eq!(destination.state.calls.load(Ordering::SeqCst), 3);
    assert!(
        !destination.state.referrer_query.load(Ordering::SeqCst),
        "redirect exposed the source query token"
    );
    assert!(
        !destination.state.unexpected_request.load(Ordering::SeqCst),
        "redirect forwarded a source URL or unexpected header"
    );
}

#[tokio::test]
async fn wrong_bytes_and_truncation_never_publish_an_artifact() {
    for body in [b"public-WASM".to_vec(), b"short".to_vec(), vec![0; 64]] {
        let mut files = bodies();
        files.insert("wasm".into(), body);
        let server = server(files).await;
        let release = authenticate(&manifest(&server.url)).unwrap();
        let root = tempfile::tempdir().unwrap();
        let config = options(root.path());
        assert!(matches!(
            release.install(CircuitKind::Transfer, &config).await,
            Err(InstallError::Integrity)
        ));
        assert!(std::fs::read_dir(&config.cache_root)
            .unwrap()
            .all(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "lock")));
    }
}

#[tokio::test]
async fn interrupted_and_timed_out_http_downloads_clean_up_and_retry() {
    use tokio_stream::StreamExt;
    for timeout in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let files = Arc::new(Files {
            bodies: bodies(),
            calls: AtomicUsize::new(0),
            unexpected_request: AtomicBool::new(false),
            referrer_query: AtomicBool::new(false),
        });
        let state = files.clone();
        let router = Router::new().route(
            "/{name}",
            get(move || async move {
                let stream = tokio_stream::iter([0, 1]).then(move |index| async move {
                    if index == 0 {
                        Ok(axum::body::Bytes::from_static(b"pub"))
                    } else {
                        tokio::time::sleep(Duration::from_millis(if timeout { 1500 } else { 30 }))
                            .await;
                        Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "interrupted public artifact",
                        ))
                    }
                });
                axum::body::Body::from_stream(stream)
            }),
        );
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let interrupted = Server { url, state, task };
        let root = tempfile::tempdir().unwrap();
        let mut config = options(root.path());
        config.timeout = Duration::from_secs(1);
        let release = authenticate(&manifest(&interrupted.url)).unwrap();
        let result = release.install(CircuitKind::Transfer, &config).await;
        assert!(
            matches!(result, Err(InstallError::Download)),
            "expected a download failure, got {result:?}"
        );
        assert!(std::fs::read_dir(&config.cache_root)
            .unwrap()
            .all(|entry| entry
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "lock")));
        drop(interrupted);
        let honest = server(bodies()).await;
        let retry = authenticate(&manifest(&honest.url)).unwrap();
        config.timeout = Duration::from_secs(5);
        let installed = retry.install(CircuitKind::Transfer, &config).await.unwrap();
        installed.artifacts.verify().unwrap();
        assert_eq!(installed.cache_hits, 0);
    }
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires a dedicated 16 MiB tmpfs mounted at /erebus-pressure"]
async fn actual_tmpfs_exhaustion_cleans_partial_download_and_can_retry() {
    use std::io::Write;

    let mounts = std::fs::read_to_string("/proc/mounts").unwrap();
    assert!(
        mounts.lines().any(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            fields.len() >= 4
                && fields[1] == "/erebus-pressure"
                && fields[2] == "tmpfs"
                && fields[3].split(',').any(|option| option == "size=16384k")
        }),
        "refusing to fill anything except the dedicated 16 MiB tmpfs"
    );
    let root = tempfile::tempdir_in("/erebus-pressure").unwrap();
    let config = options(root.path());
    std::fs::create_dir_all(&config.cache_root).unwrap();
    let mut files = bodies();
    files.insert("wasm".into(), vec![0x42; 1024 * 1024]);
    let server = server(files.clone()).await;
    let mut value = manifest(&server.url);
    value["circuits"][1]["wasm"]["bytes"] = json!(files["wasm"].len());
    value["circuits"][1]["wasm"]["sha256"] = json!(hex::encode(Sha256::digest(&files["wasm"])));
    let release = authenticate(&value).unwrap();

    let ballast_path = root.path().join("ballast");
    let mut ballast = std::fs::File::create(&ballast_path).unwrap();
    let mut exhausted = false;
    for _ in 0..4096 {
        if let Err(error) = ballast.write_all(&[0; 8192]) {
            assert_eq!(error.raw_os_error(), Some(28));
            exhausted = true;
            break;
        }
    }
    assert!(exhausted, "bounded fill did not reach ENOSPC");
    ballast
        .set_len(ballast.metadata().unwrap().len() - 128 * 1024)
        .unwrap();
    ballast.sync_all().unwrap();
    let result = release.install(CircuitKind::Transfer, &config).await;
    assert!(matches!(result, Err(InstallError::Cache)), "{result:?}");
    assert!(server.state.calls.load(Ordering::SeqCst) > 0);
    assert!(std::fs::read_dir(&config.cache_root).unwrap().all(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "lock")
    }));
    drop(ballast);
    std::fs::remove_file(ballast_path).unwrap();
    let installed = release
        .install(CircuitKind::Transfer, &config)
        .await
        .unwrap();
    installed.artifacts.verify().unwrap();
    assert_eq!(installed.cache_hits, 0);
}

#[tokio::test]
async fn concurrent_installers_serialize_and_reuse_verified_files() {
    let server = server(bodies()).await;
    let release = Arc::new(authenticate(&manifest(&server.url)).unwrap());
    let root = tempfile::tempdir().unwrap();
    let mut tasks = Vec::new();
    for _ in 0..4 {
        let release = release.clone();
        let config = options(root.path());
        tasks.push(tokio::spawn(async move {
            release
                .install(CircuitKind::Transfer, &config)
                .await
                .unwrap()
        }));
    }
    let mut downloaded = 0;
    for task in tasks {
        downloaded += task.await.unwrap().downloaded_bytes;
    }
    assert_eq!(
        downloaded,
        bodies().values().map(|body| body.len() as u64).sum::<u64>()
    );
    assert_eq!(server.state.calls.load(Ordering::SeqCst), 3);
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_cache_and_lock_are_rejected() {
    use std::os::unix::fs::symlink;
    let server = server(bodies()).await;
    let release = authenticate(&manifest(&server.url)).unwrap();
    for extension in ["artifact", "lock"] {
        let root = tempfile::tempdir().unwrap();
        let config = options(root.path());
        std::fs::create_dir(&config.cache_root).unwrap();
        let outside = root.path().join("outside");
        std::fs::write(&outside, b"untouched").unwrap();
        let digest = hex::encode(Sha256::digest(&bodies()["wasm"]));
        symlink(
            &outside,
            config.cache_root.join(format!("{digest}.{extension}")),
        )
        .unwrap();
        assert!(matches!(
            release.install(CircuitKind::Transfer, &config).await,
            Err(InstallError::Cache)
        ));
        assert_eq!(std::fs::read(outside).unwrap(), b"untouched");
    }
    assert_eq!(server.state.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn installed_binary_returns_public_paths_and_reuses_cache() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let server = server(bodies()).await;
    let root = tempfile::tempdir().unwrap();
    let bytes = serde_json::to_vec(&manifest(&server.url)).unwrap();
    let path = root.path().join("manifest.json");
    std::fs::write(&path, &bytes).unwrap();
    let request = json!({"manifest_file":path,"manifest_sha256":hex::encode(Sha256::digest(&bytes)),"circuit":"transfer","cache_root":root.path().join("cache"),"allow_test_artifacts":true,"allow_loopback_http":true});
    for expected_hits in [0, 3] {
        let request = request.clone();
        let output = tokio::task::spawn_blocking(move || {
            let mut child = Command::new(env!("CARGO_BIN_EXE_erebus-artifacts"))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(request.to_string().as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["cache_hits"], expected_hits);
        assert_eq!(response["status"], "ok");
        assert!(output.stderr.is_empty());
    }
    assert_eq!(server.state.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn mismatched_proving_domain_fails_before_installation() {
    let root = tempfile::tempdir().unwrap();
    let config = options(root.path());
    let release = authenticate(&manifest("http://127.0.0.1:9")).unwrap();
    for expected in [vec![], vec!["1".into(); 11]] {
        let result = release
            .prove(
                CircuitKind::Transfer,
                &config,
                json!({"secret":"not uploaded"}),
                expected,
            )
            .await;
        assert!(matches!(
            result,
            Err(
                erebus_shielded_prover::artifacts::InstalledProvingError::Proving(
                    erebus_shielded_prover::ProverError::PublicInputMismatch
                )
            )
        ));
    }
    assert!(!config.cache_root.exists());
}

#[tokio::test]
#[ignore = "requires generated M5 prototype artifacts; known test setup, not a release"]
async fn fresh_process_downloads_real_artifacts_and_proves_locally() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let build =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../circuits/m5/build");
    let mut files = HashMap::new();
    for kind in ["deposit", "transfer", "withdraw"] {
        for suffix in ["wasm", "r1cs", "zkey"] {
            let path = if suffix == "wasm" {
                build.join(format!("{kind}_js/{kind}.wasm"))
            } else {
                build.join(format!("{kind}.{suffix}"))
            };
            files.insert(format!("{kind}.{suffix}"), std::fs::read(path).unwrap());
        }
    }
    let server = server(files).await;
    let mut value = manifest(&server.url);
    let input: Value =
        serde_json::from_slice(&std::fs::read(build.join("deposit-input.json")).unwrap()).unwrap();
    value["chain_id"] = json!(input["chainId"].as_str().unwrap().parse::<u64>().unwrap());
    value["settlement_contract"] = json!(format!(
        "0x{:040x}",
        input["contractAddress"]
            .as_str()
            .unwrap()
            .parse::<num_bigint::BigUint>()
            .unwrap()
    ));
    value["verifier_version"] = json!(input["verifierVersion"]
        .as_str()
        .unwrap()
        .parse::<u32>()
        .unwrap());
    for circuit in value["circuits"].as_array_mut().unwrap() {
        let kind = circuit["kind"].as_str().unwrap().to_owned();
        for suffix in ["wasm", "r1cs", "zkey"] {
            let name = format!("{kind}.{suffix}");
            let body = &server.state.bodies[&name];
            circuit[suffix] = json!({"url":format!("{}/{name}",server.url),"sha256":hex::encode(Sha256::digest(body)),"bytes":body.len()});
        }
    }
    let root = tempfile::tempdir().unwrap();
    let manifest_path = root.path().join("manifest.json");
    let bytes = serde_json::to_vec(&value).unwrap();
    std::fs::write(&manifest_path, &bytes).unwrap();
    for (kind, names) in [
        (
            "deposit",
            vec![
                "chainId",
                "contractAddress",
                "verifierVersion",
                "asset",
                "amount",
                "noteCommitment",
            ],
        ),
        (
            "transfer",
            vec![
                "chainId",
                "contractAddress",
                "verifierVersion",
                "asset",
                "dealCommitment",
                "dealNullifier",
                "root",
                "inputNullifier",
                "paymentCommitment",
                "changeCommitment",
                "expiry",
            ],
        ),
        (
            "withdraw",
            vec![
                "chainId",
                "contractAddress",
                "verifierVersion",
                "asset",
                "root",
                "noteNullifier",
                "recipient",
                "amount",
            ],
        ),
    ] {
        let witness_path = root.path().join(format!("{kind}.json"));
        let witness = std::fs::read(build.join(format!("{kind}-input.json"))).unwrap();
        std::fs::write(&witness_path, &witness).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&witness_path, std::fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        let input: Value = serde_json::from_slice(&witness).unwrap();
        let expected: Vec<_> = names
            .iter()
            .map(|name| input[*name].as_str().unwrap())
            .collect();
        let request = json!({"manifest_file":manifest_path,"manifest_sha256":hex::encode(Sha256::digest(&bytes)),"circuit":kind,"cache_root":root.path().join("cache"),"witness_file":witness_path,"expected_public":expected,"allow_test_artifacts":true,"allow_loopback_http":true});
        let output = tokio::task::spawn_blocking(move || {
            let mut child = Command::new(env!("CARGO_BIN_EXE_erebus-local-prove"))
                .env_clear()
                .current_dir("/")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(request.to_string().as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        })
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{kind}: local proof failed (output redacted)"
        );
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], "ok");
        assert!(response["downloaded_bytes"].as_u64().unwrap() > 0);
        assert_eq!(response["calldata"][3], json!(expected));
        assert_eq!(response["cache_hits"], 0);
        assert!(output.stderr.is_empty());
        println!(
            "{kind}: download={} bytes, install={} ms, prove={} ms",
            response["downloaded_bytes"], response["installation_ms"], response["proving_ms"]
        );
    }
    assert_eq!(server.state.calls.load(Ordering::SeqCst), 9);
    assert!(
        !server.state.unexpected_request.load(Ordering::SeqCst),
        "artifact HTTP request included unexpected data"
    );
    // The proving cache contains only hash-authenticated public artifacts and empty locks.
    for entry in std::fs::read_dir(root.path().join("cache")).unwrap() {
        let path = entry.unwrap().path();
        let content = std::fs::read(&path).unwrap();
        if path
            .extension()
            .is_some_and(|extension| extension == "lock")
        {
            assert!(content.is_empty());
        } else {
            assert!(
                server
                    .state
                    .bodies
                    .values()
                    .any(|artifact| artifact == &content),
                "unexpected private or temporary cache file"
            );
        }
    }
}

#[tokio::test]
async fn artifact_request_auditor_rejects_body_query_and_private_headers() {
    use axum::{body::Body, http::Request};
    let request = Request::builder()
        .method("GET")
        .uri("/transfer.wasm")
        .header("host", "127.0.0.1:1234")
        .header("accept", "*/*")
        .body(Body::empty())
        .unwrap();
    assert!(public_artifact_request("transfer.wasm", request).await);
    for request in [
        Request::builder()
            .method("POST")
            .uri("/transfer.wasm")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .uri("/transfer.wasm?witness=private")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .uri("/transfer.wasm")
            .header("authorization", "private")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .uri("/transfer.wasm")
            .body(Body::from("private-witness"))
            .unwrap(),
        Request::builder()
            .uri("/private-opening")
            .body(Body::empty())
            .unwrap(),
        Request::builder()
            .uri("http://private-marker.invalid/transfer.wasm")
            .body(Body::empty())
            .unwrap(),
    ] {
        assert!(!public_artifact_request("transfer.wasm", request).await);
    }
}
