//! Complete accepted cell arrivals for nontrack ground crossings.
//!
//! VERA-internal ownership boundary, gamemd equivalent UNCHECKED. Preserve the
//! represented crossing order: fresh serialized list stamp, list relink,
//! Drive current occupation, arrival claim and matching Infantry list repair.
//! Geometry, path advancement, bridge rendering and look-ahead placement remain
//! in their existing caller phases. Arrival consumes no RNG: WalkLocomotion
//! ProcessMovement @ 0x0075BE0A reaches the NullCoord FindSubCellDest branch
//! @ 0x0075C240; see the detailed arrival evidence beside the private claim.

use crate::map::entities::EntityCategory;
use crate::sim::components::{DriveLocomotionRuntime, Position};
use crate::sim::movement::bump_crush;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::occupancy::{CellListInsertion, CellOccupationGrid, OccupancyGrid};
use crate::sim::world::EnterOrderCounter;

use super::MovementTickStats;

/// Borrow only the projections an accepted arrival must update together.
/// Position and old/new list layers have already been resolved by the caller;
/// the path layer is separate because bridge predicates need not agree with it.
pub(super) struct CellArrival<'a> {
    pub entity_id: u64,
    pub category: EntityCategory,
    pub from: (u16, u16),
    pub to: (u16, u16),
    pub old_list_layer: MovementLayer,
    pub new_list_layer: MovementLayer,
    pub position: &'a Position,
    pub locomotor: &'a mut Option<LocomotorState>,
    pub drive_locomotion: &'a mut Option<DriveLocomotionRuntime>,
    pub foot_occupation_enabled: &'a mut bool,
    pub sub_cell: &'a mut Option<u8>,
    pub occupancy_enter_order: &'a mut u64,
    pub next_occupancy_enter_order: &'a mut EnterOrderCounter,
    pub occupancy: &'a mut OccupancyGrid,
    pub cell_occupation: &'a mut CellOccupationGrid,
    pub stats: &'a mut MovementTickStats,
    pub priority: bool,
}

impl CellArrival<'_> {
    /// Ordinary crossings commit their path layer after current occupation.
    pub(super) fn ordinary(mut self, next_layer: MovementLayer) {
        self.relink();
        if let Some(loco) = self.locomotor.as_mut() {
            loco.layer = next_layer;
        }
        self.finish(next_layer);
    }

    fn relink(&mut self) {
        *self.occupancy_enter_order = self.next_occupancy_enter_order.next();
        self.occupancy.move_entity_layered(
            self.from.0,
            self.from.1,
            self.to.0,
            self.to.1,
            self.entity_id,
            self.old_list_layer,
            self.new_list_layer,
            *self.sub_cell,
            CellListInsertion::from_category(self.category),
        );
        if self.category == EntityCategory::Unit
            && let Some(drive) = self.drive_locomotion.as_mut()
        {
            crate::sim::occupancy::mark_current_drive_occupation_after_crossing(
                self.foot_occupation_enabled,
                drive,
                self.cell_occupation,
                self.entity_id,
                self.to,
                self.new_list_layer,
            );
        }
    }

    fn finish(self, path_layer: MovementLayer) {
        reserve_destination_after_transition(
            self.category,
            self.entity_id,
            self.locomotor,
            self.position,
            self.sub_cell,
            path_layer,
            self.to.0,
            self.to.1,
            self.occupancy,
            self.priority,
        );
        // Claim and list correction are one operation: callers cannot publish
        // a new Infantry sub_cell while leaving its cell-list entry stale.
        if self.category == EntityCategory::Infantry {
            self.occupancy
                .update_sub_cell(self.to.0, self.to.1, self.entity_id, *self.sub_cell);
        }
        self.stats.moved_steps = self.stats.moved_steps.saturating_add(1);
    }
}

