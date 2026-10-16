//! The write half of UnitClass's live post-Foot Facing slot.
//!
//! `combat::world_fire::visit_fire` reads and writes this slot
//! immediately after the same Unit's Fire_At_Target (`7365E1`, `7365E8`),
//! before the second Ready/Commence checkpoint and the next Logic object.
//! The current target and selected weapon are read after FireAt's mutations;
//! a Bullet it created has not yet reached its later live Logic slot.
//!
//! Facing math stays in `sim/movement/turret`; this owner commits the hull,
//! turret and rotation-latch writes in native order. SpawnManager is a
//! separate TechnoClass AI_Update consumer, not a Unit post-Foot slot.

use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;

/// Commit one or more Unit Facing slots at their caller's object boundary.
/// `FacingClass::set` is a no-op when the destination already matches.
/// Signed ROT is refreshed from rules for each apply.
///
/// The facing writes land here in native order:
/// 1. the hull destination from `UnitClass::Fire_At_Target @ 0x00736DF0` case 2
///    (`FacingClass::Set(+0x388)` at `0x00737004`, then the hull's raw
///    destination copied into the turret slot at `0x0073701C` — `FUN_004C9470`
///    returns the DESTINATION dword, not the animated value). Native runs the
///    whole of `Fire_At_Target` before `Facing_Update`
///    (`0x007365E1`/`0x007365E8`), so this precedes everything below;
/// 2. `Facing_Update` arm A's aim `Set` on the turret, `0x00736A89`
///    (`None` = native calls no `Set`, so the turret holds its aim);
/// 3. when `TurretSpins` is set (`Type+0xD21`, tested at `0x00736A95`), its
///    arm-B `Current`/formula/Set runs at `0x00736AAC`..`0x00736BDD`, after
///    arm A. This branch bypasses both writes to `+0x6AF`, so its previous
///    latch value is retained;
/// 4. on the ordinary path, the `+0x6AF` rotation latch —
///    `MOV [ESI+0x6AF],AL` at `0x00736B16`, the
///    value being `FacingClass::Is_Rotating(+0x3A0) @ 0x004C9480` called at
///    `0x00736B11`, with the unconditional clear at `0x00736AD5` covering the
///    turretless and not-rotating cases. Read next tick by
///    `UnitClass::GetFireError @ 0x00741233`;
/// 5. on the ordinary path, arm B's idle-return `Set` on the turret,
///    `0x00736BDD`.
///
/// The ordinary path's steps 4 and 5 are in that order in the binary, and it matters:
/// `FacingClass::Set @ 0x004C9220` writes `duration = abs(delta)/rate` and
/// `start = g_CurrentFrameCounter`, so any `Set` of one or more `ROT=` steps
/// makes `Is_Rotating` true immediately. An aim `Set` therefore arms the latch
/// on the frame it is issued — which is what keeps the fire gate refusing for
/// the whole arc, including its first one or two frames — while an idle-return
/// `Set` leaves the latch clear, so a unit that re-acquires a target during an
/// idle swing-back is free to aim at it on the next frame.
pub(crate) fn apply_unit_facing(
    entities: &mut EntityStore,
    updates: &[crate::sim::combat::UnitFacingUpdate],
    rules: &RuleSet,
    interner: &StringInterner,
    binary_frame: u32,
) {
    for update in updates {
        let id = update.entity_id;
        let Some(entity) = entities.get(id) else {
            continue;
        };
        let rot = interner
            .object_type(entity.type_ref(), rules)
            .map(|obj| obj.turret_rot)
            .unwrap_or(5);
        let before = UnitFacing::of(entity);
        let mut after = before;
        after.apply(update, rot, binary_frame);
        // Most units hold their aim, so their `Set`s repeat the destination
        // and write nothing: hand a unit out only when a field changes.
        if after != before {
            let entity = entities
                .get_mut(id)
                .expect("a facing update's unit was just read");
            let initial_latch = entity.turret_rotation_latch.is_set();
            let latch = apply_steps(
                &mut entity.body_facing,
                entity.barrel_facing.as_mut(),
                update,
                initial_latch,
                rot,
                binary_frame,
            );
            entity.turret_rotation_latch.rotating = latch;
        }
    }
}

