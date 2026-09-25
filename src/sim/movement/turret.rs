//! Turret rotation system — the `UnitClass::Facing_Update @ 0x00736990` port.
//!
//! Units with a barrel `FacingClass` have an independently rotating turret.
//! When attacking, the turret rotates toward the target at the unit's `ROT=`
//! speed. When the target is gone it holds the aim for
//! `GuardAreaTargetingDelay + 5` frames measured from the unit's OWN LAST SHOT
//! (`TechnoClass+0x120`), then returns to the hull — or leads toward the move
//! destination when one is set. The weapon-fire alignment gate lives in
//! `sim/combat` and reads the same `FacingClass` animated value.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/components, sim/combat, rules/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::util::fixed_math::{SimFixed, facing_from_delta_int_u16};

/// Compute the signed shortest-path rotation from `current` to `target` in
/// 8-bit facing space. No production caller; the live shortest-arc logic is the
/// signed 16-bit subtraction inside `FacingClass::current`.
/// Returns a value in -128..=127 (positive = clockwise, negative = counter-clockwise).
#[cfg(test)]
pub fn shortest_rotation(current: u8, target: u8) -> i16 {
    let diff: i16 = target as i16 - current as i16;
    // Wrap into -128..127 range for shortest path.
    if diff > 128 {
        diff - 256
    } else if diff < -128 {
        diff + 256
    } else {
        diff
    }
}

/// Compute 16-bit turret facing from source to target using lepton-precise
/// positions, providing sub-cell accuracy for targeting.
pub fn facing_toward_lepton(
    from_rx: u16,
    from_ry: u16,
    from_sub_x: SimFixed,
    from_sub_y: SimFixed,
    to_rx: u16,
    to_ry: u16,
    to_sub_x: SimFixed,
    to_sub_y: SimFixed,
) -> u16 {
    let from_lep_x: i32 = from_rx as i32 * 256 + from_sub_x.to_num::<i32>();
    let from_lep_y: i32 = from_ry as i32 * 256 + from_sub_y.to_num::<i32>();
    let to_lep_x: i32 = to_rx as i32 * 256 + to_sub_x.to_num::<i32>();
    let to_lep_y: i32 = to_ry as i32 * 256 + to_sub_y.to_num::<i32>();
    let dx: i32 = to_lep_x - from_lep_x;
    let dy: i32 = to_lep_y - from_lep_y;
    facing_from_delta_int_u16(dx, dy)
}

/// Convert 8-bit body facing to 16-bit turret facing.
/// Maps 0..255 → 0..65280 (shifts into the upper byte).
#[inline]
pub fn body_facing_to_turret(body: u8) -> u16 {
    (body as u16) << 8
}

/// NO-DIFF (GSI-08.14) — one facing is right, and pass 1's premise was wrong.
/// `TechnoClass` carries exactly TWO `FacingClass` instances: the body at
/// `+0x388` and the turret at `+0x3A0` (0x18 stride; `+0x3B8` is
/// `CurrentBurstIndex`, not a third facing). There is no separate barrel
/// facing, so `barrel_facing` here IS native's turret facing and the fire
/// location reads that same value — the claimed coupling to the FLH slice
/// (`GSI-08.04`) does not exist. `TurretROT=` likewise does not exist in
/// gamemd; the only `TurretRot`-shaped string in the image is
/// `TurretRotateSound`, so driving turret rotation from `ROT=` is correct.
///
/// The hull heading as a 16-bit facing — `FacingClass::Current` on the primary
/// facing `+0x388`. VERA keeps the animated hull in `body_facing` only while a
/// rotation is live and mirrors its top byte into `entity.facing`, so read the
/// interpolator when it exists and the byte otherwise.
pub(crate) fn hull_facing_16(entity: &GameEntity, binary_frame: u32) -> u16 {
    match entity.body_facing {
        Some(ref hull) => hull.current(binary_frame),
        None => body_facing_to_turret(entity.facing),
    }
}

