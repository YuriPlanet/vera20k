//! Scan radius for passive target acquisition.
//!
//! Retail computes the acquisition radius *inside* the threat scan, from a
//! small mask the calling mission supplies — no caller passes a range. This
//! module is that computation plus the mask selection, kept as a mechanism
//! rather than folded into the candidate loop because the two missions that
//! reach the scanner produce genuinely different filters.
//!
//! ## The two filters
//!
//! - **Guard** (and the other plain passive-acquire missions) asks for the
//!   object's own `GuardRange=`. When the type has none, the computed radius is
//!   zero, and a zero radius means *no distance cutoff of its own*: acceptance
//!   falls through to the attacker's own can-fire-at-this-target query, i.e.
//!   the range of the weapon picked against that specific candidate. That is
//!   [`ScanRange::CanFireAt`].
//! - **Area Guard** asks for `GuardRange=` **or** the wider of the type's two
//!   weapon ranges, **doubled**, clamped to `[0,` [`AREA_GUARD_MAX_SCAN_CELLS`]`]`.
//!   A non-zero radius is a hard distance cutoff between the two objects'
//!   GetCoords (`greatest_threat`'s `candidate_beyond_cutoff`) — the
//!   can-fire-at query is not consulted at all. This is why a unit parked on
//!   Area Guard reaches out roughly twice as far as the same unit sitting on
//!   plain Guard. The radius
//!   is zero only when neither slot reaches past zero: an open-topped
//!   transport carrying a Spy (`MakeupKit` is `Range=-2`) caps both at -2
//!   cells, and that zero takes the Guard zero case.
//!
//! The doubling applies to `GuardRange=` too, not just to the weapon-range
//! fallback.
//!
//! ## The third mask: Hunt asks for no filter and no ring walk
//!
//! `FootClass::Mission_Hunt @ 0x004D5373` pushes the literal `0`, and mask 0
//! does not select a third radius formula — it skips the radius block, the
//! airborne pre-pass and the ring walk outright and enumerates the global
//! object array instead, passing a literal `-1` where the ring path passes its
//! computed radius. That is a scan *topology*, so it lives in
//! [`super::greatest_threat`]; [`ScanRange::NoCutoff`] is only the
//! per-candidate half of it. See [`ScanMission::Hunt`].
//!
//! ## Why plain Guard really is `CanFireAt` — the mask literals
//!
//! This has now been challenged twice, so the chain is written down here.
//!
//! The threat scan takes a bitmask, and the radius formula is selected from it:
//! **bit0 set → the narrow formula; bit0 clear and bit1 set → the doubled one**
//! (with a Patrol-only third variant). Nobody chooses that mask at the scan —
//! the *caller* supplies it as a literal, and there are exactly two callers:
//!
//! - The common Techno AI body, which is the ONLY route to the scan for
//!   missions {Move, Harvest, Guard} — those three ids and no others — pushes
//!   the literal **1** together with the object's own coordinates.
//! - `FootClass::Mission_AreaGuard`, which pushes the literal **2** together
//!   with the guard post's coordinates.
//!
//! So Guard's mask cannot carry bit1: it is a hardcoded `1` in the caller.
//! Guard therefore takes the narrow formula, which for a type with no
//! `GuardRange=` computes zero — and radius zero is what defers acceptance to
//! the attacker's own can-fire-at query. The doubling belongs to Area Guard
//! (and Patrol) alone.
//!
//! The counter-argument — "the FootClass override that rewrites the mask only
//! makes sense if Guard's mask carried bit1" — does not hold: that override
//! reads `mask & ~bit1 | bit0`, i.e. it can only ever *downgrade* a bit1 caller
//! to the narrow formula, never add bit1 to a bit0 one. Its purpose is the
//! freshly-moved latch below, whose only bit1 callers are Area Guard and
//! Patrol. The same override clears the latch when a scan comes back empty.
//!
//! ## What is deliberately NOT modelled
//!
//! Foot's stopped-cannot-fire latch688 is private GameEntity state. Drive,
//! Ship and Team write it through that owner; the concrete scanner coerces
//! `(mask & !2) | 1` and the world scan adapter clears it only after an empty
//! Foot result. The actual scan mask selects the range below. Hover's native
//! Process/arrival continuation5164D0 still needs its separate owner.
//!
//! **The unarmed-Guard override.** When the scanning object has no usable
//! weapon at all *and* its mission is exactly Guard, retail forces the radius
//! to a flat 2 cells instead of computing one. Gated passive acquisition
//! requires an armed type, but direct Greatest_Threat callers bypass that
//! gate. The unarmed radius branch remains required outside the ordinary
//! armed MTNK/E1 controls; missing candidate weapons still reject a target.
//!
//! Retail has a third radius formula, reached only when the scanning object is
//! on **Patrol**: the same doubled-and-capped value as Area Guard but with a
//! 7-cell *floor* underneath it. The shared scalar owner reproduces mode2;
//! the Patrol caller and its scan topology still require their own audit.
//!
//! Retail walks outward cell by cell and bounds that walk at
//! `wider weapon range + 1 + AirRangeBonus` cells. That number is a **search**
//! bound, not an acceptance radius: the walk is a Chebyshev square strictly
//! wider than the reach acceptance would allow, and it exists only on the
//! radius-zero branch, so it never clips a candidate acceptance would have
//! taken. Reading that bound as the acquisition radius is a recurring wrong
//! turn — it is the reason this file exists rather than a widened per-candidate
//! range. [`super::greatest_threat`] owns the walk and applies that bound;
//! this file owns the acceptance radius the walk carries into each candidate.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ (RuleSet, ObjectType) and sim/ only.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::rules::object_type::ObjectType;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::util::fixed_math::SimFixed;

