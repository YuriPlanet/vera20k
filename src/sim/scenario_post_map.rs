//! Authoritative scenario initialization after map objects are installed.
//!
//! This owns the `ScenarioClass::Post_Map_Init @ 0x00686890` tail that must
//! complete before tick 0: native tiberium queue construction, navigation
//! publication, the skirmish AI credit grant, scenario-start crate RNG, the
//! final HouseClass alliance pass, and the post-`Full_Init` setup tail of
//! `FUN_00684C30` (per-load particle-system ID, OreTwinkle Scenario
//! draws). The app submits immutable map/session inputs and consumes only the
//! receipt.

use crate::map::basic::{BasicSection, SpecialFlagsSection};
use crate::map::houses::HouseRoster;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::crates::CratePlacement;
#[cfg(test)]
use crate::sim::ore_growth::NativeTiberiumRebuildStats;
use crate::sim::world::Simulation;
#[cfg(test)]
use crate::skirmish_launch::SkirmishLaunchSession;

/// Immutable inputs for the one-shot post-map scenario command.
pub(crate) struct ScenarioPostMapInput<'a> {
    pub(crate) map_width: u16,
    pub(crate) map_height: u16,
    pub(crate) basic: &'a BasicSection,
    pub(crate) special_flags: &'a SpecialFlagsSection,
    pub(crate) normal_lighting: crate::map::lighting::LightingProfileUnits,
    pub(crate) rules: &'a RuleSet,
    pub(crate) overlay_registry: &'a OverlayTypeRegistry,
    pub(crate) house_roster: &'a HouseRoster,
    pub(crate) skirmish_session: Option<&'a crate::sim::scenario_bootstrap::MatchLaunchDescriptor>,
}

/// Presentation/logging facts returned after authoritative initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScenarioPostMapOutput {
    pub(crate) navigation_published: bool,
    pub(crate) crates: Option<CratePlacement>,
    pub(crate) ore_twinkle: crate::sim::ore_twinkle::OreTwinkleReceipt,
    #[cfg(test)]
    pub(crate) skirmish_order: [Option<ScenarioPostMapStep>; 3],
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScenarioPostMapStep {
    StartupCrates,
    AiOpeningCredits,
    LaunchAlliances,
}

impl Simulation {
    /// Commit the post-map authority cone in native-observable order.
    ///
    /// This is a fresh-load-only command and is intentionally not idempotent:
    /// a second call would repeat the AI grant, crate RNG, and ore scheduler reset.
    /// Snapshot restore uses its narrower map-authority reconstruction path.
    pub(crate) fn finalize_scenario_post_map(
        &mut self,
        input: ScenarioPostMapInput<'_>,
    ) -> ScenarioPostMapOutput {
        // Retain the scenario's parsed ordinary lighting so the runtime crate
        // regeneration rung reaches `OverlayClass::Mark` with the same source
        // this startup pass uses.
        self.scenario_normal_lighting = input.normal_lighting;
        // Runtime rebuilds use this same sim-owned publication seam. Crate
        // placement below pins the newly published path snapshot.
        let mut navigation_published = self.rebuild_dynamic_navigation(input.rules);

        #[cfg(test)]
        let mut skirmish_order = [None; 3];
        let crates = if let Some(descriptor) = input.skirmish_session {
            let session = descriptor.session();
            let player_count = crate::sim::crates::human_player_count(self);
            let initial_path = self.path_grid_snapshot();
            #[cfg(test)]
            {
                skirmish_order[0] = Some(ScenarioPostMapStep::StartupCrates);
            }
            let placement = crate::sim::crates::place_scenario_start_crates_with_lighting(
                self,
                input.rules,
                input.overlay_registry,
                initial_path.as_deref(),
                player_count,
                input.normal_lighting,
            );
            // Startup OverlayClass::Mark completes synchronously before native
            // proceeds to AI credits. Rust's BridgeRuntimeState is a derived
            // cache built earlier in the load funnel, so rebuild it from the
            // now-final CellClass projection and publish matching first-frame
            // navigation without consuming OverlayGrid's dirty receipt.
            if self.refresh_bridge_runtime_after_crate_mark() {
                navigation_published = self.rebuild_dynamic_navigation(input.rules);
            }
            #[cfg(test)]
            {
                skirmish_order[1] = Some(ScenarioPostMapStep::AiOpeningCredits);
            }
            crate::sim::scenario_bootstrap::apply_skirmish_ai_opening_credits(self, input.rules);
            #[cfg(test)]
            {
                skirmish_order[2] = Some(ScenarioPostMapStep::LaunchAlliances);
            }
            crate::sim::scenario_bootstrap::apply_skirmish_launch_alliances(
                self,
                input.house_roster,
                session,
            );
            Some(placement)
        } else {
            self.house_alliances = input.house_roster.alliance_map();
            None
        };

        // Original684C30 performs586BF0 after the zone rebuilds and before its
        // particle/twinkle tail. This command is fresh-load-only: restore writes
        // saved real flags, and ordinary navigation rebuilds never rerun it.
        if let (Some(terrain), Some(bridges)) =
            (self.resolved_terrain.as_mut(), self.bridge_state.as_ref())
        {
            crate::sim::bridge_state::gap_restamp::restamp_inactive_high_records(
                terrain,
                bridges.endpoint_records(),
                |_, index, flags| {
                    if let Some(index) = index {
                        self.real_cell_bridge_flags_0x1180
                            .set_allocated_cell(index, flags);
                    }
                },
            );
        }

        // Native `FUN_00684C30` runs after `Full_Init` (and therefore after the
        // Post_Map_Init credit/crate/alliance work above): the GasCloudSys
        // particle-system ID, then the OreTwinkle Scenario draws.
        let ore_twinkle = self.run_post_load_ore_twinkle_pass(
            input.rules,
            input.overlay_registry,
            input.map_width,
            input.map_height,
        );

        ScenarioPostMapOutput {
            navigation_published,
            crates,
            ore_twinkle,
            #[cfg(test)]
            skirmish_order,
        }
    }

