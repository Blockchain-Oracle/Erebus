//! Invalid local inputs must fail before loading a witness engine or generating a proof.

use erebus_shielded_prover::{artifacts::MAX_ARTIFACT_BYTES, ProverError, ProvingArtifacts};
use serde_json::json;
use sha2::{Digest, Sha256};

fn pinned_files(root: &std::path::Path) -> ProvingArtifacts {
    let file = |name: &str| {
        let path = root.join(name);
        // Intentionally not a circuit: an Input error proves validation precedes engine loading.
        std::fs::write(&path, b"not a witness engine").unwrap();
        path
    };
    let hash = hex::encode(Sha256::digest(b"not a witness engine"));
    ProvingArtifacts {
        wasm: file("circuit.wasm"),
        r1cs: file("circuit.r1cs"),
        zkey: file("circuit.zkey"),
        wasm_sha256: hash.clone(),
        r1cs_sha256: hash.clone(),
        zkey_sha256: hash,
    }
}

#[test]
fn malformed_private_fields_fail_before_engine_loading_without_echoing_secrets() {
    let root = tempfile::tempdir().unwrap();
    let artifacts = pinned_files(root.path());
    for input in [json!(null), json!([]), json!(true), json!("private input")] {
        assert!(matches!(
            artifacts.prove(&input, &["1"]),
            Err(ProverError::Input("expected object"))
        ));
    }
    let modulus = "21888242871839275222246405745257275088548364400416034343698204186575808495617";
    for field in [
        json!(""),
        json!("01"),
        json!("-1"),
        json!("+1"),
        json!(" 1"),
        json!("1e2"),
        json!("1".repeat(79)),
        json!(modulus),
        json!(false),
        json!(1),
        json!(null),
        json!({"private": "secret"}),
        json!([]),
        json!([["1"]]),
        json!(["1", "private secret"]),
    ] {
        let result = artifacts.prove(&json!({"private_secret": field}), &["1"]);
        let error = result.unwrap_err();
        assert!(matches!(error, ProverError::Input(_)));
        assert!(!error.to_string().contains("private_secret"));
        assert!(!error.to_string().contains("private secret"));
    }
}

#[test]
fn malformed_public_expectations_fail_before_witness_engine_loading() {
    let root = tempfile::tempdir().unwrap();
    let artifacts = pinned_files(root.path());
    for value in [
        "",
        "01",
        "-1",
        "0x01",
        "1.0",
        "private expected value",
        "21888242871839275222246405745257275088548364400416034343698204186575808495617",
    ] {
        assert!(matches!(
            artifacts.prove(&json!({"signal": "1"}), &[value]),
            Err(ProverError::Input(_))
        ));
    }
}

#[test]
fn direct_artifact_checks_reject_missing_changed_truncated_and_oversized_files() {
    let root = tempfile::tempdir().unwrap();
    for index in 0..3 {
        for mutation in 0..4 {
            let artifacts = pinned_files(root.path());
            let (path, label) = match index {
                0 => (&artifacts.wasm, "wasm"),
                1 => (&artifacts.r1cs, "r1cs"),
                _ => (&artifacts.zkey, "zkey"),
            };
            match mutation {
                0 => std::fs::remove_file(path).unwrap(),
                1 => std::fs::write(path, b"changed bytes").unwrap(),
                2 => std::fs::write(path, b"").unwrap(),
                _ => std::fs::File::create(path)
                    .unwrap()
                    .set_len(MAX_ARTIFACT_BYTES + 1)
                    .unwrap(),
            }
            assert!(
                matches!(artifacts.verify(), Err(ProverError::Artifact(actual)) if actual == label)
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn direct_artifact_checks_reject_symlinks_and_directories() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    for directory in [false, true] {
        let artifacts = pinned_files(root.path());
        std::fs::remove_file(&artifacts.wasm).unwrap();
        if directory {
            std::fs::create_dir(&artifacts.wasm).unwrap();
        } else {
            symlink(&artifacts.r1cs, &artifacts.wasm).unwrap();
        }
        assert!(matches!(
            artifacts.verify(),
            Err(ProverError::Artifact("wasm"))
        ));
        if directory {
            std::fs::remove_dir(&artifacts.wasm).unwrap();
        } else {
            std::fs::remove_file(&artifacts.wasm).unwrap();
        }
    }
}
