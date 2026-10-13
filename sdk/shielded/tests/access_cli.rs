//! Installed-command protocol and redacted failure behavior without service access.

use std::{
    io::Write,
    process::{Command, Stdio},
};

use serde_json::{json, Value};

fn command(bytes: &[u8]) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_erebus-access"))
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn protocol_check_reads_no_keys_and_reports_snapshot_and_metered_methods() {
    let output = Command::new(env!("CARGO_BIN_EXE_erebus-access"))
        .env_clear()
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success() && output.stderr.is_empty());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        response,
        json!({"status":"ok","protocol":2,"methods":["retrieve","metered_call","metered_status"]})
    );
}

#[test]
fn malformed_oversized_and_private_fields_fail_without_echoing_input() {
    for bytes in [
        b"PRIVATE_INPUT_NOT_JSON".to_vec(),
        vec![b'X'; 16385],
        serde_json::to_vec(&json!({"private_key":"PRIVATE_INPUT_NEVER_ECHO"})).unwrap(),
    ] {
        let output = command(&bytes);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        let text = std::str::from_utf8(&output.stdout).unwrap();
        assert!(!text.contains("PRIVATE_INPUT"));
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], "error");
        assert_eq!(response["retry_without_payment"], true);
        assert!(response.get("resource_file").is_none());
    }
}

#[test]
fn metered_methods_validate_their_request_shape_before_any_key_or_service_use() {
    let root = tempfile::tempdir().unwrap();
    for request in [
        // Unknown method.
        json!({"method":"metered_other", "evidence_file":root.path().join("missing.evidence"), "buyer_key_file":root.path().join("PRIVATE_KEY_PATH"),
            "service_url":"http://127.0.0.1:1/v1/metered", "service_id":"ab".repeat(32), "cache_root":root.path().join("cache")}),
        // Snapshot retrieval never takes a call index.
        json!({"method":"retrieve", "call_index":0, "evidence_file":root.path().join("missing.evidence"), "buyer_key_file":root.path().join("PRIVATE_KEY_PATH"),
            "service_url":"https://example.invalid/v1/access", "service_id":"ab".repeat(32), "cache_root":root.path().join("cache")}),
        // Status never takes a call index.
        json!({"method":"metered_status", "call_index":0, "evidence_file":root.path().join("missing.evidence"), "buyer_key_file":root.path().join("PRIVATE_KEY_PATH"),
            "service_url":"http://127.0.0.1:1/v1/metered", "service_id":"ab".repeat(32), "cache_root":root.path().join("cache")}),
        // Metered methods never authorize a payment.
        json!({"method":"metered_call", "x402_exact":true, "evidence_file":root.path().join("missing.evidence"), "buyer_key_file":root.path().join("PRIVATE_KEY_PATH"),
            "service_url":"http://127.0.0.1:1/v1/metered", "service_id":"ab".repeat(32), "cache_root":root.path().join("cache")}),
        // A malformed service id is refused before any file is read.
        json!({"method":"metered_call", "evidence_file":root.path().join("missing.evidence"), "buyer_key_file":root.path().join("PRIVATE_KEY_PATH"),
            "service_url":"http://127.0.0.1:1/v1/metered", "service_id":"AB".repeat(32), "cache_root":root.path().join("cache")}),
    ] {
        let output = command(&serde_json::to_vec(&request).unwrap());
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        let text = std::str::from_utf8(&output.stdout).unwrap();
        assert!(!text.contains("PRIVATE_KEY_PATH") && !text.contains("example.invalid"));
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["status"], "error");
        assert_eq!(response["retry_without_payment"], true);
        assert!(response.get("payment_verified").is_none());
    }
    // The metered request shapes that pass validation reach the missing evidence file, which is
    // a local storage failure, not a payment result.
    let request = json!({"method":"metered_call", "call_index":0, "evidence_file":root.path().join("missing.evidence"), "buyer_key_file":root.path().join("PRIVATE_KEY_PATH"),
        "service_url":"http://127.0.0.1:1/v1/metered", "service_id":"ab".repeat(32), "cache_root":root.path().join("cache"), "allow_loopback_http":true});
    let output = command(&serde_json::to_vec(&request).unwrap());
    assert_eq!(output.status.code(), Some(1));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "error");
    assert_eq!(response["error_code"], "storage");
    assert_eq!(response["retry_without_payment"], true);
}

#[test]
fn unavailable_local_evidence_is_not_a_payment_or_delivery_result() {
    let root = tempfile::tempdir().unwrap();
    let request = json!({"method":"retrieve", "evidence_file":root.path().join("missing.evidence"), "buyer_key_file":root.path().join("PRIVATE_KEY_PATH"),
        "service_url":"https://example.invalid/v1/access", "service_id":"ab".repeat(32), "cache_root":root.path().join("cache")});
    let output = command(&serde_json::to_vec(&request).unwrap());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let text = std::str::from_utf8(&output.stdout).unwrap();
    assert!(!text.contains("PRIVATE_KEY_PATH") && !text.contains("example.invalid"));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "error");
    assert_eq!(response["retry_without_payment"], true);
    assert!(response.get("payment_verified").is_none());
    assert!(response.get("payload_hex").is_none());
}
