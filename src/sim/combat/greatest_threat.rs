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
//!    [`super::threat_range`]. A zero radius means "no cutoff of the scan's
//!    own", and the *walk* is then bounded by
//!    `wider weapon range + 1 + AirRangeBonus` cells (`0x006F90C8..0x006F9148`).
//! 2. Airborne pre-pass. An attacker with an AA projectile sweeps the airborne
//!    bucket grid first (`0x006F9169`), evaluating **every** aircraft in reach.
//!    Aircraft are therefore scored before any ground cell and win ties.
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
//! acquire an enemy standing in unexplored ground, and refuses the SHOT
//! instead (`GetFireError` returns the shrouded code, which the passive driver
//! at `0x00709820` turns into a target drop on the next cadence).
//!
//! RESIDUAL — VERA has the acquire half of that and not the drop half.
//! - Trigger: an enemy inside weapon range but in a cell this house has not
//!   explored. Common for the long-range artillery, whose `Sight=` is far
//!   *below* its weapon range: `[V3]` sees 7 and shoots 18, `[DRED]` sees 7 and
//!   shoots 25, and `HowitzerGun` reaches 12. Those types scan well past their
//!   own sight on every cadence.
//! - Player effect: small even so. Native drops the target on
//!   `GetFireError == 6` and then re-runs this same scan **inside the same
//!   call**, so it re-picks the same shrouded candidate; the end state is a
//!   unit holding a target it cannot fire on in both engines. What VERA loses
//!   is the one-cadence flicker, not the choice.
//! - Frequency: every artillery scan whose radius crosses unexplored ground.
//! - Downstream risk: closing it belongs with the passive driver's stale-target
//!   check, which needs VERA's fire-error query to report the shrouded case.
//!
//! ## Other residuals
//!
//! - The garrisoned-building auto-acquire scan in `combat/mod.rs` still uses
//!   the retired nearest-first key, and still carries the invented
//!   `FogState::is_cell_visible` gate (`combat/mod.rs:5388`) that this scan no
//!   longer has — so "the fog gate is gone" is true of passive acquisition and
//!   not of the garrison path. It is a separate caller with its own
//!   `OccupyWeapon` selection ladder; folding it into this walk is follow-up
//!   work. Trigger: an occupied civilian building choosing among several
//!   enemies, or one standing in unexplored ground. Frequency: garrison maps
//!   only.
//! - The `DistributedFire=` candidate/history assignment (`0x00709550`) is
//!   not represented here. It is reachable for human-owned stock AEGIS;
//!   its complete collection, firing-history and detach owners remain pending.
//!   Separately, the omitted AI ore-cell fallback (`0x006F8C10`) returns 0
//!   for a human-controlled house.
//! - The ordinary class-mask AA bit (`Weapon772A90`) gates the airborne
//!   prepass through the resolved native slot choice. The complete mask,
//!   including Infantry special mission/type rewrites and AG class bits,
//!   remains unrepresented; per-target weapon selection is still separate.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/ and sim/ only.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::BTreeMap;

use super::combat_targeting::AttackerSnapshot;
use super::combat_weapon::{
    attacker_facts, attacker_facts_from_snapshot, is_ally_by_object, is_armed, passive_scan_has_aa,
    select_weapon_for_target, techno_target_facts,
};
use super::threat_range::{ScanRange, max_weapon_range, scan_range};
use super::{armor_index, is_within_range_leptons, lepton_distance_sq_raw};
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::object_type::{ObjectType, VhpScan};
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::pathfinding::zone_map::{ZoneGrid, ZoneId};
use crate::sim::vision::FogState;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{
    MaskedX87Chop53 as ScoreX87, MaskedX87Value, NativeF64Bits, X87Chop53, sqrt_approx_f32,
};

/// `Sqrt_Approx` operand base: leptons per cell.
const LEPTONS_PER_CELL: i32 = 256;

/// `TechnoClass::Calculate_Threat_Score`'s additive base, `DAT_007F4E90`.
const THREAT_SCORE_BASE: f64 = 100_000.0;

/// `TechnoClass::Evaluate_Candidate @ 0x006F7D1F`'s Verses floor, `DAT_007F4E38`
/// — a `float` `0.02` widened to double, so the comparison is against
/// `0.019999999552965164` and an authored `Verses=2%` (exactly `0.02` as a
/// double) is *above* it and passes.
const VERSES_FLOOR: f64 = 0.02f32 as f64;
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

fn threat_coord(entity: &GameEntity, terrain: Option<&ResolvedTerrainGrid>) -> (i32, i32, i32) {
    let x = i32::from(entity.position.rx)
        .wrapping_mul(LEPTONS_PER_CELL)
        .wrapping_add(entity.position.sub_x.to_num::<i32>());
    let y = i32::from(entity.position.ry)
        .wrapping_mul(LEPTONS_PER_CELL)
        .wrapping_add(entity.position.sub_y.to_num::<i32>());
    let z = terrain
        .and_then(|terrain| super::in_range::effective_z_leptons(entity, terrain))
        .and_then(|z| i32::try_from(z).ok())
        .unwrap_or_else(|| {
            i32::from(entity.position.z)
                .wrapping_mul(crate::util::lepton::LEPTONS_PER_LEVEL as i32)
                .wrapping_add(
                    entity
                        .locomotor
                        .as_ref()
                        .map(|locomotor| locomotor.altitude.to_num::<i32>())
                        .unwrap_or(0),
                )
        });
    (x, y, z)
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
    /// `vt+0x48` (`ObjectClass::GetCoords @ 0x005F65A0`, a verbatim copy of
    /// `ObjectClass+0x9C/+0xA0/+0xA4`) and converts to **whole CELLS**:
    /// `CDQ ; AND EDX,0xff ; ADD EAX,EDX ; SAR EAX,0x8` at
    /// `0x0070D094`-`0x0070D09D`.
    NullCoord,
    /// A real coordinate supplied by the caller, which in the whole reachable
    /// set is the **scanner's own `ObjectClass+0x9C` Coords**:
    /// `FootClass::Mission_Hunt` copies its three words to a local and passes
    /// the pointer (`LEA ECX,[ESI+0x9c] @ 0x004D536D`, three dwords to
    /// `[ESP+0x14..0x1C]`, `LEA EAX,[ESP+0x14]` / `PUSH EAX @ 0x004D538B`),
    /// `Retaliate_And_Scan` forwards it as `Greatest_Threat`'s arg2, and the
    /// flat walk alone pushes it as `Evaluate_Candidate`'s arg7
    /// (`MOV EBP,[ESP+0x74] @ 0x006F9C76`, `PUSH EBP @ 0x006F9D64`), which
    /// becomes this parameter at `0x006F86FE`-`0x006F8706`.
    ///
    /// This branch measures the supplied point to the candidate's `vt+0x48`
    /// and then **jumps straight to the join** — `JMP 0x0070D0A0 @ 0x0070D021`,
    /// with no `SAR EAX,0x8` anywhere on the path — so its distance stays in
    /// **LEPTONS** while the range it is compared against was shifted to cells
    /// at `0x0070CF99`.
    ///
    /// Because `Mission_Hunt`'s copy and `GetCoords` read the same three
    /// fields, the reference *point* is identical to the `NullCoord` branch's;
    /// only the scale differs. Modelled as the scorer's own coordinate here for
    /// that reason.
    ///
    /// **The name is narrower than the native branch, deliberately.** The
    /// coordinate is chosen a level up, at `Greatest_Threat`, whose gate
    /// `TEST AL,0x3 ; JZ 0x006F9B6E @ 0x006F8FE0` sends *any* mask with the low
    /// two bits clear down the flat walk — and `search_instructions
    /// CALL "+ 0x3c4]"` finds fourteen callsites, not one.
    /// `FootClass::Mission_Rescue @ 0x004DE056` passes mask 0 with
    /// `param_1[0x86]`'s coords, i.e. the **ArchiveTarget**
    /// (`TechnoClass+0x218`) rather than the scanner's own position, and two
    /// further flat-walk sites (`0x00414B24` mask `0x40`, `0x00414B64` mask 0)
    /// sit in an undefined body with three more passing a register mask,
    /// unsettled.
    ///
    /// So this variant is exact for every path VERA runs today — Hunt is the
    /// only production caller that reaches the flat walk — but it is NOT a
    /// general "supplied coordinate" model, and a second caller cannot simply
    /// reuse it. Wiring Rescue (VERA already has a paradrop analogue in
    /// `sim::aircraft::paradrop_mission`) needs an arbitrary-point sibling
    /// carrying the ArchiveTarget, not this one.
    ScannerCoords,
}

