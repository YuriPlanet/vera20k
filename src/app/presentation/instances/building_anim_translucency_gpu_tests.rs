//! Physical building-slot lifecycle and shared original-pixel test equipment.
//!
//! The lifecycle test below uses a retail scene and ordinary commands. There
//! are no pixel expectations in this lifecycle test. The render sibling reuses
//! its actual slot emitter against the genuine GAPOWR_A original corpus.

use super::*;
use crate::headless_scenario::HeadlessScenario;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, TickLane};
use crate::skirmish_launch::LaunchCountry;
use std::path::Path;

pub(crate) const PLANT_OWNER: &str = "SlotPlant";
const ATTACKER_OWNER: &str = "SlotAttacker";

pub(crate) struct SlotScene {
    pub(crate) scene: HeadlessScenario,
    pub(crate) plant: u64,
    attacker_cells: Vec<(u16, u16)>,
    attacker: Option<(InternedId, u64)>,
}

impl SlotScene {
    pub(crate) fn load(root: &Path) -> Self {
        let mut launch = crate::headless_scenario::battle_session("XMP03T4.MAP", &[]);
        launch.options.shroud = false;
        launch.options.fog_of_war = false;
        launch.options.unit_count = 0;
        // This deliberately placed attacker owns an infantryman, not a base.
        // ShortGame's ordinary House gate counts only Buildings/BaseUnit and
        // would legitimately destroy it on the first update. Normal game rules
        // count the admitted infantry; no House defeat/health state is patched.
        launch.options.short_game = false;
        let mut scene =
            crate::headless_scenario::load_session(root, "XMP03T4.MAP", 0x4521_C0, launch)
                .expect("physical XMP03T4 scene and bound resources");
        assert_eq!(scene.map.header.theater, "TEMPERATE");
        assert!(!scene.sim().session().game_options().short_game);
        scene.add_human_house(PLANT_OWNER, LaunchCountry::America);

        // A supplied placement on genuine map cells, not modified terrain.
        // Only the real foundation and an in-range foot cell need be level;
        // neither sea level nor an otherwise empty 11x11 square is required.
        // Match Unlimbo's height-aware playfield membership: geometry alone
        // admits upper-edge cells from which Approach must first move inward.
        // Actual Unlimbo remains responsible for land/overlay/occupation admission.
        let candidates = {
            let sim = scene.sim();
            let terrain = sim.resolved_terrain().unwrap();
            let bounds = sim.playfield_bounds().unwrap();
            let offsets = scene
                .runtime
                .resources
                .rules
                .object("GAPOWR")
                .unwrap()
                .foundation
                .cell_offsets();
            let mut levels = std::collections::BTreeMap::new();
            for tile in terrain.cells().iter().filter(|tile| {
                crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (i32::from(tile.rx), i32::from(tile.ry)),
                    Some(bounds),
                    Some(terrain),
                )
            }) {
                *levels.entry(tile.level).or_insert(0_usize) += 1;
            }
            eprintln!("physical XMP03T4 playfield cell levels: {levels:?}");
            terrain
                .cells()
                .iter()
                .filter(|cell| cell.rx >= 6 && cell.ry >= 6)
                .filter(|cell| {
                    offsets.iter().all(|&(dx, dy)| {
                        let x = i32::from(cell.rx) + i32::from(dx);
                        let y = i32::from(cell.ry) + i32::from(dy);
                        crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                            (x, y),
                            Some(bounds),
                            Some(terrain),
                        ) && terrain
                            .cell(x as u16, y as u16)
                            .is_some_and(|tile| tile.level == cell.level && tile.slope_type == 0)
                    }) && sim.entities().values().all(|entity| {
                        i32::from(entity.position.rx()).abs_diff(i32::from(cell.rx)) > 5
                            || i32::from(entity.position.ry()).abs_diff(i32::from(cell.ry)) > 5
                    })
                })
                .filter_map(|cell| {
                    let (x, y) = (cell.rx, cell.ry);
                    let attacker_cells = [
                        (x - 3, y),
                        (x, y - 3),
                        (x + 4, y),
                        (x, y + 4),
                        (x - 3, y + 1),
                        (x + 1, y - 3),
                        (x + 4, y + 1),
                        (x + 1, y + 4),
                    ]
                    .into_iter()
                    .filter(|&(x, y)| {
                        crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                            (i32::from(x), i32::from(y)),
                            Some(bounds),
                            Some(terrain),
                        ) && terrain
                            .cell(x, y)
                            .is_some_and(|tile| tile.level == cell.level && tile.slope_type == 0)
                    })
                    .collect::<Vec<_>>();
                    (!attacker_cells.is_empty()).then_some((x, y, attacker_cells))
                })
                .collect::<Vec<_>>()
        };
        eprintln!(
            "physical XMP03T4 level foundation/in-range candidates: {}",
            candidates.len()
        );
        let (plant, attacker_cells) = candidates
            .into_iter()
            .find_map(|(x, y, attacker_cells)| {
                scene
                    .runtime
                    .simulation
                    .spawn_object_with_overlay_registry(
                        "GAPOWR",
                        PLANT_OWNER,
                        x,
                        y,
                        0,
                        &scene.runtime.resources.rules,
                        &scene.runtime.resources.overlay_registry,
                    )
                    .map(|id| (id, attacker_cells))
            })
            .expect("a clear physical patch admits GAPOWR");
        let fixture = Self {
            scene,
            plant,
            attacker_cells,
            attacker: None,
        };
        fixture.assert_physical_inputs();
        fixture
    }

    fn assert_physical_inputs(&self) {
        let rules = &self.scene.runtime.resources.rules;
        let object = rules.object("GAPOWR").unwrap();
        let art = rules
            .art()
            .resolve_metadata_entry("GAPOWR", &object.image)
            .unwrap();
        let slot = art
            .building_anims
            .iter()
            .find(|slot| slot.native_slot == 3)
            .unwrap();
        assert_eq!(slot.anim_type, "GAPOWR_A");
        assert_eq!((slot.z_adjust, slot.y_sort), (-32, 362));
        assert!(object.powered_special);
        assert!(art.building_anim_power[3].powered_special);
        let config = rules.art().anim_runtime_config("GAPOWR_A").unwrap();
        assert!(config.art_body_read);
        assert!(
            !config.shadow,
            "physical GAPOWR_A has no companion shadow pass"
        );
        assert!(!config.translucent && config.translucency == 0 && !config.double_thick);
        assert!(!config.flat && !config.tiled && !config.bouncer && !config.is_meteor);
        assert_eq!(
            (config.start, config.loop_start, config.loop_end),
            (0, 0, 8)
        );
        assert_eq!(config.loop_count, -1);
        assert_eq!(config.detail_level, 2);
        assert!(config.raw_shp_frame_count.is_some_and(|count| count >= 8));
        assert!(config.should_use_cell_drawer);
        let cleg = rules.object("CLEG").unwrap();
        assert_eq!(cleg.primary(), Some("NeutronRifle"));
        let weapon = rules.weapon("NeutronRifle").unwrap();
        assert_eq!(weapon.warhead.as_deref(), Some("ChronoBeam"));
        assert!(rules.warhead("ChronoBeam").unwrap().temporal);
    }

    pub(crate) fn step(&mut self, command: Option<Command>) {
        let commands = command
            .into_iter()
            .map(|payload| {
                CommandEnvelope::new(
                    self.attacker.expect("command has its real owning house").0,
                    self.scene.sim().session().tick(),
                    payload,
                )
            })
            .collect::<Vec<_>>();
        let output = self
            .scene
            .runtime
            .advance_frame(
                &commands,
                crate::app::types::SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary bound frame");
        if !commands.is_empty() || !output.tick.frame_committed {
            eprintln!("ordinary frame output: {:?}", output.tick);
        }
        if !output.tick.frame_committed {
            self.log_house_state("terminal ordinary frame");
            self.log_attacker_state("terminal ordinary frame");
            self.log_slot_state("terminal ordinary frame");
            panic!("bounded command witness requires a continuing match");
        }
        if let Some((owner, attacker)) = self.attacker {
            for shot in output
                .fire_events
                .iter()
                .filter(|shot| shot.attacker_id == attacker)
            {
                eprintln!("ordinary CLEG fire event: {shot:?}");
            }
            let sim = self.scene.sim();
            if sim.entities().get(attacker).is_none()
                || sim
                    .houses()
                    .get(&owner)
                    .is_none_or(|house| house.is_defeated())
            {
                self.log_attacker_state("unexpected attacker loss after ordinary frame");
                self.log_slot_state("plant state at attacker loss");
                panic!("the admitted CLEG and its human house survive the bounded witness");
            }
        }
    }

    pub(crate) fn wait_for(&mut self, limit: usize, label: &str, ready: impl Fn(&Self) -> bool) {
        for elapsed in 0..limit {
            if ready(self) {
                return;
            }
            if self.attacker.is_some() && matches!(elapsed, 0 | 15 | 31 | 63 | 127) {
                self.log_attacker_state(&format!("{label}, waited {elapsed} frames"));
            }
            self.step(None);
        }
        if !ready(self) {
            self.log_house_state(label);
            self.log_attacker_state(label);
            self.log_slot_state(label);
        }
        assert!(ready(self), "{label} within {limit} ordinary frames");
    }

    fn slot(&self) -> Option<u64> {
        self.scene
            .sim()
            .entities()
            .get(self.plant)?
            .building_art
            .anim_slots()[3]
    }

    fn log_slot_state(&self, boundary: &str) {
        let sim = self.scene.sim();
        let Some(plant) = sim.entities().get(self.plant) else {
            eprintln!(
                "{boundary}: plant {} is absent at {}",
                self.plant,
                sim.session().binary_frame()
            );
            return;
        };
        let slot = self.slot();
        eprintln!(
            "{boundary}: clock={} plant={} owner={} defeated={:?} hp={} alive={} limbo={} placed={} up={} mission={:?} body={:?} ready={} online={} operational={:?} previous_operational={:?} slot={slot:?} runtime={:?}",
            sim.session().binary_frame(),
            self.plant,
            sim.interner().resolve(plant.owner()),
            sim.houses()
                .get(&plant.owner())
                .map(|house| house.is_defeated()),
            plant.health.current(),
            plant.lifecycle.object_alive(),
            plant.lifecycle.in_limbo(),
            plant.building_art.actually_placed(),
            plant.building_up(),
            plant.mission,
            plant.building_body_state(),
            plant.building_ready_latch(),
            plant.building_online(),
            sim.building_operational_state(self.plant, &self.scene.runtime.resources.rules),
            plant.building_last_operational,
            slot.and_then(|id| sim.anim(id)).map(|anim| anim.runtime())
        );
    }

    fn log_attacker_state(&self, boundary: &str) {
        let Some((owner, id)) = self.attacker else {
            return;
        };
        let sim = self.scene.sim();
        let house = sim.houses().get(&owner);
        let actor = sim.entities().get(id);
        eprintln!(
            "{boundary}: clock={} short_game={} attacker={id} owner={} defeated={:?} buildings={:?} actor={:?}",
            sim.session().binary_frame(),
            sim.session().game_options().short_game,
            sim.interner().resolve(owner),
            house.map(|house| house.is_defeated()),
            house.map(|house| house.tracking().buildings()),
            actor.map(|actor| (
                actor.health.current(),
                actor.lifecycle.object_alive(),
                actor.lifecycle.in_limbo(),
                &actor.position,
                actor.mission,
                actor.temporal.warp_target()
            ))
        );
        if let Some(actor) = actor {
            let error = sim.selected_weapon_fire_error(
                &self.scene.runtime.resources.rules,
                id,
                crate::sim::combat::TargetKind::Entity(self.plant),
                true,
                Some(&self.scene.runtime.resources.overlay_registry),
            );
            eprintln!(
                "{boundary}: attack_target={:?} navigation={:?} locomotor={:?} rearm={:?} last_fire={} facing={} pose={:?} GetFireError={error:?}",
                actor.attack_target,
                actor.navigation,
                actor.locomotor,
                actor.rearm_timer,
                actor.last_fire_frame.frame(),
                actor.body_facing_byte(sim.session().binary_frame()),
                actor.infantry_sprite_pose()
            );
        }
    }

    fn log_house_state(&self, boundary: &str) {
        let sim = self.scene.sim();
        eprintln!(
            "{boundary}: session clock={} tick={} started={} input_locked={} current_house={:?}",
            sim.session().binary_frame(),
            sim.session().tick(),
            sim.session().started(),
            sim.session().input_locked(),
            sim.session()
                .current_house()
                .map(|id| sim.interner().resolve(id))
        );
        for (id, house) in sim.houses().iter() {
            eprintln!(
                "{boundary}: house={} human={} passive={} defeated={} won={} lost={} outcome={:?} buildings={}",
                sim.interner().resolve(*id),
                house.is_human(),
                house.multiplay_passive(),
                house.is_defeated(),
                house.has_won(),
                house.has_lost(),
                house.outcome(),
                house.tracking().buildings()
            );
        }
    }

    fn log_attack_placement(&self, attacker: u64) {
        let sim = self.scene.sim();
        let actor = sim.entities().get(attacker).unwrap();
        let terrain = sim.resolved_terrain().unwrap();
        let bounds = sim.playfield_bounds().unwrap();
        let cell = (
            i32::from(actor.position.rx()),
            i32::from(actor.position.ry()),
        );
        let tile = terrain.cell(actor.position.rx(), actor.position.ry());
        let geometry = crate::sim::cell_rect::cell_is_in_playfield_geometry_only(cell, bounds);
        let height_aware = crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            cell,
            Some(bounds),
            Some(terrain),
        );
        let can_fire = crate::sim::combat::can_fire_at_target(
            sim,
            &self.scene.runtime.resources.rules,
            sim.interner(),
            attacker,
            &crate::sim::combat::TargetKind::Entity(self.plant),
            terrain,
            Some(sim.house_alliances()),
            &crate::sim::combat::line_of_fire::LineOfFireInputs {
                overlay_grid: sim.overlay_grid(),
                overlay_registry: Some(&self.scene.runtime.resources.overlay_registry),
                alliances: Some(sim.house_alliances()),
            },
        );
        // Observe the unchanged placement before the command: Approach's
        // in-range return also requires the stored height-aware membership.
        // This diagnostic deliberately neither filters nor repairs the cell.
        eprintln!(
            "before explicit Attack command: placement cell={cell:?} level_slope={:?} bounds={bounds:?} stored_in_playfield={} geometry_in_playfield={geometry} height_aware_in_playfield={height_aware} CanFireAt={can_fire} plant_position={:?}",
            tile.map(|tile| (tile.level, tile.slope_type)),
            actor.in_playfield(),
            sim.entities().get(self.plant).map(|plant| &plant.position)
        );
    }

    pub(crate) fn settled_playing_slot(&mut self) -> u64 {
        // GrandOpening has installed ActiveAnim, but GAPOWR's first ordinary
        // operational edge still runs 4549B0. ActiveAnimPoweredSpecial=yes
        // recreates that slot through 451890 (building_art_power.rs). Retain
        // the attack witness only after that real initial update, never by
        // constructing a replacement from the test.
        self.log_slot_state("after placement");
        let placed = self.slot().expect("normal placement creates ActiveAnim3");
        self.assert_slot(placed, false);
        self.step(None);
        self.log_slot_state("after first ordinary Building update");
        let slot = self
            .slot()
            .expect("ordinary startup leaves a live ActiveAnim3");
        if slot != placed {
            assert!(
                self.scene.sim().anim(placed).is_none(),
                "replaced startup slot retired"
            );
        }
        self.assert_slot(slot, false);
        let plant = self.scene.sim().entities().get(self.plant).unwrap();
        assert!(plant.building_art.actually_placed() && !plant.building_up());
        assert_eq!(
            self.scene
                .sim()
                .building_operational_state(self.plant, &self.scene.runtime.resources.rules),
            Some(true)
        );
        let first = self.scene.sim().anim(slot).unwrap().runtime().current_frame;
        for _ in 0..120 {
            // A later disappearance/replacement is a failure to investigate,
            // not permission to follow another slot or repair its runtime.
            if self.slot() != Some(slot) || self.scene.sim().anim(slot).is_none() {
                self.log_slot_state("unexpected slot retirement while awaiting initial AI");
            }
            self.assert_slot(slot, false);
            let runtime = self.scene.sim().anim(slot).unwrap().runtime();
            assert_eq!(runtime.loop_remaining, u8::MAX, "physical infinite loop");
            if runtime.current_frame != first {
                self.log_slot_state("settled real Anim AI progressed");
                return slot;
            }
            self.step(None);
        }
        self.log_slot_state("initial Anim AI did not progress");
        panic!("settled ActiveAnim3 progresses within 120 ordinary frames");
    }

    pub(crate) fn assert_slot(&self, expected: u64, erased: bool) {
        let sim = self.scene.sim();
        let plant = sim.entities().get(self.plant).expect("plant remains alive");
        assert!(plant.lifecycle.object_alive() && plant.health.current() > 0);
        assert_eq!(
            self.slot(),
            Some(expected),
            "no reconstructed slot can mask the transition"
        );
        assert_eq!(plant.is_warped_out(), erased);
        assert_eq!(plant.building_online(), !erased);
        let anim = sim.anim(expected).unwrap();
        assert_eq!(sim.interner().resolve(anim.type_id()), "GAPOWR_A");
        assert_eq!(anim.building_slot(), Some((self.plant, 3)));
        assert_eq!(anim.draw_flags(), 0x1600);
        assert_eq!(anim.z_adjust(), -32);
        assert_eq!(anim.runtime().temporal_paused, erased);
        assert_eq!(anim.draw_runtime().forced_translucent, erased);
        assert!(sim.display_layers().layer_of(expected).is_some());
        assert!(anim.in_logic_vector());
    }

    pub(crate) fn resume_after_power_restoration(&mut self, released_slot: u64) -> u64 {
        // LetGo71ABC0 -> 452210 releases the retained slot first. The next
        // Building43FB20 visit sees its false -> true operational edge and
        // 4549B0 recreates ActiveAnimPoweredSpecial through451890. This is
        // the separate building_power_dispatch.json boundary, not permission
        // to follow arbitrary slot replacement during the temporal hold.
        self.assert_slot(released_slot, false);
        let sim = self.scene.sim();
        let plant = sim.entities().get(self.plant).unwrap();
        assert_eq!(plant.temporal.chain_head(), None);
        // The sample is a transparent saved bool; its accessor is core-private.
        assert_eq!(
            serde_json::to_value(plant.building_last_operational).unwrap(),
            serde_json::Value::Bool(false)
        );
        assert_eq!(
            sim.building_operational_state(self.plant, &self.scene.runtime.resources.rules),
            Some(true)
        );
        self.log_slot_state("released slot before the operational restore edge");

        self.step(None);
        let replacement = self.slot().expect("restore edge creates ActiveAnim3");
        assert_ne!(
            replacement, released_slot,
            "PoweredSpecial recreates the slot"
        );
        let sim = self.scene.sim();
        assert!(sim.anim(released_slot).is_none(), "old slot retired");
        assert!(sim.display_layers().layer_of(released_slot).is_none());
        assert_eq!(
            serde_json::to_value(
                sim.entities()
                    .get(self.plant)
                    .unwrap()
                    .building_last_operational
            )
            .unwrap(),
            serde_json::Value::Bool(true)
        );
        self.assert_slot(replacement, false);
        self.log_slot_state("one ordinary Building visit restored PoweredSpecial slot");
        let first = sim.anim(replacement).unwrap().runtime().current_frame;
        self.wait_for(120, "restored real slot AI progresses", |fixture| {
            fixture.assert_slot(replacement, false);
            let runtime = fixture.scene.sim().anim(replacement).unwrap().runtime();
            assert_eq!(runtime.loop_remaining, u8::MAX, "physical infinite loop");
            runtime.current_frame != first
        });
        self.assert_slot(replacement, false);
        self.log_slot_state("restored slot progressed through real Anim AI");
        replacement
    }

    pub(crate) fn begin_attack(&mut self) -> u64 {
        assert!(self.attacker.is_none());
        let owner = self
            .scene
            .add_human_house(ATTACKER_OWNER, LaunchCountry::Russia);
        // Spawn only after the initial slot has played. No intervening frame
        // allows Guard's automatic scan to become the tested attack producer.
        let attacker = self
            .attacker_cells
            .iter()
            .copied()
            .find_map(|(x, y)| {
                self.scene
                    .runtime
                    .simulation
                    .spawn_object_with_overlay_registry(
                        "CLEG",
                        ATTACKER_OWNER,
                        x,
                        y,
                        64,
                        &self.scene.runtime.resources.rules,
                        &self.scene.runtime.resources.overlay_registry,
                    )
            })
            .expect("the same clear patch admits an in-range CLEG");
        assert_eq!(
            self.scene
                .sim()
                .entities()
                .get(attacker)
                .unwrap()
                .temporal
                .warp_target(),
            None
        );
        assert!(!crate::map::houses::is_allied_with(
            self.scene.sim().house_alliances(),
            ATTACKER_OWNER,
            PLANT_OWNER
        ));
        self.attacker = Some((owner, attacker));
        self.log_house_state("before explicit Attack command");
        self.log_attacker_state("before explicit Attack command");
        self.log_attack_placement(attacker);
        for id in [self.plant, attacker] {
            assert!(
                self.scene.sim().entities().get(id).unwrap().in_playfield(),
                "normal placement must establish stored playfield membership for actor {id}"
            );
        }
        self.step(Some(Command::Attack {
            attacker_id: attacker,
            target_id: self.plant,
        }));
        self.log_attacker_state("after explicit Attack command's ordinary frame");
        attacker
    }

    // Shared with the render GPU test through a cfg(test)-only re-export.
    // This helper accepts a production atlas and never manufactures a piece.
    pub(crate) fn emit_slot(
        &self,
        atlas: &crate::render::sprite_atlas::SpriteAtlas,
        light: &crate::map::lighting::CellLightGrid,
        viewport: [f32; 4],
        anim_word: &dyn Fn(u64) -> u16,
    ) -> Vec<PlannedObjectInstance> {
        let sim = self.scene.sim();
        let rules = &self.scene.runtime.resources.rules;
        let building = sim.entities().get(self.plant).unwrap();
        let viewer_id = sim.session().current_house();
        let viewer = viewer_id.map(|id| sim.interner().resolve(id));
        let mut observer = super::super::helpers::observer_draw_context(
            sim,
            building,
            viewer,
            viewer_id,
            Some(rules),
        );
        observer.drawn_voxel = Some(false);
        let color = HouseColorIndex(0); // add_human_house's launch color.
        let material = DrawState::for_building_slot_anims(
            building,
            sim.session().binary_frame(),
            super::super::units::house_color_to_remap_row(color),
            observer,
        );
        assert!(material.visible);
        let cell = (building.position.rx(), building.position.ry());
        let cells = sim
            .resolved_terrain()
            .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
        let extra = crate::app::presentation::lighting::body_extra_light(
            building,
            rules,
            sim,
            cells.as_ref(),
        );
        let attached_palette =
            crate::app::presentation::lighting::body_palette_light(light, cell, extra);
        let before = (sim.state_hash(), sim.rng_state());
        let mut output = Vec::new();
        emit_building_anims(
            &mut output,
            &NativeDisplayOrder::from_display(sim.display_layers()),
            atlas,
            rules.art(),
            Some(rules),
            "GAPOWR",
            color,
            light.building_body_tint_at(cell),
            attached_palette,
            light,
            cell,
            sim,
            building,
            anim_word,
            crate::render::batch::DepthAxis::NONE.origin_y,
            crate::render::batch::DepthAxis::NONE.world_height,
            viewport,
            material.state,
            2,
        );
        assert_eq!((sim.state_hash(), sim.rng_state()), before);
        output
    }
}

