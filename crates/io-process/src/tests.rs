use std::env;
use std::io::{Read, Write};

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::{Argument, Captured, Deadline, Process, ProcessError, Program, plain};

const TEST_DEADLINE: Deadline = Deadline::seconds(60);

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
        TEST_DEADLINE,
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
    let output = Process::run(&Program::new("pwd"), &[], &directory, TEST_DEADLINE).unwrap();
    assert!(output.success());
    assert_eq!(output.stdout.text().trim_end(), directory.to_str().unwrap());
}

#[test]
fn a_program_missing_from_path_is_named_in_the_error() {
    let missing = Program::new("review-no-such-program");
    assert!(missing.find_on_path().is_none());
    match Process::run(&missing, &[], &env::temp_dir(), TEST_DEADLINE) {
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
        TEST_DEADLINE,
    )
    .unwrap();
    assert!(output.success());
    assert_eq!(output.stdout.as_bytes(), b"token");
}

#[test]
fn run_with_input_reads_the_output_while_it_writes_input_larger_than_a_pipe() {
    let input = Captured::of(&vec![b'x'; 4 << 20]);
    let output = Process::run_with_input(
        &Program::new("cat"),
        &[],
        &env::temp_dir(),
        &input,
        TEST_DEADLINE,
    )
    .unwrap();
    assert_eq!(output.stdout, input);
}

#[cfg(unix)]
#[test]
fn a_program_that_outlives_its_deadline_is_stopped_and_named() {
    let started = Instant::now();
    let deadline = Deadline::seconds(1);
    let stopped = Process::run(
        &Program::new("sleep"),
        &arguments(&["600"]),
        &env::temp_dir(),
        deadline,
    );
    match stopped {
        Err(ProcessError::TimedOut {
            program,
            leading,
            deadline: waited,
        }) => {
            assert_eq!(program, Program::new("sleep"));
            assert_eq!(leading, arguments(&["600"]));
            assert_eq!(waited, deadline);
        }
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[cfg(unix)]
#[test]
fn a_program_fed_input_that_outlives_its_deadline_is_stopped() {
    let started = Instant::now();
    let stopped = Process::run_with_input(
        &Program::new("sh"),
        &arguments(&["-c", "cat; exec sleep 600"]),
        &env::temp_dir(),
        &Captured::of(b"fed"),
        Deadline::seconds(1),
    );
    assert!(
        matches!(stopped, Err(ProcessError::TimedOut { .. })),
        "{stopped:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(30));
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