/// `TechnoClass::Calculate_Threat_Score @ 0x0070CD10`. The distance term's
/// scale is chosen by `reference`; see [`ThreatReference`].
///
/// Term order and the per-term 64-bit store are the native ones: every
/// intermediate is written back to the `[ESP+0x10]` double between terms, and
/// only the final `beyond*E + acc + 100000` is evaluated on the x87 stack.
///
/// Not modelled: the `Rules.EnemyHouseThreatBonus` term at `0x0070CF13`, which
/// is added when the scorer house's `+0x5600` "current enemy" index names the
/// candidate's owner. `+0x5600` is written only by `HouseClass::UpdateAngerNodes
/// @ 0x00504842`, an AI-strategy routine VERA does not run, so the term is
/// unreachable until an AI house ships. Recorded, not approximated.
#[allow(clippy::too_many_arguments)]
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
) -> Option<MaskedX87Value> {
    let scorer = entities.get(scorer_id)?;
    let candidate = entities.get(candidate_id)?;
    let scorer_type = rules.object(interner.resolve(scorer.type_ref()))?;
    let candidate_type = rules.object(interner.resolve(candidate.type_ref()))?;
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
        is_ally_by_object(alliances, interner, candidate.owner(), scorer.owner()),
    );
    if let Some(selected) = select_weapon_for_target(
        rules,
        candidate_type,
        &attacker_facts(candidate, candidate_type),
        &scorer_as_target,
    ) {
        let verses =
            load_threat_double(selected.warhead.verses_f64[armor_index(&scorer_type.armor)]);
        let mut term = ScoreX87::mul(coeff_b, verses);
        if candidate
            .attack_target
            .as_ref()
            .is_some_and(|target| target.target == super::TargetKind::Entity(scorer.stable_id()))
        {
            term = ScoreX87::neg(term);
        }
        score = spill_threat_double(term);
    }

    // C: candidate type SpecialThreatValue (`TechnoTypeClass+0x2C0`).
    score = spill_threat_double(ScoreX87::add(
        ScoreX87::mul(
            coeff_c,
            load_threat_double(candidate_type.special_threat_value),
        ),
        score,
    ));

    // A: the scorer's selected weapon against the candidate. Retain the
    // selected weapon for the native range term below.
    let candidate_as_target = techno_target_facts(
        candidate,
        candidate_type,
        terrain,
        is_ally_by_object(alliances, interner, scorer.owner(), candidate.owner()),
    );
    let selected_scorer_weapon = select_weapon_for_target(
        rules,
        scorer_type,
        &attacker_facts(scorer, scorer_type),
        &candidate_as_target,
    );
    if let Some(selected) = selected_scorer_weapon.as_ref() {
        let verses =
            load_threat_double(selected.warhead.verses_f64[armor_index(&candidate_type.armor)]);
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
    let scorer_coord = threat_coord(scorer, terrain);
    let candidate_coord = threat_coord(candidate, terrain);
    let dx = X87Chop53::load_i32(scorer_coord.0.wrapping_sub(candidate_coord.0));
    let dy = X87Chop53::load_i32(scorer_coord.1.wrapping_sub(candidate_coord.1));
    let dz = X87Chop53::load_i32(scorer_coord.2.wrapping_sub(candidate_coord.2));
    let distance_sq = X87Chop53::add(
        X87Chop53::add(X87Chop53::mul(dx, dx), X87Chop53::mul(dy, dy)),
        X87Chop53::mul(dz, dz),
    );
    let distance_root = X87Chop53::load_f32(sqrt_approx_f32(distance_sq).ok()?).ok()?;
    let distance_leptons = X87Chop53::ftol_i32_low_masked(distance_root);
    let distance = match reference {
        // `CDQ ; AND EDX,0xff ; ADD EAX,EDX ; SAR EAX,0x8` at `0x0070D094`.
        ThreatReference::NullCoord => {
            crate::util::direction_tables::lepton_to_cell(distance_leptons)
        }
        // `JMP 0x0070D0A0 @ 0x0070D021` — the shift is on the other branch.
        ThreatReference::ScannerCoords => distance_leptons,
    };
    let range_cells = selected_scorer_weapon.as_ref().map_or_else(
        || {
            scorer_type
                .guard_range
                .map_or(0, |range| range.to_num::<i32>())
        },
        |selected| selected.weapon.range.to_num::<i32>(),
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

/// The cells of one Chebyshev ring, in `Greatest_Threat`'s literal loop order:
/// the north row west-to-east interleaved with the south row, then the west
/// column north-to-south interleaved with the east column.
///
/// `r == 0` yields the centre twice, exactly as the native `do/while` does.
fn ring_cells(cx: i32, cy: i32, r: i32) -> Vec<(i32, i32)> {
    let mut cells = Vec::with_capacity((8 * r.max(1) + 2) as usize);
    // Row pass: `for (dx = -r; dx <= r; dx++)`.
    for dx in -r..=r {
        cells.push((cx + dx, cy - r));
        cells.push((cx + dx, cy + r));
    }
    // Column pass: `for (dy = 1 - r; dy < r; dy++)` — skipped entirely at r = 0.
    let mut dy = 1 - r;
    while dy < r {
        cells.push((cx - r, cy + dy));
        cells.push((cx + r, cy + dy));
        dy += 1;
    }
    cells
}

/// The ring index at which the native walk returns early once it holds a
/// candidate (`0x006F94D0..0x006F94F1`).
fn is_early_return_ring(ring: i32, radius: i32) -> bool {
    ring == radius / 4 || ring == radius / 2
}

/// Walk bound in cells, `iStack_34` in `Greatest_Threat`.
///
/// A non-zero `Threat_Range` result is itself the bound (`range >> 8`). A zero
/// one — plain Guard on a type with no `GuardRange=` — bounds the walk at
/// `wider weapon range + 1 + AirRangeBonus` cells instead, which is a SEARCH
/// bound, deliberately wider than the acceptance the candidate gate applies.
fn scan_radius_cells(rules: &RuleSet, obj: &ObjectType, veterancy: u16, range: ScanRange) -> i32 {
    match range {
        ScanRange::Hard(cells) => cells.to_num::<i32>(),
        ScanRange::CanFireAt => {
            let weapon_cells = max_weapon_range(rules, obj, veterancy).to_num::<i32>();
            let air_bonus_cells = obj.air_range_bonus.map_or(0, |bonus| bonus.to_num::<i32>());
            weapon_cells + 1 + air_bonus_cells
        }
        // Mask 0 computes no walk bound because it runs no walk — the jump at
        // `0x006F8FE2` skips `iStack_34` along with the rings.
        // [`greatest_threat`] takes the flat-list branch before it reaches
        // here; zero is returned so that any future caller which does ask
        // walks nothing rather than walking the map.
        ScanRange::NoCutoff => 0,
    }
}

/// A scan borrows the actual Cell lists. Native6F89BF..6F89CB reads the
/// retained +E8/+E4 head; current position, alive state and virtual +78 cannot
/// reconstruct that membership. In particular initial Jumpjet Process can
/// retain a ground-list member independently of its airborne registration.
/// The air pre-pass still builds its temporary coordinate lookup from the
/// separately owned registrations; its existing sweep-order residual is below.
struct ScanIndex<'a> {
    cells: &'a OccupancyGrid,
    airborne: BTreeMap<(u16, u16), Vec<u64>>,
}

impl<'a> ScanIndex<'a> {
    fn build(
        entities: &EntityStore,
        cells: &'a OccupancyGrid,
        scan_air: bool,
        min: (i32, i32),
        max: (i32, i32),
    ) -> Self {
        // AG-only scans borrow Cell storage without walking the entity store
        // or allocating/sorting a temporary air subset.
        if !scan_air {
            return Self {
                cells,
                airborne: BTreeMap::new(),
            };
        }
        let mut ordered: Vec<&GameEntity> = entities
            .values()
            .filter(|entity| {
                let x = i32::from(entity.position.rx);
                let y = i32::from(entity.position.ry);
                entity.air_spatial_bucket.is_some()
                    && x >= min.0
                    && x <= max.0
                    && y >= min.1
                    && y <= max.1
            })
            .collect();
        ordered.sort_by_key(|entity| (entity.air_spatial_enter_order, entity.stable_id()));
        let mut airborne: BTreeMap<(u16, u16), Vec<u64>> = BTreeMap::new();
        for entity in ordered {
            airborne
                .entry((entity.position.rx, entity.position.ry))
                .or_default()
                .push(entity.stable_id());
        }
        Self { cells, airborne }
    }

    /// `Scan_Cell_For_Target @ 0x006F89A6`: the bridge-deck list wins whenever
    /// it exists; only one list is ever walked.
    fn cell_list(
        &self,
        rx: u16,
        ry: u16,
    ) -> Option<(&crate::sim::occupancy::CellOccupancy, MovementLayer)> {
        let occupancy = self.cells.get(rx, ry)?;
        let layer = if occupancy.is_empty_on(MovementLayer::Bridge) {
            MovementLayer::Ground
        } else {
            MovementLayer::Bridge
        };
        Some((occupancy, layer))
    }
}

/// Everything one scan needs that does not change between candidates.
struct ScanContext<'a> {
    entities: &'a EntityStore,
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
    scanner_zone: Option<ZoneId>,
    /// `Evaluate_Candidate`'s **arg7**, the coordinate
    /// `Calculate_Threat_Score` measures its distance term from — and, through
    /// the sentinel test at `0x0070CFA0`-`0x0070CFBC`, the term's SCALE.
    ///
    /// Like [`Self::scanner_zone`] this is a property of the walk and not of
    /// the shared candidate ladder, and the two walks disagree in the same
    /// direction: the flat walk forwards `Greatest_Threat`'s own arg2
    /// (`PUSH EBP @ 0x006F9D64`, loaded `MOV EBP,[ESP+0x74] @ 0x006F9C76`),
    /// which for `Mission_Hunt` is a copy of the hunter's Coords, while the
    /// ring walk and the aircraft pre-walk push the `NullCoord` sentinel
    /// (`PUSH 0xb0ea90` at `0x006F929A` and `0x006F9C18`).
    threat_reference: ThreatReference,
    /// The world GetFireError reads for Evaluate_Candidate's probe
    /// (`0x006F7CE8`). Every production caller supplies it; `None` only in
    /// unit tests of the walk and the score, which then skip the probe.
    fire_world: Option<&'a crate::sim::world::Simulation>,
}

impl ScanContext<'_> {
    fn alliances(&self) -> Option<&HouseAllianceMap> {
        self.fog.map(|fog| &fog.alliances)
    }

    /// `HouseClass::Is_Ally_ByObject` — allied houses and the owner itself.
    fn is_ally(&self, candidate: &GameEntity) -> bool {
        if candidate.owner() == self.attacker.owner {
            return true;
        }
        self.fog.is_some_and(|fog| {
            fog.is_friendly(
                self.interner.resolve(self.attacker.owner),
                self.interner.resolve(candidate.owner()),
            )
        })
    }
}

