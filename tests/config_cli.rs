//! Integration tests for the CLI configuration surface.
//!
//! Each test spawns the compiled `stratus` binary with a config file and asserts the observable
//! behavior: exit code and error output. Value-level assertions (merge, precedence, defaults)
//! live as unit tests in `src/config/`, which inspect the loaded configuration directly.

use std::process::Command;
use std::process::Output;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

use tempfile::NamedTempFile;
use tempfile::TempDir;

/// Bounds each run: config errors exit before any service starts, so this only triggers when a
/// config expected to fail actually validates and boots a full node.
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);

/// Runs the stratus binary and returns its output once it exits within [`EXIT_TIMEOUT`].
fn run(mut command: Command) -> Output {
    let mut child = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + EXIT_TIMEOUT;
    while child.try_wait().unwrap().is_none() {
        assert!(
            Instant::now() < deadline,
            "stratus did not exit within {EXIT_TIMEOUT:?}: a config expected to fail may have booted a node"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    child.wait_with_output().unwrap()
}

/// Writes the content to a temporary config file and appends `--config <path>` to the arguments.
fn with_config(command: &mut Command, content: &str) -> NamedTempFile {
    let file = NamedTempFile::new().unwrap();
    std::fs::write(file.path(), content).unwrap();
    command.arg("--config").arg(file.path());
    file
}

/// Asserts that the binary rejects the configuration with the expected message.
fn assert_config_error(args: &[&str], content: &str, expected: &str) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stratus"));
    command.args(args);
    let config = with_config(&mut command, content);
    let output = run(command);
    drop(config);
    assert!(!output.status.success(), "expected the binary to fail, but it succeeded | args={args:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let output = format!("{stdout}{stderr}");
    assert!(
        output.contains(expected),
        "expected error message not found\n  expected: {expected}\n  output: {output}"
    );
}

/// Runs the binary until its stdout contains the expected text, then kills it; for configurations
/// that print what they have to assert and keep running.
fn expect_stdout(args: &[&str], content: &str, expected: &str) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stratus"));
    command.args(args);
    let config = with_config(&mut command, content);
    let mut child = command.stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::Read;
        let mut stdout = stdout;
        let mut buffer = [0u8; 1024];
        while let Ok(read) = stdout.read(&mut buffer) {
            if read == 0 || tx.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut seen = String::new();
    let deadline = Instant::now() + EXIT_TIMEOUT;
    while !seen.contains(expected) {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(chunk) => seen.push_str(&String::from_utf8_lossy(&chunk)),
            Err(_) => break, // timed out, or the process exited before printing the text
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    drop(config);
    assert!(
        seen.contains(expected),
        "expected text not found in stdout\n  expected: {expected}\n  output: {seen}"
    );
}

#[test]
fn test_config_file_not_found() {
    let dir = TempDir::new().unwrap();
    let missing_path = dir.path().join("missing.toml");
    let mut command = Command::new(env!("CARGO_BIN_EXE_stratus"));
    command.arg("--config").arg(&missing_path);
    let output = run(command);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("config file not found | path="),
        "expected error message not found\n  output: {stdout}"
    );
}

#[test]
fn test_malformed_toml_fails() {
    assert_config_error(&[], "leader =", "failed to parse config file");
}

#[test]
fn test_unknown_field_warns_but_does_not_fail() {
    // unknown fields are ignored with a warning instead of failing, so a file with stale or
    // unrecognized options does not prevent startup; the config below also lacks a node mode,
    // which makes the merged parse fail instead of booting a node
    let mut command = Command::new(env!("CARGO_BIN_EXE_stratus"));
    let config = with_config(
        &mut command,
        r#"
            unknown_field = 1

            [executor]
            chain_id = 2008
        "#,
    );
    let output = run(command);
    drop(config);
    assert!(!output.status.success(), "expected the binary to fail, but it succeeded");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("warning: unknown field in config file"),
        "expected unknown field warning in output\n  output: {stdout}"
    );
    assert!(
        stdout.contains("required arguments were not provided"),
        "expected validation failure for missing node mode\n  output: {stdout}"
    );
}

#[test]
fn test_no_node_mode_fails() {
    assert_config_error(
        &[],
        r#"
            [executor]
            chain_id = 2008
        "#,
        "required arguments were not provided",
    );
}

#[test]
fn test_missing_chain_id_fails() {
    // the chain id has no default: clap's own required-argument error surfaces
    assert_config_error(&[], "leader = true", "--executor-chain-id");
}

#[test]
fn test_leader_ignores_importer_config() {
    // #2567: a leader ignores `[importer]` with a warning instead of failing; the node keeps
    // running after the warning, so it is killed once the warning is seen
    let storage = TempDir::new().unwrap();
    let content = format!(
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [storage.permanent]
            path_prefix = "{}"

            [importer]
            external_rpc = "http://127.0.0.1:3000/"
        "#,
        storage.path().display()
    );
    expect_stdout(&[], &content, "warning: ignoring [importer] config in leader mode");
}

#[test]
fn test_leader_ignores_kafka_config() {
    // #2567: a leader ignores `[kafka]` with a warning instead of failing; the section is
    // incomplete on purpose, as a leader must tolerate it even though a follower would not
    let storage = TempDir::new().unwrap();
    let content = format!(
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [storage.permanent]
            path_prefix = "{}"

            [kafka]
            topic = "stratus-events"
        "#,
        storage.path().display()
    );
    expect_stdout(&[], &content, "warning: ignoring [kafka] config in leader mode");
}

#[test]
fn test_incomplete_kafka_fails() {
    // the all-or-none rule is enforced when the connector is created, after the node boots its
    // storage, so the storage goes to a temporary directory
    let storage = TempDir::new().unwrap();
    let content = format!(
        r#"
            follower = true

            [executor]
            chain_id = 2008

            [storage.permanent]
            path_prefix = "{}"
            file_descriptors_limit = 1024

            [importer]
            external_rpc = "http://127.0.0.1:3000/"

            [kafka]
            bootstrap_servers = "localhost:29092"
            client_id = "stratus-producer"
        "#,
        storage.path().display()
    );
    assert_config_error(&[], &content, "incomplete `[kafka]` configuration");
}

#[test]
fn test_empty_sentry_url_is_ignored() {
    // an empty sentry url disables the exporter with a warning instead of failing
    let storage = TempDir::new().unwrap();
    let content = format!(
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [storage.permanent]
            path_prefix = "{}"

            [common.sentry]
            url = ""
        "#,
        storage.path().display()
    );
    expect_stdout(&[], &content, "warning: ignoring [common.sentry] config: url is empty");
}

#[test]
fn test_cli_empty_sentry_url_is_ignored() {
    // the CLI-provided empty url disables the exporter the same way the file's does
    let storage = TempDir::new().unwrap();
    let content = format!(
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [storage.permanent]
            path_prefix = "{}"
        "#,
        storage.path().display()
    );
    expect_stdout(&["--sentry-url", ""], &content, "warning: ignoring [common.sentry] config: url is empty");
}

#[test]
fn test_invalid_tracing_filter_fails() {
    // file values go through the same clap value parsers as the command line, so an invalid
    // filter is rejected at the argument that received it
    assert_config_error(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [common.tracing]
            filter = "invalid=filter=here"
        "#,
        "invalid tracing filter \"invalid=filter=here\"",
    );
}

#[test]
fn test_invalid_block_mode_fails() {
    // file values go through the same clap value parsers as the command line
    assert_config_error(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [miner]
            block_mode = "not-a-mode"
        "#,
        "invalid value",
    );
}

#[test]
fn test_invalid_max_response_size_fails() {
    assert_config_error(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [rpc]
            max_response_size_bytes = 300
        "#,
        "must be at least",
    );
}

#[test]
fn test_cli_leader_flag_overrides_file_follower_mode() {
    // #2567: the CLI mode is authoritative over the file's, so `--leader` wins without a conflict
    // and the file's follower-only `[importer]` is ignored
    let storage = TempDir::new().unwrap();
    let content = format!(
        r#"
            follower = true

            [executor]
            chain_id = 2008

            [storage.permanent]
            path_prefix = "{}"

            [importer]
            external_rpc = "http://127.0.0.1:3000/"
        "#,
        storage.path().display()
    );
    expect_stdout(&["--leader"], &content, "warning: ignoring [importer] config in leader mode");
}

#[test]
fn test_follower_without_importer_fails() {
    assert_config_error(
        &[],
        r#"
            follower = true

            [executor]
            chain_id = 2008
        "#,
        "--external-rpc",
    );
}

#[test]
fn test_follower_with_empty_external_rpc_fails() {
    // an empty value is rejected by the value parser instead of being the "missing" sentinel
    assert_config_error(
        &[],
        r#"
            follower = true

            [executor]
            chain_id = 2008

            [importer]
            external_rpc = ""
        "#,
        "value cannot be empty",
    );
}

#[test]
fn test_cli_follower_flag_overrides_file_leader_mode() {
    // #2567: the reverse direction: `--follower` is authoritative over the file's `leader`, and
    // follower mode then fails the importer requirement, which a leader would not
    assert_config_error(
        &["--follower"],
        r#"
            leader = true

            [executor]
            chain_id = 2008
        "#,
        "--external-rpc",
    );
}

#[test]
fn test_default_config_file_falls_back_to_defaults() {
    // without --config and without a default file next to the process, defaults are used and
    // validation fails later, proving the missing default file is not an error
    let dir = TempDir::new().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_stratus"));
    command.current_dir(dir.path());
    let output = run(command);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("config file not found, using defaults"),
        "expected fallback to defaults\n  output: {stdout}"
    );
    assert!(
        stdout.contains("required arguments were not provided"),
        "expected default config to fail validation\n  output: {stdout}"
    );
}

#[test]
fn test_help_exits_successfully() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_stratus"));
    command.arg("--help");
    let output = run(command);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--config"), "expected --config in the help output\n  output: {stdout}");
}