/// Retail doubles the Area Guard radius before clamping it.
const AREA_GUARD_RANGE_MULTIPLIER: i32 = 2;

/// Ceiling on the doubled Area Guard radius, in cells. Retail clamps the
/// lepton value to 0x1000; at 256 leptons per cell that is 16 cells.
const AREA_GUARD_MAX_SCAN_CELLS: i32 = 16;

/// The threat mask a callsite pushes into the scan — retail's `Greatest_Threat`
/// argument 2, a literal at every callsite, named here after the mission that
/// pushes it. It selects the radius formula and, for [`ScanMission::Hunt`], the
/// scan topology. It is NOT a property of the object being scanned for, nor
/// read off the scanner's mission field: two callers can hand the same object
/// different masks, and `FUN_0051F330` does exactly that when it re-acquires in
/// place for a deployed infantryman that is sitting on Area Guard.
///
/// **The literal is not always what `TechnoClass::Greatest_Threat` receives.**
/// A `FootClass` dispatch goes through a `+0x3C4` override first, and two of
/// them rewrite it on the way:
/// - `UnitClass @ 0x00743190` and the Infantry override (`CALL @ 0x0051E39F`)
///   OR the attacker's own projectile class bits in (`FUN_00772A90`, AA → `4`,
///   AG → `0xB8`) when `mask & 0x1B978 == 0`, which mask 0 satisfies. The
///   topology survives — neither value carries `TEST AL,0x3` — but the derived
///   flags word changes.
/// - `FootClass::Greatest_Threat @ 0x004D9931` coerces the mask to
///   `(mask & ~2) | 1` while `FootClass+0x688` is set, turning mask 0 into mask
///   1 and so the flat walk into the ring walk, and clears that byte at
///   `0x004D9955` when the scan returns nothing.
///
/// [`super::threat_mask`] models the class overrides; Greatest_Threat applies
/// the Foot+688 coercion before selecting this radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMission {
    /// Guard, Move, Harvest — the plain passive-acquire missions. All of them
    /// take the same mask, so they share one variant.
    Guard,
    /// Area Guard — "hold this spot and cover it".
    AreaGuard,
    /// Hunt — mask **0**, which is not a third radius formula but the absence
    /// of one.
    ///
    /// `FootClass::Mission_Hunt @ 0x004D5373` pushes the literal `0` as the
    /// threat mask (`disassemble_function 0x004D5350`), and
    /// `TechnoClass::Greatest_Threat @ 0x006F8FE0` opens its radius block with
    /// `TEST AL,0x3 ; JZ 0x006F9B6E` — with neither bit set it jumps **past**
    /// both the `Threat_Range` selection (`vt+0x31C` at `0x006F900A` /
    /// `0x006F9018`, result stored into the range slot `[ESP+0x2C]` at
    /// `0x006F9020`) and the cell walk that consumes it.
    ///
    /// Where mask 0 actually lands: at `0x006F9B6E` the **first** global walk
    /// (the `0x00A8E394` list) is gated on `TEST AL,0x4` against the derived
    /// flags word `[ESP+0x14]` — zeroed at entry (`XOR EDI,EDI @ 0x006F8DFD`,
    /// stored at `0x006F8F2C`) and thereafter built from the mask alone
    /// (`0x006F8F29`-`0x006F8F72`). For mask 0 it is 0, the bit is clear, and
    /// that walk is **skipped** (`JZ 0x006F9C56`). Execution falls through
    /// `0x006F9C56` (`TEST byte ptr [ESP+0x70],0x10 ; JZ 0x006F9C67`) into the
    /// **unconditional** second walk over the `0x00A8EC7C` list at `0x006F9C67`.
    ///
    /// What removes the cutoff is that walk's range argument, not the absence
    /// of a distance test: it passes the literal `PUSH -0x1` (`0x006F9D70`) in
    /// the argument slot where the radius path passes its computed range
    /// (`MOV ECX,[ESP+0x2C] @ 0x006F9292`, `PUSH ECX @ 0x006F92A7`).
    /// `TechnoClass::Evaluate_Candidate @ 0x006F7CA0` rejects on distance only
    /// when that argument is `> 0`, and falls back to the
    /// `TechnoType+0x5B8` / `vt+0x3A8` (Sight / can-fire-at) test only when it
    /// is `== 0`; `-1` trips neither. So a hunting object has no *distance*
    /// cutoff at all — that is the mechanism that sends a berserked unit across
    /// the map.
    ///
    /// It is not unfiltered, though. The same walk that removes the distance
    /// cutoff switches a **movement-zone** gate on: it passes the scanner's own
    /// zone id (`MapClass::GetZoneID @ 0x006F8EBF`, stored at `0x006F8EC4`) in
    /// `Evaluate_Candidate`'s arg6 (`PUSH ECX @ 0x006F9D69`) where every ring
    /// callsite passes `-1`, and a non-`-1` arg6 rejects any candidate whose
    /// cell is in a different component under the attacker's own
    /// `MovementZone=` (`0x006F7E7E`-`0x006F7E9C`). A hunter reaches the far
    /// side of the map but not the far side of a river.
    ///
    /// **Mask 0 is a scan TOPOLOGY, not a radius.** The jump at `0x006F8FE2`
    /// lands past the airborne pre-pass *and* past the expanding-ring cell walk
    /// (`0x006F9169` and `0x006F94D0` both sit below `0x006F9B6E`), so a
    /// hunting object never walks a single cell ring — it walks the global
    /// object array. [`super::greatest_threat`] owns that branch; this variant
    /// only names which mask the caller pushed.
    Hunt,
    /// A quarry's mask (`Quarry_To_Threat @ 0x00645BB0`) pushed directly:
    /// - team script actions 0, Attack quarry (`TeamClass @ 0x006ED090`), and
    ///   57, the Chronosphere's (`0x006F0130`), with the TeamType's
    ///   `OnlyTargetHouseEnemy=` (`+0xF7`) as `Greatest_Threat`'s arg3
    ///   (`0x006ED14C..0x006ED15E`, `0x006F0244..0x006F0253`);
    /// - `AircraftClass::Mission_Hunt`'s multiplayer pass, the harvester mask
    ///   `0x40` (any type with `Storage=`) with arg3 0
    ///   (`0x00414AFD..0x00414B24`).
    ///
    /// No quarry mask carries bit 0 or 1, so it takes Hunt's flat walk,
    /// measured from the scanner's own Coords.
    Quarry {
        mask: u32,
        only_target_house_enemy: bool,
    },
}

