//! Simulation-owned fresh Scenario bootstrap.
//!
//! Fresh families share one staged Simulation before terrain Fill. Active-stock
//! offline startup resolves its two House/start-callback passes; campaign startup
//! constructs and reads its authored House/Super array once. The same Scenario
//! RNG and native-ID owners then continue through terrain and map objects.
//! Opening forces, shroud, AI credits and diplomacy retain their shared owners.

#[cfg(all(test, tests_sim_p_s))]
#[path = "campaign_house_bootstrap_tests.rs"]
mod campaign_house_bootstrap_tests;

#[cfg(all(test, tests_sim_p_s))]
#[path = "scenario_house_name_retail_tests.rs"]
mod scenario_house_name_retail_tests;

#[cfg(all(test, tests_sim_p_s))]
#[path = "scenario_launch_identity_retail_tests.rs"]
mod scenario_launch_identity_retail_tests;

#[cfg(all(test, tests_sim_p_s))]
#[path = "scenario_fillsilos_retail_tests.rs"]
mod scenario_fillsilos_retail_tests;

#[cfg(all(test, tests_sim_p_s))]
#[path = "scenario_special_money_retail_tests.rs"]
mod scenario_special_money_retail_tests;

#[cfg(all(test, tests_sim_p_s))]
#[path = "building_ai_sale_tag_retail_tests.rs"]
mod building_ai_sale_tag_retail_tests;

#[cfg(any(test, feature = "test-support"))]
use std::collections::HashMap;

#[cfg(test)]
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseRoster;
#[cfg(test)]
use crate::map::map_file::MapFile;
#[cfg(any(test, feature = "test-support"))]
use crate::map::resolved_terrain::ResolvedTerrainGrid;
#[cfg(any(test, feature = "test-support"))]
use crate::map::waypoints::Waypoint;
use crate::rng_continuation::MapGenRngContinuation;
#[cfg(test)]
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
#[cfg(test)]
use crate::sim::house_state::HouseDifficulty;
#[cfg(test)]
use crate::sim::house_state::HouseState;
use crate::sim::houses::Houses;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioDescriptor;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::Simulation;
#[cfg(test)]
use crate::skirmish_launch::LaunchCountry;
#[cfg(test)]
use crate::skirmish_launch::LaunchStartPosition;
#[cfg(test)]
use crate::skirmish_launch::LaunchTeam;
#[cfg(any(test, feature = "test-support"))]
use crate::skirmish_launch::SkirmishLaunchSession;

mod launch_houses;
mod prefix_plan;
mod start_positions;
mod starting_objects;

