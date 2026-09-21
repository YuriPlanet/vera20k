//! Infantry animation sequence definitions parsed from art.ini.
//!
//! RA2 art.ini assigns each infantry type a `Sequence=` key (e.g., `Sequence=ConSequence`)
//! pointing to a `[ConSequence]` section that defines the SHP frame layout for every
//! animation: stand, walk, fire, idle, die, crawl, prone, etc.
//!
//! Format per key: `Walk=8,6,6` means start=8, count=6, facing stride=6.
//! An optional 4th field is a facing direction hint: `Idle1=56,15,0,S` (face South).
//!
//! Each InfantryType owns 42 signed action records. Sequence section names are
//! arbitrary, and partial reads retain fields from an earlier rules pass.
//!
//! ## Dependency rules
//! - Part of rules/ — depends only on rules/ini_parser.
//! - Does NOT depend on sim/, render/, ui/, or any game module.

use std::collections::HashMap;

use crate::rules::animation_sequence::{
    FacingSlots, LoopMode, SequenceDef, SequenceKind, SequenceSet,
};
use crate::rules::ini_parser::IniFile;

/// Bytes 0 and 3 of gamemd's 42 four-byte infantry action records at
/// 0x007EAF7C (re-read from the binary 2026-09-15; all 42 rows match).
/// Mission readiness, Scatter and DoAction have distinct caller bypasses;
/// they share these records without borrowing each other's admission policy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InfantryActionRecord {
    pub(crate) interruptible: bool,
    pub(crate) frame_delay: u8,
}

const INFANTRY_ACTIONS: [InfantryActionRecord; 42] = [
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 0,
    }, // 0
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 0,
    }, // 1
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 6,
    }, // 2
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 3,
    }, // 3
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 4
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 5
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 6
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 7
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 8
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 3,
    }, // 9
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 3,
    }, // 10
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 11
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 12
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 13
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 14
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 15
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 3,
    }, // 16
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 17
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 3,
    }, // 18
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 3,
    }, // 19
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 20
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 21
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 22
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 2,
    }, // 23
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 24
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 25
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 26
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 27
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 28
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 29
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 30
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 31
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 3,
    }, // 32
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 33
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 3,
    }, // 34
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 1,
    }, // 35
    InfantryActionRecord {
        interruptible: false,
        frame_delay: 3,
    }, // 36
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 4,
    }, // 37
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 6,
    }, // 38
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 3,
    }, // 39
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 40
    InfantryActionRecord {
        interruptible: true,
        frame_delay: 1,
    }, // 41
];

pub(crate) fn action_record(action: i32) -> Option<&'static InfantryActionRecord> {
    usize::try_from(action)
        .ok()
        .and_then(|index| INFANTRY_ACTIONS.get(index))
}

const NORMALIZED_ACTIONS: [u8; 6] = [0x09, 0x0A, 0x12, 0x13, 0x17, 0x20];

/// Compass direction applied when this action completes. The constructor's
/// native default is -1; the optional fourth INI token replaces it with 0..=7.
/// Retail provenance: `InfantryTypeClass` constructor @ `0x005236A0` and
/// `ReadSequenceData` @ `0x00523D00`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FacingHint {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

/// One animation entry parsed from an INI value like `"8,6,6"` or `"56,15,0,S"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct InfantrySequenceEntry {
    /// First SHP frame index for this animation.
    pub start_frame: i32,
    /// Number of animation frames per facing direction.
    pub frames_per_facing: i32,
    /// Native signed frame stride between facing slots (not a facing count).
    pub facings: i32,
    /// Optional facing direction hint for non-directional animations.
    pub facing_hint: Option<FacingHint>,
}

impl Default for InfantrySequenceEntry {
    fn default() -> Self {
        // InfantryType constructor52392C..523970, all42 records.
        Self {
            start_frame: 0,
            frames_per_facing: 0,
            facings: 0,
            facing_hint: None,
        }
    }
}