impl ScanMission {
    /// The literal the caller pushes as `Greatest_Threat`'s arg1: Guard `1`
    /// (the passive block), Area Guard `2`, Hunt `0` (`0x004D5373`), a
    /// quarry its own mask.
    pub(crate) const fn literal_mask(self) -> u32 {
        match self {
            Self::Guard => 1,
            Self::AreaGuard => 2,
            Self::Hunt => 0,
            Self::Quarry { mask, .. } => mask,
        }
    }

    /// `Greatest_Threat`'s arg3, which only team actions 0 and 57 set.
    pub(crate) const fn only_target_house_enemy(self) -> bool {
        matches!(
            self,
            Self::Quarry {
                only_target_house_enemy: true,
                ..
            }
        )
    }
}

/// The distance filter the candidate acceptance test applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScanRange {
    /// Radius zero: no cutoff of the scan's own. Acceptance defers to whether
    /// the attacker can actually fire at that candidate, which is a per-
    /// candidate question because weapon choice depends on the target.
    CanFireAt,
    /// A non-zero `Threat_Range`, in leptons: `Evaluate_Candidate` refuses a
    /// candidate farther than a positive one and skips the can-fire-at query
    /// for any.
    Hard(i32),
    /// Native's literal `-1` range argument (`PUSH -0x1 @ 0x006F9D70`), which
    /// `Evaluate_Candidate` treats as neither `> 0` (no distance reject) nor
    /// `== 0` (no Sight / can-fire-at fallback). Weapon legality still decides;
    /// distance does not. Only mask 0 produces it, and it selects the flat
    /// global-list topology in [`super::greatest_threat`] as well as the
    /// per-candidate predicate.
    NoCutoff,
}

