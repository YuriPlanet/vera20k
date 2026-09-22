//! DisplayClass lifecycle queries. The vectors belong to ObjectSubstrate;
//! queries borrow the object stores without copying coordinates or sort keys.
//! Ground rendering and entity picking consume the retained vectors. Upper-layer
//! GPU interleaving and remaining locomotor resubmission writers are still open.
//! Native query/membership comparisons: tools/spatial_oracle/display_non_entity.json.

use super::Simulation;
use super::display_layers::DisplayLayer;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::ground_pose::{
    building_render_order_parts, ground_surface_z_at, position_world_coord,
};

/// Foot4DB7E0 / Aircraft41ADC0 dispatch to the active locomotor. Fly4CFCF0
/// reads physical GetHeight, Jumpjet54B8D0 additionally reads marked-on-map,
/// structural bridge, falling and its linked +2C height. Ground movers do not
/// query height. No terrain is a headless compatibility case, not native proof.
fn entity_layer(
    entity: &GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
    rules: Option<&RuleSet>,
) -> DisplayLayer {
    let kind = entity.locomotor.as_ref().map(|l| l.active_kind());
    if entity.category != EntityCategory::Structure {
        if kind == Some(LocomotorKind::Rocket) || (kind.is_none() && entity.rocket_state.is_some())
        {
            return DisplayLayer::AIR;
        }
        if !matches!(kind, Some(LocomotorKind::Fly | LocomotorKind::Jumpjet)) {
            return DisplayLayer::GROUND;
        }
        if kind == Some(LocomotorKind::Fly) {
            return if crate::sim::movement::air_movement::current_fly_height(entity, terrain) > 0 {
                DisplayLayer::TOP
            } else {
                DisplayLayer::GROUND
            };
        }
    }
    let raw = position_world_coord(&entity.position);
    let height = ground_surface_z_at([raw.x, raw.y], entity.on_bridge, terrain, None)
        .map(|ground| raw.z.wrapping_sub(ground))
        .unwrap_or_else(|| {
            entity
                .locomotor
                .as_ref()
                .map_or(0, |l| l.altitude.to_num::<i32>())
        });
    let mut adjusted_height = height;
    if kind == Some(LocomotorKind::Jumpjet) && !entity.on_bridge {
        // This map lookup precedes the high-flying gate even when +74 is false.
        let flags = terrain.map_or(0, |terrain| {
            let cell = terrain.native_cell_identity(((raw.x / 256) as i16, (raw.y / 256) as i16));
            terrain.native_cell_flags(cell)
        });
        if flags & 0x100 != 0 && height >= 416 && entity.object_is_falling_down == 0 {
            adjusted_height = height.wrapping_sub(416);
        }
    }
    // Object5F6B90 (+54 via4DE620): marked and GetHeight>=two levels.
    if !entity.lifecycle.cell_marked || height < 208 || adjusted_height == 0 {
        return DisplayLayer::GROUND;
    }
    let cruise_height = if kind == Some(LocomotorKind::Jumpjet) {
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.jumpjet_runtime())
            .expect("active Jumpjet owns its runtime")
            .params
            .height
    } else {
        // Object5F4260 uses Rules+420, not FlightLevel or a type's height.
        rules.map_or(400, |rules| rules.general.display_cruise_height)
    };
    if adjusted_height < cruise_height {
        DisplayLayer::AIR
    } else {
        DisplayLayer::TOP
    }
}

fn entity_sort_key(
    entity: &GameEntity,
    object: Option<&crate::rules::object_type::ObjectType>,
) -> i32 {
    let raw = position_world_coord(&entity.position);
    let (coord, adjust) = if entity.category == EntityCategory::Structure {
        building_render_order_parts(
            raw,
            object.is_some_and(|object| object.turret_anim_is_voxel),
            object.is_some_and(|object| object.gate),
        )
    } else {
        (raw, 0)
    };
    coord.x.wrapping_add(coord.y).wrapping_add(adjust)
}

/// Ground comparisons read the live receiver each time (5F6220), independently
/// of Logic membership. Air/Surface/Top insertions never call GetYSort.
struct GroundSortView<'a> {
    entities: &'a crate::sim::entity_store::EntityStore,
    anims: &'a crate::sim::anim_class::AnimStore,
    particles: &'a crate::sim::particles::ParticleSystemStore,
    terrain: &'a std::collections::BTreeMap<u64, crate::sim::terrain_object::TerrainObjectState>,
    interner: &'a crate::sim::intern::StringInterner,
    rules: Option<&'a RuleSet>,
}