/// Lepton-precise facing from `entity` toward a resolved attack target, using
/// the target's own coordinate slot. gamemd reaches the target through
/// `DirectionToTarget @ 0x005F3DB0`, which calls `GetCoords` (vtable `+0x48`) on
/// both objects; for a `BuildingClass` that slot returns the FOUNDATION CENTRE,
/// not the north-west anchor cell. `combat::resolve_target_coords` applies the
/// same centre shift, so route through it rather than reading `position`.
pub(crate) fn facing_toward_target(
    entity: &GameEntity,
    target: &crate::sim::combat::TargetKind,
    entities: &EntityStore,
    rules: Option<&RuleSet>,
    interner: &crate::sim::intern::StringInterner,
) -> Option<u16> {
    let (trx, try_, tsx, tsy) =
        crate::sim::combat::resolve_target_coords(target, entities, rules, interner)?;
    Some(facing_toward_lepton(
        entity.position.rx,
        entity.position.ry,
        entity.position.sub_x,
        entity.position.sub_y,
        trx,
        try_,
        tsx,
        tsy,
    ))
}

/// One frame of `UnitClass::Facing_Update @ 0x00736990`, expressed as data so
/// the read window can run in the combat Phase-2 pass and the writes land at the
/// post-batch apply point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct FacingUpdate {
    /// Turret (`+0x3A0`) destination to hand `FacingClass::set`. `None` when
    /// native calls no `Set` on the turret this frame — which is the whole
    /// difference between "hold the aim" and "swing back to the hull".
    pub turret_destination: Option<u16>,
    /// Hull (`+0x388`) destination from the turretless arm-A commit.
    pub hull_destination: Option<u16>,
    /// Which side of the `+0x6AF` store `turret_destination` belongs on.
    ///
    /// Native writes the latch mid-arm-B: `MOV [ESI+0x6AF],AL` at
    /// `0x00736B16` sits BETWEEN arm A's aim `Set` (`0x00736A89`) and arm B's
    /// idle-return `Set` (`0x00736BDD`), so an aim `Set` arms the latch on the
    /// same frame while an idle-return `Set` does not. `apply_unit_facing`
    /// commits the two in that order and reads the latch in between; this flag
    /// is how it tells them apart. There is no `latch` field: the latch is not
    /// a decision this pure read can make, it is `Is_Rotating(+0x3A0)`
    /// evaluated on the barrel as it stands after the aim write.
    pub turret_destination_is_idle_return: bool,
}