/// Original action-name table8255C8..825670. Guard is its own action1.
pub(crate) const NATIVE_SEQUENCE_NAMES: [&str; 42] = [
    "Ready",
    "Guard",
    "Prone",
    "Walk",
    "FireUp",
    "Down",
    "Crawl",
    "Up",
    "FireProne",
    "Idle1",
    "Idle2",
    "Die1",
    "Die2",
    "Die3",
    "Die4",
    "Die5",
    "Tread",
    "Swim",
    "WetIdle1",
    "WetIdle2",
    "WetDie1",
    "WetDie2",
    "WetAttack",
    "Hover",
    "Fly",
    "Tumble",
    "FireFly",
    "Deploy",
    "Deployed",
    "DeployedFire",
    "DeployedIdle",
    "Undeploy",
    "Cheer",
    "Paradrop",
    "AirDeathStart",
    "AirDeathFalling",
    "AirDeathFinish",
    "Panic",
    "Shovel",
    "Carry",
    "SecondaryFire",
    "SecondaryProne",
];

/// All animation entries from one section, keyed by INI key name (uppercase).
#[derive(Debug, Clone)]
pub struct InfantrySequenceDef {
    /// Animation entries keyed by uppercase INI key (e.g., "WALK", "FIREUP", "IDLE1").
    pub entries: HashMap<String, InfantrySequenceEntry>,
}

/// Registry of all parsed sequence sections, keyed by uppercase section name.
pub type InfantrySequenceRegistry = HashMap<String, InfantrySequenceDef>;

/// Parse a single sequence value string like `"8,6,6"` or `"56,15,0,S"`.
///
/// Returns `None` for an empty ReadString result. Partial conversions retain
/// constructor values; malformed nonempty input still returns that record.
#[cfg(test)]
pub fn parse_sequence_value(value: &str) -> Option<InfantrySequenceEntry> {
    let value = crate::rules::ini_value::truncate_bytes(value, 31);
    if crate::rules::ini_value::strtrim_ascii(value).is_empty() {
        return None;
    }
    let mut record = InfantrySequenceEntry::default();
    read_sequence_value(value, &mut record);
    Some(record)
}

/// Original523D00 partial sscanf retains every field whose conversion fails.
/// ReadString's31-byte copy occurs before trimming; literal commas skip no
/// whitespace. Corpus: tools/spatial_oracle/infantry_sequence_rules.
fn read_sequence_value(value: &str, record: &mut InfantrySequenceEntry) {
    let value = crate::rules::ini_value::truncate_bytes(value, 31);
    let mut bytes = crate::rules::ini_value::strtrim_ascii(value).as_bytes();
    for slot in [
        &mut record.start_frame,
        &mut record.frames_per_facing,
        &mut record.facings,
    ] {
        let Some(value) = crate::rules::ini_value::scan_decimal_i32(&mut bytes) else {
            return;
        };
        *slot = value;
        if bytes.first() != Some(&b',') {
            return;
        }
        bytes = &bytes[1..];
    }
    let token = bytes
        .split(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 11 | 12))
        .find(|token| !token.is_empty())
        .unwrap_or_default();
    if let Ok(token) = std::str::from_utf8(token)
        && let Some(hint) = parse_facing_hint(token)
    {
        record.facing_hint = Some(hint);
    }
}

/// Parse a facing direction hint string (e.g., "S", "NE", "W").
fn parse_facing_hint(s: &str) -> Option<FacingHint> {
    match s {
        "N" => Some(FacingHint::N),
        "NE" => Some(FacingHint::NE),
        "E" => Some(FacingHint::E),
        "SE" => Some(FacingHint::SE),
        "S" => Some(FacingHint::S),
        "SW" => Some(FacingHint::SW),
        "W" => Some(FacingHint::W),
        "NW" => Some(FacingHint::NW),
        _ => None,
    }
}

fn completion_facing(hint: Option<FacingHint>) -> Option<u8> {
    match hint {
        Some(FacingHint::N) => Some(0),
        Some(FacingHint::NE) => Some(32),
        Some(FacingHint::E) => Some(64),
        Some(FacingHint::SE) => Some(96),
        Some(FacingHint::S) => Some(128),
        Some(FacingHint::SW) => Some(160),
        Some(FacingHint::W) => Some(192),
        Some(FacingHint::NW) => Some(224),
        None => None,
    }
}

