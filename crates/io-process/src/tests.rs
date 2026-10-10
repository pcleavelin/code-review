use std::env;
use std::io::{Read, Write};

use std::path::PathBuf;

use crate::{Argument, Captured, Process, ProcessError, Program, plain};

fn arguments(list: &[&str]) -> Vec<Argument> {
    list.iter()
        .map(|argument| Argument::new(argument))
        .collect()
}

#[test]
fn run_captures_both_streams_and_the_exit_status() {
    let output = Process::run(
        &Program::new("sh"),
        &arguments(&["-c", "echo out; echo err >&2; exit 3"]),
        &env::temp_dir(),
    )
    .unwrap();
    assert!(!output.success());
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(output.stdout.text(), "out\n");
    assert_eq!(output.stderr.as_bytes(), b"err\n");
}

#[cfg(unix)]
#[test]
fn run_starts_in_the_given_directory() {
    let directory = env::temp_dir().canonicalize().unwrap();
    let output = Process::run(&Program::new("pwd"), &[], &directory).unwrap();
    assert!(output.success());
    assert_eq!(output.stdout.text().trim_end(), directory.to_str().unwrap());
}

#[test]
fn a_program_missing_from_path_is_named_in_the_error() {
    let missing = Program::new("review-no-such-program");
    assert!(missing.find_on_path().is_none());
    match Process::run(&missing, &[], &env::temp_dir()) {
        Err(ProcessError::Missing(program)) => assert_eq!(program, missing),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_program_given_by_path_is_used_as_is() {
    let found = Program::new("sh").find_on_path().unwrap();
    let direct = Program::new(found.to_str().unwrap());
    assert_eq!(direct.find_on_path(), Some(found));
}

#[test]
fn run_with_input_feeds_stdin_and_waits() {
    let output = Process::run_with_input(
        &Program::new("cat"),
        &[],
        &env::temp_dir(),
        &Captured::of(b"token"),
    )
    .unwrap();
    assert!(output.success());
    assert_eq!(output.stdout.as_bytes(), b"token");
}

#[test]
fn spawn_pipes_input_to_output() {
    let mut spawned = Process::spawn(&Program::new("cat"), &[], &env::temp_dir()).unwrap();
    spawned.input.write_all(b"hello").unwrap();
    drop(spawned.input);
    let mut echoed = String::new();
    spawned.output.read_to_string(&mut echoed).unwrap();
    assert_eq!(echoed, "hello");
    assert!(spawned.child.wait().unwrap().success());
}

#[test]
fn a_verbatim_drive_path_loses_its_prefix_and_other_paths_stay() {
    assert_eq!(
        plain(PathBuf::from(r"\\?\C:\repo")),
        PathBuf::from(r"C:\repo")
    );
    assert_eq!(
        plain(PathBuf::from(r"\\?\UNC\server\share")),
        PathBuf::from(r"\\?\UNC\server\share")
    );
    assert_eq!(plain(PathBuf::from("/home/me")), PathBuf::from("/home/me"));
}
