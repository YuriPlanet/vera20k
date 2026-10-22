//! `TechnoClass::Greatest_Threat @ 0x006F8DF0` — how an object picks what to
//! shoot when nobody told it to.
//!
//! This is the passive-acquisition scanner: the expanding-ring cell walk, the
//! one-candidate-per-cell rule, and the weighted score that ranks what the walk
//! finds. It is reached from `TechnoClass::AI_Update @ 0x006F9E50` through the
//! passive driver at `0x00709820` (vtable `+0x39C`) and the per-class `+0x3C4`
//! wrappers (`UnitClass @ 0x00743190`, `BuildingClass @ 0x00445F00`,
//! `FootClass @ 0x004D9920`).
//!
//! ## Shape of the native scan
//!
//! 1. Radius. `TechnoClass::Threat_Range @ 0x00707E60` (vtable `+0x31C`) turns
//!    the mission's mask into a lepton radius — that part lives in
//!    [`super::threat_range`]; a healer on Guard takes 0x200 instead
//!    (`0x006F9024..0x006F903E`). A zero radius means "no cutoff of the scan's
//!    own". The walk then uses the current weapon's range for a
//!    non-Gattling turreted type, GuardRange for an underwater, organic,
//!    self-healing type, or the wider of weapon slots 0/1 otherwise. It adds
//!    one cell and AirRangeBonus (`0x006F9068..0x006F9148`).
//! 2. Airborne pre-pass. A mask with the AA bit (`4`) sweeps the airborne
//!    bucket grid first (`0x006F91A7`), evaluating **every** airborne object in
//!    reach — aircraft and jumpjets — with the Unit and Infantry class bits
//!    added (`0x006F91C2`). They are therefore scored before any ground cell
//!    and win ties; a mask of exactly 5 walks no rings after it.
//! 3. Ring walk. `for r in 0..radius`: a row pass over `(cx-r..=cx+r, cy-r)`
//!    then `(…, cy+r)`, then a column pass over `(cx-r, cy+1-r..cy+r-1)` then
//!    `(cx+r, …)`. At `r == 0` the centre cell is visited twice — that is the
//!    literal loop, and it is harmless because the second visit re-picks the
//!    same candidate at the same score.
//! 4. Each cell contributes **at most one** candidate:
//!    `TechnoClass::Scan_Cell_For_Target @ 0x006F8960` walks a single object
//!    list — the bridge-deck list `CellClass+0xE8` when it is non-empty, else
//!    the ground list `CellClass+0xE4` — and stops at the first hostile entry.
//!    If that one entry is then rejected by the candidate gate, the cell yields
//!    nothing; the objects behind it are never looked at.
//! 5. The best is kept only on a **strictly greater** score (init −1), so ties
//!    go to whatever the walk reached first.
//! 6. After each ring, `if (best && (r == radius/4 || r == radius/2)) return`
//!    (`0x006F94F1`). A close-enough target ends the scan early — this is why a
//!    defence shoots the attacker at its feet rather than the juicier one three
//!    cells further out.
//!
//! ## The score
//!
//! `TechnoClass::Calculate_Threat_Score @ 0x0070CD10` is a weighted sum around
//! a `100000.0` base, truncated to an integer by the caller. Five coefficients
//! select between two sets on the scorer house's `HouseClass+0x1FB` byte
//! (`0x0070CD4E`): clear takes the `[General] Dumb*Coefficient` set, set takes
//! the scorer TYPE's own `MyEffectivenessCoefficient=` family. The byte is set
//! by `HouseClass::Constructor @ 0x004F644E` for any house built from a
//! `HouseTypeClass`, which is every house `ScenarioClass::Create_Houses @
//! 0x00687F10` makes, and nothing ever clears it — so the per-type set is live
//! from the first frame of the match. See [`HOUSE_SELECTS_OWN_COEFFICIENTS`].
//!
//! ## Shroud
//!
//! There is no shroud gate here, and the gate VERA used to apply
//! (`FogState::is_cell_visible` on the candidate's cell) has been removed.
//! `Evaluate_Candidate` was read end to end: its only visibility arms are the
//! cloak/sensor test at `0x006F7DA9` and the "discovered by the local player"
//! test at `0x006F81B8`, and the latter is behind `g_GameMode == 0` — campaign
//! only, since a skirmish runs mode 5. Retail therefore lets a skirmish unit
//! acquire an enemy standing in unexplored ground, and fires on it: no fire
//! path reads shroud either (GetFireError `0x006FC0B0` has no shroud code, and
//! `IsFogged @ 0x005865E0` is a constant false).
//!
//! ## Other residuals
//!
//! DistributedFire collects accepted air/ring scores here; its private
//! GameEntity owner implements709550 and the world commits class assignment.
//! Flat scans deliberately clear candidates without appending.
//!
//! - The ring walk's wall fallback (`0x006F8C10`, per ring cell, read not
//!   executed) is not ported: for a computer house it can return a cell
//!   holding a `Wall=` overlay that the scanner's `Wall=` warhead and AG
//!   projectile can hit, gated by a per-difficulty Rules byte (`+0x1581 +
//!   difficulty * 0x50`, identity UNCHECKED), as the scan's cell target
//!   (`0x006F9B55..0x006F9B5F`). Trigger: a computer unit on Guard or Area
//!   Guard whose rings reach a wall. Effect: it does not shoot the wall.
//! - The threat mask (the callsite literal, the class overrides and the
//!   flags word) is [`super::threat_mask`]'s; its residuals are listed there.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/ and sim/ only.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use std::ops::ControlFlow;

use super::combat_targeting::AttackerSnapshot;
use super::combat_weapon::{
    AttackerFacts, SelectedWeapon, attacker_facts, controlled_transport_house, get_weapon,
    is_ally_by_object, is_armed, is_armed_from_facts, is_occupied, occupied_reach_cells,
    passive_scan_class_bits, primary_warhead_spares_medium_and_wood, resolve_selected_weapon,
    resolve_weapon_index, techno_target_facts, weapon_damage_value, what_weapon_should_i_use,
};
use super::threat_mask::{self, InfantryScanner};
use super::threat_range::{ScanRange, scan_range};
use crate::map::cell_index::{NativeCellIdentity, square_ring_cells};
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::object_type::{ObjectType, VhpScan};
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::components::DriveCoord;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::{DistributedFireCandidate, GameEntity};
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::mission::MissionType;
use crate::sim::movement::ground_pose::{object_get_coords, position_world_coord};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellObjectMember, OccupancyGrid};
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::sim::rng::SimRng;
use crate::sim::vision::FogState;
use crate::util::lepton::{lepton_to_cell, lepton_to_cell_packed};
use crate::util::native_x87::{
    MaskedX87Chop53 as ScoreX87, MaskedX87Ordering, MaskedX87Value, NativeF64Bits,
};

/// `TechnoClass::Calculate_Threat_Score`'s additive base, `DAT_007F4E90`.
const THREAT_SCORE_BASE: f64 = 100_000.0;

/// Foot `+0x688`: a stopped object that could not fire at its target narrows
/// its next scans (`FootClass::Greatest_Threat @ 0x004D9920`). The Foot
/// constructor clears it (`0x004D33A8`). Independent of Techno's
/// passive-target byte `+0x50C`.
///
/// - [`Self::mark_stopped_cannot_fire`]: Drive (`0x004B2E9F`), Ship
///   (`0x006A24F2`) and Hover (`0x0051684D`) stop with a target they cannot
///   fire at, before the team's Scan_Limit and target clear; the team action
///   (`0x006EC3BD`) also sets it after each member's class target clear.
/// - [`Self::finish_scan`]: the Foot scan clears it after an empty completed
///   base scan (`0x004D9951..0x004D9955`); class overrides that return before
///   Foot's leave it.
/// - [`Self::clear_for_rescue`]: `Mission_Rescue` resets it before its direct
///   mask-0 scan (`0x004DE03E`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct RetargetAfterStop(bool);

impl RetargetAfterStop {
    /// Whether the next scans narrow.
    pub(crate) const fn get(self) -> bool {
        self.0
    }

    /// A stop with a held target it cannot fire at.
    pub(crate) fn mark_stopped_cannot_fire(&mut self) {
        self.0 = true;
    }

    /// The Foot scan's clear when the completed base scan found nothing.
    pub(crate) fn finish_scan(&mut self, found: bool) {
        if !found {
            self.0 = false;
        }
    }

    /// `Mission_Rescue`'s reset before its direct scan.
    pub(crate) fn clear_for_rescue(&mut self) {
        self.0 = false;
    }

    /// The Foot scan's mask (`0x004D9931..0x004D9933`): set, it clears bit 2
    /// and sets bit 1, changing AL only.
    pub(crate) const fn coerce_mask(self, mask: u32) -> u32 {
        if self.0 { (mask & !2) | 1 } else { mask }
    }
}

/// The five weights of `TechnoClass::Calculate_Threat_Score @ 0x0070CD10`, in
/// the order the native body loads them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ThreatCoefficients {
    /// `A` — the scorer's own weapon effectiveness against the candidate.
    pub my_effectiveness: f64,
    /// `B` — the candidate's weapon effectiveness against the scorer, negated
    /// when the candidate is already aiming at the scorer.
    pub target_effectiveness: f64,
    /// `C` — the candidate type's `SpecialThreatValue=`.
    pub target_special_threat: f64,
    /// `D` — the candidate's live health ratio.
    pub target_strength: f64,
    /// `E` — distance beyond the scorer's weapon range, in whole cells on the
    /// `NullCoord` branch and in LEPTONS on the supplied-coordinate one; see
    /// [`ThreatReference`].
    pub target_distance: f64,
}

impl ThreatCoefficients {
    /// The `HouseClass+0x1FB` branch at `0x0070CD4E`, which reads the byte on
    /// the SCORER's owner (`0x0070CD48: EAX = [EDI+0x21C]`, EDI = `this` = the
    /// attacker) and takes the scorer TYPE's own coefficients when it is set.
    ///
    /// Pass [`HOUSE_SELECTS_OWN_COEFFICIENTS`] for it; the clear branch is kept
    /// because it is the native alternative, not because VERA can reach it.
    pub(crate) fn resolve(
        rules: &RuleSet,
        scorer_type: &ObjectType,
        scorer_house_byte_set: bool,
    ) -> Self {
        let general = &rules.general;
        if scorer_house_byte_set {
            Self {
                my_effectiveness: scorer_type
                    .my_effectiveness_coefficient
                    .unwrap_or(general.my_effectiveness_coefficient_default),
                target_effectiveness: scorer_type
                    .target_effectiveness_coefficient
                    .unwrap_or(general.target_effectiveness_coefficient_default),
                target_special_threat: scorer_type
                    .target_special_threat_coefficient
                    .unwrap_or(general.target_special_threat_coefficient_default),
                target_strength: scorer_type
                    .target_strength_coefficient
                    .unwrap_or(general.target_strength_coefficient_default),
                target_distance: scorer_type
                    .target_distance_coefficient
                    .unwrap_or(general.target_distance_coefficient_default),
            }
        } else {
            Self {
                my_effectiveness: general.dumb_my_effectiveness_coefficient,
                target_effectiveness: general.dumb_target_effectiveness_coefficient,
                target_special_threat: general.dumb_target_special_threat_coefficient,
                target_strength: general.dumb_target_strength_coefficient,
                target_distance: general.dumb_target_distance_coefficient,
            }
        }
    }
}

/// `HouseClass+0x1FB` for every house VERA can create: **set**.
///
/// The byte has three writers in the whole image and none of them clears it
/// after construction:
/// - `HouseClass::Constructor @ 0x004F54A0` zeroes it at `0x004F5740`
///   (`MOV [EBP+0x1FB], BL` with `EBX == 0`) and then sets it at `0x004F644E`
///   (`MOV byte [EBP+0x1FB], 1`) behind `0x004F6448 CMP ESI,EBX / JZ` — ESI is
///   the constructor's `HouseTypeClass*` argument, the same pointer stored to
///   `HouseClass+0x34` and used for the country name copy a few lines later.
/// - `HouseClass::Mark_Has_Buildings @ 0x00509130` (`*(byte*)(this+0x1FB) = 1`
///   and nothing else), called only from `BuildingClass::Limbo @ 0x00445DEF`
///   and `BuildingClass::Unlimbo @ 0x00440A17`.
///
/// All four `HouseClass` constructions in `ScenarioClass::Create_Houses @
/// 0x00687F10` (`0x00687FC3`, `0x006881A0`, `0x006882FE`, `0x00688351`) push a
/// `HouseTypeClass*` taken from the type array at `0x00A83C9C` — by country
/// index for the player houses, by name lookup (`0x005117D0`) for the fixed
/// special houses. So the byte is 1 from the frame a house is created, the
/// Limbo/Unlimbo marker only re-sets an already-set byte, and the scorer TYPE's
/// own `MyEffectivenessCoefficient=` family is the live set for the whole
/// match — including the opening minute before the MCV deploys.
///
/// This replaces an earlier "does the house own a live building right now?"
/// derivation, which took the `[General] Dumb*Coefficient` branch at match
/// start and again after a house lost its last structure. Native takes neither.
pub(crate) const HOUSE_SELECTS_OWN_COEFFICIENTS: bool = true;

fn load_threat_double(value: f64) -> MaskedX87Value {
    ScoreX87::load_f64(NativeF64Bits::from_bits(value.to_bits()))
}

fn spill_threat_double(value: MaskedX87Value) -> MaskedX87Value {
    ScoreX87::load_f64(ScoreX87::store_f64_masked_chop(value))
}

/// Term C (`0x0070CEDC..0x0070CEF9`): the candidate type's
/// `SpecialThreatValue=` (`TechnoTypeClass+0x2C0`) weighted and added; then
/// `enemy_bonus`, `EnemyHouseThreatBonus=` when the scorer house's enemy
/// owns the candidate (`0x0070CEFD..0x0070CF19`).
fn special_threat_terms(
    score: MaskedX87Value,
    coefficient: MaskedX87Value,
    special_threat_value: f64,
    enemy_bonus: Option<f64>,
) -> MaskedX87Value {
    let score = spill_threat_double(ScoreX87::add(
        ScoreX87::mul(coefficient, load_threat_double(special_threat_value)),
        score,
    ));
    match enemy_bonus {
        Some(bonus) => spill_threat_double(ScoreX87::add(score, load_threat_double(bonus))),
        None => score,
    }
}

/// `TechnoClass::Calculate_Threat_Score`'s third parameter, in the only two
/// shapes its three callsites pass (`get_xrefs_to 0x0070CD10`).
///
/// The parameter is a `CoordStruct*`, and the first thing the distance term
/// does with it is compare all three words against the `NullCoord` globals
/// `0x00B0EA90/94/98` (`CMP EAX,ECX` / `CMP ECX,[0x00b0ea94] @ 0x0070CFA9` /
/// `CMP EDX,[0x00b0ea98] @ 0x0070CFB5`, then `JZ 0x0070D023 @ 0x0070CFBC`).
/// **The two branches are not the same computation at a different starting
/// point — they are the same geometry at a 256x different SCALE**, so which one
/// runs decides whether distance barely matters or dominates the whole score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreatReference {
    /// `PUSH 0xb0ea90` — the sentinel, taken by `TechnoClass::ShouldRetaliate`
    /// at both its callsites (`0x00708A81`, `0x00708A92`) and by every
    /// `Evaluate_Candidate` callsite except the mask-0 flat walk
    /// (`PUSH 0xb0ea90 @ 0x006F929A` on the ring walk, `@ 0x006F9C18` on the
    /// aircraft pre-walk).
    ///
    /// Native then measures scorer-to-candidate through both objects' own
    /// `vt+0x48` (`0x0070D02C`, `0x0070D03A`): a building's foundation centre
    /// (`BuildingClass::GetCoords @ 0x00447AC0`), any other object's Location
    /// (`ObjectClass::GetCoords @ 0x005F65A0`), both from
    /// [`object_get_coords`]. It converts the distance to **whole CELLS**:
    /// `CDQ ; AND EDX,0xff ; ADD EAX,EDX ; SAR EAX,0x8` at
    /// `0x0070D094`-`0x0070D09D`.
    NullCoord,
    /// `Greatest_Threat`'s actual coordinate argument, forwarded by the flat
    /// walk at6F9D64. Hunt supplies Object coords; Rescue supplies ArchiveTarget
    /// coords (4DE056). The distance remains in leptons (70D021), while the
    /// weapon range remains in cells. An exact [0,0,0] value takes NullCoord's
    /// object-distance/cell branch; [0,0,1] is a real coordinate.
    Coords([i32; 3]),
}