/// `UnitClass::Facing_Update @ 0x00736990` — verified this session by
/// `decompile_function` plus `disassemble_function`, which is where every
/// receiver binding below comes from (`LEA ECX,[ESI+0x3A0]` = turret at
/// `0x00736A1A`/`0x00736A26`/`0x00736A9F`/`0x00736AEA`/`0x00736BF3`;
/// `LEA [ESI+0x388]` = hull at `0x00736A0E`/`0x00736A66`/`0x00736BCF`). It
/// consumes no RNG and arms no timer other than the `FacingClass` countdown.
///
/// PURE READ — mutates neither the entity nor the store. Shared by the global
/// turret sweep and the per-object Fire→Facing host so both compute identical
/// destinations (per-entity, so id-order and live-order walks agree).
///
/// Native order, and what each arm does here:
///
/// **A. AIM** (`0x00736997`..`0x00736A89`), only when `Target != 0` AND the
/// latch `+0x6AF` is clear. `tgt = DirectionToTarget(this, Target)`.
/// - `Turret=yes`: fetch the current weapon slot through vtable `+0x3F4`; skip
///   the whole aim when its `WeaponType` sets `OmniFire=` (`+0x12B`, checked at
///   `0x007369F4`) — an omni weapon never turns the turret. Otherwise
///   `turret.Set(tgt)`.
/// - `Turret=no`: only when `SpeedType == Track` (`Type+0x67C == 1`), no NavCom
///   and the locomotor reports not-moving, and then only when the ANIMATED hull
///   already equals `tgt` exactly (`0x00736A78` compares the low word) — the
///   mid-arc pass-through pin.
///
/// **B. IDLE** (`0x00736A8E`..`0x00736BDD`). The latch is cleared
/// unconditionally at `0x00736AD5`, before the `Turret=` test, so a turretless
/// unit leaves this arm with a clear latch and nothing else. For a turret:
/// still rotating → re-arm the latch and stop; target still held → stop;
/// otherwise the idle return, gated on
/// `frame - LastFireFrame(+0x120) >= GuardAreaTargetingDelay(Rules+0xE04) + 5`
/// (41 frames in stock) and suppressed while the unit is bunkered (`+0x2E4`).
/// The destination is the MOVE DESTINATION when a NavCom is set, else the hull
/// heading. The latch VALUE is committed by `apply_unit_facing` rather than
/// here, because native's store sits between this arm's two `Set` calls — see
/// [`FacingUpdate::turret_destination_is_idle_return`].
///
/// **C. CACHE** (`0x00736BE2`) writes `+0x4A0 = Is_Rotating()`; that field has
/// no verified consumer, so it is not modelled.
///
/// RESIDUAL (GSI-08.14) — `TurretSpins=` (`Type+0xD21`) replaces both arms with
/// the permaspin formula at `0x00736AB1`..`0x00736ACB`. Trigger: a type that
/// authors the key. Player effect: its turret does not idle-spin. Frequency:
/// `[DISK]` alone in stock rulesmd, and the key is not parsed here at all.
/// Downstream risk: none — the arm is self-contained.
///
/// Native execution of this arm pair frame by frame:
/// `tools/spatial_oracle/turret_cadence.py`.
///
/// RESIDUAL (GSI-08.14) — three idle-hold inputs have no VERA analogue and are
/// therefore not gated: the per-weapon-slot lock byte `WeaponStruct+0x18`
/// (`0x00736A02`/`0x00736B96`; executed: with a target it aims the turret at
/// the hull's current facing instead of the target, and its idle return
/// ignores the NavCom; zero stock authors for the adjacent
/// `WeaponXTurretLocked` art key), the locomotor-piggyback flag
/// `+0x6AD` (`0x00736BB1`) and the simple-deployer byte `+0x6E0`
/// (`0x00736B70`, paired with `IsSimpleDeployer=`). Trigger: a piggybacked or
/// mid-deploy simple deployer losing its target. Player effect: its turret
/// returns to the hull where retail holds the last aim. Frequency: six stock
/// `IsSimpleDeployer=` types, and only during the deploy transition.
/// Downstream risk: none — each is a pure suppression.
///
/// RESIDUAL (GSI-08.14) — recoil is not modelled. Native keeps two `RecoilData`
/// structs at `+0x3D8`/`+0x3F8`, arms them in `Fire_At` and advances them from
/// `TechnoClass::AI_Update` through `0x0070ED10`; the displacement is read by
/// four instructions, all in draw code, so it is render-only and feeds no
/// gameplay path.
/// - Trigger: firing a type that authors `TurretRecoil=`.
/// - Player effect: the barrel does not slide back on firing.
/// - Frequency: two stock authors, both BUILDINGS (the Grand Cannon and
///   CAEAST02); no stock vehicle recoils at all.
/// - Downstream risk: none to the simulation. It consumes no RNG.
pub(crate) fn facing_update(
    entity: &GameEntity,
    entities: &EntityStore,
    rules: Option<&RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
) -> FacingUpdate {
    let mut out = FacingUpdate {
        turret_destination: None,
        hull_destination: None,
        turret_destination_is_idle_return: false,
    };
    let obj = rules.and_then(|r| r.object(interner.resolve(entity.type_ref())));
    let has_turret = entity.barrel_facing.is_some();

    // --- A. AIM ---------------------------------------------------------
    let target_facing: Option<u16> = entity
        .attack_target
        .as_ref()
        .and_then(|attack| facing_toward_target(entity, &attack.target, entities, rules, interner));
    if let Some(tgt) = target_facing
        && !entity.turret_rotation_latch
    {
        if has_turret {
            if !current_weapon_is_omni_fire(entity, rules, interner) {
                out.turret_destination = Some(tgt);
            }
        } else if obj
            .is_some_and(|o| o.speed_type == crate::rules::locomotor_type::SpeedType::Track)
            && entity.movement_target.is_none()
            && hull_facing_16(entity, binary_frame) == tgt
        {
            out.hull_destination = Some(tgt);
        }
    }

    // --- B. IDLE --------------------------------------------------------
    // The `+0x6AF` write itself is NOT made here — `apply_unit_facing` makes
    // it, between the two `Set` calls, exactly where `0x00736B16` sits. What
    // this arm still owns is the BRANCH: native picks between "commit to the
    // arc" and "idle return" with `Is_Rotating(+0x3A0)` evaluated AFTER arm A's
    // `Set` (`CALL 0x004C9480` at `0x00736AF2`), while the read below runs
    // before it. The two agree in every reachable case, which is why the branch
    // may stay in this pure read: arm A only `Set`s when a target exists, the
    // idle return only runs when none does, and when native's post-`Set`
    // `Is_Rotating` is false with a target held it falls into the idle arm and
    // is thrown straight back out by the `Target != 0` test at `0x00736B21`.
    if has_turret {
        let barrel = entity.barrel_facing.as_ref().expect("has_turret");
        // `0x00736AF9` not taken — the unit has committed to an arc, arm A
        // stays shut until it finishes, and there is no idle return on that
        // path (`JMP 0x00736BE2` at `0x00736B1C`). The turret therefore steps
        // in completed turns instead of re-snapshotting `prev` every frame
        // against a mover.
        let committed_to_an_arc = barrel.is_rotating(binary_frame);
        if !committed_to_an_arc && entity.attack_target.is_none() {
            let dwell = rules.map_or(NATIVE_IDLE_TURRET_DWELL_FALLBACK, |r| {
                i64::from(r.general.guard_area_targeting_delay)
            }) + NATIVE_IDLE_TURRET_DWELL_BIAS;
            let dwell_elapsed = i64::from(binary_frame) - entity.last_fire_frame >= dwell;
            // `+0x2E4` — a tank riding a Battle Bunker holds its aim forever.
            let bunkered = entity.bunker_link.installed_in().is_some();
            if dwell_elapsed && !bunkered {
                out.turret_destination = Some(match nav_destination_facing(entity, entities) {
                    Some(nav) => nav,
                    None => hull_facing_16(entity, binary_frame),
                });
                // `Set` at `0x00736BDD`, which native reaches only AFTER the
                // `+0x6AF` store — so the arc this starts leaves the latch
                // clear on its first frame.
                out.turret_destination_is_idle_return = true;
            }
        }
    }

    out
}