#[test]
#[ignore = "requires physical retail XMP03T4 assets; ordinary CLEG attack and Stop on a real GAPOWR active slot"]
fn retail_cleg_attack_freezes_and_releases_power_plant_slot() {
    let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut fixture = SlotScene::load(&root);
    let slot = fixture.settled_playing_slot();
    let attacker = fixture.begin_attack();
    fixture.wait_for(240, "ordinary temporal shot links the plant", |fixture| {
        fixture
            .scene
            .sim()
            .entities()
            .get(attacker)
            .unwrap()
            .temporal
            .warp_target()
            == Some(fixture.plant)
    });
    fixture.assert_slot(slot, true);
    assert_eq!(
        fixture
            .scene
            .sim()
            .entities()
            .get(fixture.plant)
            .unwrap()
            .temporal
            .chain_head(),
        Some(attacker)
    );
    let held_frame = fixture
        .scene
        .sim()
        .anim(slot)
        .unwrap()
        .runtime()
        .current_frame;
    let held_at = fixture.scene.sim().session().binary_frame();
    for _ in 0..12 {
        fixture.step(None);
        fixture.assert_slot(slot, true);
        assert_eq!(
            fixture
                .scene
                .sim()
                .anim(slot)
                .unwrap()
                .runtime()
                .current_frame,
            held_frame
        );
    }
    fixture.step(Some(Command::Stop {
        entity_id: attacker,
    }));
    // Stop does not replace Attack's retained dispatch timer. Let its genuine
    // due callback release the beam instead of demanding immediate unpause.
    fixture.wait_for(
        240,
        "retained Attack dispatch releases the beam",
        |fixture| {
            fixture
                .scene
                .sim()
                .entities()
                .get(attacker)
                .unwrap()
                .temporal
                .warp_target()
                .is_none()
        },
    );
    fixture.assert_slot(slot, false);
    assert_eq!(
        fixture
            .scene
            .sim()
            .entities()
            .get(fixture.plant)
            .unwrap()
            .temporal
            .chain_head(),
        None
    );
    let released_at = fixture.scene.sim().session().binary_frame();
    let released_frame = fixture
        .scene
        .sim()
        .anim(slot)
        .unwrap()
        .runtime()
        .current_frame;
    let replacement = fixture.resume_after_power_restoration(slot);
    eprintln!(
        "retail GAPOWR slot {slot}: held frame {held_frame} at {held_at}, released frame {released_frame} at {released_at}; the next operational edge created slot {replacement}, which progressed at {}; no native pixel comparison in this lifecycle witness",
        fixture.scene.sim().session().binary_frame()
    );
}
