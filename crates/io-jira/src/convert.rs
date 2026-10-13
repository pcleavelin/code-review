use domain::{Card, CardField, CardKey, JiraSite, Prose};

use crate::wire::WireCard;

pub(crate) fn card(site: &JiraSite, wire: &WireCard) -> Option<Card> {
    let key = CardKey::new(&wire.key)?;
    Some(Card {
        link: site.card_link(&key),
        key,
        summary: CardField::new(&wire.summary),
        status: CardField::new(&wire.status),
        stage: wire.stage,
        kind: CardField::new(&wire.kind),
        family: wire.family,
        assignee: wire.assignee.as_deref().map(CardField::new),
        description: Prose::new(&wire.description),
    })
}
