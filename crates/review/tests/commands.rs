#![expect(
    clippy::unwrap_used,
    clippy::disallowed_methods,
    reason = "a test harness fails by panicking and runs the binary"
)]

use std::fs;
use std::path::Path;
use std::process::Command;

fn commands() -> Vec<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_review"))
        .arg("help")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    help.lines()
        .skip_while(|line| !line.starts_with("Commands:"))
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| *name != "help")
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_command_runs_in_a_cli_scenario() {
    let scenarios =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cli.rs")).unwrap();
    let listed = commands();
    assert!(listed.len() > 10, "help listed {listed:?}");
    let missing: Vec<&String> = listed
        .iter()
        .filter(|name| !scenarios.contains(&format!("&[\"{name}\"")))
        .collect();
    assert!(
        missing.is_empty(),
        "commands no scenario in tests/cli.rs runs: {missing:?}"
    );
}