impl GroundSortView<'_> {
    fn key(&self, id: u64) -> i32 {
        if let Some(entity) = self.entities.get(id) {
            return entity_sort_key(
                entity,
                self.rules
                    .and_then(|r| r.object(self.interner.resolve(entity.type_ref()))),
            );
        }
        if let Some(system) = self.particles.get(id) {
            // ParticleSystem VT7EFB9C: +AC ->41BE00 ->+48 ->5F65A0,
            // +B8 ->5F6BD0. Attachment updates coords in its AI, not here.
            return system.coords.x.wrapping_add(system.coords.y);
        }
        if let Some(anim) = self.anims.get(id) {
            // Anim422BC0 -> Object5F6BD0 -> GetCoords422BE0, then retained
            // instance+104. Type changes do not recopy this constructor field.
            return crate::sim::anim_class::anim_display_sort_key(anim, self.entities);
        }
        if let Some(terrain) = self.terrain.get(&id) {
            // Terrain ctor71BC4A..71BC76 sign-extends the cell coordinates,
            // centers them and passes Z=0 to Unlimbo. VT7F522C inherits
            // Object GetYSort5F6BD0; terrain has no runtime relocation writer.
            let x = i32::from(terrain.rx as i16) * 256 + 128;
            let y = i32::from(terrain.ry as i16) * 256 + 128;
            return x.wrapping_add(y);
        }
        panic!("unrepresented Ground display identity {id}");
    }
}

impl Simulation {
    pub(crate) fn submit_object_display(
        &mut self,
        id: u64,
        layer: DisplayLayer,
        rules: Option<&RuleSet>,
    ) {
        let view = GroundSortView {
            entities: &self.substrate.entities,
            anims: &self.substrate.anims,
            particles: &self.substrate.particle_systems,
            terrain: &self.production.terrain_objects,
            interner: &self.interner,
            rules,
        };
        self.substrate
            .display
            .submit(id, Some(layer), |id| view.key(id));
    }

    pub(super) fn submit_entity_display(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        terrain: Option<&ResolvedTerrainGrid>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let layer = entity_layer(entity, terrain.or(self.resolved_terrain.as_ref()), rules);
        self.submit_object_display(id, layer, rules);
    }

    pub(super) fn entity_display_layer(
        &self,
        id: u64,
        rules: Option<&RuleSet>,
    ) -> Option<DisplayLayer> {
        self.substrate
            .entities
            .get(id)
            .map(|entity| entity_layer(entity, self.resolved_terrain.as_ref(), rules))
    }

    /// Jumpjet54AECB/54AED1 captures the live query before Process.54B16F
    /// checks alive, then54B17F compares the new answer with that capture,
    /// not Object+94. Fly has different explicit resubmission sites and must
    /// not be normalized here by a generic per-frame layer/cache comparison.
    pub(super) fn complete_jumpjet_display_process(
        &mut self,
        id: u64,
        before: DisplayLayer,
        rules: Option<&RuleSet>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.lifecycle.object_alive
            && entity_layer(entity, self.resolved_terrain.as_ref(), rules) != before
        {
            self.submit_entity_display(id, rules, None);
        }
    }