/// `TechnoClass::Calculate_Threat_Score @ 0x0070CD10`. The distance term's
/// scale is chosen by `reference`; see [`ThreatReference`].
///
/// Term order and the per-term 64-bit store are the native ones: every
/// intermediate is written back to the `[ESP+0x10]` double between terms, and
/// only the final `beyond*E + acc + 100000` is evaluated on the x87 stack.
///
/// `scorer_enemy_house` is the scorer house's current enemy (`+0x5600`,
/// written by `HouseClass::UpdateAngerNodes @ 0x00504842`); a candidate it
/// owns earns `[General] EnemyHouseThreatBonus=` (`0x0070CEE6..0x0070CF19`).
pub(crate) fn calculate_threat_score(
    entities: &EntityStore,
    scorer_id: u64,
    candidate_id: u64,
    rules: &RuleSet,
    interner: &StringInterner,
    terrain: Option<&ResolvedTerrainGrid>,
    alliances: Option<&HouseAllianceMap>,
    coefficients: ThreatCoefficients,
    reference: ThreatReference,
    scorer_enemy_house: Option<InternedId>,
) -> Option<MaskedX87Value> {
    let scorer = entities.get(scorer_id)?;
    let candidate = entities.get(candidate_id)?;
    let pair = ThreatPair {
        scorer,
        scorer_type: interner.object_type(scorer.type_ref(), rules)?,
        candidate,
        candidate_type: interner.object_type(candidate.type_ref(), rules)?,
        candidate_allies_scorer: is_ally_by_object(
            alliances,
            interner,
            candidate.owner(),
            scorer.owner(),
        ),
    };
    // A: the scorer's selected weapon against the candidate.
    let candidate_as_target = techno_target_facts(
        candidate,
        pair.candidate_type,
        terrain,
        is_ally_by_object(alliances, interner, scorer.owner(), candidate.owner()),
        rules,
        interner,
    );
    let scorer_weapon = resolve_selected_weapon(
        rules,
        scorer,
        pair.scorer_type,
        &attacker_facts(scorer, pair.scorer_type),
        Some(&candidate_as_target),
        entities,
        interner,
    );
    threat_score(
        &pair,
        scorer_weapon.as_ref(),
        entities,
        rules,
        interner,
        terrain,
        coefficients,
        reference,
        scorer_enemy_house,
    )
}

/// The two objects [`threat_score`] weighs, their types, and whether the
/// candidate's house counts the scorer's as an ally (the B term's question).
struct ThreatPair<'e, 'r> {
    scorer: &'e GameEntity,
    scorer_type: &'r ObjectType,
    candidate: &'e GameEntity,
    candidate_type: &'r ObjectType,
    candidate_allies_scorer: bool,
}

/// [`calculate_threat_score`] with the scorer's selected weapon against the
/// candidate (its A term) already chosen: a target scan has chosen it for the
/// candidate's verses gate.
fn threat_score(
    pair: &ThreatPair<'_, '_>,
    scorer_weapon: Option<&SelectedWeapon<'_>>,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    terrain: Option<&ResolvedTerrainGrid>,
    coefficients: ThreatCoefficients,
    reference: ThreatReference,
    scorer_enemy_house: Option<InternedId>,
) -> Option<MaskedX87Value> {
    let &ThreatPair {
        scorer,
        scorer_type,
        candidate,
        candidate_type,
        candidate_allies_scorer,
    } = pair;
    let coeff_a = load_threat_double(coefficients.my_effectiveness);
    let coeff_b = load_threat_double(coefficients.target_effectiveness);
    let coeff_c = load_threat_double(coefficients.target_special_threat);
    let coeff_d = load_threat_double(coefficients.target_strength);
    let coeff_e = load_threat_double(coefficients.target_distance);
    let mut score = ScoreX87::load_i32(0);

    // B: the candidate's selected weapon against the scorer. A candidate
    // already targeting the scorer contributes the negated term (`FCHS` at
    // `0x0070CEB9`).
    let scorer_as_target = techno_target_facts(
        scorer,
        scorer_type,
        terrain,
        candidate_allies_scorer,
        rules,
        interner,
    );
    if let Some(selected) = resolve_selected_weapon(
        rules,
        candidate,
        candidate_type,
        &attacker_facts(candidate, candidate_type),
        Some(&scorer_as_target),
        entities,
        interner,
    ) {
        let verses = ScoreX87::load_f64(
            selected
                .warhead
                .interaction(scorer_type.armor_id())
                .multiplier(),
        );
        let mut term = ScoreX87::mul(coeff_b, verses);
        if candidate
            .attack_target
            .target()
            .is_some_and(|target| target == super::TargetKind::Entity(scorer.stable_id()))
        {
            term = ScoreX87::neg(term);
        }
        score = spill_threat_double(term);
    }

    score = special_threat_terms(
        score,
        coeff_c,
        candidate_type.special_threat_value,
        (scorer_enemy_house == Some(candidate.owner()))
            .then_some(rules.general.enemy_house_threat_bonus),
    );

    // A: the scorer's selected weapon against the candidate, retained for the
    // native range term below.
    if let Some(selected) = scorer_weapon {
        let verses = ScoreX87::load_f64(
            selected
                .warhead
                .interaction(candidate_type.armor_id())
                .multiplier(),
        );
        score = spill_threat_double(ScoreX87::add(ScoreX87::mul(coeff_a, verses), score));
    }

    // D: live candidate health ratio (`ObjectClass::GetHealthRatio @ 0x005F5C60`).
    let health_ratio = candidate.health.ratio(candidate_type.strength);
    score = spill_threat_double(ScoreX87::add(ScoreX87::mul(health_ratio, coeff_d), score));

    // E: distance beyond the scorer's selected weapon range. With no weapon
    // selected native falls back to the scorer type's `GuardRange`
    // (`TechnoTypeClass+0x5B8`, `0x0070CF81`) — NOT `Sight`.
    //
    // The RANGE is always shifted to whole cells (`SAR EAX,0x8 @ 0x0070CF99`,
    // stored to `[ESP+0xC]` and reloaded as EDI at `0x0070D0A0`). The DISTANCE
    // is shifted only on the `NullCoord` branch, so the subtraction at
    // `SUB EAX,EDI @ 0x0070D0A9` mixes leptons with cells whenever a caller
    // supplies a coordinate. That is not a bug being reproduced blind: it is
    // what makes a hunting unit overwhelmingly nearest-first, because with
    // stock `[General] TargetDistanceCoefficientDefault=-10` against the
    // `100000` base at `0x0070D0C4`, anything past roughly 39 cells drives the
    // score negative and `Evaluate_Candidate`'s tail clamps it to 1.
    // Both objects' GetCoords (vt+0x48: a building's foundation centre):
    // `0x0070CFC7` for the candidate against a supplied coordinate,
    // `0x0070D02C`/`0x0070D03A` for both on the NullCoord branch. Either sum
    // is `(dx² + dz²) + dy²`.
    let (scorer_coord, null_coord) = match reference {
        ThreatReference::NullCoord | ThreatReference::Coords([0, 0, 0]) => {
            (object_get_coords(scorer, terrain), true)
        }
        ThreatReference::Coords([x, y, z]) => (DriveCoord { x, y, z }, false),
    };
    let candidate_coord = object_get_coords(candidate, terrain);
    let distance_leptons = crate::util::native_x87::sqrt_approx_length([
        scorer_coord.x.wrapping_sub(candidate_coord.x),
        scorer_coord.z.wrapping_sub(candidate_coord.z),
        scorer_coord.y.wrapping_sub(candidate_coord.y),
    ]);
    let distance = if null_coord {
        // `CDQ ; AND EDX,0xff ; ADD EAX,EDX ; SAR EAX,0x8` at `0x0070D094`.
        lepton_to_cell(distance_leptons)
    } else {
        // `JMP 0x0070D0A0 @ 0x0070D021` — the shift is on the other branch.
        distance_leptons
    };
    let range_cells = scorer_weapon.map_or_else(
        || {
            scorer_type
                .guard_range
                .map_or(0, |range| range.to_num::<i32>())
        },
        // Weapon+B4 leptons (`0x0070CF6F`), which `CDQ ; AND EDX,0xff ;
        // ADD EAX,EDX ; SAR EAX,8` (`0x0070CF90..0x0070CF99`) divides by 256
        // toward zero.
        |selected| selected.weapon.range_leptons / 256,
    );
    let beyond_range = distance.wrapping_sub(range_cells).max(0);
    score = ScoreX87::add(
        ScoreX87::mul(ScoreX87::load_i32(beyond_range), coeff_e),
        score,
    );
    Some(ScoreX87::add(score, load_threat_double(THREAT_SCORE_BASE)))
}

/// `EvaluateCandidate6F7CF7..6F7D13`, before its weapon-verses check.
fn rejects_vhp_candidate(mode: VhpScan, estimated_health: i32) -> bool {
    mode == VhpScan::Strong && estimated_health <= 0
}

/// Native `_ftol` at6F870B precedes VHPScan and the later integer modifiers.
fn truncate_score(score: MaskedX87Value) -> i32 {
    // Native reads low EAX, including wrapping signed64 results and masked
    // conversion-indefinite. A checked i32 conversion wrongly rejects them.
    ScoreX87::ftol_i32_low_masked(score)
}

/// `EvaluateCandidate6F8719..6F875F`; original-byte corpus: vhp_scan.json.
fn adjust_vhp_score(mode: VhpScan, estimated_health: i32, strength: i32, score: i32) -> i32 {
    if mode != VhpScan::Normal {
        score
    } else if estimated_health <= 0 {
        score / 2
    } else if estimated_health <= strength / 2 {
        score.wrapping_mul(2)
    } else {
        score
    }
}

/// Final6F8928..6F8939 acceptance, after all integer score modifiers.
fn finish_score(score: i32) -> Option<i32> {
    if score == 0 {
        return None;
    }
    Some(score.max(1))
}

/// The ring index at which the native walk returns early once it holds a
/// candidate (`0x006F94D0..0x006F94F1`).
fn is_early_return_ring(ring: i32, radius: i32) -> bool {
    ring == radius / 4 || ring == radius / 2
}

/// Walk bound in cells, `iStack_34` in `Greatest_Threat`.
///
/// A non-zero radius (`Threat_Range`'s, or a Guard healer's 0x200) is itself
/// the bound, truncated to cells toward zero (`0x006F9042..0x006F904D`). A zero
/// one — plain Guard on a type with no `GuardRange=` — selects a range:
/// `GetWeaponRange(CurrentWeaponNumber)` for a non-Gattling type with
/// `TurretCount > 0` (`0x006F9068..0x006F908E`), else `GuardRange` for an
/// underwater, organic, self-healing type (`0x006F9090..0x006F90DC`), else
/// the wider range of weapon slots 0/1. The selected range and AirRangeBonus
/// each truncate to cells toward zero, then the bound adds both and one cell.
/// This bounds the SEARCH; the candidate's own InRange still gates acceptance.
/// An occupied building replaces that prior bound with one ring beyond its
/// reach (`0x006F917D..0x006F91A7`; tools/garrison_oracle/weapon_range.json).
fn scan_radius_cells(
    scanner: &GameEntity,
    obj: &ObjectType,
    rules: &RuleSet,
    range: ScanRange,
    weapon_ranges: [i32; 2],
    entities: &EntityStore,
    interner: &StringInterner,
) -> i32 {
    if is_occupied(scanner, obj) {
        return occupied_reach_cells(obj, rules).wrapping_add(1);
    }
    match range {
        ScanRange::Hard(leptons) => lepton_to_cell(leptons),
        ScanRange::CanFireAt => {
            let leptons = if obj.turret_count > 0 && !obj.is_gattling {
                super::combat_weapon::weapon_range(
                    scanner,
                    obj,
                    scanner.current_weapon_number(),
                    entities,
                    rules,
                    interner,
                )
            } else if obj.underwater && obj.organic && obj.self_healing {
                obj.guard_range.map_or(0, |range| range.to_bits() >> 8)
            } else {
                weapon_ranges[0].max(weapon_ranges[1])
            };
            let air_bonus = obj.air_range_bonus.map_or(0, |bonus| bonus.to_bits() >> 8);
            lepton_to_cell(leptons)
                .wrapping_add(lepton_to_cell(air_bonus))
                .wrapping_add(1)
        }
        // Mask 0 computes no walk bound because it runs no walk — the jump at
        // `0x006F8FE2` skips `iStack_34` along with the rings.
        // [`greatest_threat`] takes the flat-list branch before it reaches
        // here; zero is returned so that any future caller which does ask
        // walks nothing rather than walking the map.
        ScanRange::NoCutoff => 0,
    }
}

/// Everything one scan needs that does not change between candidates.
/// Candidates scored side by side ([`flat_scores`]) each take a clone, so the
/// answer memos below are each worker's own.
#[derive(Clone)]
struct ScanContext<'a> {
    entities: &'a EntityStore,
    /// The live scanner, `Greatest_Threat`'s `this`.
    scanner: &'a GameEntity,
    /// Overlay/alliance state the InRange line-of-fire walk consults
    /// (0x006F7642). Target selection runs the same `TechnoClass::InRange`
    /// the fire site does, so it has to see the same walls and cliffs or a
    /// unit picks a target it can never legally shoot.
    los: crate::sim::combat::line_of_fire::LineOfFireInputs<'a>,
    rules: &'a RuleSet,
    interner: &'a StringInterner,
    attacker: &'a AttackerSnapshot,
    attacker_obj: &'a ObjectType,
    fog: Option<&'a FogState>,
    terrain: Option<&'a ResolvedTerrainGrid>,
    require_playfield_membership: bool,
    range: ScanRange,
    coefficients: ThreatCoefficients,
    zone_grid: Option<&'a ZoneGrid>,
    /// `Greatest_Threat`'s mask after the scanner's override and the
    /// preamble ([`scanner_mask`]).
    mask: u32,
    /// Actual Greatest_Threat arg2, used as the ring center and forwarded to
    /// Calculate_Threat_Score only by the flat Techno walk.
    scan_coord: [i32; 3],
    standing: ScannerStanding<'a>,
    /// The Cell lists, borrowed as they are: the cell walk reads the retained
    /// `+0xE8`/`+0xE4` head (`0x006F89BF..0x006F89CB`), which current position,
    /// alive state and virtual +78 cannot reconstruct (an initial Jumpjet
    /// Process can retain a ground-list member apart from its airborne
    /// registration). G10's building-in-cell test reads them too.
    cells: &'a OccupancyGrid,
    /// `GetWeaponDamageValue(-1) @ 0x006F3970` is negative: the scanner's
    /// weapon heals. The walks and Evaluate_Candidate ask it of every class
    /// (`0x006F7F3A`, `0x006F8A36`, `0x006F8B72`, `0x006F9C9F`); the preamble
    /// asks only infantry and units ([`threat_mask::preamble_rewrite`]).
    healer: bool,
    /// vt+0x330: an `Engineer=` infantryman (InfantryClass `0x005224D0`
    /// reads InfantryType `+0xEC3`; every other class answers 0).
    engineer: bool,
    /// Berserk (`+0x298`).
    berserk: bool,
    /// Infantry+6D8, read only by Evaluate_Candidate6F7EE6 and the
    /// cell stopping gate6F8ADC. The later offers/air/global gates omit it.
    psycho: bool,
    /// The house (`+0x21C`) of whoever mind-controls (`+0x2C0`) the
    /// `OpenTopped=` transport (`+0x11C`, TechnoType `+0x5E4`) the scanner
    /// rides: `BL` and `EBP` in Evaluate_Candidate (`0x006F7EA2..0x006F7ED8`),
    /// `[ESP+0x1C]` and `EBX` in the cell walk (`0x006F89D1..0x006F8A0A`),
    /// `[ESP+0x13]` in the global walks (`0x006F8FA8..0x006F8FD7`).
    controller_house: Option<InternedId>,
    /// The world GetFireError reads for Evaluate_Candidate's probe
    /// (`0x006F7CE8`). Every production caller supplies it; `None` only in
    /// unit tests of the walk and the score, which then skip the probe.
    fire_world: Option<&'a crate::sim::world::Simulation>,
    /// VERA-only: [`Self::is_ally`]'s answers so far, by candidate owner.
    /// A scan changes no alliance, and its cell walk asks about the same few
    /// houses for each object it passes.
    allies: std::cell::RefCell<Vec<(InternedId, bool)>>,
    /// VERA-only: [`Self::allies_scanner`]'s answers so far, by candidate owner.
    allies_of_scanner: std::cell::RefCell<Vec<(InternedId, bool)>>,
    /// VERA-only: the scanner's half of the G2b probe's facts, read at the
    /// first probe. A scan changes nothing of the scanner.
    probe_firer: std::cell::OnceCell<super::fire_error_world::FirerPart>,
    /// The scanner's selection-ladder facts (`attacker_facts`), read once.
    scanner_facts: AttackerFacts,
    /// Scenario+0 bit `0x800` while the scan runs: campaign's retained map
    /// `[SpecialFlags] HarvesterImmune=`, or a nonzero game mode's Harvester
    /// Truce session bit (`0x00687C16..0x00687C29`). World-less fixtures
    /// (walk and score unit tests) leave it off.
    harvester_immune: bool,
    /// VERA-only: the [`crate::rules::ruleset::RuleSet::harvester_unit_types`]
    /// names resolved through the interner once per scan, so the
    /// HarvesterImmune gate (`0x006F8009..0x006F8047`) compares ids instead
    /// of re-uppercasing each name per candidate. Empty while the bit is
    /// clear, the ordinary skirmish path.
    harvester_unit_ids: Vec<InternedId>,
    /// VERA-only: the Scenario stream (Scenario+0x218) the scan draws from,
    /// copied from `fire_world` at its first draw and handed back through
    /// [`ThreatScanOutcome`] for the caller to commit. Only the computer's
    /// disguise detection draws.
    scenario_rng: std::cell::RefCell<Option<SimRng>>,
    /// VERA-only: the draws so far, so side-by-side scoring can tell which
    /// candidate drew ([`flat_scores`]).
    scenario_draws: std::cell::Cell<u32>,
    /// VERA-only: [`ThreatScanOutcome`]'s `met_idle_disguise`.
    met_idle_disguise: std::cell::Cell<bool>,
}

