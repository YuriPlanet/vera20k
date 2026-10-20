//! Rules-owned mission selector vocabulary and immutable control data.
//!
//! The native 32-name table is shared by scenario MISSION lookup and
//! mission control-section parsing, so both live here as one static rules-data
//! boundary. Runtime selectors, transition state, dispatch timers, authority,
//! and handler execution remain in sim::mission.
//!
//! Float appears only here, at parse time — the per-minute control rates are
//! pre-converted to integer frames so no float ever reaches a tick path.
//!
//! ## Dependency rules
//! - Part of rules/; parses INI values and uses the shared numeric conversion owner.
//! - No dependency on sim/ or runtime scheduling.

use crate::rules::ini_parser::IniFile;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Number of dispatched mission ids (0..=31). The `None` sentinel is outside
/// this range and is never iterated by [`MissionType::all`].
pub const MISSION_COUNT: usize = 32;

/// The canonical mission selector. Discriminants 0..=31 equal the wire mission
/// id; `None = 0xFF` is the idle sentinel. `repr(u16)` so the discriminant folds
/// stably into the state hash in later slices.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[repr(u16)]
pub enum MissionType {
    /// No committed mission (idle). Sentinel discriminant `0xFF`; the `Default`.
    #[default]
    None = 0xFF,
    Sleep = 0,
    Attack = 1,
    Move = 2,
    /// Index 3, `"QMove"`. **Dormant in stock YR, proven — do not write a
    /// handler for it.** The dispatcher has no case for it (it falls through
    /// the jump table's `default` arm to the same 450-frame base stub `Sleep(0)`
    /// uses), and a whole-image sweep of the mission-assign, mission-queue and
    /// player-order call sites found no writer and no reader of the id
    /// anywhere: the client's own order verb emits only
    /// {Attack, Move, Enter, Capture, Eaten, Harvest, Area Guard, Unload,
    /// Sabotage, Patrol}. Stock YR's queued/deferred movement is **Planning
    /// Mode** — a separate bindable command class that commits over its own
    /// network events and parks each unit on `Deliberate`(28, `"Wait"` in the
    /// INI table) plus a planning-token route. `[QMove] Rate=.016` is parsed and
    /// never consumed, exactly as in the original. Round-trips for map-INI name
    /// fidelity only.
    QMove = 3,
    Retreat = 4,
    Guard = 5,
    Sticky = 6,
    Enter = 7,
    Capture = 8,
    /// Grinder approach. Script60 queues this mission; PerCell consumes the
    /// arriving Foot (4DFA70, Infantry51973C).
    Eaten = 9,
    Harvest = 10,
    AreaGuard = 11,
    Return = 12,
    Stop = 13,
    /// Dead stub in YR (no live assigner). Round-trips for map-INI name
    /// fidelity. Its dispatch slot is a *distinct* base stub from Sleep's —
    /// the return value coincides (450 frames, do nothing) but a subclass
    /// could override one without the other.
    Ambush = 14,
    Hunt = 15,
    Unload = 16,
    Sabotage = 17,
    Construction = 18,
    Selling = 19,
    Repair = 20,
    /// Live AI-only behavior: idle teammates are tasked to converge on an
    /// attacker. A real handler is required; it is never player-assigned.
    Rescue = 21,
    Missile = 22,
    Harmless = 23,
    Open = 24,
    Patrol = 25,
    ParadropApproach = 26,
    ParadropOverfly = 27,
    /// Index 28. Named "Wait" in the INI mission-name table and "Deliberate"
    /// in unit reports — one mission. The guard-protected interrupt mission.
    Deliberate = 28,
    /// Index 29. The dispatcher has no *case* for it: like `QMove(3)` and
    /// every out-of-range id, it falls through the jump table's `default` arm
    /// to the same base handler `Sleep(0)` uses, which re-arms the dispatch
    /// timer with a flat 450 frames. VERA resolves attack-move upstream as a
    /// standing order and never commits this selector, so the arm is dead
    /// either way — but the dispatcher does NOT skip it.
    AttackMove = 29,
    SpyplaneApproach = 30,
    SpyplaneOverfly = 31,
}

impl MissionType {
    /// The wire mission id (0..=31; `0xFF` for `None`).
    #[cfg(test)]
    #[inline]
    pub fn id(self) -> u8 {
        self as u8
    }

    /// Alias of `MissionType::id` for dispatch call sites.
    #[cfg(any(test, feature = "test-support"))]
    #[inline]
    pub fn dispatch_id(self) -> u8 {
        self as u8
    }

