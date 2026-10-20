//! Authoritative scenario initialization after map objects are installed.
//!
//! This owns the `ScenarioClass::Post_Map_Init @ 0x00686890` tail that must
//! complete before tick 0: native tiberium queue construction, navigation
//! publication, the skirmish AI credit grant, scenario-start crate RNG, the
//! final HouseClass alliance pass, Secret Lab rewards, and the post-`Full_Init` setup tail of
//! `FUN_00684C30` (per-load particle-system ID, OreTwinkle Scenario
//! draws). The app submits immutable map/session inputs and consumes only the
//! receipt.

use crate::map::houses::HouseRoster;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::crates::CratePlacement;
use crate::sim::world::Simulation;

/// Immutable inputs for the one-shot post-map scenario command.
pub(crate) struct ScenarioPostMapInput<'a> {
    pub(crate) map_width: u16,
    pub(crate) map_height: u16,
    /// Map Basic.FillSilos after the fresh Scenario reset(false).
    pub(crate) fill_silos: bool,
    pub(crate) rules: &'a RuleSet,
    pub(crate) overlay_registry: &'a OverlayTypeRegistry,
    pub(crate) house_roster: &'a HouseRoster,
    pub(crate) skirmish_session: Option<&'a crate::sim::scenario_bootstrap::MatchLaunchDescriptor>,
    /// Ordinary NewCampaign only: HomeCell selects the opening waypoint after
    /// Evade's entry reset. Continuation/save AltHome belongs to its later route.
    pub(crate) campaign_new_game_map: Option<&'a crate::map::map_file::MapFile>,
}

/// Presentation/logging facts returned after authoritative initialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenarioPostMapOutput {
    pub navigation_published: bool,
    pub crates: Option<CratePlacement>,
    pub(crate) ore_twinkle: crate::sim::ore_twinkle::OreTwinkleReceipt,
    pub opening_view_cell: Option<(u16, u16)>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) skirmish_order: [Option<ScenarioPostMapStep>; 3],
}

