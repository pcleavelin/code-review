use std::env;
use std::fmt;
use std::io::{self, Read, Write};
use std::iter;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prefix(&'static str);

const VERBATIM: Prefix = Prefix(r"\\?\");

pub fn plain(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(VERBATIM.0) {
        Some(rest) if rest.chars().nth(1) == Some(':') => PathBuf::from(rest),
        _ => path,
    }
}

pub fn temp_directory() -> PathBuf {
    env::temp_dir()
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Program(String);

impl Program {
    pub fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn find_on_path(&self) -> Option<PathBuf> {
        let named = Path::new(&self.0);
        if named.components().count() > 1 {
            return named.is_file().then(|| named.to_path_buf());
        }
        let suffixes = Self::executable_suffixes();
        env::split_paths(&env::var_os("PATH")?).find_map(|directory| {
            iter::once(String::new())
                .chain(suffixes.iter().cloned())
                .map(|suffix| directory.join(format!("{}{suffix}", self.0)))
                .find(|candidate| candidate.is_file())
        })
    }

    fn executable_suffixes() -> Vec<String> {
        if cfg!(windows) {
            env::var("PATHEXT")
                .unwrap_or_else(|_| ".EXE;.CMD;.BAT".to_owned())
                .split(';')
                .map(str::to_lowercase)
                .collect()
        } else {
            Vec::new()
        }
    }
}

impl fmt::Display for Program {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Argument(String);

impl Argument {
    pub fn new(argument: &str) -> Self {
        Self(argument.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Argument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Captured(Vec<u8>);

impl Captured {
    pub fn of(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0).into_owned()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Captured,
    pub stderr: Captured,
}

impl Output {
    pub fn success(&self) -> bool {
        self.status.success()
    }
}

#[derive(Debug)]
pub struct Spawned {
    pub child: Child,
    pub input: ChildStdin,
    pub output: ChildStdout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Deadline(Duration);

impl Deadline {
    pub const fn seconds(count: u64) -> Self {
        Self(Duration::from_secs(count))
    }

    pub const fn whole_seconds(self) -> u64 {
        self.0.as_secs()
    }
}

impl fmt::Display for Deadline {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} s", self.0.as_secs())
    }
}

#[derive(Clone, Copy, Debug)]
struct Leading(usize);

const NAMED_ARGUMENTS: Leading = Leading(2);

#[derive(Debug)]
pub enum ProcessError {
    Missing(Program),
    Start {
        program: Program,
        error: io::Error,
    },
    NoPipe(Program),
    TimedOut {
        program: Program,
        leading: Vec<Argument>,
        deadline: Deadline,
    },
}

impl fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(program) => write!(formatter, "{program} is not on PATH: install it"),
            Self::Start { program, error } => write!(formatter, "cannot run {program}: {error}"),
            Self::NoPipe(program) => write!(formatter, "cannot write to {program}"),
            Self::TimedOut {
                program,
                leading,
                deadline,
            } => {
                write!(formatter, "{program}")?;
                for argument in leading {
                    write!(formatter, " {argument}")?;
                }
                write!(
                    formatter,
                    " did not finish within {deadline} and was stopped"
                )
            }
        }
    }
}

enum Stream {
    Stdin(io::Result<()>),
    Stdout(io::Result<Captured>),
    Stderr(io::Result<Captured>),
}

fn read_all(mut pipe: impl Read) -> io::Result<Captured> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes).map(|_| Captured(bytes))
}

fn abandon(mut child: Child) {
    drop(child.kill());
    drop(child.wait());
}

#[expect(
    clippy::disallowed_methods,
    reason = "io-process feeds and drains a program's pipes on threads of its own, left behind when the program outlives its deadline"
)]
fn detached(body: impl FnOnce() + Send + 'static) {
    drop(thread::spawn(body));
}

