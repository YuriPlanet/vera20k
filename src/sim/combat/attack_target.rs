//! What an attacker holds as its target ([`TarCom`], [`TargetKind`])
//! and the coordinates and distances read from it.

use super::*;

/// What a [`TarCom`] points at: an entity or a ground cell.
///
/// Force-fire on empty terrain (Ctrl + click cell) sets the `Cell` variant.
/// Auto-acquired and explicit attack-on-unit orders set `Entity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum TargetKind {
    /// Entity-targeted attack (normal Attack / ForceAttack on a unit/building).
    Entity(u64),
    /// Ground-targeted attack (force-fire on a cell). Cell coord in map space.
    Cell(u16, u16),
}

impl From<TargetKind> for crate::sim::projectile::ProjectileTarget {
    fn from(target: TargetKind) -> Self {
        match target {
            TargetKind::Entity(id) => Self::Entity(id),
            TargetKind::Cell(rx, ry) => Self::Cell { rx, ry },
        }
    }
}

impl From<crate::sim::components::NavTargetRef> for TargetKind {
    /// A NavCom (`Foot+0x5A4`) as a target: its cell, or the object it names.
    fn from(target: crate::sim::components::NavTargetRef) -> Self {
        use crate::sim::components::NavTargetRef;
        match target {
            NavTargetRef::Cell { rx, ry } => Self::Cell(rx, ry),
            NavTargetRef::Entity { id }
            | NavTargetRef::Object { id }
            | NavTargetRef::Building { id } => Self::Entity(id),
        }
    }
}

impl From<TargetKind> for crate::sim::components::NavTargetRef {
    /// A target as a destination (a Target handed to `Assign_Destination`).
    fn from(target: TargetKind) -> Self {
        match target {
            TargetKind::Entity(id) => Self::Entity { id },
            TargetKind::Cell(rx, ry) => Self::cell(rx, ry),
        }
    }
}

impl TargetKind {
    /// Recover an already-retained native Cell allocation without a map lookup.
    /// A Cell TarCom is a pointer: later queries may have changed Dummy.coord,
    /// so reading the target must not restamp it with the command coordinate.
    /// GetFireError6FC197 and CanFireAt6F77B0 share this identity authority.
    pub(crate) fn cell_identity(
        self,
        terrain: &ResolvedTerrainGrid,
    ) -> Option<crate::map::cell_index::NativeCellIdentity> {
        let Self::Cell(x, y) = self else {
            return None;
        };
        Some(terrain.native_fixed_cell_index(x as i16, y as i16).map_or(
            crate::map::cell_index::NativeCellIdentity::Dummy,
            crate::map::cell_index::NativeCellIdentity::Real,
        ))
    }
}

/// `TechnoClass+0x2B4`, TarCom: the object or cell this Techno attacks, or
/// nothing (the constructor's NULL, `0x006F2E39`).
///
/// The class target setter (`TechnoClass::Assign_Target @ 0x006FCDB0` and its
/// class overrides) writes it; `sim::mission::concrete_effects` represents
/// that setter. The combat system fires the attacker's weapon at the held
/// target; the reload between shots is the object's own
/// `GameEntity::rearm_timer`, not the target's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TarCom {
    target: Option<TargetKind>,
}

impl TarCom {
    /// The held target: an object by stable id, or a cell.
    pub const fn target(self) -> Option<TargetKind> {
        self.target
    }

    /// Store `target`. The writers:
    /// - the class setter's store (`0x006FCF3E`), after its same-target
    ///   early-out and admission (`represented_assign_target_admitted`);
    /// - `FootClass::Arrival_Target_Handler @ 0x004DF3A0`, which stores NULL
    ///   and its MegaTarget without the setter, so the passive flag, burst
    ///   index and SpawnManager keep their state.
    ///
    /// An object that dies or is UnInit is dropped by its attackers:
    /// PointerExpired's TarCom arm (`0x007077C0`) calls the class setter with
    /// NULL (`0x00707A33`) in the killing hit's announcement
    /// (`Simulation::object_destroy_callback`) and in UnInit's. An
    /// infantryman deployed without `DeployFire=` refuses that write
    /// (`0x0051B273..0x0051B299`) and keeps the target.
    pub(crate) fn set(&mut self, target: Option<TargetKind>) {
        self.target = target;
    }

