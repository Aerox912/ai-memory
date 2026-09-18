//! Use-case end-to-end test for `ai-memory doctor` (capture-coverage check).
//!
//! Doctor exists to make one silent failure visible: a harness that ran in this
//! project but has no ai-memory hook installed still writes its own local
//! session transcripts, yet nothing reaches the server. The operator believes
//! every harness feeds one memory and is quietly wrong (audit finding F1). For
//! the current project doctor enumerates each known harness's local native
//! sessions, asks the server how many it actually captured per agent
//! (`GET /admin/sessions/by-agent`), and warns — with the exact
//! `install-hooks --agent <agent> --apply` remediation — about any harness that
//! ran here recently but captured zero.
//!
//! The pure verdict logic (`build_rows`) and the local detector (`scan_local`)
//! are unit-tested in the command module. The UNTESTED half is the whole live
//! flow against a real server: does the shipped `doctor` subcommand, driven
//! exactly as an operator would, actually reconcile the local side against the
//! server's captured counts and print the right verdict? This drives that flow:
//!
//! 1. **Gap detected.** A planted Claude transcript for this cwd with an empty
//!    store (server captured zero) → doctor flags `claude-code` as uncaptured
//!    and prints its `install-hooks --agent claude-code --apply` fix.
//! 2. **Healthy control.** After `backfill` imports that same transcript through
//!    `/hook`, the server has captured sessions for `claude-code` → doctor no
//!    longer flags it, and reports every harness as captured. This control is
//!    what stops a blanket "everything is uncaptured" warning from passing #1
//!    for the wrong reason.
//!
//! Unix-gated: the fixture uses the POSIX-encoded `~/.claude/projects/<enc-cwd>/`
//! native layout (as the sibling `backfill_e2e` test and the `scan_local` unit
//! test do); cross-platform native-store discovery is owned by
//! `ai-memory-workstream`.

#![cfg(unix)]

/// Spawns a server and several subprocesses: seconds, not milliseconds, so this
/// lives in the slow tier (`cargo tf` / CI), not the everyday loop.
mod slow {
    use std::fs;
    use std::net::TcpListener;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    const BIN: &str = env!("CARGO_BIN_EXE_ai-memory");
    const WORKSPACE: &str = "doctor-e2e-ws";
    const PROJECT: &str = "doctor-e2e-proj";

    /// Start from a clean, hermetic environment: drop every ambient
    /// `AI_MEMORY_*` var (a developer box or this project's own MCP config may
    /// export `AI_MEMORY_AUTH_TOKEN`, `AI_MEMORY_SERVER_URL`, scope names, …)
    /// so the child sees only what this test sets. Without this the spawned
    /// server would inherit an auth token and reject the test's own requests.
    fn hermetic(program: &str) -> Command {
        let mut cmd = Command::new(program);
        for (key, _) in std::env::vars_os() {
            if key.to_string_lossy().starts_with("AI_MEMORY_") {
                cmd.env_remove(key);
            }
        }
        cmd
    }

    /// Kill the spawned server when the test ends, pass or fail.
    struct ServerGuard(Child);
    impl Drop for ServerGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// A free loopback port. The brief unbind→rebind race is acceptable for a
    /// slow-tier test and is the same approach the shell smoke test uses.
    fn free_port() -> u16 {
        TcpListener::bind("127.0.0.1:0")
            .expect("bind ephemeral port")
            .local_addr()
            .expect("local addr")
            .port()
    }

    /// Write the JSONL lines (each already a `Value`) as one transcript file.
    fn write_jsonl(path: &Path, lines: &[Value]) {
        let mut body = String::new();
        for line in lines {
            body.push_str(&line.to_string());
            body.push('\n');
        }
        fs::write(path, body).expect("write transcript");
    }

    /// Sum the server's per-agent session counts for the scope. A 404 (the
    /// no-create scope lookup for a project that has never been written to)
    /// counts as zero — the pre-import state.
    async fn session_count(client: &reqwest::Client, base: &str) -> u64 {
        let resp = client
            .get(format!("{base}/admin/sessions/by-agent"))
            .query(&[("workspace", WORKSPACE), ("project", PROJECT)])
            .send()
            .await
            .expect("by-agent request");
        if !resp.status().is_success() {
            return 0;
        }
        let body: Value = resp.json().await.expect("by-agent json");
        body["by_agent"]
            .as_array()
            .map(|agents| {
                agents
                    .iter()
                    .filter_map(|a| a["sessions"].as_u64())
                    .sum::<u64>()
            })
            .unwrap_or(0)
    }

