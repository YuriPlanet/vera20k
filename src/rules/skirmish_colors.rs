//! Lobby player colours: YR's eight, VERA's extra colours, and Phobos
//! `SkirmishUnlimitedColors`.
//!
//! ## VERA's extra lobby colours
//!
//! gamemd's lobby lists eight colours, priorities 0..=7, and gives an observer
//! priority 8; `Game_GetLinkedColor` (`0x0069A310`) reads all nine from the
//! SessionClass table ([`crate::rules::color_scheme::PRIORITY_TO_SCHEME_INDEX`]).
//! VERA seats thirty players, so its lobbies list up to twenty-two more:
//! priorities 9..=30, each a `[Colors]` entry named by `[SkirmishColors]`.
//! gamemd has no such list, so nothing here cites a native address beyond the
//! conversion it extends. Priorities 0..=8 keep their native meaning, and a
//! priority past the list still passes through the native tail.
//!
//! `[SkirmishColors]` is a registry in the rules INI: `N=Name`, in order, at
//! most [`MAX_EXTRA_COLORS`] names, each a `[Colors]` entry. Without the
//! section the list is the built-in `skirmish_colors.ini`, whose `[Colors]`
//! also supplies the H,S,V of a listed name the rules `[Colors]` leaves out;
//! those entries are appended after the rules' own. A name neither defines is
//! skipped, as is `none`, a second mention of an entry, and an entry one of
//! YR's eight already shows. Rules whose `[Colors]` is too short for YR's
//! eight get no built-in colour: an appended entry would land where the
//! table points and repaint one of the eight.
//!
//! ## Phobos `SkirmishUnlimitedColors`
//!
//! Native identity: Phobos `Phobos.INI.cpp`, hook
//! `InitRules_ThingsThatShouldntBeSerailized` (`0x0052D21F`, 6 bytes), reads
//! `[General] SkirmishUnlimitedColors=` with `CCINIClass::ReadBool`, and while
//! the value is set it writes seven bytes over the entry of `Game_GetLinkedColor`
//! (`0x0069A310`):
//!
//! ```text
//! mov eax, [esp+4]   ; the passed colour index
//! shl eax, 1
//! inc eax
//! ```
//!
//! The source comment for that patch is "Disable Ares hook at this address so
//! that our logic can run": Ares hooks the same seven bytes for its customisable
//! dropdown colours, which is why Phobos lists the two as mutually exclusive.
//!
//! The patch has no return of its own, so execution continues into the native
//! body at `0x0069A317`: its `jne` is always taken (`2 * index + 1` is odd, never
//! zero), and the tail at `0x0069A328` still reads the SessionClass table for a
//! value below 9 ([`crate::rules::color_scheme::linked_color_tail`]). From index
//! 4 on, the runtime scheme index is `2 * index + 1`. gamemd builds two schemes
//! per `[Colors]` entry and the odd half is the one a player colour selects
//! ([`crate::rules::color_scheme::scheme_for_priority`]), so the entry is the
//! index itself: a spawner that sends 6 gets the seventh `[Colors]` entry
//! (`Orange` in stock rulesmd.ini). Indices 0..=3 read table entries 1, 3, 5 and
//! 7 instead (schemes 11, 29, 25 and 15), so they resolve to entries 5, 14, 12
//! and 7.
//!
//! Because the hook replaces the whole conversion, this mode also changes the
//! non-spawner callers that pass a fixed priority (trigger message colours,
//! production and rally tints): Phobos documents the feature as spawner-only
//! and warns that the stock launcher's own priorities then draw other colours.
//! The hook owns every index, so this mode lists no extra lobby colours.
//!
//! ## Dependency rules
//! - Part of rules/ — depends on rules/color_scheme, rules/house_colors and
//!   rules/ini_parser only.

use std::sync::LazyLock;