#[cfg(any(test, feature = "test-support"))]
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
        // Runtime rebuilds use this same sim-owned publication seam. Crate
        // placement below reads the newly published canonical grid.
        let navigation_published = self.rebuild_dynamic_navigation(input.rules);

        #[cfg(any(test, feature = "test-support"))]
        let mut skirmish_order = [None; 3];
        let crates = if let Some(descriptor) = input.skirmish_session {
            let session = descriptor.session();
            let player_count = crate::sim::crates::human_player_count(self);
            #[cfg(any(test, feature = "test-support"))]
            {
                skirmish_order[0] = Some(ScenarioPostMapStep::StartupCrates);
            }
            let normal_lighting = self
                .session
                .lighting()
                .profile(crate::sim::scenario_session::ScenarioLightingProfile::Normal)
                .into();
            let placement = crate::sim::crates::place_scenario_start_crates_with_lighting(
                self,
                input.rules,
                input.overlay_registry,
                player_count,
                normal_lighting,
            );
            // Startup OverlayClass::Mark completes synchronously before native
            // proceeds to AI credits; the placement refreshed the bridge
            // records and navigation from the now-final CellClass state.
            #[cfg(any(test, feature = "test-support"))]
            {
                skirmish_order[1] = Some(ScenarioPostMapStep::AiOpeningCredits);
            }
            crate::sim::scenario_bootstrap::apply_skirmish_ai_opening_credits(self, input.rules);
            #[cfg(any(test, feature = "test-support"))]
            {
                skirmish_order[2] = Some(ScenarioPostMapStep::LaunchAlliances);
            }
            crate::sim::scenario_bootstrap::apply_skirmish_launch_alliances(
                self,
                input.house_roster,
                session,
                input.rules,
            );
            Some(placement)
        } else {
            if self.session.campaign_mission_counter().is_none() {
                self.install_house_alliances(
                    crate::sim::house_diplomacy::HouseAllianceAdmission::Admitted(
                        input.house_roster.alliance_map(),
                    ),
                    input.rules,
                );
            }
            None
        };

        // Post_Map_Init686AFE calls68C050 after launch alliances, before
        // Full_Init's particle/OreTwinkle tail. Restore never reruns this pass.
        self.assign_secret_lab_production(input.rules);

        // ReadScenario684ACE → FillInData684F02..684F7F, before restamp
        // and the particle/OreTwinkle tail. Every nonnull House participates;
        // restore never enters this fresh-only owner or repeats its credits.
        if input.fill_silos {
            for owner in self.houses.order().to_vec() {
                let income = crate::sim::house_state::income_ppm_for_owner(
                    &self.houses,
                    &self.interner,
                    input.rules,
                    self.interner.resolve(owner),
                );
                let Some(house) = self.houses.get_mut(&owner) else {
                    continue;
                };
                // Native dereferences Tiberium[0], even if its Value is0.
                // Do not substitute a name/default or silently skip an invalid
                // empty registry on an enabled nonempty-House scenario.
                let value = input
                    .rules
                    .tiberium_types
                    .types()
                    .first()
                    .expect("native FillSilos requires Tiberium registry slot0")
                    .value;
                let capacity = house.tracking().storage_capacity();
                house
                    .economy_mut()
                    .fill_silos_fresh_empty_store(value, capacity, income);
            }
        }

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

        // Original684C30's opening-view choice consumes the already-parsed
        // Scenario HomeCell/waypoint table. NewCampaign52DF05→4C6140 clears
        // Evade, so this route uses HomeCell rather than AltHomeCell. Both
        // indexes reset to699 in SetDefaults683610 before the Basic read.
        let opening_view_cell = input.campaign_new_game_map.and_then(|map| {
            assert!(
                input.skirmish_session.is_none() && !self.session.game_mode_nonzero(),
                "campaign opening view requires the campaign family"
            );
            u32::try_from(map.basic.home_cell.unwrap_or(699))
                .ok()
                .map(|index| {
                    map.waypoints
                        .get(&index)
                        .map_or((0, 0), |waypoint| (waypoint.rx, waypoint.ry))
                })
        });

        // ReadScenario684ACE completes684C30 before684C05..684C1B captures
        // each House's initial Technos. Saved-world restore never enters this
        // fresh-only owner and therefore retains, rather than recaptures, them.
        Self::retain_initial_house_technos(&self.substrate, &mut self.houses);

        ScenarioPostMapOutput {
            navigation_published,
            crates,
            ore_twinkle,
            opening_view_cell,
            #[cfg(any(test, feature = "test-support"))]
            skirmish_order,
        }
    }

    /// Rebuild the derived bridge record vector after a crate `OverlayClass::Mark`
    /// batch. Native Mark mutates live CellClass state synchronously; Rust builds
    /// `BridgeRuntimeState` earlier in the load funnel, so both the startup batch
    /// and the per-tick regeneration rung refresh it from the now-final CellClass
    /// state.
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
        let (Some(bounds), Some(size_height)) =
            (self.playfield_bounds(), self.playfield_size_height())
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

    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};

    use crate::rules::ini_parser::IniFile;

    use crate::sim::overlay_grid::OverlayGrid;

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
        crate::map::resolved_terrain::test_grid(MAP_SIZE, MAP_SIZE, |rx, ry| ResolvedTerrainCell {
            speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            ..crate::map::resolved_terrain::test_tiberium_cell(rx, ry)
        })
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
        rules.replace_art_registry_for_test(art);
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
            fill_silos: false,
            rules,
            overlay_registry: overlays,
            house_roster,
            skirmish_session: None,
            campaign_new_game_map: None,
        }
    }

    #[test]
    fn campaign_opening_view_matches_original_new_game_basic_start_rows() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start.json",
        ))
        .unwrap();
        let (rules, overlays) = post_map_rules_and_overlays();
        let roster = HouseRoster::default();
        let mut compared = 0;
        for row in native["basic_start"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["alternate"].as_i64() == Some(0))
        {
            let mut map = crate::map::map_file::MapFile::from_bytes(
                b"[Map]\nSize=0,0,4,4\n[IsoMapPack5]\n1=CAAEABUAAAAAEQAA\n",
            )
            .unwrap();
            let ini = IniFile::from_sections_for_test(["Basic", "Waypoints"].map(|name| {
                let mut section = crate::rules::ini_parser::IniSection::new(name.to_string());
                for (key, value) in row["sections"][name].as_object().unwrap() {
                    section.set(key, value.as_str().unwrap());
                }
                section
            }));
            map.basic = crate::map::basic::parse_basic_section(&ini);
            map.waypoints = crate::map::waypoints::parse_waypoints(&ini);
            // Ordinary NewCampaign cleared Evade; a different AltHome must
            // not alter the selected HomeCell in this shared post-load owner.
            map.basic.alt_home_cell = Some(-1);
            let mut sim = Simulation::from_descriptor(&Default::default());
            sim.session.initialize_campaign_startup(1, 1, None).unwrap();
            let before = sim.rng_state();
            let mut input = generic_post_map_input(&rules, &overlays, &roster);
            input.campaign_new_game_map = Some(&map);
            let output = sim.finalize_scenario_post_map(input);
            assert_eq!(
                output.opening_view_cell,
                Some((
                    row["cell"][0].as_u64().unwrap() as u16,
                    row["cell"][1].as_u64().unwrap() as u16
                )),
                "{}",
                row["filename"]
            );
            assert_eq!(sim.rng_state(), before);
            compared += 1;
        }
        assert_eq!(
            compared, 2,
            "alternate1 remains continuation-bound evidence"
        );
    }

    #[test]
    fn campaign_post_map_keeps_the_once_installed_house_graph_and_constructor_cursors() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/campaign_start_houses.json",
        ))
        .unwrap();
        let row = native["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == "one_way_allies")
            .unwrap();
        assert_eq!(row["scenario_init"], 2);
        let ini = IniFile::from_sections_for_test(row["sections"].as_object().unwrap().iter().map(
            |(name, values)| {
                let mut section = crate::rules::ini_parser::IniSection::new(name.clone());
                for (key, value) in values.as_object().unwrap() {
                    section.set(key, value.as_str().unwrap());
                }
                section
            },
        ));
        let (rules, overlays) = post_map_rules_and_overlays();
        let roster = crate::map::houses::parse_house_roster(&ini, &[], Some(&rules));
        let mut sim = Simulation::new();
        sim.session.initialize_campaign_startup(1, 1, None).unwrap();
        crate::sim::scenario_bootstrap::initialize_map_roster_houses(
            &mut sim,
            &roster,
            Some(&rules),
            Some(&ini),
        );
        assert!(crate::map::houses::is_allied_with(
            sim.house_alliances.as_map(),
            "A",
            "B"
        ));
        assert!(!crate::map::houses::is_allied_with(
            sim.house_alliances.as_map(),
            "B",
            "A"
        ));
        let alliances = sim.house_alliances.as_map().clone();
        let order = sim.houses.order().to_vec();
        let house_ids: Vec<_> = order
            .iter()
            .map(|owner| sim.houses[owner].native_unique_id())
            .collect();
        let rng = sim.rng_state();
        let ids = sim.native_unique_ids.as_ref().unwrap().current_raw();
        let absent_late_roster = HouseRoster::default();
        let output = sim.finalize_scenario_post_map(generic_post_map_input(
            &rules,
            &overlays,
            &absent_late_roster,
        ));
        assert_eq!(sim.house_alliances.as_map(), &alliances);
        assert_eq!(sim.houses.order(), order);
        assert_eq!(
            order
                .iter()
                .map(|owner| sim.houses[owner].native_unique_id())
                .collect::<Vec<_>>(),
            house_ids
        );
        assert_eq!(sim.rng_state(), rng);
        // Original68503B constructs GasCloudSys even without OreTwinkle.
        // It spends one ID after the House pass, never another House/Super ID.
        assert_eq!(
            output.ore_twinkle,
            crate::sim::ore_twinkle::OreTwinkleReceipt {
                particle_system_id_consumed: true,
                ..Default::default()
            }
        );
        assert_eq!(
            sim.native_unique_ids.as_ref().unwrap().current_raw(),
            ids.wrapping_add(1)
        );
    }

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
        sim.session.set_map_width_for_test(MAP_SIZE);
        sim.session.set_map_height_for_test(MAP_SIZE);
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
                    (
                        anim.world_coord().x,
                        anim.world_coord().y,
                        anim.world_coord().z,
                    ),
                    anim.native_unique_id(),
                    anim.draw_flags(),
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
}
