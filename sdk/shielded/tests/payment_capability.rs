//! `erebus-payment` capability replies, both invocation forms.

use std::{
    io::Write,
    process::{Command, Stdio},
};

fn run(args: &[&str], input: Option<&str>) -> (i32, serde_json::Value, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_erebus-payment"));
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    if let Some(input) = input {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    let text = String::from_utf8(output.stdout.clone()).unwrap();
    let value = serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null);
    (output.status.code().unwrap_or(-1), value, text)
}

#[test]
fn version_replies_advertise_sponsored_relay_for_stdin_and_flag_forms() {
    let (code, stdin, _) = run(&[], Some(r#"{"method":"version"}"#));
    assert_eq!(code, 0);
    assert_eq!(stdin["status"], "ok");
    assert_eq!(stdin["protocol_version"], 1);
    assert_eq!(stdin["service"], "erebus-payment");
    assert_eq!(
        stdin["modes"],
        serde_json::json!(["public-bound", "shielded"])
    );
    assert_eq!(stdin["automatic_rebroadcast"], false);
    assert_eq!(
        stdin["sponsored_relay"], true,
        "the relay block is supported by this binary"
    );

    let (code, flag, _) = run(&["--version"], None);
    assert_eq!(code, 0);
    assert_eq!(flag, stdin, "both invocation forms must agree");
}

#[test]
fn version_flag_does_not_read_stdin_or_echo_private_inputs() {
    let (code, flag, text) = run(&["--version"], Some("PRIVATE_KEY_NEVER_ECHO"));
    assert_eq!(code, 0);
    assert!(!flag.to_string().contains("PRIVATE_"));
    assert!(!text.contains("PRIVATE_"));

    let (code, _, help) = run(&["--help"], None);
    assert_eq!(code, 0);
    assert!(help.contains("Methods: version, funding, settle, observe"));
}