/// Parse all infantry sequence definition sections from art.ini.
///
/// Section names are arbitrary. Only references bound by an Infantry type's
/// Sequence key become type data; the table reads exactly42 named actions.
pub fn parse_infantry_sequence_registry(ini: &IniFile) -> InfantrySequenceRegistry {
    let mut registry: InfantrySequenceRegistry = HashMap::new();

    for section_name in ini.section_names() {
        let section = match ini.section(section_name) {
            Some(s) => s,
            None => continue,
        };

        let mut entries: HashMap<String, InfantrySequenceEntry> = HashMap::new();

        for key in NATIVE_SEQUENCE_NAMES {
            let value = match section.get(key) {
                Some(v) => v,
                None => continue,
            };

            let mut entry = InfantrySequenceEntry::default();
            if let Some(values) = section.projected_values(key) {
                for value in values {
                    read_sequence_value(value, &mut entry);
                }
            } else {
                read_sequence_value(value, &mut entry);
            }
            entries.insert(key.to_ascii_uppercase(), entry);
        }

        if !entries.is_empty() {
            registry.insert(section_name.to_uppercase(), InfantrySequenceDef { entries });
        }
    }

    log::info!(
        "InfantrySequenceRegistry: {} sequence definitions loaded from art.ini",
        registry.len()
    );
    registry
}

/// Map an INI key name (e.g., "Ready", "Walk", "FireUp") to the engine's SequenceKind.
///
/// Returns None for keys the engine doesn't yet support (Fly, Swim, Deploy, etc.).
pub fn sequence_kind_from_ini_key(key: &str) -> Option<SequenceKind> {
    match key.to_uppercase().as_str() {
        "READY" | "GUARD" => Some(SequenceKind::Stand),
        "WALK" => Some(SequenceKind::Walk),
        "PRONE" => Some(SequenceKind::Prone),
        "CRAWL" => Some(SequenceKind::Crawl),
        "FIREUP" => Some(SequenceKind::Attack),
        "FIREPRONE" => Some(SequenceKind::FireProne),
        "DOWN" => Some(SequenceKind::Down),
        "UP" => Some(SequenceKind::Up),
        "IDLE1" => Some(SequenceKind::Idle1),
        "IDLE2" => Some(SequenceKind::Idle2),
        "DIE1" => Some(SequenceKind::Die1),
        "DIE2" => Some(SequenceKind::Die2),
        "DIE3" => Some(SequenceKind::Die3),
        "DIE4" => Some(SequenceKind::Die4),
        "DIE5" => Some(SequenceKind::Die5),
        "CHEER" => Some(SequenceKind::Cheer),
        "PARADROP" => Some(SequenceKind::Paradrop),
        "PANIC" => Some(SequenceKind::Panic),
        "DEPLOY" => Some(SequenceKind::Deploy),
        "UNDEPLOY" => Some(SequenceKind::Undeploy),
        "DEPLOYED" => Some(SequenceKind::Deployed),
        "DEPLOYEDFIRE" => Some(SequenceKind::DeployedFire),
        "DEPLOYEDIDLE" => Some(SequenceKind::DeployedIdle),
        "SECONDARYFIRE" => Some(SequenceKind::SecondaryFire),
        "SECONDARYPRONE" => Some(SequenceKind::SecondaryProne),
        "SWIM" => Some(SequenceKind::Swim),
        "FLY" => Some(SequenceKind::Fly),
        "FIREFLY" => Some(SequenceKind::FireFly),
        "HOVER" => Some(SequenceKind::Hover),
        "TREAD" => Some(SequenceKind::Tread),
        "WETATTACK" => Some(SequenceKind::WetAttack),
        "WETIDLE1" => Some(SequenceKind::WetIdle1),
        "WETIDLE2" => Some(SequenceKind::WetIdle2),
        _ => None,
    }
}