impl ScanContext<'_> {
    /// [`disguise_detection_roll`] on the Scenario stream (Scenario+0x218)
    /// for the scanner's house.
    fn computer_detects_disguise(&self) -> bool {
        let Some(world) = self.fire_world else {
            return false;
        };
        self.scenario_draws.set(self.scenario_draws.get() + 1);
        disguise_detection_roll(
            self.scenario_rng
                .borrow_mut()
                .get_or_insert_with(|| world.scenario_rng.clone()),
            self.standing.house,
            self.rules,
        )
    }

    /// `Scan_Cell_For_Target @ 0x006F89BF..0x006F89CB`: the bridge-deck list
    /// (`+0xE8`) wins whenever it is not empty; only one list is ever walked.
    fn cell_list(&self, rx: u16, ry: u16) -> Option<MovementLayer> {
        let occupancy = self.cells.get(rx, ry)?;
        Some(if occupancy.is_empty_on(MovementLayer::Bridge) {
            MovementLayer::Ground
        } else {
            MovementLayer::Bridge
        })
    }

    fn alliances(&self) -> Option<&HouseAllianceMap> {
        self.los.alliances
    }

    /// `HouseClass::Is_Ally_ByObject` asked by the scanner's house
    /// (`0x006F7F22`; the walks read its `+0x21C` too, e.g. `0x006F8A76`,
    /// `0x006F9D4C`): the house itself or one its own mask names.
    fn is_ally(&self, candidate: &GameEntity) -> bool {
        self.is_ally_owner(candidate.owner())
    }

    /// [`Self::is_ally`] for any object `owner` owns.
    fn is_ally_owner(&self, owner: InternedId) -> bool {
        if let Some(&(_, ally)) = self
            .allies
            .borrow()
            .iter()
            .find(|(asked, _)| *asked == owner)
        {
            return ally;
        }
        let ally = is_ally_by_object(self.alliances(), self.interner, self.attacker.owner, owner);
        self.allies.borrow_mut().push((owner, ally));
        ally
    }

    /// Whether `owner`'s house counts the scanner's as an ally, the question
    /// the threat score's B term asks.
    fn allies_scanner(&self, owner: InternedId) -> bool {
        if let Some(&(_, ally)) = self
            .allies_of_scanner
            .borrow()
            .iter()
            .find(|(asked, _)| *asked == owner)
        {
            return ally;
        }
        let ally = is_ally_by_object(self.alliances(), self.interner, owner, self.attacker.owner);
        self.allies_of_scanner.borrow_mut().push((owner, ally));
        ally
    }

    /// Whether the cell walk stops at the first non-ally and offers it: a
    /// scanner that neither heals, nor is an engineer, nor attacks allies
    /// ([`Self::cell_walk_stops_at`], [`Self::offers`]).
    fn offers_first_non_ally(&self) -> bool {
        !self.healer && !self.engineer && !self.attacks_allies() && !self.psycho
    }

    /// The walks' house gates: `OnlyTargetHouseEnemy` (arg3) admits only the
    /// objects of the owner's current enemy, and none without one
    /// (`0x006F9D1C..0x006F9D3B`); mask `0x4000` (a healer's) admits only
    /// allied ones (`HouseClass::IsAlliedWith @ 0x004F9A50`,
    /// `0x006F9D3D..0x006F9D5A`).
    fn house_gates_admit(&self, candidate: &GameEntity) -> bool {
        if self.attacker.scan_mission.only_target_house_enemy()
            && self.standing.enemy() != Some(candidate.owner())
        {
            return false;
        }
        self.mask & 0x4000 == 0 || self.is_ally(candidate)
    }

    /// The controller's house asks about `candidate` (`0x006F7FF4`,
    /// `0x006F8AC7`).
    fn controller_allies(&self, candidate: &GameEntity) -> bool {
        self.controller_house.is_some_and(|house| {
            crate::sim::combat::combat_weapon::is_ally_by_object(
                self.alliances(),
                self.interner,
                house,
                candidate.owner(),
            )
        })
    }

    /// The walks pass an ally on for an `AttackFriendlies=` scanner
    /// (TechnoType `+0x6C0`), a berserk one or a controlled transport's
    /// passenger: the aircraft walks (`0x006F9B99..0x006F9BD2`,
    /// `0x006F91FB..0x006F9267`) and, with
    /// [`Self::scans_allies`], [`Self::offers`].
    fn attacks_allies(&self) -> bool {
        self.attacker_obj.attacks_friendlies() || self.berserk || self.controller_house.is_some()
    }

    /// A healer, or a computer house's engineer (`0x006F9C9B..0x006F9CE7`,
    /// `0x006F8B6E..0x006F8BBA`).
    fn scans_allies(&self) -> bool {
        self.healer || (self.engineer && !self.standing.human)
    }

    /// The Techno walk's ally skip (`0x006F9C92..0x006F9D0D`) and the cell
    /// walk's post-walk test (`0x006F8B5F..0x006F8BE0`): a non-ally, or an
    /// ally for a scanner that scans or attacks allies.
    fn offers(&self, candidate: &GameEntity) -> bool {
        !self.is_ally(candidate) || self.scans_allies() || self.attacks_allies()
    }

    /// The cell walk's arms (`0x006F8A30..0x006F8B3C`): whether it stops at
    /// `candidate`, a Techno other than the scanner.
    fn cell_walk_stops_at(&self, candidate: &GameEntity) -> bool {
        // Only the healer and engineer arms read the type.
        let obj = || self.interner.object_type(candidate.type_ref(), self.rules);
        // HealthRatio (`0x005F5C60`) against a Rules threshold.
        let ratio =
            |threshold| obj().map(|obj| candidate.health.compare_ratio(obj.strength, threshold));
        if self.healer {
            // `0x006F8A3F..0x006F8A6B`: a damaged ally (C0: below 1.0, or
            // unordered).
            return matches!(
                ratio(1.0),
                Some(MaskedX87Ordering::Less | MaskedX87Ordering::Unordered)
            ) && self.is_ally(candidate);
        }
        // `0x006F8A76..0x006F8AB2`: a non-ally, unless the scanner rides a
        // controlled transport; anything, for a berserk scanner.
        if (!self.is_ally(candidate) && self.controller_house.is_none())
            || self.attacker_obj.attacks_friendlies()
            || self.berserk
        {
            return true;
        }
        // `0x006F8AB8..0x006F8ACE`: what the transport's controller allies.
        if self.controller_allies(candidate) {
            return true;
        }
        //6F8AD0..6F8AE4 stops at the first Techno for Psycho Infantry,
        //even when the later6F8B5F..6F8BE0 offer rejects that ally. The
        //objects behind it are hidden; this is not global attacks_allies.
        if self.psycho {
            return true;
        }
        // `0x006F8AD0..0x006F8B3C`: an Area Guard (`+0xAC`) engineer stops at
        // a building at or below ConditionRed (`Rules+0x1708`, C0|C3) that
        // costs its owner something (BuildingType vt+0x84, `0x0045EDD0`).
        self.engineer
            && self.scanner.mission.current().known() == Some(MissionType::AreaGuard)
            && ratio(self.rules.general.condition_red)
                .is_some_and(|order| order != MaskedX87Ordering::Greater)
            && candidate.category() == EntityCategory::Structure
            && obj()
                .zip(self.fire_world)
                .is_some_and(|(obj, world)| world.cost_of(candidate.owner(), obj, self.rules) > 0)
    }

    /// G10, `0x006F7EA2..0x006F7FFB`: whether the ally arm refuses
    /// `candidate`.
    fn ally_arm_refuses(&self, candidate: &GameEntity, candidate_obj: &ObjectType) -> bool {
        // `0x006F7EDA..0x006F7F1C`: an `AttackFriendlies=` scanner, an
        // `InfantryClass+0x6D8` infantryman,
        // a berserk one or a controlled transport's passenger skips its own house's
        // test; the passenger refuses what the controller's house allies
        // (`0x006F7FE9..0x006F7FFB`).
        if self.psycho || self.attacks_allies() {
            return self.controller_allies(candidate);
        }
        if !self.is_ally(candidate) {
            return false;
        }
        // `0x006F7F36..0x006F7F49`: only a healer or an engineer takes an
        // ally, and only a damaged one (`0x006F7F4F..0x006F7F67`: C3, equal or
        // unordered, refuses).
        if (!self.healer && !self.engineer)
            || matches!(
                candidate.health.compare_ratio(candidate_obj.strength, 1.0),
                MaskedX87Ordering::Equal | MaskedX87Ordering::Unordered
            )
        {
            return true;
        }
        if self.attacker.category != EntityCategory::Unit {
            return false;
        }
        if candidate.category() == EntityCategory::Aircraft {
            // `0x006F7F85..0x006F7FB7`: a vehicle takes an aircraft on the
            // ground (GetHeight, vt+0x1C8), and not on a building's cell (the
            // cell at its GetCoords, `0x00565730`; `0x0047C520` walks `+0xE4`).
            let coords = object_get_coords(candidate, self.terrain);
            return crate::sim::movement::air_movement::current_fly_height(candidate, self.terrain)
                > 0
                || self
                    .cells
                    .first_building_on_layer(
                        lepton_to_cell(coords.x) as u16,
                        lepton_to_cell(coords.y) as u16,
                        MovementLayer::Ground,
                    )
                    .is_some();
        }
        // `0x006F7FC5..0x006F7FDD`: anything else answers vt+0x80.
        !vehicle_like(self, candidate, candidate_obj)
    }

    /// The candidate's owner is `MultiplayPassive` (HouseType `+0x1A6`).
    fn owner_is_passive(&self, candidate: &GameEntity) -> bool {
        self.fire_world
            .and_then(|world| world.houses.get(&candidate.owner()))
            .is_some_and(|house| house.multiplay_passive())
    }

    /// The exception both passive-house gates share (`0x006F8292..0x006F835E`,
    /// `0x006F83B1..0x006F847D`): outside the campaign, a computer house's
    /// infantryman outside a base-defence team may take a building it would
    /// capture as an engineer (`NeedsEngineer=` `+0x1552`, another owner) or
    /// enter as an occupier (`Occupier=` `+0xEB4`, `0x00457CE0`).
    fn passive_building_exception(
        &self,
        candidate: &GameEntity,
        candidate_obj: &ObjectType,
    ) -> bool {
        if self.standing.human
            || !self.standing.game_mode_nonzero
            || candidate.category() != EntityCategory::Structure
            || self.attacker.category != EntityCategory::Infantry
            || self.standing.team == Some(true)
        {
            return false;
        }
        let capture = candidate_obj.needs_engineer
            && self.attacker_obj.engineer
            && candidate.owner() != self.attacker.owner;
        capture
            || (self.attacker_obj.occupier
                && self.fire_world.is_some_and(|world| {
                    let (Some(scanner), Some(cargo)) = (
                        self.entities.get(self.attacker.stable_id),
                        candidate.passenger_role.cargo(),
                    ) else {
                        return false;
                    };
                    crate::sim::passenger::can_dock_occupier_garrison(
                        scanner,
                        candidate,
                        self.attacker_obj,
                        candidate_obj,
                        cargo,
                        self.rules,
                        world,
                    )
                }))
    }
}

/// What one walk passes `Evaluate_Candidate` besides the candidate: the
/// flags word (arg2), the zone (arg6) and the coordinate (arg7).
#[derive(Debug, Clone, Copy)]
struct WalkArgs {
    /// The flags word ([`threat_mask::flags_for`]), which the flat walk and
    /// the ring walk widen with `4` before their Techno/cell walks when the
    /// mask has `0x10` (`0x006F9C56..0x006F9C63`, `0x006F938B..0x006F9393`);
    /// the ring walk's air pre-pass adds `0x8002` (`0x006F91C2`).
    flags: u32,
    /// `[ESP+0x3C]` in `Greatest_Threat` — the scanner's own movement-zone
    /// component id, or `None` for native's `-1` "gate off".
    ///
    /// Native seeds the slot with `-1` (`OR EBP,0xffffffff @ 0x006F8DFF`,
    /// `MOV [ESP+0x3c],EBP @ 0x006F8E15`) and overwrites it at `0x006F8EC4`
    /// with `MapClass::GetZoneID(my cell, myType->MovementZone, 1)` when the
    /// mask has bit0 CLEAR (`TEST BL,0x1 ; JNZ 0x006F8EC8` at
    /// `0x006F8E48`) and `What_Am_I()` is neither `6` (Building) nor `2`
    /// (Aircraft) — the two classes with no movement zone (`CMP EAX,0x6 ; JZ`
    /// at `0x006F8E54`, `CMP EAX,0x2 ; JZ` at `0x006F8E60`).
    ///
    /// It reaches [`evaluate_candidate`] as `Evaluate_Candidate`'s **arg6**
    /// only on the mask-0 flat walk (`PUSH ECX @ 0x006F9D69`); every ring-walk
    /// callsite fills the same slot with the literal `-1`
    /// (`PUSH -0x1 @ 0x006F92A3`, `@ 0x006F9C21`), so the gate is a property of
    /// the flat walk, not of the shared candidate ladder.
    zone: Option<u32>,
    /// `Evaluate_Candidate`'s **arg7**, the coordinate
    /// `Calculate_Threat_Score` measures its distance term from — and, through
    /// the sentinel test at `0x0070CFA0`-`0x0070CFBC`, the term's SCALE.
    ///
    /// Like [`Self::zone`] this is a property of the walk and not of
    /// the shared candidate ladder, and the two walks disagree in the same
    /// direction: the flat walk forwards `Greatest_Threat`'s own arg2
    /// (`PUSH EBP @ 0x006F9D64`, loaded `MOV EBP,[ESP+0x74] @ 0x006F9C76`),
    /// which for `Mission_Hunt` is a copy of the hunter's Coords, while the
    /// ring walk and the aircraft pre-walk push the `NullCoord` sentinel
    /// (`PUSH 0xb0ea90` at `0x006F929A` and `0x006F9C18`).
    reference: ThreatReference,
}

/// The scanner's house and team, as `Greatest_Threat`'s gates read them.
#[derive(Debug, Clone, Copy)]
struct ScannerStanding<'a> {
    /// The owner (`+0x21C`); `None` without a world.
    house: Option<&'a crate::sim::house_state::HouseState>,
    /// `HouseClass::IsControlledByHuman @ 0x0050B730` of the owner.
    human: bool,
    /// `GameMode != 0` (`[0x00A8B238]`): every mode but the campaign.
    game_mode_nonzero: bool,
    /// A Foot in a team (`+0x14 & 4`, `FootClass+0x5D4`), and whether its
    /// TeamType is `IsBaseDefense=` (`+0xF6`).
    team: Option<bool>,
}

impl<'a> ScannerStanding<'a> {
    /// With no world (unit tests of the walk and the score) the scanner is a
    /// human player's outside any team, the case those fixtures describe.
    fn resolve(
        world: Option<&'a crate::sim::world::Simulation>,
        attacker: &AttackerSnapshot,
    ) -> Self {
        let Some(world) = world else {
            return Self {
                house: None,
                human: true,
                game_mode_nonzero: true,
                team: None,
            };
        };
        let house = world.houses.get(&attacker.owner);
        Self {
            house,
            human: house.is_none_or(|house| {
                house.is_controlled_by_human(world.session.game_mode_nonzero())
            }),
            game_mode_nonzero: world.session.game_mode_nonzero(),
            team: (attacker.category != EntityCategory::Structure)
                .then(|| world.team_script_vm.team_for_member(attacker.stable_id))
                .flatten()
                .map(|(_, is_base_defense)| is_base_defense),
        }
    }

    /// The owner's current enemy (`House+0x5600`).
    fn enemy(&self) -> Option<InternedId> {
        self.house.and_then(|house| house.enemy_house())
    }
}

