//! The colours enemies are drawn in under VERA's team-colour view.
//!
//! The relation-colour view (Alt+A) paints every house by how it stands with
//! the local player: blue for your own side, green for allies, red for
//! everyone else. That red says "hostile" but not "which hostile", which is
//! what a hundred-player melee needs. The team-colour view (Alt+T) keeps the
//! blue and the green exactly as they are and splits the hostile side apart:
//! each enemy team draws in a colour of its own, and so does every enemy the
//! lobby left off every team.
//!
//! The colours come from the ones the lobby already offers — the rules'
//! `[Colors]` entries listed by `[SkirmishColors]` — so a mod's palette stays
//! in charge and a player recognises the swatch. Only the warm end of the
//! wheel is used: the ally band owns green, the self band owns blue, and a
//! hostile side that turned green would read as friendly.
//!
//! ## Who gets what
//!
//! - **Teams first, in order.** The enemy teams present, lowest first, take
//!   the palette in order: the first is the colour nearest red, and each next
//!   is the one farthest (RGB) from those already taken, so a four-team match
//!   gives three well-separated colours.
//! - **Teamless enemies share what is left, at random.** Each such house
//!   draws a colour from the entries no team holds; once every colour is
//!   held, it draws from the whole palette. The draw is a hash of the owner's
//!   name, so it is scattered across houses yet stable frame to frame and
//!   from one view to the next.
//! - **Past the palette, teams repeat at random.** A match with more enemy
//!   teams than warm colours — fifteen teams against a dozen colours in stock
//!   rules — reuses the palette the way the lobby's Random colour does once
//!   every colour is taken. Fifteen teams are the expected ceiling; beyond
//!   that the view still works, it just stops being able to tell every team
//!   apart, and Relation Colours is the view for that size of battle.
//!
//! ## Dependency rules
//! - Part of rules/ — depends on rules/color_scheme, rules/house_colors and
//!   rules/skirmish_colors only.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::color_scheme::{ColorSchemeEntry, hsv_to_rgb};
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::skirmish_colors::SkirmishColorMode;

/// The most enemy teams one match colours apart. Skirmish seats fifteen teams
/// (`skirmish_launch::SKIRMISH_TEAMS`); past that the palette repeats.
pub const MAX_TEAM_COLORS: usize = 15;

/// A hue at or below this is warm — red through yellow. Above it the wheel
/// runs through the ally band's green (85), so a colour there would read as a
/// teammate.
const WARM_END: u8 = 48;
/// A hue at or above this wraps past magenta into red, so it is warm too.
const WARM_START: u8 = 210;

/// Saturation and value floor: a grey or a near-black cannot mark a team.
const MIN_CHROMA: u8 = 90;

/// Two picked colours keep at least this Euclidean RGB distance (0..=441),
/// so a team is not handed a shade of the team beside it.
const MIN_SEPARATION: i32 = 60;

/// The warm colours a rules load can draw enemies in, in the order enemy
/// teams take them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnemyTeamPalette {
    entries: Vec<HouseColorIndex>,
}