/// `[General] GuardAreaTargetingDelay=` fallback for rules-less fixtures. Stock
/// rulesmd sets 36; `RulesClass` stores it at `+0xE04` (`0x006701B4`).
const NATIVE_IDLE_TURRET_DWELL_FALLBACK: i64 = 36;

/// The `+5` the idle-return comparison adds to `GuardAreaTargetingDelay`
/// (`ADD EDX,0x5` at `0x00736B4B`), giving 41 frames in stock.
const NATIVE_IDLE_TURRET_DWELL_BIAS: i64 = 5;

/// The idle turret's aim when a move order is live — `DirectionToTarget(this,
/// NavCom)` at `0x00736BC3`. Native reads the destination coordinate at
/// `+0x5A4`; VERA's equivalent is the last cell of the active path.
fn nav_destination_facing(entity: &GameEntity, _entities: &EntityStore) -> Option<u16> {
    let goal = entity.movement_target.as_ref()?.path.last().copied()?;
    Some(facing_toward_lepton(
        entity.position.rx,
        entity.position.ry,
        entity.position.sub_x,
        entity.position.sub_y,
        goal.0,
        goal.1,
        SimFixed::from_num(128),
        SimFixed::from_num(128),
    ))
}

/// Whether this object's currently selected weapon sets `OmniFire=`. Native
/// takes the slot through `TechnoClass` vtable `+0x3F4` (`0x0070E1A0` —
/// `GetWeapon(TurretCount(+0x808) > 0 ? CurrentWeaponNumber(+0x138) : 0)`) and
/// reads `WeaponType+0x12B`. An omni weapon is skipped by both the aim arm
/// (`0x007369F4`) and the fire gate's facing test (`0x0074125C`) — it shoots in
/// any direction and never turns the turret.
pub(crate) fn current_weapon_is_omni_fire(
    entity: &GameEntity,
    rules: Option<&RuleSet>,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    let Some(rules) = rules else { return false };
    let Some(obj) = rules.object(interner.resolve(entity.type_ref())) else {
        return false;
    };
    let index: i32 = if obj.turret_count > 0 {
        i32::from(entity.current_weapon_index)
    } else {
        0
    };
    crate::sim::combat::combat_weapon::weapon_for_index(obj, entity.veterancy, index)
        .and_then(|(weapon_id, _)| rules.weapon(weapon_id))
        .is_some_and(|weapon| weapon.omni_fire)
}

