use std::fmt;

use crate::review::{ForgeKind, Host};

struct Graphic(String);

impl Graphic {
    fn of(text: &str) -> Option<Self> {
        let text = text.trim();
        (!text.is_empty() && text.chars().all(|character| character.is_ascii_graphic()))
            .then(|| Self(text.to_owned()))
    }
}

macro_rules! secret_type {
    ($name:ident) => {
        #[derive(Clone, PartialEq, Eq)]
        pub struct $name(String);

        impl $name {
            pub fn new(text: &str) -> Option<Self> {
                Graphic::of(text).map(|graphic| Self(graphic.0))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "{}(hidden)", stringify!($name))
            }
        }
    };
}

secret_type!(Token);
secret_type!(RefreshToken);

macro_rules! code_type {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(text: &str) -> Option<Self> {
                Graphic::of(text).map(|graphic| Self(graphic.0))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

code_type!(ClientId);
code_type!(UserCode);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnixTime(u64);

impl UnixTime {
    pub const fn new(seconds: u64) -> Self {
        Self(seconds)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn later_by(self, seconds: Lifetime) -> Self {
        Self(self.0.saturating_add(seconds.0))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lifetime(u64);

impl Lifetime {
    pub const fn seconds(seconds: u64) -> Self {
        Self(seconds)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Renewal {
    pub refresh: RefreshToken,
    pub expires: UnixTime,
    pub client: ClientId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    pub token: Token,
    pub renewal: Option<Renewal>,
}

impl Grant {
    const MARGIN: Lifetime = Lifetime::seconds(60);

    pub const fn lasting(token: Token) -> Self {
        Self {
            token,
            renewal: None,
        }
    }

    pub fn expired(&self, now: UnixTime) -> bool {
        self.renewal
            .as_ref()
            .is_some_and(|renewal| now.later_by(Self::MARGIN) >= renewal.expires)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OAuthApp {
    pub host: Host,
    pub client: ClientId,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForgeHost {
    kind: ForgeKind,
    host: Host,
}

impl ForgeHost {
    pub const fn new(kind: ForgeKind, host: Host) -> Self {
        Self { kind, host }
    }

    pub const fn kind(&self) -> ForgeKind {
        self.kind
    }

    pub const fn host(&self) -> &Host {
        &self.host
    }
}

impl fmt::Display for ForgeHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.host.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(text: &str) -> Token {
        Token::new(text).unwrap()
    }

    #[test]
    fn a_token_is_trimmed_and_refuses_blanks_spaces_and_control_characters() {
        assert_eq!(token("  gho_abc-1.2\n").as_str(), "gho_abc-1.2");
        assert!(Token::new("   ").is_none());
        assert!(Token::new("two words").is_none());
        assert!(Token::new("tab\there").is_none());
        assert!(Token::new("caf\u{e9}").is_none());
    }

    #[test]
    fn a_secret_never_shows_in_debug_output() {
        let grant = Grant {
            token: token("gho_secret"),
            renewal: Some(Renewal {
                refresh: RefreshToken::new("refresh_secret").unwrap(),
                expires: UnixTime::new(10),
                client: ClientId::new("client2f1e").unwrap(),
            }),
        };
        let shown = format!("{grant:?}");
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("client2f1e"), "{shown}");
    }

    #[test]
    fn a_renewable_grant_expires_a_minute_before_its_time_and_a_lasting_one_never() {
        let renewable = Grant {
            token: token("t"),
            renewal: Some(Renewal {
                refresh: RefreshToken::new("r").unwrap(),
                expires: UnixTime::new(1000),
                client: ClientId::new("c").unwrap(),
            }),
        };
        assert!(!renewable.expired(UnixTime::new(939)));
        assert!(renewable.expired(UnixTime::new(940)));
        assert!(renewable.expired(UnixTime::new(5000)));
        assert!(!Grant::lasting(token("t")).expired(UnixTime::new(u64::MAX)));
    }
}