impl EnemyTeamPalette {
    /// The palette for a rules load: `[Colors]` entries, enemy team A first.
    /// Empty when the lobby lists no warm colour at all, which leaves every
    /// enemy on the relation view's red.
    pub fn from_rules(colors: SkirmishColorMode, schemes: &[ColorSchemeEntry]) -> Self {
        let mut candidates: Vec<(usize, [u8; 3], [u8; 3])> = Vec::new();
        for priority in colors.lobby_priorities() {
            let entry = colors.entry_for_priority(i32::from(priority));
            let Some(scheme) = schemes.get(entry) else {
                continue;
            };
            if candidates.iter().any(|&(seen, _, _)| seen == entry) {
                continue;
            }
            let hsv = scheme.hsv;
            if !is_warm(hsv[0]) || hsv[1] < MIN_CHROMA || hsv[2] < MIN_CHROMA {
                continue;
            }
            candidates.push((entry, hsv, hsv_to_rgb(hsv)));
        }
        if candidates.is_empty() {
            return Self::default();
        }
        // The first enemy team takes the colour nearest red, whichever side of
        // the wrap it sits on; the entry index breaks ties the same way on
        // every load.
        candidates.sort_by_key(|&(entry, hsv, _)| (red_distance(hsv[0]), entry));

        let mut picked: Vec<(usize, [u8; 3])> = vec![(candidates[0].0, candidates[0].2)];
        while picked.len() < MAX_TEAM_COLORS && picked.len() < candidates.len() {
            let next = candidates
                .iter()
                .filter(|(entry, _, _)| !picked.iter().any(|&(taken, _)| taken == *entry))
                .map(|&(entry, _, rgb)| (entry, rgb, separation(rgb, &picked)))
                .max_by_key(|&(entry, _, distance)| (distance, std::cmp::Reverse(entry)));
            let Some((entry, rgb, distance)) = next else {
                break;
            };
            if distance < MIN_SEPARATION * MIN_SEPARATION {
                break;
            }
            picked.push((entry, rgb));
        }
        Self {
            entries: picked
                .into_iter()
                .map(|(entry, _)| HouseColorIndex(u8::try_from(entry).unwrap_or(0)))
                .collect(),
        }
    }

    /// A palette of exactly these entries, for tests.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_entries(entries: &[HouseColorIndex]) -> Self {
        Self {
            entries: entries.to_vec(),
        }
    }

    /// The entry list enemies draw from, team A first.
    pub fn entries(&self) -> &[HouseColorIndex] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The colour of the `rank`-th enemy team (0 = the first one present): the
    /// palette in order while it lasts, then a colour drawn at random, as the
    /// lobby's own Random colour reuses one once every colour is taken.
    pub fn team_color(&self, rank: usize) -> Option<HouseColorIndex> {
        if self.entries.is_empty() {
            return None;
        }
        let index = if rank < self.entries.len() {
            rank
        } else {
            stable_index(&format!("team{rank}"), self.entries.len())
        };
        self.entries.get(index).copied()
    }

    /// The colour of a house on no team: one no team holds yet, so a teamless
    /// enemy stays distinguishable from the teams for as long as the palette
    /// allows, then any entry at all. `taken` is every colour already handed
    /// out in this view. Chosen from `owner`, so the same house keeps its
    /// colour from frame to frame.
    pub fn teamless_color(
        &self,
        owner: &str,
        taken: &[HouseColorIndex],
    ) -> Option<HouseColorIndex> {
        if self.entries.is_empty() {
            return None;
        }
        let free: Vec<HouseColorIndex> = self
            .entries
            .iter()
            .copied()
            .filter(|entry| !taken.contains(entry))
            .collect();
        let pool = if free.is_empty() {
            &self.entries
        } else {
            &free
        };
        pool.get(stable_index(owner, pool.len())).copied()
    }
}

/// The rank each enemy team draws at, 0 first: the teams present, lowest team
/// first, without the local player's own team. Launch teams never change, but
/// which of them count as hostile does, so a team's rank moves when another
/// team leaves the hostile side.
pub fn enemy_team_ranks(
    teams: impl IntoIterator<Item = u8>,
    local_team: Option<u8>,
) -> BTreeMap<u8, usize> {
    let mut present: BTreeSet<u8> = teams.into_iter().collect();
    if let Some(local) = local_team {
        present.remove(&local);
    }
    present
        .into_iter()
        .enumerate()
        .map(|(rank, team)| (team, rank))
        .collect()
}

/// A stable index into `len` choices for `key`: FNV-1a over the bytes, so the
/// same house lands on the same colour on every frame and every machine.
fn stable_index(key: &str, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % len as u64) as usize
}

/// Whether a hue sits on the warm end of the wheel, clear of the ally band's
/// green and the self band's blue.
fn is_warm(hue: u8) -> bool {
    hue <= WARM_END || hue >= WARM_START
}

/// How far a hue lies from red, in hue steps: red is 0, and the wrap past
/// 255 approaches it from the other side, so 250 is nearer red than 210.
fn red_distance(hue: u8) -> i16 {
    let hue = i16::from(hue);
    hue.min(255 - hue)
}