    /// Install a fixture's target without the setter's effects.
    #[cfg(test)]
    pub(crate) fn set_for_test(&mut self, target: Option<TargetKind>) {
        self.target = target;
    }
}

/// An entity's GetCoords XY ([`object_center_xy`]) as a cell and in-cell
/// leptons: a structure's foundation centre, any other object's Location.
/// Callers which consume stored Location rather than this virtual point keep
/// that distinction.
///
/// [`object_center_xy`]: crate::sim::movement::ground_pose::object_center_xy
pub(super) fn target_coords(entity: &GameEntity) -> (u16, u16, SimFixed, SimFixed) {
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
    (
        (x / 256) as u16,
        (y / 256) as u16,
        SimFixed::from_num(x % 256),
        SimFixed::from_num(y % 256),
    )
}

/// Compute lepton-precise coordinates for a cell target (force-fire on terrain).
///
/// Cell-center convention: leptons = `cell_index * 256 + 128`. Returns the
/// shape `target_coords` returns for entities (rx, ry, sub_x, sub_y) so
/// callers can branch on `TargetKind` and feed the result into the same
/// projectile-spawn pipeline.
pub(super) fn cell_center_coords(rx: u16, ry: u16) -> (u16, u16, SimFixed, SimFixed) {
    (rx, ry, SimFixed::from_num(128), SimFixed::from_num(128))
}

/// The native Target pointer (`+0x2B4`) is non-NULL: a cell, or an object
/// not yet dead. Aircraft Mission_Attack's states, the aircraft idle decision
/// and the Infantry fire tail read it through this. Natively the killing
/// hit's Destroy = `Detach_All(1)` (`ObjectClass::ReceiveDamage
/// 0x005F5765..0x005F57AF`) announces the expiry, so no Target names a dying
/// object; VERA can keep a dying infantryman's id until its death sequence
/// ends.
pub(crate) fn target_present(
    target: Option<TargetKind>,
    entities: &crate::sim::entity_store::EntityStore,
) -> bool {
    match target {
        None => false,
        Some(TargetKind::Cell(..)) => true,
        Some(TargetKind::Entity(id)) => entities
            .get(id)
            .is_some_and(|target| !target.death.is_dying() && target.health.current() > 0),
    }
}

/// Resolve target coords from a `TargetKind`, looking up entity position when
/// needed and using cell-center for `Cell` targets.
///
/// Returns `None` if the target is `Entity(id)` and the entity no longer
/// exists (despawned). `Cell` targets always resolve.
///
/// Read by the combat tick, the fire coordinate and the Temporal weapon.
pub(crate) fn resolve_target_coords(
    target: &TargetKind,
    entities: &EntityStore,
) -> Option<(u16, u16, SimFixed, SimFixed)> {
    match *target {
        TargetKind::Entity(id) => entities.get(id).map(target_coords),
        TargetKind::Cell(rx, ry) => Some(cell_center_coords(rx, ry)),
    }
}

/// ObjectClass::Distance_To5F6440: planar GetCoords distance, then the target
/// building's (foundation width + height)*64 discount, clamped to zero. This
/// differs from the weapon CanFireAt/InRange gate and has no altitude bonus.
/// Reuse the coordinate projection and deterministic native sqrt owner: exact
/// integer sqrt changes observable lepton ties (1281 becomes1280 natively).
/// Native comparisons: tools/spatial_oracle/aircraft_approach_range.{py,json}.
/// The discount reads the same stamped foundation as the target's GetCoords.
pub(crate) fn object_distance_to(
    source: &GameEntity,
    target: &TargetKind,
    entities: &EntityStore,
) -> Option<i32> {
    let planar = |(rx, ry, sx, sy): (u16, u16, SimFixed, SimFixed)| {
        [
            i32::from(rx) * 256 + sx.to_num::<i32>(),
            i32::from(ry) * 256 + sy.to_num::<i32>(),
            0,
        ]
    };
    let from = planar(target_coords(source));
    let to = planar(resolve_target_coords(target, entities)?);
    let distance = crate::util::native_x87::distance_3d_leptons(from, to);
    if let TargetKind::Entity(id) = *target
        && let Some(building) = entities.get(id)
        && building.category() == EntityCategory::Structure
    {
        let (width, height) = foundation_dimensions(building.type_facts.foundation());
        // Height query45ECA0 receives false: Bib never adds to this discount.
        return Some(
            distance
                .wrapping_sub((i32::from(width) + i32::from(height)) * 64)
                .max(0),
        );
    }
    Some(distance)
}

