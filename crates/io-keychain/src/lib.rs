mod convert;
mod wire;

use std::env;
use std::fmt;
use std::path::{Path, PathBuf};

use domain::{Grant, Host};
use io_process::{Argument, Captured, Deadline, Output, Process, ProcessError, Program};

use crate::wire::{KEYCHAIN_DEADLINE, KEYCHAIN_VARIABLE, SERVICE, Tool};

#[derive(Debug)]
pub enum KeychainError {
    Unsupported,
    NoKeychain(PathBuf),
    Process(ProcessError),
    Refused {
        program: Program,
        stderr: Captured,
    },
    Unreadable(Host),
    TimedOut {
        program: Program,
        deadline: Deadline,
    },
}

impl fmt::Display for KeychainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => formatter.write_str(
                "this system has no keychain the tool can use (macOS security, or secret-tool for the Secret Service): set the forge's token variable instead",
            ),
            Self::NoKeychain(path) => write!(
                formatter,
                "{KEYCHAIN_VARIABLE} names {}, which is not a keychain file: create it first (security create-keychain -p '' <file>)",
                path.display()
            ),
            Self::Process(error) => write!(formatter, "{error}"),
            Self::TimedOut { program, deadline } => write!(
                formatter,
                "the keychain did not answer within {deadline}: {program} may be waiting on a dialog that asks to unlock the keychain or to allow access to the {SERVICE} item; answer it (or unlock the keychain), then refresh the Inbox or run the command again"
            ),
            Self::Refused { program, stderr } => write!(
                formatter,
                "{program} refused: {}",
                stderr.text().trim()
            ),
            Self::Unreadable(host) => write!(
                formatter,
                "the keychain holds something for {host} that is not a login of this tool: log out of {host} and log in again"
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forgotten {
    Removed,
    Absent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Keychain {
    file: Option<PathBuf>,
}

fn named() -> Option<PathBuf> {
    env::var_os(KEYCHAIN_VARIABLE).map(PathBuf::from)
}

impl Keychain {
    pub fn user() -> Self {
        Self { file: named() }
    }

    pub fn override_only() -> Option<Self> {
        named().map(|file| Self { file: Some(file) })
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    fn tool(&self) -> Result<Tool, KeychainError> {
        let tool = Tool::here().ok_or(KeychainError::Unsupported)?;
        if let Some(file) = &self.file
            && tool == Tool::Security
            && !file.is_file()
        {
            return Err(KeychainError::NoKeychain(file.clone()));
        }
        Ok(tool)
    }

    fn run(
        tool: Tool,
        arguments: &[Argument],
        input: Option<&Captured>,
    ) -> Result<Output, KeychainError> {
        let program = Program::new(tool.program());
        let directory = env::temp_dir();
        match input {
            Some(input) => {
                Process::run_with_input(&program, arguments, &directory, input, KEYCHAIN_DEADLINE)
            }
            None => Process::run(&program, arguments, &directory, KEYCHAIN_DEADLINE),
        }
        .map_err(|error| match error {
            ProcessError::TimedOut {
                program: stopped,
                deadline,
                ..
            } => KeychainError::TimedOut {
                program: stopped,
                deadline,
            },
            other => KeychainError::Process(other),
        })
    }

    fn refused(tool: Tool, output: Output) -> KeychainError {
        KeychainError::Refused {
            program: Program::new(tool.program()),
            stderr: output.stderr,
        }
    }

    fn stored(&self, host: &Host) -> Result<Option<Captured>, KeychainError> {
        let tool = self.tool()?;
        let output = Self::run(
            tool,
            &wire::find_arguments(tool, host.as_str(), self.file()),
            None,
        )?;
        if tool.absent(output.status.code()) {
            return Ok(None);
        }
        if !output.success() {
            return Err(Self::refused(tool, output));
        }
        Ok((!output.stdout.text().trim().is_empty()).then_some(output.stdout))
    }

    pub fn load(&self, host: &Host) -> Result<Option<Grant>, KeychainError> {
        let Some(stored) = self.stored(host)? else {
            return Ok(None);
        };
        wire::parse_secret(&stored.text())
            .and_then(|secret| convert::grant(&secret))
            .map(Some)
            .ok_or_else(|| KeychainError::Unreadable(host.clone()))
    }

    pub fn save(&self, host: &Host, grant: &Grant) -> Result<(), KeychainError> {
        let tool = self.tool()?;
        let secret = wire::print_secret(&convert::wire(grant));
        let store = wire::store(tool, host.as_str(), &secret, self.file());
        let output = Self::run(tool, &store.arguments, Some(&store.input))?;
        if output.success() {
            Ok(())
        } else {
            Err(Self::refused(tool, output))
        }
    }

    pub fn forget(&self, host: &Host) -> Result<Forgotten, KeychainError> {
        if self.stored(host)?.is_none() {
            return Ok(Forgotten::Absent);
        }
        let tool = self.tool()?;
        let output = Self::run(
            tool,
            &wire::delete_arguments(tool, host.as_str(), self.file()),
            None,
        )?;
        if output.success() {
            Ok(Forgotten::Removed)
        } else if tool.absent(output.status.code()) {
            Ok(Forgotten::Absent)
        } else {
            Err(Self::refused(tool, output))
        }
    }
}

#[cfg(test)]
mod tests;