/// Commit the arrival slot. Infallible, like the native arrival branch — the
/// old `bool` return existed only for the failure path removed below.
fn reserve_destination_after_transition(
    category: EntityCategory,
    entity_id: u64,
    locomotor: &mut Option<LocomotorState>,
    position: &Position,
    sub_cell: &mut Option<u8>,
    next_layer: MovementLayer,
    nx: u16,
    ny: u16,
    occupancy: &OccupancyGrid,
    priority: bool,
) {
    if category == EntityCategory::Infantry {
        // The slot the look-ahead reserved for this cell, recovered from the
        // lepton destination it stored on the locomotor. Functional by
        // construction, which is what makes it a usable arrival fallback.
        let preferred = locomotor
            .as_ref()
            .and_then(|loco| loco.subcell_dest)
            .and_then(bump_crush::functional_sub_cell_from_offset);
        // Priority placement bypasses every occupancy and blocker gate, exactly
        // as the original engine's priority branch does.
        let claimed = if priority {
            Some(bump_crush::priority_sub_cell(
                position.sub_x,
                position.sub_y,
            ))
        } else {
            bump_crush::claim_reserved_sub_cell(
                occupancy.get(nx, ny),
                next_layer,
                entity_id,
                preferred,
            )
        };
        // **Arrival cannot fail.** `WalkLocomotionClass::ProcessMovement` @
        // `0x0075BE0A` hands `FindSubCellDest` @ `0x0075C240` a NullCoord; that
        // stores it into `+0x28..0x30` and jumps to `LAB_0075C5C5`, which
        // reloads the infantryman's own `+0x9C` coordinate and marks it through
        // Infantry `vtable+0xF0` before `XOR AL,AL; RET 4`. The caller never
        // reads the result — `0x0075BE1D` goes straight on to `[+0x5E0]`. So the
        // slot is *derived from the man's own leptons*, nothing is scanned, and
        // there is no refusal.
        //
        // VERA moves the entity in the occupancy grid before this runs and then
        // picks a slot, so `claim_reserved_sub_cell` can come back empty when a
        // vehicle settles on the cell or three other infantry hold the
        // functional slots. Falling back to the man's own lepton offset is the
        // native answer to exactly that: it keeps the arrival total, as
        // `LAB_0075C5C5` does. VERA falls back to the look-ahead's own slot
        // (functional by construction) and, failing that, the first
        // functional slot — a VERA-internal choice of *which* slot, since the
        // native derives it from the man's leptons rather than from a stored
        // reservation. gamemd equivalent of the ordering UNCHECKED.
        //
        // The previous behaviour — snap to cell centre, drop the drive track and
        // return `false` — broke the crossing loop before
        // `configure_motion_after_transition` could advance `next_index`, so the
        // next tick re-read the cell the man was already standing in, re-ran the
        // transition and failed again, with no `aborted_for_stuck` and no route
        // through `movement_blocked`. That is a permanent freeze until the
        // contention clears on its own, and the cell centre it snapped to is a
        // position the ordinary chooser can never assign.
        let sub = claimed
            .or(preferred)
            .unwrap_or(bump_crush::FUNCTIONAL_SUB_CELLS[0]);
        *sub_cell = Some(sub);
        if let Some(loco) = locomotor {
            let (dest_x, dest_y) = crate::util::lepton::subcell_lepton_offset(Some(sub));
            loco.subcell_dest = Some((dest_x, dest_y));
        }
    } else {
        if let Some(loco) = locomotor {
            loco.subcell_dest = Some((
                crate::util::lepton::CELL_CENTER_LEPTON,
                crate::util::lepton::CELL_CENTER_LEPTON,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::Simulation;

    #[test]
    fn restored_retained_list_drives_acquisition_arrival_and_removal() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n1=SCOUT\n2=PRIZE\n\
             [TANK]\nStrength=300\nArmor=heavy\nPrimary=Gun\n\
             [SCOUT]\nStrength=300\nArmor=heavy\n\
             [PRIZE]\nStrength=300\nArmor=heavy\nSpecialThreatValue=10\n\
             [Gun]\nDamage=100\nROF=110\nRange=5\nWarhead=AP\nProjectile=Bullet\n\
             [Bullet]\nAA=yes\nAG=yes\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .unwrap();
        let mut sim = Simulation::with_seed(20);
        for (type_name, owner, cell) in [
            ("TANK", "Americans", (5, 5)),
            ("SCOUT", "Soviet", (6, 5)),
            ("PRIZE", "Soviet", (6, 5)),
            ("SCOUT", "Americans", (8, 5)),
            ("SCOUT", "Americans", (9, 5)),
        ] {
            let id = sim.allocate_stable_id();
            let mut entity = GameEntity::test_default(id, type_name, owner, cell.0, cell.1);
            entity.lifecycle.in_limbo = false;
            sim.substrate.entities.insert(entity);
            sim.add_entity_occupancy(id);
        }
        sim.interner = crate::sim::intern::test_interner();
        sim.remove_entity_occupancy(2);
        sim.add_entity_occupancy(2);
        // Supply the already retained state at this storage boundary. Original
        // initial Jumpjet Process can leave a ground Cell list after phase0->1;
        // this test does not pretend to produce that native phase transition.
        // Both enemies remain outside the old phase-derived list projection.
        for id in [2, 3] {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            let mut loco = LocomotorState::for_test_kind(LocomotorKind::Jumpjet);
            loco.layer = MovementLayer::Air;
            entity.locomotor = Some(loco);
        }
        // Independent AirTracker registrations/order are saved as their own
        // authority; they neither remove nor manufacture a ground list.
        for id in [5, 4] {
            let order = sim.substrate.next_occupancy_enter_order.next();
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.air_spatial_bucket = Some(7);
            entity.air_spatial_enter_order = order;
        }
        let air_order = |sim: &Simulation| {
            let mut rows: Vec<_> = sim
                .substrate
                .entities
                .values()
                .filter_map(|e| {
                    e.air_spatial_bucket
                        .map(|bucket| (e.air_spatial_enter_order, e.stable_id(), bucket))
                })
                .collect();
            rows.sort_unstable();
            rows
        };
        let list = |sim: &Simulation, cell: (u16, u16)| -> Vec<u64> {
            sim.substrate
                .occupancy
                .get(cell.0, cell.1)
                .map(|list| {
                    list.iter_layer(MovementLayer::Ground)
                        .map(|o| o.entity_id)
                        .collect()
                })
                .unwrap_or_default()
        };
        let acquire = |sim: &Simulation, occupancy: &OccupancyGrid| {
            crate::sim::combat::acquire_best_target_for_entity(
                &sim.substrate.entities,
                occupancy,
                &rules,
                &sim.interner,
                1,
                None,
                None,
                false,
                crate::sim::combat::ScanMission::Guard,
                None,
                crate::sim::combat::line_of_fire::LineOfFireInputs::default(),
            )
        };
        assert_eq!(list(&sim, (6, 5)), vec![2, 3]);
        assert_eq!(acquire(&sim, &sim.substrate.occupancy), Some(2));
        assert_eq!(
            acquire(&sim, &OccupancyGrid::rebuild(&sim.substrate.entities)),
            None
        );
        let expected_air = air_order(&sim);
        assert_eq!(
            expected_air.iter().map(|row| row.1).collect::<Vec<_>>(),
            vec![5, 4]
        );
        // Snapshot deserialization intentionally executes native Scenario Seed0.
        // Compare this state-only roundtrip on that same admitted RNG cursor.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let hash = sim.state_hash();
        let saved = GameSnapshot::save(&sim, 0, 0, "retained Cell membership", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(restored.state_hash(), hash);
        assert_eq!(list(&restored, (6, 5)), vec![2, 3]);
        assert_eq!(air_order(&restored), expected_air);
        for id in [2, 3] {
            assert_eq!(
                crate::sim::occupancy::cell_list_layer_for_entity(
                    restored.substrate.entities.get(id).unwrap()
                ),
                None
            );
        }
        assert_eq!(acquire(&restored, &restored.substrate.occupancy), Some(2));
        // Enter the existing production accepted-arrival owner with its
        // caller-resolved old/new Cell lists; no destination/path admission is
        // bypassed under a claim of complete Jumpjet movement here.
        let old_layer = restored
            .substrate
            .occupancy
            .get(6, 5)
            .unwrap()
            .occupants
            .iter()
            .find(|o| o.entity_id == 2)
            .unwrap()
            .layer;
        let mut stats = MovementTickStats::default();
        let substrate = &mut restored.substrate;
        let entity = substrate.entities.get_mut(2).unwrap();
        entity.position.rx = 7;
        CellArrival {
            entity_id: 2,
            category: entity.category,
            from: (6, 5),
            to: (7, 5),
            old_list_layer: old_layer,
            new_list_layer: MovementLayer::Ground,
            position: &entity.position,
            locomotor: &mut entity.locomotor,
            drive_locomotion: &mut entity.drive_locomotion,
            foot_occupation_enabled: &mut entity.foot_occupation_enabled,
            sub_cell: &mut entity.sub_cell,
            occupancy_enter_order: &mut entity.occupancy_enter_order,
            next_occupancy_enter_order: &mut substrate.next_occupancy_enter_order,
            occupancy: &mut substrate.occupancy,
            cell_occupation: &mut substrate.cell_occupation,
            stats: &mut stats,
            priority: false,
        }
        .ordinary(MovementLayer::Ground);
        assert_eq!(stats.moved_steps, 1);
        assert_eq!(list(&restored, (6, 5)), vec![3]);
        assert_eq!(list(&restored, (7, 5)), vec![2]);
        restored.object_conceal(2);
        assert_eq!(list(&restored, (7, 5)), Vec::<u64>::new());
        assert_eq!(list(&restored, (6, 5)), vec![3]);
        assert_eq!(air_order(&restored), expected_air);
        assert_eq!(acquire(&restored, &restored.substrate.occupancy), Some(3));
    }
}
