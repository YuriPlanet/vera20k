//! The lobby's own lines in the player's language.
//!
//! Chat the lobby writes itself, from its notice sender and the matchbot's
//! `Info` lines, carries `phrases` beside its English `text`
//! (`docs/lobby-protocol.md`): each sentence a key that VERA's language files
//! hold as `VERA:Lobby<key>`, with the values to fill in. VERA shows the
//! sentences in the language in use, and the lobby's `text` when a language
//! file lacks a key, as for a key added to the lobby after this release.

use std::collections::BTreeMap;
use std::fmt::Display;

use icu_plurals::PluralOperands;
use serde::Deserialize;

/// One sentence, or a part of one: `VERA:Lobby<key>` with its `{name}`
/// values. A phrase whose values hold `count` takes the plural form its
/// language gives that number.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub(crate) struct Phrase {
    key: String,
    #[serde(default)]
    values: BTreeMap<String, PhraseValue>,
}

/// A value a phrase fills in.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub(crate) enum PhraseValue {
    /// Shown as it is, such as a player's name.
    Text(String),
    Number(serde_json::Number),
    /// A list read "a, b and c".
    And {
        and: Vec<PhraseValue>,
    },
    /// A list read "a, b, c".
    Unit {
        unit: Vec<PhraseValue>,
    },
    Phrase(Phrase),
}

/// A lobby line's `phrases` in the language in use, or `None` when a
/// language file lacks one of their keys or a value is malformed, and the
/// line's own text stands.
pub(crate) fn localized(phrases: &[Phrase]) -> Option<String> {
    let sentences = phrases
        .iter()
        .map(Phrase::render)
        .collect::<Option<Vec<_>>>()?;
    joined(&sentences, "VERA:LobbySentences", "VERA:LobbySentences")
}

impl Phrase {
    fn render(&self) -> Option<String> {
        let key = format!("VERA:Lobby{}", self.key);
        let text = match self.values.get("count") {
            Some(PhraseValue::Number(count)) => {
                crate::i18n::find_count(&key, PluralOperands::from(count.as_u64()?))?
            }
            Some(_) => return None,
            None => crate::i18n::find(&key)?,
        };
        let values = self
            .values
            .iter()
            .map(|(name, value)| Some((name.as_str(), value.render()?)))
            .collect::<Option<Vec<_>>>()?;
        let values: Vec<(&str, &dyn Display)> = values
            .iter()
            .map(|(name, value)| (*name, value as &dyn Display))
            .collect();
        Some(crate::i18n::fill(&text, &values))
    }
}

impl PhraseValue {
    fn render(&self) -> Option<String> {
        match self {
            Self::Text(text) => Some(text.clone()),
            Self::Number(number) => Some(number.to_string()),
            Self::And { and } => list(and, AND_LIST),
            Self::Unit { unit } => list(unit, UNIT_LIST),
            Self::Phrase(phrase) => phrase.render(),
        }
    }
}

/// A list's patterns, as CLDR names them: two items, and of more, each
/// before the last two and the last two.
struct ListKeys {
    pair: &'static str,
    middle: &'static str,
    end: &'static str,
}

const AND_LIST: ListKeys = ListKeys {
    pair: "VERA:LobbyListAndPair",
    middle: "VERA:LobbyListAndMiddle",
    end: "VERA:LobbyListAndEnd",
};

const UNIT_LIST: ListKeys = ListKeys {
    pair: "VERA:LobbyListUnit",
    middle: "VERA:LobbyListUnit",
    end: "VERA:LobbyListUnit",
};

fn list(items: &[PhraseValue], keys: ListKeys) -> Option<String> {
    let items = items
        .iter()
        .map(PhraseValue::render)
        .collect::<Option<Vec<_>>>()?;
    match items.as_slice() {
        [first, second] => join(keys.pair, first, second),
        _ => joined(&items, keys.middle, keys.end),
    }
}

/// `items` joined as CLDR list patterns join them: the last two with `end`,
/// each earlier one onto the rest with `middle`, `{first}` before `{second}`.
fn joined(items: &[String], middle: &str, end: &str) -> Option<String> {
    let Some((last, rest)) = items.split_last() else {
        return Some(String::new());
    };
    let Some((before, rest)) = rest.split_last() else {
        return Some(last.clone());
    };
    let tail = join(end, before, last)?;
    rest.iter()
        .rev()
        .try_fold(tail, |tail, item| join(middle, item, &tail))
}

fn join(key: &str, first: &str, second: &str) -> Option<String> {
    let pattern = crate::i18n::find(key)?;
    Some(crate::i18n::fill(
        &pattern,
        &[("first", &first), ("second", &second)],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lobby's status line, as `service.js` sends it, reads in English as
    /// the lobby's own text: the English file holds the lobby's phrases.
    #[test]
    fn a_lobby_line_reads_as_the_lobby_wrote_it_in_english() {
        let line: Vec<Phrase> = serde_json::from_str(
            r#"[{"key":"PlayersOnline","values":{"count":6,"places":{"and":[
                {"key":"InThisLobby","values":{"count":2}},
                {"key":"InGameRooms","values":{"count":1}},
                {"key":"Playing","values":{"count":1}},
                {"key":"Elsewhere","values":{"count":2}}]}}},
               {"key":"GamesHidden","values":{"count":1}},
               {"key":"Odds","values":{"sides":{"unit":[
                {"key":"SideChance","values":{"side":{"key":"Team","values":{"letter":"A"}},"percent":54}},
                {"key":"SideChance","values":{"side":"{count}","percent":46}}]}}}]"#,
        )
        .unwrap();
        assert_eq!(
            localized(&line).as_deref(),
            Some(
                "6 players are online: 2 in this lobby, 1 in a game room, 1 playing and \
                 2 elsewhere. 1 game is hidden because it runs another VERA20k version. \
                 Team A 54%, {count} 46%."
            )
        );
        let unknown: Vec<Phrase> =
            serde_json::from_str(r#"[{"key":"OnlyPlayerOnline"},{"key":"NotYetKnown"}]"#).unwrap();
        assert_eq!(localized(&unknown), None, "the lobby's own text stands");
    }
}