/// The mask the *passive* acquisition callsites push.
///
/// In retail no callsite derives the mask from the object it is scanning for —
/// it is a literal at the caller (what the `+0x3C4` overrides then do to that
/// literal is documented on [`ScanMission`]), and the
/// three literals are `1` (the common Techno AI body, the only route to the
/// scan for missions Move/Guard/Harvest, and `FUN_0051F330`'s in-place
/// re-acquire), `2` (`FootClass::Mission_AreaGuard`) and `0`
/// (`FootClass::Mission_Hunt @ 0x004D5373`). This function is therefore not a
/// general "what mask is this object on" reader: it is the *passive* block's
/// own choice between its literal `1` and the Area Guard handler's literal `2`,
/// and it is called BY those callsites. Hunt and the deploy shim pass their own
/// literals and never come through here — reading [`ScanMission::Hunt`] off an
/// entity's mission field would make the mask a property of the mission, which
/// it is not.
///
/// Player-issued and map-created Area Guard share the committed mission and
/// its ArchiveTarget post. A queued order changes this choice only when the
/// mission owner promotes it.
pub(crate) fn scan_mission_for(entity: &GameEntity) -> ScanMission {
    if entity.mission.current().known() == Some(MissionType::AreaGuard) {
        ScanMission::AreaGuard
    } else {
        ScanMission::Guard
    }
}

/// Acquisition radius for one scanning object. Weapon ranges are the live
/// `GetWeaponRange(0/1)` results from [`super::combat_weapon::weapon_range`].
/// The native scalar owns only mode selection, doubling and clamps; it does
/// not duplicate GetWeapon, veterancy or the cargo minimum reader.
pub(crate) fn scan_range(obj: &ObjectType, mask: u32, weapon_ranges: [i32; 2]) -> ScanRange {
    match mask & 3 {
        1 | 3 => range_from_leptons(threat_range_leptons(obj, 0, weapon_ranges)),
        // Mask 0 never reaches the radius block at all: `TEST AL,0x3 ; JZ
        // 0x006F9B6E` at `0x006F8FE0` jumps past `Threat_Range` *and* past the
        // ring walk, so no radius is computed for Hunt and `GuardRange=` is not
        // consulted — a `GuardRange=9` V3 on Hunt is no more limited than a
        // Grizzly is. `greatest_threat` branches on the mask before it asks for
        // a radius, so this arm is not on the live path; it carries native's
        // literal `PUSH -0x1` (`0x006F9D70`) so that a caller which does ask
        // gets the same answer the flat walk hardcodes.
        0 => ScanRange::NoCutoff,
        2 => range_from_leptons(threat_range_leptons(obj, 1, weapon_ranges)),
        _ => unreachable!("two mask bits"),
    }
}

