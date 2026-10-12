//! Runs only after `cd circuits/m5 && npm ci && npm run prototype`.

use std::{fs, path::PathBuf};

use ark_bn254::{Bn254, Fr};
use ark_crypto_primitives::snark::SNARK;
use ark_groth16::Groth16;
use erebus_shielded_prover::{ProverError, ProvingArtifacts};
use serde_json::Value;

fn artifacts_for_transition(name: &str) -> (ProvingArtifacts, Value) {
    let build = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../circuits/m5/build");
    let manifest: Value = serde_json::from_slice(
        &fs::read(build.join("artifact-manifest.json")).expect("M5 local artifacts"),
    )
    .expect("manifest JSON");
    let entry = &manifest["circuits"][name];
    let string = |field: &str| entry[field].as_str().expect("manifest hash").to_owned();
    let artifacts = ProvingArtifacts {
        wasm: build.join(format!("{name}_js/{name}.wasm")),
        r1cs: build.join(format!("{name}.r1cs")),
        zkey: build.join(format!("{name}.zkey")),
        wasm_sha256: string("wasmSha256"),
        r1cs_sha256: string("r1csSha256"),
        zkey_sha256: string("zkeySha256"),
    };
    let input: Value = serde_json::from_slice(
        &fs::read(build.join(format!("{name}-input.json"))).expect("M5 witness input"),
    )
    .expect("input JSON");
    (artifacts, input)
}

#[test]
#[ignore = "requires M5 local artifact generation"]
fn native_rust_proves_the_same_pool_transitions() {
    let transitions: &[(&str, &[&str], usize)] = &[
        (
            "deposit",
            &[
                "chainId",
                "contractAddress",
                "verifierVersion",
                "asset",
                "amount",
                "noteCommitment",
            ],
            4,
        ),
        (
            "transfer",
            &[
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
            8,
        ),
        (
            "withdraw",
            &[
                "chainId",
                "contractAddress",
                "verifierVersion",
                "asset",
                "root",
                "noteNullifier",
                "recipient",
                "amount",
            ],
            6,
        ),
    ];
    for (name, public_names, changed_index) in transitions {
        let (artifacts, input) = artifacts_for_transition(name);
        let expected: Vec<&str> = public_names
            .iter()
            .map(|field| input[*field].as_str().expect("public input"))
            .collect();
        let mut altered = artifacts.clone();
        altered.wasm_sha256 = "0".repeat(64);
        assert!(matches!(
            altered.verify(),
            Err(ProverError::Artifact("wasm"))
        ));
        let proof = artifacts
            .prove(&input, &expected)
            .expect("native local proof");
        assert_eq!(proof.public_inputs.len(), expected.len());
        let (key, _) =
            ark_circom::read_zkey(&mut fs::File::open(&artifacts.zkey).unwrap()).unwrap();
        let verifier = Groth16::<Bn254>::process_vk(&key.vk).unwrap();
        for component in 0..3 {
            let mut damaged = proof.proof.clone();
            match component {
                0 => damaged.a = Default::default(),
                1 => damaged.b = Default::default(),
                _ => damaged.c = Default::default(),
            }
            assert!(
                !Groth16::<Bn254>::verify_with_processed_vk(
                    &verifier,
                    &proof.public_inputs,
                    &damaged
                )
                .unwrap_or(false),
                "{name}: identity proof component {component} must fail"
            );
        }
        for (index, field) in public_names.iter().enumerate() {
            let mut changed_public = proof.public_inputs.clone();
            changed_public[index] += Fr::from(1u64);
            assert!(
                !Groth16::<Bn254>::verify_with_processed_vk(
                    &verifier,
                    &changed_public,
                    &proof.proof
                )
                .unwrap_or(false),
                "{name}: changed public {field} must fail"
            );
        }
        for other_name in ["deposit", "transfer", "withdraw"] {
            if other_name == *name {
                continue;
            }
            let (other_artifacts, _) = artifacts_for_transition(other_name);
            let (other_key, _) =
                ark_circom::read_zkey(&mut fs::File::open(&other_artifacts.zkey).unwrap()).unwrap();
            let other_verifier = Groth16::<Bn254>::process_vk(&other_key.vk).unwrap();
            assert!(
                !Groth16::<Bn254>::verify_with_processed_vk(
                    &other_verifier,
                    &proof.public_inputs,
                    &proof.proof
                )
                .unwrap_or(false),
                "{name}: a {other_name} verifier must not accept this transition"
            );
        }
        if *name == "transfer" {
            let output = std::env::var_os("EREBUS_M5_TEST_OUTPUT")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../circuits/m5/build")
                })
                .join("rust-transfer-calldata.json");
            fs::write(
                output,
                serde_json::to_vec(&proof.solidity_calldata()).expect("proof JSON"),
            )
            .expect("write public proof calldata");
        }
        let mut wrong = expected;
        wrong[*changed_index] = "1";
        assert!(matches!(
            artifacts.prove(&input, &wrong),
            Err(ProverError::PublicInputMismatch)
        ));
    }
}