use crate::rules::color_scheme::{
    ColorSchemeEntry, read_scheme_hsv, scheme_entry_by_name, scheme_entry_for_priority,
};
use crate::rules::ini_parser::IniFile;

/// YR's lobby colours: priorities 0..=7 of the SessionClass table.
pub const RETAIL_COLOR_COUNT: usize = 8;
/// The priority gamemd gives an observer (`0x0069B97D`): the table's default.
pub const OBSERVER_PRIORITY: u8 = 8;
/// The first priority of VERA's extra lobby colours.
pub const FIRST_EXTRA_PRIORITY: u8 = OBSERVER_PRIORITY + 1;
/// Most extra colours a lobby lists: with YR's eight, one for each of thirty
/// players.
pub const MAX_EXTRA_COLORS: usize = 22;

const LIST_SECTION: &str = "SkirmishColors";
/// The buffer the `[Colors]` reader takes its values into (`0x00474C70`); a
/// listed name is cut the same way.
const NAME_CAPACITY: usize = 0x40;

/// `skirmish_colors.ini`: the default list and the H,S,V of its names.
fn built_in() -> &'static IniFile {
    static BUILT_IN: LazyLock<IniFile> =
        LazyLock::new(|| IniFile::from_str(include_str!("skirmish_colors.ini")));
    &BUILT_IN
}

/// The names `[SkirmishColors]` lists, in order. `None` without the section.
fn listed_names(ini: &IniFile) -> Option<Vec<String>> {
    let section = ini.section(LIST_SECTION)?;
    Some(
        section
            .keys()
            .filter_map(|key| section.read_name(key, NAME_CAPACITY))
            .map(str::to_owned)
            .collect(),
    )
}

/// The built-in `[Colors]` entry for `name`, in the built-in spelling.
fn built_in_scheme(name: &str) -> Option<ColorSchemeEntry> {
    let section = built_in().section("Colors")?;
    let key = section
        .keys()
        .find(|key| key.eq_ignore_ascii_case(name.trim()))?;
    Some(ColorSchemeEntry {
        name: key.to_string(),
        hsv: read_scheme_hsv(section, key),
    })
}

/// The player-colour conversion a rules load selects, and the colours its
/// lobby lists.
///
/// `Default` is YR's own: the eight-priority table and no extra colours.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct SkirmishColorMode {
    unlimited: bool,
    /// The `[Colors]` entry of each extra lobby colour, in list order; only
    /// the first `extra_count` are set.
    #[serde(default)]
    extra_entries: [u8; MAX_EXTRA_COLORS],
    #[serde(default)]
    extra_count: u8,
}

impl SkirmishColorMode {
    /// YR's eight-priority conversion (`PRIORITY_TO_SCHEME_INDEX`), with no
    /// extra lobby colours.
    pub const VANILLA: Self = Self {
        unlimited: false,
        extra_entries: [0; MAX_EXTRA_COLORS],
        extra_count: 0,
    };

    /// Phobos `SkirmishUnlimitedColors=true`.
    pub const UNLIMITED: Self = Self {
        unlimited: true,
        ..Self::VANILLA
    };

    pub fn unlimited(self) -> bool {
        self.unlimited
    }