    pub(super) fn sort_display_ground(&mut self, rules: Option<&RuleSet>) {
        let view = GroundSortView {
            entities: &self.substrate.entities,
            anims: &self.substrate.anims,
            particles: &self.substrate.particle_systems,
            terrain: &self.production.terrain_objects,
            interner: &self.interner,
            rules,
        };
        self.substrate.display.sort_ground_pass(|id| view.key(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::util::fixed_math::SimFixed;

    #[test]
    fn building_rules_key_terms_feed_display_submission() {
        // Building459EF0 subtracts128 from X/Y; GetYSort449410 independently
        // adds32 for TurretAnimIsVoxel and subtracts16 for Gate. Keep these
        // regressions at the authority, not in the presentation planner.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=NORMAL\n1=TURRET\n2=GATE\n3=BOTH\n\
             [NORMAL]\nStrength=100\n[TURRET]\nStrength=100\nTurretAnimIsVoxel=yes\n\
             [GATE]\nStrength=100\nGate=yes\n\
             [BOTH]\nStrength=100\nTurretAnimIsVoxel=yes\nGate=yes\n",
        ))
        .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        for (index, (name, expected)) in [
            ("NORMAL", 7680),
            ("TURRET", 7712),
            ("GATE", 7664),
            ("BOTH", 7696),
        ]
        .into_iter()
        .enumerate()
        {
            let id = index as u64 + 1;
            let mut entity = GameEntity::new_at_frame_zero_for_test(
                id,
                10,
                20,
                13,
                0,
                owner,
                crate::sim::components::Health { current: 100 },
                sim.interner.intern(name),
                EntityCategory::Structure,
                0,
                5,
                false,
            );
            entity.position.sub_x = SimFixed::from_num(128);
            entity.position.sub_y = SimFixed::from_num(128);
            assert_eq!(
                entity_sort_key(&entity, rules.object(name)),
                expected,
                "{name}"
            );
            sim.entities_mut().insert(entity);
            sim.submit_object_display(id, DisplayLayer::GROUND, Some(&rules));
        }
        assert_eq!(
            sim.display_layers().members(DisplayLayer::GROUND),
            [3, 1, 4, 2]
        );
    }

    #[test]
    fn entity_layer_matches_original_queries() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/display_entity_layer.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 88);
        for row in rows {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let mut sim = Simulation::new();
            let kind = input["kind"].as_str().unwrap();
            super::super::lifecycle_tests::insert_entity(
                &mut sim,
                1,
                if kind == "building" {
                    EntityCategory::Structure
                } else {
                    EntityCategory::Unit
                },
            );
            let entity = sim.substrate.entities.get_mut(1).unwrap();
            entity.position.rx = 10;
            entity.position.ry = 10;
            entity.position.sub_x = SimFixed::from_num(128);
            entity.position.sub_y = SimFixed::from_num(128);
            entity.position.exact_z_leptons = Some(input["z"].as_i64().unwrap() as i32);
            entity.lifecycle.cell_marked = input["marked"].as_bool().unwrap_or(true);
            entity.on_bridge = input["on_bridge"].as_bool().unwrap_or(false);
            entity.object_is_falling_down = u8::from(input["falling"].as_bool().unwrap_or(false));
            let loco_kind = match kind {
                "walk" => Some(LocomotorKind::Walk),
                "drive" => Some(LocomotorKind::Drive),
                "fly" => Some(LocomotorKind::Fly),
                "jumpjet" => Some(LocomotorKind::Jumpjet),
                "building" => None,
                _ => panic!("unexpected {kind}"),
            };
            entity.locomotor = loco_kind.map(LocomotorState::for_test_kind);
            if let Some(runtime) = entity
                .locomotor
                .as_mut()
                .and_then(|l| l.jumpjet_runtime_mut())
            {
                runtime.params.height = input["linked_height"].as_i64().unwrap_or(500) as i32;
            }
            let mut cell = super::super::common_raw_test_terrain_cell(
                10,
                10,
                input["level"].as_u64().unwrap_or(0) as u8,
                input["bridge"].as_bool().unwrap_or(false),
            );
            cell.slope_type = input["slope"].as_u64().unwrap_or(0) as u8;
            // from_cells uses dense row-major slots, not the cell's coordinates
            // to index a sparse vector. The native fixture asserts real10,10.
            let mut cells = (0..16)
                .flat_map(|y| {
                    (0..16).map(move |x| super::super::common_raw_test_terrain_cell(x, y, 0, false))
                })
                .collect::<Vec<_>>();
            cells[10 * 16 + 10] = cell;
            let terrain = ResolvedTerrainGrid::from_cells(16, 16, cells);
            // Exercise the independent section reader, including no General.
            let ini = format!(
                "[JumpjetControls]\nCruiseHeight={}\n",
                input["global_cruise_height"].as_i64().unwrap_or(400)
            );
            let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
            assert_eq!(
                entity_layer(entity, Some(&terrain), Some(&rules)),
                DisplayLayer::from_index(row["layer"].as_u64().unwrap() as u8).unwrap(),
                "{name}"
            );
            if kind == "fly" {
                // Both consumers dispatch Fly4CFCF0. A stale compatibility
                // cache must not put an airborne object on the ground list
                // (or remove a landed one), independently of display retention.
                for cached_height in [-1000, 1000] {
                    entity.locomotor.as_mut().unwrap().altitude = SimFixed::from_num(cached_height);
                    assert_eq!(
                        crate::sim::occupancy::cell_list_layer_for_entity(entity, Some(&terrain))
                            .is_some(),
                        row["layer"] == 2,
                        "{name}, cached height {cached_height}",
                    );
                    assert_eq!(
                        entity_layer(entity, Some(&terrain), Some(&rules)),
                        DisplayLayer::from_index(row["layer"].as_u64().unwrap() as u8).unwrap(),
                        "{name}, cached height {cached_height}",
                    );
                }
            }
        }
    }
}