/// Install a bare combat fixture's entity target. Production commands use
/// `Simulation::assign_target_represented`, including concrete class effects.
/// This adapter stages the admitted Techno base state without running an event.
#[cfg(test)]
pub(crate) fn install_entity_attack_target_for_test(
    entities: &mut EntityStore,
    attacker_id: u64,
    target_id: u64,
) -> bool {
    if entities.get(target_id).is_none() {
        return false;
    }
    let attacker = match entities.get_mut(attacker_id) {
        Some(a) => a,
        None => return false,
    };

    crate::sim::mission::concrete_effects::represented_assign_target_admitted(
        attacker,
        Some(TargetKind::Entity(target_id)),
        true,
    );

    true
}

/// Install a bare combat fixture's cell target. Keep its current-weapon gate
/// through the shared weapon reader; production ForceAttackCell uses the
/// concrete class authority in `world_commands`, not this setup adapter.
#[cfg(test)]
pub(crate) fn install_cell_attack_target_for_test(
    entities: &mut EntityStore,
    attacker_id: u64,
    target_rx: u16,
    target_ry: u16,
    rules: Option<&RuleSet>,
    interner: &StringInterner,
) -> bool {
    // Read weapon presence before the mutable borrow.
    let has_weapon = match entities.get(attacker_id) {
        Some(a) => rules
            .and_then(|r| interner.object_type(a.type_ref(), r))
            .is_some_and(|obj| combat_weapon::is_armed(a, obj)),
        None => return false,
    };

    if !has_weapon {
        // Defensive: client-side filter should have routed this to Move.
        // Warn-log so the desync is visible rather than silent.
        log::warn!(
            "ForceAttackCell rejected for unarmed attacker {} (target cell {},{})",
            attacker_id,
            target_rx,
            target_ry
        );
        return false;
    }

    let attacker = match entities.get_mut(attacker_id) {
        Some(a) => a,
        None => return false,
    };

    crate::sim::mission::concrete_effects::represented_assign_target(
        attacker,
        Some(TargetKind::Cell(target_rx, target_ry)),
    );
    true
}

pub(super) fn attack_world_z_leptons(
    target: TargetKind,
    entities: &EntityStore,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> i32 {
    match target {
        TargetKind::Entity(entity_id) => entities
            .get(entity_id)
            .map(|entity| object_world_z_leptons(entity, terrain))
            .unwrap_or(0),
        // FireAt6FE1FF and Get_Led_Target_Coords70BCB0 read Cell+58;
        // Bullet::Fire468707 freezes that same unled aim. The flight/impact
        // owners separately resolve ground contact and the final detonation.
        TargetKind::Cell(rx, ry) => crate::sim::projectile::cell_target_coord(terrain, rx, ry).z,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The saved shape before [`TarCom`]: `Option<AttackTarget { target }>`.
    #[derive(serde::Serialize)]
    struct AttackTargetBefore {
        target: TargetKind,
    }

    #[test]
    fn tarcom_saves_the_bytes_of_the_optional_attack_target_it_replaced() {
        for target in [
            None,
            Some(TargetKind::Entity(7)),
            Some(TargetKind::Cell(1, 2)),
        ] {
            let before = target.map(|target| AttackTargetBefore { target });
            assert_eq!(
                bincode::serialize(&TarCom { target }).unwrap(),
                bincode::serialize(&before).unwrap(),
                "{target:?}"
            );
        }
    }
}