    /// Map a wire id to its mission. Explicit match (no transmute) so a
    /// malformed map byte `>= 32` yields `None` rather than UB — lockstep-safe.
    pub fn from_id(id: u8) -> Option<Self> {
        Some(match id {
            0 => Self::Sleep,
            1 => Self::Attack,
            2 => Self::Move,
            3 => Self::QMove,
            4 => Self::Retreat,
            5 => Self::Guard,
            6 => Self::Sticky,
            7 => Self::Enter,
            8 => Self::Capture,
            9 => Self::Eaten,
            10 => Self::Harvest,
            11 => Self::AreaGuard,
            12 => Self::Return,
            13 => Self::Stop,
            14 => Self::Ambush,
            15 => Self::Hunt,
            16 => Self::Unload,
            17 => Self::Sabotage,
            18 => Self::Construction,
            19 => Self::Selling,
            20 => Self::Repair,
            21 => Self::Rescue,
            22 => Self::Missile,
            23 => Self::Harmless,
            24 => Self::Open,
            25 => Self::Patrol,
            26 => Self::ParadropApproach,
            27 => Self::ParadropOverfly,
            28 => Self::Deliberate,
            29 => Self::AttackMove,
            30 => Self::SpyplaneApproach,
            31 => Self::SpyplaneOverfly,
            _ => return None,
        })
    }

    /// The `[<MissionName>]` INI section header for this mission's control entry.
    ///
    /// These are the literal strings from the original's mission-name pointer
    /// table, which is what its `MissionControl` reader hands to the section
    /// lookup. Six of the 32 contain a space — `Area Guard`, `Paradrop
    /// Approach`, `Paradrop Overfly`, `Attack Move`, `Spyplane Approach`,
    /// `Spyplane Overfly` — and must be spelled with it or a mod/map INI
    /// declaring one is silently ignored.
    pub fn ini_section(self) -> &'static str {
        match self {
            Self::Sleep => "Sleep",
            Self::Attack => "Attack",
            Self::Move => "Move",
            Self::QMove => "QMove",
            Self::Retreat => "Retreat",
            Self::Guard => "Guard",
            Self::Sticky => "Sticky",
            Self::Enter => "Enter",
            Self::Capture => "Capture",
            Self::Eaten => "Eaten",
            Self::Harvest => "Harvest",
            Self::AreaGuard => "Area Guard",
            Self::Return => "Return",
            Self::Stop => "Stop",
            Self::Ambush => "Ambush",
            Self::Hunt => "Hunt",
            Self::Unload => "Unload",
            Self::Sabotage => "Sabotage",
            Self::Construction => "Construction",
            Self::Selling => "Selling",
            Self::Repair => "Repair",
            Self::Rescue => "Rescue",
            Self::Missile => "Missile",
            Self::Harmless => "Harmless",
            Self::Open => "Open",
            Self::Patrol => "Patrol",
            Self::ParadropApproach => "Paradrop Approach",
            Self::ParadropOverfly => "Paradrop Overfly",
            Self::Deliberate => "Wait",
            Self::AttackMove => "Attack Move",
            Self::SpyplaneApproach => "Spyplane Approach",
            Self::SpyplaneOverfly => "Spyplane Overfly",
            Self::None => "None",
        }
    }

    /// Iterate all 32 dispatched missions in id order (table builds, round-trip).
    pub fn all() -> impl Iterator<Item = MissionType> {
        (0u8..MISSION_COUNT as u8).filter_map(MissionType::from_id)
    }

    /// Resolve a map-INI `MISSION=` name, the way the original's
    /// `Mission_From_Name` does: a linear, **case-insensitive**, ASCII-only
    /// scan of the same 32-entry name table in id order, first match wins,
    /// and no match (or an absent field) yields the `-1` idle sentinel —
    /// which is a *distinct* selector from `Sleep(0)`, even though both run
    /// the same base handler.
    ///
    /// The table is [`MissionType::ini_section`]: the original hands the very
    /// same pointer table to both its map-name lookup and its
    /// `[<MissionName>]` control-section lookup. Five of those names carry a
    /// space (`Area Guard`, `Paradrop Approach`, `Paradrop Overfly`,
    /// `Attack Move`, `Spyplane Approach`/`Spyplane Overfly`) and index 28 is
    /// spelled `Wait`; a name table that loses those silently resolves the
    /// affected map lines to the sentinel.
    pub fn from_map_name(name: &str) -> Option<MissionType> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        MissionType::all().find(|mission| mission.ini_section().eq_ignore_ascii_case(name))
    }
}

/// Simulation frames per game-minute (15 fps × 60 s). `Rate=`/`AARate=` are
/// expressed in minutes; multiply by this and truncate (ftol) to get integer frames.
const FRAMES_PER_MINUTE: f64 = 900.0;