/// `0x006F851C..0x006F854B`: a computer attacker sees through a blinking
/// disguise when `RandomRanged(0, 99)` is at most its house's
/// `[General] DisabledDisguiseDetectionPercent=` entry (Rules+0xE10) for its
/// difficulty (`+0x184`).
fn disguise_detection_roll(
    rng: &mut SimRng,
    house: Option<&crate::sim::house_state::HouseState>,
    rules: &RuleSet,
) -> bool {
    let roll = rng.next_range_i32_inclusive(0, 99);
    roll <= house.map_or(0, |house| {
        house.difficulty_value(&rules.general.disabled_disguise_detection_percent)
    })
}

/// Result of the concrete Greatest_Threat dispatch. World callers use the
/// entry fact to apply Foot4D9955's latch write without clearing it when an
/// Infantry class gate returned before reaching Foot4D9920.
#[derive(Debug, Clone, Default)]
pub(crate) struct ThreatScanOutcome {
    target: Option<u64>,
    foot_wrapper_entered: bool,
    collected: Option<Vec<DistributedFireCandidate>>,
    /// The Scenario stream after the scan's draws, `None` when it drew
    /// nothing ([`ScanContext::computer_detects_disguise`]).
    scenario_rng: Option<SimRng>,
    /// The scan refused a disguised candidate only because it was not
    /// blinking: a blink starting later would have it roll.
    met_idle_disguise: bool,
}

impl ThreatScanOutcome {
    pub(crate) fn take_collected(&mut self) -> Option<Vec<DistributedFireCandidate>> {
        self.collected.take()
    }

    /// The Scenario stream the scan left, for the caller to commit.
    pub(crate) fn take_scenario_rng(&mut self) -> Option<SimRng> {
        self.scenario_rng.take()
    }

    /// The scan drew from the Scenario stream.
    pub(crate) const fn drew_scenario_rng(&self) -> bool {
        self.scenario_rng.is_some()
    }

    /// See [`Self::met_idle_disguise`]'s field.
    pub(crate) const fn met_idle_disguise(&self) -> bool {
        self.met_idle_disguise
    }

    pub(crate) const fn target(&self) -> Option<u64> {
        self.target
    }

    pub(crate) const fn foot_wrapper_entered(&self) -> bool {
        self.foot_wrapper_entered
    }
}