fn range_from_leptons(leptons: i32) -> ScanRange {
    if leptons == 0 {
        ScanRange::CanFireAt
    } else {
        ScanRange::Hard(leptons)
    }
}

/// Sole `TechnoClass::Threat_Range @ 0x00707E60` port, in whole leptons.
/// The scanner, Rescue4DE0ED and AreaGuard4D6E4B use this same owner.
/// Mode0 returns a nonzero GuardRange unchanged unless vt+330 identifies an
/// Engineer; mode1 doubles GuardRange or the wider live weapon range, then
/// clamps0..4096. The native ADD wraps before the clamp. Mode2 applies the
/// Patrol floor1792; mode-1 returns-1. Native execution: threat_range_cargo
/// and anytown_damage/foot_missions (ordinary MTNK/E1 mode1 and leash rows).
pub(crate) fn threat_range_leptons(obj: &ObjectType, mode: i32, weapon_ranges: [i32; 2]) -> i32 {
    if mode == -1 {
        return -1;
    }
    let guard = obj.guard_range.map_or(0, |range| range.to_bits() >> 8);
    if mode == 0 {
        return if obj.category == crate::rules::object_type::ObjectCategory::Infantry
            && obj.engineer
        {
            0
        } else {
            guard
        };
    }
    let base = if guard != 0 {
        guard
    } else {
        weapon_ranges[0].max(weapon_ranges[1])
    };
    let doubled = base.wrapping_mul(AREA_GUARD_RANGE_MULTIPLIER);
    doubled.clamp(
        if mode == 2 { 0x700 } else { 0 },
        AREA_GUARD_MAX_SCAN_CELLS * 256,
    )
}