/// The `Rate=`/`AARate=` value a mission-control slot holds *before* any INI is
/// read: 0.016 minutes, i.e. `ftol(0.016 x 900) = 14` frames.
///
/// The original's `MissionControlClass` constructor stores this same double
/// into both the `Rate` and `AARate` fields of all 32 slots at process start;
/// its reader then passes the *current* double as the `Rate` default (so an
/// absent `Rate=` keeps it) and returns without touching the slot at all when
/// the `[<MissionName>]` section is absent. Eleven stock missions — Return,
/// Stop, Ambush, Construction, Selling, both Paradrop legs, Wait, Attack Move
/// and both Spyplane legs — reach their cadence through one of those two paths
/// and therefore resolve to 14 frames, never 0.
const CONSTRUCTED_RATE_MINUTES: f64 = 0.016;

/// Convert an INI rate (minutes between processings) to integer frames,
/// modelling gamemd's `Math::ftol(Rate * 900)` truncate-toward-zero (the
/// signed result is retained through the dispatch timer).
///
/// The minutes come from `CCINIClass::ReadDouble`, whose `%f` scan widens a
/// **float**: `[Guard] Rate=.030` reads as 0.029999999329447746 and truncates
/// to 26 frames, not 27 (likewise `[Area Guard] .040` -> 35, `[Repair] .08` ->
/// 71). Guard is the mission every idle unit holds and its dispatch consumes
/// a scenario-RNG draw, so the width moves deterministic state.
#[inline]
fn rate_to_frames(minutes: f64) -> i32 {
    // DeployedGuard521484..5214B8 executes negative rates unchanged;
    // infantry_deployed_guard.json pins Rate=-.1 through original ftol/RNG.
    // ReadDouble widens f32, whose x900 product fits binary64 precision.
    // Reuse the existing ftol owner for signed64-to-low32/invalid semantics.
    use crate::util::native_x87::{MaskedX87Chop53, NativeF64Bits};
    MaskedX87Chop53::ftol_i32_low_masked(MaskedX87Chop53::load_f64(NativeF64Bits::from_bits(
        (minutes * FRAMES_PER_MINUTE).to_bits(),
    )))
}

/// One mission's processing cadence and behaviour flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MissionControlEntry {
    /// Frames between normal processings (`Rate=` × 900, ftol-truncated).
    pub rate_frames: i32,
    /// Frames between anti-aircraft processings (`AARate=`; copies `rate_frames`
    /// when the key is absent or zero).
    pub aa_rate_frames: i32,
    /// The threat scan skips an object on this mission (`NoThreat=`, read at
    /// `0x005B37AE..0x005B37BF` over the constructed `false`, `0x005B3706`).
    ///
    /// Stock YR sets it on exactly two sections, `[Harmless]` and `[Selling]`.
    /// Two readers take it from the object's current mission
    /// (`sim::mission::control::current_control`); no other code reads it:
    /// - `TechnoClass::Evaluate_Candidate` refuses the object as a candidate
    ///   (`0x006F7DFF..0x006F7E0B`, `combat::greatest_threat`), so no scan
    ///   picks a building being sold;
    /// - `FootClass::ReceiveDamage`'s teamless idle tail
    ///   (`0x004D749F..0x004D74C7`, `combat::world_receiver`): a Harmless Foot
    ///   hit by a source takes `Enter_Idle_Mode(0, 1)`.
    pub no_threat: bool,
    /// Frozen forever, never recovers (`Zombie=`, def no).
    pub zombie: bool,
    /// Can be recruited into a team / base defence (`Recruitable=`, def yes).
    /// `[Sticky]` and `[Area Guard]` are the two stock sections that set it
    /// explicitly.
    ///
    /// `RespondToBaseAttack` reads it from the object's current mission
    /// (`0x0070825B`, `0x00708491`, `combat::base_defense_response`), and
    /// team recruitment through `0x005B36E0`
    /// (`sim::mission::control::mission_recruitable`).
    pub recruitable: bool,
    /// Frozen in place but can still fire and function (`Paralyzed=`, def no).
    ///
    /// Two verified consumer families, both indexing the object's **own
    /// current** mission slot:
    /// 1. the Unit and Infantry arrival selectors — modelled, on the infantry
    ///    side, in the Move handler's arrival arm. Note the vehicle side reads
    ///    it only on the Area-Guard-promotion branch, not on the ordinary Guard
    ///    fall, so a `Paralyzed=` mission does not stop a vehicle falling to
    ///    Guard;
    /// 2. `UnitClass::Scatter`'s coordinate arm (`0x00743CA7`,
    ///    `movement::scatter`), which refuses it. That body's prefix already
    ///    refuses Sticky (`vt+0x28C`), the one stock section that sets it.
    ///    `InfantryClass::Scatter` does not read it.
    pub paralyzed: bool,
    /// Allowed to retaliate while on this mission (`Retaliate=`, def yes).
    pub retaliate: bool,
    /// Allowed to scatter from threats (`Scatter=`, def yes).
    ///
    /// The Infantry and Unit Scatter receivers read it unless their first
    /// flag forces the call (`movement::scatter::mission_permits_scatter`);
    /// the Aircraft receiver reads it whatever the flags. `Scatter=no`
    /// covers Sleep, Sticky, Attack, Capture, **Harvest**, Unload,
    /// Construction and Selling in stock rules.
    pub scatter: bool,
}

