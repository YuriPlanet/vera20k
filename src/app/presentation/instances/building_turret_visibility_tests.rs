//! Real retail construction/sale state feeds the production building emitter.
//! Sentinel atlas entries test part presence, not raster or pixel equivalence.
use super::*;
use crate::map::{houses::HouseColorMap, lighting::CellLightGrid};
use crate::render::sprite_atlas::SpriteAtlas;
use crate::render::unit_atlas::{UnitAtlas, UnitSpriteEntry};
use crate::render::voxel_image_cache::VoxelImageCache;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::runtime::SimRuntime;
use std::cell::RefCell;

struct Patriot {
    runtime: SimRuntime,
    id: u64,
    buildup_control: [i32; 3],
    shp: SpriteAtlas,
    voxel: UnitAtlas,
}

impl Patriot {
    fn placed() -> Option<Self> {
        let mut rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
        let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
        crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
        rules.bind_building_buildup_assets(&assets, "TEMPERATE");
        let buildup_control = rules.buildup_control("NASAM");
        assert_eq!(buildup_control, [0, 8, 6]);
        let object = rules.object("NASAM").unwrap();
        assert!(object.turret_anim_is_voxel && !object.artillary);
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 43);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.spawn_object("GACNST", "Americans", 6, 10, 0, &rules)
            .unwrap();
        let owner = sim.interner().get("Americans").unwrap();
        let type_id = sim.interner().get("NASAM").unwrap();
        let id = crate::sim::production::complete_held_building_for_test(
            &mut sim, &rules, owner, type_id,
        );
        assert!(sim.apply_command(
            "Americans",
            &Command::PlaceReadyBuilding {
                type_id,
                rx: 10,
                ry: 10
            },
            Some(&rules),
        ));
        let mut runtime = SimRuntime::from_simulation(sim);
        runtime.resources_mut().rules = rules;
        let mut shp = SpriteAtlas::from_test_entries(Default::default());
        shp.make_frame_counts.insert("NASAM_MAKE".into(), 8);
        let entries = (0..32)
            .map(|facing| {
                (
                    UnitSpriteKey {
                        type_id: "NASAM".into(),
                        turret_index: 0,
                        facing,
                        layer: VxlLayer::Turret,
                        frame: 0,
                        slope_type: 0,
                        barrel_pitch: 0,
                    },
                    UnitSpriteEntry {
                        uv_origin: [0.0; 2],
                        uv_size: [1.0; 2],
                        pixel_size: [8.0; 2],
                        offset_x: 0.0,
                        offset_y: 0.0,
                        native_draw_bounds: Some([0, 0, 8, 8]),
                        page: 0,
                    },
                )
            })
            .collect();
        let mut voxel = UnitAtlas::from_test_entries(entries);
        voxel
            .frame_counts
            .insert(("NASAM".into(), VxlLayer::Turret, 0), 1);
        Some(Self {
            runtime,
            id,
            buildup_control,
            shp,
            voxel,
        })
    }

    fn step(&mut self) {
        self.runtime.simulation.advance_tick(
            &[],
            Some(&self.runtime.resources.rules),
            None,
            None,
            67,
        );
    }

    fn entity(&self) -> &crate::sim::game_entity::GameEntity {
        self.runtime.simulation.entities().get(self.id).unwrap()
    }

    fn complete(&mut self) {
        while self.entity().building_up() || !self.entity().building_art.actually_placed() {
            assert!(self.runtime.simulation.session().tick() < 100);
            self.step();
        }
        assert!(self.turret_drawn(), "completed Patriot keeps its turret");
    }

    fn turret_drawn(&self) -> bool {
        let lighting = CellLightGrid::new();
        let voxel_images = RefCell::new(VoxelImageCache::default());
        let colors = HouseColorMap::default();
        let input = ShpInputs {
            runtime: &self.runtime,
            atlas: Some(&self.shp),
            overlay_atlas: None,
            unit_atlas: Some(&self.voxel),
            voxel_images: &voxel_images,
            depth_tested_images: None,
            camera: [-1000.0; 2],
            render_size: [5000; 2],
            zoom: 1.0,
            building_viewport: crate::app::presentation::render::tactical_world_viewport(
                [-1000.0; 2],
                [0, 0, 5000, 5000],
                None,
            ),
            axis: DepthAxis::NONE,
            local_owner: None,
            ignore_visibility: true,
            lighting: &lighting,
            house_colors: &colors,
            building_zshape: false,
        };
        let order = NativeDisplayOrder::from_display(self.runtime.simulation.display_layers());
        let mut planned = Vec::new();
        build_shp_instances_from_inputs(&input, &mut planned, &order);
        planned
            .iter()
            .find(|object| object.parent.id == self.id)
            .expect("placed Patriot reaches the production emitter")
            .building_pieces
            .as_ref()
            .unwrap()
            .iter()
            .any(|piece| matches!(piece.target, ObjectTexture::UnitAtlasPage(_)))
    }
}

#[test]
fn retail_patriot_turret_appears_on_the_last_buildup_frame() {
    let Some(mut scene) = Patriot::placed() else {
        return;
    };
    let entity = scene.entity();
    assert_eq!(entity.building_body_state(), Some(0));
    assert!(!entity.building_art.actually_placed());
    assert_eq!(
        entity.mission.queued().known(),
        Some(crate::sim::mission::MissionType::Construction)
    );
    assert!(
        !scene.turret_drawn(),
        "queued Construction frame immediately after PLACE"
    );
    assert!(
        building_voxel_turret_visible(entity, true),
        "native Artillary bypasses the construction visibility gate"
    );
    let last = scene.buildup_control[0] + scene.buildup_control[1] - 1;
    scene
        .runtime
        .simulation
        .entities_mut_for_test()
        .get_mut(scene.id)
        .unwrap()
        .set_native_stage_value(last - 1);
    assert!(!scene.turret_drawn(), "penultimate buildup frame");
    scene
        .runtime
        .simulation
        .entities_mut_for_test()
        .get_mut(scene.id)
        .unwrap()
        .set_native_stage_value(last);
    assert!(scene.turret_drawn(), "last buildup frame");
    scene.complete();
}

#[test]
fn retail_patriot_sale_keeps_the_turret_at_stage_zero() {
    let Some(mut scene) = Patriot::placed() else {
        return;
    };
    scene.complete();
    let sim = &mut scene.runtime.simulation;
    let command = CommandEnvelope::new(
        sim.entities().get(scene.id).unwrap().owner(),
        sim.session().tick() + 1,
        Command::SellBuilding {
            entity_id: scene.id,
        },
    );
    sim.advance_tick(
        &[command],
        Some(&scene.runtime.resources.rules),
        None,
        None,
        67,
    );
    while !scene
        .runtime
        .simulation
        .entities()
        .get(scene.id)
        .unwrap()
        .in_construction_bstate()
    {
        assert!(scene.runtime.simulation.session().tick() < 120);
        scene.step();
    }
    assert_eq!(
        scene
            .runtime
            .simulation
            .entities()
            .get(scene.id)
            .unwrap()
            .construction_stage_value(),
        0
    );
    assert!(
        scene.turret_drawn(),
        "native 43DFA5 keeps the gun on the first sale frame"
    );
    while scene
        .runtime
        .simulation
        .entities()
        .get(scene.id)
        .unwrap()
        .construction_stage_value()
        == 0
    {
        assert!(scene.runtime.simulation.session().tick() < 140);
        scene.step();
    }
    assert!(!scene.turret_drawn(), "later sale stages hide the gun");
}
