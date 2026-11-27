//! DisplayClass lifecycle queries. The vectors belong to ObjectSubstrate;
//! queries borrow the object stores without copying coordinates. Ground is an
//! unordered set in the simulation (`DisplayLayers::submit`); the picture and
//! entity picking order it by GetYSort through [`Simulation::ground_draw_order`].
//! Upper-layer GPU interleaving and remaining locomotor resubmission writers
//! are still open.
//! Native query/membership comparisons: tools/spatial_oracle/display_non_entity.json.

use super::Simulation;
use super::display_layers::DisplayLayer;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::movement::ground_pose::{building_render_order_parts, position_world_coord};
use crate::sim::world::ObjectSubstrate;

/// Foot4DB7E0 / Aircraft41ADC0 dispatch to the active locomotor. Tunnel72A1A0
/// answers from its State, Fly4CFCF0 reads physical GetHeight, Jumpjet54B8D0 additionally reads structural
/// bridge, falling, its owner's virtual +54 (IsHighFlying,
/// [`is_high_flying`]) and its linked +2C height. Ground movers do not query
/// height. No terrain is a headless compatibility case, not native proof.
///
/// [`is_high_flying`]: crate::sim::movement::air_movement::is_high_flying
pub(crate) fn entity_layer(
    entity: &GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
    rules: Option<(&RuleSet, &crate::sim::intern::StringInterner)>,
) -> DisplayLayer {
    let kind = entity.locomotor.as_ref().map(|l| l.active_kind());
    if entity.category() != EntityCategory::Structure {
        // Rocket In_Which_Layer (`0x00663460`) answers the Air layer.
        if kind == Some(LocomotorKind::Rocket) {
            return DisplayLayer::AIR;
        }
        // Tunnel In_Which_Layer (`0x0072A1A0`): underground in State 4.
        if entity
            .locomotor
            .as_ref()
            .and_then(|locomotor| locomotor.tunnel_runtime())
            .is_some_and(crate::sim::movement::tunnel_movement::TunnelRuntime::underground)
        {
            return DisplayLayer::UNDERGROUND;
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
    let height = crate::sim::movement::air_movement::current_fly_height(entity, terrain);
    let mut adjusted_height = height;
    if kind == Some(LocomotorKind::Jumpjet) && !entity.on_bridge() {
        // This map lookup precedes the high-flying gate even when +74 is false.
        if let Some(terrain) = terrain {
            let cell = terrain.native_cell_identity(((raw.x / 256) as i16, (raw.y / 256) as i16));
            if terrain.native_cell_flags(cell) & 0x100 != 0 && !entity.is_falling_down() {
                let above_deck = crate::sim::movement::ground_pose::corrected_deck_z(terrain, cell)
                    .map_or_else(
                        || height.wrapping_sub(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS),
                        |deck| raw.z.wrapping_sub(deck),
                    );
                if above_deck >= 0 {
                    adjusted_height = above_deck;
                }
            }
        }
    }
    // The owner's +54 (Foot4DE620 -> Object5F6B90, Aircraft41B920).
    if !crate::sim::movement::air_movement::is_high_flying(entity, terrain, rules)
        || adjusted_height == 0
    {
        return DisplayLayer::GROUND;
    }
    let cruise_height = if kind == Some(LocomotorKind::Jumpjet) {
        entity
            .locomotor
            .as_ref()
            .and_then(|l| l.jumpjet_runtime())
            .expect("active Jumpjet owns its runtime")
            .params()
            .height()
    } else {
        // Object5F4260 uses Rules+420, not FlightLevel or a type's height.
        rules.map_or(400, |(rules, _)| rules.general.display_cruise_height)
    };
    DisplayLayer::for_object(true, || adjusted_height, cruise_height)
}

fn entity_sort_key(
    entity: &GameEntity,
    object: Option<&crate::rules::object_type::ObjectType>,
) -> i32 {
    let raw = position_world_coord(&entity.position);
    let (coord, adjust) = if entity.category() == EntityCategory::Structure {
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

/// What GetYSort (5F6220's comparison read) reads of a Ground member,
/// independently of Logic membership.
struct GroundSortView<'a> {
    entities: &'a crate::sim::entity_store::EntityStore,
    anims: &'a crate::sim::anim_class::AnimStore,
    particles: &'a crate::sim::particles::ParticleSystemStore,
    terrain: &'a std::collections::BTreeMap<u64, crate::sim::terrain_object::TerrainObjectState>,
    interner: &'a crate::sim::intern::StringInterner,
    rules: Option<&'a RuleSet>,
}

impl GroundSortView<'_> {
    /// GetYSort of Ground member `id`.
    fn key(&self, id: u64) -> Option<i32> {
        if let Some(entity) = self.entities.get(id) {
            // Only BuildingClass GetYSort reads type terms.
            let object = (entity.category() == EntityCategory::Structure)
                .then_some(self.rules)
                .flatten()
                .and_then(|rules| {
                    self.interner.object_type_in_category(
                        entity.type_ref(),
                        entity.category().into(),
                        rules,
                    )
                });
            return Some(entity_sort_key(entity, object));
        }
        if let Some(system) = self.particles.get(id) {
            // ParticleSystem VT7EFB9C: +AC ->41BE00 ->+48 ->5F65A0,
            // +B8 ->5F6BD0.
            return Some(system.coords().x.wrapping_add(system.coords().y));
        }
        if let Some(anim) = self.anims.get(id) {
            // Anim422BC0 -> Object5F6BD0 -> GetCoords422BE0, then retained
            // instance+104. Type changes do not recopy this constructor field.
            return Some(crate::sim::anim_class::anim_display_sort_key(
                anim,
                self.entities,
            ));
        }
        // Terrain VT7F522C inherits Object GetYSort5F6BD0: retained
        // GetCoords XY, independent of presentation lift.
        let coord = self.terrain.get(&id)?.world_coord();
        Some(coord.x.wrapping_add(coord.y))
    }
}

impl Simulation {
    /// Display4A9720 for `id` into `layer` (`DisplayLayers::submit`).
    pub(crate) fn submit_object_display(
        substrate: &mut ObjectSubstrate,
        id: u64,
        layer: DisplayLayer,
    ) {
        substrate.display.submit(id, Some(layer));
    }

    /// The Ground members in draw order, by GetYSort key: what
    /// the native ordered insert (Ground551A90) and per-frame adjacent pass
    /// (Layer551A30) keep the Ground vector close to. The simulation keeps
    /// Ground unordered (`DisplayLayers::submit`), so the picture and entity
    /// picking sort it here, off the simulation's frame. A member no store
    /// holds sorts first. Equal keys keep submission order
    /// (`DisplayLayers::submission_order`), the local anims (a LAN or online
    /// MoveFlash) among them. Sim-scale deviation: a member that moved is
    /// placed at once rather than one neighbour per frame. Without `rules` a
    /// Building's key leaves out its type terms (`entity_sort_key`).
    pub fn ground_draw_order(&self, rules: Option<&RuleSet>) -> Vec<u64> {
        let mut order =
            Vec::with_capacity(self.substrate.display.members(DisplayLayer::GROUND).len());
        self.visit_layer_draws(DisplayLayer::GROUND, rules, &[], |id, _| order.push(id));
        order
    }

    /// Every Display member in draw order: the five layers in index order,
    /// Ground as [`Self::ground_draw_order`] sorts it, the other layers as
    /// they hold their members. What the native walks of the retained
    /// vectors (Tactical6D8F19, TacticalPick6DA380) read.
    pub fn display_draw_order(&self, rules: Option<&RuleSet>) -> Vec<(DisplayLayer, u64)> {
        self.draw_order(rules, false)
    }

    /// [`Self::display_draw_order`] with each revealed presentation anim
    /// (`Simulation::presentation_anims`) where gamemd's Display would hold
    /// it as an ordinary anim: at the place in submission order it took when
    /// it was placed (`AnimObject::presentation_order`), Ground by its
    /// GetYSort key first (Anim422BC0). The presentation particle systems
    /// and their particles take their places the same way
    /// (`ParticleSystemStore::presented_systems`). What the picture draws.
    pub fn picture_draw_order(&self, rules: Option<&RuleSet>) -> Vec<(DisplayLayer, u64)> {
        self.draw_order(rules, true)
    }

    /// Visit the same picture in draw order, carrying the already computed
    /// Ground GetYSort key. The client can retain it in the frame before the
    /// simulation lends its entity store. Other layers sort by submission.
    pub fn visit_picture_draw_order(
        &self,
        rules: Option<&RuleSet>,
        visit: impl FnMut(DisplayLayer, u64, i32),
    ) {
        self.visit_draw_order(rules, true, visit);
    }

    fn draw_order(&self, rules: Option<&RuleSet>, presentation: bool) -> Vec<(DisplayLayer, u64)> {
        let mut order =
            Vec::with_capacity(self.substrate.display.members(DisplayLayer::GROUND).len());
        self.visit_draw_order(rules, presentation, |layer, id, _| order.push((layer, id)));
        order
    }

    fn visit_draw_order(
        &self,
        rules: Option<&RuleSet>,
        presentation: bool,
        mut visit: impl FnMut(DisplayLayer, u64, i32),
    ) {
        let mut unshared: [Vec<(u64, u64)>; 5] = Default::default();
        if presentation {
            for (&id, anim) in self.substrate.anims.presentation_iter() {
                if let Some(layer) = anim.presentation_layer()
                    && !self.substrate.anims.local_delete_queued(id)
                {
                    unshared[usize::from(layer.index())].push((anim.presentation_order(), id));
                }
            }
            // A presentation particle system draws where Ground would hold
            // it (ParticleSystem VT7EFB9C+78 ->62FE80), and each of its
            // particles where Air would (GetLayer62D770).
            for (id, system) in self.substrate.particle_systems.presented_systems() {
                unshared[usize::from(DisplayLayer::GROUND.index())]
                    .push((system.presentation_order(), id));
                unshared[usize::from(DisplayLayer::AIR.index())].extend(
                    system
                        .particles()
                        .iter()
                        .map(|particle| (particle.presentation_order(), particle.stable_id())),
                );
            }
        }
        for (index, unshared) in unshared.iter().enumerate() {
            let layer = DisplayLayer::from_index(index as u8).expect("native Display layer index");
            self.visit_layer_draws(layer, rules, unshared, |id, key| visit(layer, id, key));
        }
    }

    /// `layer`'s members and `unshared` draws (`(place, id)`) in draw order:
    /// by submission order, Ground by GetYSort key first.
    fn visit_layer_draws(
        &self,
        layer: DisplayLayer,
        rules: Option<&RuleSet>,
        unshared: &[(u64, u64)],
        mut visit: impl FnMut(u64, i32),
    ) {
        let display = &self.substrate.display;
        let members = display.members(layer);
        let local: &[u64] = if layer == DisplayLayer::GROUND {
            display.local_ground()
        } else {
            &[]
        };
        // The other layers already hold their members in submission order.
        if layer != DisplayLayer::GROUND && unshared.is_empty() {
            for &id in members {
                visit(id, 0);
            }
            return;
        }
        let view = GroundSortView {
            entities: &self.substrate.entities,
            anims: &self.substrate.anims,
            particles: &self.substrate.particle_systems,
            terrain: self.terrain_objects.objects(),
            interner: &self.interner,
            rules,
        };
        let key = |id: u64| {
            if layer == DisplayLayer::GROUND {
                view.key(id).unwrap_or(i32::MIN)
            } else {
                0
            }
        };
        let mut keyed: Vec<(i32, u64, u64)> = members
            .iter()
            .chain(local)
            .map(|&id| (id, display.submission_order(id).unwrap_or(0)))
            .chain(unshared.iter().map(|&(place, id)| (id, place)))
            .map(|(id, place)| (key(id), place, id))
            .collect();
        // Each id appears once, so the order is total.
        keyed.sort_unstable();
        for (key, _, id) in keyed {
            visit(id, key);
        }
    }

    pub(super) fn submit_entity_display(
        interner: &StringInterner,
        substrate: &mut ObjectSubstrate,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        id: u64,
        rules: &RuleSet,
        terrain: Option<&ResolvedTerrainGrid>,
    ) {
        let Some(entity) = substrate.entities.get(id) else {
            return;
        };
        let layer = entity_layer(
            entity,
            terrain.or(resolved_terrain.as_ref()),
            Some((rules, interner)),
        );
        Self::submit_object_display(substrate, id, layer);
    }

    pub(crate) fn entity_display_layer(&self, id: u64, rules: &RuleSet) -> Option<DisplayLayer> {
        self.substrate.entities.get(id).map(|entity| {
            entity_layer(
                entity,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            )
        })
    }

    /// Resubmit `id`'s display from its current state, as a native
    /// resubmission site does.
    #[cfg(any(test, feature = "test-support"))]
    pub fn resubmit_entity_display_for_test(&mut self, id: u64) {
        Self::submit_entity_display(
            &self.interner,
            &mut self.substrate,
            &self.resolved_terrain,
            id,
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
        );
    }

    /// Jumpjet54AECB/54AED1 captures the live query before Process.54B16F
    /// checks alive, then54B17F compares the new answer with that capture,
    /// not Object+94. Fly has different explicit resubmission sites and must
    /// not be normalized here by a generic per-frame layer/cache comparison.
    pub(super) fn complete_jumpjet_display_process(
        interner: &StringInterner,
        substrate: &mut ObjectSubstrate,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        id: u64,
        before: DisplayLayer,
        rules: &RuleSet,
    ) {
        let Some(entity) = substrate.entities.get(id) else {
            return;
        };
        if entity.lifecycle.object_alive()
            && entity_layer(entity, resolved_terrain.as_ref(), Some((rules, interner))) != before
        {
            Self::submit_entity_display(interner, substrate, resolved_terrain, id, rules, None);
        }
    }
}

#[cfg(all(test, tests_sim_world_c_o))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::util::fixed_math::SimFixed;

    #[test]
    fn building_rules_key_terms_feed_the_ground_draw_order() {
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
                crate::sim::combat::Health::new(100),
                sim.interner.intern(name),
                EntityCategory::Structure,
                0,
                5,
                false,
            );
            entity.position.set_sub_x_for_test(SimFixed::from_num(128));
            entity.position.set_sub_y_for_test(SimFixed::from_num(128));
            assert_eq!(
                entity_sort_key(&entity, rules.object(name)),
                expected,
                "{name}"
            );
            sim.substrate.entities.insert(entity);
            Simulation::submit_object_display(&mut sim.substrate, id, DisplayLayer::GROUND);
        }
        assert_eq!(sim.ground_draw_order(Some(&rules)), [3, 1, 4, 2]);
    }

    /// Ground551A90 inserts after equal keys and Display4A9770 removes in
    /// order, so equal keys draw in submission order; VERA's swap-removal
    /// must not hand the last submit an earlier member's place.
    #[test]
    fn equal_ground_keys_keep_submission_order_after_a_swap_removal() {
        let mut sim = Simulation::new();
        for id in [1, 2, 3] {
            Simulation::submit_object_display(&mut sim.substrate, id, DisplayLayer::GROUND);
        }
        sim.substrate.display.remove(1);
        assert_eq!(sim.substrate.display.members(DisplayLayer::GROUND), [3, 2]);
        // No store holds them, so every key is equal (i32::MIN).
        assert_eq!(sim.ground_draw_order(None), [2, 3]);
    }

    #[test]
    fn entity_layer_matches_original_queries() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/display_entity_layer.json",
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
            entity.position.set_rx_for_test(10);
            entity.position.set_ry_for_test(10);
            entity.position.set_sub_x_for_test(SimFixed::from_num(128));
            entity.position.set_sub_y_for_test(SimFixed::from_num(128));
            entity
                .position
                .set_exact_z_leptons_for_test(Some(input["z"].as_i64().unwrap() as i32));
            entity
                .lifecycle
                .set_cell_marked_for_test(input["marked"].as_bool().unwrap_or(true));
            entity.set_on_bridge(input["on_bridge"].as_bool().unwrap_or(false));
            entity.set_falling_down_for_test(input["falling"].as_bool().unwrap_or(false));
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
                let mut params = runtime.params();
                params.set_height_for_test(input["linked_height"].as_i64().unwrap_or(500) as i32);
                *runtime = runtime.clone().with_params_for_test(params);
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
                entity_layer(entity, Some(&terrain), Some((&rules, &sim.interner))),
                DisplayLayer::from_index(row["layer"].as_u64().unwrap() as u8).unwrap(),
                "{name}"
            );
            // Place_Down and Pick_Up ask this query: a list exactly when the
            // native answer is Ground, then the OnBridge one.
            assert_eq!(
                crate::sim::occupancy::cell_list_layer_for_entity(entity, Some(&terrain)),
                (row["layer"] == 2).then_some(if entity.on_bridge() {
                    crate::sim::movement::locomotor::MovementLayer::Bridge
                } else {
                    crate::sim::movement::locomotor::MovementLayer::Ground
                }),
                "{name}: cell list"
            );
        }
    }
}