impl Default for MissionControlEntry {
    /// The constructed defaults — the values each table slot holds before its
    /// section is read (so an absent key, or an absent section, keeps these).
    /// The six bools match the `; def=` comments in the RULESMD header block;
    /// the rate pair has no INI comment and comes from the constructor
    /// (see [`CONSTRUCTED_RATE_MINUTES`]).
    fn default() -> Self {
        Self {
            rate_frames: rate_to_frames(CONSTRUCTED_RATE_MINUTES),
            aa_rate_frames: rate_to_frames(CONSTRUCTED_RATE_MINUTES),
            no_threat: false,
            zombie: false,
            recruitable: true,
            paralyzed: false,
            retaliate: true,
            scatter: true,
        }
    }
}

/// The full mission-control table, one entry per dispatched mission id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissionControl {
    entries: BTreeMap<MissionType, MissionControlEntry>,
}

impl MissionControl {
    /// Install an original-executed frame projection for executable replay.
    /// Distinctive controls directly write native double fields; their saved
    /// conversion receipts bypass INI scanning. Retail uses the reader below.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_native_rate_frames_for_test(&mut self, mission: MissionType, frames: i32) {
        self.entries.get_mut(&mission).unwrap().rate_frames = frames;
    }

    /// Parse every dispatched mission's `[<MissionName>]` section. A mission
    /// whose section is absent keeps the documented defaults (matching the
    /// original reader, which leaves an unread slot at its constructed value).
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut entries = BTreeMap::new();
        for mission in MissionType::all() {
            let mut entry = MissionControlEntry::default();
            if let Some(section) = ini.section(mission.ini_section()) {
                entry.no_threat = section.read_bool("NoThreat", entry.no_threat);
                entry.zombie = section.read_bool("Zombie", entry.zombie);
                entry.recruitable = section.read_bool("Recruitable", entry.recruitable);
                entry.paralyzed = section.read_bool("Paralyzed", entry.paralyzed);
                entry.retaliate = section.read_bool("Retaliate", entry.retaliate);
                entry.scatter = section.read_bool("Scatter", entry.scatter);
                // `0x005B38B4` reads Rate over the constructed value.
                entry.rate_frames =
                    rate_to_frames(section.read_double("Rate", CONSTRUCTED_RATE_MINUTES));
                // AARate reads over 0 (`0x005B38DD`); zero copies Rate.
                entry.aa_rate_frames = match section.read_double("AARate", 0.0) {
                    0.0 => entry.rate_frames,
                    aa => rate_to_frames(aa),
                };
            } else {
                entry.aa_rate_frames = entry.rate_frames;
            }
            entries.insert(mission, entry);
        }
        Self { entries }
    }

    /// The control entry for a mission (present for every dispatched id).
    #[inline]
    pub fn entry(&self, mission: MissionType) -> Option<&MissionControlEntry> {
        self.entries.get(&mission)
    }

    /// Processing cadence in frames for a mission (0 if unknown).
    #[inline]
    pub fn rate_frames(&self, mission: MissionType) -> i32 {
        self.entries.get(&mission).map_or(0, |e| e.rate_frames)
    }

    /// Anti-air processing cadence in frames for a mission (0 if unknown).
    ///
    /// This is a *different* number from [`MissionControl::rate_frames`] on the
    /// two stock missions that declare both: Guard resolves 26 / **14** and
    /// Area Guard 35 / **28**, so a consumer that reaches for the wrong field
    /// is wrong by a factor of two.
    ///
    /// **Who actually reads it, and when** (resolved from the building Guard
    /// handler — the function at the BuildingClass vtable slot the mission
    /// dispatcher's Guard and Sticky cases call; slot identity proven by
    /// reading that vtable entry, not by a label. Ghidra leaves the function
    /// unnamed and its plate comment guesses a different mission; ignore it):
    ///
    /// The handler branches once, at the top, on the object's "has a weapon"
    /// query, and that single branch selects the field. It reaches the control
    /// table on three paths, and all three are accounted for:
    ///
    /// - **Armed** (a base defence) → the timer return reads `AARate` and
    ///   yields `ftol(AARate x 900) + RandomRanged(0, 2)`. This path is taken
    ///   whenever the defence did not commit Attack this dispatch.
    /// - **Unarmed**, repair-depot-flagged → reads `Rate`, same shape.
    /// - **Unarmed**, otherwise → reads `Rate` and returns **three times** it,
    ///   plus the same jitter.
    ///
    /// So the selector is a property of the BUILDING, not of the target — NOT
    /// "the current target is an aircraft", which is what the key's name
    /// suggests and what an earlier note here assumed. An armed structure
    /// re-arms at `AARate` against ground and air alike.
    ///
    /// The query itself is broader than "has a weapon": `BuildingClass +0x2AC`
    /// (0x00458DB0) returns 1 for its vtable `+0x400` case BEFORE falling
    /// through to the TechnoClass armed test at 0x00701120, so it is
    /// *armed **or** occupied* — a garrisoned civilian building takes the
    /// `AARate` path too.
    ///
    /// Two building handlers read the field, not one: `Mission_Guard`
    /// 0x004496B0 at 0x004497B6, and `Mission_Attack` 0x0044ACF0 at 0x0044AD2D.
    /// Whether any non-building handler reads `AARate` stays UNCHECKED — a
    /// `FLD double [reg+0x18]` sweep found only those two, but that pattern
    /// misses computed-base and indexed forms, and a scan that finds nothing
    /// certifies nothing.
    ///
    /// Its `sim/` reader is the building Guard handler,
    /// `sim::world::techno_ai::building_missions`; `Mission_Attack`'s read is
    /// its SAM arm, dormant with retail data.
    #[inline]
    pub fn aa_rate_frames(&self, mission: MissionType) -> i32 {
        self.entries.get(&mission).map_or(0, |e| e.aa_rate_frames)
    }

    /// Number of populated mission entries.
    #[cfg(test)]
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table holds no entries.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod mission_type_tests {
    use super::*;

    /// Every mission name the original's table spells with a space must be
    /// spelled with it here, or the `[<MissionName>]` lookup misses.
    #[test]
    fn spaced_section_names_keep_their_space() {
        assert_eq!(MissionType::AreaGuard.ini_section(), "Area Guard");
        assert_eq!(
            MissionType::ParadropApproach.ini_section(),
            "Paradrop Approach"
        );
        assert_eq!(
            MissionType::ParadropOverfly.ini_section(),
            "Paradrop Overfly"
        );
        assert_eq!(MissionType::AttackMove.ini_section(), "Attack Move");
        assert_eq!(
            MissionType::SpyplaneApproach.ini_section(),
            "Spyplane Approach"
        );
        assert_eq!(
            MissionType::SpyplaneOverfly.ini_section(),
            "Spyplane Overfly"
        );
    }

    /// `Mission_From_Name` returns the table index for an exact-but-
    /// case-insensitive name, so every one of the 32 names round-trips.
    #[test]
    fn map_name_round_trips_every_mission() {
        for mission in MissionType::all() {
            assert_eq!(
                MissionType::from_map_name(mission.ini_section()),
                Some(mission),
                "{mission:?} did not round-trip through its map name"
            );
            assert_eq!(
                MissionType::from_map_name(&mission.ini_section().to_ascii_lowercase()),
                Some(mission),
                "{mission:?} map name is not case-insensitive"
            );
        }
    }

    /// The retail comparator is `stricmp`, and the five spaced names plus
    /// `Wait` are the ones a naive `{:?}` spelling would lose.
    #[test]
    fn map_name_resolves_the_spaced_and_renamed_entries() {
        assert_eq!(
            MissionType::from_map_name("Area Guard"),
            Some(MissionType::AreaGuard)
        );
        assert_eq!(
            MissionType::from_map_name("attack move"),
            Some(MissionType::AttackMove)
        );
        assert_eq!(
            MissionType::from_map_name("Wait"),
            Some(MissionType::Deliberate)
        );
        // The enum's own Rust name is NOT the table name.
        assert_eq!(MissionType::from_map_name("AreaGuard"), None);
        assert_eq!(MissionType::from_map_name("Deliberate"), None);
    }

    /// An unknown or absent name is the `-1` sentinel, NOT `Sleep(0)`.
    #[test]
    fn unknown_map_name_is_the_idle_sentinel() {
        assert_eq!(MissionType::from_map_name("Wibble"), None);
        assert_eq!(MissionType::from_map_name(""), None);
        assert_eq!(MissionType::from_map_name("   "), None);
        // "None" is this project's spelling of the sentinel, not a table name.
        assert_eq!(MissionType::from_map_name("None"), None);
        assert_eq!(
            MissionType::from_map_name("Sleep"),
            Some(MissionType::Sleep)
        );
    }

    /// The 32 section names are distinct — a duplicate would make two missions
    /// share one control slot.
    #[test]
    fn section_names_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for mission in MissionType::all() {
            assert!(
                seen.insert(mission.ini_section()),
                "duplicate section name for {mission:?}"
            );
        }
        assert_eq!(seen.len(), MISSION_COUNT);
    }
}