/// Native action id for a sequence, i.e. its slot in the 42-entry action table.
///
/// The ids are the index order of the engine's own sequence-name array, not the
/// alphabetical or INI order. They are only meaningful as indices into
/// `INFANTRY_ACTIONS` / `NORMALIZED_ACTIONS`, so an id that is off by even one
/// hands the sequence another action's playback speed. That is exactly what the
/// water, flight, deploy-adjacent and secondary-weapon rows did before this
/// table was walked out of the binary: `SecondaryFire` in particular ran three
/// times slower than retail, and because an infantryman discharges his weapon on
/// a specific frame of the fire sequence rather than at its start, that slowed
/// the Brute's building-smash rate to a third.
///
/// Ids 20/21 (WetDie1/WetDie2), 25 (Tumble), 34–36 (AirDeath*) and 38/39
/// (Shovel/Carry) have no `SequenceKind` yet and so are absent below.
pub(crate) fn action_id(kind: SequenceKind) -> u8 {
    match kind {
        SequenceKind::Stand => 0,
        SequenceKind::Prone => 2,
        SequenceKind::Walk => 3,
        SequenceKind::Attack => 4,
        SequenceKind::Down => 5,
        SequenceKind::Crawl => 6,
        SequenceKind::Up => 7,
        SequenceKind::FireProne => 8,
        SequenceKind::Idle1 => 9,
        SequenceKind::Idle2 => 10,
        SequenceKind::Die1 => 11,
        SequenceKind::Die2 => 12,
        SequenceKind::Die3 => 13,
        SequenceKind::Die4 => 14,
        SequenceKind::Die5 => 15,
        SequenceKind::Tread => 16,
        SequenceKind::Swim => 17,
        SequenceKind::WetIdle1 => 18,
        SequenceKind::WetIdle2 => 19,
        SequenceKind::WetAttack => 22,
        SequenceKind::Hover => 23,
        SequenceKind::Fly => 24,
        SequenceKind::FireFly => 26,
        SequenceKind::Deploy => 27,
        SequenceKind::Deployed => 28,
        SequenceKind::DeployedFire => 29,
        SequenceKind::DeployedIdle => 30,
        SequenceKind::Undeploy => 31,
        SequenceKind::Cheer => 32,
        SequenceKind::Paradrop => 33,
        SequenceKind::Panic => 37,
        SequenceKind::SecondaryFire => 40,
        SequenceKind::SecondaryProne => 41,
    }
}

fn action_timing(kind: SequenceKind) -> (u16, bool) {
    let id = action_id(kind);
    (
        u16::from(INFANTRY_ACTIONS[id as usize].frame_delay),
        NORMALIZED_ACTIONS.contains(&id),
    )
}

/// Get the default LoopMode for a given SequenceKind.
fn default_loop_mode(kind: SequenceKind) -> LoopMode {
    match kind {
        SequenceKind::Stand
        | SequenceKind::Walk
        | SequenceKind::Crawl
        | SequenceKind::Prone
        | SequenceKind::Deployed
        | SequenceKind::Panic
        | SequenceKind::Swim
        | SequenceKind::Fly
        | SequenceKind::Hover
        | SequenceKind::Tread => LoopMode::Loop,
        SequenceKind::Die1
        | SequenceKind::Die2
        | SequenceKind::Die3
        | SequenceKind::Die4
        | SequenceKind::Die5
        | SequenceKind::Paradrop => LoopMode::HoldLast,
        SequenceKind::Attack
        | SequenceKind::FireProne
        | SequenceKind::SecondaryFire
        | SequenceKind::SecondaryProne => LoopMode::TransitionTo(SequenceKind::Stand),
        SequenceKind::DeployedFire => LoopMode::TransitionTo(SequenceKind::Deployed),
        SequenceKind::FireFly => LoopMode::TransitionTo(SequenceKind::Fly),
        SequenceKind::WetAttack => LoopMode::TransitionTo(SequenceKind::Swim),
        SequenceKind::Idle1 | SequenceKind::Idle2 | SequenceKind::Cheer => {
            LoopMode::TransitionTo(SequenceKind::Stand)
        }
        SequenceKind::DeployedIdle => LoopMode::TransitionTo(SequenceKind::Deployed),
        SequenceKind::WetIdle1 | SequenceKind::WetIdle2 => {
            LoopMode::TransitionTo(SequenceKind::Swim)
        }
        SequenceKind::Down => LoopMode::TransitionTo(SequenceKind::Prone),
        SequenceKind::Up | SequenceKind::Undeploy => LoopMode::TransitionTo(SequenceKind::Stand),
        SequenceKind::Deploy => LoopMode::TransitionTo(SequenceKind::Deployed),
    }
}

