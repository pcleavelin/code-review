mod convert;
mod wire;

use std::env;
use std::fmt;
use std::path::{Path, PathBuf};

use domain::{Grant, Host};
use io_process::{Argument, Captured, Output, Process, ProcessError, Program};

use crate::wire::{KEYCHAIN_VARIABLE, Tool};

#[derive(Debug)]
pub enum KeychainError {
    Unsupported,
    NoKeychain(PathBuf),
    Process(ProcessError),
    Refused { program: Program, stderr: Captured },
    Unreadable(Host),
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
            Self::Process(ProcessError::Missing(program)) => {
                write!(formatter, "{program} is not on PATH: install it")
            }
            Self::Process(ProcessError::Start { program, error }) => {
                write!(formatter, "cannot run {program}: {error}")
            }
            Self::Process(ProcessError::NoPipe(program)) => {
                write!(formatter, "cannot write to {program}")
            }
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
        match input {
            Some(input) => Process::run_with_input(&program, arguments, &env::temp_dir(), input),
            None => Process::run(&program, arguments, &env::temp_dir()),
        }
        .map_err(KeychainError::Process)
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
