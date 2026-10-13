//! `erebus-shielded-relayer` argument, capability and testnet-policy gates.
//!
//! The funded engine behavior (resume, replacement, older winner) is exercised in
//! `relay_admission.rs`; the CLI itself refuses any chain other than Monad testnet, so a
//! funded local Anvil run through this binary is intentionally impossible.

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn run(args: &[&str], input: &str, config: Option<&Path>) -> (i32, String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_erebus-shielded-relayer"));
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(config) = config {
        command.env("EREBUS_SHIELDED_RELAY_CONFIG", config);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8(output.stdout).unwrap(),
        String::from_utf8(output.stderr).unwrap(),
    )
}

fn private(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

fn local_config(root: &Path) -> std::path::PathBuf {
    let config = root.join("relay.json");
    private(
        &config,
        &serde_json::to_vec(&serde_json::json!({
            "version": 1,
            "chain_id": 31337,
            "pool": "11".repeat(20),
            "asset": "22".repeat(20),
            "verifier_version": 2,
            "first_block": 1,
            "first_hash": "01".repeat(32),
            "runtime_pins": {
                "pool": "02".repeat(32), "poseidon": "03".repeat(32),
                "deposit": "04".repeat(32), "transfer": "05".repeat(32),
                "withdraw": "06".repeat(32),
            },
            "transfer_zkey_file": root.join("transfer.zkey"),
            "transfer_zkey_sha256": "07".repeat(32),
            "transaction_key_file": root.join("gas.key"),
            "authentication_key_file": root.join("auth.key"),
            "sponsor": "33".repeat(20),
            "state_root": root.join("state"),
            "signer_journal_root": root.join("nonces"),
            "rpc_url": "http://127.0.0.1:1",
            "peer_rpc_url": "http://127.0.0.1:2",
            "port": 8555,
            "gas_limit": 3_000_000,
            "max_fee_per_gas": "2000000000",
            "max_priority_fee_per_gas": "1000000000",
            "maximum_gas_budget": "6000000000000000",
            "maximum_operations": 1,
            "requests_per_window": 10,
            "window_seconds": 60,
            "timeout_seconds": 30,
            "allow_test_artifacts": true,
        }))
        .unwrap(),
    );
    config
}

#[test]
fn version_and_help_advertise_explicit_recovery_on_testnet_only() {
    let (code, stdout, _) = run(&["--version"], "", None);
    assert_eq!(code, 0);
    let version: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(version["service"], "erebus-shielded-relayer");
    assert_eq!(version["testnet_only"], true);
    assert_eq!(version["user_fee"], 0);
    assert_eq!(version["automatic_rebroadcast"], false);
    assert_eq!(version["explicit_resume"], true);
    assert_eq!(version["explicit_fee_replacement"], true);

    let (code, help, _) = run(&["--help"], "", None);
    assert_eq!(code, 0);
    assert!(help.contains("Testnet only"));
    assert!(help.contains("--replace"));
}

#[test]
fn a_local_chain_is_refused_before_any_state_or_network_use() {
    let root = tempfile::tempdir().unwrap();
    let config = local_config(root.path());
    let (code, _, stderr) = run(&["--resume"], "{}", Some(&config));
    assert_eq!(code, 1);
    let error: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(error["status"], "error");
    assert_eq!(
        error["error"],
        "relay requires explicit Monad testnet policy"
    );
    assert!(
        !root.path().join("state").exists(),
        "no journal may be created for a refused policy"
    );
    assert!(!root.path().join("nonces").exists());
}

#[test]
fn missing_configuration_and_invalid_arguments_fail_closed() {
    let (code, _, stderr) = run(&["--resume"], "{}", None);
    assert_eq!(code, 1);
    assert!(stderr.contains("configure EREBUS_SHIELDED_RELAY_CONFIG"));

    let (code, _, stderr) = run(&["--resume", "extra"], "{}", None);
    assert_eq!(code, 1);
    assert!(stderr.contains("invalid relay arguments"));

    let root = tempfile::tempdir().unwrap();
    let config = local_config(root.path());
    let (code, _, stderr) = run(&["--replace"], "{}", Some(&config));
    assert_eq!(code, 1);
    assert!(stderr.contains("relay requires explicit Monad testnet policy"));
}