/// `TechnoClass::Greatest_Threat @ 0x006F8DF0` — the scan a passive object runs
/// to choose a target. Returns the winning candidate's stable id.
///
/// `scan_range_override` replaces the mission-derived radius with a hard cutoff;
/// it exists for garrisoned buildings, whose reach is foundation-derived.
///
/// ## Two topologies, selected by the caller's threat mask
///
/// The mask is `Greatest_Threat`'s own second argument, a literal at every
/// callsite (see [`super::threat_range::ScanMission`]), and the first thing the
/// body does with it is
/// `MOV AL,[ESP+0x70] ; TEST AL,0x3 ; JZ 0x006F9B6E` at `0x006F8FDC`-
/// `0x006F8FE2`. With bit0 or bit1 set the function runs the radius block, the
/// airborne pre-pass (`0x006F9169`) and the expanding-ring cell walk. With
/// **neither** set — mask 0, which only `FootClass::Mission_Hunt @ 0x004D5373`
/// pushes — the jump lands past all three, and the scan that actually runs is
/// the flat walk over the global object array; see [`global_list_scan`].
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
///   `0x004D9951` jumps past the store otherwise). Not modelled; residual
///   below.
///
/// RESIDUAL — the `FootClass+0x688` "arrived but cannot fire" retarget latch.
///
/// `MOV AL,[ESI+0x688] ; TEST AL,AL ; MOV EAX,[ESP+0x8] ; JZ 0x004D9935 ;
/// AND AL,0xfd ; OR AL,0x1` at `0x004D9923`-`0x004D9933`, then the base call at
/// `0x004D9942` and `MOV [ESI+0x688],AL @ 0x004D9955` clearing the byte when
/// the base returned 0. `UnitClass @ 0x00743190` (`CALL @ 0x0074325C`) and the
/// Infantry override (`CALL @ 0x0051E39F`) both chain through it, so it covers
/// every ground attacker.
///
/// The byte is written `1` in the locomotor "movement finished" tail under
/// three conditions, read this session at
/// `DriveLocomotionClass::Process_Movement @ 0x004B2E7B`-`0x004B2EA2` and
/// instruction-for-instruction at the infantry twin `FUN_005164D0 @
/// 0x00516828`-`0x0051684D`: the locomotor's `vt+0x10` says not moving,
/// `TechnoClass+0x2B4` — the Target, written by `TechnoClass::Assign_Target @
/// 0x006FCF3E` — is non-null, and `vt+0x3AC` = `TechnoClass::CanFireAtTarget @
/// 0x006F7780` says the attacker cannot fire at it.
/// `ShipLocomotionClass::Process_Movement @ 0x006A24F2`, `FUN_0069B170 @
/// 0x0069B4D4` and `TechnoClass::Clear_Convoy_Chain @ 0x006EC3BD` write the
/// same `1`; `FootClass::Mission_Rescue @ 0x004DE03E` clears it.
/// - Trigger: a ground attacker finishes a move still holding a target it
///   cannot fire on, and then scans after that target goes away.
/// - Player effect: for one scan cadence a hunting unit looks only as far as
///   plain Guard reaches instead of over the whole map, and an Area Guard unit
///   uses the plain-Guard radius instead of the doubled one — so it takes a
///   nearer target, or none, one cadence sooner than VERA does.
/// - Frequency: the *set* is common — any unit that drives up to something it
///   cannot shoot. The clear is narrower than "the next scan", though: it is
///   conditional on that scan returning **nothing**. `TEST EAX,EAX ;
///   JNZ 0x004D995B` at `0x004D9951` jumps past the store when the coerced
///   scan found a target, so the byte survives, and nothing clears it on a
///   mission change — the only other clears in the image are
///   `FootClass::Mission_Rescue @ 0x004DE03E` and `FUN_0069B170 @ 0x0069B17D`,
///   while `FootClass::ComputeChecksum @ 0x004DBCE2` reads it, so it is
///   durable sim state. So the honest ceiling is "until the first coerced scan
///   comes back empty", not one cadence: a latched hunter that keeps finding
///   targets inside the coerced ring walk stays coerced for as long as it keeps
///   finding them.
///
///   The load-bearing conclusion is unchanged, because the case where the flat
///   walk matters is the case that clears the latch: with nothing inside plain
///   Guard reach the coerced ring scan returns nothing, the byte is cleared,
///   and the next cadence runs the flat walk. The mask-0 flat walk stays the
///   ordinary Hunt scan, so the zone gate and the lepton-scale distance term
///   above are both on the common path.
/// - Downstream risk: closing it needs a persistent per-entity latch written by
///   the locomotor arrival path, i.e. a new snapshot field plus a
///   `SNAPSHOT_VERSION` bump and a movement-side write. Target choice only — no
///   RNG is drawn on either path.
///
/// `zone_grid` supplies `MapClass`'s per-movement-zone connectivity for the
/// mask-0 gate; see [`ScanContext::scanner_zone`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn greatest_threat(
    entities: &EntityStore,
    occupancy: &OccupancyGrid,
    rules: &RuleSet,
    interner: &StringInterner,
    attacker: &AttackerSnapshot,
    attacker_obj: &ObjectType,
    fog: Option<&FogState>,
    scan_range_override: Option<SimFixed>,
    terrain: Option<&ResolvedTerrainGrid>,
    require_playfield_membership: bool,
    zone_grid: Option<&ZoneGrid>,
    los: crate::sim::combat::line_of_fire::LineOfFireInputs<'_>,
    fire_world: Option<&crate::sim::world::Simulation>,
) -> Option<u64> {
    let range = match scan_range_override {
        Some(cells) => ScanRange::Hard(cells),
        None => scan_range(
            rules,
            attacker_obj,
            attacker.veterancy,
            attacker.scan_mission,
        ),
    };

    // `MOV [ESP+0x3c],EAX @ 0x006F8EC4`, guarded by `TEST BL,0x1 ; JNZ` at
    // `0x006F8E48` and the two `What_Am_I()` skips at `0x006F8E54` /
    // `0x006F8E60`.
    //
    // RESIDUAL — the ring walk's own use of the same slot. Native computes it
    // for every mask with bit0 clear, which includes Area Guard's mask 2, and
    // the ring path threads it into `Scan_Cell_For_Target @ 0x006F8960`
    // (`MOV ECX,[ESP+0x3c] @ 0x006F941B`, `PUSH ECX @ 0x006F9427` — arg7 of the
    // seven pushes before `CALL @ 0x006F9440`), which is a different argument
    // from the `Evaluate_Candidate` arg6 the flat walk fills, and one
    // `scan_cell_for_target` below does not take. Trigger: any Guard or Area
    // Guard ring scan whose cells span more than one movement-zone component —
    // a defender beside water, a cliff edge, a bridge. Player effect: at most
    // which occupant of a cell that cell offers. Frequency: UNCHECKED — the
    // argument's role inside `0x006F8960` was not decompiled. Downstream risk:
    // none here; it belongs with the ring walk's own port. So the slot is
    // filled only on the branch that reads it in this module.
    let scanner_zone = if matches!(range, ScanRange::NoCutoff)
        && !matches!(
            attacker.category,
            EntityCategory::Structure | EntityCategory::Aircraft
        ) {
        zone_grid.and_then(|zones| {
            zones.get_zone_id_native(
                (i32::from(attacker.pos_rx), i32::from(attacker.pos_ry)),
                attacker_obj.movement_zone,
                // `PUSH 0x1 @ 0x006F8E73` — the scanner's own cell is always
                // asked with bridge resolution on, whatever layer it stands on.
                true,
            )
        })
    } else {
        None
    };

    let ctx = ScanContext {
        entities,
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
        scanner_zone,
        // `PUSH EBP @ 0x006F9D64` on the flat walk against `PUSH 0xb0ea90` at
        // `0x006F929A` / `0x006F9C18` on the two ring-shaped walks.
        threat_reference: if matches!(range, ScanRange::NoCutoff) {
            ThreatReference::ScannerCoords
        } else {
            ThreatReference::NullCoord
        },
        fire_world,
    };

    // `TEST AL,0x3 ; JZ 0x006F9B6E`. Mask 0 takes the flat topology; every
    // other mask falls through into the radius block below.
    if matches!(range, ScanRange::NoCutoff) {
        return global_list_scan(&ctx);
    }

    let radius = scan_radius_cells(rules, attacker_obj, attacker.veterancy, range);
    if radius <= 0 {
        // `for (r = 0; r < radius; r++)` never executes.
        return None;
    }

    let cx = i32::from(attacker.pos_rx);
    let cy = i32::from(attacker.pos_ry);
    let facts = entities.get(attacker.stable_id).map_or_else(
        || attacker_facts_from_snapshot(attacker, attacker_obj),
        |entity| attacker_facts(entity, attacker_obj),
    );
    let garrison = attacker.garrison.as_ref().map(|occupant| {
        (
            interner.resolve(occupant.occupant_type_id),
            occupant.occupant_veterancy,
        )
    });
    let scan_air = passive_scan_has_aa(rules, attacker_obj, facts, garrison);
    let index = ScanIndex::build(
        entities,
        occupancy,
        scan_air,
        (cx - radius, cy - radius),
        (cx + radius, cy + radius),
    );

    let mut best: Option<u64> = None;
    // `local_50 = -1` — the keep test is strictly greater, so a score of 0 (the
    // lowest an accepted candidate can carry, and itself a rejection) can never
    // displace nothing.
    let mut best_score: i32 = -1;

    // Airborne prepass: the ordinary wrapper's resolved AA bit gates the
    // entire pass (6F8F3A -> 6F91A7), independently of target weapon selection.
    // Actual AirTracker registration alone does not grant eligibility:
    // 6F9251 requires Object+74, then6F925C rejects live +78 Ground(2).
    // Ground and Bridge selected-list layers both denote that native Ground.
    // The existing cell_list_layer_for_entity adapter handles landed Fly here;
    // its complete Jumpjet54B8D0 query remains a separate unresolved boundary.
    // Never rewrite stored Cell lists or AirTracker membership from this gate.
    //
    // DRIFT — sweep order. Native iterates a 20x20 bucket grid
    // (`FUN_00412B40`/`FUN_004137A0`); this sweeps the same ring order the
    // ground walk uses. It selects the same aircraft except when two tie on
    // score, where the winner can differ. Frequency: two identical aircraft at
    // the same range and health, on the same tick.
    for ring in 0..if scan_air { radius } else { 0 } {
        for (x, y) in ring_cells(cx, cy, ring) {
            let (Ok(rx), Ok(ry)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            let Some(occupants) = index.airborne.get(&(rx, ry)) else {
                continue;
            };
            for &candidate_id in occupants {
                if candidate_id == attacker.stable_id {
                    continue;
                }
                let Some(candidate) = entities.get(candidate_id) else {
                    continue;
                };
                if !candidate.lifecycle.cell_marked
                    || crate::sim::occupancy::cell_list_layer_for_entity(candidate, terrain)
                        .is_some()
                    || ctx.is_ally(candidate)
                {
                    continue;
                }
                let Some(score) = evaluate_candidate(&ctx, candidate) else {
                    continue;
                };
                if score > best_score {
                    best_score = score;
                    best = Some(candidate_id);
                }
            }
        }
    }

    for ring in 0..radius {
        for (x, y) in ring_cells(cx, cy, ring) {
            let (Ok(rx), Ok(ry)) = (u16::try_from(x), u16::try_from(y)) else {
                // `Cell_in_bounds_check @ 0x00568300`.
                continue;
            };
            let Some(candidate_id) = scan_cell_for_target(&ctx, &index, rx, ry) else {
                continue;
            };
            let Some(candidate) = entities.get(candidate_id) else {
                continue;
            };
            let Some(score) = evaluate_candidate(&ctx, candidate) else {
                continue;
            };
            if score > best_score {
                best_score = score;
                best = Some(candidate_id);
            }
        }
        if best.is_some() && is_early_return_ring(ring, radius) {
            return best;
        }
    }
    best
}

/// The mask-0 scan: `Greatest_Threat`'s flat walk over the global object array,
/// `0x006F9C67`-`0x006F9D9B`.
///
/// This is where a hunting object's mask lands. Verified from the disassembly
/// this session:
///
/// - `0x006F9B6E` `MOV AL,[ESP+0x14] ; TEST AL,0x4 ; JZ 0x006F9C56` — the
///   derived flags word is built from the mask alone
///   (`0x006F8F29`-`0x006F8F72`, seeded `XOR EDI,EDI`), so for mask 0 it is
///   zero and the FIRST global walk (the `0x00A8E394` list) is skipped.
/// - `0x006F9C56` `TEST byte ptr [ESP+0x70],0x10 ; JZ 0x006F9C67` — mask 0
///   falls into the **unconditional** second walk.
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
/// gate is carried here by [`ScanContext::scanner_zone`].
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
/// all. Carried here by [`ScanContext::threat_reference`].
///
/// There is no airborne pre-pass and no band early return on this path: both
/// sit at `0x006F9169` and `0x006F94D0`, below the mask-0 jump target, so a
/// hunting object simply sees every object once. Aircraft are reached here like
/// anything else, because the global array holds them too.
///
/// DRIFT — enumeration order. Native walks the array in registration order;
/// [`EntityStore`] is a `BTreeMap`, so this walks in `stable_id` order. The
/// keep is strictly-greater in both, so the order is observable only when two
/// candidates score *exactly* equal, where the winner can differ.
/// - Trigger: a hunting object with two equally-scored enemies in view.
/// - Player effect: which of two interchangeable targets it walks to.
/// - Frequency: rare — the score folds health, distance and armour
///   effectiveness, so exact ties need near-identical candidates.
/// - Downstream risk: none beyond target choice; no RNG is drawn here.
///
/// COST — this walks every live entity once per Hunt scan, which is the shape
/// native has (`[0x00A8EC88]` is the global object count) and is bounded by the
/// object count rather than by the map. The ring path borrows maintained Cell
/// lists, while its airborne pre-pass still builds a temporary index. A hunting
/// object scans only when it holds
/// no target and its `NormalTargetingDelay` timer has expired, so at charter
/// scale the cost is one N-pass per hunting object per cadence — not per tick,
/// and not per ring.
fn global_list_scan(ctx: &ScanContext<'_>) -> Option<u64> {
    let mut best: Option<u64> = None;
    // `local_50 = -1` on the ring path; the same strictly-greater keep, so a
    // score of 0 (itself a rejection) can never displace nothing.
    let mut best_score: i32 = -1;
    for candidate in ctx.entities.values() {
        let Some(score) = evaluate_candidate(ctx, candidate) else {
            continue;
        };
        if score > best_score {
            best_score = score;
            best = Some(candidate.stable_id());
        }
    }
    best
}

/// `TechnoClass::Scan_Cell_For_Target @ 0x006F8960`, narrowed to the armed
/// attacker every reachable caller has.
///
/// The walk stops at the first hostile Techno in the selected list; allied
/// entries are stepped over but do not end the walk, and if the list runs out
/// with only allies the cell contributes nothing (the post-check at
/// `0x006F8909` rejects the ally the walk was left holding). The result is that
/// a cell holding three enemy infantry offers only the list head — if the gate
/// then rejects that one, the other two are never seen.
///
/// Not modelled: the unarmed-attacker arm (`0x006F89EE`, an unarmed object
/// picks a WOUNDED ALLY, which is how a medic-like type finds its patient) and
/// the two infantry specials at `0x006F8A96`. VERA's acquisition entry requires
/// an armed attacker upstream, and neither infantry flag (`InfantryType+0x6D8`,
/// `+0xEC3`) is parsed.
fn scan_cell_for_target(
    ctx: &ScanContext<'_>,
    index: &ScanIndex<'_>,
    rx: u16,
    ry: u16,
) -> Option<u64> {
    let (occupancy, layer) = index.cell_list(rx, ry)?;
    for occupant in occupancy.iter_layer(layer) {
        if occupant.entity_id == ctx.attacker.stable_id {
            continue;
        }
        let Some(candidate) = ctx.entities.get(occupant.entity_id) else {
            continue;
        };
        if !ctx.is_ally(candidate) {
            return Some(occupant.entity_id);
        }
    }
    None
}

/// `TechnoClass::Evaluate_Candidate @ 0x006F7CA0` — the gate ladder, in native
/// order, followed by the score.
///
/// Returns the candidate's integer threat score, or `None` for a rejection.
/// Gate labels below are the ones used in the mapping of this function; the
/// AI-only flag branches (`G22`, `G23`, `G26`, `P2`–`P7`) are not represented
/// because no flag bit that reaches them is ever set on the passive path, where
/// the flag word is `1 | {AA 0x4, AG 0xB8}`.
fn evaluate_candidate(ctx: &ScanContext<'_>, candidate: &GameEntity) -> Option<i32> {
    // G1/G2/G4 — select the weapon this attacker would use against this
    // candidate, then refuse the candidate when that weapon's Verses against
    // its armor is at or below the `0.02f` floor. `select_weapon_for_target`
    // is the `SelectWeaponAgainst @ 0x006F3330` ladder and already carries the
    // 0% fallback, so a `None` here is native's `FIRE_ILLEGAL`.
    let candidate_obj = ctx
        .rules
        .object(ctx.interner.resolve(candidate.type_ref()))?;
    let scanner_facts = ctx
        .entities
        .get(ctx.attacker.stable_id)
        .map(|entity| attacker_facts(entity, ctx.attacker_obj))
        .unwrap_or_else(|| attacker_facts_from_snapshot(ctx.attacker, ctx.attacker_obj));
    let candidate_facts = techno_target_facts(
        candidate,
        candidate_obj,
        ctx.terrain,
        is_ally_by_object(
            ctx.alliances(),
            ctx.interner,
            ctx.attacker.owner,
            candidate.owner(),
        ),
    );
    let selected = select_weapon_for_target(
        ctx.rules,
        ctx.attacker_obj,
        &scanner_facts,
        &candidate_facts,
    )?;
    // G2b, the GetFireError probe, runs after the last gate below.
    // G3 sits between selection and the verses gate. The rest of the
    // conditional native +3BC FIRE_ILLEGAL probe and the null-weapon
    // continuation remain separate gaps in the existing early ladder; this
    // check adds neither callback nor RNG.
    if rejects_vhp_candidate(ctx.attacker_obj.vhp_scan, candidate.estimated_health.get()) {
        return None;
    }
    if selected.warhead.verses_f64[armor_index(&candidate_obj.armor)] <= VERSES_FLOOR {
        return None;
    }

    // G6 — `InLimbo`/`Health == 0`. Actual retained Cell lists and separate
    // airborne registrations are not filtered or reconstructed from these
    // facts; eligibility belongs to this receiver.
    if candidate.health.current == 0 || candidate.dying || candidate.lifecycle.in_limbo {
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
            fog_state.has_sensor_for_house(
                ctx.attacker.owner,
                candidate.position.rx,
                candidate.position.ry,
            )
        }),
        candidate.owner() == ctx.attacker.owner,
    ) {
        return None;
    }

    // G8 — `TechnoClass+0x3D5`, the represented-in-the-playfield byte.
    if ctx.require_playfield_membership && !candidate.in_playfield {
        return None;
    }

    // G10/G11 — the ally arm and the self test. The cell walk already stopped
    // on the first hostile, but the airborne pre-pass and the retarget callers
    // arrive here directly.
    if candidate.stable_id() == ctx.attacker.stable_id || ctx.is_ally(candidate) {
        return None;
    }
    if candidate.passenger_role.is_inside_transport() {
        return None;
    }

    // G12 — the movement-zone gate, `0x006F7E32`-`0x006F7E9C`, in its native
    // slot (between the in-transport reject and the distance test).
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
    if let Some(scanner_zone) = ctx.scanner_zone {
        let candidate_zone = ctx.zone_grid.and_then(|zones| {
            zones.get_zone_id_native(
                (
                    i32::from(candidate.position.rx),
                    i32::from(candidate.position.ry),
                ),
                ctx.attacker_obj.movement_zone,
                candidate.on_bridge,
            )
        });
        if candidate_zone.is_some_and(|zone| zone != scanner_zone) {
            return None;
        }
    }

    // G13 — distance. A non-zero `Threat_Range` is a hard cutoff; a zero one
    // defers to the attacker's own can-fire-at query, which is the range of the
    // weapon picked against this very candidate; a NEGATIVE one — the literal
    // `-1` the mask-0 walk pushes at `0x006F9D70` — satisfies neither
    // `0 < range` nor `range == 0`, so both gates are skipped and the candidate
    // is admitted at any separation.
    let dist_sq = lepton_distance_sq_raw(
        ctx.attacker.pos_rx,
        ctx.attacker.pos_ry,
        ctx.attacker.sub_x,
        ctx.attacker.sub_y,
        candidate.position.rx,
        candidate.position.ry,
        candidate.position.sub_x,
        candidate.position.sub_y,
    );
    let in_range = match ctx.range {
        // RESIDUAL (line of fire) — the hard cutoff is a plain radius, so this
        // arm never runs the wall/cliff walk `TechnoClass::InRange` 0x006F7220
        // ends in at 0x006F7642. Its one production author is the garrison
        // passive scan (`combat::mod`'s `scan_range` override), which is also
        // the branch whose fire gate skips the walk, so scan and fire agree.
        // - Trigger: a garrison passive scan with a wall or a ≥4-Level step
        //   between the building and the candidate.
        // - Player effect: garrisoned infantry acquire and open fire on a
        //   target gamemd would refuse.
        // - Frequency: routine on urban maps.
        // - Downstream risk: none to deterministic state; it is consistent
        //   with the garrison fire gate rather than deadlocking against it.
        //   Cured by the same override-aware range chain.
        ScanRange::Hard(cells) => is_within_range_leptons(dist_sq, cells),
        ScanRange::NoCutoff => true,
        ScanRange::CanFireAt => match (ctx.terrain, ctx.entities.get(ctx.attacker.stable_id)) {
            (Some(terrain), Some(attacker_entity)) => {
                let candidate_target = super::TargetKind::Entity(candidate.stable_id());
                let src = super::in_range::fire_source_coords(
                    attacker_entity,
                    &candidate_target,
                    selected.weapon,
                    ctx.entities,
                    terrain,
                )?;
                super::in_range::compute_in_range(
                    attacker_entity,
                    src,
                    &candidate_target,
                    selected.weapon,
                    ctx.rules,
                    ctx.interner,
                    ctx.entities,
                    terrain,
                    &ctx.los,
                )
            }
            _ => is_within_range_leptons(dist_sq, selected.weapon.range),
        },
    };
    if !in_range {
        return None;
    }

    // G19 — `Insignificant=` at `0x006F8451`. Native only reaches the reject
    // for a non-Building candidate, or a Building owned by a `MultiplayPassive`
    // house; a player-owned Insignificant Building (every wall segment) falls
    // through here and is refused by G24 instead. VERA has no per-house
    // `MultiplayPassive` view at this site, so the Building arm is left out and
    // only the non-Building one is applied — which is the arm that matters:
    // stock authors `Insignificant=yes` on 26 civilian vehicles (cars, buses,
    // taxis) and 22 civilian infantry types (civilians, the cow). Without it a
    // Grizzly on Guard opens fire on passing traffic.
    if candidate_obj.insignificant
        && candidate.category != EntityCategory::Structure
        && !candidate.mind_control.is_mind_controlled()
    {
        return None;
    }

    // G21 — the disguise arm at `0x006F84B1..0x006F854B`, in its native slot.
    //
    // A `DetectDisguise=` attacker TYPE skips the whole arm — the dogs',
    // Yuri's and the Psi Corps Trooper's role against Spies and Mirage Tanks.
    //
    // RESIDUAL — the blink window and the AI detection roll. Native, having
    // rejected a `DetectDisguise`-less attacker, gives it a second chance while
    // the candidate's `+0x1EC`/`+0x1F4` timer is running AND the attacking
    // house is computer-controlled, at the cost of one `RandomRanged(0, 99)`
    // compared against `[General] DisabledDisguiseDetectionPercent=15,5,2`. The
    // mapping pass found the only writers of both timer fields to be
    // `TechnoClass::Constructor @ 0x006F2CDB` (start = creation frame,
    // duration = 0), which makes `elapsed >= duration` true forever and the
    // draw unreachable for every attacker in YR, human or AI. VERA stores no
    // blink timer and passes 0, which is the same reject.
    // - Trigger: any attacker without `DetectDisguise=` evaluating a disguised
    //   Spy or Mirage Tank. - Player effect: none. - Frequency: n/a.
    // - Downstream risk: if a computed-pointer writer of `+0x1F4` does exist,
    //   wiring it costs one scenario draw per evaluated disguised candidate and
    //   shifts RNG order for every later consumer in the tick.
    const BLINK_TIMER_NOT_MODELLED: i32 = 0;
    let attacker_owner_str = ctx.interner.resolve(ctx.attacker.owner);
    let candidate_disguised_to_attacker = candidate.disguise.as_ref().is_some_and(|disguise| {
        crate::sim::cloak_disguise::is_disguised_to(
            disguise.disguised,
            false,
            ctx.fog.is_some_and(|fog_state| {
                fog_state.detects_disguise_for_house(
                    ctx.attacker.owner,
                    candidate.position.rx,
                    candidate.position.ry,
                )
            }),
            disguise.disguised_as_house.is_some_and(|fake| {
                fake == ctx.attacker.owner
                    || ctx.fog.is_some_and(|fog_state| {
                        fog_state.is_friendly(attacker_owner_str, ctx.interner.resolve(fake))
                    })
            }),
            disguise.disguised_as_house.is_some(),
        )
    });
    if !matches!(
        crate::sim::cloak_disguise::disguise_rejects_candidate(
            candidate_disguised_to_attacker,
            ctx.attacker_obj.detect_disguise,
            BLINK_TIMER_NOT_MODELLED,
            true,
        ),
        crate::sim::cloak_disguise::DisguiseGateOutcome::Accept
    ) {
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
    // Every VERA house is human-controlled, and `IsControlledByHuman @
    // 0x0050B730` is true for any human in a multiplayer game, so this arm is
    // always taken. The AI-team bypass (`TechnoClass+0x14 & 4` and a non-null
    // `FootClass+0x5D4` Team) has no VERA counterpart and is unreachable until
    // AI teams ship.
    //
    // RESIDUAL — the reject is unconditional here, where native falls through to
    // `0x006F860C` when the attacker's `vtable+0x330` byte (stored at
    // `0x006F7CBA`) is set. That byte also lets an ALLIED candidate past the
    // ally gate at `0x006F7F43`, and `0x006F860C` only accepts a damaged allied
    // Building — the engineer-repair scan. Five vtables hold a `return 0` stub
    // for `+0x330`; only `InfantryClass` overrides it, at `0x005224D0`, reading
    // `InfantryTypeClass+0xEC3`, whose ReadINI key string at `0x0082596C` is
    // `Engineer`. Trigger: an Engineer scanning for a damaged allied structure.
    // Player effect: none, and not merely none *today* —
    // `CanAcquireTarget @ 0x007091D0` returns 0 outright when that flag is set
    // for a human-controlled owner, so no human player's engineer ever reaches
    // this scan. Frequency: n/a. Downstream risk: wiring the AI's engineer
    // repair in must carry the whole `0x006F860C` arm, not just this
    // fall-through.
    if candidate.category == EntityCategory::Structure
        && !is_one_by_one_undeployable(candidate_obj)
        && (!is_armed(candidate, candidate_obj)
            || live_threat_posed(ctx.rules, candidate, candidate_obj) == 0)
    {
        return None;
    }

    // RESIDUAL — G27, the bridge-layer gate at `0x006F8672`: when the ATTACKER's
    // cell and the CANDIDATE's cell both carry a bridge (`CellClass+0x140 &
    // 0x100`) and the two objects are on opposite sides of the deck, the
    // candidate is refused. VERA has `GameEntity::on_bridge` but no per-cell
    // "this cell carries a bridge" read at this site, so applying the deck test
    // alone would also reject the ordinary ground-vs-elevated pair the native
    // gate lets through.
    // - Trigger: an armed object on or under a bridge with an enemy on the
    //   other layer, inside its scan radius.
    // - Player effect: a unit under a bridge auto-acquires one crossing it (and
    //   vice versa) where retail would not, so it holds a target it usually
    //   cannot hit.
    // - Frequency: bridge maps only, and only while something is crossing.
    // - Downstream risk: closing it needs the bridge bit threaded from
    //   `ResolvedTerrainGrid` into the scan, which is terrain plumbing rather
    //   than targeting.

    // G2b — the GetFireError probe (`0x006F7CDB..0x006F7CF1`): vt+0x3BC, the
    // function with check_range 0, against this candidate with the slot
    // SelectWeapon chose (`0x006F7CBE`). ILLEGAL rejects; every other code
    // scores on. Native asks it right after SelectWeapon; every gate of this
    // ladder is a pure predicate, so asking it after the cheaper ones rejects
    // the same candidates. The scan methods that skip it (`0x18200`: capture,
    // occupiable and tech buildings) have no represented caller.
    if let Some(world) = ctx.fire_world
        && probe_is_illegal(ctx, world, candidate, selected.index)
    {
        return None;
    }

    // G28/P9 — truncate first, then Normal VHPScan, then final acceptance.
    // Native's additional house/target/zone modifiers at6F875F..6F8928 remain
    // unrepresented; they belong after this VHP transform and before finish_score.
    let score = calculate_threat_score(
        ctx.entities,
        ctx.attacker.stable_id,
        candidate.stable_id(),
        ctx.rules,
        ctx.interner,
        ctx.terrain,
        ctx.alliances(),
        ctx.coefficients,
        ctx.threat_reference,
    )?;
    let score = truncate_score(score);
    let score = adjust_vhp_score(
        ctx.attacker_obj.vhp_scan,
        candidate.estimated_health.get(),
        candidate_obj.strength,
        score,
    );
    finish_score(score)
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
    let target = super::TargetKind::Entity(candidate.stable_id());
    super::fire_error_world::FireSubject {
        world,
        rules: ctx.rules,
        overlay_registry: ctx.los.overlay_registry,
        fog: ctx.fog,
        firer,
        obj: ctx.attacker_obj,
        target: Some(target),
        weapon_index,
        garrison: super::fire_error_world::garrison_weapon(
            world,
            ctx.rules,
            firer,
            ctx.attacker_obj,
            target,
        ),
    }
    .fire_error(false)
        == super::fire_error::FireError::Illegal
}

