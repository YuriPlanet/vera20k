//! Native cell fog stamps and the frozen building-picture lifecycle.
//!
//! FogState owns stamps, live draw suppression and immutable records. The
//! live object keeps simulating; only a later reveal removes its saved image.

use super::{LifecycleOutput, Simulation, UninitContext};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellObjectMember, entity_occupancy_cells};
use crate::sim::vision::{FoggedDrawKind, FoggedDrawRecord, FrozenBuilding};

impl Simulation {
    /// `CellClass::FogCell @ 0x00486A70`: eight height-admitted diagonal
    /// cells; stamp before visiting the ground list, so IsAllFogged457A10
    /// freezes a building only after its entire foundation has been stamped.
    pub(crate) fn fog_cell(
        &mut self,
        viewer: InternedId,
        cell: (u16, u16),
        context: UninitContext<'_>,
    ) {
        if !self.session.game_options().fog_of_war {
            return;
        }
        for rung in 0..8i16 {
            let candidate = {
                let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                    return;
                };
                let cells = NativeCellQuery::canonical(terrain);
                let identity = cells.lookup((
                    (cell.0 as i16).wrapping_add(rung),
                    (cell.1 as i16).wrapping_add(rung),
                ));
                let level = i16::from(cells.ground_fields(identity).0 as i8);
                if level < 2 * rung - 1 || level > 2 * rung + 1 {
                    continue;
                }
                let coord = cells.coord(identity);
                let candidate = (coord.0 as u16, coord.1 as u16);
                if self
                    .fog
                    .is_cell_object_fogged(viewer, candidate, Some(&cells))
                {
                    continue;
                }
                self.fog
                    .set_cell_object_fogged(viewer, candidate, Some(&cells), true);
                candidate
            };
            let mut member = self.cell_objects(candidate, MovementLayer::Ground).next();
            while let Some(current) = member {
                if let CellObjectMember::Entity(id) = current {
                    match self
                        .substrate
                        .entities
                        .get(id)
                        .map(|entity| entity.category())
                    {
                        Some(EntityCategory::Structure) => {
                            self.freeze_building_in_fog(viewer, id, true, context);
                        }
                        Some(
                            EntityCategory::Infantry
                            | EntityCategory::Unit
                            | EntityCategory::Aircraft,
                        ) => {
                            self.deselect_fogged_object(viewer, id);
                        }
                        _ => {}
                    }
                }
                // FogCell486BA4 reads NextObject after the virtual call.
                member = self.next_cell_object(current);
            }
        }
    }

    /// `BuildingClass::FreezeInFog @ 0x00457AA0`, with IsAllFogged457A10's
    /// admission. All foundation vectors share one record; no RNG or object
    /// ID allocation occurs in FoggedObject4D0EF0.
    pub(super) fn freeze_building_in_fog(
        &mut self,
        viewer: InternedId,
        id: u64,
        visible: bool,
        context: UninitContext<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let footprint = entity_occupancy_cells(entity);
        let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
            return;
        };
        let cells = NativeCellQuery::canonical(terrain);
        if !footprint
            .iter()
            .all(|&cell| self.fog.is_cell_object_fogged(viewer, cell, Some(&cells)))
        {
            return;
        }
        let rules = context.rules();
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        let location = position_world_coord(&entity.position);
        let origin = cells.lookup_world(location.x, location.y);
        let slots = *entity.building_art.anim_slots();
        let mut records = vec![FoggedDrawRecord {
            kind: FoggedDrawKind::Building,
            type_ref: entity.type_ref(),
            frame: crate::sim::building_art::current_body_frame(
                entity,
                object,
                rules,
                self.object_tech_level(object),
            ),
            no_zshape: object.firestorm_wall
                || (object.laser_fence && matches!(entity.building_body_state(), Some(8 | 12))),
            z_adjust: 0,
        }];
        for anim in slots.into_iter().flatten().filter_map(|id| self.anim(id)) {
            let start = rules
                .art()
                .anim_runtime_config(self.interner.resolve(anim.type_id()))
                .map_or(0, |config| config.start);
            records.push(FoggedDrawRecord {
                kind: FoggedDrawKind::Animation,
                type_ref: anim.type_id(),
                frame: anim.runtime().current_frame.wrapping_add(start),
                no_zshape: false,
                z_adjust: anim.z_adjust(),
            });
        }
        let drawing = FrozenBuilding {
            owner: entity.owner(),
            location,
            level: i32::from(cells.ground_fields(origin).0 as i8),
            // FogCell486B5A calls virtual80 (Is1x1WithUndeploy). The other
            // native exclusion is Building+6ED ==15; that translucency state
            // has no producer in the current ordinary-building path.
            visible: visible && !object.is_1x1_with_undeploy(),
            records,
        };
        self.deselect_fogged_object(viewer, id);
        self.fog.set_object_fogged(viewer, id, true);
        for anim in slots.into_iter().flatten() {
            self.fog.set_object_fogged(viewer, anim, true);
        }
        self.fog
            .insert_frozen_building(viewer, id, footprint, drawing);
    }

    /// `CellClass::CleanFog @ 0x00486BF0` visits seven diagonals (unlike
    /// FogCell's eight), clears their stamp and deletes shared snapshots in
    /// reverse cell-vector order through ClearFoggedObjects486C50.
    pub(crate) fn clean_fog(
        &mut self,
        viewer: InternedId,
        cell: (u16, u16),
        context: UninitContext<'_>,
    ) {
        // Native reveal callers admit CleanFog only for Scenario FogOfWar.
        if !self.session.game_options().fog_of_war {
            return;
        }
        for rung in 0..7i16 {
            let candidate = {
                let Some(terrain) = context.terrain().or(self.resolved_terrain.as_ref()) else {
                    return;
                };
                let cells = NativeCellQuery::canonical(terrain);
                let identity = cells.lookup((
                    (cell.0 as i16).wrapping_add(rung),
                    (cell.1 as i16).wrapping_add(rung),
                ));
                let level = i16::from(cells.ground_fields(identity).0 as i8);
                if level < 2 * rung - 1 || level > 2 * rung + 1 {
                    continue;
                }
                let coord = cells.coord(identity);
                let candidate = (coord.0 as u16, coord.1 as u16);
                self.fog
                    .set_cell_object_fogged(viewer, candidate, Some(&cells), false);
                candidate
            };
            for record in self
                .fog
                .clear_fogged_objects_at(viewer, candidate.0, candidate.1)
            {
                let Some(drawing) = record.drawing else {
                    continue;
                };
                // FoggedObject destructor4D17D2 looks up the CURRENT ground
                // building at saved XYZ. It never follows the old source ID.
                let current = {
                    let terrain = context
                        .terrain()
                        .or(self.resolved_terrain.as_ref())
                        .unwrap();
                    let cells = NativeCellQuery::canonical(terrain);
                    let identity = cells.lookup_world(drawing.location.x, drawing.location.y);
                    let coord = cells.coord(identity);
                    self.substrate.occupancy.first_building_on_layer(
                        coord.0 as u16,
                        coord.1 as u16,
                        MovementLayer::Ground,
                    )
                };
                if let Some(id) = current {
                    self.fog.set_object_fogged(viewer, id, false);
                    if let Some(entity) = self.substrate.entities.get(id) {
                        for anim in entity.building_art.anim_slots().iter().flatten() {
                            self.fog.set_object_fogged(viewer, *anim, false);
                        }
                    }
                }
            }
        }
    }

    /// ObjectClass::Deselect5F44A0, called by FogCell486B9E and
    /// FreezeInFog457AC4. The app's selection ledger consumes the retained
    /// selection bit; FollowReleased carries Deselect5F4503's camera release.
    /// The original Building, Unit, Infantry and Aircraft vtable+150 entries
    /// all hold5F44A0 (7E400C,7F5DC0,7EB1A8,7E23F4).
    fn deselect_fogged_object(&mut self, viewer: InternedId, id: u64) {
        if self.session.current_house() != Some(viewer) {
            return;
        }
        if let Some(entity) = self.substrate.entities.get_mut(id)
            && entity.selection.is_selected()
        {
            entity.selection.deselect();
            self.lifecycle_outputs
                .emit(LifecycleOutput::FollowReleased { stable_id: id });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::FrameEffects;

    fn world() -> (Simulation, RuleSet, InternedId) {
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str("[BuildingTypes]\n0=DEPOT\n[DEPOT]\nStrength=100\nImage=DEPOT\n[InfantryTypes]\n0=SCOUT\n[SCOUT]\nStrength=100\nSight=4\n"),
            &IniFile::from_str("[DEPOT]\nFoundation=2x2\n"),
        )
        .unwrap();
        let mut sim = Simulation::with_seed(31);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let viewer = sim.interner.intern("Player");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                viewer, 0, None, true, 0, 10,
            ));
        let other = sim.interner.intern("Other");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                other, 0, None, false, 0, 10,
            ));
        sim.session.set_current_house_for_test(Some(viewer));
        sim.session.game_options_mut_for_test().fog_of_war = true;
        sim.fog.set_width_for_test(32);
        sim.fog.set_height_for_test(32);
        let cells = (0..32)
            .flat_map(|y| {
                (0..32).map(move |x| {
                    super::super::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
                })
            })
            .collect();
        sim.install_resolved_terrain_for_new_map(
            crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(32, 32, cells),
        );
        (sim, rules, viewer)
    }

    fn place(sim: &mut Simulation, rules: &RuleSet) -> u64 {
        let id = sim
            .construct_object_limbo_at_height(
                "DEPOT",
                "Other",
                12,
                12,
                0,
                0,
                rules,
                FrameEffects::default(),
            )
            .unwrap();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .position
            .put_location(crate::sim::components::DriveCoord {
                x: 12 * 256 + 128,
                y: 12 * 256 + 128,
                z: 0,
            });
        sim.reveal_entity_with_rules(id, rules);
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.finish_building_construction_for_test();
        entity.selection.set_selected_for_test(true);
        id
    }

    #[test]
    fn automatic_shroud_growth_reaches_the_map_without_fog_and_preserves_saved_cadence() {
        let (mut sim, mut rules, viewer) = world();
        sim.playfield_bounds = Some(
            crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(16, 0, 0, 16, 16),
        );
        sim.playfield_size_height = Some(16);
        rules.general = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str("[AudioVisual]\nShroudGrow=yes\nShroudRate=4\n"),
            &IniFile::empty(),
        )
        .unwrap()
        .general;
        sim.session.game_options_mut_for_test().fog_of_war = false;
        sim.fog.set_ground_bits_for_test(viewer, 10, 10, 8);
        sim.fog.set_ground_bits_for_test(viewer, 11, 10, 24);
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        assert!(!sim.fog.is_ground_unshrouded(viewer, 10, 10));
        assert!(sim.fog.is_ground_unshrouded(viewer, 11, 10));
        let timer = crate::sim::timer::CdTimer::started(0, 3600);
        assert_eq!(sim.session.shroud_growth_timer(), timer);
        let saved = GameSnapshot::save(&sim, 0, 0, "shroud", 0);
        let mut restored = GameSnapshot::load(&saved).unwrap().sim;
        restored.session.set_binary_frame_for_test(3599);
        restored.advance_tick(&[], Some(&rules), None, None, 67);
        assert_eq!(restored.session.shroud_growth_timer(), timer);
        assert!(restored.fog.is_ground_unshrouded(viewer, 11, 10));
        restored.advance_tick(&[], Some(&rules), None, None, 67);
        assert_eq!(restored.session.shroud_growth_timer().start_frame(), 3600);
        assert!(!restored.fog.is_ground_unshrouded(viewer, 11, 10));
    }

    #[test]
    fn disabled_zero_and_unordered_shroud_growth_do_not_rearm_the_timer() {
        let (mut sim, mut rules, _) = world();
        sim.session.set_binary_frame_for_test(7);
        let initial = sim.session.shroud_growth_timer();
        sim.tick_shroud_growth(UninitContext::with_rules(&rules));
        assert_eq!(sim.session.shroud_growth_timer(), initial);
        rules.general.shroud_grow = true;
        rules.general.shroud_rate = crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO;
        sim.tick_shroud_growth(UninitContext::with_rules(&rules));
        assert_eq!(sim.session.shroud_growth_timer(), initial);
        rules.general.shroud_rate =
            crate::util::native_x87::NativeF64Bits::from_bits(f64::NAN.to_bits());
        sim.tick_shroud_growth(UninitContext::with_rules(&rules));
        assert_eq!(sim.session.shroud_growth_timer(), initial);
        rules.general.shroud_rate = crate::rules::ruleset::default_shroud_rate();
        sim.tick_shroud_growth(UninitContext::with_rules(&rules));
        assert_eq!(
            sim.session.shroud_growth_timer(),
            crate::sim::timer::CdTimer::started(7, 3600)
        );
    }

    #[test]
    fn frozen_foundation_survives_damage_destruction_and_load_until_revealed() {
        let (mut sim, rules, viewer) = world();
        let id = place(&mut sim, &rules);
        let context = UninitContext::with_rules(&rules);
        for cell in [(12, 12), (13, 12), (12, 13)] {
            sim.fog_cell(viewer, cell, context);
            assert!(sim.fog.fogged_objects().is_empty());
        }
        sim.fog_cell(viewer, (13, 13), context);
        let record = sim.fog.fogged_objects().values().next().unwrap().clone();
        assert_eq!(sim.fog.fogged_objects().len(), 1);
        assert_eq!(record.occupied_cells.len(), 4);
        assert!(
            !sim.substrate
                .entities
                .get(id)
                .unwrap()
                .selection
                .is_selected()
        );
        assert!(sim.fog.is_object_fogged(viewer, id));
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.health.set(1);
        assert_ne!(
            record.drawing.as_ref().unwrap().records[0].frame,
            crate::sim::building_art::current_body_frame(
                entity,
                rules.object("DEPOT").unwrap(),
                &rules,
                0,
            ),
        );
        sim.uninit_with_context(id, context);
        sim.flush_pending_delete();
        assert!(!sim.fog.is_object_fogged(viewer, id));
        assert_eq!(sim.fog.fogged_objects().values().next(), Some(&record));

        let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
        let bytes = GameSnapshot::save(&sim, 0, 0, "frozen_building", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.install_resolved_terrain_for_new_map(terrain);
        restored.resolve_type_handles(&rules);
        assert_eq!(restored.fog.fogged_objects().values().next(), Some(&record));
        let replacement = place(&mut restored, &rules);
        assert_eq!(restored.fog.fogged_objects().values().next(), Some(&record));
        assert!(restored.fog.is_object_fogged(viewer, replacement));
        restored.clean_fog(viewer, (12, 12), context);
        assert!(restored.fog.fogged_objects().is_empty());
        assert!(!restored.fog.is_object_fogged(viewer, replacement));
        for cell in &record.occupied_cells {
            assert!(
                restored
                    .fog
                    .fogged_object_cells()
                    .get(&(viewer, cell.0, cell.1))
                    .is_none_or(Vec::is_empty)
            );
        }
    }
    #[test]
    fn building_placed_in_existing_fog_stays_hidden_until_revealed() {
        let (mut sim, rules, viewer) = world();
        let context = UninitContext::with_rules(&rules);
        for cell in [(12, 12), (13, 12), (12, 13), (13, 13)] {
            sim.fog_cell(viewer, cell, context);
        }
        assert!(sim.fog.fogged_objects().is_empty());
        let building = place(&mut sim, &rules);
        assert!(sim.fog.is_object_fogged(viewer, building));
        let record = sim.fog.fogged_objects().values().next().unwrap();
        assert!(!record.drawing.as_ref().unwrap().visible);
        assert_eq!(record.occupied_cells.len(), 4);
        sim.clean_fog(viewer, (12, 12), context);
        assert!(!sim.fog.is_object_fogged(viewer, building));
        assert!(sim.fog.fogged_objects().is_empty());
    }

    #[test]
    fn regrowth_after_scout_leaves_freezes_building_on_the_native_timer() {
        let (mut sim, rules, viewer) = world();
        let building = place(&mut sim, &rules);
        let scout = sim
            .construct_object_limbo_at_height(
                "SCOUT",
                "Player",
                12,
                12,
                0,
                0,
                &rules,
                FrameEffects::default(),
            )
            .unwrap();
        sim.substrate
            .entities
            .get_mut(scout)
            .unwrap()
            .position
            .put_location(crate::sim::components::DriveCoord {
                x: 12 * 256 + 128,
                y: 12 * 256 + 128,
                z: 0,
            });
        sim.reveal_entity_with_rules(scout, &rules);
        sim.tick_fog_regrowth(UninitContext::with_rules(&rules));
        assert!(
            !sim.fog.is_object_fogged(viewer, building),
            "live scout protects the building"
        );
        let deadline = sim.session.fog_regrow_timer().duration();
        assert!(deadline > 0);
        sim.techno_limbo_with_context(scout, UninitContext::with_rules(&rules));
        sim.session.set_binary_frame_for_test(deadline as u32 - 1);
        sim.tick_fog_regrowth(UninitContext::with_rules(&rules));
        assert!(!sim.fog.is_object_fogged(viewer, building));
        sim.session.set_binary_frame_for_test(deadline as u32);
        sim.tick_fog_regrowth(UninitContext::with_rules(&rules));
        // RegrowFog4ACC50 removes one frontier at a time: the old sight's
        // outer ring fogs first, then the next period consumes its new edge.
        assert!(sim.fog.is_cell_object_fogged(viewer, (8, 12), None));
        assert!(!sim.fog.is_object_fogged(viewer, building));
        for period in 2..=5 {
            sim.session
                .set_binary_frame_for_test((period * deadline) as u32);
            sim.tick_fog_regrowth(UninitContext::with_rules(&rules));
        }
        assert!(sim.fog.is_object_fogged(viewer, building));
        assert_eq!(sim.session.fog_regrow_timer().start_frame(), 5 * deadline);
        assert_eq!(sim.fog.fogged_objects().len(), 1);
    }
    #[test]
    fn fog_records_and_timer_do_not_depend_on_the_local_viewer() {
        let make_peer = || {
            let (mut sim, rules, viewer) = world();
            // Two human viewers use noncampaign House control/defeat rules.
            sim.session
                .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
            // Both players intentionally have scouts and no base.
            sim.session.game_options_mut_for_test().short_game = false;
            let remote = sim.interner.intern("Remote");
            sim.houses
                .register(crate::sim::house_state::HouseState::new(
                    remote, 0, None, true, 0, 10,
                ));
            let building = place(&mut sim, &rules);
            for (owner, x, y) in [("Player", 12, 10), ("Remote", 24, 24)] {
                let scout = sim
                    .construct_object_limbo_at_height(
                        "SCOUT",
                        owner,
                        x,
                        y,
                        0,
                        0,
                        &rules,
                        FrameEffects::default(),
                    )
                    .unwrap();
                sim.substrate
                    .entities
                    .get_mut(scout)
                    .unwrap()
                    .position
                    .put_location(crate::sim::components::DriveCoord {
                        x: i32::from(x) * 256 + 128,
                        y: i32::from(y) * 256 + 128,
                        z: 0,
                    });
                sim.reveal_entity_with_rules(scout, &rules);
            }
            for house in [viewer, remote] {
                sim.fog.reveal_forced_map_cell(house, (20, 20), None);
            }
            (sim, rules, viewer, remote, building)
        };
        let (mut first, rules, viewer, remote, building) = make_peer();
        let (mut second, _, _, _, _) = make_peer();
        second.session.set_current_house_for_test(Some(remote));
        assert_eq!(first.state_hash(), second.state_hash());
        first.initialize_fog_of_war(UninitContext::with_rules(&rules));
        second.initialize_fog_of_war(UninitContext::with_rules(&rules));
        first.tick_fog_regrowth(UninitContext::with_rules(&rules));
        second.tick_fog_regrowth(UninitContext::with_rules(&rules));
        assert!(!first.fog.is_object_fogged(viewer, building));
        assert!(first.fog.is_object_fogged(remote, building));
        // These are the complete fog owner and timer hashes. Other owners
        // retain process-local discovery notifications until their own phase.
        let fog_hash = |sim: &Simulation| {
            sim.hash_owner_diagnostics_for_test()
                .into_iter()
                .find(|(name, _)| *name == "fog_alliances")
                .unwrap()
                .1
        };
        assert_eq!(fog_hash(&first), fog_hash(&second));
        assert_eq!(
            first.session.fog_regrow_timer(),
            second.session.fog_regrow_timer()
        );
    }
}