    /// Rebuild the derived bridge runtime cache after a crate `OverlayClass::Mark`
    /// batch. Native Mark mutates live CellClass state synchronously; Rust builds
    /// `BridgeRuntimeState` earlier in the load funnel, so both the startup batch
    /// and the per-tick regeneration rung refresh it from the now-final CellClass
    /// projection.
    pub(crate) fn refresh_bridge_runtime_after_crate_mark(&mut self) -> bool {
        let Some((destroyable, bridge_strength)) = self
            .bridge_state
            .as_ref()
            .map(|state| (state.is_destroyable(), state.bridge_strength()))
        else {
            return false;
        };
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let (Some(bounds), Some(size_height)) = (self.playfield_bounds, self.playfield_size_height)
        else {
            return false;
        };
        self.bridge_state = Some(
            crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain_with_map_size(
                terrain,
                destroyable,
                bridge_strength,
                (bounds.base, size_height),
            ),
        );
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;
    use std::fmt::Write as _;

    use crate::map::bridge_facts::BridgeCellFacts;
    use crate::map::houses::HouseDefinition;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    use crate::rules::house_colors::HouseColorIndex;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
    use crate::sim::ai::AiPlayerState;
    use crate::sim::house_state::HouseState;
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::skirmish_launch::{
        AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishAiSlot,
        SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLocalSlot,
    };

    const MAP_SIZE: u16 = 8;

    fn post_map_rules_and_overlays() -> (RuleSet, OverlayTypeRegistry) {
        let ini = IniFile::from_str(
            "[General]\n\
             TiberiumGrows=yes\n\
             TiberiumSpreads=yes\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Tiberiums]\n\
             0=Riparius\n\
             [Riparius]\n\
             Image=1\n\
             Growth=2200\n\
             GrowthPercentage=.06\n\
             Spread=2200\n\
             SpreadPercentage=.06\n\
             [OverlayTypes]\n\
             0=TIBCELL\n\
             1=WOOD\n\
             2=WATER\n\
             [TIBCELL]\n\
             Tiberium=yes\n\
             [WOOD]\n\
             Crate=yes\n\
             [WATER]\n\
             Crate=yes\n\
             [CrateRules]\n\
             CrateImg=WOOD\n\
             WoodCrateImg=WOOD\n\
             WaterCrateImg=WATER\n\
             CrateMinimum=1\n\
             CrateMaximum=1\n",
        );
        (
            RuleSet::from_ini(&ini).expect("post-map rules"),
            OverlayTypeRegistry::from_ini(&ini, None),
        )
    }

    fn flat_terrain() -> ResolvedTerrainGrid {
        let land_type = LandType::Clear.as_index();
        let speed_costs = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            float: Some(100),
            amphibious: Some(100),
            float_beach: Some(100),
            hover: Some(100),
        };
        let mut cells = Vec::with_capacity(MAP_SIZE as usize * MAP_SIZE as usize);
        for ry in 0..MAP_SIZE {
            for rx in 0..MAP_SIZE {
                cells.push(ResolvedTerrainCell {
                    rx,
                    ry,
                    source_tile_index: 0,
                    source_sub_tile: 0,
                    final_tile_index: 0,
                    final_sub_tile: 0,
                    is_wood_bridge_repair_tile: false,
                    level: 0,
                    filled_clear: false,
                    tileset_index: Some(0),
                    land_type,
                    yr_cell_land_type: land_type,
                    slope_type: 0,
                    template_height: 0,
                    render_offset_x: 0,
                    render_offset_y: 0,
                    terrain_class: TerrainClass::Clear,
                    speed_costs,
                    is_water: false,
                    is_cliff_like: false,
                    is_rough: false,
                    is_road: false,
                    accepts_smudge: true,
                    allows_tiberium: true,
                    height_in_pixels: 0,
                    variant: 0,
                    has_ramp: false,
                    canonical_ramp: None,
                    ground_walk_blocked: false,
                    terrain_object_blocks: false,
                    terrain_object_occupation: None,
                    overlay_blocks: false,
                    overlay_zone_type: None,
                    outside_playfield: false,
                    zone_type: zone_class::GROUND,
                    base_ground_walk_blocked: false,
                    base_build_blocked: false,
                    base_land_type: land_type,
                    base_yr_cell_land_type: land_type,
                    base_terrain_class: TerrainClass::Clear,
                    base_speed_costs: speed_costs,
                    build_blocked: false,
                    has_bridge_deck: false,
                    bridge_walkable: false,
                    bridge_transition: false,
                    bridge_deck_level: 0,
                    bridge_layer: None,
                    bridge_facts: BridgeCellFacts::default(),
                    tube_index: None,
                    radar_left: [0; 3],
                    radar_right: [0; 3],
                    has_damaged_data: false,
                    bridgehead_anchor_class_at_load: None,
                });
            }
        }
        ResolvedTerrainGrid::from_cells(MAP_SIZE, MAP_SIZE, cells)
    }