/// The turret rotation latch, `UnitClass+0x6AF`, written only by
/// `UnitClass::Facing_Update @ 0x00736990` (cleared at `0x00736AD5`, re-set
/// from `FacingClass::Is_Rotating` at `0x00736B16`): the Facing slot's store
/// below. Its readers include the transport unload (`0x0073D892`), the radio
/// receivers and Scatter's coordinate arm (`0x00743CFD`), besides
/// `UnitClass::GetFireError @ 0x00741233`. While set, the aim block is
/// skipped so the turret finishes the arc it started instead of
/// re-snapshotting every frame, and — unless the projectile homes — the fire
/// gate answers FIRE_ROTATING(4) at `0x00741250`, refusing the shot until the
/// arc completes.
///
/// Not folded into `world_hash`: it is last tick's `barrel_facing`
/// `Is_Rotating`, which the hash already folds, so a divergence surfaces one
/// tick later through the interpolator itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct TurretRotationLatch {
    rotating: bool,
}

impl TurretRotationLatch {
    /// Set: the turret is finishing the arc it started.
    pub(crate) const fn is_set(self) -> bool {
        self.rotating
    }

    /// Install a fixture's latch.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_for_test(&mut self, rotating: bool) {
        self.rotating = rotating;
    }
}

/// The fields the Facing slot writes, stepped on a copy.
#[derive(Clone, Copy, PartialEq)]
struct UnitFacing {
    /// The hull, `+0x388`.
    hull: crate::sim::movement::FacingClass,
    /// The turret, `+0x3A0`.
    barrel: Option<crate::sim::movement::FacingClass>,
    /// The `+0x6AF` rotation latch.
    latch: bool,
}

impl UnitFacing {
    /// Reads the hull `+0x388`, the turret `+0x3A0` and the `+0x6AF` latch.
    fn of(entity: &GameEntity) -> Self {
        Self {
            hull: entity.body_facing,
            barrel: entity.barrel_facing,
            latch: entity.turret_rotation_latch.is_set(),
        }
    }

    /// Steps 1–5 in binary order, on the copy.
    fn apply(
        &mut self,
        update: &crate::sim::combat::UnitFacingUpdate,
        rot: i32,
        binary_frame: u32,
    ) {
        self.latch = apply_steps(
            &mut self.hull,
            self.barrel.as_mut(),
            update,
            self.latch,
            rot,
            binary_frame,
        );
    }
}

/// Applies a Unit's facing writes in binary order on its hull and turret, through
/// `FacingClass::Set`; returns the `+0x6AF` latch.
fn apply_steps(
    hull: &mut crate::sim::movement::FacingClass,
    mut barrel: Option<&mut crate::sim::movement::FacingClass>,
    update: &crate::sim::combat::UnitFacingUpdate,
    initial_latch: bool,
    rot: i32,
    binary_frame: u32,
) -> bool {
    // 1. `Fire_At_Target` case 2's hull turn, which native completes before
    //    `Facing_Update` is entered at all (`0x007365E1`/`0x007365E8`).
    if let Some(desired) = update.hull_destination {
        hull.set(desired, binary_frame);
        let raw_destination = hull.destination();
        if let Some(barrel) = barrel.as_deref_mut() {
            barrel.set_rot(rot);
            barrel.set(raw_destination, binary_frame);
        }
    }
    // 2. arm A's aim `Set` (`0x00736A89`) — before the latch store.
    if let Some(desired) = update.turret_destination
        && !update.turret_destination_is_idle_return
        && let Some(barrel) = barrel.as_deref_mut()
    {
        barrel.set_rot(rot);
        barrel.set(desired, binary_frame);
    }
    // 3. The native `TurretSpins` arm follows arm A and bypasses both ordinary
    //    latch writes (`0x00736AD5`, `0x00736B16`) and idle return. `Current`
    //    is sampled after arm A's Set, then the same shared FacingClass::Set
    //    is called at `0x00736BDD`.
    if update.turret_spins {
        if let Some(barrel) = barrel.as_deref_mut() {
            let current = barrel.current(binary_frame);
            let destination = crate::sim::movement::turret::turret_spin_destination(current);
            barrel.set_rot(rot);
            barrel.set(destination, binary_frame);
        }
        return initial_latch;
    }
    // 4. the `+0x6AF` store (`0x00736B16`), evaluated on the barrel as it
    //    stands after the writes above. `is_some_and` reproduces the
    //    `0x00736AD5` clear for a turretless unit, which native leaves at 0
    //    because the `Type+0xCA1` test at `0x00736ADC` sends it past both
    //    `Is_Rotating` calls.
    let latch = barrel
        .as_deref()
        .is_some_and(|barrel| barrel.is_rotating(binary_frame));
    // 5. arm B's idle-return `Set` (`0x00736BDD`) — after the latch store,
    //    so the swing-back arc does not arm it.
    if let Some(desired) = update.turret_destination
        && update.turret_destination_is_idle_return
        && let Some(barrel) = barrel
    {
        barrel.set_rot(rot);
        barrel.set(desired, binary_frame);
    }
    latch
}