/// Squared RGB distance from the nearest colour already picked.
fn separation(rgb: [u8; 3], picked: &[(usize, [u8; 3])]) -> i32 {
    picked
        .iter()
        .map(|&(_, other)| {
            let dr = i32::from(rgb[0]) - i32::from(other[0]);
            let dg = i32::from(rgb[1]) - i32::from(other[1]);
            let db = i32::from(rgb[2]) - i32::from(other[2]);
            dr * dr + dg * dg + db * db
        })
        .min()
        .unwrap_or(0)
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;

    fn scheme(name: &str, hsv: [u8; 3]) -> ColorSchemeEntry {
        ColorSchemeEntry {
            name: name.to_owned(),
            hsv,
        }
    }

    /// Retail `[Colors]` plus VERA's own list, as a rules load builds them:
    /// the eight priorities and the extras the lobby offers.
    fn retail_like() -> (SkirmishColorMode, Vec<ColorSchemeEntry>) {
        let raw: &[(&str, [u8; 3])] = &[
            ("LightGold", [25, 255, 255]),
            ("Gold", [43, 239, 255]),
            ("LightGrey", [0, 0, 240]),
            ("Grey", [0, 0, 131]),
            ("Red", [20, 255, 184]),
            ("DarkRed", [0, 230, 255]),
            ("Orange", [25, 230, 255]),
            ("Magenta", [221, 102, 255]),
            ("Purple", [201, 201, 189]),
            ("LightBlue", [119, 143, 255]),
            ("DarkBlue", [153, 214, 212]),
            ("NeonBlue", [185, 156, 238]),
            ("DarkSky", [131, 200, 230]),
            ("Green", [104, 241, 195]),
            ("DarkGreen", [81, 200, 210]),
            ("NeonGreen", [0, 0, 0]),
            ("Yellow", [38, 159, 255]),
            ("VeraBurgundy", [250, 255, 120]),
            ("VeraRose", [249, 135, 252]),
            ("VeraHotPink", [227, 255, 255]),
            ("VeraPlum", [229, 255, 170]),
            ("VeraBrown", [23, 255, 144]),
            ("VeraTan", [18, 150, 195]),
            ("VeraBronze", [41, 240, 190]),
            ("VeraLime", [54, 255, 235]),
            ("VeraOlive", [49, 159, 140]),
            ("VeraNeonGreen", [84, 255, 255]),
            ("VeraForest", [86, 251, 120]),
            ("VeraMint", [107, 120, 255]),
            ("VeraSeaGreen", [113, 210, 200]),
            ("VeraTeal", [123, 255, 145]),
            ("VeraAzure", [138, 255, 215]),
            ("VeraRoyalBlue", [168, 255, 215]),
            ("VeraNavy", [165, 210, 125]),
            ("VeraSteelBlue", [139, 240, 125]),
            ("VeraPeriwinkle", [166, 120, 255]),
            ("VeraViolet", [188, 255, 255]),
            ("VeraLavender", [185, 120, 175]),
            ("VeraOrchid", [204, 165, 255]),
        ];
        let schemes: Vec<ColorSchemeEntry> =
            raw.iter().map(|&(name, hsv)| scheme(name, hsv)).collect();
        // The lobby lists YR's eight (priorities 0..=7) and the built-in
        // extras, which here are the entries past the retail fifteen.
        let extras: Vec<u8> = (16..38).collect();
        (SkirmishColorMode::with_extra_entries(&extras), schemes)
    }

    fn palette() -> (EnemyTeamPalette, Vec<ColorSchemeEntry>) {
        let (colors, schemes) = retail_like();
        (EnemyTeamPalette::from_rules(colors, &schemes), schemes)
    }

    /// The palette stays on the warm end, never repeats an entry, and starts
    /// at red.
    #[test]
    fn the_palette_is_warm_distinct_and_starts_at_red() {
        let (palette, schemes) = palette();
        let entries = palette.entries();
        assert!(!entries.is_empty(), "retail rules list warm colours");
        assert!(entries.len() <= MAX_TEAM_COLORS);

        let mut seen = Vec::new();
        for (slot, &entry) in entries.iter().enumerate() {
            assert!(!seen.contains(&entry), "slot {slot} repeats an entry");
            seen.push(entry);
            let hsv = schemes[usize::from(entry.0)].hsv;
            assert!(
                is_warm(hsv[0]),
                "slot {slot} takes a hue outside the warm end: {hsv:?}"
            );
            assert!(hsv[1] >= MIN_CHROMA && hsv[2] >= MIN_CHROMA);
        }
        // Slot 0 sits nearest red.
        let first = schemes[usize::from(entries[0].0)].hsv[0];
        let nearest = entries
            .iter()
            .map(|entry| red_distance(schemes[usize::from(entry.0)].hsv[0]))
            .min()
            .unwrap();
        assert_eq!(red_distance(first), nearest);
    }

    /// Four teams: the three enemies take the first three colours, red first,
    /// and the local team's own slot is skipped rather than consumed.
    #[test]
    fn enemy_teams_rank_from_red_whatever_their_team_number() {
        let (palette, schemes) = palette();
        let ranks = enemy_team_ranks([0, 1, 2, 3], Some(0));
        assert_eq!(ranks.get(&1), Some(&0));
        assert_eq!(ranks.get(&2), Some(&1));
        assert_eq!(ranks.get(&3), Some(&2));
        assert_eq!(ranks.get(&0), None, "the local team draws no enemy colour");

        let first = palette.team_color(0).unwrap();
        assert_eq!(first, palette.entries()[0]);
        // The first entry is the palette's nearest red, so one enemy team
        // draws the colour the relation view gives every enemy.
        let first_hue = schemes[usize::from(first.0)].hsv[0];
        assert!(red_distance(first_hue) <= 8, "team A reads as red");
    }

    /// Past the palette, teams reuse it at random rather than going blank,
    /// and the same rank always lands on the same entry.
    #[test]
    fn teams_past_the_palette_repeat_stably() {
        let (palette, _) = palette();
        let len = palette.entries().len();
        for rank in len..len + 20 {
            let color = palette.team_color(rank).expect("a reused entry");
            assert!(palette.entries().contains(&color));
            assert_eq!(palette.team_color(rank), Some(color), "stable per rank");
        }
    }

    /// Teamless enemies take colours no team holds while any are free, and
    /// keep their own from frame to frame.
    #[test]
    fn teamless_houses_take_free_colours_then_share() {
        let (palette, _) = palette();
        let entries = palette.entries().to_vec();
        let mut taken: Vec<HouseColorIndex> = vec![entries[0], entries[1]];
        let mut handed = Vec::new();
        for owner in ["Computer1", "Computer2", "Computer3"] {
            let color = palette.teamless_color(owner, &taken).unwrap();
            assert!(entries.contains(&color));
            assert!(!taken.contains(&color), "{owner} takes a free colour");
            assert_eq!(
                palette.teamless_color(owner, &taken),
                Some(color),
                "stable per owner"
            );
            taken.push(color);
            handed.push(color);
        }
        // Every colour taken: the draw falls back to the whole palette and
        // still answers.
        let full: Vec<HouseColorIndex> = entries.clone();
        assert!(palette.teamless_color("Computer4", &full).is_some());
    }

    /// A rules load with nothing warm leaves the relation red alone instead of
    /// inventing colours.
    #[test]
    fn a_palette_without_warm_colours_is_empty() {
        let schemes = vec![
            scheme("Green", [85, 230, 255]),
            scheme("Blue", [170, 230, 255]),
        ];
        let palette =
            EnemyTeamPalette::from_rules(SkirmishColorMode::with_extra_entries(&[]), &schemes);
        assert!(palette.is_empty());
        assert_eq!(palette.team_color(0), None);
        assert_eq!(palette.teamless_color("Computer1", &[]), None);
    }

    /// The same rules give the same palette and the same ranks.
    #[test]
    fn the_palette_is_deterministic() {
        let (colors, schemes) = retail_like();
        assert_eq!(
            EnemyTeamPalette::from_rules(colors, &schemes),
            EnemyTeamPalette::from_rules(colors, &schemes)
        );
        assert_eq!(
            enemy_team_ranks([3, 1, 2], Some(9)),
            enemy_team_ranks([1, 2, 3], None)
        );
    }
}