#[cfg(test)]
mod mission_control_tests {
    use super::*;

    fn ini(text: &str) -> IniFile {
        IniFile::from_str(text)
    }

    #[test]
    fn rate_to_frames_uses_900_per_minute() {
        assert_eq!(rate_to_frames(1.0), 900);
        assert_eq!(rate_to_frames(0.016), 14); // 14.4 -> ftol 14 (stock, unchanged)
        // `rate_to_frames` takes a double, so these are the exact-decimal
        // answers. The INI path never produces them: `ReadDouble` widens an
        // f32 first, so `.030` arrives as 0.029999999329447746 and truncates to
        // 26. See `stock_rates_go_through_the_f32_widening` below.
        assert_eq!(rate_to_frames(0.030), 27);
        assert_eq!(rate_to_frames(0.040), 36);
        // ftol truncates toward zero: a modded rate whose ×900 has a ≥.5
        // fraction lands one frame below what `.round()` would have produced.
        assert_eq!(rate_to_frames(0.0206), 18); // 18.54 -> ftol 18 (round would give 19)
    }

    #[test]
    fn aarate_absent_copies_rate_present_overrides() {
        let mc =
            MissionControl::from_ini(&ini("[Move]\nRate=.016\n[Guard]\nRate=.030\nAARate=.016\n"));
        let mv = mc.entry(MissionType::Move).unwrap();
        assert_eq!(mv.rate_frames, 14);
        assert_eq!(mv.aa_rate_frames, 14); // copied from Rate
        let gd = mc.entry(MissionType::Guard).unwrap();
        assert_eq!(gd.rate_frames, 26);
        assert_eq!(gd.aa_rate_frames, 14); // overridden by AARate
    }