    fn twinkle_rules_and_overlays() -> (RuleSet, OverlayTypeRegistry) {
        let ini = IniFile::from_str(
            "[General]\n\
             OreTwinkle=TWNK1\n\
             TiberiumGrows=no\n\
             TiberiumSpreads=no\n\
             [AudioVisual]\n\
             OreTwinkleChance=2\n\
             [InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [Tiberiums]\n\
             0=Riparius\n\
             [Riparius]\n\
             Image=1\n\
             Value=25\n\
             [OverlayTypes]\n\
             0=TIBCELL\n\
             [TIBCELL]\n\
             Tiberium=yes\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("twinkle rules");
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
            "[TWNK1]\nLoopCount=-1\nRandomLoopDelay=120,300\nDetailLevel=2\nHideIfNoOre=true\nRate=450\n",
        ));
        art.bind_anim_frame_count_for_test("TWNK1", 8);
        rules.art_registry = art;
        (rules, OverlayTypeRegistry::from_ini(&ini, None))
    }

    fn generic_post_map_input<'a>(
        rules: &'a RuleSet,
        overlays: &'a OverlayTypeRegistry,
        house_roster: &'a HouseRoster,
    ) -> ScenarioPostMapInput<'a> {
        ScenarioPostMapInput {
            map_width: MAP_SIZE,
            map_height: MAP_SIZE,
            basic: &BASIC_DEFAULT,
            special_flags: &SPECIAL_FLAGS_DEFAULT,
            normal_lighting: crate::map::lighting::ParsedLightingProfiles::default().normal,
            rules,
            overlay_registry: overlays,
            house_roster,
            skirmish_session: None,
        }
    }

    static BASIC_DEFAULT: std::sync::LazyLock<BasicSection> =
        std::sync::LazyLock::new(BasicSection::default);
    static SPECIAL_FLAGS_DEFAULT: std::sync::LazyLock<SpecialFlagsSection> =
        std::sync::LazyLock::new(SpecialFlagsSection::default);

    /// `FUN_00684C30 @ 0x0068504D..0x006850F3`: one `RandomRanged(0, N-1)`
    /// Scenario draw per resource cell in `CellIterator` order, one native ID
    /// for the GasCloudSys particle system first, and one `AnimClass` per zero
    /// roll at the cell centre with draw flags 0x600.
    #[test]
    fn post_load_ore_twinkle_pass_rolls_each_resource_cell_in_native_order() {
        use crate::sim::native_identity::build_noncampaign_fresh_id_prefix;
        use crate::sim::ore_twinkle::OreTwinkleReceipt;
        use crate::sim::rng::SimRng;

        let (rules, overlays) = twinkle_rules_and_overlays();
        let seed = 0x0037_7EA5;
        let mut sim = Simulation::with_seed(seed);
        sim.session.map_width = MAP_SIZE;
        sim.session.map_height = MAP_SIZE;
        sim.resolved_terrain = Some(flat_terrain());
        let mut overlay_grid = OverlayGrid::new(MAP_SIZE, MAP_SIZE);
        // The (8,8) fixed-grid diamond admits only x+y > 8 here; the cells are
        // deliberately listed out of native order.
        let ore_cells: [(u16, u16); 4] = [(7, 7), (3, 7), (7, 2), (5, 4)];
        for (rx, ry) in ore_cells {
            overlay_grid.place_overlay(rx, ry, 0, 3);
        }
        // Outside the native diamond: never visited, never rolled.
        overlay_grid.place_overlay(1, 1, 0, 3);
        sim.overlay_grid = Some(overlay_grid);
        sim.native_unique_ids =
            Some(build_noncampaign_fresh_id_prefix(0, 0, 0, 0, 0, 0, 1, 1).into_cursor());
        let native_before = sim
            .native_unique_ids
            .as_ref()
            .expect("native cursor")
            .current_raw();
        let main_before = sim.main_rng.state();
        let roster = HouseRoster::default();

        let output =
            sim.finalize_scenario_post_map(generic_post_map_input(&rules, &overlays, &roster));

        let native_order: Vec<(u16, u16)> =
            crate::map::authored_overlay::NativeOverlayMapShape::new(8, 8)
                .recalc_cells()
                .into_iter()
                .filter_map(|(x, y)| {
                    let cell = (u16::try_from(x).ok()?, u16::try_from(y).ok()?);
                    ore_cells.contains(&cell).then_some(cell)
                })
                .collect();
        assert_eq!(native_order, vec![(5, 4), (7, 2), (3, 7), (7, 7)]);
        let mut expected = SimRng::new(seed);
        let mut expected_spawns = Vec::new();
        for cell in &native_order {
            if expected.next_range_u32_inclusive(0, 1) == 0 {
                expected_spawns.push(*cell);
            }
        }
        assert!(
            !expected_spawns.is_empty() && expected_spawns.len() < native_order.len(),
            "seed must exercise both roll outcomes: {expected_spawns:?}"
        );
        assert_eq!(sim.scenario_rng.state(), expected.state());
        assert_eq!(
            sim.main_rng.state(),
            main_before,
            "Main RNG is absent from this corridor"
        );
        assert_eq!(
            output.ore_twinkle,
            OreTwinkleReceipt {
                resource_cells_rolled: 4,
                spawned: expected_spawns.len() as u32,
                spawn_failures: 0,
                particle_system_id_consumed: true,
            }
        );
        let anims: Vec<((i32, i32, i32), i32, u32)> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| {
                (
                    (anim.world_coord.x, anim.world_coord.y, anim.world_coord.z),
                    anim.native_unique_id,
                    anim.draw_flags,
                )
            })
            .collect();
        let expected_anims: Vec<((i32, i32, i32), i32, u32)> = expected_spawns
            .iter()
            .enumerate()
            .map(|(index, (rx, ry))| {
                (
                    (i32::from(*rx) * 256 + 128, i32::from(*ry) * 256 + 128, 0),
                    // One ID for the particle system, then one per twinkle.
                    native_before.wrapping_add(2 + index as u32) as i32,
                    0x600,
                )
            })
            .collect();
        assert_eq!(anims, expected_anims);
    }

    /// A generated launch spends the particle-system ID from the synthetic
    /// `Full_Init`'s setup call (`0x00599A5B`); the post-`Post_Map_Init` setup
    /// then finds `DAT_00A8ED78` set and only draws twinkles.
    #[test]
    fn generated_launch_particle_id_precedes_constructors_and_post_map_skips_it() {
        use crate::sim::native_identity::build_noncampaign_fresh_id_prefix;

        let (rules, overlays) = twinkle_rules_and_overlays();
        let mut sim = Simulation::with_seed(0x0037_7EA7);
        sim.session.map_width = MAP_SIZE;
        sim.session.map_height = MAP_SIZE;
        sim.resolved_terrain = Some(flat_terrain());
        let mut overlay_grid = OverlayGrid::new(MAP_SIZE, MAP_SIZE);
        overlay_grid.place_overlay(7, 7, 0, 3);
        sim.overlay_grid = Some(overlay_grid);
        sim.native_unique_ids =
            Some(build_noncampaign_fresh_id_prefix(0, 0, 0, 0, 0, 0, 1, 1).into_cursor());
        let native_before = sim
            .native_unique_ids
            .as_ref()
            .expect("native cursor")
            .current_raw();

        assert!(sim.construct_post_load_particle_system_id());
        assert!(
            !sim.construct_post_load_particle_system_id(),
            "the object persists until the next Clear_Scene"
        );
        let after_synthetic_setup = sim
            .native_unique_ids
            .as_ref()
            .expect("native cursor")
            .current_raw();
        assert_eq!(after_synthetic_setup, native_before.wrapping_add(1));
        let generated_building_id = sim.next_native_load_id().expect("generator constructor");
        assert_eq!(generated_building_id, native_before.wrapping_add(2) as i32);

        let roster = HouseRoster::default();
        let output =
            sim.finalize_scenario_post_map(generic_post_map_input(&rules, &overlays, &roster));

        assert!(!output.ore_twinkle.particle_system_id_consumed);
        assert_eq!(output.ore_twinkle.resource_cells_rolled, 1);
        let spawned_ids: Vec<i32> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| anim.native_unique_id)
            .collect();
        assert_eq!(spawned_ids.len(), output.ore_twinkle.spawned as usize);
        for (index, id) in spawned_ids.iter().enumerate() {
            assert_eq!(*id, native_before.wrapping_add(3 + index as u32) as i32);
        }
    }

    /// `AnimClass::AI @ 0x00423AC0` rewrites `AnimClass+0x19D` every tick for
    /// `HideIfNoOre` types from the cell's `Get_Tiberium_Value`.
    #[test]
    fn ore_twinkle_hides_while_its_cell_has_no_ore_and_reappears_with_it() {
        let (rules, overlays) = twinkle_rules_and_overlays();
        let mut sim = Simulation::with_seed(0x0037_7EA8);
        sim.session.map_width = MAP_SIZE;
        sim.session.map_height = MAP_SIZE;
        sim.resolved_terrain = Some(flat_terrain());
        let mut overlay_grid = OverlayGrid::new(MAP_SIZE, MAP_SIZE);
        overlay_grid.place_overlay(7, 7, 0, 3);
        sim.overlay_grid = Some(overlay_grid);
        let type_id = sim.interner.intern("TWNK1");
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor::new(
            type_id,
            7,
            7,
            crate::util::fixed_math::SimFixed::from_num(128),
            crate::util::fixed_math::SimFixed::from_num(128),
            0,
        );
        let id = sim
            .spawn_load_anim_at_world(
                &rules.art_registry,
                descriptor,
                crate::sim::anim_class::AnimWorldCoord {
                    x: 7 * 256 + 128,
                    y: 7 * 256 + 128,
                    z: 0,
                },
                1,
            )
            .expect("TWNK1 spawns");
        let hidden = |sim: &Simulation| {
            sim.substrate
                .anims
                .get(id)
                .expect("anim")
                .draw_runtime
                .hidden
        };

        assert!(!hidden(&sim));
        sim.visit_anim(id, &rules, Some(&overlays));
        assert!(!hidden(&sim), "ore under the twinkle keeps it visible");

        *sim.overlay_grid.as_mut().unwrap().cell_mut(7, 7) = Default::default();
        sim.visit_anim(id, &rules, Some(&overlays));
        assert!(
            hidden(&sim),
            "harvested ore hides the twinkle within one AI visit"
        );
        assert!(
            sim.substrate.anims.get(id).is_some(),
            "HideIfNoOre suppresses drawing only; the anim keeps running"
        );

        sim.overlay_grid.as_mut().unwrap().place_overlay(7, 7, 0, 1);
        sim.visit_anim(id, &rules, Some(&overlays));
        assert!(!hidden(&sim), "regrown ore shows it again");

        *sim.overlay_grid.as_mut().unwrap().cell_mut(7, 7) = Default::default();
        sim.visit_anim(id, &rules, None);
        assert!(
            !hidden(&sim),
            "registry-less fixture visits leave the flag alone"
        );
    }

    #[test]
    fn post_load_ore_twinkle_pass_is_inert_without_the_rules_anim() {
        let (rules, overlays) = post_map_rules_and_overlays();
        let mut sim = Simulation::with_seed(0x0037_7EA6);
        sim.session.map_width = MAP_SIZE;
        sim.session.map_height = MAP_SIZE;
        sim.resolved_terrain = Some(flat_terrain());
        let mut overlay_grid = OverlayGrid::new(MAP_SIZE, MAP_SIZE);
        overlay_grid.place_overlay(7, 7, 0, 3);
        sim.overlay_grid = Some(overlay_grid);
        let scenario_before = sim.scenario_rng.state();
        let roster = HouseRoster::default();

        let output =
            sim.finalize_scenario_post_map(generic_post_map_input(&rules, &overlays, &roster));

        assert_eq!(output.ore_twinkle, Default::default());
        assert_eq!(sim.scenario_rng.state(), scenario_before);
        assert_eq!(sim.substrate.anims.iter().count(), 0);
    }

    fn allied_skirmish_session() -> SkirmishLaunchSession {
        SkirmishLaunchSession {
            mode: SkirmishLaunchMode {
                id: 1,
                ui_name_key: "GUI:Battle".to_string(),
                tooltip_key: "STT:ModeBattle".to_string(),
                override_file: "MPBattleMD.ini".to_string(),
                map_filter: "standard".to_string(),
                random_maps_allowed: true,
                allies_allowed: true,
                must_ally: false,
            },
            selected_map_file: Some("post-map-test.mmx".to_string()),
            player_name: "Player".to_string(),
            local: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: false,
                color_index: 1,
                color_random: false,
                start_position: LaunchStartPosition::Position(1),
                team: LaunchTeam::Team(0),
            },
            opponents: vec![SkirmishAiSlot {
                country: LaunchCountry::Russia,
                country_random: false,
                color_index: 2,
                color_random: false,
                start_position: LaunchStartPosition::Position(2),
                team: LaunchTeam::Team(0),
                difficulty: AiDifficulty::Easy,
            }],
            pre_fill_house_roster:
                crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(1),
            options: SkirmishLaunchOptions::default(),
        }
    }

    #[test]
    fn skirmish_post_map_finalizes_authority_in_one_call() {
        let (rules, overlays) = post_map_rules_and_overlays();
        let mut sim = Simulation::with_seed(0x51C0_0401);
        sim.session.map_width = MAP_SIZE;
        sim.session.map_height = MAP_SIZE;
        // The app installs normalized MapClass bounds before this production
        // post-map command. Model that prerequisite instead of relying on the
        // now-removed permissive headless fallback.
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 4,
            off_fc: -32,
            off_100: -32,
            off_104: 64,
            off_108: 64,
        });
        sim.playfield_size_height = Some(4);
        sim.session.game_options.crates = true;
        sim.resolved_terrain = Some(flat_terrain());
        sim.overlay_grid = Some(OverlayGrid::new(MAP_SIZE, MAP_SIZE));
        let tiberium = overlays.id_for_name("TIBCELL").expect("TIBCELL overlay");
        sim.overlay_grid
            .as_mut()
            .expect("overlay grid")
            .place_overlay(5, 5, tiberium, 10);

        let player = sim.interner.intern("Player");
        let computer = sim.interner.intern("Computer1");
        sim.houses
            .insert(player, HouseState::new(player, 0, None, true, 5_000, 10));
        sim.houses.insert(
            computer,
            HouseState::new(computer, 1, None, false, 5_000, 10),
        );
        sim.ai_players.push(AiPlayerState::new(computer));

        let mut expected_rng = sim.scenario_rng.clone();
        let expected_crate_cell = (
            expected_rng.next_range_u32_inclusive(1, u32::from(MAP_SIZE - 1)) as u16,
            expected_rng.next_range_u32_inclusive(1, u32::from(MAP_SIZE - 1)) as u16,
        );
        let _ = expected_rng.next_range_u32_inclusive(0, 0x7fff_fffe);
        let basic = BasicSection::default();
        let special_flags = SpecialFlagsSection::default();
        let roster = HouseRoster::default();
        let descriptor = crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(
            allied_skirmish_session(),
        )
        .expect("fixture session is fully resolved");

        // The queues are built where the load builds them, before the post-map
        // tail, which leaves them alone.
        let overlay_grid_for_queues = sim.overlay_grid.clone();
        let queue_stats = crate::sim::runtime::initialize_native_tiberium_queues(
            &mut sim,
            &basic,
            &special_flags,
            &rules,
            &overlays,
            overlay_grid_for_queues.as_ref(),
            (MAP_SIZE, MAP_SIZE),
        );
        let output = sim.finalize_scenario_post_map(ScenarioPostMapInput {
            map_width: MAP_SIZE,
            map_height: MAP_SIZE,
            basic: &basic,
            special_flags: &special_flags,
            normal_lighting: crate::map::lighting::ParsedLightingProfiles::default().normal,
            rules: &rules,
            overlay_registry: &overlays,
            house_roster: &roster,
            skirmish_session: Some(&descriptor),
        });

        assert_eq!(
            queue_stats,
            Some(NativeTiberiumRebuildStats {
                growth_entries: 1,
                spread_entries: 1,
            })
        );
        let native = sim.production.ore_growth_state.native_tiberium_state();
        assert_eq!(native.classes.len(), 1);
        assert_eq!(native.classes[0].growth_bitmap, BTreeSet::from([(5, 5)]));
        assert_eq!(native.classes[0].spread_bitmap, BTreeSet::from([(5, 5)]));
        assert!(output.navigation_published);
        assert!(sim.path_grid().is_some());
        assert_eq!(sim.houses[&player].economy.credits, 5_000);
        // The fixture AI is Easy and the fixture rules carry no MultiplayerAICM
        // entry for it, so Post_Map_Init adds ftol(0 * 0.01 * 5000) = 0.
        assert_eq!(sim.houses[&computer].economy.credits, 5_000);
        assert_eq!(
            output.crates,
            Some(CratePlacement {
                requested: 1,
                accepted: 1,
                visible: 1,
            })
        );
        let wood = overlays.id_for_name("WOOD").expect("WOOD overlay");
        let crate_cells: Vec<_> = sim
            .overlay_grid
            .as_ref()
            .expect("overlay grid")
            .iter_occupied()
            .filter_map(|(rx, ry, cell)| (cell.overlay_id == Some(wood)).then_some((rx, ry)))
            .collect();
        assert_eq!(crate_cells, vec![expected_crate_cell]);
        assert_eq!(sim.scenario_rng.state(), expected_rng.state());
        assert_eq!(
            output.skirmish_order,
            [
                Some(ScenarioPostMapStep::StartupCrates),
                Some(ScenarioPostMapStep::AiOpeningCredits),
                Some(ScenarioPostMapStep::LaunchAlliances),
            ]
        );
        assert!(
            sim.house_alliances
                .get("PLAYER")
                .is_some_and(|allies| allies.contains("COMPUTER1"))
        );
        assert!(
            sim.house_alliances
                .get("COMPUTER1")
                .is_some_and(|allies| allies.contains("PLAYER"))
        );
    }

    #[test]
    fn authored_post_map_preserves_preinitialized_tiberium_queues() {
        let (rules, overlays) = post_map_rules_and_overlays();
        let mut sim = Simulation::with_seed(0x51C0_0403);
        sim.session.map_width = MAP_SIZE;
        sim.session.map_height = MAP_SIZE;
        sim.resolved_terrain = Some(flat_terrain());
        sim.overlay_grid = Some(OverlayGrid::new(MAP_SIZE, MAP_SIZE));
        let tiberium = overlays.id_for_name("TIBCELL").expect("TIBCELL overlay");
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .place_overlay(5, 5, tiberium, 10);
        sim.production.ore_growth_state =
            crate::sim::ore_growth::OreGrowthState::new(MAP_SIZE, MAP_SIZE);
        let seeded = sim
            .production
            .ore_growth_state
            .rebuild_native_tiberium_queues_from_overlays(
                sim.overlay_grid.as_ref().unwrap(),
                &overlays,
                &rules.tiberium_types,
                sim.resolved_terrain.as_ref(),
                &BTreeSet::new(),
                true,
                true,
                sim.session.binary_frame,
                (MAP_SIZE, MAP_SIZE),
            );
        assert_eq!(seeded.growth_entries, 1);
        assert_eq!(seeded.spread_entries, 1);

        sim.finalize_scenario_post_map(ScenarioPostMapInput {
            map_width: MAP_SIZE,
            map_height: MAP_SIZE,
            basic: &BasicSection::default(),
            special_flags: &SpecialFlagsSection::default(),
            normal_lighting: crate::map::lighting::ParsedLightingProfiles::default().normal,
            rules: &rules,
            overlay_registry: &overlays,
            house_roster: &HouseRoster::default(),
            skirmish_session: None,
        });

        let native = sim.production.ore_growth_state.native_tiberium_state();
        assert_eq!(native.classes[0].growth_bitmap, BTreeSet::from([(5, 5)]));
        assert_eq!(native.classes[0].spread_bitmap, BTreeSet::from([(5, 5)]));
    }

    #[test]
    fn startup_high_bridge_reaches_bridge_runtime_and_initial_navigation() {
        let mut ini_text = String::from(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [OverlayTypes]\n",
        );
        for overlay_id in 0..=0x18u8 {
            let name = if overlay_id == 0x18 {
                "BRIDGE1".to_string()
            } else {
                format!("OV{overlay_id:03}")
            };
            writeln!(&mut ini_text, "{overlay_id}={name}").unwrap();
        }
        ini_text.push_str(
            "[BRIDGE1]\nCrate=yes\n\
             [CrateRules]\nCrateImg=BRIDGE1\nWoodCrateImg=BRIDGE1\n\
             WaterCrateImg=BRIDGE1\nCrateMinimum=1\nCrateMaximum=1\n",
        );
        let ini = IniFile::from_str(&ini_text);
        let rules = RuleSet::from_ini(&ini).expect("high startup crate rules");
        let overlays = OverlayTypeRegistry::from_ini(&ini, None);
        let mut sim = Simulation::with_seed(0x51C0_0414);
        sim.session.map_width = MAP_SIZE;
        sim.session.map_height = MAP_SIZE;
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 4,
            off_fc: -32,
            off_100: -32,
            off_104: 64,
            off_108: 64,
        });
        sim.playfield_size_height = Some(4);
        sim.session.game_options.crates = true;
        let terrain = flat_terrain();
        sim.bridge_state = Some(
            crate::sim::bridge_state::BridgeRuntimeState::from_resolved_terrain(
                &terrain, true, 300,
            ),
        );
        sim.resolved_terrain = Some(terrain);
        sim.overlay_grid = Some(OverlayGrid::new(MAP_SIZE, MAP_SIZE));
        let player = sim.interner.intern("Player");
        sim.houses
            .insert(player, HouseState::new(player, 0, None, true, 5_000, 10));
        let descriptor = crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(
            allied_skirmish_session(),
        )
        .expect("fixture session is fully resolved");

        // The queues are built where the load builds them, before the post-map
        // tail, which leaves them alone.
        let overlay_grid_for_queues = sim.overlay_grid.clone();
        let _ = crate::sim::runtime::initialize_native_tiberium_queues(
            &mut sim,
            &BasicSection::default(),
            &SpecialFlagsSection::default(),
            &rules,
            &overlays,
            overlay_grid_for_queues.as_ref(),
            (MAP_SIZE, MAP_SIZE),
        );
        let output = sim.finalize_scenario_post_map(ScenarioPostMapInput {
            map_width: MAP_SIZE,
            map_height: MAP_SIZE,
            basic: &BasicSection::default(),
            special_flags: &SpecialFlagsSection::default(),
            normal_lighting: crate::map::lighting::ParsedLightingProfiles::default().normal,
            rules: &rules,
            overlay_registry: &overlays,
            house_roster: &HouseRoster::default(),
            skirmish_session: Some(&descriptor),
        });

        assert_eq!(
            output.crates,
            Some(CratePlacement {
                requested: 1,
                accepted: 1,
                visible: 1,
            })
        );
        let anchor = sim
            .crate_authority
            .occupied_cells()
            .next()
            .expect("one startup crate anchor");
        let bridge_cell = sim
            .bridge_state
            .as_ref()
            .and_then(|state| state.cell(anchor.0, anchor.1))
            .expect("startup high anchor reaches BridgeRuntimeState");
        assert!(bridge_cell.deck_present);
        assert_eq!(bridge_cell.overlay_byte, 0x18);
        let path_cell = sim
            .path_grid()
            .and_then(|grid| grid.cell(anchor.0, anchor.1))
            .expect("republished initial path cell");
        assert!(path_cell.bridge_structural);
        assert!(path_cell.bridge_walkable);
        assert!(
            !sim.overlay_grid
                .as_ref()
                .unwrap()
                .pending_dirty_cells()
                .is_empty(),
            "bridge/nav rebuild must not consume the first-frame overlay receipt"
        );
    }

    #[test]
    fn generic_post_map_preserves_rng_and_credits_when_overlay_authority_is_absent() {
        let (rules, overlays) = post_map_rules_and_overlays();
        let mut sim = Simulation::with_seed(0x51C0_0402);
        sim.session.map_width = MAP_SIZE;
        sim.session.map_height = MAP_SIZE;
        sim.resolved_terrain = Some(flat_terrain());

        let owner = sim.interner.intern("HouseA");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, false, 7_500, 10));
        sim.ai_players.push(AiPlayerState::new(owner));
        let rng_before = sim.scenario_rng.state();
        let roster = HouseRoster {
            houses: vec![
                HouseDefinition {
                    name: "HouseA".to_string(),
                    color: HouseColorIndex(0),
                    country: None,
                    side: None,
                    player_control: Some(false),
                    iq: None,
                    allies: vec!["HouseB".to_string()],
                    base_plan: Default::default(),
                },
                HouseDefinition {
                    name: "HouseB".to_string(),
                    color: HouseColorIndex(1),
                    country: None,
                    side: None,
                    player_control: Some(true),
                    iq: None,
                    allies: Vec::new(),
                    base_plan: Default::default(),
                },
            ],
        };

        // The queues are built where the load builds them, before the post-map
        // tail, which leaves them alone.
        let overlay_grid_for_queues = sim.overlay_grid.clone();
        let _ = crate::sim::runtime::initialize_native_tiberium_queues(
            &mut sim,
            &BasicSection::default(),
            &SpecialFlagsSection::default(),
            &rules,
            &overlays,
            overlay_grid_for_queues.as_ref(),
            (MAP_SIZE, MAP_SIZE),
        );
        let output = sim.finalize_scenario_post_map(ScenarioPostMapInput {
            map_width: MAP_SIZE,
            map_height: MAP_SIZE,
            basic: &BasicSection::default(),
            special_flags: &SpecialFlagsSection::default(),
            normal_lighting: crate::map::lighting::ParsedLightingProfiles::default().normal,
            rules: &rules,
            overlay_registry: &overlays,
            house_roster: &roster,
            skirmish_session: None,
        });

        assert!(output.navigation_published);
        assert_eq!(output.crates, None);
        assert_eq!(output.skirmish_order, [None; 3]);
        assert_eq!(sim.scenario_rng.state(), rng_before);
        assert_eq!(sim.houses[&owner].economy.credits, 7_500);
        assert!(
            sim.production
                .ore_growth_state
                .native_tiberium_state()
                .classes
                .is_empty()
        );
        assert!(
            sim.house_alliances
                .get("HOUSEA")
                .is_some_and(|allies| allies.contains("HOUSEB"))
        );
        assert!(
            sim.house_alliances
                .get("HOUSEB")
                .is_some_and(|allies| allies.contains("HOUSEA"))
        );
    }
}
