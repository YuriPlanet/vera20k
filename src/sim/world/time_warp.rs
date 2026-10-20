//! Retained initial Techno records and House50D320/50D440 restoration (130/126).
//!
//! Original Steam gamemd.
//! Native producer/repair execution and original bodies: time-warp-records-plan
//! and time-warp-restore-placement-native in the local ALL01 evidence packet.
//! Creation/458A80 placement is instruction-backed, not a rendered parity claim.

use super::{FrameEffects, Simulation, UninitContext};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::object_type::ObjectCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags};
use crate::sim::components::DriveCoord;
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::movement::{ground_pose, locomotor::MovementLayer};
use crate::sim::world::ObjectSubstrate;

impl Simulation {
    /// Execute1186E2C20 -> Map588AC0. Clear footprint identities in the
    /// existing shared allocated iterator, then publish existing drawing
    /// notifications.4F42F0(1)/578AC0 invalidate native surface/blit caches;
    /// VERA rebuilds/uploads smudge instances every frame, including count0.
    /// No native redraw epoch becomes simulation/persistent/hash state.
    pub(crate) fn trigger_clear_all_smudges(&mut self, rules: &RuleSet) -> bool {
        if self.resolved_terrain.is_none() || !self.reset_allocated_map_cell_cursor() {
            return false;
        }
        let context = UninitContext::with_rules(rules);
        while let Some(source) = self.next_allocated_map_cell(context) {
            if let (Some(grid), Some(terrain)) =
                (self.smudge_grid.as_mut(), self.resolved_terrain.as_ref())
                && grid
                    .clear_footprint_identity_at(source, &rules.smudge_types, terrain)
                    .is_none()
            {
                log::debug!("Map588AC0 source has an unavailable SmudgeType");
                Self::flush_smudge_dirty(&mut self.smudge_grid, &mut self.dirty_cells);
                return false;
            }
        }
        Self::flush_smudge_dirty(&mut self.smudge_grid, &mut self.dirty_cells);
        true
    }

    /// Successful ReadScenario684C05..684C1B: reverse House array, then
    /// House50D610's reverse full Techno registry. No alive/limbo/class gate.
    /// Repeated calls append duplicates; saved-world restoration never calls it.
    pub(crate) fn retain_initial_house_technos(substrate: &ObjectSubstrate, houses: &mut Houses) {
        for house_index in (0..houses.order().len()).rev() {
            let owner = houses.order()[house_index];
            for index in (0..substrate.entities.techno_registry_len()).rev() {
                let Some(id) = substrate.entities.techno_registry_at(index) else {
                    continue;
                };
                let Some(entity) = substrate.entities.get(id) else {
                    continue;
                };
                if entity.owner() != owner {
                    continue;
                }
                // Actual virtual+1B8 is41BEA0 in all four Techno classes.
                // It truncates raw Location, not Building447AC0 GetCoords.
                let [x, y] = ground_pose::position_world_xy(&entity.position);
                let record = (
                    entity.type_ref(),
                    (
                        crate::util::lepton::lepton_to_cell_packed(x),
                        crate::util::lepton::lepton_to_cell_packed(y),
                    ),
                );
                if let Some(house) = houses.get_mut(&owner) {
                    house.retain_initial_techno(record.0, record.1);
                }
            }
        }
    }

