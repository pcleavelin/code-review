use std::fmt;
use std::io;
use std::io::{BufRead, Write};

use cli::{Channel, Output, PastedToken};

pub(crate) fn text(channel: Channel, output: &Output) {
    write(channel, format_args!("{output}"));
}

pub(crate) fn line(channel: Channel, message: &impl fmt::Display) {
    write(channel, format_args!("{message}\n"));
}

fn write(channel: Channel, arguments: fmt::Arguments<'_>) {
    match channel {
        Channel::Stdout => {
            let mut stdout = io::stdout().lock();
            stdout.write_fmt(arguments).unwrap_or_default();
            stdout.flush().unwrap_or_default();
        }
        Channel::Stderr => io::stderr().lock().write_fmt(arguments).unwrap_or_default(),
    }
}

pub(crate) fn pasted_line() -> Option<PastedToken> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line).ok()?;
    Some(PastedToken::new(&line))
}