/// Greatest_Threat6F90DE..6F9110's wider live weapon range in cells.
/// Both raw signed inputs come from the sole GetWeaponRange owner.
pub(crate) fn max_weapon_range(weapon_ranges: [i32; 2]) -> SimFixed {
    let leptons = weapon_ranges[0].max(weapon_ranges[1]);
    // One lepton is 1/256 cell: exact in I16F16 for map-space ranges.
    SimFixed::from_bits(leptons.saturating_mul(256))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    /// Three armed types: one with no `GuardRange=`, one with `GuardRange=9`
    /// (the retail V3 Launcher value), one with a single weapon slot. Weapon
    /// ranges are deliberately asymmetric so `max(primary, secondary)` is
    /// observable and slot order cannot be mistaken for the answer.
    fn test_rules() -> RuleSet {
        let ini_str: &str = "\
[VehicleTypes]\n0=NOGUARD\n1=WITHGUARD\n2=ONLYPRIMARY\n\n\
[NOGUARD]\nStrength=100\nArmor=heavy\nSpeed=6\nPrimary=ShortGun\nSecondary=LongGun\n\n\
[WITHGUARD]\nStrength=100\nArmor=heavy\nSpeed=6\nPrimary=ShortGun\nSecondary=LongGun\n\
GuardRange=9\n\n\
[ONLYPRIMARY]\nStrength=100\nArmor=heavy\nSpeed=6\nPrimary=ShortGun\n\n\
[ShortGun]\nDamage=50\nROF=20\nRange=4\nWarhead=AP\n\n\
[LongGun]\nDamage=50\nROF=20\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
        RuleSet::from_ini(&IniFile::from_str(ini_str))
            .expect("threat-range test rules should parse")
    }

    fn obj<'a>(rules: &'a RuleSet, id: &str) -> &'a ObjectType {
        rules.object(id).expect("test type present")
    }

    fn type_weapon_ranges(rules: &RuleSet, obj: &ObjectType, veterancy: u16) -> [i32; 2] {
        let mut actor = crate::sim::game_entity::GameEntity::test_default(1, &obj.id, "Test", 0, 0);
        actor.set_veterancy_rank(veterancy);
        let entities = crate::sim::entity_store::EntityStore::new();
        let interner = crate::sim::intern::test_interner();
        std::array::from_fn(|index| {
            super::super::combat_weapon::weapon_range(
                &actor,
                obj,
                index as i32,
                &entities,
                rules,
                &interner,
            )
        })
    }

    #[test]
    fn guard_without_guard_range_defers_to_can_fire_at() {
        // Retail radius 0 — the acceptance test falls through to the
        // attacker's own can-fire-at query rather than applying a cutoff.
        let rules = test_rules();
        assert_eq!(
            scan_range(
                obj(&rules, "NOGUARD"),
                ScanMission::Guard.literal_mask(),
                type_weapon_ranges(&rules, obj(&rules, "NOGUARD"), 0)
            ),
            ScanRange::CanFireAt
        );
    }

    /// The mask literals are the whole argument, so pin the consequence: for
    /// one and the same type, Guard and Area Guard must NOT resolve to the same
    /// filter. Guard's caller pushes mask 1 (narrow formula → radius 0 for a
    /// type with no `GuardRange=` → defer to can-fire-at); Area Guard's pushes
    /// mask 2 (doubled formula → a hard cutoff). A regression that made plain
    /// Guard take the doubled radius would collapse these two into one value.
    #[test]
    fn guard_and_area_guard_do_not_share_a_filter() {
        let rules = test_rules();
        for type_id in ["NOGUARD", "WITHGUARD", "ONLYPRIMARY"] {
            let guard = scan_range(
                obj(&rules, type_id),
                ScanMission::Guard.literal_mask(),
                type_weapon_ranges(&rules, obj(&rules, type_id), 0),
            );
            let area = scan_range(
                obj(&rules, type_id),
                ScanMission::AreaGuard.literal_mask(),
                type_weapon_ranges(&rules, obj(&rules, type_id), 0),
            );
            assert_ne!(
                guard, area,
                "{type_id}: Guard (mask 1) and Area Guard (mask 2) select different formulas"
            );
            assert!(
                matches!(area, ScanRange::Hard(_)),
                "{type_id}: Area Guard is always a hard cutoff"
            );
        }
        // And the doubling is Area Guard's alone: a type WITH GuardRange keeps
        // it undoubled on Guard.
        assert_eq!(
            scan_range(
                obj(&rules, "WITHGUARD"),
                ScanMission::Guard.literal_mask(),
                type_weapon_ranges(&rules, obj(&rules, "WITHGUARD"), 0)
            ),
            ScanRange::Hard(9 * 256)
        );
    }

    #[test]
    fn guard_with_guard_range_is_a_hard_undoubled_cutoff() {
        // GuardRange=9 on plain Guard is used as-is. The doubling belongs to
        // the Area Guard branch only — a V3 on Guard scans 9 cells, not 18.
        let rules = test_rules();
        assert_eq!(
            scan_range(
                obj(&rules, "WITHGUARD"),
                ScanMission::Guard.literal_mask(),
                type_weapon_ranges(&rules, obj(&rules, "WITHGUARD"), 0)
            ),
            ScanRange::Hard(9 * 256)
        );
    }

    #[test]
    fn area_guard_without_guard_range_doubles_the_wider_weapon() {
        // max(Primary 4, Secondary 6) = 6, doubled = 12. Note it is the wider
        // of the two SLOTS, not the weapon that would be picked against any
        // particular candidate.
        let rules = test_rules();
        assert_eq!(
            scan_range(
                obj(&rules, "NOGUARD"),
                ScanMission::AreaGuard.literal_mask(),
                type_weapon_ranges(&rules, obj(&rules, "NOGUARD"), 0)
            ),
            ScanRange::Hard(12 * 256)
        );
    }

    #[test]
    fn area_guard_prefers_guard_range_over_the_weapon_and_clamps_the_double() {
        // GuardRange=9 wins over max weapon range 6 (proved by the result
        // exceeding the 2 * 6 = 12 the weapon path would give), is itself
        // doubled to 18, and is then clamped to the 16-cell ceiling.
        let rules = test_rules();
        let clamped = scan_range(
            obj(&rules, "WITHGUARD"),
            ScanMission::AreaGuard.literal_mask(),
            type_weapon_ranges(&rules, obj(&rules, "WITHGUARD"), 0),
        );
        let ScanRange::Hard(leptons) = clamped else {
            panic!("Area Guard always produces a hard cutoff");
        };
        assert!(leptons > 12 * 256, "GuardRange must win: {leptons}");
        assert_eq!(clamped, ScanRange::Hard(16 * 256));
    }

    #[test]
    fn area_guard_uses_the_only_slot_when_the_type_has_one_weapon() {
        // Primary 4 only -> 8. A missing Secondary must not drag the max to 0.
        let rules = test_rules();
        assert_eq!(
            scan_range(
                obj(&rules, "ONLYPRIMARY"),
                ScanMission::AreaGuard.literal_mask(),
                type_weapon_ranges(&rules, obj(&rules, "ONLYPRIMARY"), 0)
            ),
            ScanRange::Hard(8 * 256)
        );
    }

    /// `tools/spatial_oracle/threat_range_cargo.json`: original
    /// `GetWeaponRange 0x007012C0` and `Threat_Range 0x00707E60` on a Unit
    /// transport, closed and open-topped, with infantry and unit riders.
    /// All four recorded scalar modes go through the shared range owner;
    /// the scanner adapter separately covers Hunt, Guard and Area Guard.
    #[test]
    fn original_threat_range_and_cargo_rows() {
        use crate::sim::combat::combat_weapon::weapon_range;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::passenger::{PassengerCargo, PassengerRole};

        let payload: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/threat_range_cargo.json",
        ))
        .unwrap();
        assert_eq!(payload["modes"], serde_json::json!([-1, 0, 1, 2]));
        let rows = payload["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 26);
        let lookup = |group: &serde_json::Value, defaults: &serde_json::Value, key: &str| {
            group.get(key).unwrap_or(&defaults[key]).clone()
        };
        // Ranges are whole multiples of 128 leptons, exact as `Range=` cells.
        let cells = |leptons: i64| format!("{}", leptons as f64 / 256.0);
        let veterancy = |value: serde_json::Value| (value.as_f64().unwrap() * 100.0) as u16;
        let weapon_keys = |turreted: bool| {
            if turreted {
                ["Weapon1", "Weapon2", "EliteWeapon1", "EliteWeapon2"]
            } else {
                ["Primary", "Secondary", "ElitePrimary", "EliteSecondary"]
            }
        };
        for row in rows {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let defaults = &payload["defaults"];
            let mut infantry = String::new();
            let mut vehicles = String::from("0=TRN\n");
            let mut sections = String::new();
            let mut weapons = |owner: &str, spec: &serde_json::Value, turreted: bool| -> String {
                let mut keys = String::new();
                for (slot, key) in ["slot0", "slot1", "elite_slot0", "elite_slot1"]
                    .into_iter()
                    .zip(weapon_keys(turreted))
                {
                    if let Some(range) = spec[slot].as_i64() {
                        keys += &format!("{key}={owner}{slot}\n");
                        sections += &format!(
                            "[{owner}{slot}]\nDamage=10\nWarhead=WH\nRange={}\n",
                            cells(range)
                        );
                    }
                }
                keys
            };
            let transport_weapons = {
                let mut spec = defaults["weapons"].clone();
                for (key, value) in input
                    .get("weapons")
                    .and_then(|group| group.as_object())
                    .into_iter()
                    .flatten()
                {
                    spec[key] = value.clone();
                }
                weapons("TRN", &spec, false)
            };
            let open_topped = lookup(input, defaults, "open_topped").as_i64().unwrap() != 0;
            let guard_range = lookup(input, defaults, "guard_range").as_i64().unwrap();
            let mut types = format!(
                "[TRN]\nStrength=100\nOpenTopped={}\nGuardRange={}\n{transport_weapons}",
                if open_topped { "yes" } else { "no" },
                cells(guard_range),
            );
            let riders: Vec<serde_json::Value> = input
                .get("passengers")
                .and_then(|list| list.as_array())
                .cloned()
                .unwrap_or_default();
            let rider_defaults = &payload["passenger_defaults"];
            for (index, rider) in riders.iter().enumerate() {
                let rider_type = format!("P{index}");
                let turret_count = lookup(rider, rider_defaults, "turret_count")
                    .as_i64()
                    .unwrap();
                let mut spec = rider_defaults.clone();
                for (key, value) in rider.as_object().unwrap() {
                    spec[key] = value.clone();
                }
                let keys = weapons(&rider_type, &spec, turret_count > 0);
                if lookup(rider, rider_defaults, "class") == "unit" {
                    vehicles += &format!("{}={rider_type}\n", index + 1);
                } else {
                    infantry += &format!("{index}={rider_type}\n");
                }
                types += &format!(
                    "[{rider_type}]\nStrength=100\nTurretCount={turret_count}\nWeaponCount=2\n{keys}"
                );
            }
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[InfantryTypes]\n{infantry}[VehicleTypes]\n{vehicles}[AircraftTypes]\n\
                 [BuildingTypes]\n{types}{sections}[WH]\nVerses=100%,100%,100%,100%,100%,\
                 100%,100%,100%,100%,100%,100%\n"
            )))
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));

            let mut entities = EntityStore::new();
            let mut cargo = PassengerCargo::new(5, 100);
            for (index, rider) in riders.iter().enumerate().rev() {
                let id = 10 + index as u64;
                let mut passenger =
                    GameEntity::test_default(id, &format!("P{index}"), "Test", 0, 0);
                passenger.set_veterancy_rank(veterancy(lookup(rider, rider_defaults, "veterancy")));
                let current = lookup(rider, rider_defaults, "current_weapon")
                    .as_u64()
                    .unwrap();
                passenger.set_gunner_selection_for_test(current as i32, -1);
                passenger.passenger_role = PassengerRole::Inside {
                    transport_id: 1,
                    open_topped,
                };
                entities.insert(passenger);
                assert!(cargo.board(id, 1));
            }
            let mut transport = GameEntity::test_default(1, "TRN", "Test", 0, 0);
            transport.set_veterancy_rank(veterancy(lookup(input, defaults, "veterancy")));
            transport.passenger_role = PassengerRole::Transport { cargo };
            let interner = crate::sim::intern::test_interner();
            let obj = rules.object("TRN").unwrap();

            let ranges = std::array::from_fn(|index| {
                weapon_range(&transport, obj, index as i32, &entities, &rules, &interner)
            });
            assert_eq!(serde_json::json!(ranges), row["weapon_range"], "{name}");
            let expected = |leptons: i64| match leptons {
                -1 => ScanRange::NoCutoff,
                0 => ScanRange::CanFireAt,
                leptons => ScanRange::Hard(leptons as i32),
            };
            let native = row["threat_range"].as_array().unwrap();
            for (index, mode) in [-1, 0, 1, 2].into_iter().enumerate() {
                assert_eq!(
                    threat_range_leptons(obj, mode, ranges),
                    native[index].as_i64().unwrap() as i32,
                    "{name}: scalar mode {mode}"
                );
            }
            for (mode, mission) in [
                (0, ScanMission::Hunt),
                (1, ScanMission::Guard),
                (2, ScanMission::AreaGuard),
            ] {
                assert_eq!(
                    scan_range(obj, mission.literal_mask(), ranges),
                    expected(native[mode].as_i64().unwrap()),
                    "{name}: {mission:?}"
                );
            }
        }
    }
}
