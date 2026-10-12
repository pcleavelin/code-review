mod output;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use cli::{Argument, Channel, Invocation, Output, Touched};
use domain::Root;
use session::{Desk, Reach};
use strum::VariantArray;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WordName(&'static str);

impl WordName {
    const fn as_str(self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, VariantArray)]
enum HelpWord {
    Word,
    Flag,
    Letter,
}

impl HelpWord {
    const fn name(self) -> WordName {
        WordName(match self {
            Self::Word => "help",
            Self::Flag => "--help",
            Self::Letter => "-h",
        })
    }
}

enum Status {
    Done,
    WindowFailed,
    Refused,
}

impl Status {
    fn code(self) -> ExitCode {
        match self {
            Self::Done => ExitCode::SUCCESS,
            Self::WindowFailed => ExitCode::from(1),
            Self::Refused => ExitCode::from(2),
        }
    }
}

fn run(root: &Root, arguments: &[Argument]) -> Status {
    let invocation = match Invocation::parse(arguments) {
        Ok(invocation) => invocation,
        Err(failure) => {
            output::text(failure.channel(), &failure.text());
            return match failure.channel() {
                Channel::Stdout => Status::Done,
                Channel::Stderr => Status::Refused,
            };
        }
    };
    let mut desk = Desk::load(Some(root), Reach::User);
    let mut report = Output::new();
    let done = cli::exec(&mut desk, invocation, &mut report);
    output::text(Channel::Stdout, &report);
    let done = match done {
        Ok(Touched::LoginStarted(started)) => {
            let pasted = match &started {
                session::StartedLogin::Paste { .. } => output::pasted_line(),
                session::StartedLogin::Device(_) => None,
            };
            let mut finished = Output::new();
            let result = cli::finish_login(&desk, &started, pasted.as_ref(), &mut finished);
            output::text(Channel::Stdout, &finished);
            result.map(|()| Touched::Nothing)
        }
        other => other,
    };
    match done {
        Ok(_) => Status::Done,
        Err(failure) => {
            output::line(Channel::Stderr, &failure);
            Status::Refused
        }
    }
}

fn window(root: &Root) -> Status {
    match gui::run(root) {
        Ok(()) => Status::Done,
        Err(error) => {
            output::line(Channel::Stderr, &error);
            Status::WindowFailed
        }
    }
}

fn main() -> ExitCode {
    let arguments: Vec<Argument> = env::args()
        .skip(1)
        .map(|argument| Argument::new(&argument))
        .collect();
    let first = arguments.first();
    if first.is_some_and(|first| {
        HelpWord::VARIANTS
            .iter()
            .any(|entry| entry.name().as_str() == first.as_str())
    }) {
        output::text(Channel::Stdout, &cli::help());
        return ExitCode::SUCCESS;
    }
    let here = Root::new(&env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    if arguments.is_empty() {
        return window(&here).code();
    }
    run(&here, &arguments).code()
}
