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
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::util::direction_tables::facing16_from_delta;
use crate::util::fixed_math::SimFixed;

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
    facing16_from_delta(dx, dy)
}

/// Convert an 8-bit facing to a 16-bit one (shifts into the upper byte).
/// No production caller; test fixtures author 8-bit facings.
#[cfg(any(test, feature = "test-support"))]
pub fn body_facing_to_turret(body: u8) -> u16 {
    (body as u16) << 8
}

// NO-DIFF (GSI-08.14) — one turret facing is right, and pass 1's premise was
// wrong. `TechnoClass` carries three `FacingClass` instances (0x18 stride): the
// barrel elevation at `+0x370` (`GameEntity::barrel_elevation`), the body at
// `+0x388` (`GameEntity::body_facing`) and the turret at `+0x3A0` (`+0x3B8` is
// `CurrentBurstIndex`, not a fourth). `barrel_facing` here is native's turret
// facing, and the fire location reads that same value — the claimed coupling to
// the FLH slice (`GSI-08.04`) does not exist. `TurretROT=` likewise does not
// exist in gamemd; the only `TurretRot`-shaped string in the image is
// `TurretRotateSound`, so driving turret rotation from `ROT=` is correct.

/// Lepton-precise facing from `entity` toward a resolved attack target, using
/// the target's own coordinate slot. gamemd reaches the target through
/// `DirectionToTarget @ 0x005F3DB0`, which calls `GetCoords` (vtable `+0x48`) on
/// both objects; for a `BuildingClass` that slot returns the FOUNDATION CENTRE,
/// not the north-west anchor cell. `combat::resolve_target_coords` applies the
/// same centre shift, so route through it rather than reading `position`.
pub fn facing_toward_target(
    entity: &GameEntity,
    target: &crate::sim::combat::TargetKind,
    world: &crate::sim::world::Simulation,
) -> Option<u16> {
    let (trx, try_, tsx, tsy) = crate::sim::combat::resolve_target_coords(target, world)?;
    Some(facing_toward_lepton(
        entity.position.rx(),
        entity.position.ry(),
        entity.position.sub_x(),
        entity.position.sub_y(),
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
    /// `UnitClass::Facing_Update` enters the `TurretSpins` arm after arm A's
    /// optional aim Set. The write owner must sample the barrel's animated
    /// `Current` after that Set, then issue the second Set and skip ordinary
    /// latch/idle handling.
    pub turret_spins: bool,
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
/// PURE READ — mutates neither the entity nor the store; the per-object
/// Fire→Facing host commits what it returns.
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
/// **B. TURRET SPINS** (`0x00736A95`..`0x00736BDD`), tested after arm A when
/// `TurretSpins` is set. A target can first set its aim at `0x00736A89`; the
/// spin arm then samples turret `Current` and issues a second Set, which
/// becomes the final same-frame destination. The type flag is read at
/// `0x00713357`..`0x00713371`. Its formula and Set execute in
/// `apply_unit_facing`, after any arm-A Set, because `Current` observes that
/// first write. This branch bypasses the ordinary latch and idle path, leaving
/// the prior latch byte unchanged.
///
/// **C. IDLE** (`0x00736A8E`..`0x00736BDD`) when the spin branch is not taken.
/// The latch is cleared
/// unconditionally at `0x00736AD5`, before the `Turret=` test, so a turretless
/// unit leaves this arm with a clear latch and nothing else. For a turret:
/// still rotating → re-arm the latch and stop; target still held → stop;
/// otherwise the idle return, gated on
/// `frame - LastFireFrame(+0x120) >= GuardAreaTargetingDelay(Rules+0xE04) + 5`
/// (41 frames in stock) and suppressed while the unit is bunkered (`+0x2E4`)
/// or is a deployed (`+0x6E0`) `IsSimpleDeployer=` type (`0x00736B60..0x00736B78`),
/// such as a deployed Siege Chopper, which holds its last aim.
/// The destination is the MOVE DESTINATION when a NavCom is set, else the hull
/// heading. The latch VALUE is committed by `apply_unit_facing` rather than
/// here, because native's store sits between this arm's two `Set` calls — see
/// [`FacingUpdate::turret_destination_is_idle_return`].
///
/// **D. CACHE** (`0x00736BE2`) writes `+0x4A0 = Is_Rotating()`; that field has
/// no verified consumer, so it is not modelled.
///
/// Native execution of this arm pair frame by frame:
/// `tools/spatial_oracle/turret_cadence.py`.
///
/// RESIDUAL (GSI-08.14) — one Facing_Update input has no VERA analogue:
/// - The per-weapon-slot lock byte `WeaponStruct+0x18` (`0x00736A02`,
///   `0x00736B96`). Executed: with a target it aims the turret at the hull's
///   current facing instead of the target, and its idle return ignores the
///   NavCom. Trigger: a turret-locked weapon slot. Frequency: none in retail;
///   no stock type authors the adjacent `WeaponXTurretLocked` art key.
///   Downstream risk: the aim feeds the fire-facing gate, so a modded locked
///   weapon would fire on VERA's aim, not native's.
///
/// Recoil's separate presentation state (+3D8/+3F8) is owned by
/// `game_entity::voxel_recoil`; it does not change aim or consume RNG.
pub(crate) fn facing_update(
    entity: &GameEntity,
    world: &crate::sim::world::Simulation,
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
    binary_frame: u32,
) -> FacingUpdate {
    let entities = &world.substrate.entities;
    let mut out = FacingUpdate {
        turret_destination: None,
        turret_spins: false,
        hull_destination: None,
        turret_destination_is_idle_return: false,
    };
    let obj = interner.object_type(entity.type_ref(), rules);
    let has_turret = entity.barrel_facing.is_some();

    // --- A. AIM ---------------------------------------------------------
    let target_facing: Option<u16> = entity
        .attack_target
        .target()
        .and_then(|attack| facing_toward_target(entity, &attack, world));
    if let Some(tgt) = target_facing
        && !entity.turret_rotation_latch.is_set()
    {
        if has_turret {
            if !current_weapon_is_omni_fire(entity, entities, rules, interner) {
                out.turret_destination = Some(tgt);
            }
        } else if obj
            .is_some_and(|o| o.speed_type == crate::rules::locomotor_type::SpeedType::Track)
            // No NavCom (`+0x5A4`, `0x00736A38`) and the locomotor's Is_Moving
            // (ILocomotion+0x10, `0x00736A5F`) false. A pending order is
            // neither; a locomotor the query does not answer reads as still.
            && entity.navigation.nav_com().is_none()
            && super::motion_query::is_moving(entity) != Some(true)
            && entity.body_facing_current(binary_frame) == tgt
        {
            out.hull_destination = Some(tgt);
        }
    }

    // Native tests Type+0xD21 after arm A and directly jumps to the spin Set,
    // bypassing the ordinary latch and idle-return path. The second Set must
    // remain in the writer so it samples Current after arm A's optional Set.
    if has_turret && obj.is_some_and(|object| object.turret_spins) {
        out.turret_spins = true;
        return out;
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
        if !committed_to_an_arc && entity.attack_target.target().is_none() {
            let dwell =
                i64::from(rules.general.guard_area_targeting_delay) + NATIVE_IDLE_TURRET_DWELL_BIAS;
            let dwell_elapsed = i64::from(binary_frame) - entity.last_fire_frame.frame() >= dwell;
            // `+0x2E4` — a tank riding a Battle Bunker holds its aim forever,
            // as does a deployed simple deployer (`0x00736B60..0x00736B78`).
            let bunkered = entity.bunker.link().installed_in().is_some();
            let deployed_simple_deployer =
                obj.is_some_and(|obj| obj.is_simple_deployer) && entity.is_fully_deployed();
            // Facing_Update736BA3..736BB9 keeps the last aim while a
            // Magnetron's +6AD latch and NavCom are both set. This is the
            // lift owner's byte, not generic locomotor piggyback state;
            // Imbue710326/710352 sets the destination and latch, and
            // Jumpjet End_Piggyback54DA50 clears the latch on landing.
            // Native controls: turret_cadence.json idle_navcom_magnetron.
            let lifted_with_destination =
                entity.lift.swap_active() && entity.navigation.nav_com().is_some();
            if dwell_elapsed && !bunkered && !deployed_simple_deployer && !lifted_with_destination {
                out.turret_destination = Some(match nav_destination_facing(entity, world) {
                    Some(nav) => nav,
                    None => entity.body_facing_current(binary_frame),
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

/// Compute the `TurretSpins` destination from the barrel's animated Current
/// value after arm A's optional Set. Native instructions at
/// `UnitClass::Facing_Update @ 0x00736AB7`..`0x00736ACB` shift, increment,
/// mask to one byte, add 8, then shift into the high byte before calling the
/// shared `FacingClass::Set @ 0x004C9220` at `0x00736BDD`.
pub(crate) fn turret_spin_destination(current: u16) -> u16 {
    u16::from(crate::util::direction_tables::round_facing16_to_8(current))
        .wrapping_add(8)
        .wrapping_shl(8)
}

/// The `+5` the idle-return comparison adds to `GuardAreaTargetingDelay`
/// (`ADD EDX,0x5` at `0x00736B4B`), giving 41 frames in stock.
const NATIVE_IDLE_TURRET_DWELL_BIAS: i64 = 5;

/// The idle turret's aim when a move order is live: the NavCom (`+0x5A4`,
/// read at `0x00736BA3`) through `DirectionToTarget @ 0x005F3DB0`
/// (`0x00736BC3`), the helper arm A aims with.
fn nav_destination_facing(
    entity: &GameEntity,
    world: &crate::sim::world::Simulation,
) -> Option<u16> {
    let nav_com = entity.navigation.nav_com()?;
    facing_toward_target(entity, &nav_com.into(), world)
}

/// Whether this object's currently selected weapon sets `OmniFire=`. Native
/// takes the slot through `TechnoClass` vtable `+0x3F4` (`0x0070E1A0` —
/// `GetWeapon(TurretCount(+0x808) > 0 ? CurrentWeaponNumber(+0x138) : 0)`) and
/// reads `WeaponType+0x12B`. An omni weapon is skipped by both the aim arm
/// (`0x007369F4`) and the fire gate's facing test (`0x0074125C`) — it shoots in
/// any direction and never turns the turret.
pub(crate) fn current_weapon_is_omni_fire(
    entity: &GameEntity,
    entities: &EntityStore,
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    let Some(obj) = interner.object_type(entity.type_ref(), rules) else {
        return false;
    };
    crate::sim::combat::combat_weapon::current_weapon(entity, obj, entities, rules, interner)
        .is_some_and(|weapon| weapon.omni_fire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gunner_selection_controls_omni_fire_before_the_first_shot_and_after_reset() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::combat::TargetKind;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=FV\n[InfantryTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [FV]\nTurret=yes\nTurretCount=4\nGunner=yes\nWeaponCount=3\n\
             NormalTurretWeapon=0\nMachineGunTurretWeapon=2\n\
             Weapon1=Direct\nWeapon2=Direct\nWeapon3=Omni\n\
             [Direct]\nDamage=10\nROF=20\nRange=6\nWarhead=WH\n\
             [Omni]\nDamage=10\nROF=20\nRange=6\nWarhead=WH\nOmniFire=yes\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
        let object = rules.object("FV").unwrap();
        let mut entity = GameEntity::test_default(1, "FV", "Americans", 5, 5);
        entity.barrel_facing = Some(crate::sim::movement::FacingClass::new(0, 5));
        entity
            .attack_target
            .set_for_test(Some(TargetKind::Cell(10, 5)));
        let mut sim = crate::sim::world::Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        for (mode, turns) in [(0, true), (2, false), (0, true)] {
            entity.set_gunner_weapon(mode, object);
            assert_eq!(
                facing_update(&entity, &sim, &rules, &sim.interner, 100)
                    .turret_destination
                    .is_some(),
                turns,
                "selected mode {mode} must govern facing without a previous shot",
            );
        }
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
}