#[cfg(any(test, feature = "test-support"))]
pub use launch_houses::apply_explicit_skirmish_launch_session;
pub(crate) use launch_houses::apply_skirmish_launch_alliances;
#[cfg(test)]
pub(crate) use launch_houses::native_ai_opening_grant;
pub use launch_houses::{
    LaunchController, MatchLaunchDescriptor, MatchNetworkStart, NormalizedSkirmishSlot,
    SkirmishLaunchApplyResult, UnresolvedShellChoice, apply_launch_shroud_option,
    apply_pre_fill_scenario_prefix_launch_session, apply_skirmish_ai_opening_credits,
    initialize_skirmish_launch_houses, launch_alliance_map, normalized_launch_slots,
    populate_launch_houses, populate_special_houses,
};
use prefix_plan::draw_house_constructor_timer;
use prefix_plan::finish_campaign_roster;
pub use prefix_plan::{
    AuthoredCampaignLaunch, AuthoredCampaignStart, BoundStockOfflineFreshPrefixPlan,
    FreshScenarioPrefix, FreshScenarioPrefixError, HOUSE_CONSTRUCTOR_TIMER_MAX,
    HOUSE_CONSTRUCTOR_TIMER_MIN, PreFillScenarioPrefixPlan, PreFillScenarioPrefixPlanError,
    StockOfflinePrefixProjection, StockOfflineStartCallbackFamily,
    prepare_stock_offline_scenario_prefix_plan, stock_offline_start_callback_family,
};
pub use start_positions::{
    NativeStartAssignment, NativeStartBounds, choose_battle_automatic_start, native_start_distance,
};
#[cfg(any(test, feature = "test-support"))]
pub use start_positions::{
    find_nearby_start_rect, native_assign_cooperative_launch_starts, native_assign_launch_starts,
    native_gather_start_positions,
};
use start_positions::{
    native_assign_cooperative_starts_from_preassignment,
    native_assign_launch_starts_from_preassignment,
    native_assign_quick_coop_starts_from_preassignment,
};
pub(crate) use start_positions::{
    native_gather_pre_fill_start_positions, native_preassign_launch_start_table,
};
#[cfg(any(test, feature = "test-support"))]
pub use starting_objects::seed_starting_extra_units;
pub use starting_objects::{
    STARTING_EXTRA_UNIT_FALLBACK_START_RADIUS, STARTING_EXTRA_UNIT_MAX_PLACEMENT_FAILURES,
    STARTING_MCV_FACING, STARTING_MCV_FALLBACK_MAX_RADIUS, StartingUnitCandidate,
    StartingUnitCandidates, launch_mcv_type_for_country, starting_unit_budget,
    starting_unit_candidates_for_country, starting_unit_prefers_vehicle,
};
use starting_objects::{
    ares_place_starting_construction_yard, ares_settle_starting_construction_yards,
    place_starting_mcv, place_unholy_base_units, seed_starting_extra_units_with_overlay_registry,
};

/// Opaque owner for the RNG cursors used while a map becomes a world.
///
/// The app may drive content-loading algorithms through the two narrow draw
/// wrappers and may transport the launch-generated MapGen continuation, but it
/// cannot replace, extract, or draw from those cursors. Consuming this owner is
/// the only production handoff into a freshly constructed Simulation.
pub struct ScenarioBootstrapRng {
    scenario: SimRng,
    main: SimRng,
    mapgen: Option<SimRng>,
}

/// Scenario-stream access granted only to the terrain Fill callback.
pub struct ScenarioFillRng<'a> {
    rng: &'a mut SimRng,
}

impl ScenarioFillRng<'_> {
    pub fn next_range_u32_inclusive(&mut self, low: u32, high: u32) -> u32 {
        self.rng.next_range_u32_inclusive(low, high)
    }
}

impl ScenarioBootstrapRng {
    pub fn new(seed: u32) -> Self {
        let seed = u64::from(seed);
        Self {
            scenario: SimRng::new(seed),
            main: SimRng::new(seed),
            mapgen: None,
        }
    }

    /// Install the previous process-global cursor exactly once. Authored
    /// loads retain it; a completed .SED generation has already published its
    /// newer cursor through the app's one process-retention owner.
    pub fn install_process_mapgen_continuation(&mut self, continuation: MapGenRngContinuation) {
        assert!(
            self.mapgen.is_none(),
            "process MapGen continuation may only be installed once"
        );
        self.mapgen = Some(SimRng::from_mapgen_continuation(continuation));
    }

    /// Install the already-resolved stock-offline pre-Fill prefix exactly once.
    #[cfg(any(test, feature = "test-support"))]
    pub fn install_pre_fill_scenario_prefix_plan(
        &mut self,
        plan: PreFillScenarioPrefixPlan,
    ) -> Result<StockOfflinePrefixProjection, PreFillScenarioPrefixPlanError> {
        plan.install_before_terrain(&mut self.scenario)
    }

    /// Consume the paired stock-offline RNG/native-ID prefix into the one staged
    /// Simulation before terrain Fill.
    pub fn into_stock_offline_staged_simulation(
        mut self,
        descriptor: &ScenarioDescriptor,
        bound_prefix: BoundStockOfflineFreshPrefixPlan,
    ) -> Result<(Simulation, StockOfflinePrefixProjection), PreFillScenarioPrefixPlanError> {
        let (scenario_prefix, native_id_prefix) = bound_prefix.into_parts();
        let start_header = scenario_prefix.start_header;
        let projection = scenario_prefix.install_before_terrain(&mut self.scenario)?;
        let mut simulation = self.into_simulation(descriptor);
        simulation.session.install_start_header(start_header);
        simulation.install_native_unique_id_cursor(native_id_prefix.into_cursor());
        Ok((simulation, projection))
    }