    /// Run a subcommand of the built binary to completion, returning its stdout.
    /// The scope/server/home environment is shared with the spawned server so
    /// the client talks to the same store the way a real install does.
    fn run_cli(args: &[&str], data_dir: &Path, home: &Path, cwd: &Path, base: &str) -> String {
        let out = hermetic(BIN)
            .args(args)
            .current_dir(cwd)
            .env("AI_MEMORY_DATA_DIR", data_dir)
            .env("AI_MEMORY_HOME", home)
            .env("AI_MEMORY_SERVER_URL", base)
            .env("AI_MEMORY_EMBEDDING_PROVIDER", "none")
            .env("RUST_LOG", "off")
            .output()
            .unwrap_or_else(|e| panic!("spawn `{}`: {e}", args.join(" ")));
        assert!(
            out.status.success(),
            "`{}` failed: {}\nstdout: {}\nstderr: {}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
        String::from_utf8(out.stdout).expect("stdout utf8")
    }

    /// The row for one agent kind in a `doctor --json` report, or `None`.
    fn find_row<'a>(report: &'a Value, agent: &str) -> Option<&'a Value> {
        report["rows"]
            .as_array()?
            .iter()
            .find(|r| r["agent"] == agent)
    }

    #[tokio::test]
    async fn doctor_flags_an_uncaptured_harness_then_clears_it_after_backfill() {
        let data_dir = tempfile::tempdir().expect("data dir");
        let home = tempfile::tempdir().expect("home");
        let project = tempfile::tempdir().expect("project cwd");
        // The child's `std::env::current_dir()` returns the canonical path, and
        // the native-session discovery matches the transcript's `cwd` header
        // against it — so plant and encode with the same canonical path.
        let cwd = fs::canonicalize(project.path()).expect("canonicalize project cwd");

        // Plant a Claude transcript for this cwd under the native layout. This
        // makes `claude-code` a harness that "ran here recently" for doctor,
        // while the server starts with zero captured — the F1 gap.
        let session_id = "11111111-2222-3333-4444-555555555555";
        let encoded = cwd.to_string_lossy().replace('/', "-");
        let session_dir = home.path().join(".claude").join("projects").join(encoded);
        fs::create_dir_all(&session_dir).expect("session dir");
        write_jsonl(
            &session_dir.join(format!("{session_id}.jsonl")),
            &[
                json!({ "sessionId": session_id, "cwd": cwd.to_string_lossy() }),
                json!({
                    "type": "user",
                    "message": {
                        "role": "user",
                        "content": [{ "type": "text", "text": "Record the doctor-e2e decision." }],
                    },
                }),
                json!({
                    "type": "assistant",
                    "message": {
                        "role": "assistant",
                        "content": [{ "type": "text", "text": "Acknowledged the doctor-e2e plan." }],
                    },
                }),
            ],
        );

        // Start the real server (loopback, no auth, hermetic: no embedder, no
        // wiki watcher — a machine-global inotify instance concurrent server
        // children can exhaust, #745).
        let port = free_port();
        let base = format!("http://127.0.0.1:{port}");
        let server = ServerGuard(
            hermetic(BIN)
                .args([
                    "serve",
                    "--transport",
                    "http",
                    "--bind",
                    &format!("127.0.0.1:{port}"),
                    "--workspace",
                    WORKSPACE,
                    "--project",
                    PROJECT,
                    "--no-watcher",
                ])
                .current_dir(&cwd)
                .env("AI_MEMORY_DATA_DIR", data_dir.path())
                .env("AI_MEMORY_HOME", home.path())
                .env("AI_MEMORY_EMBEDDING_PROVIDER", "none")
                .env("RUST_LOG", "off")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn serve"),
        );

        let client = reqwest::Client::new();

        // Wait for the server to accept connections.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if client
                .get(format!("{base}/mcp"))
                .timeout(Duration::from_secs(2))
                .send()
                .await
                .is_ok()
            {
                break;
            }
            assert!(Instant::now() < deadline, "server never became reachable");
            tokio::time::sleep(Duration::from_millis(150)).await;
        }

        // Precondition: nothing captured yet for this scope.
        assert_eq!(
            session_count(&client, &base).await,
            0,
            "a brand-new project store must start with no captured sessions",
        );

        // Phase 1 — gap detected. `--since-days 0` makes every on-disk session
        // count as recent, so the planted transcript is unambiguously "recent".
        let report: Value = serde_json::from_str(&run_cli(
            &[
                "doctor",
                "--workspace",
                WORKSPACE,
                "--project",
                PROJECT,
                "--since-days",
                "0",
                "--json",
            ],
            data_dir.path(),
            home.path(),
            &cwd,
            &base,
        ))
        .expect("doctor --json report");

        let claude = find_row(&report, "claude-code")
            .unwrap_or_else(|| panic!("expected a claude-code row: {report}"));
        assert_eq!(
            claude["uncaptured"], true,
            "a harness that ran here with zero captured must be flagged: {report}"
        );
        assert!(
            claude["local_recent"].as_u64().unwrap_or(0) >= 1,
            "the planted session must count as recent: {report}"
        );
        assert_eq!(
            claude["captured"], 0,
            "the server captured nothing for it yet: {report}"
        );
        assert_eq!(
            report["uncaptured"]
                .as_array()
                .map(|a| a.iter().any(|v| v == "claude-code")),
            Some(true),
            "claude-code must appear in the top-level uncaptured list: {report}"
        );

        // Phase 1 — the human-readable output prints the exact remediation the
        // operator is meant to run, naming the specific agent.
        let human = run_cli(
            &[
                "doctor",
                "--workspace",
                WORKSPACE,
                "--project",
                PROJECT,
                "--since-days",
                "0",
            ],
            data_dir.path(),
            home.path(),
            &cwd,
            &base,
        );
        assert!(
            human.contains("ai-memory install-hooks --agent claude-code --apply"),
            "doctor must print the exact install-hooks remediation: {human}"
        );
        assert!(
            human.contains("ran here but nothing was captured"),
            "doctor must explain the gap in prose: {human}"
        );

        // Import the local history through the same `/hook` ingress live capture
        // uses, so the server now has captured sessions for claude-code.
        let backfill: Value = serde_json::from_str(&run_cli(
            &[
                "backfill",
                "--workspace",
                WORKSPACE,
                "--project",
                PROJECT,
                "--json",
            ],
            data_dir.path(),
            home.path(),
            &cwd,
            &base,
        ))
        .expect("backfill --json report");
        assert_eq!(
            backfill["imported_sessions"], 1,
            "backfill must import the one planted session: {backfill}"
        );

        // The captured count is now nonzero (poll briefly for commit lag).
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if session_count(&client, &base).await >= 1 {
                break;
            }
            assert!(Instant::now() < deadline, "captured session never appeared");
            tokio::time::sleep(Duration::from_millis(150)).await;
        }

        // Phase 2 — healthy control. The same harness with the same local
        // transcript is now captured, so doctor must NOT flag it.
        let report2: Value = serde_json::from_str(&run_cli(
            &[
                "doctor",
                "--workspace",
                WORKSPACE,
                "--project",
                PROJECT,
                "--since-days",
                "0",
                "--json",
            ],
            data_dir.path(),
            home.path(),
            &cwd,
            &base,
        ))
        .expect("second doctor --json report");

        let claude2 = find_row(&report2, "claude-code")
            .unwrap_or_else(|| panic!("expected a claude-code row after backfill: {report2}"));
        assert_eq!(
            claude2["uncaptured"], false,
            "one captured session must clear the flag: {report2}"
        );
        assert!(
            claude2["captured"].as_u64().unwrap_or(0) >= 1,
            "the backfilled session must be counted as captured: {report2}"
        );
        assert_eq!(
            report2["uncaptured"]
                .as_array()
                .map(|a| a.iter().any(|v| v == "claude-code")),
            Some(false),
            "claude-code must no longer be in the uncaptured list: {report2}"
        );

        // Phase 2 — the human output no longer nags for claude-code's hook.
        let human2 = run_cli(
            &[
                "doctor",
                "--workspace",
                WORKSPACE,
                "--project",
                PROJECT,
                "--since-days",
                "0",
            ],
            data_dir.path(),
            home.path(),
            &cwd,
            &base,
        );
        assert!(
            !human2.contains("install-hooks --agent claude-code --apply"),
            "a captured harness must not get an install-hooks nag: {human2}"
        );

        drop(server);
    }
}