    /// House50D440 (126/6E30C0) recreates all four Techno categories;
    /// House50D320 (130/6E3120) repairs/recreates only Buildings.
    /// Both use the same saved history, constructor and placement owners.
    /// Count is captured once; each slot/type/current resident is read anew
    /// after synchronous repair/create/lifecycle callbacks. Placement refusal
    /// is an ordinary native continuation, not failure of action admission.
    pub(crate) fn restore_initial_house_technos(
        &mut self,
        owner: InternedId,
        recreate_all: bool,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(house) = self.houses.get(&owner) else {
            return false;
        };
        let count = house.initial_techno_record_count();
        for index in (0..count).rev() {
            let Some((type_id, at)) = self
                .houses
                .get(&owner)
                .and_then(|house| house.initial_techno_record(index))
            else {
                continue;
            };
            let Some(object) = self.interner.object_type(type_id, rules) else {
                log::debug!("Initial-Techno record has an unavailable Type");
                return false;
            };
            //50D524/50D530 admits Type RTTI3/7/16/40, not just Units.
            // Native50D320 instead requires7 (BuildingType; instance RTTI6).
            let category = object.category;
            if !recreate_all && category != ObjectCategory::Building {
                continue;
            }
            let strength = object.strength;
            if !recreate_all {
                let Some(terrain) = self.resolved_terrain.as_ref() else {
                    return false;
                };
                let cell = NativeCellQuery::canonical(terrain).lookup(at);
                let resident = match cell {
                    NativeCellIdentity::Real(_) => {
                        let (x, y) = terrain.native_cell_coord(cell);
                        self.substrate.occupancy.first_building_on_layer(
                            x as u16,
                            y as u16,
                            MovementLayer::Ground,
                        )
                    }
                    // The modeled shared Dummy has no stored object-list members.
                    // Retail initial Building records are allocated real cells;
                    // nonempty native Dummy lists remain an unrepresented boundary.
                    NativeCellIdentity::Dummy => None,
                };
                if let Some(id) = resident.filter(|&id| {
                    self.substrate.entities.get(id).is_some_and(|entity| {
                        entity.type_ref() == type_id && entity.owner() == owner
                    })
                }) {
                    let Some(c4) = rules.warhead(&rules.bridge_warheads.c4_name) else {
                        return false;
                    };
                    let warhead_id = self.interner.intern(&c4.id);
                    self.commit_direct_damage_receiver(
                        rules,
                        overlay_registry,
                        EntityDamageEvent::direct_receiver(
                            id,
                            strength.wrapping_neg(),
                            0,
                            0,
                            None,
                            warhead_id,
                            ReceiverCallFlags {
                                ignore_defenses: false,
                                arg6: false,
                            },
                        ),
                        frame_effects,
                    );
                    continue;
                }
            }

            let type_name = self.interner.resolve(type_id).to_owned();
            let owner_name = self.interner.resolve(owner).to_owned();
            // Type+8C constructs at zero held Location.50D48C/50D49F
            // do not seek an existing resident or skip a still-present object.
            // For Buildings, Type+8C=45E880 constructs43B740.
            // Allocation/cast failure is unsafe natively at50D40F. Rust reports
            // unavailable construction rather than inventing a successful no-op.
            let Some(created) = self.construct_object_limbo_at_height(
                &type_name,
                &owner_name,
                0,
                0,
                0,
                0,
                rules,
                frame_effects,
            ) else {
                return false;
            };
            //50D3E3/50D4C9 query Map5657A0 after the actual constructor callbacks.
            let coord = self.resolved_terrain.as_ref().and_then(|terrain| {
                let cells = NativeCellQuery::canonical(terrain);
                crate::sim::cell_kernel::native_cell_own_coords(cells.lookup(at), &cells).map(
                    |(x, y, z)| DriveCoord {
                        x: x as i32,
                        y: y as i32,
                        z: z as i32,
                    },
                )
            });
            let Some(coord) = coord else {
                self.uninit_with_context(
                    created,
                    UninitContext::new(Some(rules), overlay_registry).with_effects(frame_effects),
                );
                return false;
            };
            //50D4F9/50D413 call +490, not ordinary Unlimbo:458A80
            // for Buildings,4DF510 for all three Foot classes.
            let placed = if category == ObjectCategory::Building {
                self.place_building_with_cleared_foundation(
                    created,
                    coord,
                    0,
                    rules,
                    overlay_registry,
                    frame_effects,
                )
            } else {
                self.place_foot_with_cleared_cell(
                    created,
                    coord,
                    0,
                    rules,
                    overlay_registry,
                    frame_effects,
                )
            };
            if placed.is_none() {
                //50D421/50D507 call F8 even after the placement method
                // has already uninitialized a failed new object. Preserve
                // the shared owner's repeated lifecycle/queue semantics.
                self.uninit_with_context(
                    created,
                    UninitContext::new(Some(rules), overlay_registry).with_effects(frame_effects),
                );
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::house_state::HouseState;
    use crate::sim::snapshot::GameSnapshot;

    #[test]
    fn initial_records_keep_raw_cell_reverse_order_duplicates_and_saved_history() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=NANRCT\n[NANRCT]\nStrength=1000\nFoundation=4x4\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Confederation");
        let foreign = sim.interner.intern("British");
        for (id, name) in [(owner, "Confederation"), (foreign, "British")] {
            sim.houses
                .register(HouseState::new(id, 0, Some(id), false, 0, 0));
            assert_eq!(sim.interner.resolve(id), name);
        }
        for (cell, name) in [
            (10, "Confederation"),
            (11, "British"),
            (12, "Confederation"),
        ] {
            let id = sim
                .construct_object_limbo_at_height(
                    "NANRCT",
                    name,
                    0,
                    0,
                    0,
                    0,
                    &rules,
                    FrameEffects::default(),
                )
                .unwrap();
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .position
                .put_location(DriveCoord {
                    x: cell * 256 + 128,
                    y: cell * 256 + 128,
                    z: 0,
                });
        }
        let type_id = sim.interner.get("NANRCT").unwrap();
        let rng = sim.scenario_rng.native_state_hex();
        Simulation::retain_initial_house_technos(&sim.substrate, &mut sim.houses);
        // Native50D610 executed controls observe12,12 then10,10 for these
        // original constructor/owner/location inputs. GetCoords' Building
        // center would differ; limbo and foreign identity remain independent.
        let expected = [(type_id, (12, 12)), (type_id, (10, 10))];
        let house = sim.houses.get(&owner).unwrap();
        assert_eq!(house.initial_techno_record_count(), expected.len());
        for (index, record) in expected.into_iter().enumerate() {
            assert_eq!(house.initial_techno_record(index), Some(record));
        }
        let before_append = sim.state_hash();
        Simulation::retain_initial_house_technos(&sim.substrate, &mut sim.houses);
        assert_ne!(sim.state_hash(), before_append);
        assert_eq!(sim.scenario_rng.native_state_hex(), rng);
        let bytes = GameSnapshot::save(&sim, 0, 0, "initial-Techno-history", 0);
        let restored = GameSnapshot::load(&bytes).unwrap().sim;
        let house = restored.houses.get(&owner).unwrap();
        assert_eq!(house.initial_techno_record_count(), 4);
        for index in 0..4 {
            assert_eq!(
                house.initial_techno_record(index),
                Some(expected[index % 2])
            );
        }
    }
}