    /// A mode whose extra lobby colours are these `[Colors]` entries.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_extra_entries(entries: &[u8]) -> Self {
        let mut mode = Self::VANILLA;
        for &entry in entries {
            mode.push_extra(entry);
        }
        mode
    }

    fn push_extra(&mut self, entry: u8) {
        self.extra_entries[usize::from(self.extra_count)] = entry;
        self.extra_count += 1;
    }

    fn extras(&self) -> &[u8] {
        &self.extra_entries[..usize::from(self.extra_count)]
    }

    /// The `[Colors]` entry of the extra lobby colour `priority` names.
    fn extra_entry(self, priority: i32) -> Option<usize> {
        let index = priority.checked_sub(i32::from(FIRST_EXTRA_PRIORITY))?;
        let entry = self.extras().get(usize::try_from(index).ok()?)?;
        Some(usize::from(*entry))
    }

    /// The colour priorities a lobby lists, in order: YR's eight, then the
    /// extra colours. Priority 8 is the observer's and never listed.
    pub fn lobby_priorities(self) -> impl Iterator<Item = u8> + Clone {
        (0..RETAIL_COLOR_COUNT as u8)
            .chain((0..self.extra_count).map(|index| FIRST_EXTRA_PRIORITY + index))
    }

    pub fn lobby_color_count(self) -> usize {
        RETAIL_COLOR_COUNT + usize::from(self.extra_count)
    }

    /// Whether a lobby lists `priority` as a colour a player can hold.
    pub fn is_lobby_priority(self, priority: i32) -> bool {
        (0..RETAIL_COLOR_COUNT as i32).contains(&priority) || self.extra_entry(priority).is_some()
    }

    /// The lobby colour after `priority` in list order, wrapping; the first
    /// for a priority the lobby does not list.
    pub fn next_lobby_priority(self, priority: i32) -> u8 {
        let mut listed = self.lobby_priorities();
        listed
            .clone()
            .skip_while(|listed| i32::from(*listed) != priority)
            .nth(1)
            .or_else(|| listed.next())
            .unwrap_or(0)
    }

    /// Native `Game_GetLinkedColor` (`0x0069A310`) as the active build leaves
    /// it: the priority table, VERA's extra lobby colours, or Phobos'
    /// `2 * index + 1` run through the native tail (see the module doc).
    ///
    /// All return a scheme index, not an entry: callers divide by two
    /// ([`Self::entry_for_priority`]). An extra colour answers the odd half of
    /// its entry's pair, the half a player colour selects. A negative priority
    /// is the lobby's random/observer marker; the patched function returns a
    /// negative index for it, which addresses no `[Colors]` entry, so VERA
    /// clamps it to the first entry.
    pub fn scheme_index_for_priority(self, priority: i32) -> usize {
        if self.unlimited {
            crate::rules::color_scheme::linked_color_tail(priority.wrapping_mul(2).wrapping_add(1))
        } else if let Some(entry) = self.extra_entry(priority) {
            entry * 2 + 1
        } else {
            crate::rules::color_scheme::scheme_index_for_priority(priority)
        }
    }

    /// `[Colors]` entry a player colour priority selects under this mode.
    ///
    /// The vanilla answer stays owned by
    /// [`crate::rules::color_scheme::scheme_entry_for_priority`].
    pub fn entry_for_priority(self, priority: i32) -> usize {
        if self.unlimited {
            self.scheme_index_for_priority(priority) / 2
        } else if let Some(entry) = self.extra_entry(priority) {
            entry
        } else {
            scheme_entry_for_priority(priority)
        }
    }

    /// Read the mode from one rules load. A listed name the rules `[Colors]`
    /// leaves out is appended to `schemes` from the built-in definitions, so
    /// every consumer of the scheme list sees it.
    pub(crate) fn from_ini(ini: &IniFile, schemes: &mut Vec<ColorSchemeEntry>) -> Self {
        // Phobos reads the key with the native ReadBool semantics and its
        // constructor default of false. Its hook owns every colour index.
        #[cfg(feature = "phobos")]
        {
            if ini
                .section_or_empty("General")
                .read_bool("SkirmishUnlimitedColors", false)
            {
                return Self::UNLIMITED;
            }
        }
        let names = listed_names(ini)
            .or_else(|| listed_names(built_in()))
            .unwrap_or_default();
        // An extra colour that repeats one of YR's eight helps nobody tell two
        // players apart.
        let mut shown: Vec<usize> = (0..RETAIL_COLOR_COUNT as i32)
            .map(scheme_entry_for_priority)
            .collect();
        // Appending to a list too short for the eight would put a built-in
        // colour where the priority table points.
        let rules_hold_the_eight = shown.iter().all(|&entry| entry < schemes.len());
        let mut mode = Self::VANILLA;
        for name in names {
            if name.eq_ignore_ascii_case("none") || name.eq_ignore_ascii_case("<none>") {
                continue;
            }
            if usize::from(mode.extra_count) == MAX_EXTRA_COLORS {
                log::warn!(
                    "[{LIST_SECTION}] lists more than {MAX_EXTRA_COLORS} colours; {name} and later are ignored"
                );
                break;
            }
            let known = scheme_entry_by_name(schemes, &name);
            let entry = known.unwrap_or(schemes.len());
            // A house colour is a byte, and its top value means "no remap".
            let Some(entry_byte) = u8::try_from(entry)
                .ok()
                .filter(|&entry| entry != crate::rules::house_colors::NO_REMAP.0)
            else {
                log::warn!(
                    "[{LIST_SECTION}] {name}: [Colors] entry {entry} is past the last house colour"
                );
                continue;
            };
            if known.is_none() {
                let Some(scheme) = built_in_scheme(&name).filter(|_| rules_hold_the_eight) else {
                    log::warn!("[{LIST_SECTION}] names {name}, but [Colors] has no such entry");
                    continue;
                };
                schemes.push(scheme);
            }
            if shown.contains(&entry) {
                continue;
            }
            shown.push(entry);
            mode.push_extra(entry_byte);
        }
        mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlimited_mode_runs_the_native_tail_after_the_patch() {
        // Vanilla keeps the SessionClass table: priority 6 is runtime scheme 17,
        // whose doubled pair is [Colors] entry 8.
        assert_eq!(SkirmishColorMode::VANILLA.scheme_index_for_priority(6), 17);
        assert_eq!(SkirmishColorMode::VANILLA.entry_for_priority(6), 8);

        // Oracle: Phobos' seven patch bytes written over gamemd's 0x69A310 and
        // executed in Unicorn. 2 * index + 1 still reads the table below 9, so
        // 0..=3 return schemes 11, 29, 25 and 15; from 4 on the index is the
        // entry.
        let mode = SkirmishColorMode::UNLIMITED;
        for (index, scheme) in [
            (0, 11),
            (1, 29),
            (2, 25),
            (3, 15),
            (4, 9),
            (6, 13),
            (11, 23),
        ] {
            assert_eq!(mode.scheme_index_for_priority(index), scheme, "{index}");
            assert_eq!(mode.entry_for_priority(index), scheme / 2, "{index}");
        }

        // -2 is the lobby's random/observer marker, not an entry: the hook
        // returns -3 there, which addresses no [Colors] entry.
        assert_eq!(mode.entry_for_priority(-2), 0);
    }

    #[cfg(feature = "phobos")]
    #[test]
    fn the_ini_key_defaults_off_and_needs_a_phobos_build() {
        // Fifteen entries of the rules' own, as YR's eight priorities expect.
        let read = |text: &str| {
            let mut schemes: Vec<ColorSchemeEntry> = (0..15)
                .map(|index| ColorSchemeEntry {
                    name: format!("Stock{index}"),
                    hsv: [index, 200, 200],
                })
                .collect();
            SkirmishColorMode::from_ini(&IniFile::from_str(text), &mut schemes)
        };

        let default = read("[General]\n");
        assert!(!default.unlimited());
        assert_eq!(default.lobby_color_count(), 30);

        // The hook owns every colour index, so the mode lists no extras.
        assert_eq!(
            read("[General]\nSkirmishUnlimitedColors=yes\n"),
            SkirmishColorMode::UNLIMITED
        );

        // Native ReadBool takes the first character: 'o' is neither a true nor
        // a false token, so `on`/`off` both retain the default.
        assert!(!read("[General]\nSkirmishUnlimitedColors=off\n").unlimited());
    }

    /// The retail `[Colors]` entries YR's eight priorities select.
    const RETAIL_EIGHT: [usize; 8] = [1, 5, 10, 14, 6, 12, 8, 7];

    #[test]
    fn retail_rules_list_thirty_colours_none_black_white_or_grey() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        let mode = rules.skirmish_colors();
        let listed: Vec<u8> = mode.lobby_priorities().collect();
        assert_eq!(listed.len(), 30);
        assert_eq!(listed[..8], [0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(listed[8..], (9..=30).collect::<Vec<u8>>()[..]);
        assert!(!mode.is_lobby_priority(i32::from(OBSERVER_PRIORITY)));

        // YR's eight keep their entries; the extras follow the rules' own
        // [Colors] in list order, the first of them VeraBurgundy.
        let entries: Vec<usize> = listed
            .iter()
            .map(|&priority| mode.entry_for_priority(i32::from(priority)))
            .collect();
        assert_eq!(entries[..8], RETAIL_EIGHT);
        let first_extra = rules.color_schemes.len() - 22;
        assert_eq!(
            entries[8..],
            (first_extra..first_extra + 22).collect::<Vec<_>>()[..]
        );
        assert_eq!(rules.color_schemes[first_extra].name, "VeraBurgundy");
        assert_eq!(mode.scheme_index_for_priority(9), first_extra * 2 + 1);

        // Every listed colour has a hue a player can name: neither washed out
        // nor dark, and no two share an entry.
        for (index, &entry) in entries.iter().enumerate() {
            let scheme = &rules.color_schemes[entry];
            let [_, saturation, value] = scheme.hsv;
            assert!(
                saturation >= 100 && value >= 100,
                "{}: {:?}",
                scheme.name,
                scheme.hsv
            );
            assert!(
                !entries[..index].contains(&entry),
                "{} listed twice",
                scheme.name
            );
        }

        // 8 stays the observer's default and a priority past the list keeps
        // the native pass-through.
        assert_eq!(mode.entry_for_priority(8), 2);
        assert_eq!(mode.scheme_index_for_priority(31), 31);
    }

    #[test]
    fn a_rules_list_replaces_the_built_in_one_and_skips_what_it_cannot_show() {
        let mut text = String::from("[Colors]\n");
        for index in 0..15 {
            text.push_str(&format!("Stock{index}={index},200,200\n"));
        }
        // Mine is the rules' own entry 15; VeraTeal is a built-in name the
        // rules redefine as entry 16; VeraLime comes from the built-in file.
        text.push_str(
            "Mine=10,20,30\nVeraTeal=1,2,3\n\
             [SkirmishColors]\n1=Mine\n2=none\n3=NoSuchColour\n4=veralime\n\
             5=Stock5\n6=MINE\n7=VeraTeal\n",
        );
        let ini = IniFile::from_str(&text);
        let mut schemes = crate::rules::color_scheme::parse_color_schemes(&ini);
        let mode = SkirmishColorMode::from_ini(&ini, &mut schemes);

        // `none` and the unknown name are skipped, Stock5 is YR's red, and the
        // second Mine repeats the first.
        assert_eq!(
            mode.lobby_priorities().collect::<Vec<_>>()[8..],
            [9, 10, 11]
        );
        assert_eq!(mode.entry_for_priority(9), 15);
        assert_eq!(mode.entry_for_priority(10), 17);
        assert_eq!(mode.entry_for_priority(11), 16);
        assert_eq!(schemes.len(), 18);
        assert_eq!(schemes[17].name, "VeraLime");
        assert_eq!(schemes[17].hsv, [54, 255, 235]);
        assert_eq!(schemes[16].hsv, [1, 2, 3]);
        // Past the list, the native tail still answers.
        assert_eq!(mode.entry_for_priority(12), 6);

        assert_eq!(mode.next_lobby_priority(7), 9);
        assert_eq!(mode.next_lobby_priority(11), 0);
        assert_eq!(mode.next_lobby_priority(8), 0);
    }
}
