//! Process-level coverage for file-backed runtime secrets.

use std::io::Write as _;
use std::process::{Command, Output, Stdio};

fn run_with_stdin(command: &mut Command, input: &str) -> Output {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn command_with_data_dir(data_dir: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ai-memory"));
    command
        .arg("--data-dir")
        .arg(data_dir)
        .arg("generate-auth-token")
        .env_remove("AI_MEMORY_AUTH_TOKEN")
        .env_remove("AI_MEMORY_AUTH_TOKEN_FILE")
        .env_remove("AI_MEMORY_TOKEN_PEPPER_FILE");
    command
}

#[test]
fn process_secret_files_pass_through_the_single_config_loader() {
    let tmp = tempfile::tempdir().unwrap();
    let bearer_path = tmp.path().join("bearer");
    let pepper_path = tmp.path().join("pepper");
    std::fs::write(&bearer_path, "routine-token\n").unwrap();
    std::fs::write(&pepper_path, "pepper-value\n").unwrap();

    let output = command_with_data_dir(tmp.path())
        .env("AI_MEMORY_AUTH_TOKEN_FILE", &bearer_path)
        .env("AI_MEMORY_TOKEN_PEPPER_FILE", &pepper_path)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.trim().len(), 64);
    assert!(!stdout.contains("routine-token"));
    assert!(!stdout.contains("pepper-value"));
}

#[test]
fn process_rejects_direct_and_file_bearer_sources_together() {
    let tmp = tempfile::tempdir().unwrap();
    let bearer_path = tmp.path().join("bearer");
    std::fs::write(&bearer_path, "file-token\n").unwrap();

    let output = command_with_data_dir(tmp.path())
        .env("AI_MEMORY_AUTH_TOKEN", "direct-token")
        .env("AI_MEMORY_AUTH_TOKEN_FILE", &bearer_path)
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("supplied both directly and through a secret file"),
        "unexpected stderr: {stderr}"
    );
    assert!(!stderr.contains("direct-token"));
    assert!(!stderr.contains("file-token"));
}

#[test]
fn hook_shortcut_accepts_a_file_bearer_without_echoing_it() {
    let tmp = tempfile::tempdir().unwrap();
    let bearer_path = tmp.path().join("bearer");
    std::fs::write(&bearer_path, "routine-token\n").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ai-memory"));
    command
        .arg("--data-dir")
        .arg(tmp.path())
        .args([
            "hook",
            "--event",
            "user-prompt",
            "--agent",
            "claude-code",
            "--server-url",
            "http://127.0.0.1:1",
            "--check-capture",
        ])
        .env_remove("AI_MEMORY_AUTH_TOKEN")
        .env("AI_MEMORY_AUTH_TOKEN_FILE", &bearer_path);

    let output = run_with_stdin(&mut command, r#"{"prompt":"remember this"}"#);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("routine-token"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("routine-token"));
}

#[test]
fn hidden_drainer_accepts_its_bearer_over_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ai-memory"));
    command
        .arg("--data-dir")
        .arg(tmp.path())
        .args([
            "hook-drain",
            "--server-url",
            "http://127.0.0.1:1",
            "--auth-token-stdin",
        ])
        .env_remove("AI_MEMORY_AUTH_TOKEN")
        .env_remove("AI_MEMORY_AUTH_TOKEN_FILE");

    let output = run_with_stdin(&mut command, "runtime-secret\n");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("runtime-secret"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("runtime-secret"));
}
