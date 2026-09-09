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
fn test_unknown_field_fails() {
    assert_config_error(
        &[],
        r#"
            leader = true
            unknown_field = 1

            [executor]
            chain_id = 2008
        "#,
        "unknown field `unknown_field`",
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
        "no node mode configured",
    );
}

#[test]
fn test_missing_chain_id_fails() {
    assert_config_error(&[], "leader = true", "`executor.chain_id` is required");
}

#[test]
fn test_leader_with_importer_fails() {
    assert_config_error(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [importer]
            external_rpc = "http://127.0.0.1:3000/"
        "#,
        "leader mode cannot be used with `[importer]`",
    );
}

#[test]
fn test_leader_with_kafka_fails() {
    assert_config_error(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [kafka]
            bootstrap_servers = "localhost:29092"
            topic = "stratus-events"
            client_id = "stratus-producer"
        "#,
        "`[kafka]` configuration requires follower or fake-leader mode",
    );
}

#[test]
fn test_incomplete_kafka_fails() {
    assert_config_error(
        &[],
        r#"
            follower = true

            [executor]
            chain_id = 2008

            [importer]
            external_rpc = "http://127.0.0.1:3000/"

            [kafka]
            bootstrap_servers = "localhost:29092"
            client_id = "stratus-producer"
        "#,
        "incomplete `[kafka]` configuration",
    );
}

#[test]
fn test_empty_sentry_url_fails() {
    assert_config_error(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [common.sentry]
            url = ""
        "#,
        "`[sentry]` configuration requires a non-empty `url`",
    );
}

#[test]
fn test_invalid_tracing_filter_fails() {
    assert_config_error(
        &[],
        r#"
            leader = true

            [executor]
            chain_id = 2008

            [common.tracing]
            filter = "invalid=filter=here"
        "#,
        "`[common.tracing] filter` is invalid",
    );
}

#[test]
fn test_cli_leader_flag_merges_with_file_follower_mode() {
    // the explicit `--leader` is merged over the file's `follower` instead of replacing it,
    // so both modes end up set and validation reports the conflict
    assert_config_error(
        &["--leader"],
        r#"
            follower = true

            [executor]
            chain_id = 2008

            [importer]
            external_rpc = "http://127.0.0.1:3000/"
        "#,
        "multiple node modes configured",
    );
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
        "follower and fake-leader modes require `[importer]` configuration",
    );
}

#[test]
fn test_follower_with_empty_external_rpc_fails() {
    assert_config_error(
        &[],
        r#"
            follower = true

            [executor]
            chain_id = 2008

            [importer]
            external_rpc = ""
        "#,
        "`importer.external_rpc` is required for follower and fake-leader modes",
    );
}

#[test]
fn test_cli_follower_flag_merges_with_file_leader_mode() {
    // the reverse direction of the leader test: the file's mode and the CLI's mode end up merged
    assert_config_error(
        &["--follower"],
        r#"
            leader = true

            [executor]
            chain_id = 2008
        "#,
        "multiple node modes configured",
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
        stdout.contains("no node mode configured"),
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