/// `TechnoClass::Greatest_Threat @ 0x006F8DF0` — the scan a passive object runs
/// to choose a target. Returns its winner and whether Foot was reached.
///
/// `scan_coord` is the scan's arg2. `None` stands for the scanner's own
/// GetCoords (vt+0x48), which `TechnoClass::AI_Update`'s passive scan passes
/// (`0x006FA6D4`); Mission_Hunt and the direct team and aircraft callers pass
/// the Location, the same point for the Foot and Aircraft scanners they serve.
///
/// ## Two topologies, selected by the caller's threat mask
///
/// The mask is `Greatest_Threat`'s own second argument, a literal at every
/// callsite (see [`super::threat_range::ScanMission`]), and the first thing the
/// body does with it is
/// `MOV AL,[ESP+0x70] ; TEST AL,0x3 ; JZ 0x006F9B6E` at `0x006F8FDC`-
/// `0x006F8FE2`. With bit0 or bit1 set the function runs the radius block, the
/// airborne pre-pass (`0x006F91A7`) and the expanding-ring cell walk. With
/// **neither** set — Hunt's mask 0 (`FootClass::Mission_Hunt @ 0x004D5373`)
/// or a team's quarry mask (`0x006ED15E`) — the jump lands past all three, and
/// the scan that actually runs is the flat walk over the global object array;
/// see [`global_list_scan`].
///
/// Modelling that as a wider *radius* would be wrong twice over: the ring walk
/// does not enumerate every object (one candidate per cell, and it stops at the
/// `radius/4` and `radius/2` bands), and mask 0 does not reach the ring walk at
/// all.
///
/// ## The mask is a callsite literal that two overrides may still rewrite
///
/// Every callsite pushes a constant, but a `FootClass` dispatch does not land
/// here directly — it lands in the per-class `+0x3C4` overrides first, and both
/// of them can change what arrives:
///
/// - `UnitClass::Greatest_Threat @ 0x00743190` ORs the attacker's own
///   projectile class bits into the mask (`FUN_00772A90`, AA → `4`,
///   AG → `0xB8`) whenever `mask & 0x1B978 == 0`, which mask 0 satisfies. The
///   topology is untouched — neither `4` nor `0xB8` carries `TEST AL,0x3` — but
///   the derived flags word `[ESP+0x14]` that becomes `Evaluate_Candidate`'s
///   arg2 does change, and an AA-capable hunter additionally runs the aircraft
///   list pre-walk at `0x006F9B7E`. See the class-bit residual in this module's
///   header; it now reaches the flat walk too.
/// - `FootClass::Greatest_Threat @ 0x004D9920` rewrites the mask to
///   `(mask & ~2) | 1` — mask 0 becomes mask 1, so the RING walk runs — while
///   `FootClass+0x688` is set, and clears that byte at `0x004D9955` only when
///   the coerced scan **returned 0** (`TEST EAX,EAX ; JNZ 0x004D995B` at
///   `0x004D9951` jumps past the store otherwise).
///
/// The Foot override changes the mask before the range, zone and topology
/// decisions. World dispatch applies its empty-result latch write only when
/// the concrete class reached Foot. Drive/Ship stopped-target tails and
/// Team6EC3A0 write the same private, persisted byte; Rescue clears it before
/// its direct scan. Hover5164D0 still requires its native path/arrival
/// continuation; the generic Hover finalizer cannot substitute for it.
/// 69B170 writes Session+688, not Foot state.
///
/// `zone_grid` supplies `MapClass`'s per-movement-zone connectivity for the
/// flat walk's gate ([`WalkArgs::zone`]) and the ring walk's per-cell gate
/// ([`scan_cell_for_target`]).
pub(crate) fn greatest_threat(
    entities: &EntityStore,
    occupancy: &OccupancyGrid,
    rules: &RuleSet,
    interner: &StringInterner,
    attacker: &AttackerSnapshot,
    attacker_obj: &ObjectType,
    fog: Option<&FogState>,
    terrain: Option<&ResolvedTerrainGrid>,
    require_playfield_membership: bool,
    zone_grid: Option<&ZoneGrid>,
    los: crate::sim::combat::line_of_fire::LineOfFireInputs<'_>,
    fire_world: Option<&crate::sim::world::Simulation>,
    scan_coord: Option<[i32; 3]>,
) -> ThreatScanOutcome {
    // The scan runs on a live scanner: its one caller,
    // `combat_targeting::greatest_threat_for_entity`, resolves it first.
    let Some(scanner) = entities.get(attacker.stable_id) else {
        return ThreatScanOutcome::default();
    };
    let scan_coord = scan_coord.unwrap_or_else(|| {
        let DriveCoord { x, y, z } = object_get_coords(scanner, terrain);
        [x, y, z]
    });
    let facts = attacker_facts(scanner, attacker_obj);
    let standing = ScannerStanding::resolve(fire_world, attacker);
    let healer = weapon_damage_value(scanner, attacker_obj, entities, rules, interner) < 0;
    let controller_house = controlled_transport_house(entities, rules, interner, scanner);
    // House+54E0 within `0xF00` leptons, centre to centre (vt+0x48 of both):
    // `(dx² + dz²) + dy²` through `Sqrt_Approx` and ftol
    // (`0x0051E16E..0x0051E1F6`).
    let lost_building = || {
        let building = standing.house?.lost_building()?;
        let from = object_get_coords(scanner, terrain);
        let to = object_get_coords(entities.get(building)?, terrain);
        let distance = crate::util::native_x87::sqrt_approx_length([
            from.x.wrapping_sub(to.x),
            from.z.wrapping_sub(to.z),
            from.y.wrapping_sub(to.y),
        ]);
        (distance < 0xF00).then_some(building)
    };
    let (mask, foot_wrapper_entered) = match scanner_mask(
        rules,
        attacker,
        attacker_obj,
        facts,
        |index| get_weapon(scanner, attacker_obj, index, entities, rules, interner),
        standing.human,
        healer,
        scanner,
        lost_building,
    ) {
        ControlFlow::Continue(scan) => scan,
        // An override returned before Foot `0x004D9920`.
        ControlFlow::Break(target) => {
            return ThreatScanOutcome {
                target,
                foot_wrapper_entered: false,
                collected: None,
                scenario_rng: None,
                met_idle_disguise: false,
            };
        }
    };
    let result = |target, collected, ctx: &ScanContext<'_>| ThreatScanOutcome {
        target,
        foot_wrapper_entered,
        collected,
        scenario_rng: ctx.scenario_rng.take(),
        met_idle_disguise: ctx.met_idle_disguise.get(),
    };
    // Base GreatestThreat6F8F80..6F8FA5 clears these two lists, retaining
    // history. Class returns before the base must preserve the prior lists.
    let mut collected = attacker_obj.distributed_fire.then(Vec::new);
    // Only a radius path without GuardRange needs the live range readers:
    // Threat_Range707ED0 or the zero-radius walk bound6F90DE. Mask0 and a
    // retained GuardRange bypass them. Do not duplicate GetWeaponRange's
    // rank/cargo logic in the scanner.
    let weapon_ranges = if mask & 3 != 0
        && attacker_obj
            .guard_range
            .is_none_or(|range| range.to_bits() == 0)
    {
        std::array::from_fn(|index| {
            super::combat_weapon::weapon_range(
                scanner,
                attacker_obj,
                index as i32,
                entities,
                rules,
                interner,
            )
        })
    } else {
        [0, 0]
    };
    // `0x006F9024..0x006F903E`: a healer on mission Guard (`+0xAC == 5`).
    let range = scan_range(
        attacker_obj,
        mask,
        weapon_ranges,
        healer,
        scanner.mission.current().known(),
    );
    let flags = threat_mask::flags_for(mask);

    // `TEST AL,0x3 ; JZ 0x006F9B6E @ 0x006F8FE0`: a mask without bit 0 or 1
    // takes the flat walk; every other mask walks the rings.
    let flat = mask & 3 == 0;
    // `MOV [ESP+0x3c],EAX @ 0x006F8EC4`, guarded by `TEST BL,0x1 ; JNZ` at
    // `0x006F8E48` and the two `What_Am_I()` skips at `0x006F8E54` /
    // `0x006F8E60`. The flat walk hands it to `Evaluate_Candidate` (G12), the
    // ring walk to `Scan_Cell_For_Target` (`PUSH ECX @ 0x006F9427`), so an Area
    // Guard ring scan (mask 2) skips cells outside the scanner's zone.
    let scanner_zone = if mask & 1 == 0
        && !matches!(
            attacker.category,
            EntityCategory::Structure | EntityCategory::Aircraft
        ) {
        zone_grid
            .zip(terrain)
            .and_then(|(zones, terrain)| {
                zones.get_zone_id_native(
                    terrain,
                    (attacker.pos_rx, attacker.pos_ry),
                    attacker_obj.movement_zone,
                    // `PUSH 0x1 @ 0x006F8E73` — the scanner's own cell is always
                    // asked with bridge resolution on, whatever layer it stands on.
                    true,
                )
            })
            // Both consumers skip the gate for DWORD -1 (`CMP EBP,-0x1 ; JZ` at
            // `0x006F7E36` and `0x006F898F`): a scanner on a structural cell
            // with no matching high bridge record scans unfiltered.
            .filter(|&zone| zone != u32::MAX)
    } else {
        None
    };

    // Scenario+0 bit `0x800`, read once per scan; the HarvesterImmune gate's
    // Rules list resolves with it so the ordinary bit-off scan builds nothing.
    let harvester_immune = fire_world.is_some_and(|world| world.session.harvester_immune());
    let ctx = ScanContext {
        entities,
        scanner,
        los,
        rules,
        interner,
        attacker,
        attacker_obj,
        fog,
        terrain,
        require_playfield_membership,
        range,
        coefficients: ThreatCoefficients::resolve(
            rules,
            attacker_obj,
            HOUSE_SELECTS_OWN_COEFFICIENTS,
        ),
        zone_grid,
        mask,
        scan_coord,
        standing,
        cells: occupancy,
        healer,
        engineer: attacker.category == EntityCategory::Infantry && attacker_obj.engineer,
        berserk: scanner.berserk.is_active(),
        psycho: attacker.category == EntityCategory::Infantry
            && scanner
                .infantry
                .as_ref()
                .is_some_and(|state| state.is_psycho()),
        controller_house,
        fire_world,
        allies: Default::default(),
        allies_of_scanner: Default::default(),
        probe_firer: Default::default(),
        scanner_facts: facts,
        harvester_immune,
        harvester_unit_ids: if harvester_immune {
            rules
                .harvester_unit_types
                .iter()
                .filter_map(|type_id| interner.get(type_id))
                .collect()
        } else {
            Vec::new()
        },
        scenario_rng: Default::default(),
        scenario_draws: Default::default(),
        met_idle_disguise: Default::default(),
    };

    if flat {
        return result(global_list_scan(&ctx, flags, scanner_zone), collected, &ctx);
    }

    let radius = scan_radius_cells(
        scanner,
        attacker_obj,
        rules,
        range,
        weapon_ranges,
        entities,
        interner,
    );
    if radius <= 0 {
        // `for (r = 0; r < radius; r++)` never executes.
        return result(None, collected, &ctx);
    }

    //6F9148..6F9179: arg2 is converted with signed /256 then packed to
    // CellStruct, independently of the scanner's own movement-zone query.
    let cx = i32::from((scan_coord[0] / 256) as i16);
    let cy = i32::from((scan_coord[1] / 256) as i16);
    // `AND EBX,0x4 @ 0x006F8F3A` gates the whole airborne pre-pass
    // (`TEST EBX,EBX @ 0x006F91A7`).
    let scan_air = mask & 4 != 0;
    let mut best: Option<u64> = None;
    // `local_50 = -1` — the keep test is strictly greater, so a score of 0 (the
    // lowest an accepted candidate can carry, and itself a rejection) can never
    // displace nothing.
    let mut best_score: i32 = -1;
    let mut walk = WalkArgs {
        flags,
        // `PUSH -0x1 @ 0x006F92A3`: the ring callsites pass no zone.
        zone: None,
        // `PUSH 0xb0ea90 @ 0x006F929A`.
        reference: ThreatReference::NullCoord,
    };

    // Airborne prepass: the mask's AA bit gates the entire pass
    // (6F8F3A -> 6F91A7), independently of target weapon selection.
    // Actual AirTracker registration alone does not grant eligibility:
    // 6F9251 requires Object+74, then6F9261 rejects live +78 Ground(2).
    // cell_list_layer_for_entity answers Some exactly when that layer query
    // is Ground, Ground and Bridge lists alike: a Jumpjet still tracked below
    // two levels, taking off or descending (Jumpjet54B8D0), is skipped
    // although its path layer stays Air. Touchdown untracks it (54C9DC).
    // Never rewrite stored Cell lists or AirTracker membership from this gate.
    //
    // One shared retained-bucket query serves this scanner and area damage.
    //6F91C8's Map5657A0 returns a real alias or the live Dummy. The shared
    // query owns CellGetCoords486840 ->413A30 conversion for both consumers.
    let air_center =
        scan_air.then(|| crate::sim::cell_rect::get_cellclass_fallback(terrain, cx, cy));
    // `OR EBX,0x8002 @ 0x006F91C2`: airborne infantry and units pass the
    // class gate whatever the mask.
    let air_walk = WalkArgs {
        flags: flags | 0x8002,
        ..walk
    };
    let air_extent = terrain
        .map(|terrain| (terrain.width(), terrain.height()))
        .or_else(|| {
            fire_world.map(|world| (world.session.map_width(), world.session.map_height()))
        });
    let airborne = if scan_air {
        air_extent
            .map(|extent| {
                entities.airborne_in_query_order(
                    air_center.as_ref().expect("AA source Cell"),
                    radius,
                    extent,
                )
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    for candidate_id in airborne {
        if candidate_id == attacker.stable_id {
            continue;
        }
        let Some(candidate) = entities.get(candidate_id) else {
            continue;
        };
        //6F91FB..6F9267: tracker membership does not bypass live layer/gates.
        if (ctx.is_ally(candidate) && !ctx.attacks_allies())
            || !ctx.house_gates_admit(candidate)
            || !candidate.lifecycle.cell_marked()
            || crate::sim::occupancy::cell_list_layer_for_entity(candidate, terrain).is_some()
        {
            continue;
        }
        let Some(score) = evaluate_candidate(&ctx, air_walk, candidate) else {
            continue;
        };
        //6F92B9..6F9363 append every accepted score before the best keep.
        if let Some(entries) = &mut collected {
            entries.push(DistributedFireCandidate {
                target: Some(super::TargetKind::Entity(candidate_id)),
                score,
            });
        }
        if score > best_score {
            best_score = score;
            best = Some(candidate_id);
        }
    }

    // `0x006F938B..0x006F9393`, then `XOR EAX,5 ; JZ 0x006F9DA1`: an AA-only
    // mask (a Guard scanner whose weapons are AA-only) walks no rings.
    if mask & 0x10 != 0 {
        walk.flags |= 4;
    }
    if mask == 5 {
        return result(best, collected, &ctx);
    }

    for ring in 0..radius {
        for (x, y) in square_ring_cells((cx, cy), ring) {
            // Native ring coordinates cross the packed CellStruct seam before
            // Map568300 tests Size's diamond. Loaded-world callers own Size;
            // mapless walk fixtures retain their rectangular coordinate domain.
            let coord = (x as i16, y as i16);
            if let Some((width, height)) = fire_world.and_then(|world| world.map_size_diamond())
                && !crate::map::playfield::size_diamond_contains(width, height, coord)
            {
                continue;
            }
            let Some(candidate_id) = scan_cell_for_target(&ctx, coord, scanner_zone) else {
                continue;
            };
            let Some(candidate) = entities.get(candidate_id) else {
                continue;
            };
            // `0x006F9459..0x006F9497`, after `Scan_Cell_For_Target`.
            let Some(score) = evaluate_candidate(&ctx, walk, candidate)
                .filter(|_| ctx.house_gates_admit(candidate))
            else {
                continue;
            };
            //6F94A7/6F9654/6F9839/6F99ED: ring append points. Flat
            //6F9B6E..6F9DA1 deliberately has no corresponding collection.
            if let Some(entries) = &mut collected {
                entries.push(DistributedFireCandidate {
                    target: Some(super::TargetKind::Entity(candidate_id)),
                    score,
                });
            }
            if score > best_score {
                best_score = score;
                best = Some(candidate_id);
            }
        }
        if best.is_some() && is_early_return_ring(ring, radius) {
            return result(best, collected, &ctx);
        }
    }
    result(best, collected, &ctx)
}

/// `Greatest_Threat`'s mask: the caller's literal through the scanner's
/// `+0x3C4` override ([`threat_mask`]), then the preamble's class rewrites
/// (`0x006F8EC8..0x006F8F25`). It breaks with what an override returns
/// before any scan.
fn scanner_mask<'r>(
    rules: &'r RuleSet,
    attacker: &AttackerSnapshot,
    attacker_obj: &ObjectType,
    facts: AttackerFacts,
    get_weapon: impl Fn(i32) -> Option<&'r WeaponType>,
    human: bool,
    healer: bool,
    scanner: &GameEntity,
    lost_building: impl FnOnce() -> Option<u64>,
) -> ControlFlow<Option<u64>, (u32, bool)> {
    let literal = attacker.scan_mission.literal_mask();
    let class_bits = || passive_scan_class_bits(rules, attacker_obj, facts, &get_weapon);
    let mask = match attacker.category {
        EntityCategory::Unit => threat_mask::unit_override(literal, class_bits),
        EntityCategory::Infantry => threat_mask::infantry_override(
            literal,
            InfantryScanner {
                human,
                engineer: attacker_obj.engineer,
                lost_building,
                armed: is_armed_from_facts(attacker_obj, facts),
                infiltrate: attacker_obj.infiltrate,
                vehicle_thief: attacker_obj.vehicle_thief,
                c4: super::veterancy::holds_c4(
                    super::veterancy::rank_from_u16(attacker.veterancy),
                    attacker_obj,
                ),
                class_bits,
                primary_warhead_spares_medium_and_wood: || {
                    primary_warhead_spares_medium_and_wood(rules, attacker_obj, attacker.veterancy)
                },
            },
        )?,
        EntityCategory::Structure => threat_mask::building_override(literal, class_bits()),
        EntityCategory::Aircraft => literal,
    };
    // Aircraft inherits Foot's+3C4: original7E2668 contains4D9920.
    let foot_wrapper_entered = matches!(
        attacker.category,
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    );
    let mask = if foot_wrapper_entered {
        scanner.foot_retarget_after_stop.coerce_mask(mask)
    } else {
        mask
    };
    ControlFlow::Continue((
        threat_mask::preamble_rewrite(mask, attacker.category, healer, attacker_obj.engineer),
        foot_wrapper_entered,
    ))
}

/// The flat scan: `Greatest_Threat`'s walks over the global object arrays,
/// `0x006F9B6E`-`0x006F9DA1`, for a mask without bit 0 or 1 — Hunt's `0` and
/// every team quarry.
///
/// - `0x006F9B6E` `MOV AL,[ESP+0x14] ; TEST AL,0x4 ; JZ 0x006F9C56`: with the
///   flags word's Aircraft bit (an AA weapon), the aircraft array
///   (`0x00A8E394`, count `0x00A8E3A0`) is walked first, each through the
///   ally, `OnlyTargetHouseEnemy` and `0x4000` gates, with no zone and the
///   `NullCoord` sentinel (`0x006F9C18..0x006F9C2B`).
/// - `0x006F9C56` `TEST byte ptr [ESP+0x70],0x10`: a mask with `0x10` adds
///   the Aircraft bit for the second walk (`0x006F9C5D..0x006F9C63`), which is
///   **unconditional**.
/// - `0x006F9C67` `MOV EAX,[0x00A8EC88]` (the element count),
///   `XOR EBX,EBX ; TEST EAX,EAX ; JLE 0x006F9DA1`, then
///   `MOV EDX,[0x00A8EC7C] ; MOV EDI,[EDX + EBX*0x4]` — a plain indexed walk of
///   the global techno array, closed by `INC EBX ; CMP EBX,EAX ; JL 0x006F9C7A`
///   at `0x006F9D98`. **The loop is bounded by the object count**, exactly like
///   this one; nothing here is bounded by a radius because nothing here uses
///   one.
/// - `0x006F9D70` `PUSH -0x1` sits in the arg3 range slot (counting the seven
///   pushes back from `CALL 0x006F7CA0`), the same slot the ring path fills
///   with its computed radius (`MOV ECX,[ESP+0x2C] @ 0x006F9292`,
///   `PUSH ECX @ 0x006F92A7`). `Evaluate_Candidate @ 0x006F7CA0` rejects on
///   distance only under `0 < range` and falls back to `TechnoType+0x5B8` /
///   `vt+0x3A8` only under `range == 0`, so `-1` trips neither gate.
///
/// Every candidate goes through the same [`evaluate_candidate`] ladder the ring
/// walk uses, because native calls the same `Evaluate_Candidate` — the self,
/// ally, limbo, dead and in-transport rejects all live inside that function.
///
/// **The two walks do not pass that function the same arguments.** The flat
/// walk pushes the scanner's movement-zone id in arg6
/// (`MOV ECX,[ESP+0x3c] @ 0x006F9D5C`, `PUSH ECX @ 0x006F9D69`) where every
/// ring callsite pushes `-1` (`0x006F92A3`, `0x006F9C21`), and a non-`-1` arg6
/// switches on a hard reject inside `Evaluate_Candidate`: the candidate's cell
/// must resolve to the same movement-zone component
/// (`MOV EBP,[ESP+0x54] ; CMP EBP,-0x1 ; JZ 0x006F7EA2` at
/// `0x006F7E32`-`0x006F7E45`, then
/// `GetZoneID(candidate cell, ATTACKER type +0x5B4, candidate +0x8C)` and
/// `CMP EAX,EBP ; JNZ 0x006F894F` at `0x006F7E7E`-`0x006F7E9C`). So a hunting
/// object considers only what its own movement zone can reach from where it
/// stands — it does not walk at something across water or up a cliff. That
/// gate is carried here by [`WalkArgs::zone`].
///
/// arg6 is not the only argument the two walks disagree on. **arg7 differs
/// too, and it decides the SCALE of the score's distance term.** The flat walk
/// forwards `Greatest_Threat`'s own arg2 — for `Mission_Hunt` a copy of the
/// hunter's Coords (`0x004D536D`-`0x004D538B`) — with
/// `MOV EBP,[ESP+0x74] @ 0x006F9C76` and `PUSH EBP @ 0x006F9D64`, where both
/// ring-shaped walks push the `NullCoord` sentinel (`PUSH 0xb0ea90` at
/// `0x006F929A` and `0x006F9C18`). That argument reaches
/// `Calculate_Threat_Score` at `0x006F86FE`-`0x006F8706`, and a non-sentinel
/// value takes the branch that never shifts its distance to cells
/// (`JMP 0x0070D0A0 @ 0x0070D021` against `SAR EAX,0x8 @ 0x0070D09D`). With
/// stock `TargetDistanceCoefficientDefault=-10` that makes a hunting object
/// overwhelmingly nearest-first where a ring scanner barely weighs distance at
/// all. Carried here by [`WalkArgs::reference`].
///
/// There is no band early return on this path (`0x006F94D0` sits below the
/// jump target), so each walk sees every object once. A landed aircraft is
/// Ground-layer and can be scored by both walks, at the two scales.
///
/// Native Techno construction appends at6F31BD..6F31D2; its destructor
/// removes by ordered compaction through63F000 (6F463B), never swap-last.
/// Production world constructors allocate monotonically increasing stable
/// identities, so sorted EntityStore iteration preserves that construction
/// order while keeping dead/limbo objects registered until destruction.
/// Logic and Display order are independent owners. Native foot_missions
/// registers six objects with original constructors/Unlimbo and records an
/// equal-score keep of the earlier Techno entry; supplied-ID fixtures must
/// use constructor ordinals if they claim the same registration premise.
///
/// COST — this walks every live entity once per flat scan, which is the shape
/// native has (`[0x00A8EC88]` is the global object count) and is bounded by the
/// object count rather than by the map. The ring path borrows maintained Cell
/// lists, while its airborne pre-pass uses the shared retained-bucket query. A hunting
/// object scans at each run of its Hunt handler (`0x004D5350`) while it
/// holds no target, so at charter scale the cost is one N-pass per targetless
/// hunter per handler run — not per tick, and not per ring.
fn global_list_scan(ctx: &ScanContext<'_>, flags: u32, zone: Option<u32>) -> Option<u64> {
    let mut best: Option<u64> = None;
    // `local_50 = -1` on the ring path; the same strictly-greater keep, so a
    // score of 0 (itself a rejection) can never displace nothing.
    let mut best_score: i32 = -1;
    let mut keep = |candidate: &GameEntity, score: i32| {
        if score > best_score {
            best_score = score;
            best = Some(candidate.stable_id());
        }
    };
    let mut walk = WalkArgs {
        flags,
        zone: None,
        reference: ThreatReference::NullCoord,
    };
    if flags & 4 != 0 {
        // `0x006F9B99..0x006F9BD2`: the aircraft walk's ally test spares
        // only the `AttackFriendlies=`, berserk and mind-controlled-transport
        // scanners.
        for candidate in ctx.entities.values_of_category(EntityCategory::Aircraft) {
            if (ctx.is_ally(candidate) && !ctx.attacks_allies())
                || !ctx.house_gates_admit(candidate)
            {
                continue;
            }
            if let Some(score) = evaluate_candidate(ctx, walk, candidate) {
                keep(candidate, score);
            }
        }
    }
    if ctx.mask & 0x10 != 0 {
        walk.flags |= 4;
    }
    walk.zone = zone;
    // `MOV EBP,[ESP+0x74] @ 0x006F9C76`, `PUSH EBP @ 0x006F9D64`.
    walk.reference = ThreatReference::Coords(ctx.scan_coord);
    let candidates: Vec<&GameEntity> = ctx.entities.values().collect();
    for (candidate, score) in candidates.iter().zip(flat_scores(ctx, walk, &candidates)) {
        if let Some(score) = score {
            keep(candidate, score);
        }
    }
    best
}

/// The global list at which [`flat_scores`] scores side by side.
const SIDE_BY_SIDE_CANDIDATES: usize = 256;

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Tests set this to score every flat walk one by one, the reference
    /// side-by-side scoring must match; [`FLAT_WALKS_SIDE_BY_SIDE`] counts
    /// the walks scored side by side.
    static SERIAL_FLAT_SCORES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static FLAT_WALKS_SIDE_BY_SIDE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The flat walk's score of each object in `candidates`, in order
/// ([`flat_score`]).
///
/// VERA-only: a mask-0 scan (a Hunt) scores every object on the map, so a
/// long list is scored side by side on the worker pool, results in list
/// order: the walk keeps exactly what the one-by-one walk keeps. A score
/// reads nothing the walk changes except the shared Dummy, which a
/// candidate's cell lookups can stamp and a later candidate's read, and the
/// Scenario stream a computer's disguise detection draws from. Each
/// candidate is therefore scored on a private copy of both. When any of them
/// stamped the Dummy the list is scored again one by one on the shared
/// Dummy, as natively; otherwise only the candidates that drew are, in list
/// order on the shared stream.
fn flat_scores(
    ctx: &ScanContext<'_>,
    walk: WalkArgs,
    candidates: &[&GameEntity],
) -> Vec<Option<i32>> {
    #[cfg(any(test, feature = "test-support"))]
    let serial = SERIAL_FLAT_SCORES.get();
    #[cfg(not(any(test, feature = "test-support")))]
    let serial = false;
    if !serial
        && candidates.len() >= SIDE_BY_SIDE_CANDIDATES
        && let Some(terrain) = ctx.terrain
    {
        let dummy = terrain.shared_cell_dummy();
        let scored = crate::sim::parallel::map_with(candidates, ctx.clone(), |ctx, candidate| {
            let draws = ctx.scenario_draws.get();
            let (score, stamped) = dummy.detached_stamps(|| flat_score(ctx, walk, candidate));
            let drew = ctx.scenario_draws.get() != draws;
            (score, stamped, drew, ctx.met_idle_disguise.replace(false))
        });
        if scored.iter().all(|&(_, stamped, _, _)| !stamped) {
            #[cfg(any(test, feature = "test-support"))]
            FLAT_WALKS_SIDE_BY_SIDE.set(FLAT_WALKS_SIDE_BY_SIDE.get() + 1);
            // Whether a candidate draws does not depend on the stream: the
            // ones that drew are scored again, in list order, on the shared
            // stream.
            return scored
                .into_iter()
                .zip(candidates)
                .map(|((score, _, drew, met_idle_disguise), candidate)| {
                    if met_idle_disguise {
                        ctx.met_idle_disguise.set(true);
                    }
                    if drew {
                        flat_score(ctx, walk, candidate)
                    } else {
                        score
                    }
                })
                .collect();
        }
    }
    candidates
        .iter()
        .map(|candidate| flat_score(ctx, walk, candidate))
        .collect()
}

/// One object's score in the flat walk: `None` for an object the walk's
/// gates or [`evaluate_candidate`] refuse.
fn flat_score(ctx: &ScanContext<'_>, walk: WalkArgs, candidate: &GameEntity) -> Option<i32> {
    // `0x006F9C92..0x006F9D0D`: a healer and a computer engineer also look
    // at allies.
    if !ctx.offers(candidate) {
        return None;
    }
    // `CMP [EDI+0x94],0x2 ; JNZ @ 0x006F9D13`: only objects the display last
    // submitted to its Ground layer. Airborne aircraft are left to the
    // aircraft walk; an airborne jumpjet is in neither walk, as native.
    // VERA-internal: with no world (walk unit tests) there is no display and
    // the gate is off.
    if let Some(world) = ctx.fire_world
        && world.display_layers().layer_of(candidate.stable_id())
            != Some(crate::sim::world::display_layers::DisplayLayer::GROUND)
    {
        return None;
    }
    if !ctx.house_gates_admit(candidate) {
        return None;
    }
    evaluate_candidate(ctx, walk, candidate)
}

/// `TechnoClass::Scan_Cell_For_Target @ 0x006F8960`, narrowed to the armed
/// attacker every reachable caller has: the object it offers to
/// [`evaluate_candidate`].
///
/// The walk (`0x006F8A17..0x006F8B43`) holds the last Techno it visited
/// (`EBP`, `0x006F8A34`): another member, such as a tree, drops it
/// (`0x006F8A23`) and the scanner itself leaves it (`0x006F8A19`). It stops at
/// the first Techno [`ScanContext::cell_walk_stops_at`] names: an ordinary
/// scanner's first non-ally, a healer's first damaged ally. If the list runs
/// out, the walk keeps what it held. It offers what it holds when
/// [`ScanContext::offers`] passes it (`0x006F8B5F..0x006F8BE0`), so an
/// ordinary scanner's cell of allies offers nothing, and a cell of three enemy
/// infantry offers only the list head: if the gate then rejects that one, the
/// other two are never seen.
///
/// A cell outside the scanner's zone offers nothing; see [`greatest_threat`].
fn scan_cell_for_target(
    ctx: &ScanContext<'_>,
    coord: (i16, i16),
    zone: Option<u32>,
) -> Option<u64> {
    // `0x006F898F..0x006F89B4`: a cell outside the scanner's zone offers
    // nothing (`MapClass::GetZoneID @ 0x0056D230`, bridge resolution on).
    if let Some(zone) = zone
        && ctx
            .zone_grid
            .zip(ctx.terrain)
            .is_some_and(|(zones, terrain)| {
                zones
                    .get_zone_id_native(
                        terrain,
                        (coord.0 as u16, coord.1 as u16),
                        ctx.attacker_obj.movement_zone,
                        true,
                    )
                    .is_some_and(|cell_zone| cell_zone != zone)
            })
    {
        return None;
    }
    // ScanCell6F8984 calls Map5657A0 even for an empty list. Its retained
    // Cell pointer selects E8 then E4; do not index occupancy by the requested
    // coordinate when fixed-stride lookup resolves an alias instead.
    let (rx, ry) = if let Some(terrain) = ctx.terrain {
        let cell = terrain.native_cell_identity(coord);
        let NativeCellIdentity::Real(index) = cell else {
            // Ordinary loaded-map admission has no object on the constructor-
            // empty Dummy lists. A missing-cell object admission/lifetime owner
            // is not represented by OccupancyGrid; never borrow a real list
            // merely because a later lookup stamped that coordinate on Dummy.
            return None;
        };
        (terrain.cells()[index].rx, terrain.cells()[index].ry)
    } else {
        (u16::try_from(coord.0).ok()?, u16::try_from(coord.1).ok()?)
    };
    let layer = ctx.cell_list(rx, ry)?;
    if !ctx.offers_first_non_ally() {
        return walk_cell_list(ctx, rx, ry, layer);
    }
    // VERA-only: an ordinary scanner's walk ends at the first non-ally and
    // offers it, so it asks only whose each member is, from the owner column
    // and not the members' records. A terrain member only drops what the walk
    // holds, which is an ally it would not offer. Debug builds compare the
    // answer with the full walk (the live-read check).
    let offered = ctx
        .cells
        .cell_objects(rx, ry, layer, None)
        .find_map(|member| match member {
            CellObjectMember::Entity(id) if id != ctx.attacker.stable_id => ctx
                .entities
                .owner_of(id)
                .filter(|&owner| !ctx.is_ally_owner(owner))
                .map(|_| id),
            _ => None,
        });
    debug_assert!(
        !crate::sim::touch_log::live_read_check_enabled()
            || offered == walk_cell_list(ctx, rx, ry, layer),
        "the owner-column walk disagrees with the full walk at ({rx}, {ry})"
    );
    offered
}

/// The cell walk itself (`0x006F8A17..0x006F8BE0`) over `layer`'s list at
/// (`rx`, `ry`), reading each member's record: what it holds when it stops or
/// runs out, if [`ScanContext::offers`] passes it.
fn walk_cell_list(ctx: &ScanContext<'_>, rx: u16, ry: u16, layer: MovementLayer) -> Option<u64> {
    let terrain = ctx
        .fire_world
        .into_iter()
        .flat_map(|world| world.terrain_objects.members_at((rx, ry)));
    let mut held = None;
    for member in ctx.cells.cell_objects(rx, ry, layer, terrain) {
        let CellObjectMember::Entity(id) = member else {
            // `0x006F8A2A`: not a Techno.
            held = None;
            continue;
        };
        if id == ctx.attacker.stable_id {
            continue;
        }
        let Some(candidate) = ctx.entities.get(id) else {
            continue;
        };
        held = Some(candidate);
        if ctx.cell_walk_stops_at(candidate) {
            break;
        }
    }
    held.filter(|held| ctx.offers(held))
        .map(GameEntity::stable_id)
}

/// `Evaluate_Candidate`'s distance cutoff, `0x006F8049..0x006F8170`: a
/// positive range refuses a candidate farther than it ([`cutoff_distance`]);
/// any other range refuses none. Native execution:
/// `tools/threat_mask_oracle.py`'s `cutoff` rows.
fn candidate_beyond_cutoff(
    scanner: DriveCoord,
    candidate: DriveCoord,
    candidate_high_flying: bool,
    range: i32,
) -> bool {
    range > 0 && cutoff_distance(scanner, candidate, candidate_high_flying) > range
}

/// The cutoff's distance between the candidate's and the scanner's GetCoords
/// (vt+0x48), through Sqrt_Approx and ftol: flat when the candidate answers
/// vt+0x54 (high flying, `0x006F805A..0x006F80CD`, summed `dy² + dx²`), 3-D
/// otherwise (`0x006F80F4..0x006F815F`, `(dx² + dz²) + dy²`).
fn cutoff_distance(scanner: DriveCoord, candidate: DriveCoord, candidate_high_flying: bool) -> i32 {
    let dx = scanner.x.wrapping_sub(candidate.x);
    let dy = scanner.y.wrapping_sub(candidate.y);
    if candidate_high_flying {
        crate::util::native_x87::sqrt_approx_length([dy, dx])
    } else {
        crate::util::native_x87::sqrt_approx_length([dx, scanner.z.wrapping_sub(candidate.z), dy])
    }
}

/// `TechnoClass::Evaluate_Candidate @ 0x006F7CA0` — the gate ladder, in native
/// order, followed by the score.
///
/// Returns the candidate's integer threat score, or `None` for a rejection.
/// Gate labels below are the ones used in the mapping of this function. The
/// mask ([`ScanContext::mask`]) and the walk's flags word decide the class
/// gate and the quarry gates; see [`threat_mask`].
fn evaluate_candidate(
    ctx: &ScanContext<'_>,
    walk: WalkArgs,
    candidate: &GameEntity,
) -> Option<i32> {
    // G1/G2/G4 — select the weapon this attacker would use against this
    // candidate, then refuse the candidate when that weapon's Verses against
    // its armor is at or below the `0.02f` floor. Resolve the native selection
    // without the legacy partial fire-error filter; G2b below owns the full
    // native probe, including early REARM before later target-legality gates.
    let candidate_obj = ctx.interner.object_type(candidate.type_ref(), ctx.rules)?;
    let scanner_facts = &ctx.scanner_facts;
    let candidate_facts = techno_target_facts(
        candidate,
        candidate_obj,
        ctx.terrain,
        ctx.is_ally(candidate),
        ctx.rules,
        ctx.interner,
    );
    // G1 `0x006F7CBE..0x006F7CD5`: SelectWeapon's index always answers, and
    // GetWeapon of it may hold no weapon — an unarmed scanner (a map type
    // without a weapon; retail engineers and spies carry kit weapons). The
    // ladder below carries such a scanner on instead of refusing its
    // candidates: `test ebp,ebp ; je 0x6f7d55 @ 0x006F7D19` skips the verses
    // gate, and G13's zero-range arm asks `Is_Armed` separately
    // (`0x006F817A..0x006F8198`). A weapon whose warhead section is missing
    // (`resolve_weapon_index` answers `None`) takes the same null-weapon
    // path. Native tests a null WeaponType apart from its slot elsewhere (the
    // candidate's current weapon, `0x006F85D3..0x006F85DD`), so the pair can
    // diverge only on that data error.
    let weapon_index = what_weapon_should_i_use(
        ctx.rules,
        ctx.attacker_obj,
        scanner_facts,
        Some(&candidate_facts),
    );
    let selected = resolve_weapon_index(
        ctx.rules,
        ctx.scanner,
        ctx.attacker_obj,
        weapon_index,
        ctx.entities,
        ctx.interner,
    );
    // G2b — GetFireError without range, immediately after SelectWeapon
    // (6F7CDB..6F7CF1). Its native cell getters can move the shared dummy even
    // when a later eligibility gate rejects this candidate. It is not a pure
    // predicate that can be deferred until after those gates or G27. A mask
    // with `0x18200` (Capturable, occupants, NeedsEngineer) skips it
    // (`0x006F7CD7`). The probe asks SelectWeapon's index whatever GetWeapon
    // holds (`push ebx @ 0x006F7CE4`): GetFireError's own no-weapon term
    // (`0x006FC31C`) answers CANT, which this reject does not take.
    if ctx.mask & 0x18200 == 0
        && let Some(world) = ctx.fire_world
        && probe_is_illegal(ctx, world, candidate, weapon_index)
    {
        return None;
    }

    // G3 sits between the fire-error probe and the verses gate.
    if rejects_vhp_candidate(ctx.attacker_obj.vhp_scan, candidate.estimated_health.get()) {
        return None;
    }
    // G4 `0x006F7D19..0x006F7D4F`: a scanner with no weapon in the selected
    // slot skips the floor test (`test ebp,ebp ; je 0x6f7d55 @ 0x006F7D1B`).
    if let Some(selected) = &selected
        // Ares10054270 replaces only the native numeric floor; a missing
        // selected weapon still skips this gate as in the original caller.
        && !selected.warhead.interaction(candidate_obj.armor_id()).can_passively_acquire()
    {
        return None;
    }

    // `0x006F7D55..0x006F7D82`: a `LandTargeting=2` scanner (`+0x604`; on
    // retail only the Boomer) refuses a candidate low enough to count as on
    // the floor (vt+0x50, [`is_low_flying`]) whose Cell (vt+0x1BC) is not
    // Water (`+0xEC`); Beach counts as land. An explicit order, retaliation
    // (`ShouldRetaliate @ 0x007087C0`, which asks no candidate gate) or another
    // team member's quarry can still send it at land.
    //
    // [`is_low_flying`]: crate::sim::movement::air_movement::is_low_flying
    if ctx.attacker_obj.land_targeting == 2
        && let crate::sim::combat::combat_weapon::TargetFacts::Techno { cell_land_type, .. } =
            &candidate_facts
        && crate::sim::movement::air_movement::is_low_flying(
            candidate,
            ctx.terrain,
            Some((ctx.rules, ctx.interner)),
        )
        && *cell_land_type != crate::rules::terrain_rules::LandType::Water.as_index()
    {
        return None;
    }

    // G6 — `InLimbo`/`Health == 0`. Actual retained Cell lists and separate
    // airborne registrations are not filtered or reconstructed from these
    // facts; eligibility belongs to this receiver.
    if candidate.health.current() == 0
        || candidate.death.is_dying()
        || candidate.lifecycle.in_limbo()
    {
        return None;
    }

    // G7 — the cloak arm at `0x006F7DA9`. A fully cloaked candidate is illegal
    // unless the ATTACKER's house holds a sensor on the candidate's own cell,
    // or the two share an owner. Alliance does not exempt.
    if crate::sim::cloak_disguise::cloak_rejects_candidate(
        candidate
            .cloak
            .as_ref()
            .is_some_and(|cloak| cloak.is_fully_cloaked()),
        ctx.fog.is_some_and(|fog_state| {
            // The cell `MapClass[] @ 0x00565730` finds at the candidate's
            // GetCoords (`0x006F7DC4`): a building's centre cell.
            let coords = object_get_coords(candidate, ctx.terrain);
            fog_state.has_sensor_for_house(
                ctx.attacker.owner,
                lepton_to_cell(coords.x) as u16,
                lepton_to_cell(coords.y) as u16,
            )
        }),
        candidate.owner() == ctx.attacker.owner,
    ) {
        return None;
    }

    // G8 — `TechnoClass+0x3D5`, the represented-in-the-playfield byte.
    if ctx.require_playfield_membership && !candidate.in_playfield() {
        return None;
    }

    // The current mission's `NoThreat=` (`0x006F7DFF..0x006F7E0B`). Retail
    // sets it on `[Selling]` and `[Harmless]`, so a building being sold is
    // no candidate.
    if crate::sim::mission::control::current_control(ctx.rules, &candidate.mission)
        .is_some_and(|entry| entry.no_threat)
    {
        return None;
    }

    // The underground gate (`0x006F7E11..0x006F7E1E`), in its native slot
    // between NoThreat and the in-transport reject: GetHeight (vt+0x1C8,
    // `ObjectClass::GetHeight @ 0x005F5F40` — the Location's Z less the
    // ground under it and any bridge deck; VERA's
    // `air_movement::current_fly_height`, the one port of that virtual every
    // class shares) below -20 leptons refuses the candidate. That is an
    // object more than 20 leptons under the ground under it — one riding the
    // Tunnel locomotor, which VERA does not port and no retail type uses, so
    // the gate is dormant until that locomotor exists.
    if crate::sim::movement::air_movement::current_fly_height(candidate, ctx.terrain) < -20 {
        return None;
    }

    if candidate.passenger_role.is_inside_transport() {
        return None;
    }

    // G12 — the movement-zone gate, `0x006F7E32`-`0x006F7E9C`, in its native
    // slot (between the in-transport reject and the ally arm).
    //
    // `MOV EBP,[ESP+0x54]` reads arg6; `CMP EBP,-0x1 ; JZ 0x006F7EA2` skips the
    // whole block when the caller passed `-1`, which every ring-walk callsite
    // does. Otherwise the candidate's coords are converted to a cell
    // (`CALL [vt+0x48] @ 0x006F7E2D`, `SAR EAX,0x8` twice) and
    // `MapClass::GetZoneID(that cell, ATTACKER type +0x5B4, candidate +0x8C)`
    // at `0x006F7E7E`-`0x006F7E95` must equal arg6, or `JNZ 0x006F894F` rejects.
    // Note both zone lookups use the ATTACKER's `MovementZone=`; only the
    // bridge-resolution flag differs — a literal `1` for the scanner's own cell
    // (`PUSH 0x1 @ 0x006F8E73`), the candidate's own on-bridge byte here
    // (`MOV CL,[ESI+0x8c] @ 0x006F7E5B`, `PUSH ECX @ 0x006F7E77`).
    //
    // The player-visible effect is that a hunting unit will not commit to a
    // victim across water, up a cliff, or in a disconnected pocket: it picks
    // something it can actually walk to, or nothing.
    //
    // VERA-internal, gamemd has no equivalent: a `None` from either lookup
    // leaves the gate off rather than rejecting. `ZoneGrid`'s exact GetZoneID
    // surface refuses a non-square or topology-less grid instead of performing
    // native's unchecked read, and `zone_grid` is `None` in headless fixtures
    // with no map. Trigger: unit tests and any map whose resolved terrain is
    // not square. Player effect in production: none — `rebuild_zone_grid_full`
    // always builds from resolved terrain. Frequency: nil in a real match.
    if let Some(scanner_zone) = walk.zone {
        let coords = object_get_coords(candidate, ctx.terrain);
        let cell = (
            lepton_to_cell_packed(coords.x) as u16,
            lepton_to_cell_packed(coords.y) as u16,
        );
        let candidate_zone = ctx.zone_grid.zip(ctx.terrain).and_then(|(zones, terrain)| {
            zones.get_zone_id_native(
                terrain,
                cell,
                ctx.attacker_obj.movement_zone,
                candidate.on_bridge(),
            )
        });
        if candidate_zone.is_some_and(|zone| zone != scanner_zone) {
            return None;
        }
    }

    // G10/G11 — the ally arm ([`ScanContext::ally_arm_refuses`]), then the
    // self test (`0x006F8001`). A healer (retail: an IFV carrying an engineer,
    // `[FV] Weapon2=RepairBullet`) takes a damaged ally.
    //
    // The shared owner above keeps AttackFriendlies and Psycho distinct:
    // Psycho bypasses this candidate ally arm, but the cell walk's later
    // offer still refuses an ally unless a separate native condition admits it.
    if ctx.ally_arm_refuses(candidate, candidate_obj)
        || candidate.stable_id() == ctx.attacker.stable_id
    {
        return None;
    }

    // The HarvesterImmune gate (`0x006F8009..0x006F8047`), in its native
    // slot between the self test (`0x006F8001`) and the distance cutoff
    // (`0x006F8049`). While Scenario+0 bit `0x800` is set, a candidate whose
    // TechnoType is in Rules' `HarvesterUnit=` list (`+0xB3C`, filled at
    // `0x0066F973`) is refused — native compares the type pointers, which
    // the interner-id comparison mirrors, so only the listed Vehicle types
    // ever match. Campaign sets the bit from the map's `[SpecialFlags]
    // HarvesterImmune=` (`0x006B8EBF..0x006B8ED7`); a nonzero game mode
    // replaces it with the session's Harvester Truce (`0x005E7495..0x005E74D4`
    // staging, `0x00687C16..0x00687C29` copy), which skirmish always leaves
    // off (`0x006AD89D` clears the byte).
    if ctx.harvester_immune && ctx.harvester_unit_ids.contains(&candidate.type_ref()) {
        return None;
    }

    // G13 — distance. A non-zero `Threat_Range` is a hard cutoff; a zero one
    // defers to the attacker's own can-fire-at query, which is the range of the
    // weapon picked against this very candidate; a NEGATIVE one — the literal
    // `-1` the mask-0 walk pushes at `0x006F9D70` — satisfies neither
    // `0 < range` nor `range == 0`, so both gates are skipped and the candidate
    // is admitted at any separation.
    //
    // The zero-range arm (`0x006F817A..0x006F8198`) asks `Is_Armed` (vt+0x2AC)
    // first, not GetWeapon of the selected slot: an armed scanner runs the
    // InRange query with SelectWeapon's index, an unarmed one compares the
    // already-computed whole-lepton cutoff distance with the type's
    // `GuardRange=` (TechnoType+0x5B8) and passes on `<=`. With no
    // `GuardRange=` only a same-position candidate passes.
    let unarmed_range_ok = || {
        cutoff_distance(
            object_get_coords(ctx.scanner, ctx.terrain),
            object_get_coords(candidate, ctx.terrain),
            crate::sim::movement::air_movement::is_high_flying(
                candidate,
                ctx.terrain,
                Some((ctx.rules, ctx.interner)),
            ),
        ) <= ctx
            .attacker_obj
            .guard_range
            .map_or(0, |range| range.to_bits() >> 8)
    };
    let in_range = match ctx.range {
        // Native runs no InRange here (`JNZ 0x006F81BE @ 0x006F8178`), so no
        // line-of-fire walk either: the distance alone decides.
        ScanRange::Hard(range) => !candidate_beyond_cutoff(
            object_get_coords(ctx.scanner, ctx.terrain),
            object_get_coords(candidate, ctx.terrain),
            crate::sim::movement::air_movement::is_high_flying(
                candidate,
                ctx.terrain,
                Some((ctx.rules, ctx.interner)),
            ),
            range,
        ),
        ScanRange::NoCutoff => true,
        ScanRange::CanFireAt => match (
            &selected,
            is_armed_from_facts(ctx.attacker_obj, ctx.scanner_facts),
        ) {
            // RESIDUAL: an Is_Armed-true scanner with no weapon in the
            // selected slot (an occupied building whose occupant is unarmed, a
            // TurretCount gunner edge) falls back to the unarmed compare;
            // native runs InRange with the index, whose null-weapon behavior
            // VERA does not port. Trigger: data errors and gunner states
            // retail rules do not author. Effect: such a scanner keeps the
            // GuardRange acceptance instead. Frequency: nil on retail data.
            (Some(selected), true) => match ctx.terrain {
                Some(terrain) => {
                    // EvaluateCandidate6F8178 already holds the actual Techno.
                    // Source and InRange share one native Cell/Dummy domain; pure
                    // scan fixtures need no synthetic World for this receiver.
                    let cells = NativeCellQuery::canonical(terrain);
                    let target = super::in_range::RangeTarget::Techno(candidate);
                    let src = super::in_range::fire_source_for_target(
                        ctx.scanner,
                        target,
                        selected.weapon,
                        &cells,
                        (ctx.rules, ctx.interner),
                    )?;
                    super::in_range::compute_range_target(
                        ctx.scanner,
                        src,
                        target,
                        selected.weapon,
                        ctx.rules,
                        ctx.interner,
                        &cells,
                        &ctx.los,
                    )
                }
                None => super::in_range::mapless_in_range(
                    ctx.scanner,
                    super::in_range::MaplessTarget::Techno(candidate),
                    selected.weapon,
                    ctx.rules,
                    ctx.interner,
                ),
            },
            _ => unarmed_range_ok(),
        },
    };
    if !in_range {
        return None;
    }

    // `0x006F81FA..0x006F8214`: an `InvisibleInGame=` building (`+0x1701`).
    let building = candidate.category() == EntityCategory::Structure;
    if building && candidate_obj.invisible_in_game {
        return None;
    }

    // The class gate, `0x006F821A..0x006F824A`.
    if !threat_mask::admits_class(walk.flags, candidate.category(), || {
        vehicle_like(ctx, candidate, candidate_obj)
    }) {
        return None;
    }

    // `LegalTarget=` (`+0x231`) at `0x006F8260`: every retail techno leaves
    // it at its `yes` default; VERA does not read it for technos.

    // `0x006F826E..0x006F835E`: outside the campaign, a `MultiplayPassive`
    // house's objects are off limits but for the building exception.
    let passive_owner = ctx.standing.game_mode_nonzero && ctx.owner_is_passive(candidate);
    if passive_owner && !ctx.passive_building_exception(candidate, candidate_obj) {
        return None;
    }

    // G19 — `Insignificant=` (`+0x232`), `0x006F8364..0x006F847D`, on an
    // object neither mind-controlled (`+0x2C0`) nor permanently controlled
    // by the Psychic Dominator (`+0x2C4`): a building passes unless its owner is
    // `MultiplayPassive`, and the rest take the passive-house exception,
    // which only a building meets. Stock authors `Insignificant=yes` on 26
    // civilian vehicles (cars, buses, taxis), 22 civilian infantry types
    // (civilians, the cow) and every wall.
    if candidate_obj.insignificant
        && !candidate.mind_control.is_mind_controlled()
        && (!building || ctx.owner_is_passive(candidate))
        && !ctx.passive_building_exception(candidate, candidate_obj)
    {
        return None;
    }

    // G21 — the disguise arm at `0x006F84B1..0x006F854B`, in its native slot.
    //
    // A `DetectDisguise=` attacker TYPE skips the whole arm — the dogs',
    // Yuri's and the Psi Corps Trooper's role against Spies and Mirage Tanks.
    // Otherwise a disguised candidate is refused unless its firing blink
    // (`+0x1EC`/`+0x1F4`, armed by `UnitClass::Fire_At @ 0x0074143E`) is
    // running and a computer attacker's detection roll succeeds.
    let candidate_disguised_to_attacker =
        candidate.is_disguised_to(ctx.attacker.owner, ctx.fog, ctx.alliances(), ctx.interner);
    let frame = ctx
        .fire_world
        .map_or(0, |world| world.session.binary_frame());
    let blink = candidate
        .disguise
        .as_ref()
        .map_or(0, |disguise| disguise.blink_remaining(frame));
    match crate::sim::cloak_disguise::disguise_rejects_candidate(
        candidate_disguised_to_attacker,
        ctx.attacker_obj.detect_disguise,
        blink,
        ctx.standing.human,
    ) {
        crate::sim::cloak_disguise::DisguiseGateOutcome::Accept => {}
        crate::sim::cloak_disguise::DisguiseGateOutcome::Reject => {
            if candidate_disguised_to_attacker
                && !ctx.attacker_obj.detect_disguise
                && blink == 0
                && !ctx.standing.human
            {
                ctx.met_idle_disguise.set(true);
            }
            return None;
        }
        crate::sim::cloak_disguise::DisguiseGateOutcome::AiDetectionRoll => {
            if !ctx.computer_detects_disguise() {
                return None;
            }
        }
    }

    // `0x006F8551..0x006F8589`: mask `0x100` refuses every candidate, and
    // mask `0x200` (a computer engineer's) takes only `Capturable=`
    // buildings (`+0x1572`).
    if ctx.mask & 0x100 != 0 {
        return None;
    }
    if ctx.mask & 0x200 != 0 && !(building && candidate_obj.capturable) {
        return None;
    }

    // G24 — the human-attacker building gate at `0x006F85AB..0x006F8601`, the
    // one and only consumer of `ThreatPosed=` inside acquisition.
    //
    // For a human-controlled attacker that is not an AI team member, an enemy
    // BUILDING is a legal passive target only if it is 1x1-with-undeploy, or it
    // is armed AND its live `ThreatPosed` is non-zero. Both halves are needed:
    // the native reject at `0x006F85DB`/`0x006F85E0`/`0x006F85EE` fires on
    // **(no current weapon) OR (`ThreatPosed` == 0)**.
    //
    //   006f85d3  CALL [candidate vtable+0x3f4]   ; GetCurrentWeapon 0x0070E1A0
    //   006f85d9  TEST EAX,EAX      JZ 006f85f0   ; no weapon      -> reject
    //   006f85dd  CMP  [EAX],0x0    JZ 006f85f0   ; null WeaponType-> reject
    //   006f85e6  CALL [candidate vtable+0x2c0]   ; Get_ThreatPosed 0x00708B40
    //   006f85ee  TEST EAX,EAX      JNZ 006f8604  ; non-zero       -> pass
    //
    // Dropping the weapon half would make seven stock buildings that author a
    // non-zero `ThreatPosed=` with no weapon key of any kind auto-targets that
    // gamemd refuses: `GACSPH` Chronosphere, `GAWEAT` Weather Control, `NAIRON`
    // Iron Curtain, `YAGNTC` Genetic Mutator, `YAPPET`, `GADUMY` (all 1) and
    // `AMMOCRAT` (10). Dropping the `ThreatPosed` half would put every wall,
    // power plant, refinery and war factory back on the auto-target list, along
    // with the SAM Site, Flak Cannon and Grand Cannon, which are armed but
    // author `ThreatPosed=0` or omit the key. Pillbox, Sentry Gun, Tesla Coil,
    // Prism Tower, Gattling Cannon and Psychic Tower are armed and author 30-40,
    // so they stay auto-acquired.
    //
    // The weapon test is `combat_weapon::is_armed`, which models
    // `GetCurrentWeapon @ 0x0070E1A0` — the turret slot when `TurretCount > 0`,
    // else slot 0, plus `BuildingClass::GetWeapon @ 0x004526F0`'s occupied-
    // building arm. Reading `Primary=` directly would disarm `[YAGGUN]`
    // (`WeaponCount=6`, no `Primary=`) and every garrisoned civilian building.
    // `0x0070E1A0` is not overridden: `get_xrefs_to` shows it in all six Techno
    // vtable `+0x3F4` slots.
    //
    // The gate is skipped for a Foot in a team (`TechnoClass+0x14 & 4` and a
    // non-null `FootClass+0x5D4`, `0x006F858F..0x006F85A9`) and for a
    // computer house's object (`0x006F85AB..0x006F85B8`): the computer's
    // units shoot power plants, refineries and walls.
    //
    // An Engineer (Infantry vt+330,5224D0) instead falls through to its own
    // building gate below, including from the human reject (6F85F0..85F6).
    if ctx.standing.team.is_none()
        && ctx.standing.human
        && !vehicle_like(ctx, candidate, candidate_obj)
        && building
        && !ctx.engineer
        && (!is_armed(candidate, candidate_obj)
            || super::threat_posed::live_threat_posed(
                candidate,
                Some(candidate_obj),
                ctx.entities,
                ctx.rules,
                ctx.interner,
            ) == 0)
    {
        return None;
    }

    // Engineer arm,6F860C..8667: only buildings. For allies, HealthRatio
    // must be <= ConditionRed (TEST AH,41 also admits unordered), and
    // BuildingType::CostOf must be nonzero for the SCANNER's house. The
    // cell walk instead tests positive cost for the candidate's house;
    // exhausting its list can still offer a building that did not stop it.
    // Enemy buildings bypass both repair gates and retain the earlier
    // Capturable mask gate. Production uses the world's live House factors;
    // a snapshot-only scan has no House and takes CostOf's raw-cost arm.
    if ctx.engineer {
        if !building {
            return None;
        }
        if ctx.is_ally(candidate)
            && (candidate
                .health
                .compare_ratio(candidate_obj.strength, ctx.rules.general.condition_red)
                == MaskedX87Ordering::Greater
                || ctx.fire_world.map_or_else(
                    || ctx.rules.cost_of(candidate_obj, None),
                    |world| world.cost_of(ctx.scanner.owner(), candidate_obj, ctx.rules),
                ) == 0)
        {
            return None;
        }
    }

    // `0x006F866D..0x006F867C`: mask `0x40` takes only types with
    // `Storage=` (`+0x800`).
    if ctx.mask & 0x40 != 0 && candidate_obj.storage == 0 {
        return None;
    }

    // G27 uses current raw structural flags and the live objects' OnBridge.
    // Mapped production supplies both owners; terrain-less/snapshot-only
    // synthetic scans have no native cell domain and omit this gate.
    if let Some(terrain) = ctx.terrain
        && bridge_layer_rejects_candidate(
            terrain,
            position_world_coord(&ctx.scanner.position),
            ctx.scanner.on_bridge(),
            position_world_coord(&candidate.position),
            candidate.on_bridge(),
        )
    {
        return None;
    }

    // G28/P9 — truncate first, then Normal VHPScan, then the house and
    // quarry terms, then final acceptance. The score's A term is G1's
    // selection: the same scanner, candidate and ally answer; with no
    // weapon selected (`None`) the score skips the term and falls back to
    // `GuardRange=` for term E (`0x0070CF81`).
    let pair = ThreatPair {
        scorer: ctx.scanner,
        scorer_type: ctx.attacker_obj,
        candidate,
        candidate_type: candidate_obj,
        candidate_allies_scorer: ctx.allies_scanner(candidate.owner()),
    };
    let score = threat_score(
        &pair,
        selected.as_ref(),
        ctx.entities,
        ctx.rules,
        ctx.interner,
        ctx.terrain,
        ctx.coefficients,
        walk.reference,
        ctx.standing.enemy(),
    );
    debug_assert!(
        !crate::sim::touch_log::live_read_check_enabled()
            || score
                == calculate_threat_score(
                    ctx.entities,
                    ctx.attacker.stable_id,
                    candidate.stable_id(),
                    ctx.rules,
                    ctx.interner,
                    ctx.terrain,
                    ctx.alliances(),
                    ctx.coefficients,
                    walk.reference,
                    ctx.standing.enemy(),
                ),
        "candidate {}: the scan's threat score diverged from a fresh one",
        candidate.stable_id()
    );
    let score = score?;
    let score = truncate_score(score);
    let score = adjust_vhp_score(
        ctx.attacker_obj.vhp_scan,
        candidate.estimated_health.get(),
        candidate_obj.strength,
        score,
    );
    let all_to_hunt = ctx.standing.house.is_some_and(|house| {
        crate::sim::house_strategy::all_to_hunt_score_override(house, candidate.owner()).is_some()
    });
    let score = quarry_terms(
        ctx.mask,
        score,
        all_to_hunt,
        QuarryFacts {
            building,
            power_bonus: candidate_obj.power.max(0),
            max_occupants: candidate_obj.max_number_occupants as i32,
            needs_engineer: candidate_obj.needs_engineer,
            factory: candidate_obj.has_factory(),
            can_be_occupied: candidate_obj.can_be_occupied,
            occupants: candidate
                .passenger_role
                .cargo()
                .map_or(0, |cargo| cargo.count() as i32),
            armed: is_armed(candidate, candidate_obj),
        },
    )?;
    // `0x006F88BF..0x006F8924`: the ThreatAvoidance factor is 1.0 unless the
    // scanner's current weapon is `Supress=` (`Weapon+0x146`), which no
    // retail weapon is; VERA does not read the key.
    finish_score(score)
}

/// What `Evaluate_Candidate`'s quarry terms read of a candidate.
#[derive(Debug, Clone, Copy, Default)]
struct QuarryFacts {
    building: bool,
    /// BuildingType `+0xEE0`: `Power=` when positive (`0x00461060..0x0046109A`).
    power_bonus: i32,
    /// `MaxNumberOccupants=` (`+0x1580`).
    max_occupants: i32,
    /// `NeedsEngineer=` (`+0x1552`).
    needs_engineer: bool,
    /// `Factory=` names a type (`+0xEB8`).
    factory: bool,
    /// `CanBeOccupied=` (`+0x157B`).
    can_be_occupied: bool,
    /// The occupant count (`vt+0x408`, `0x004581F0`).
    occupants: i32,
    /// GetCurrentWeapon (`vt+0x3F4`) has a WeaponType.
    armed: bool,
}

/// `Evaluate_Candidate @ 0x006F875F..0x006F88B9`, after the VHP transform:
/// under the owner's All-To-Hunt bias a candidate its current enemy does
/// not own scores 1; then the quarry's building terms. `None` rejects; a
/// score of 0 is rejected by [`finish_score`].
fn quarry_terms(mask: u32, score: i32, all_to_hunt: bool, facts: QuarryFacts) -> Option<i32> {
    let mut score = score;
    if all_to_hunt {
        score = 1;
    }
    // Quarry 9, power plants.
    if mask & 0x800 != 0 && facts.building {
        score = if facts.power_bonus > 0 {
            score.wrapping_add(facts.power_bonus.wrapping_mul(1000))
        } else {
            0
        };
    }
    // Quarry 10, occupiable buildings.
    if mask & 0x8000 != 0 {
        if !facts.building || facts.max_occupants <= 0 {
            return None;
        }
        score = score.wrapping_add(facts.max_occupants.wrapping_mul(1000));
    }
    // Quarry 11, tech buildings.
    if mask & 0x10000 != 0 {
        if !facts.building || !facts.needs_engineer {
            return None;
        }
        score = score.wrapping_add(1000);
    }
    // Quarry 6, factories.
    if mask & 0x1000 != 0 && facts.building && !facts.factory {
        score = 0;
    }
    // Quarry 7, base defences: an occupiable building with occupants, or
    // an armed one.
    if mask & 0x2000 != 0 {
        let admitted = if facts.building && facts.can_be_occupied {
            facts.occupants != 0
        } else {
            facts.armed && facts.building
        };
        if !admitted {
            return None;
        }
    }
    Some(score)
}

/// The candidate's `vt+0x80` ([`super::vehicle_like`]), its vt+0x50 asked
/// through the scan's map.
fn vehicle_like(ctx: &ScanContext<'_>, candidate: &GameEntity, obj: &ObjectType) -> bool {
    super::vehicle_like(
        candidate.category(),
        obj.non_vehicle,
        obj.is_1x1_with_undeploy(),
        || {
            crate::sim::movement::air_movement::is_low_flying(
                candidate,
                ctx.terrain,
                Some((ctx.rules, ctx.interner)),
            )
        },
    )
}

/// EvaluateCandidate6F8682..6F86FE: both complete Map565730 lookups precede
/// either raw0x100 test, even for equal layers or a nonstructural first cell.
/// Retain their identities, because two misses alias the same mutable dummy.
/// Native execution and boundary coverage: tools/spatial_oracle/bridge_target_layer.
fn bridge_layer_rejects_candidate(
    terrain: &ResolvedTerrainGrid,
    attacker: DriveCoord,
    attacker_on_bridge: bool,
    candidate: DriveCoord,
    candidate_on_bridge: bool,
) -> bool {
    let cells = NativeCellQuery::canonical(terrain);
    let attacker_cell = cells.lookup_world(attacker.x, attacker.y);
    let candidate_cell = cells.lookup_world(candidate.x, candidate.y);
    cells.flags(attacker_cell) & 0x100 != 0
        && cells.flags(candidate_cell) & 0x100 != 0
        && attacker_on_bridge != candidate_on_bridge
}

/// Evaluate_Candidate's GetFireError probe: the scanner's own code against
/// `candidate` with `weapon_index`, range unasked.
fn probe_is_illegal(
    ctx: &ScanContext<'_>,
    world: &crate::sim::world::Simulation,
    candidate: &GameEntity,
    weapon_index: i32,
) -> bool {
    let Some(firer) = world.substrate.entities.get(ctx.attacker.stable_id) else {
        return false;
    };
    let subject = super::fire_error_world::FireSubject {
        world,
        rules: ctx.rules,
        overlay_registry: ctx.los.overlay_registry,
        fog: ctx
            .fog
            .map(|fog| fog.fire_view(world.house_alliances.as_map())),
        firer,
        obj: ctx.attacker_obj,
        target: Some(super::TargetKind::Entity(candidate.stable_id())),
        weapon_index,
    };
    subject.fire_error_with(ctx.probe_firer.get_or_init(|| subject.firer_part()), false)
        == super::fire_error::FireError::Illegal
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::rules::ini_parser::IniFile;

    use crate::sim::combat::threat_posed::live_threat_posed;
    use crate::sim::intern::test_interner;

    #[test]
    fn occupied_scan_bound_matches_original_override_slice() {
        super::super::garrison_oracle_tests::for_each_native_case(
            |case, scanner, object, entities, rules, interner| {
                let previous = case["input"]["previous_ring"].as_i64().unwrap() as i32;
                assert_eq!(
                    i64::from(scan_radius_cells(
                        scanner,
                        object,
                        rules,
                        ScanRange::Hard(previous * 256),
                        [0, 0],
                        entities,
                        interner,
                    )),
                    case["ring_bound_cells"].as_i64().unwrap(),
                    "{}",
                    case["input"]["name"],
                );
            },
        );
    }

    /// Executes the bound selected by the native Greatest_Threat interior
    /// slice, with supplied type/weapon/cargo state. This corpus does not
    /// execute the later cell walk or the occupied-building override.
    #[test]
    fn scan_bound_matches_original_weapon_selection_slice() {
        use crate::sim::passenger::{PassengerCargo, PassengerRole};
        use serde_json::Value;

        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/threat_scan_bound.json",
        ))
        .unwrap();
        let rows = corpus["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 65);
        // The corpus supplies runtime leptons, not authored INI values.
        // ReadRange treats exactly -1 cell as "retain default"; a half-lepton
        // lower value avoids that sentinel and still truncates to raw -256.
        let cells = |leptons: &Value| {
            let raw = leptons.as_i64().unwrap();
            if raw == -256 {
                -256.5 / 256.0
            } else {
                raw as f64 / 256.0
            }
        };
        let enabled = |value: &Value| {
            value
                .as_bool()
                .unwrap_or_else(|| value.as_i64().unwrap() != 0)
        };
        for row in rows {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let mut infantry = String::new();
            let mut vehicles = String::from("0=TRN\n");
            let mut sections = String::new();
            let mut expected_weapon_ranges = Vec::new();
            let mut weapons = |owner: &str, spec: &Value, turreted: bool| {
                let mut keys = String::new();
                for (slot, key) in [
                    ("slot0", if turreted { "Weapon1" } else { "Primary" }),
                    ("slot1", if turreted { "Weapon2" } else { "Secondary" }),
                    ("slot2", "Weapon3"),
                    (
                        "elite_slot0",
                        if turreted {
                            "EliteWeapon1"
                        } else {
                            "ElitePrimary"
                        },
                    ),
                    (
                        "elite_slot1",
                        if turreted {
                            "EliteWeapon2"
                        } else {
                            "EliteSecondary"
                        },
                    ),
                    ("elite_slot2", "EliteWeapon3"),
                ] {
                    // Non-turreted types expose only Primary/Secondary. Their
                    // supplied third native slot is deliberately not consulted.
                    if !turreted && matches!(slot, "slot2" | "elite_slot2") {
                        continue;
                    }
                    if spec[slot].is_number() {
                        expected_weapon_ranges.push((
                            format!("{owner}{slot}"),
                            spec[slot].as_i64().unwrap() as i32,
                        ));
                        keys += &format!("{key}={owner}{slot}\n");
                        sections += &format!(
                            "[{owner}{slot}]\nDamage=10\nWarhead=WH\nRange={}\n",
                            cells(&spec[slot]),
                        );
                    }
                }
                keys
            };
            let turret_count = input["turret_count"].as_i64().unwrap();
            let transport_weapons = weapons("TRN", &input["weapons"], turret_count > 0);
            let open_topped = enabled(&input["open_topped"]);
            let mut types = format!(
                "[TRN]\nStrength=100\nTurretCount={turret_count}\nWeaponCount=3\n\
                 OpenTopped={open_topped}\nIsGattling={}\nUnderwater={}\nOrganic={}\n\
                 SelfHealing={}\nGuardRange={}\nAirRangeBonus={}\n{transport_weapons}",
                enabled(&input["is_gattling"]),
                enabled(&input["underwater"]),
                enabled(&input["organic"]),
                enabled(&input["self_healing"]),
                cells(&input["guard_range"]),
                cells(&input["air_range_bonus"]),
            );
            let riders = input["passengers"].as_array().unwrap();
            for (index, rider) in riders.iter().enumerate() {
                let kind = format!("P{index}");
                let turret_count = rider["turret_count"].as_i64().unwrap();
                let keys = weapons(&kind, rider, turret_count > 0);
                if rider["class"] == "unit" {
                    vehicles += &format!("{}={kind}\n", index + 1);
                } else {
                    infantry += &format!("{index}={kind}\n");
                }
                types += &format!(
                    "[{kind}]\nStrength=100\nTurretCount={turret_count}\nWeaponCount=3\n{keys}"
                );
            }
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[InfantryTypes]\n{infantry}[VehicleTypes]\n{vehicles}{types}{sections}\
                 [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
            )))
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            for (weapon, expected) in expected_weapon_ranges {
                assert_eq!(
                    rules
                        .weapon(&weapon)
                        .unwrap_or_else(|| panic!("{name}: missing supplied weapon {weapon}"))
                        .range_leptons,
                    expected,
                    "{name}: supplied {weapon} runtime range",
                );
            }
            let object = rules.object("TRN").unwrap();
            for (field, parsed) in [
                ("guard_range", object.guard_range),
                ("air_range_bonus", object.air_range_bonus),
            ] {
                assert_eq!(
                    parsed.map_or(0, |range| range.to_bits() >> 8),
                    input[field].as_i64().unwrap() as i32,
                    "{name}: supplied {field} runtime range",
                );
            }
            let configure = |entity: &mut GameEntity, spec: &Value| {
                entity
                    .veterancy
                    .set_rank_for_test((spec["veterancy"].as_f64().unwrap() * 100.0) as u16);
                entity.set_gunner_selection_for_test(
                    spec["current_weapon"].as_i64().unwrap() as i32,
                    -1,
                );
            };
            let mut entities = EntityStore::new();
            let mut cargo = PassengerCargo::new(5, 100.0);
            for (index, rider) in riders.iter().enumerate().rev() {
                let id = 10 + index as u64;
                let category = if rider["class"] == "unit" {
                    EntityCategory::Unit
                } else {
                    EntityCategory::Infantry
                };
                let mut passenger = GameEntity::test_default_of_category(
                    id,
                    &format!("P{index}"),
                    "Test",
                    0,
                    0,
                    category,
                );
                configure(&mut passenger, rider);
                passenger.passenger_role = PassengerRole::inside_for_test(1, open_topped);
                entities.insert(passenger);
                cargo.board_forced_chain(&[(id, 1.0)]);
            }
            let mut scanner = GameEntity::test_default(1, "TRN", "Test", 0, 0);
            configure(&mut scanner, input);
            scanner.passenger_role = PassengerRole::transport(cargo);
            let interner = test_interner();
            let ranges = std::array::from_fn(|index| {
                super::super::combat_weapon::weapon_range(
                    &scanner,
                    object,
                    index as i32,
                    &entities,
                    &rules,
                    &interner,
                )
            });
            let range = match input["threat_range"].as_i64().unwrap() {
                0 => ScanRange::CanFireAt,
                leptons => ScanRange::Hard(leptons as i32),
            };
            assert_eq!(
                i64::from(scan_radius_cells(
                    &scanner, object, &rules, range, ranges, &entities, &interner,
                )),
                row["ring_bound_cells"].as_i64().unwrap(),
                "{name}",
            );
        }
    }

    /// Supplied gunner state through SetGunnerWeapon, then the production
    /// Guard scanner: a slot beyond 0/1 can reach a target their ring bound
    /// would never visit. Candidate acceptance still uses the selected weapon.
    #[test]
    fn guarding_ifv_scans_to_its_selected_gunners_range() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=FV\n1=TARGET\n\
             [FV]\nStrength=200\nArmor=light\nTurretCount=4\nWeaponCount=3\n\
             Weapon1=SHORT\nWeapon2=MEDIUM\nWeapon3=LONG\n\
             [TARGET]\nStrength=200\nArmor=heavy\nPrimary=SHORT\nThreatPosed=10\n\
             [WeaponTypes]\n0=SHORT\n1=MEDIUM\n2=LONG\n\
             [SHORT]\nRange=4\nDamage=10\nROF=20\nProjectile=SHOT\nWarhead=AP\n\
             [MEDIUM]\nRange=6\nDamage=10\nROF=20\nProjectile=SHOT\nWarhead=AP\n\
             [LONG]\nRange=12\nDamage=10\nROF=20\nProjectile=SHOT\nWarhead=AP\n\
             [SHOT]\nInviso=yes\nAG=yes\nAA=no\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
        let object = rules.object("FV").unwrap();
        assert!(object.guard_range.is_none());
        for (slot, distance, expected) in [(2, 9, true), (0, 9, false), (2, 13, false)] {
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let ifv = sim
                .spawn_object("FV", "Americans", 10, 16, 0, &rules)
                .unwrap();
            let target = sim
                .spawn_object("TARGET", "Russians", 10 + distance, 16, 0, &rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(ifv)
                .unwrap()
                .set_gunner_weapon(slot, object);
            let pick = sim.greatest_threat_represented(
                &rules,
                None,
                ifv,
                super::super::ScanMission::Guard,
                None,
            );
            assert_eq!(
                pick,
                expected.then_some(target),
                "gunner slot {slot}, target {distance} cells away",
            );
            assert_eq!(
                sim.substrate
                    .entities
                    .get(ifv)
                    .unwrap()
                    .current_weapon_number(),
                slot,
                "the scan only reads gunner selection",
            );
        }
    }

    #[test]
    fn air_prepass_mask_uses_resolved_ifv_slot_elite_fallback_and_gattling_pair() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=FV\n1=GATT\n\
             [FV]\nTurretCount=4\nWeaponCount=3\nWeapon1=GROUND\nWeapon2=AIR\nWeapon3=GROUND\nEliteWeapon3=AIR\n\
             [GATT]\nTurretCount=1\nIsGattling=yes\nWeaponCount=3\nWeapon1=GROUND\nWeapon2=AIR\nWeapon3=GROUND\n\
             [WeaponTypes]\n0=GROUND\n1=AIR\n[GROUND]\nProjectile=AG\n[AIR]\nProjectile=AA\n\
             [AG]\nAG=yes\nAA=no\n[AA]\nAG=yes\nAA=yes\n"
        )).unwrap();
        for (kind, slot, veterancy, expected) in [
            ("FV", 0, 0, false),
            ("FV", 1, 0, true),
            ("FV", 2, 0, false),
            ("FV", 2, 200, true),
            ("FV", 0, 200, false),
            ("GATT", 2, 0, true),
        ] {
            let obj = rules.object(kind).unwrap();
            let mut entity = GameEntity::test_default(1, kind, "Americans", 5, 5);
            entity.veterancy.set_rank_for_test(veterancy);
            entity.set_gunner_selection_for_test(slot, -1);
            let entities = EntityStore::new();
            let interner = test_interner();
            assert_eq!(
                passive_scan_class_bits(&rules, obj, attacker_facts(&entity, obj), |index| {
                    get_weapon(&entity, obj, index, &entities, &rules, &interner)
                }) & 4
                    != 0,
                expected,
                "{kind} slot{slot} veterancy{veterancy}"
            );
        }
    }

    /// Original708B40 reads the installed tank's type threat when the
    /// building is empty. Building+2E4 is the reciprocal tank-bunker link.
    /// Executed branches: spatial_oracle/base_defense_response.json.
    #[test]
    fn native_base_response_threat_uses_the_empty_bunkers_installed_tank() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=BUNKER\n[VehicleTypes]\n0=TANK\n\
             [BUNKER]\nStrength=100\nThreatPosed=31\n\
             [TANK]\nStrength=100\nThreatPosed=7\n",
        ))
        .unwrap();
        let mut building = GameEntity::test_default_of_category(
            1,
            "BUNKER",
            "Owner",
            5,
            5,
            EntityCategory::Structure,
        );
        building.bunker.set_bunker_occupant_for_test(Some(2));
        let tank = GameEntity::test_default(2, "TANK", "Owner", 5, 5);
        let mut entities = EntityStore::new();
        entities.insert(building);
        entities.insert(tank);
        assert_eq!(
            live_threat_posed(
                entities.get(1).unwrap(),
                rules.object("BUNKER"),
                &entities,
                &rules,
                &test_interner(),
            ),
            7,
        );
    }

    #[test]
    fn vhp_scan_matches_original_integer_slices() {
        let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/vhp_scan.json",
        ))
        .unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 498);
        for row in rows {
            let mode = match row["mode"].as_i64().unwrap() {
                0 => VhpScan::None,
                1 => VhpScan::Normal,
                2 => VhpScan::Strong,
                _ => unreachable!(),
            };
            let integer = |key: &str| row[key].as_i64().unwrap() as i32;
            let optional = |key: &str| row[key].as_i64().map(|value| value as i32);
            if let Some(bits) = row["float_score_bits"].as_str() {
                let native = crate::util::native_x87::NativeF64Bits::from_bits(
                    u64::from_str_radix(bits, 16).unwrap(),
                );
                let score = ScoreX87::load_f64(native);
                assert_eq!(truncate_score(score), integer("score"), "{row}");
            }
            let rejected = rejects_vhp_candidate(mode, integer("estimated"));
            assert_eq!(rejected, row["early_rejected"].as_bool().unwrap(), "{row}");
            let score = adjust_vhp_score(
                mode,
                integer("estimated"),
                integer("strength"),
                integer("score"),
            );
            assert_eq!(score, integer("adjusted"), "{row}");
            assert_eq!(finish_score(score), optional("score_accepted"), "{row}");
            assert_eq!(
                if rejected { None } else { finish_score(score) },
                optional("combined"),
                "{row}"
            );
        }
    }
}

#[cfg(test)]
#[path = "greatest_threat_health_tests.rs"]
mod health_tests;

#[cfg(test)]
#[path = "greatest_threat_engineer_tests.rs"]
mod engineer_tests;

#[cfg(test)]
#[path = "greatest_threat_bridge_tests.rs"]
mod bridge_tests;

#[cfg(test)]
#[path = "greatest_threat_mask_oracle_tests.rs"]
mod mask_oracle_tests;

#[cfg(test)]
#[path = "greatest_threat_scenario_gate_tests.rs"]
mod scenario_gate_tests;

#[cfg(test)]
#[path = "greatest_threat_land_targeting_tests.rs"]
mod land_targeting_tests;

#[cfg(test)]
#[path = "greatest_threat_disguise_tests.rs"]
mod disguise_tests;

#[cfg(test)]
#[path = "greatest_threat_unarmed_tests.rs"]
mod unarmed_tests;

#[cfg(test)]
#[path = "greatest_threat_healer_guard_tests.rs"]
mod healer_guard_tests;
