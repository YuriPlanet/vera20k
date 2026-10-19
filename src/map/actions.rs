//! Map action parsing.
//!
//! `[Actions]` stores a counted list of 8-field action chunks per trigger id.
//! We preserve the raw field list and also expose normalized action entries so
//! runtime code can stop guessing at the original payload shape.

use std::collections::{BTreeSet, HashMap};

use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::{crt_atoi, strtok, truncate_native_bytes};
use crate::rules::ruleset::RuleSet;

/// Definition selection performed by original TAction Read6DD5B0. A short
/// token is a retained registry ordinal; a longer token is a name, including
/// its spaces. Registry resolution belongs to the corresponding definition
/// owner, not to the diagnostic HashMaps in this parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DefinitionSelector<'a> {
    Index(i32),
    Name(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionDefinitionReference<'a> {
    Team(DefinitionSelector<'a>),
    Trigger(DefinitionSelector<'a>),
    Tag(DefinitionSelector<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ActionEntry {
    pub kind: i32,
    pub params: Vec<String>,
    /// Materialized native `TActionClass+0x44` waypoint destination.
    ///
    /// `Some(0)` includes the constructor default retained by a missing token
    /// and parameter types 5/9/11. `None` safely represents native `-1` from
    /// a present non-alphabetic token.
    pub waypoint_index: Option<u32>,
}

impl ActionEntry {
    /// Inline CSF label at TAction+6D, materialized by Read6DD6D0..6DD728.
    /// Metadata4 checks atoi(token3)==-1 before skipping leading control/space
    /// bytes, copying at most31 source bytes and clearing trailing bytes<=20.
    /// This inline trim differs from the shared727CF0 strtrim reader.
    pub(crate) fn csf_label(&self) -> &str {
        if self.params.first().map_or(0, |value| crt_atoi(value)) != 4 {
            return "";
        }
        let Some(label) = self.params.get(1) else {
            return "";
        };
        if crt_atoi(label) == -1 {
            return "";
        }
        let label = label.trim_start_matches(|character: char| character <= '\u{20}');
        truncate_native_bytes(label, 31).trim_end_matches(|character: char| character <= '\u{20}')
    }

    /// Original Read6DD5B0 stores token3 at Team+30 (types1/5), Trigger+50
    /// (type2) or Tag+4C (type3). Token2 selects that destination; it never
    /// names the target. `atoi(token3)==-1` clears it before the byte-length
    /// check, even when token3 has spaces or a suffix. See the reference-read
    /// controls in tools/spatial_oracle/trigger_action_values.
    pub(crate) fn definition_reference(&self) -> Option<ActionDefinitionReference<'_>> {
        let parameter_type = self.params.first().map_or(0, |value| crt_atoi(value));
        if !matches!(parameter_type, 1 | 2 | 3 | 5) {
            return None;
        }
        let operand = self.params.get(1)?.as_str();
        let index = crt_atoi(operand);
        if index == -1 {
            return None;
        }
        let selector = if operand.len() < 3 {
            DefinitionSelector::Index(index)
        } else {
            DefinitionSelector::Name(operand)
        };
        Some(match parameter_type {
            1 | 5 => ActionDefinitionReference::Team(selector),
            2 => ActionDefinitionReference::Trigger(selector),
            3 => ActionDefinitionReference::Tag(selector),
            _ => unreachable!("definition parameter type was admitted above"),
        })
    }

    /// Literal `TActionClass+0x90` value after `Read @ 0x006DD5B0`.
    ///
    /// Parameter type is params[0], not the action's numeric operand. Types
    /// 0/11 read params[1]; types 5/9 replace the value with token 8. Other
    /// literal types retain zero. Types 6/7/8 need EVA/sound/theme registry
    /// resolution and remain unsupported here, rather than becoming an index.
    /// Native reader/dispatch comparisons: `trigger_action_values.json`.
    pub fn literal_value(&self) -> Option<i32> {
        let number = |index| {
            self.params
                .get(index)
                .map_or(0, |value: &String| crt_atoi(value))
        };
        match number(0) {
            0 | 11 => Some(number(1)),
            5 | 9 => Some(number(6)),
            6..=8 => None,
            _ => Some(0),
        }
    }

    /// `TActionClass+0x48`: Read6DD852..6DD86A atoi-reads token8 only for
    /// parameter type11. Other types retain constructor6DD000's zero.
    /// This is independent of the +90 literal and +44 waypoint. Executed
    /// constructor/read/dispatch controls: trigger_super_charge_percent.json.
    pub(crate) fn auxiliary_value(&self) -> i32 {
        if self.params.first().map_or(0, |value| crt_atoi(value)) != 11 {
            return 0;
        }
        self.params.get(6).map_or(0, |value| crt_atoi(value))
    }

    /// Types9/10 share the inline type ID at TAction+54. The original switch
    /// table6DD880 routes both to6DD7F6; Read6DD801..6DD831 copies
    /// 24 bytes and appends NUL; it does not trim or canonicalize case.
    /// FlashBuildings6E4560 and CreateBuilding6E4200 use this same literal.
    /// Type10 retains alphabetic +44; type9 reads token8 into +90 instead.
    /// Executed reader controls: tools/spatial_oracle/trigger_inline_type.json.
    pub(crate) fn inline_type_name(&self) -> &str {
        if !matches!(
            self.params.first().map_or(0, |value| crt_atoi(value)),
            9 | 10
        ) {
            return "";
        }
        let value = self.params.get(1).map_or("", String::as_str);
        truncate_native_bytes(value.split('\0').next().unwrap_or(""), 24)
    }
}

/// One `[Actions]` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapAction {
    id: String,
    fields: Vec<String>,
    entries: Vec<ActionEntry>,
}

impl MapAction {
    /// The actions in authored order, as TActionClass::Read (`0x006DD5B0`)
    /// consumed them.
    pub fn entries(&self) -> &[ActionEntry] {
        &self.entries
    }
}

pub type ActionMap = HashMap<String, MapAction>;

/// Decode the alphabetic waypoint token stored in action field 8.
///
/// gamemd-derived: `TActionClass::Read @ 0x006DD5B0` routes non-numeric
/// parameter types through `FUN_00763690`. That helper examines at most two
/// ASCII letters, case-insensitively: `A..Z` are `0..25`, and a second letter
/// extends the index as `26 * first + second + 26`. A non-letter first byte is
/// the native `-1` sentinel, represented safely as `None` here.
pub fn decode_waypoint_token(token: &str) -> Option<u32> {
    let bytes = token.as_bytes();
    let first = ascii_waypoint_letter(*bytes.first()?)?;
    let Some(&second_byte) = bytes.get(1) else {
        return Some(first);
    };
    let Some(second) = ascii_waypoint_letter(second_byte) else {
        return Some(first);
    };
    Some(26 * first + second + 26)
}

/// Apply the `TActionClass` constructor/read contract around the decoder.
///
/// The constructor initializes the destination to waypoint 0. `Read` replaces
/// it only when `strtok` returns token 8, so an absent or empty-at-end token
/// retains zero. A present whitespace/non-letter token does reach the decoder
/// and is invalid.
pub fn read_waypoint_token(token: Option<&str>) -> Option<u32> {
    match token {
        None | Some("") => Some(0),
        Some(token) => decode_waypoint_token(token),
    }
}

fn ascii_waypoint_letter(byte: u8) -> Option<u32> {
    byte.to_ascii_uppercase()
        .checked_sub(b'A')
        .filter(|value| *value < 26)
        .map(u32::from)
}

/// Parse `[Actions]` into exact-key action records.
///
/// The TriggerType read (`0x00727516..0x007275A5`) ReadStrings the entry of
/// the Type's own ID, exact case, into 0x200 bytes and takes `atoi` of the
/// first `strtok(",")` token as the action count; TActionClass::Read
/// (`0x006DD5B0`) then consumes eight tokens per action from the same cursor.
/// Empty comma fields are skipped; whitespace-only fields still consume a
/// token.
pub fn parse_actions(ini: &IniFile) -> ActionMap {
    let Some(section) = ini.section("Actions") else {
        return HashMap::new();
    };

    let mut actions: ActionMap = HashMap::new();
    for key in section.keys() {
        let Some(value) = section.read_name(key, 0x200) else {
            continue;
        };
        let id = key.to_string();
        let tokens: Vec<&str> = strtok(value, &[',']).collect();
        let entries = parse_action_entries(&tokens);
        actions.insert(
            id.clone(),
            MapAction {
                id,
                fields: tokens.iter().map(|token| token.to_string()).collect(),
                entries,
            },
        );
    }

    if !actions.is_empty() {
        log::info!("Parsed {} actions from [Actions]", actions.len());
    }
    actions
}

/// Native reads past the last token as `atoi(NULL)`; Rust stops at the
/// tokens the line has.
fn parse_action_entries(tokens: &[&str]) -> Vec<ActionEntry> {
    let Some((count, payload)) = tokens.split_first() else {
        return Vec::new();
    };
    let count = usize::try_from(crt_atoi(count)).unwrap_or(0);
    payload
        .chunks(8)
        .take(count)
        .map(|chunk| ActionEntry {
            kind: crt_atoi(chunk[0]),
            params: chunk[1..].iter().map(|param| param.to_string()).collect(),
            waypoint_index: materialize_waypoint_index(
                chunk.get(1).copied(),
                chunk.get(7).copied(),
            ),
        })
        .collect()
}

fn materialize_waypoint_index(param_type: Option<&str>, token_8: Option<&str>) -> Option<u32> {
    if matches!(param_type.map(crt_atoi), Some(5 | 9 | 11)) {
        // These parameter types parse token 8 numerically into other TAction
        // fields and leave the constructor-initialized +0x44 untouched.
        return Some(0);
    }
    read_waypoint_token(token_8)
}

/// TAction8 binds the retained Rules.DropZoneAnim; TAction41 resolves its
/// numeric AnimType through the same native registry as runtime construction
/// (6E2357). TAction58 roots the two find-only meteor literals (6E2D11/1D).
/// All use the existing scheduler closure at load and restoration.
pub fn trigger_anim_class_roots(rules: &RuleSet, actions: &ActionMap) -> Vec<String> {
    actions
        .values()
        .flat_map(|action| &action.entries)
        .flat_map(|action| match action.kind {
            58 => [
                rules
                    .anim_type_names
                    .contains("METSMALL")
                    .then_some("METSMALL"),
                rules
                    .anim_type_names
                    .contains("METLARGE")
                    .then_some("METLARGE"),
            ],
            8 => [rules.general.drop_zone_anim.as_deref(), None],
            41 => [
                action
                    .literal_value()
                    .and_then(|index| rules.anim_type_name_by_index(index)),
                None,
            ],
            _ => [None, None],
        })
        .flatten()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_inline_type_reader_keeps_type10_waypoint_separate_from_type9_value() {
        let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/trigger_inline_type.json",
        ))
        .unwrap();
        for row in rows.as_array().unwrap() {
            let ini = IniFile::from_str(&format!(
                "[Actions]\nINLINE=1,{}\n",
                row["raw"].as_str().unwrap()
            ));
            let parsed = parse_actions(&ini);
            let action = &parsed["INLINE"].entries[0];
            assert_eq!(
                action.inline_type_name(),
                row["inline_type"].as_str().unwrap()
            );
            assert_eq!(
                action.literal_value(),
                Some(row["value"].as_i64().unwrap() as i32)
            );
            assert_eq!(
                action.waypoint_index,
                Some(row["waypoint"].as_u64().unwrap() as u32)
            );
        }
    }

    #[test]
    fn test_parse_actions() {
        let ini = IniFile::from_str(
            "[Actions]\nAC_A=2,28,7,0,0,0,0,0,0,112,0,0,0,0,0,0,9\nAC_B=11,Americans,5\n",
        );
        let actions = parse_actions(&ini);
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions.get("AC_A"),
            Some(&MapAction {
                id: "AC_A".to_string(),
                fields: vec![
                    "2".to_string(),
                    "28".to_string(),
                    "7".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "112".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "0".to_string(),
                    "9".to_string(),
                ],
                entries: vec![
                    ActionEntry {
                        kind: 28,
                        params: vec![
                            "7".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                        ],
                        waypoint_index: None,
                    },
                    ActionEntry {
                        kind: 112,
                        params: vec![
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "0".to_string(),
                            "9".to_string(),
                        ],
                        waypoint_index: None,
                    },
                ],
            })
        );
        assert_eq!(
            actions.get("AC_B").map(|action| action.fields.as_slice()),
            Some(&["11".to_string(), "Americans".to_string(), "5".to_string()][..])
        );
        // The first token is the action count, so this line is eleven
        // actions whose first kind is `atoi("Americans")`.
        assert_eq!(
            actions.get("AC_B").map(|action| action.entries.as_slice()),
            Some(
                &[ActionEntry {
                    kind: 0,
                    params: vec!["5".to_string()],
                    waypoint_index: Some(0),
                }][..]
            )
        );
    }

    #[test]
    fn test_missing_actions_is_empty() {
        let ini = IniFile::from_str("[Map]\nTheater=TEMPERATE\n");
        assert!(parse_actions(&ini).is_empty());
    }

    #[test]
    fn waypoint_tokens_follow_the_native_two_letter_decoder() {
        for (token, expected) in [
            ("A", Some(0)),
            ("Z", Some(25)),
            ("a", Some(0)),
            ("P", Some(15)),
            ("AA", Some(26)),
            ("aa", Some(26)),
            ("NZ", Some(389)),
            ("ZZ", Some(701)),
            ("", None),
            ("7", None),
            ("   ", None),
            ("A7", Some(0)),
            ("NZignored", Some(389)),
        ] {
            assert_eq!(decode_waypoint_token(token), expected, "token {token:?}");
        }
    }

    #[test]
    fn action_read_preserves_ctor_zero_but_not_present_whitespace() {
        assert_eq!(read_waypoint_token(None), Some(0));
        assert_eq!(read_waypoint_token(Some("")), Some(0));
        assert_eq!(read_waypoint_token(Some("   ")), None);

        let ini = IniFile::from_str(
            "[Actions]\n\
             EMPTY=1,137,0,0,0,0,0,0,\n\
             MISSING=1,112,0,0,0,0,0,0\n\
             ALPHA=2,48,0,0,0,0,0,0,NZ,112,0,0,0,0,0,0,7\n\
             NUMERIC=3,48,5,0,0,0,0,0,P,112,9,0,0,0,0,0,NZ,137,11,0,0,0,0,0,AA\n",
        );
        let actions = parse_actions(&ini);
        assert_eq!(actions["EMPTY"].entries[0].waypoint_index, Some(0));
        assert_eq!(actions["MISSING"].entries[0].kind, 112);
        assert_eq!(actions["MISSING"].entries[0].params.len(), 6);
        assert_eq!(actions["MISSING"].entries[0].waypoint_index, Some(0));
        assert_eq!(actions["ALPHA"].entries[0].waypoint_index, Some(389));
        assert_eq!(actions["ALPHA"].entries[1].waypoint_index, None);
        assert_eq!(
            actions["NUMERIC"]
                .entries
                .iter()
                .map(|entry| (entry.kind, entry.waypoint_index))
                .collect::<Vec<_>>(),
            vec![(48, Some(0)), (112, Some(0)), (137, Some(0))]
        );
    }
    #[test]
    fn waypoint_cue_flare_asset_root_uses_rules_only_when_the_map_authors_action_eight() {
        let flare = parse_actions(&IniFile::from_str("[Actions]\nCUE=1,8,0,99,0,0,0,0,B\n"));
        let sound = parse_actions(&IniFile::from_str(
            "[Actions]\nCUE=1,98,7,Sound,0,0,0,0,B\n",
        ));
        let rules =
            RuleSet::from_ini(&IniFile::from_str("[General]\nDropZoneAnim=FLARE\n")).unwrap();
        assert_eq!(trigger_anim_class_roots(&rules, &flare), ["FLARE"]);
        assert!(trigger_anim_class_roots(&rules, &sound).is_empty());
        let cleared =
            RuleSet::from_ini(&IniFile::from_str("[General]\nDropZoneAnim=none\n")).unwrap();
        assert!(trigger_anim_class_roots(&cleared, &flare).is_empty());
    }
    #[test]
    fn waypoint_cue_retail_beacon_binds_and_constructs_through_the_shared_asset_root() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let mut rules = retail.rules;
        let actions = parse_actions(&IniFile::from_str("[Actions]\nCUE=1,8,0,99,0,0,0,0,B\n"));
        let roots = trigger_anim_class_roots(&rules, &actions);
        assert_eq!(roots, ["BEACON"]);
        // Same strict owner used by both app load roots and saved resources.
        // Native BEACON controls read its physical ten-frame SHP and ART.
        rules
            .bind_scheduler_anim_assets(&roots, &assets, "TEM", "TEMPERATE")
            .unwrap();
        let mut sim = crate::sim::world::Simulation::with_seed(31);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        sim.session
            .install_waypoints(&std::collections::HashMap::from([(
                1,
                crate::map::waypoints::Waypoint {
                    index: 1,
                    rx: 10,
                    ry: 10,
                },
            )]));
        assert!(
            sim.trigger_drop_zone_flare(&rules, 1),
            "the retail asset closure admits the production constructor"
        );
    }
}