/// Standard number of facing directions for infantry.
const INFANTRY_FACINGS: u8 = 8;

/// Convert a parsed INI sequence definition into the engine's SequenceSet.
///
/// Maps known INI keys (Ready, Walk, FireUp, etc.) to SequenceKind variants.
/// Unknown keys are silently skipped — they can be added to SequenceKind later
/// when the engine supports those gameplay systems.
///
/// The INI 3rd field is the FacingMultiplier (stride between facings), NOT the
/// facing count. For directional sequences the actual facing count is always 8
/// (standard infantry). For non-directional sequences (multiplier=0), facings=1.
pub fn build_sequence_set(def: &InfantrySequenceDef) -> SequenceSet {
    let mut set: SequenceSet = SequenceSet::new();
    // One signed bank supplies gameplay admission/completion. The generic
    // SequenceDef map below is a derived projection for existing SHP consumers.
    set.set_infantry_actions(
        NATIVE_SEQUENCE_NAMES
            .iter()
            .map(|name| {
                def.entries
                    .get(&name.to_ascii_uppercase())
                    .copied()
                    .unwrap_or_default()
            })
            .collect(),
    );
    let mut entries: Vec<(&String, &InfantrySequenceEntry)> = def.entries.iter().collect();
    entries.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));

    for (key, entry) in entries {
        // Stand projects Ready0. Guard1 remains independent in the signed bank.
        if key == "GUARD" {
            continue;
        }
        let kind: SequenceKind = match sequence_kind_from_ini_key(key) {
            Some(k) => k,
            None => continue,
        };

        // Any future aliases use stable lexical precedence.
        if set.get(&kind).is_some() {
            continue;
        }

        // INI 3rd field is FacingMultiplier (stride), not facing count.
        // Multiplier=0 → non-directional (facings=1).
        // Multiplier>0 → directional with 8 infantry facings.
        // The existing generic SHP interface represents u16 asset indices.
        // It must never narrow the signed bank used by gameplay. Wider/negative
        // native draw selection remains a separate presentation migration.
        let (Ok(start_frame), Ok(frame_count), Ok(stride)) = (
            u16::try_from(entry.start_frame),
            u16::try_from(entry.frames_per_facing),
            u16::try_from(entry.facings),
        ) else {
            continue;
        };
        let (facings, facing_multiplier): (u8, u16) = if entry.facings == 0 {
            (1, 0)
        } else {
            (INFANTRY_FACINGS, stride)
        };

        let (frame_delay, normalized) = action_timing(kind);
        set.insert(
            kind,
            SequenceDef {
                start_frame,
                frame_count,
                facings,
                facing_multiplier,
                frame_delay,
                normalized,
                completion_facing: completion_facing(entry.facing_hint),
                loop_mode: default_loop_mode(kind),
                facing_slots: FacingSlots::InfantryTable,
            },
        );
    }

    set
}

#[cfg(test)]
#[path = "infantry_sequence_tests.rs"]
mod tests;

/// Infantry Scatter 0x51D1AA..0x51D1C3 reads byte 0 of the 42 Doing records
/// at 0x007EAF7C. The -1 and 0x1F bypasses precede that lookup.
/// This is only the immutable permission leaf; each caller owns Doing's producer.
pub(crate) fn scatter_allowed_by_doing(doing: i32) -> Option<bool> {
    if doing == -1 || doing == 31 {
        return Some(true);
    }
    action_record(doing).map(|record| record.interruptible)
}