    /// `CCINIClass::ReadDouble` 0x005283D0 parses `"%f"` — a 4-byte float — and
    /// only then widens, so an INI rate never carries more precision than an
    /// f32 can hold. Three stock cadences straddle an integer boundary because
    /// of it, and reading them as `f64` would give each one an extra frame.
    #[test]
    fn stock_rates_go_through_the_f32_widening() {
        for (section, mission, widened, exact_f64) in [
            ("Guard", MissionType::Guard, 26i32, 27i32),
            ("Area Guard", MissionType::AreaGuard, 35, 36),
            ("Repair", MissionType::Repair, 71, 72),
        ] {
            let raw = match mission {
                MissionType::Guard => ".030",
                MissionType::AreaGuard => ".040",
                _ => ".08",
            };
            let mc = MissionControl::from_ini(&ini(&format!("[{section}]\nRate={raw}\n")));
            assert_eq!(
                mc.rate_frames(mission),
                widened,
                "[{section}] Rate={raw} must widen through f32"
            );
            assert_eq!(
                rate_to_frames(raw.parse::<f64>().unwrap()),
                exact_f64,
                "and an f64 parse would have given one frame more"
            );
        }
        // Values whose x900 lands the same side of the boundary in both widths.
        // Only `1` is exactly representable; the other three are inexact as f32
        // AND as f64, but not near enough to a boundary for the width to matter.
        for (raw, frames) in [(".016", 14i32), (".032", 28), (".1", 90), ("1", 900)] {
            let mc = MissionControl::from_ini(&ini(&format!("[Move]\nRate={raw}\n")));
            assert_eq!(mc.rate_frames(MissionType::Move), frames);
        }
    }

    #[test]
    fn explicit_zero_aarate_copies_rate() {
        let mc = MissionControl::from_ini(&ini("[Guard]\nRate=.030\nAARate=0\n"));
        let gd = mc.entry(MissionType::Guard).unwrap();
        assert_eq!(gd.aa_rate_frames, gd.rate_frames);
        assert_eq!(gd.aa_rate_frames, 26);
    }