    /// Consume every fresh family through the same staged Simulation, before
    /// terrain Fill can draw or construct any map object. The campaign arm
    /// follows original Full_Init686B20: E/P types, one actual House/Super
    /// generation, current-House selection, then one Resize checkpoint.
    pub fn into_fresh_staged_simulation(
        self,
        descriptor: &ScenarioDescriptor,
        prefix: FreshScenarioPrefix<'_>,
    ) -> Result<(Simulation, Option<StockOfflinePrefixProjection>), FreshScenarioPrefixError> {
        match prefix {
            FreshScenarioPrefix::StockOffline(bound_prefix) => {
                let (simulation, projection) =
                    self.into_stock_offline_staged_simulation(descriptor, bound_prefix)?;
                Ok((simulation, Some(projection)))
            }
            FreshScenarioPrefix::Campaign {
                native_rules_receipt,
                rules,
                map_data,
                house_roster,
                difficulty,
                mission_counter,
                game_options,
                campaign_index,
                restart_carry_over,
                network,
            } => {
                NativeStartBounds::from_map_header(&map_data.header)
                    .ok_or(PreFillScenarioPrefixPlanError::InvalidMapCellExtent)?;
                if house_roster.houses.is_empty() {
                    return Err(FreshScenarioPrefixError::MissingCampaignHouseRoster);
                }
                // ReadHouseType475540 can allocate an unknown nonempty
                // Country at4755B8. That path must extend the live Rules
                // registry/ID owner; the selected stock route has only known
                // or missing tokens. Reject it before any House IDs/RNG,
                // rather than silently binding another Country's defaults.
                let mut names = std::collections::BTreeSet::new();
                for house in &house_roster.houses {
                    if !names.insert(house.name.to_ascii_lowercase()) {
                        return Err(FreshScenarioPrefixError::DuplicateCampaignHouse(
                            house.name.clone(),
                        ));
                    }
                    if rules
                        .scenario_country_name(house.country.as_deref())
                        .is_none()
                    {
                        return Err(match &house.country {
                            Some(country) => {
                                FreshScenarioPrefixError::UnsupportedCampaignCountryConstructor {
                                    house: house.name.clone(),
                                    country: country.clone(),
                                }
                            }
                            None => FreshScenarioPrefixError::MissingCampaignCountryRegistry,
                        });
                    }
                }
                let (early, rebuilt) = native_rules_receipt.into_parts();
                let (early_events, _) = early.into_parts();
                let (rebuilt_events, super_weapon_type_count) = rebuilt.into_parts();
                if super_weapon_type_count != rules.super_weapon_order.len() {
                    return Err(FreshScenarioPrefixError::SuperWeaponRegistryMismatch {
                        allocated: super_weapon_type_count,
                        projected: rules.super_weapon_order.len(),
                    });
                }
                // House4F6202..4F6290 constructs every registered Super via
                // 6CAF90, even when Type+B4 is -1 or has no implemented
                // dispatch body. The instance owner already uses this order;
                // launch support is not a prerequisite for House admission.
                // Evidence: input_oracle/campaign_start.py --houses (native
                // SuperType registration without its later body reader).
                let mut simulation = self.into_simulation(descriptor);
                let (difficulty, mission_counter, network_roster) = match network {
                    Some(launch) => (
                        launch.start.difficulty,
                        launch.start.mission_counter,
                        Some(
                            launch
                                .admit_sources(&mut simulation, house_roster, &map_data.ini)
                                .map_err(FreshScenarioPrefixError::AuthoredCampaign)?,
                        ),
                    ),
                    None => (difficulty, mission_counter, None),
                };
                let mut start_header = crate::map::preview::ScenarioStartHeader::default();
                start_header.read_ini(&map_data.ini);
                simulation.session.install_start_header(start_header);
                simulation.session.initialize_campaign_startup(
                    difficulty,
                    mission_counter,
                    game_options,
                    campaign_index,
                    restart_carry_over,
                    map_data.special_flags.fog_of_war,
                )?;
                simulation.install_native_unique_id_cursor(
                    crate::sim::native_identity::NativeUniqueIdCursor::begin_campaign_prefix(
                        early_events.len(),
                        rebuilt_events.len(),
                    ),
                );
                initialize_map_roster_houses(
                    &mut simulation,
                    house_roster,
                    rules,
                    Some(&map_data.ini),
                );
                if let Some((local_house, grants)) = network_roster {
                    finish_campaign_roster(&mut simulation, local_house, grants)
                        .map_err(FreshScenarioPrefixError::AuthoredCampaign)?;
                } else {
                    initialize_campaign_current_house(
                        &mut simulation.session,
                        &mut simulation.houses,
                        &map_data.ini,
                    );
                }
                initialize_campaign_scenario_side(&mut simulation, &map_data.ini, rules);
                simulation
                    .native_unique_ids
                    .as_mut()
                    .expect("campaign prefix installed the sole native-ID cursor")
                    .finish_campaign_prefix_after_resize(
                        map_data.header.width,
                        map_data.header.height,
                    )?;
                Ok((simulation, None))
            }
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn logical_states_for_test(
        &self,
    ) -> (
        crate::sim::rng::SimRngLogicalState,
        crate::sim::rng::SimRngLogicalState,
        Option<crate::sim::rng::SimRngLogicalState>,
    ) {
        (
            self.scenario.logical_state(),
            self.main.logical_state(),
            self.mapgen.as_ref().map(SimRng::logical_state),
        )
    }

    /// Finish the app-to-sim construction handoff with every bound cursor.
    pub fn into_simulation(self, descriptor: &ScenarioDescriptor) -> Simulation {
        let mut sim = Simulation::from_descriptor(descriptor);
        sim.install_terrain_load_advanced_scenario_rng(self.scenario);
        sim.install_variant_advanced_main_rng(self.main);
        if let Some(mapgen) = self.mapgen {
            sim.install_generated_mapgen_rng(mapgen);
        }
        sim
    }
}

fn replay_generated_construction_trace_with_rng(
    scenario: &mut SimRng,
    native_ids: &mut Option<crate::sim::native_identity::NativeUniqueIdCursor>,
    trace: &crate::map::construction_trace::RmgConstructionTrace,
) -> Result<crate::sim::world::GeneratedTechnoInitTable, crate::sim::world::GeneratedTechnoInitError>
{
    let mut emitted = Vec::new();
    for (expected_ordinal, event) in trace.events.iter().enumerate() {
        if event.ordinal != expected_ordinal {
            return Err(
                crate::sim::world::GeneratedTechnoInitError::TraceOrdinalMismatch {
                    expected: expected_ordinal,
                    found: event.ordinal,
                },
            );
        }
        let techno_ctor_random_word = (scenario.next_u32() & 0xFFFF) as u16;
        // Building43BA15 assigns after base Techno6F3254, before placement.
        // Discarded construction attempts spend both effects, like live rows.
        let native_unique_id =
            crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(native_ids);
        if let crate::map::construction_trace::RmgConstructionOutcome::Emitted {
            entity_index,
            cell,
        } = &event.outcome
        {
            emitted.push(crate::sim::game_entity::GeneratedTechnoInit {
                entity_index: *entity_index,
                techno_type: event.techno_type.clone(),
                cell: *cell,
                techno_ctor_random_word,
                native_unique_id,
            });
        }
    }
    crate::sim::world::GeneratedTechnoInitTable::try_new(emitted)
}

impl Simulation {
    /// Borrow the two load-time streams from the already-staged gameplay owner.
    /// No second bootstrap owner can exist after `into_simulation` consumes it.
    pub fn terrain_load_draws(&mut self) -> (ScenarioFillRng<'_>, &mut SimRng) {
        (
            ScenarioFillRng {
                rng: &mut self.scenario_rng,
            },
            &mut self.main_rng,
        )
    }

    /// Replay accepted launch-generation constructor effects on the staged
    /// Scenario stream after Fill, before authored object-section projection.
    ///
    /// Consume the launch generation's ordered Building constructor trace on
    /// the same Scenario owner that already passed through the stock-offline
    /// prefix and terrain Fill. Successful rows retain their low word for later
    /// projection; discarded rows spend the word and deliberately bind none.
    ///
    /// gamemd provenance: TechnoClass constructor 0x006F3254 consumes one raw
    /// Scenario word before RMG placement can succeed or fail. Launch reader
    /// 0x00684620 regenerates the accepted `.SED` after match RNG reseeding.
    pub fn replay_staged_generated_construction_trace(
        &mut self,
        trace: &crate::map::construction_trace::RmgConstructionTrace,
    ) -> Result<
        crate::sim::world::GeneratedTechnoInitTable,
        crate::sim::world::GeneratedTechnoInitError,
    > {
        if let Some(lighting) = trace.lighting {
            self.session
                .lighting_mut()
                .install_generated_lighting(lighting);
        }
        replay_generated_construction_trace_with_rng(
            &mut self.scenario_rng,
            &mut self.native_unique_ids,
            trace,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn gather_native_start_positions(
        &mut self,
        waypoints: &HashMap<u32, Waypoint>,
        participant_count: usize,
        terrain: &ResolvedTerrainGrid,
        bounds: NativeStartBounds,
    ) -> Vec<Waypoint> {
        let map_size_height = self.playfield_size_height();
        let binary_frame = self.session.binary_frame();
        native_gather_start_positions(
            waypoints,
            participant_count,
            terrain,
            &self.substrate.occupancy,
            bounds,
            self.playfield_bounds(),
            map_size_height,
            binary_frame,
            &mut self.scenario_rng,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn assign_native_battle_starts(
        &mut self,
        session: &SkirmishLaunchSession,
        starts: &[Waypoint],
    ) -> NativeStartAssignment {
        native_assign_launch_starts(session, starts, &mut self.scenario_rng)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn assign_native_cooperative_starts(
        &mut self,
        session: &SkirmishLaunchSession,
        starts: &[Waypoint],
        human_start_spots: usize,
    ) -> NativeStartAssignment {
        native_assign_cooperative_launch_starts(
            session,
            starts,
            human_start_spots,
            &mut self.scenario_rng,
        )
    }
}

/// Campaign68ACAA chooses Basic.Player with a 20-byte ReadString buffer,
/// byte-exact House name lookup50C170, and index0 fallback on a miss.
/// Source: original68ACAA..68AD18 and the consumed campaign_start_houses corpus.
/// Empty dev substrates retain unavailable identity; this is not a claim that
/// native can launch an empty House array.
pub fn initialize_campaign_current_house(
    session: &mut ScenarioSession,
    houses: &mut Houses,
    ini: &crate::rules::ini_parser::IniFile,
) {
    let current_house = campaign_player_house(houses, ini);
    session.install_current_house(current_house);
    if let Some(owner) = session.current_house() {
        // Original 68AD0C/68AD18 set the selected House's +1EC/+1ED.
        // This is scenario construction, never a snapshot/viewer rebind.
        let house = houses.get_mut(&owner).expect("registered current House");
        house.make_campaign_player();
    }
}

fn campaign_player_name(ini: &crate::rules::ini_parser::IniFile) -> String {
    ini.section("Basic")
        .map(|section| section.read_string("Player", "", 20))
        .unwrap_or_default()
}

fn campaign_player_house(
    houses: &Houses,
    ini: &crate::rules::ini_parser::IniFile,
) -> Option<crate::sim::intern::InternedId> {
    let requested = campaign_player_name(ini);
    // Interning folds lookup case and preserves the first spelling it saw.
    // The native strcmp reads the retained raw House name instead.
    Simulation::scenario_house_named(houses, &requested).or_else(|| houses.order().first().copied())
}

/// Full_Init687874..6878C5/68792A reads Basic.Player independently of the
/// current-House reader68ACAA: capacity32, default literal Americans, miss0.
/// House.Find50C170 and the retained Country reader remain their sole owners.
fn initialize_campaign_scenario_side(
    sim: &mut Simulation,
    ini: &crate::rules::ini_parser::IniFile,
    rules: &RuleSet,
) {
    let requested = ini.section("Basic").map_or_else(
        || "Americans".to_owned(),
        |section| section.read_string("Player", "Americans", 32),
    );
    let side = Simulation::scenario_house_named(&sim.houses, &requested)
        .and_then(|owner| sim.houses.get(&owner))
        .and_then(|house| rules.country_side_index_raw(sim.interner.resolve(house.house_type_id())))
        .unwrap_or(0);
    sim.session.install_scenario_side(side);
}

/// Map-roster house construction shared by app and headless (F09):
/// native order requires houses before every object section.
pub fn initialize_map_roster_houses(
    sim: &mut Simulation,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    map_ini: Option<&crate::rules::ini_parser::IniFile>,
) {
    assert!(
        sim.houses.is_empty()
            && sim.houses.order().is_empty()
            && sim.entities().is_empty()
            && sim.terrain_objects.objects().is_empty(),
        "scenario houses must be initialized before map objects"
    );
    let campaign_difficulties = sim.session.campaign_house_difficulties();
    if campaign_difficulties.is_some() {
        assert!(
            map_ini.is_some(),
            "campaign Houses require admitted map/rules"
        );
        assert!(
            sim.super_weapons.is_empty(),
            "campaign House Supers are constructed once"
        );
    }
    let frame = sim.session.binary_frame() as i32;
    let mission_counter = sim.session.campaign_mission_counter().unwrap_or(1);
    let campaign_role_houses = if campaign_difficulties.is_some() && sim.session.shares_houses() {
        sim.session
            .network_sources(house_roster.houses.len())
            .map(|(_, house)| house)
            .collect::<std::collections::BTreeSet<_>>()
    } else {
        std::collections::BTreeSet::new()
    };
    for house in &house_roster.houses {
        let country_name = rules
            .scenario_country_name(house.country.as_deref())
            .or(house.country.as_deref());
        let fallback_side = crate::sim::house_state::side_index_from_name(house.side.as_deref());
        let side_idx = crate::sim::house_state::resolve_house_side_index(
            rules,
            country_name,
            house.side.as_deref(),
            fallback_side,
        );
        let name_id = sim.interner.intern(&house.name);
        let country_id = country_name.map(|c| sim.interner.intern(c));
        let mut house_state = crate::sim::house_state::HouseState::new(
            name_id,
            side_idx,
            country_id,
            false,
            sim.session.game_options().starting_credits,
            sim.session.game_options().tech_level,
        );
        let campaign_house_id = campaign_difficulties.map(|_| {
            sim.next_native_load_id()
                .expect("admitted campaign native-ID cursor")
        });
        // Every native House constructs its whole Supers vector before map
        // objects. Only the admitted campaign prefix has a measured ID cursor.
        crate::sim::superweapon::construct_house_supers(
            sim,
            rules,
            name_id,
            campaign_house_id.is_some(),
        );
        if let Some((_, computer_difficulty)) = campaign_difficulties {
            let draw = draw_house_constructor_timer(&mut sim.scenario_rng);
            house_state.bind_native_construction(
                campaign_house_id.expect("admitted campaign House ID"),
                computer_difficulty,
                frame,
                draw,
                rules.general.attack_delay,
            );
        }
        // HouseReadINI500A62..500A7B overwrites the constructor's country ID
        // with the bounded roster entry before any object reader runs.
        house_state.initialize_scenario_name(&house.name);
        // Read_Scenario_INI 500C5D..500D05: a non-empty `UIName=` names the
        // House through the string table, else the type's UIName stays.
        // Residual: native copies with wcsncpy(.., 0x20), whose zero padding
        // runs past the 20-character field through +0x16069 (the colour
        // index +0x16054 that the later `Color=` read defaults from, and the
        // constructor's +0x16058..+0x16064) before terminating +0x16052.
        // VERA keeps the visible 20 characters and none of that overrun;
        // whether retail campaign maps set House `UIName=` is unchecked.
        if let Some(key) = &house.ui_name {
            house_state.set_ui_name(crate::sim::house_state::HouseUiName::Csf(key.clone()));
        }
        house_state.resolve_multiplay_passive(rules, country_name);
        house_state.project_country_mults(rules, &sim.interner);
        // Registration admits distinct House names only.
        sim.houses.register(house_state);
    }

    // Original5009B0 constructs the whole House/Super array before its
    // second loop calls ReadScenarioINI500B40 and SetDifficulty4F6EC0.
    // Cross-House map reads must therefore see every constructor at once.
    for (index, house) in house_roster.houses.iter().enumerate() {
        let name_id = sim.houses.order()[index];
        let house_state = sim
            .houses
            .get_mut(&name_id)
            .expect("constructed scenario House");
        let country_name = house_state
            .country()
            .map(|country| sim.interner.resolve(country));
        if rules.ares() {
            house_state.read_scenario_factory_plan(country_name.and_then(|country| {
                crate::sim::factory_owners::parent_or_self_country(rules, country)
            }));
        }
        house_state.assign_player_control(house.player_control == Some(true));
        // Admit lockstep seats before the credit tail of Read_Scenario_INI
        // (0x00500BDD..0x00500C57) and the player/computer row selection
        // (0x00500AAD..0x00500ADE). Keep the authored PlayerControl byte.
        if campaign_role_houses.contains(&index) {
            house_state.admit_campaign_role();
        }
        let policy = sim.session.rule_policy();
        let controlled_by_player = house_state.is_controlled_by_human(policy);
        if let Some(ini) = map_ini {
            house_state.initialize_scenario_parameters(
                house.read_scenario_parameters(ini, mission_counter),
                campaign_difficulties
                    .filter(|_| controlled_by_player)
                    .map(|(player, _)| player),
                Some(&rules.general),
            );
            house_state
                .team_creation_mut()
                .read_scenario_ratio(ini.section_or_empty(&house.name));
            // Ares hook 0x0050114D at the end of Read_Scenario_INI.
            if rules.ares() {
                house_state.read_ares_degrades(ini.section_or_empty(&house.name));
            }
            let temporary_delay = house_state
                .difficulty_value(&rules.general.team_delays)
                .wrapping_add((index as i32).wrapping_mul(175));
            house_state
                .team_creation_mut()
                .restart(frame, temporary_delay);
        }
        house_state.base_plan_mut().read_scenario(
            house.base_plan.percent_built,
            house
                .base_plan
                .nodes
                .iter()
                .map(|node| crate::sim::base_plan::BasePlanNode {
                    type_or_control: node.type_or_control,
                    packed_cell: node.packed_cell,
                    filled: node.filled,
                    retry_count: node.retry_count,
                }),
        );
        // HouseClass::Read_Scenario_INI reads `IQ=` from this exact named
        // house section, defaults it to zero, and changes a value above
        // MaxIQLevels to literal one before storing it as both the authored
        // IQ (+0x1D0) and CurrentIQ (+0x24C) (0x00500DBA..0x00500DC0).
        house_state.install_scenario_iq(house.scenario_current_iq(rules.general.max_iq_levels));
        if let Some((player, computer)) = campaign_difficulties {
            let difficulty = if controlled_by_player {
                player
            } else {
                computer
            };
            house_state.set_difficulty(
                difficulty,
                &rules.general,
                country_name.map_or_else(Default::default, |name| {
                    rules.country_difficulty_biases(name)
                }),
                policy,
                index as i32,
                frame,
            );
        }
    }
    if campaign_difficulties.is_some() {
        sim.install_house_alliances(
            crate::sim::house_diplomacy::HouseAllianceAdmission::ScenarioInitialization {
                roster: house_roster,
            },
            rules,
        );
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests;
