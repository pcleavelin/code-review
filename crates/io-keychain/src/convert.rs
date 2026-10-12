use domain::{ClientId, Grant, RefreshToken, Renewal, Token, UnixTime};

use crate::wire::{WireRenewal, WireSecret};

pub(crate) fn grant(wire: &WireSecret) -> Option<Grant> {
    let renewal = match &wire.renewal {
        None => None,
        Some(renewal) => Some(Renewal {
            refresh: RefreshToken::new(&renewal.refresh)?,
            expires: UnixTime::new(renewal.expires),
            client: ClientId::new(&renewal.client)?,
        }),
    };
    Some(Grant {
        token: Token::new(&wire.token)?,
        renewal,
    })
}

pub(crate) fn wire(grant: &Grant) -> WireSecret {
    WireSecret {
        token: grant.token.as_str().to_owned(),
        renewal: grant.renewal.as_ref().map(|renewal| WireRenewal {
            refresh: renewal.refresh.as_str().to_owned(),
            expires: renewal.expires.get(),
            client: renewal.client.as_str().to_owned(),
        }),
    }
}