fn exchange(
    program: &Program,
    arguments: &[Argument],
    directory: &Path,
    input: Option<&Captured>,
    deadline: Deadline,
) -> Result<Output, ProcessError> {
    let failed = |error| ProcessError::Start {
        program: program.clone(),
        error,
    };
    let ends = Instant::now() + deadline.0;
    let mut child = command(program, arguments, directory)?
        .stdin(input.map_or_else(Stdio::null, |_| Stdio::piped()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(failed)?;
    let (sender, streams) = mpsc::channel();
    let mut awaited = 2;
    if let Some(input) = input {
        let Some(mut stdin) = child.stdin.take() else {
            abandon(child);
            return Err(ProcessError::NoPipe(program.clone()));
        };
        let bytes = input.0.clone();
        let fed = sender.clone();
        detached(move || {
            let written = stdin.write_all(&bytes);
            drop(stdin);
            drop(fed.send(Stream::Stdin(written)));
        });
        awaited += 1;
    }
    let (Some(out_pipe), Some(err_pipe)) = (child.stdout.take(), child.stderr.take()) else {
        abandon(child);
        return Err(ProcessError::NoPipe(program.clone()));
    };
    let out = sender.clone();
    detached(move || drop(out.send(Stream::Stdout(read_all(out_pipe)))));
    detached(move || drop(sender.send(Stream::Stderr(read_all(err_pipe)))));
    let mut written = Ok(());
    let (mut stdout, mut stderr) = (Captured::default(), Captured::default());
    for _ in 0..awaited {
        match streams.recv_timeout(ends.saturating_duration_since(Instant::now())) {
            Ok(Stream::Stdin(result)) => written = result,
            Ok(Stream::Stdout(result)) => stdout = result.map_err(failed)?,
            Ok(Stream::Stderr(result)) => stderr = result.map_err(failed)?,
            Err(RecvTimeoutError::Timeout) => {
                abandon(child);
                return Err(ProcessError::TimedOut {
                    program: program.clone(),
                    leading: arguments.iter().take(NAMED_ARGUMENTS.0).cloned().collect(),
                    deadline,
                });
            }
            Err(RecvTimeoutError::Disconnected) => {
                abandon(child);
                return Err(ProcessError::NoPipe(program.clone()));
            }
        }
    }
    let status = child.wait().map_err(failed)?;
    written.map_err(failed)?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct Process;

impl Process {
    pub fn run(
        program: &Program,
        arguments: &[Argument],
        directory: &Path,
        deadline: Deadline,
    ) -> Result<Output, ProcessError> {
        exchange(program, arguments, directory, None, deadline)
    }

    pub fn run_with_input(
        program: &Program,
        arguments: &[Argument],
        directory: &Path,
        input: &Captured,
        deadline: Deadline,
    ) -> Result<Output, ProcessError> {
        exchange(program, arguments, directory, Some(input), deadline)
    }

    pub fn spawn(
        program: &Program,
        arguments: &[Argument],
        directory: &Path,
    ) -> Result<Spawned, ProcessError> {
        let mut child = command(program, arguments, directory)?
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| ProcessError::Start {
                program: program.clone(),
                error,
            })?;
        let pipes = child.stdin.take().zip(child.stdout.take());
        let Some((input, output)) = pipes else {
            drop(child.kill());
            drop(child.wait());
            return Err(ProcessError::NoPipe(program.clone()));
        };
        Ok(Spawned {
            child,
            input,
            output,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Opener;

impl Opener {
    const fn program() -> (&'static str, &'static [&'static str]) {
        if cfg!(windows) {
            ("rundll32", &["url.dll,FileProtocolHandler"])
        } else if cfg!(target_os = "macos") {
            ("open", &[])
        } else {
            ("xdg-open", &[])
        }
    }

    pub fn open(target: &Argument) -> Result<(), ProcessError> {
        let (name, leading) = Self::program();
        let program = Program::new(name);
        let arguments: Vec<Argument> = leading
            .iter()
            .map(|argument| Argument::new(argument))
            .chain([target.clone()])
            .collect();
        command(&program, &arguments, &env::temp_dir())?
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
            .map_err(|error| ProcessError::Start { program, error })
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "io-process is the one place that starts processes"
)]
fn command(
    program: &Program,
    arguments: &[Argument],
    directory: &Path,
) -> Result<Command, ProcessError> {
    let executable = program
        .find_on_path()
        .ok_or_else(|| ProcessError::Missing(program.clone()))?;
    let mut command = Command::new(executable);
    command
        .args(arguments.iter().map(Argument::as_str))
        .current_dir(directory);
    Ok(command)
}

#[cfg(test)]
mod tests;