    #[test]
    fn no_carry_forward_between_missions() {
        // Guard sets AARate/Rate; a keyless mission must NOT inherit them —
        // it keeps its own constructed values, which for the rate pair is
        // 0.016 minutes = 14 frames, NOT zero.
        let mc = MissionControl::from_ini(&ini("[Guard]\nRate=.030\nAARate=.016\n"));
        let stop = mc.entry(MissionType::Stop).unwrap(); // no [Stop] section
        assert_eq!(stop.rate_frames, 14);
        assert_eq!(stop.aa_rate_frames, 14);
        assert!(stop.recruitable); // documented defaults, not Guard's values
        assert!(stop.retaliate);
        assert!(stop.scatter);
    }

    #[test]
    fn constructed_rate_is_fourteen_frames_not_zero() {
        // The constructor stores 0.016 minutes into Rate and AARate alike, and
        // neither the absent-section path nor the absent-key path overwrites
        // it. Both must therefore land on ftol(0.016 * 900) = 14.
        assert_eq!(rate_to_frames(CONSTRUCTED_RATE_MINUTES), 14);

        // Absent section (rows 26-31 in stock: the paradrop/spyplane legs,
        // Wait and Attack Move declare no `[<MissionName>]` section at all).
        let absent_section = MissionControl::from_ini(&ini("[Guard]\nRate=.030\nAARate=.016\n"));
        for mission in [
            MissionType::ParadropApproach,
            MissionType::ParadropOverfly,
            MissionType::Deliberate,
            MissionType::AttackMove,
            MissionType::SpyplaneApproach,
            MissionType::SpyplaneOverfly,
        ] {
            let entry = absent_section.entry(mission).unwrap();
            assert_eq!(entry.rate_frames, 14, "{mission:?} Rate");
            assert_eq!(entry.aa_rate_frames, 14, "{mission:?} AARate");
        }

        // Present-but-Rate-less section (rows 12/13/14/18/19 in stock:
        // Return, Stop, Ambush, Construction, Selling).
        let rate_less = MissionControl::from_ini(&ini("[Selling]\nNoThreat=yes\nRetaliate=no\n"));
        let selling = rate_less.entry(MissionType::Selling).unwrap();
        assert_eq!(selling.rate_frames, 14);
        assert_eq!(selling.aa_rate_frames, 14);
        assert!(selling.no_threat);
        assert!(!selling.retaliate);
    }

    #[test]
    fn guard_and_area_guard_keep_distinct_rate_and_aa_rate() {
        // The two stock missions whose AARate differs from Rate. A building
        // cadence must read AARate (14 / 28), not Rate (26 / 35).
        let mc = MissionControl::from_ini(&ini(
            "[Guard]\nRate=.030\nAARate=.016\n[Area Guard]\nRate=.040\nAARate=.032\n",
        ));
        assert_eq!(mc.rate_frames(MissionType::Guard), 26);
        assert_eq!(mc.aa_rate_frames(MissionType::Guard), 14);
        // .032 * 900 = 28.8; ftol truncates toward zero -> 28 (round gives 29).
        assert_eq!(mc.rate_frames(MissionType::AreaGuard), 35);
        assert_eq!(mc.aa_rate_frames(MissionType::AreaGuard), 28);
    }

    #[test]
    fn table_is_fully_populated_even_with_empty_ini() {
        let mc = MissionControl::from_ini(&ini(""));
        assert_eq!(mc.len(), MissionType::all().count());
        for m in MissionType::all() {
            assert!(mc.entry(m).is_some(), "missing entry for {m:?}");
        }
    }
}

#[cfg(test)]
mod acceptance_tests {
    use super::*;
    use crate::rules::ruleset::RuleSet;