/// The turret destination this entity's owning native path drives it toward
/// this frame, or `None` when that path calls no `Set`.
///
/// Dispatches on the class that actually owns the facing in gamemd:
/// - **Unit** — `UnitClass::Facing_Update @ 0x00736990` ([`facing_update`]).
/// - **Structure** — `BuildingClass::Mission_Attack @ 0x0044ACF0`, whose only
///   facing traffic is `turret(+0x388).Set(GetTargetCoords(Target))` on the
///   non-firing error arms (`0x0044B187`/`0x0044B1DE`/`0x0044B14E`). A LEA
///   census over `BuildingClass::Update`, `Mission_Guard` and every idle path
///   finds no other `Set`/`UpdateFacing` of `+0x388`, so **a building turret
///   keeps its last aim** — it never swings back.
/// - **Aircraft** — its mission/locomotor owns its secondary-facing writes.
/// - **Infantry** — legacy target-else-body rule.
///
/// Aircraft initializes BOTH facings even
/// without Turret=yes (413FD2..414015). Its self-writers belong to Mission_Attack
/// and Fly steering/takeoff (4181BB..4185DF, 4CE680, 4CF285/4CF3C5).
/// AI41514C instead READS its secondary facing and copies it into a Carryall
/// passenger; FireAt416041 samples it for launch math. Neither writes its own
/// facing. State4's two setters now run at the firing boundary. This generic
/// sweep must not overwrite them. Other Mission_Attack and Fly secondary
/// setters remain required residuals of those mechanisms.
pub(crate) fn desired_turret_facing(
    entity: &GameEntity,
    entities: &EntityStore,
    rules: Option<&RuleSet>,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
) -> Option<u16> {
    entity.barrel_facing.as_ref()?;
    match entity.category {
        crate::map::entities::EntityCategory::Unit => {
            facing_update(entity, entities, rules, interner, binary_frame).turret_destination
        }
        crate::map::entities::EntityCategory::Structure => entity
            .attack_target
            .as_ref()
            .and_then(|attack| {
                facing_toward_target(entity, &attack.target, entities, rules, interner)
            })
            .or_else(|| {
                // A target that despawned this tick: native re-reads `+0x2B4`,
                // which the death helper has already cleared, so `Mission_Attack`
                // takes no facing action at all. Hold.
                None
            }),
        crate::map::entities::EntityCategory::Aircraft => None,
        _ => Some(
            entity
                .attack_target
                .as_ref()
                .and_then(|attack| {
                    facing_toward_target(entity, &attack.target, entities, rules, interner)
                })
                .unwrap_or_else(|| body_facing_to_turret(entity.facing)),
        ),
    }
}

