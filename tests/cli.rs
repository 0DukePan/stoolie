//! End-to-end tests: run the real binary and check what a user would see.
//!
//! These are deliberately shallow — they assert on observable behaviour (exit
//! codes, JSON shape, the promise that the receipt is self-contained) rather
//! than on internals, so a refactor does not have to rewrite them.

use std::path::PathBuf;
use std::process::Command;

fn stoolie() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stoolie"));
    command.env("NO_COLOR", "1");
    command
}

fn output(args: &[&str]) -> (String, String, i32) {
    let result = stoolie()
        .args(args)
        .output()
        .expect("failed to run stoolie");
    (
        String::from_utf8_lossy(&result.stdout).to_string(),
        String::from_utf8_lossy(&result.stderr).to_string(),
        result.status.code().unwrap_or(-1),
    )
}

fn temp_dir() -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("stoolie-test-{stamp}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn version_is_reported() {
    let (stdout, _, code) = output(&["--version"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("stoolie"), "got: {stdout}");
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_lists_every_command() {
    let (stdout, _, code) = output(&["--help"]);
    assert_eq!(code, 0);
    for command in ["receipt", "json", "watch", "doctor"] {
        assert!(stdout.contains(command), "help is missing {command}");
    }
}

#[test]
fn json_is_valid_and_has_the_documented_shape() {
    let (stdout, stderr, code) = output(&["json"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let value: serde_json::Value = serde_json::from_str(&stdout).expect("json output must parse");
    for key in [
        "version",
        "generated",
        "days",
        "os",
        "sessions",
        "events",
        "timeline",
        "archive_sessions",
    ] {
        assert!(value.get(key).is_some(), "json is missing {key}");
    }
    assert!(value["sessions"].is_array());
    assert!(value["events"].is_array());
    assert_eq!(value["days"], 13);
}

#[test]
fn compact_json_has_no_newlines() {
    let (stdout, _, code) = output(&["json", "--compact"]);
    assert_eq!(code, 0);
    let body = stdout.trim_end();
    assert!(!body.contains('\n'), "compact json must be one line");
    assert!(serde_json::from_str::<serde_json::Value>(body).is_ok());
}

#[test]
fn since_changes_the_window() {
    let (stdout, _, code) = output(&["json", "--compact", "--since", "3"]);
    assert_eq!(code, 0);
    let value: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(value["days"], 3);
}

#[test]
fn receipt_is_written_and_self_contained() {
    let directory = temp_dir();
    let path = directory.join("receipt.html");
    let (stdout, stderr, code) =
        output(&["receipt", "--out", path.to_str().expect("utf-8 temp path")]);
    assert_eq!(code, 0, "stderr: {stderr}\nstdout: {stdout}");
    assert!(path.exists(), "receipt was not written");

    let html = std::fs::read_to_string(&path).unwrap();
    assert!(html.contains("SENSOR RECEIPT"));
    assert!(html.contains("0 bytes sent"));

    // A receipt that phones home is not a receipt.
    assert!(!html.contains("<script"), "receipt must not run javascript");
    assert!(
        !html.contains("http://") && !html.contains("https://"),
        "receipt must not reference a remote resource"
    );

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn network_audit_measures_zero() {
    let (stdout, _, code) = output(&["--network"]);
    assert_eq!(code, 0);
    assert!(
        stdout.contains("0 outbound connections"),
        "the binary must own no sockets, got: {stdout}"
    );
}

#[test]
fn doctor_reports_each_source() {
    let (stdout, _, code) = output(&["doctor"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("consent store"));
    assert!(stdout.contains("privacy audit"));
    assert!(stdout.contains("timeline"));
    assert!(stdout.contains("archive"));
    assert!(stdout.contains("network code      none"));
}

#[test]
fn summary_honours_no_color() {
    let (stdout, _, code) = output(&["--no-color"]);
    assert_eq!(code, 0);
    assert!(!stdout.contains('\u{1b}'), "NO_COLOR output must be plain");
    assert!(stdout.contains("SENSOR RECEIPT"));
    assert!(stdout.contains("stoolie receipt"));
}

#[test]
fn unknown_command_fails_loudly() {
    let (_, _, code) = output(&["definitely-not-a-command"]);
    assert_ne!(code, 0, "an unknown subcommand must not exit 0");
}