/// `BuildingTypeClass::Is1x1WithUndeploy @ 0x00465D40` (reached through the
/// candidate's vtable `+0x80`): a one-cell building that carries an
/// `UndeploysInto=` vehicle. Such a building is a legal passive target for a
/// human attacker regardless of its `ThreatPosed`, because it is really a
/// parked unit.
fn is_one_by_one_undeployable(obj: &ObjectType) -> bool {
    let (width, height) = crate::rules::foundation::foundation_dimensions(&obj.foundation);
    width == 1 && height == 1 && obj.undeploys_into.is_some()
}

/// `TechnoClass::Get_ThreatPosed @ 0x00708B40` (vtable `+0x2C0`).
///
/// A garrisoned building's threat is `occupants * [General] ThreatPerOccupant`
/// (`RulesClass+0x0DF4`) instead of its own type value; everything else reports
/// `TechnoTypeClass+0x670` directly. The mind-control substitution at
/// `TechnoClass+0x2E4` is not represented — VERA reads the candidate's own type
/// either way.
fn live_threat_posed(rules: &RuleSet, candidate: &GameEntity, obj: &ObjectType) -> i32 {
    if candidate.category == EntityCategory::Structure {
        let occupants = candidate
            .passenger_role
            .cargo()
            .map_or(0, |cargo| cargo.count());
        if occupants > 0 {
            return occupants as i32 * rules.general.threat_per_occupant;
        }
    }
    obj.threat_posed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::bridge_facts::BridgeCellFacts;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, zone_class};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::MovementZone;
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::intern::test_interner;

    /// A skirmish-shaped fixture: one gun tank, the two civilian object classes
    /// that stock authors `Insignificant=yes` on, an unarmed enemy structure, an
    /// armed enemy defence with a real `ThreatPosed=`, and two enemies that
    /// differ only in `SpecialThreatValue=` so the score ordering is observable
    /// on its own.
    ///
    /// The `[General]` coefficient defaults are the stock `rulesmd.ini` values.
    fn scan_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[General]\n\
             MyEffectivenessCoefficientDefault=200\n\
             TargetEffectivenessCoefficientDefault=-200\n\
             TargetSpecialThreatCoefficientDefault=200\n\
             TargetStrengthCoefficientDefault=-200\n\
             TargetDistanceCoefficientDefault=-10\n\
             ThreatPerOccupant=10\n\
             [VehicleTypes]\n0=GRIZZLY\n1=SCOUT\n2=PRIZE\n3=CAR\n\
             [InfantryTypes]\n0=ENGINEER\n\
             [BuildingTypes]\n0=WALL\n1=PILLBOX\n2=POWER\n3=CHRONO\n4=GATTLING\n\
             [WeaponTypes]\n0=105mm\n1=Vulcan\n\
             [GRIZZLY]\nStrength=300\nArmor=heavy\nPrimary=105mm\nMovementZone=Normal\n\
             [SCOUT]\nStrength=300\nArmor=heavy\n\
             [PRIZE]\nStrength=300\nArmor=heavy\nSpecialThreatValue=10\n\
             [CAR]\nStrength=100\nArmor=light\nInsignificant=yes\n\
             [ENGINEER]\nStrength=100\nArmor=none\nSpecialThreatValue=1\nThreatPosed=0\n\
             [WALL]\nStrength=100\nArmor=wood\nFoundation=1x1\nInsignificant=yes\nThreatPosed=0\n\
             [POWER]\nStrength=750\nArmor=wood\nFoundation=1x1\nThreatPosed=0\n\
             [PILLBOX]\nStrength=400\nArmor=wood\nFoundation=1x1\nPrimary=Vulcan\nThreatPosed=30\n\
             [CHRONO]\nStrength=1000\nArmor=wood\nFoundation=1x1\nThreatPosed=1\n\
             [GATTLING]\nStrength=400\nArmor=wood\nFoundation=1x1\nTurret=yes\nTurretCount=1\n\
             WeaponCount=2\nWeapon1=Vulcan\nWeapon2=Vulcan\nWeaponStages=1\nThreatPosed=30\n\
             [105mm]\nDamage=100\nROF=110\nRange=5\nWarhead=AP\n\
             [Vulcan]\nDamage=15\nROF=10\nRange=5\nWarhead=AP\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("greatest-threat fixture")
    }

    fn place(
        entities: &mut EntityStore,
        id: u64,
        type_id: &str,
        owner: &str,
        rx: u16,
        ry: u16,
        category: EntityCategory,
    ) {
        let mut entity = GameEntity::test_default(id, type_id, owner, rx, ry);
        entity.category = category;
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        if category == EntityCategory::Structure {
            entity.foundation = "1x1".to_string();
        }
        entities.insert(entity);
    }

    fn pick(entities: &EntityStore, rules: &RuleSet, attacker: u64) -> Option<u64> {
        pick_with_mask(entities, rules, attacker, super::super::ScanMission::Guard)
    }

    #[test]
    fn acquisition_reads_retained_cell_order_instead_of_rebuilding_from_entities() {
        use crate::sim::occupancy::CellListInsertion;
        let rules = scan_rules();
        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            5,
            5,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            2,
            "SCOUT",
            "Soviet",
            6,
            5,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            3,
            "PRIZE",
            "Soviet",
            6,
            5,
            EntityCategory::Unit,
        );
        let mut occupancy = OccupancyGrid::new();
        // The live list was relinked after construction: 2 precedes the more
        // valuable3. A sorted EntityStore reconstruction puts3 first instead.
        for id in [3, 2] {
            occupancy.add(
                6,
                5,
                id,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        let interner = test_interner();
        let acquire = |occupancy: &OccupancyGrid| {
            super::super::acquire_best_target_for_entity(
                &entities,
                occupancy,
                &rules,
                &interner,
                1,
                None,
                None,
                false,
                super::super::ScanMission::Guard,
                None,
                crate::sim::combat::line_of_fire::LineOfFireInputs::default(),
                None,
            )
        };
        assert_eq!(acquire(&occupancy), Some(2));
        assert_eq!(acquire(&OccupancyGrid::rebuild(&entities)), Some(3));
    }

    #[test]
    fn air_prepass_preserves_landed_fly_cell_order_and_checks_marked_and_aa() {
        use crate::rules::locomotor_type::LocomotorKind;
        use crate::sim::movement::locomotor::LocomotorState;
        use crate::sim::world::Simulation;

        for aa in [false, true] {
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[VehicleTypes]\n0=TANK\n1=SCOUT\n[AircraftTypes]\n0=PLANE\n\
                 [TANK]\nStrength=300\nPrimary=GUN\n\
                 [SCOUT]\nStrength=100\n[PLANE]\nStrength=100\nSpecialThreatValue=10\n\
                 [WeaponTypes]\n0=GUN\n[GUN]\nDamage=100\nRange=5\nProjectile=SHOT\nWarhead=WH\n\
                 [SHOT]\nAA={aa}\nAG=yes\n\
                 [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
            )))
            .unwrap();
            for bridge in [false, true] {
                let mut sim = Simulation::with_seed(0);
                sim.session.map_width = 30;
                sim.session.map_height = 30;
                place(
                    &mut sim.substrate.entities,
                    1,
                    "TANK",
                    "Americans",
                    5,
                    5,
                    EntityCategory::Unit,
                );
                place(
                    &mut sim.substrate.entities,
                    2,
                    "SCOUT",
                    "Soviet",
                    6,
                    5,
                    EntityCategory::Unit,
                );
                place(
                    &mut sim.substrate.entities,
                    3,
                    "PLANE",
                    "Soviet",
                    6,
                    5,
                    EntityCategory::Aircraft,
                );
                sim.interner = test_interner();
                for id in [3, 2, 1] {
                    let entity = sim.substrate.entities.get_mut(id).unwrap();
                    entity.lifecycle.cell_marked = false;
                    entity.on_bridge = bridge && id != 1;
                    if id == 3 {
                        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Fly));
                    }
                    // BeginTakeoff may register a still-grounded Fly before a
                    // physical step. Mark retains that registration; it does
                    // not create AirTracker membership from height or kind.
                    if id == 3 {
                        sim.begin_fly_takeoff(id, Some(&rules));
                    }
                    sim.add_entity_occupancy(id);
                }
                let layer = if bridge {
                    MovementLayer::Bridge
                } else {
                    MovementLayer::Ground
                };
                let selected = || {
                    sim.substrate
                        .occupancy
                        .get(6, 5)
                        .unwrap()
                        .iter_layer(layer)
                        .map(|entry| entry.entity_id)
                        .collect::<Vec<_>>()
                };
                assert_eq!(selected(), vec![2, 3]);
                assert!(
                    sim.substrate
                        .entities
                        .get(3)
                        .unwrap()
                        .air_spatial_bucket
                        .is_some()
                );
                let acquire = |sim: &Simulation| {
                    super::super::acquire_best_target_for_entity(
                        &sim.substrate.entities,
                        &sim.substrate.occupancy,
                        &rules,
                        &sim.interner,
                        1,
                        None,
                        None,
                        false,
                        super::super::ScanMission::Guard,
                        None,
                        crate::sim::combat::line_of_fire::LineOfFireInputs::default(),
                        Some(sim),
                    )
                };
                assert_eq!(
                    acquire(&sim),
                    Some(2),
                    "landed Fly must respect first hostile; aa={aa}, bridge={bridge}"
                );
                // A supplied positive low height changes the existing live query
                // while retained Cell order and AirTracker registration stay put.
                // Below target_is_high_flying, weapon selection alone allows AG.
                sim.substrate
                    .entities
                    .get_mut(3)
                    .unwrap()
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .altitude = SimFixed::from_num(1);
                assert_eq!(acquire(&sim), if aa { Some(3) } else { Some(2) });
                sim.substrate
                    .entities
                    .get_mut(3)
                    .unwrap()
                    .lifecycle
                    .cell_marked = false;
                assert_eq!(
                    acquire(&sim),
                    Some(2),
                    "an unmarked registration is skipped"
                );
                assert!(
                    sim.substrate
                        .entities
                        .get(3)
                        .unwrap()
                        .air_spatial_bucket
                        .is_some()
                );
                assert_eq!(
                    sim.substrate
                        .occupancy
                        .get(6, 5)
                        .unwrap()
                        .iter_layer(layer)
                        .map(|entry| entry.entity_id)
                        .collect::<Vec<_>>(),
                    vec![2, 3]
                );
            }
        }
    }

    #[test]
    fn air_prepass_mask_uses_resolved_ifv_slot_elite_fallback_and_gattling_pair() {
        use super::super::combat_weapon::WeaponOverride;
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
            entity.veterancy = veterancy;
            entity.weapon_override = Some(WeaponOverride::IfvSlot(slot));
            assert_eq!(
                passive_scan_has_aa(&rules, obj, attacker_facts(&entity, obj), None),
                expected,
                "{kind} slot{slot} veterancy{veterancy}"
            );
        }
    }

    /// The same acquisition entry, with the threat mask the caller would push.
    fn pick_with_mask(
        entities: &EntityStore,
        rules: &RuleSet,
        attacker: u64,
        mask: super::super::ScanMission,
    ) -> Option<u64> {
        pick_with_mask_and_zones(entities, rules, attacker, mask, None)
    }

    /// The acquisition entry with `MapClass`'s zone topology supplied, which is
    /// what the mask-0 walk needs to run its movement-zone gate.
    fn pick_with_mask_and_zones(
        entities: &EntityStore,
        rules: &RuleSet,
        attacker: u64,
        mask: super::super::ScanMission,
        zones: Option<&ZoneGrid>,
    ) -> Option<u64> {
        let interner = test_interner();
        super::super::acquire_best_target_for_entity(
            entities,
            &OccupancyGrid::rebuild(entities),
            rules,
            &interner,
            attacker,
            None,
            None,
            false,
            mask,
            zones,
            crate::sim::combat::line_of_fire::LineOfFireInputs::default(),
            None,
        )
    }

    /// One clear ground cell.
    fn zone_test_cell(rx: u16, ry: u16, impassable: bool) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs: SpeedCostProfile::default(),
            is_water: false,
            is_cliff_like: impassable,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            height_in_pixels: 0,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: impassable,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: if impassable {
                zone_class::IMPASSABLE
            } else {
                zone_class::GROUND
            },
            base_ground_walk_blocked: impassable,
            base_build_blocked: impassable,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: SpeedCostProfile::default(),
            build_blocked: impassable,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    /// A square map cut in two by one impassable column, so that
    /// `MovementZone::Normal` has two disconnected components.
    fn split_zone_grid(side: u16, barrier_rx: u16) -> ZoneGrid {
        let cells = (0..side)
            .flat_map(|ry| (0..side).map(move |rx| zone_test_cell(rx, ry, rx == barrier_rx)))
            .collect();
        let terrain = ResolvedTerrainGrid::from_cells(side, side, cells);
        let path_grid = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&terrain);
        ZoneGrid::build_with_terrain(
            &path_grid,
            &BTreeMap::new(),
            Some(&terrain),
            &[],
            side,
            side,
        )
    }

    /// The mask-0 walk is movement-zone filtered, and the ring walk is not.
    ///
    /// `Greatest_Threat` computes the scanner's own zone id at `0x006F8EC4`
    /// whenever the mask has bit0 clear, and the flat walk pushes it into
    /// `Evaluate_Candidate`'s arg6 (`PUSH ECX @ 0x006F9D69`) where every ring
    /// callsite pushes `-1` (`0x006F92A3`). A non-`-1` arg6 turns on the reject
    /// at `0x006F7E7E`-`0x006F7E9C`: the candidate's cell must resolve to the
    /// same component under the ATTACKER's `MovementZone=`.
    ///
    /// Fixture: an impassable column at `rx = 5` splits an 12x12 map. The
    /// attacker sits at `(2, 2)`; the nearer enemy at `(8, 2)` is on the far
    /// side and unreachable, the further one at `(1, 10)` is on its own side.
    /// Distance carries a negative coefficient, so without the gate the nearer
    /// one wins — which is exactly what the same fixture returns when no zone
    /// topology is supplied.
    #[test]
    fn gsi_07_20_mask_zero_refuses_a_target_its_movement_zone_cannot_reach() {
        let rules = scan_rules();
        let zones = split_zone_grid(12, 5);
        let near_side = zones
            .get_zone_id_nonbridge_native((2, 2), MovementZone::Normal)
            .expect("attacker cell resolves a Normal zone id");
        let far_side = zones
            .get_zone_id_nonbridge_native((8, 2), MovementZone::Normal)
            .expect("across-the-barrier cell resolves a Normal zone id");
        assert_ne!(
            near_side, far_side,
            "the fixture must actually split MovementZone=Normal into two components"
        );

        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            2,
            2,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            2,
            "SCOUT",
            "Soviets",
            8,
            2,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            3,
            "SCOUT",
            "Soviets",
            1,
            10,
            EntityCategory::Unit,
        );

        assert_eq!(
            pick_with_mask_and_zones(&entities, &rules, 1, super::super::ScanMission::Hunt, None),
            Some(2),
            "with no zone topology the gate is off and the nearer target wins on score"
        );
        assert_eq!(
            pick_with_mask_and_zones(
                &entities,
                &rules,
                1,
                super::super::ScanMission::Hunt,
                Some(&zones)
            ),
            Some(3),
            "the zone gate refuses the nearer target across the barrier and takes the reachable one"
        );
    }

    /// The gate belongs to the flat walk alone. Same fixture, one enemy, and it
    /// is on the far side of the barrier and inside plain Guard's ring bound:
    /// mask 1 still takes it, because the ring callsite pushes `-1` into arg6
    /// (`PUSH -0x1 @ 0x006F92A3`) and the reject at `0x006F7E45` is skipped.
    #[test]
    fn gsi_07_20_the_ring_walk_ignores_the_zone_gate_the_flat_walk_applies() {
        let rules = scan_rules();
        let zones = split_zone_grid(12, 5);

        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            4,
            2,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            2,
            "SCOUT",
            "Soviets",
            6,
            2,
            EntityCategory::Unit,
        );

        assert_eq!(
            pick_with_mask_and_zones(
                &entities,
                &rules,
                1,
                super::super::ScanMission::Guard,
                Some(&zones)
            ),
            Some(2),
            "mask 1 pushes -1 in the arg6 slot, so the zone reject never runs"
        );
        assert_eq!(
            pick_with_mask_and_zones(
                &entities,
                &rules,
                1,
                super::super::ScanMission::Hunt,
                Some(&zones)
            ),
            None,
            "mask 0 supplies the zone and refuses the same candidate"
        );
    }

    /// A 20-cell-range attacker with one long weapon, a plain enemy and an
    /// otherwise identical enemy worth `SpecialThreatValue=10`. Nothing else
    /// separates the two candidates, so the score ordering is decided by the
    /// special-threat term against the distance term alone.
    fn lepton_scale_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[General]\n\
             MyEffectivenessCoefficientDefault=200\n\
             TargetEffectivenessCoefficientDefault=-200\n\
             TargetSpecialThreatCoefficientDefault=200\n\
             TargetStrengthCoefficientDefault=-200\n\
             TargetDistanceCoefficientDefault=-10\n\
             [VehicleTypes]\n0=LONGTANK\n1=PLAIN\n2=JUICY\n\
             [WeaponTypes]\n0=LongGun\n\
             [LONGTANK]\nStrength=300\nArmor=heavy\nPrimary=LongGun\nMovementZone=Normal\n\
             [PLAIN]\nStrength=300\nArmor=heavy\n\
             [JUICY]\nStrength=300\nArmor=heavy\nSpecialThreatValue=10\n\
             [LongGun]\nDamage=100\nROF=110\nRange=20\nWarhead=AP\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("lepton-scale fixture")
    }

    /// The flat walk scores distance in LEPTONS where every ring-shaped walk
    /// scores it in CELLS — the same geometry at 256x the scale, in the one
    /// term that separates two candidates.
    ///
    /// `Mission_Hunt` copies its own `ObjectClass+0x9C` Coords to a local and
    /// passes the pointer (`LEA ECX,[ESI+0x9c] @ 0x004D536D` … `LEA EAX,
    /// [ESP+0x14]` / `PUSH EAX @ 0x004D538B`); `Retaliate_And_Scan` forwards it
    /// as `Greatest_Threat`'s arg2, and the flat walk alone pushes it as
    /// `Evaluate_Candidate`'s arg7 (`MOV EBP,[ESP+0x74] @ 0x006F9C76`,
    /// `PUSH EBP @ 0x006F9D64`), which becomes `Calculate_Threat_Score`'s third
    /// parameter at `0x006F86FE`-`0x006F8706`. The ring walk and the aircraft
    /// pre-walk push the `NullCoord` sentinel instead (`PUSH 0xb0ea90` at
    /// `0x006F929A` and `0x006F9C18`), and a sentinel takes the branch that
    /// ends `SAR EAX,0x8 @ 0x0070D09D`; the supplied-coordinate branch has no
    /// shift at all (`JMP 0x0070D0A0 @ 0x0070D021`).
    ///
    /// Fixture: the attacker at `(10, 10)`, the plain enemy 2 cells east, the
    /// juicier one 4 cells east. Both sit well inside `Range=20`, so on the
    /// CELL scale `max(0, dist − range)` is zero for both and the
    /// special-threat term (`200 x 10 = 2000`) decides; on the LEPTON scale the
    /// same pair is ~492 and ~1004 beyond range, i.e. about `-4920` against
    /// `-8040 = -10040 + 2000`, and the nearer, cheaper target wins.
    ///
    /// Both bands of the ring walk's early return (`radius/4 = 5`,
    /// `radius/2 = 10` with `radius = 21`) sit outside ring 4, so the ring walk
    /// really does score both candidates before it can return.
    #[test]
    fn gsi_07_20_the_flat_walk_scores_distance_in_leptons_where_the_ring_walk_scores_cells() {
        let rules = lepton_scale_rules();

        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "LONGTANK",
            "Americans",
            10,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            2,
            "PLAIN",
            "Soviets",
            12,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            3,
            "JUICY",
            "Soviets",
            14,
            10,
            EntityCategory::Unit,
        );

        assert_eq!(
            pick_with_mask(&entities, &rules, 1, super::super::ScanMission::Guard),
            Some(3),
            "the ring walk pushes the NullCoord sentinel, so its distance term \
             is in cells, both candidates are inside the weapon range, and \
             SpecialThreatValue alone decides"
        );
        assert_eq!(
            pick_with_mask(&entities, &rules, 1, super::super::ScanMission::Hunt),
            Some(2),
            "the flat walk forwards the hunter's own Coords, so its distance \
             term is in leptons and the nearer candidate wins despite being \
             worth 2000 less"
        );

        // The same pair through the scorer directly, so the walk-level result
        // above cannot be explained by anything but the reference coordinate.
        let interner = test_interner();
        let attacker_obj = rules.object("LONGTANK").expect("attacker type");
        let coefficients =
            ThreatCoefficients::resolve(&rules, attacker_obj, HOUSE_SELECTS_OWN_COEFFICIENTS);
        let score = |candidate: u64, reference: ThreatReference| {
            finish_score(truncate_score(
                calculate_threat_score(
                    &entities,
                    1,
                    candidate,
                    &rules,
                    &interner,
                    None,
                    None,
                    coefficients,
                    reference,
                )
                .expect("scored"),
            ))
            .expect("accepted")
        };
        assert!(
            score(3, ThreatReference::NullCoord) > score(2, ThreatReference::NullCoord),
            "on the cell scale the juicier candidate outscores the nearer one: {} vs {}",
            score(3, ThreatReference::NullCoord),
            score(2, ThreatReference::NullCoord)
        );
        assert!(
            score(2, ThreatReference::ScannerCoords) > score(3, ThreatReference::ScannerCoords),
            "on the lepton scale the ordering flips: {} vs {}",
            score(2, ThreatReference::ScannerCoords),
            score(3, ThreatReference::ScannerCoords)
        );
    }

    /// Mask 0 is a different scan TOPOLOGY, not a wider radius: `TEST AL,0x3 ;
    /// JZ 0x006F9B6E` at `0x006F8FE0` skips the radius block and the ring walk
    /// outright, and the walk that runs instead enumerates the global object
    /// array with a literal `-1` in the range slot (`PUSH -0x1 @ 0x006F9D70`).
    ///
    /// 30 cells is far outside anything the ring walk could reach for this
    /// type: `105mm Range=5` with no `GuardRange=` bounds the Guard walk at
    /// `5 + 1 + 0 = 6` rings, and even the Area Guard formula caps at
    /// [`AREA_GUARD_MAX_SCAN_CELLS`] = 16. Both are asserted, so the Hunt result
    /// cannot be explained by a radius the walk happened to compute.
    #[test]
    fn gsi_07_20_mask_zero_reaches_past_every_ring_the_walk_could_compute() {
        let rules = scan_rules();
        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            10,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            2,
            "SCOUT",
            "Soviets",
            10,
            40,
            EntityCategory::Unit,
        );

        assert_eq!(
            pick_with_mask(&entities, &rules, 1, super::super::ScanMission::Guard),
            None,
            "the mask-1 ring walk bounds at weapon range + 1 = 6 cells"
        );
        assert_eq!(
            pick_with_mask(&entities, &rules, 1, super::super::ScanMission::AreaGuard),
            None,
            "even the doubled-and-capped mask-2 radius stops at 16 cells"
        );
        assert_eq!(
            pick_with_mask(&entities, &rules, 1, super::super::ScanMission::Hunt),
            Some(2),
            "mask 0 enumerates the global list with range -1, so 30 cells is not a cutoff"
        );
    }

    /// The other half of the topology difference, and the one a radius cannot
    /// imitate at any width: the ring walk asks each cell for a single
    /// candidate (`Scan_Cell_For_Target @ 0x006F8960` stops at the list head),
    /// so a cell whose head the gate refuses contributes nothing at all. The
    /// mask-0 walk has no cells — it runs `Evaluate_Candidate` on every array
    /// element — so the second occupant of that cell is still seen.
    ///
    /// Fixture: a hostile `CAR` (`Insignificant=yes`, refused by G19) sharing a
    /// cell with a hostile `SCOUT`, one cell from the attacker and well inside
    /// every radius. Guard finds nothing; Hunt finds the scout.
    #[test]
    fn gsi_07_20_mask_zero_sees_past_a_refused_cell_list_head() {
        let rules = scan_rules();
        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            10,
            10,
            EntityCategory::Unit,
        );
        // The cell list prepends non-buildings, so the later-ordered entry ends
        // up at the head; the CAR must be the one the walk stops on.
        place(
            &mut entities,
            2,
            "SCOUT",
            "Soviets",
            11,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            3,
            "CAR",
            "Soviets",
            11,
            10,
            EntityCategory::Unit,
        );

        assert_eq!(
            pick_with_mask(&entities, &rules, 1, super::super::ScanMission::Guard),
            None,
            "the ring walk takes the cell's list head, and an Insignificant civilian is refused"
        );
        assert_eq!(
            pick_with_mask(&entities, &rules, 1, super::super::ScanMission::Hunt),
            Some(2),
            "the mask-0 walk evaluates every object, so the scout behind the car is still seen"
        );
    }

    /// The headline scenario for row 121: a gun tank sitting on Guard with an
    /// enemy WALL one cell away and an enemy ENGINEER three cells away shoots
    /// the engineer.
    ///
    /// The wall is refused by the human-attacker building gate at
    /// `TechnoClass::Evaluate_Candidate @ 0x006F85AB` — no weapon, so no threat
    /// posed, so not a legal passive target however close it is. VERA's retired
    /// nearest-first key picked the wall.
    #[test]
    fn gsi_08_01_a_guard_tank_shoots_the_engineer_not_the_nearer_wall() {
        let rules = scan_rules();
        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            10,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            2,
            "WALL",
            "Russians",
            11,
            10,
            EntityCategory::Structure,
        );
        place(
            &mut entities,
            3,
            "ENGINEER",
            "Russians",
            13,
            10,
            EntityCategory::Infantry,
        );
        assert_eq!(pick(&entities, &rules, 1), Some(3));
    }

    /// The other half of the same gate: an armed defence with a real
    /// `ThreatPosed=` IS auto-acquired, an unarmed structure never is. Stock
    /// authors 30-40 on the Pillbox, Sentry Gun, Tesla Coil, Prism Tower,
    /// Gattling Cannon and Psychic Tower, and 0 (or nothing) on every economic
    /// building, on walls, and on the SAM Site, Flak Cannon and Grand Cannon.
    #[test]
    fn gsi_08_01_threat_posed_decides_which_enemy_buildings_are_auto_targets() {
        let rules = scan_rules();
        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            10,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            2,
            "POWER",
            "Russians",
            11,
            10,
            EntityCategory::Structure,
        );
        assert_eq!(
            pick(&entities, &rules, 1),
            None,
            "an unarmed enemy structure is invisible to auto-acquire"
        );

        place(
            &mut entities,
            3,
            "PILLBOX",
            "Russians",
            12,
            10,
            EntityCategory::Structure,
        );
        assert_eq!(
            pick(&entities, &rules, 1),
            Some(3),
            "a defence with ThreatPosed=30 is acquired even though it is further away"
        );
    }

    /// The conjunct the gate would be missing without the weapon test: an
    /// UNARMED building that authors a non-zero `ThreatPosed=` is still refused.
    ///
    /// Native rejects on **(no current weapon) OR (`ThreatPosed` == 0)** —
    /// `0x006F85D3` calls `GetCurrentWeapon` and both misses jump to the reject
    /// at `0x006F85F0` before `ThreatPosed` is ever asked for. Seven stock
    /// buildings sit in exactly this shape: the Chronosphere `GACSPH`, Weather
    /// Control `GAWEAT`, Iron Curtain `NAIRON` and Genetic Mutator `YAGNTC`
    /// (`ThreatPosed=1`, no weapon key), plus `YAPPET`, `GADUMY` and
    /// `AMMOCRAT`. A player's units must never spontaneously open fire on an
    /// enemy superweapon.
    #[test]
    fn gsi_08_01_an_unarmed_building_with_threat_posed_is_still_refused() {
        let rules = scan_rules();
        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            10,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            2,
            "CHRONO",
            "Russians",
            11,
            10,
            EntityCategory::Structure,
        );
        assert_eq!(
            pick(&entities, &rules, 1),
            None,
            "ThreatPosed=1 with no weapon is not a legal passive target"
        );

        place(
            &mut entities,
            3,
            "PILLBOX",
            "Russians",
            12,
            10,
            EntityCategory::Structure,
        );
        assert_eq!(
            pick(&entities, &rules, 1),
            Some(3),
            "the armed defence further out is taken over the nearer superweapon"
        );
    }

    /// The trap in the other direction: the weapon test is
    /// `GetCurrentWeapon @ 0x0070E1A0`, not `Primary=`. `[YAGGUN]` the Gattling
    /// Cannon authors `TurretCount=1`, `WeaponCount=6` and `Weapon1..6=` with no
    /// `Primary=` key at all, so a naive `primary.is_some()` would disarm it and
    /// hand the player a base defence their units silently ignore.
    #[test]
    fn gsi_08_01_a_weapon_array_defence_with_no_primary_key_stays_a_target() {
        let rules = scan_rules();
        let gattling = rules.object("GATTLING").expect("GATTLING");
        let mut building = GameEntity::test_default(2, "GATTLING", "Russians", 11, 10);
        building.category = EntityCategory::Structure;
        assert!(
            is_armed(&building, gattling),
            "a WeaponCount= defence resolves slot 0 through GetCurrentWeapon"
        );

        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            10,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            2,
            "GATTLING",
            "Russians",
            11,
            10,
            EntityCategory::Structure,
        );
        assert_eq!(pick(&entities, &rules, 1), Some(2));
    }

    /// `Insignificant=` at `0x006F8451`: civilian traffic is not a target.
    /// Stock puts the flag on 26 vehicle types and 22 infantry types.
    #[test]
    fn gsi_08_01_insignificant_civilians_are_skipped_for_the_real_enemy() {
        let rules = scan_rules();
        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            10,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            2,
            "CAR",
            "Russians",
            11,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            3,
            "SCOUT",
            "Russians",
            13,
            10,
            EntityCategory::Unit,
        );
        assert_eq!(pick(&entities, &rules, 1), Some(3));
    }

    /// Inside one band the maximum score wins, not the first cell reached.
    ///
    /// Both enemies sit on ring 2, and the ring's row pass reaches the
    /// north-west cell before the south-east one, so walk order alone would
    /// pick the plain SCOUT. `PRIZE` carries `SpecialThreatValue=10`, worth
    /// `+2000` through the `C` coefficient, and displaces it on the
    /// strictly-greater keep at `0x006F94A0`.
    #[test]
    fn gsi_08_01_the_higher_score_beats_the_earlier_cell_in_the_same_band() {
        let rules = scan_rules();
        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            10,
            10,
            EntityCategory::Unit,
        );
        // Owning a building puts this house on the per-type coefficient set,
        // which is what `HouseClass+0x1FB` selects once an MCV has deployed.
        place(
            &mut entities,
            9,
            "POWER",
            "Americans",
            20,
            20,
            EntityCategory::Structure,
        );
        place(
            &mut entities,
            2,
            "SCOUT",
            "Russians",
            8,
            8,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            3,
            "PRIZE",
            "Russians",
            12,
            12,
            EntityCategory::Unit,
        );
        assert_eq!(pick(&entities, &rules, 1), Some(3));
    }

    /// The band early-return at `0x006F94D0` is what makes a defence commit to
    /// whatever is at its feet: with a radius of 6 the walk returns as soon as
    /// ring 1 has produced anything, so the far higher-scoring `PRIZE` is never
    /// evaluated at all.
    #[test]
    fn gsi_08_01_the_first_band_ends_the_scan_before_the_better_target_is_seen() {
        let rules = scan_rules();
        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            10,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            9,
            "POWER",
            "Americans",
            20,
            20,
            EntityCategory::Structure,
        );
        place(
            &mut entities,
            2,
            "SCOUT",
            "Russians",
            11,
            10,
            EntityCategory::Unit,
        );
        place(
            &mut entities,
            3,
            "PRIZE",
            "Russians",
            14,
            10,
            EntityCategory::Unit,
        );
        assert_eq!(pick(&entities, &rules, 1), Some(2));
    }

    /// `0x0070CD4E` picks between two coefficient sets on the scorer house's
    /// `+0x1FB` byte. The sets are not variations on a theme: two of the five
    /// weights change sign and the distance weight changes by 10x, so a scorer
    /// on the wrong set can rank the same two candidates in the opposite order.
    ///
    /// Every house in a skirmish carries the byte SET from construction
    /// (`HouseClass::Constructor @ 0x004F644E`), so the per-type branch is the
    /// live one for the whole match — see [`HOUSE_SELECTS_OWN_COEFFICIENTS`].
    #[test]
    fn gsi_08_01_every_house_takes_the_per_type_coefficient_set() {
        let rules = scan_rules();
        let obj = rules.object("GRIZZLY").expect("GRIZZLY");
        let live = ThreatCoefficients::resolve(&rules, obj, HOUSE_SELECTS_OWN_COEFFICIENTS);
        let byte_clear = ThreatCoefficients::resolve(&rules, obj, false);
        assert_eq!(live.target_effectiveness, -200.0);
        assert_eq!(live.target_strength, -200.0);
        assert_eq!(live.target_distance, -10.0);
        assert_eq!(byte_clear.target_effectiveness, 200.0);
        assert_eq!(byte_clear.target_strength, 200.0);
        assert_eq!(byte_clear.target_distance, -1.0);
    }

    /// `TechnoClass::Get_ThreatPosed @ 0x00708B40`: a garrisoned building's
    /// threat comes from its occupants, not its own type value, which is how a
    /// civilian house full of GIs becomes a target for units that would ignore
    /// the empty one.
    #[test]
    fn gsi_08_01_threat_per_occupant_replaces_the_type_value() {
        let rules = scan_rules();
        let obj = rules.object("POWER").expect("POWER");
        let mut building = GameEntity::test_default(1, "POWER", "Russians", 5, 5);
        building.category = EntityCategory::Structure;
        assert_eq!(live_threat_posed(&rules, &building, obj), 0);
        assert_eq!(rules.general.threat_per_occupant, 10);
    }

    /// The literal loop order of `Greatest_Threat`'s ring walk, including the
    /// centre cell being visited twice at r = 0. Both facts are load-bearing:
    /// the order decides which of two equal-scoring candidates wins, and the
    /// duplicate is what the native `do/while` does.
    #[test]
    fn ring_zero_visits_the_centre_twice() {
        assert_eq!(ring_cells(10, 10, 0), vec![(10, 10), (10, 10)]);
    }

    #[test]
    fn ring_one_is_the_eight_neighbours_row_pass_first() {
        assert_eq!(
            ring_cells(10, 10, 1),
            vec![
                // Row pass: north row and south row interleaved, west to east.
                (9, 9),
                (9, 11),
                (10, 9),
                (10, 11),
                (11, 9),
                (11, 11),
                // Column pass: west then east, dy = 0 only.
                (9, 10),
                (11, 10),
            ]
        );
    }

    #[test]
    fn ring_two_has_the_sixteen_perimeter_cells_once_each() {
        let cells = ring_cells(10, 10, 2);
        assert_eq!(cells.len(), 16);
        let mut sorted = cells.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 16, "no cell is visited twice on ring 2");
        for (x, y) in cells {
            assert!(
                (x - 10).abs() == 2 || (y - 10).abs() == 2,
                "({x}, {y}) is not on the ring-2 perimeter"
            );
        }
    }

    /// `0x006F94D0`: the walk gives up early at a quarter and a half of the
    /// radius. For a stock Grizzly (radius 6) that means rings {0,1} decide
    /// first, then {2,3}, then {4,5}.
    #[test]
    fn early_return_rings_for_a_stock_grizzly_radius() {
        let radius = 6;
        let bands: Vec<i32> = (0..radius)
            .filter(|ring| is_early_return_ring(*ring, radius))
            .collect();
        assert_eq!(bands, vec![1, 3]);
    }

    /// A score of exactly zero is a rejection, not a low accept; a negative one
    /// is clamped up to 1 and accepted.
    #[test]
    fn zero_score_rejects_and_negative_clamps_to_one() {
        assert_eq!(finish_score(0), None);
        assert_eq!(finish_score(-5), Some(1));
        assert_eq!(finish_score(42), Some(42));
    }

    #[test]
    fn vhp_scan_matches_original_integer_slices() {
        let rows: serde_json::Value =
            serde_json::from_str(include_str!("../../../tools/spatial_oracle/vhp_scan.json"))
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

    fn vhp_rules(mode: &str, score: i32) -> RuleSet {
        let strength_coefficient = score - 100_000;
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nMyEffectivenessCoefficientDefault=0\nTargetEffectivenessCoefficientDefault=0\n\
             TargetSpecialThreatCoefficientDefault=0\nTargetStrengthCoefficientDefault={strength_coefficient}\n\
             TargetDistanceCoefficientDefault=0\n\
             [VehicleTypes]\n0=GRIZZLY\n1=SCOUT\n\
             [GRIZZLY]\nStrength=300\nPrimary=GUN\nVHPScan={mode}\n\
             [SCOUT]\nStrength=301\nArmor=heavy\n\
             [WeaponTypes]\n0=GUN\n[GUN]\nDamage=100\nRange=5\nWarhead=WH\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
        ))).unwrap()
    }

    fn vhp_entities(estimates: [i32; 2]) -> EntityStore {
        let mut entities = EntityStore::new();
        place(
            &mut entities,
            1,
            "GRIZZLY",
            "Americans",
            5,
            5,
            EntityCategory::Unit,
        );
        for (id, estimate) in [2, 3].into_iter().zip(estimates) {
            place(
                &mut entities,
                id,
                "SCOUT",
                "Soviets",
                5 + id as u16,
                5,
                EntityCategory::Unit,
            );
            let entity = entities.get_mut(id).unwrap();
            entity.health.current = 301;
            entity.estimated_health =
                crate::sim::estimated_health::EstimatedHealth::from_raw(estimate);
        }
        entities
    }

    #[test]
    fn vhp_scan_changes_live_flat_scan_selection_at_estimate_boundaries() {
        for (mode, estimates, expected) in [
            ("None", [0, 301], Some(2)),
            ("Strong", [0, 301], Some(3)),
            ("Strong", [-1, 0], None),
            ("Strong", [1, 301], Some(2)),
            ("Normal", [0, 301], Some(3)),
            ("Normal", [301, 150], Some(3)),
            ("Normal", [301, 151], Some(2)),
        ] {
            let rules = vhp_rules(mode, 100);
            let entities = vhp_entities(estimates);
            assert_eq!(
                pick_with_mask(&entities, &rules, 1, super::super::ScanMission::Hunt),
                expected,
                "{mode} {estimates:?}"
            );
        }
    }

    #[test]
    fn vhp_scan_halves_before_zero_rejection_without_changing_raw_retaliation_score() {
        for score in [-1, 1] {
            let entities = vhp_entities([0, -10]);
            let rules = vhp_rules("Normal", score);
            assert_eq!(pick(&entities, &rules, 1), None, "{score}/2 rejects");
            let interner = test_interner();
            let raw = super::super::combat_targeting::calculate_ai_threat_score(
                &entities, 1, 2, &rules, &interner, None, None,
            )
            .unwrap();
            assert_eq!(truncate_score(raw), score, "raw70CD10 remains unchanged");
            let none_rules = vhp_rules("None", score);
            assert!(
                pick(&entities, &none_rules, 1).is_some(),
                "None accepts {score}"
            );
        }
    }

    /// The `0.02f` Verses floor is a FLOAT constant widened to double, so an
    /// authored `Verses=2%` sits just above it and survives while 1% does not.
    #[test]
    fn verses_floor_rejects_one_percent_but_not_two() {
        assert!(0.01_f64 <= VERSES_FLOOR);
        assert!(0.02_f64 > VERSES_FLOOR);
        assert!(0.0_f64 <= VERSES_FLOOR);
    }
}

#[cfg(test)]
#[path = "greatest_threat_health_tests.rs"]
mod health_tests;