/// Per-binary-frame turret rotation for the classes this sweep still owns —
/// Buildings and legacy Infantry. Unit turrets are driven per-object by the combat
/// Phase-2 read window plus `unit_post::apply_unit_facing` while
/// `L2_UNIT_POST_AUTHORITATIVE` holds.
///
/// Calls `FacingClass::set`, which is a no-op when the desired facing equals the
/// current destination — so this function is idempotent. `None` from
/// [`desired_turret_facing`] means "native calls no `Set` this frame", which is
/// how a building turret holds its last aim.
///
/// gamemd-derived: `BuildingClass::Mission_Attack @ 0x0044ACF0` for structures
/// (all four facing sites are on `+0x388`, `0x0044B14E`/`0x0044B187`/
/// `0x0044B1DE` plus the voxel snap at `0x0044B0AC`); ROT comes from
/// `BuildingType+0x71C`, the same `ROT=` key this reads.
pub fn tick_turret_rotation(
    entities: &mut EntityStore,
    rules: &RuleSet,
    native_frame: u32,
    interner: &crate::sim::intern::StringInterner,
) {
    struct TurretUpdate {
        id: u64,
        target_facing: u16,
    }
    let mut updates: Vec<TurretUpdate> = Vec::new();

    // Phase 1: read each turreted entity's desired facing.
    let keys: Vec<u64> = entities.keys_sorted();
    for &id in &keys {
        let entity = match entities.get(id) {
            Some(e) => e,
            None => continue,
        };
        // Unit turrets are driven per-object by unit_post once authoritative; leave
        // Aircraft/Building turrets on this sweep.
        if crate::sim::world::unit_post::L2_UNIT_POST_AUTHORITATIVE
            && entity.category == crate::map::entities::EntityCategory::Unit
        {
            continue;
        }
        // A warped object's AI sets no facing; its barrel finishes the turn it
        // was given (`GameEntity::ai_frozen`).
        if entity.ai_frozen() {
            continue;
        }
        // Skip non-turreted entities; otherwise take the per-entity desired facing
        // from the shared helper (single source for sweep + per-object host).
        let Some(desired_facing) =
            desired_turret_facing(entity, entities, Some(rules), interner, native_frame)
        else {
            continue;
        };

        updates.push(TurretUpdate {
            id,
            target_facing: desired_facing,
        });
    }

    // Phase 2: apply rotation via FacingClass::set. Idempotent — no-op when
    // target already equals current destination.
    for update in &updates {
        let rot = rules
            .object(
                interner.resolve(
                    entities
                        .get(update.id)
                        .map(|e| e.type_ref())
                        .unwrap_or_default(),
                ),
            )
            .map(|obj| obj.turret_rot)
            .unwrap_or(5);
        if let Some(entity) = entities.get_mut(update.id) {
            if let Some(ref mut barrel) = entity.barrel_facing {
                // Refresh ROT in case rules changed (cheap; idempotent).
                barrel.set_rot(rot);
                barrel.set(update.target_facing, native_frame);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shortest_rotation_clockwise() {
        assert_eq!(shortest_rotation(0, 10), 10);
        assert_eq!(shortest_rotation(200, 210), 10);
    }

    #[test]
    fn test_shortest_rotation_counter_clockwise() {
        assert_eq!(shortest_rotation(10, 0), -10);
        assert_eq!(shortest_rotation(10, 250), -16); // 250 - 10 = 240 > 128, so 240-256=-16
    }

    #[test]
    fn test_shortest_rotation_wrap_around() {
        // From 250 to 10: clockwise is +16, counter-clockwise is -240. Should pick +16.
        assert_eq!(shortest_rotation(250, 10), 16);
        // From 10 to 250: clockwise is +240, counter-clockwise is -16. Should pick -16.
        assert_eq!(shortest_rotation(10, 250), -16);
    }

    #[test]
    fn test_facing_toward_lepton_cardinal() {
        use crate::util::fixed_math::SimFixed;
        let center = SimFixed::from_num(128);
        // Target 5 cells east: should be ~16384 (E).
        let f = facing_toward_lepton(10, 10, center, center, 15, 10, center, center);
        assert!((f as i32 - 16384).abs() < 2, "east facing={f}");
        // Target 5 cells south: should be ~32768 (S).
        let f = facing_toward_lepton(10, 10, center, center, 10, 15, center, center);
        assert!((f as i32 - 32768).abs() < 2, "south facing={f}");
    }

    #[test]
    fn test_facing_toward_lepton_subcell_precision() {
        use crate::util::fixed_math::SimFixed;
        // Same cell, but target is at sub_x=200, sub_y=128 vs source at sub_x=50, sub_y=128.
        // Delta: dx_lep = +150, dy_lep = 0 → pure east → ~16384.
        let f = facing_toward_lepton(
            10,
            10,
            SimFixed::from_num(50),
            SimFixed::from_num(128),
            10,
            10,
            SimFixed::from_num(200),
            SimFixed::from_num(128),
        );
        assert!((f as i32 - 16384).abs() < 2, "sub-cell east facing={f}");
    }

    #[test]
    fn test_body_facing_to_turret() {
        assert_eq!(body_facing_to_turret(0), 0);
        assert_eq!(body_facing_to_turret(64), 16384);
        assert_eq!(body_facing_to_turret(128), 32768);
        assert_eq!(body_facing_to_turret(255), 65280);
    }
}