    /// F04a acceptance: the ownership move must not change native selector
    /// discriminants, serde wire values, MissionControl field order, or the
    /// deterministic hash of a fixed rules source.
    #[test]
    fn rules_hash_and_enum_wire_values_survive_vocabulary_move() {
        // The first integer is the native selector discriminant. The second is
        // serde's declaration-order enum variant index (bincode u32 LE). None
        // is deliberately declared first while carrying native value 0xFF.
        const MISSION_WIRE: &[(MissionType, u16, u32, &str)] = &[
            (MissionType::None, 0x00FF, 0, "\"None\""),
            (MissionType::Sleep, 0, 1, "\"Sleep\""),
            (MissionType::Attack, 1, 2, "\"Attack\""),
            (MissionType::Move, 2, 3, "\"Move\""),
            (MissionType::QMove, 3, 4, "\"QMove\""),
            (MissionType::Retreat, 4, 5, "\"Retreat\""),
            (MissionType::Guard, 5, 6, "\"Guard\""),
            (MissionType::Sticky, 6, 7, "\"Sticky\""),
            (MissionType::Enter, 7, 8, "\"Enter\""),
            (MissionType::Capture, 8, 9, "\"Capture\""),
            (MissionType::Eaten, 9, 10, "\"Eaten\""),
            (MissionType::Harvest, 10, 11, "\"Harvest\""),
            (MissionType::AreaGuard, 11, 12, "\"AreaGuard\""),
            (MissionType::Return, 12, 13, "\"Return\""),
            (MissionType::Stop, 13, 14, "\"Stop\""),
            (MissionType::Ambush, 14, 15, "\"Ambush\""),
            (MissionType::Hunt, 15, 16, "\"Hunt\""),
            (MissionType::Unload, 16, 17, "\"Unload\""),
            (MissionType::Sabotage, 17, 18, "\"Sabotage\""),
            (MissionType::Construction, 18, 19, "\"Construction\""),
            (MissionType::Selling, 19, 20, "\"Selling\""),
            (MissionType::Repair, 20, 21, "\"Repair\""),
            (MissionType::Rescue, 21, 22, "\"Rescue\""),
            (MissionType::Missile, 22, 23, "\"Missile\""),
            (MissionType::Harmless, 23, 24, "\"Harmless\""),
            (MissionType::Open, 24, 25, "\"Open\""),
            (MissionType::Patrol, 25, 26, "\"Patrol\""),
            (
                MissionType::ParadropApproach,
                26,
                27,
                "\"ParadropApproach\"",
            ),
            (MissionType::ParadropOverfly, 27, 28, "\"ParadropOverfly\""),
            (MissionType::Deliberate, 28, 29, "\"Deliberate\""),
            (MissionType::AttackMove, 29, 30, "\"AttackMove\""),
            (
                MissionType::SpyplaneApproach,
                30,
                31,
                "\"SpyplaneApproach\"",
            ),
            (MissionType::SpyplaneOverfly, 31, 32, "\"SpyplaneOverfly\""),
        ];

        for &(mission, native_discriminant, serde_variant_index, json) in MISSION_WIRE {
            assert_eq!(
                mission as u16, native_discriminant,
                "{mission:?} discriminant"
            );
            assert_eq!(
                crate::util::save_codec::serialize(&mission).expect("serialize mission selector"),
                serde_variant_index.to_le_bytes(),
                "{mission:?} bincode declaration index"
            );
            assert_eq!(
                serde_json::to_string(&mission).expect("serialize mission selector as JSON"),
                json,
                "{mission:?} JSON variant name"
            );
        }

        // Fixed bytes/JSON pin the public field order as well as every field's
        // primitive wire representation. This is intentionally not a
        // serialize-then-deserialize self-comparison.
        let entry = MissionControlEntry {
            rate_frames: 0x0102_0304,
            aa_rate_frames: 0x1112_1314,
            no_threat: true,
            zombie: false,
            recruitable: true,
            paralyzed: false,
            retaliate: true,
            scatter: false,
        };
        assert_eq!(
            crate::util::save_codec::serialize(&entry).expect("serialize mission control entry"),
            [
                0x04, 0x03, 0x02, 0x01, 0x14, 0x13, 0x12, 0x11, 1, 0, 1, 0, 1, 0,
            ]
        );
        assert_eq!(
            serde_json::to_string(&entry).expect("serialize mission control entry as JSON"),
            "{\"rate_frames\":16909060,\"aa_rate_frames\":286397204,\"no_threat\":true,\"zombie\":false,\"recruitable\":true,\"paralyzed\":false,\"retaliate\":true,\"scatter\":false}"
        );

        // This literal was captured from the pre-move source-owned hash
        // contract. It pins exact parsed section/key/value order rather than
        // comparing two hashes produced by the relocated Rust definitions.
        const RULES_SOURCE: &str = "[Guard]\nRate=.030\nAARate=.016\nRetaliate=yes\n\
                                    [Area Guard]\nRate=.040\nAARate=.032\nScatter=no\n";
        const EXPECTED_INI_HASH: u64 = 0x4635_CE56_7472_58B8;
        const EXPECTED_ONE_LAYER_HASH: u64 = 0x37E7_F896_E2C0_585E;
        let ini = IniFile::from_str(RULES_SOURCE);
        assert_eq!(ini.content_hash(), EXPECTED_INI_HASH);
        let rules = RuleSet::from_ini(&ini).expect("fixed mission rules source parses");
        assert_eq!(rules.source_ini_hash(), EXPECTED_ONE_LAYER_HASH);
    }
}
