//! Skirmish launch houses: normalized launch slots, the match launch
//! descriptor, House population for launch and special houses, alliances,
//! shroud option and the AI opening credit grant.

use super::{
    NativeStartAssignment, NativeStartBounds, PreFillScenarioPrefixPlanError,
    StockOfflinePrefixProjection, ares_place_starting_construction_yard,
    ares_settle_starting_construction_yards, launch_mcv_type_for_country, place_starting_mcv,
    place_unholy_base_units, seed_starting_extra_units_with_overlay_registry,
};
use crate::map::houses::{HouseRoster, normalize_house_name};
use crate::map::map_file::MapFile;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::waypoints::Waypoint;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::{HouseDifficulty, HouseState};
use crate::sim::world::Simulation;
use crate::skirmish_launch::{
    LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishLaunchSession,
};
use crate::util::native_x87::{NativeF64Bits, X87Chop53};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishLaunchApplyResult {
    pub spawned_mcvs: u32,
    pub active_slots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedSkirmishSlot {
    pub owner_name: String,
    pub(crate) country: LaunchCountry,
    pub color_index: u8,
    pub(crate) start_position: LaunchStartPosition,
    pub(crate) team: LaunchTeam,
    pub is_human: bool,
    pub observer: bool,
    /// The humans who control this House, its leader first; empty for AI.
    /// VERA shared control: players on one start share a House.
    pub controllers: Vec<LaunchController>,
    /// A human node's handle, CreateHouses' source for House+0x1602A.
    pub handle: Option<String>,
    pub(crate) difficulty: HouseDifficulty,
    pub driver_policy: Option<crate::sim::driver::DriverPolicy>,
    /// The commander this house must get, if any.
    ///
    /// Reserved seam: no UI offers it yet, so every launch path passes `None`
    /// and no house gets a commander at all. A selector, `spawn.ini`, or a map
    /// with preset AI sets it to a `[AIPersonalities]` name, to `Standard`
    /// (native behaviour, no clips) or to `none`, and
    /// [`crate::rules::personality::personality_for_house`] does the rest.
    pub commander: Option<String>,
}

/// One human controlling a launch House.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchController {
    /// The human's index in `SkirmishLaunchSession::humans`.
    pub human_index: usize,
    /// The sender byte of this human's command records: its place in the
    /// color-sorted human order (CreateHouses687F66), which is its House
    /// index unless players share Houses.
    pub source: i8,
}

impl NormalizedSkirmishSlot {
    /// The human node this House was built for (688137), its leader.
    pub fn source_human_index(&self) -> Option<usize> {
        self.controllers
            .first()
            .map(|controller| controller.human_index)
    }

    pub fn is_controlled_by(&self, human_index: usize) -> bool {
        self.controllers
            .iter()
            .any(|controller| controller.human_index == human_index)
    }

    /// The launch team this House was assigned on, read by the app's
    /// presentation layer (e.g. the grouped superweapon timer panel) to group
    /// by the real teams instead of the mutable alliance graph.
    pub fn team(&self) -> LaunchTeam {
        self.team
    }
}

/// Sim-owned behavioral launch descriptor (F09).
///
/// Wraps a `SkirmishLaunchSession` whose shell-random choices — country and
/// color, for the local slot and every AI slot — are proven resolved. The
/// validating constructor is the only way in, so sim entry points cannot
/// receive an unresolved frontend session and silently launch with the
/// placeholder country/color a random slot still carries. Start positions
/// remain potentially random by design: gamemd assigns them at scenario load
/// with the gameplay Scenario RNG (see
/// `prepare_stock_offline_scenario_prefix_plan`), not in the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchLaunchDescriptor {
    pub(super) session: SkirmishLaunchSession,
    rule_policy_override: Option<crate::rules::rule_policy::RulePolicy>,
    control_grants: Vec<crate::sim::control_authority::ControlGrant>,
    personal_rules: Option<crate::rules::personal_rules::PersonalRules>,
    network_start: Option<MatchNetworkStart>,
    /// WOL Quick Co-op (`[0xA8B244] == 2`) at scenario load: the start
    /// callback is AssignStartingPoints `0x005EE9D0`.
    pub(super) quick_coop_starts: bool,
}

/// Deterministic lockstep settings installed before House construction.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MatchNetworkStart {
    pub max_ahead_frames: Option<i32>,
    pub tournament: i32,
    pub squad_lists: [Vec<String>; 2],
}

impl MatchNetworkStart {
    /// Requires an already admitted lockstep execution topology.
    pub fn install(&self, sim: &mut Simulation) {
        if let Some(max_ahead_frames) = self.max_ahead_frames {
            sim.install_network_timing(max_ahead_frames);
        }
        sim.install_wol_start(self.tournament, self.squad_lists.clone());
    }
}

/// A launch slot still carried an unresolved random shell choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnresolvedShellChoice {
    /// `None` = the local player slot; `Some(i)` = AI opponent index `i`.
    pub ai_slot: Option<usize>,
    /// Which choice was left random: `"country"` or `"color"`.
    pub choice: &'static str,
}

impl std::fmt::Display for UnresolvedShellChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.ai_slot {
            None => write!(f, "local slot still has a random {}", self.choice),
            Some(index) => write!(f, "AI slot {index} still has a random {}", self.choice),
        }
    }
}

impl MatchLaunchDescriptor {
    /// Validate that the app's shell close transaction resolved every
    /// random choice before the session crosses into sim.
    pub fn from_resolved(session: SkirmishLaunchSession) -> Result<Self, UnresolvedShellChoice> {
        for human in &session.humans {
            if human.slot.country_random {
                return Err(UnresolvedShellChoice {
                    ai_slot: None,
                    choice: "country",
                });
            }
            if human.slot.color_random {
                return Err(UnresolvedShellChoice {
                    ai_slot: None,
                    choice: "color",
                });
            }
        }
        for (index, opponent) in session.opponents.iter().enumerate() {
            if opponent.country_random {
                return Err(UnresolvedShellChoice {
                    ai_slot: Some(index),
                    choice: "country",
                });
            }
            if opponent.color_random {
                return Err(UnresolvedShellChoice {
                    ai_slot: Some(index),
                    choice: "color",
                });
            }
        }
        Ok(Self {
            session,
            rule_policy_override: None,
            control_grants: Vec::new(),
            personal_rules: None,
            network_start: None,
            quick_coop_starts: false,
        })
    }

    /// Use admitted custom rules while constructing and running the world.
    pub fn with_rule_policy(mut self, policy: crate::rules::rule_policy::RulePolicy) -> Self {
        self.rule_policy_override = Some(policy);
        self
    }

    pub fn rule_policy_override(&self) -> Option<crate::rules::rule_policy::RulePolicy> {
        self.rule_policy_override
    }

    /// Supply grants in resolved source and House order. The constructed
    /// scenario validates these indices before admitting the initial world.
    pub fn with_control_grants(
        mut self,
        grants: Vec<crate::sim::control_authority::ControlGrant>,
    ) -> Self {
        self.control_grants = grants;
        self
    }

    pub(crate) fn control_grants(&self) -> &[crate::sim::control_authority::ControlGrant] {
        &self.control_grants
    }

    pub fn personal_rules(&self) -> Option<&crate::rules::personal_rules::PersonalRules> {
        self.personal_rules.as_ref()
    }

    pub fn set_personal_rules(&mut self, rules: crate::rules::personal_rules::PersonalRules) {
        self.personal_rules = Some(rules);
    }

    pub fn network_start(&self) -> Option<&MatchNetworkStart> {
        self.network_start.as_ref()
    }

    pub fn set_network_start(&mut self, start: MatchNetworkStart) {
        self.network_start = Some(start);
    }

    /// Assign starts as WOL Quick Co-op does (`0x0068755E`).
    pub fn with_quick_coop_starts(mut self) -> Self {
        self.quick_coop_starts = true;
        self
    }

    /// Full_Init6876AC deletes all HouseTypes (6689C3..6689E5), then
    /// rebuilds them before CreateHouses687FBD indexes the final array without
    /// a bounds gate. Reject a disappeared shell allocation rather than
    /// reconstructing it or entering the native undefined-pointer domain.
    pub fn validate_countries(
        &self,
        rules: &RuleSet,
    ) -> Result<(), PreFillScenarioPrefixPlanError> {
        for slot in normalized_launch_slots(&self.session) {
            if rules.country_name(slot.country.index()).is_none() {
                return Err(PreFillScenarioPrefixPlanError::MissingLaunchCountry {
                    index: slot.country.raw_index(),
                });
            }
        }
        Ok(())
    }

    /// The resolved session's gameplay facts.
    pub fn session(&self) -> &SkirmishLaunchSession {
        &self.session
    }
}

/// Construct the active offline-skirmish House array before any map object.
///
/// Active YR `ScenarioClass__Full_Init @ 0x00686B20` calls
/// `ScenarioClass__Create_Houses @ 0x00687F10` before terrain and Techno map
/// sections. The standard offline order is the human participant, AI slots,
/// then Neutral and Special. Object construction and Reveal can therefore
/// commit the house tracking counts and house-indexed base reservations
/// directly, without a repair pass.
pub fn initialize_skirmish_launch_houses(
    sim: &mut Simulation,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    descriptor: &MatchLaunchDescriptor,
) {
    let session = descriptor.session();
    assert!(
        sim.houses.is_empty()
            && sim.houses.order().is_empty()
            && sim.entities().is_empty()
            && sim.terrain_objects.objects().is_empty(),
        "skirmish houses must be initialized before map objects"
    );
    let slots = normalized_launch_slots(session);
    sim.session
        .install_game_options(session.options.to_game_options(session.ai_player_count));
    populate_launch_houses(sim, &slots, rules);
    // 688109 installs the original participant-array index0. These normalized
    // slots retain that launch identity; later MCV placement/viewer choice
    // cannot clear or replace the current house.
    let current_house = slots
        .iter()
        .find(|slot| slot.is_controlled_by(session.local_player_index))
        .map(|slot| {
            let id = sim
                .interner
                .get(&slot.owner_name)
                .expect("created launch house");
            assert!(sim.houses.contains_key(&id));
            id
        });
    sim.session.install_current_house(current_house);
    // Full_Init68782D/687938 takes the local participant's Country.Side,
    // before the color-ordered House array is consulted. Native source node0
    // is the current local node (CreateHouses6880F8..68810F); VERA represents
    // that identity by local_player_index, independently of host node order.
    // A player who shares a House plays its leader's Country.
    let scenario_side = slots
        .iter()
        .find(|slot| slot.is_controlled_by(session.local_player_index))
        .map(|slot| {
            let name = rules
                .country_name(slot.country.index())
                .expect("accepted local Country exists");
            rules.country_side_index_raw(name).unwrap_or(0)
        })
        .unwrap_or(0);
    sim.session.install_scenario_side(scenario_side);
    install_shared_house_sources(sim, &slots, session.local_player_index);
    let observers = slots
        .iter()
        .filter(|slot| slot.observer)
        .filter_map(|slot| sim.interner.get(&slot.owner_name))
        .collect();
    sim.session
        .install_multiplayer_house_identity(session.mode.class, observers);
    // 688109..10F sets player-control only for the local source node;
    // remote human Houses remain human without acquiring local control.
    for slot in &slots {
        if let Some(id) = sim.interner.get(&slot.owner_name) {
            sim.houses
                .get_mut(&id)
                .expect("created launch House")
                .assign_player_control(slot.is_controlled_by(session.local_player_index));
        }
    }
    populate_special_houses(sim, house_roster, rules);
    // VERA: "mod AI" seats (RFC 0001 6i). The Wasm modules were installed
    // before the houses existed, so their claims are given here.
    sim.seat_mod_ai_houses(&session.mod_ai_house_names());
}

/// VERA shared control: when players share Houses, record which House each
/// command sender controls and this process's own sender. Otherwise every
/// sender is its House index, as in gamemd, and nothing is installed.
fn install_shared_house_sources(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    local_player_index: usize,
) {
    let controllers = slots.iter().enumerate().flat_map(|(house, slot)| {
        slot.controllers
            .iter()
            .map(move |controller| (controller, house))
    });
    if controllers
        .clone()
        .all(|(controller, house)| usize::try_from(controller.source) == Ok(house))
    {
        return;
    }
    let mut source_houses = vec![0; controllers.clone().count()];
    let mut local_source = None;
    for (controller, house) in controllers {
        source_houses[controller.source as usize] =
            u8::try_from(house).expect("human Houses come first");
        if controller.human_index == local_player_index {
            local_source = Some(controller.source);
        }
    }
    sim.session
        .install_shared_house_sources(source_houses, local_source);
}

/// Commit the explicit launch-session diplomacy graph at the post-crate boundary.
/// RESIDUAL (invisible): in GameMode 4 native MakeAlly runs its live tail at
/// launch (`0x004F9C1F..0x004F9C28`), so Paranoid gives each computer one
/// anger point per human allying; VERA installs the graph without it.
pub(crate) fn apply_skirmish_launch_alliances(
    sim: &mut Simulation,
    session: &SkirmishLaunchSession,
    rules: &RuleSet,
) {
    let slots = normalized_launch_slots(session);
    let rule_policy = sim.session.rule_policy();
    sim.install_house_alliances(
        crate::sim::house_diplomacy::HouseAllianceAdmission::Admitted(launch_alliance_map(
            &slots,
            rule_policy,
        )),
        rules,
    );
}

/// Test-only compatibility path for direct post-Fill launch fixtures.
#[cfg(any(test, feature = "test-support"))]
pub fn apply_explicit_skirmish_launch_session(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    descriptor: &MatchLaunchDescriptor,
) -> SkirmishLaunchApplyResult {
    apply_resolved_skirmish_launch_session(
        sim,
        map_data,
        house_roster,
        rules,
        resolved_terrain,
        descriptor,
        None,
        LaunchStartResolution::PostFillTestCompatibility,
    )
}

/// Apply the retained stock-offline House/start projection without repeating
/// any prefix draw. Existing starting-force and later initialization draws
/// remain after this projection.
pub fn apply_pre_fill_scenario_prefix_launch_session(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    descriptor: &MatchLaunchDescriptor,
    plan: &StockOfflinePrefixProjection,
) -> SkirmishLaunchApplyResult {
    apply_resolved_skirmish_launch_session(
        sim,
        map_data,
        house_roster,
        rules,
        resolved_terrain,
        descriptor,
        Some(rules.overlay_registry()),
        LaunchStartResolution::Prefix(plan),
    )
}

enum LaunchStartResolution<'a> {
    Prefix(&'a StockOfflinePrefixProjection),
    #[cfg(any(test, feature = "test-support"))]
    PostFillTestCompatibility,
}

fn apply_resolved_skirmish_launch_session(
    sim: &mut Simulation,
    map_data: &MapFile,
    house_roster: &HouseRoster,
    rules: &RuleSet,
    resolved_terrain: &ResolvedTerrainGrid,
    descriptor: &MatchLaunchDescriptor,
    overlay_registry: Option<&OverlayTypeRegistry>,
    start_resolution: LaunchStartResolution<'_>,
) -> SkirmishLaunchApplyResult {
    // Direct frontend/unit-test launch paths can enter before the shared
    // scenario construction funnel. Retail has already completed
    // `0x00654490 -> MapClass::Set_Clipped_LocalSize @ 0x00567230` before
    // Battle start gathering and placement consume the playfield fields.
    sim.install_playfield_from_map_header(&map_data.header);

    let session = descriptor.session();
    let slots = normalized_launch_slots(session);
    if sim.houses.is_empty() {
        // Direct unit-level callers may enter before the shared app load funnel.
        // The initializer rejects any already-constructed map object, so this
        // fallback cannot recreate the former object-before-house production path.
        initialize_skirmish_launch_houses(sim, house_roster, rules, descriptor);
    }
    assert_eq!(
        sim.houses.order().len(),
        slots.len() + 2,
        "launch House array must be complete before start assignment"
    );
    for (registered, slot) in sim.houses.order().iter().zip(&slots) {
        assert_eq!(
            sim.interner.resolve(*registered),
            slot.owner_name,
            "participant House order changed after map construction"
        );
    }
    for (registered, expected) in sim.houses.order()[slots.len()..]
        .iter()
        .zip(["Neutral", "Special"])
    {
        assert!(
            sim.interner
                .resolve(*registered)
                .eq_ignore_ascii_case(expected),
            "special House order changed after map construction"
        );
    }

    let bounds = NativeStartBounds::from_session(sim, resolved_terrain);
    let (_starts, start_assignment) = match start_resolution {
        LaunchStartResolution::Prefix(plan) => {
            // The same immutable table already drove the first loading markers.
            // Its complete House/Gather/assignment prefix was installed before
            // Fill, so projecting it here cannot draw or replace the live cursor.
            (
                plan.final_gathered_starts().to_vec(),
                plan.assignment().clone(),
            )
        }
        #[cfg(any(test, feature = "test-support"))]
        LaunchStartResolution::PostFillTestCompatibility => {
            let cooperative = session.mode.class == crate::skirmish_modes::MpModeClass::Cooperative;
            // Direct unit fixtures historically enter with a constructed map.
            // Keep their compatibility resolver outside production compilation,
            // while still modeling two independent Gather callbacks.
            let _first_gather = sim.gather_native_start_positions(
                &map_data.waypoints,
                slots.len(),
                resolved_terrain,
                bounds,
            );
            let starts = sim.gather_native_start_positions(
                &map_data.waypoints,
                slots.len(),
                resolved_terrain,
                bounds,
            );
            let assignment = if cooperative {
                let mut header = crate::map::preview::ScenarioStartHeader::default();
                header.read_ini(&map_data.ini);
                sim.session.install_start_header(header);
                let human_start_spots =
                    sim.session.start_header().coop_human_start_spots().max(0) as usize;
                sim.assign_native_cooperative_starts(session, &starts, human_start_spots)
            } else {
                sim.assign_native_battle_starts(session, &starts)
            };
            (starts, assignment)
        }
    };
    project_pre_fill_start_assignment(sim, &slots, &start_assignment);
    let assignments = &start_assignment.placements;
    let mut spawned_mcvs = 0;

    let ares = rules.ares_form();
    // Unholy Alliance's own base-unit callback has no Bases test.
    let unholy = session.mode.class == crate::skirmish_modes::MpModeClass::Unholy;
    // Ares 3.0 MPGameModeClass_CreateStartingUnits_BaseCenter (1001ED60,
    // hook 0x5D6F61) wraps the base-unit callback of every house, so its
    // construction-yard pass also runs with Bases off.
    let visit_houses = if ares {
        true
    } else {
        session.options.bases || unholy
    };
    if visit_houses {
        for (slot_idx, waypoint) in assignments {
            let Some(slot) = slots.get(*slot_idx) else {
                continue;
            };
            let Some(country) = crate::sim::house_state::house_state_for_owner(
                &sim.houses,
                &slot.owner_name,
                &sim.interner,
            )
            .and_then(|house| house.country())
            .and_then(|name| rules.country_index(sim.interner.resolve(name))) else {
                continue;
            };
            // GenerateStartingUnits5D6EF0..5D6EF8 skips passive Countries
            // before either the MCV (+C8) or extra-unit (+CC) callback.
            if slot.observer
                || rules
                    .country_name(country)
                    .is_some_and(|name| rules.country_multiplay_passive(name))
            {
                continue;
            }
            // 1001ED60 keeps BaseSpawnCell (+0x5490) from before the callback.
            let saved_spawn_cell = if ares {
                crate::sim::house_state::house_state_for_owner(
                    &sim.houses,
                    &slot.owner_name,
                    &sim.interner,
                )
                .and_then(|house| house.base_center())
            } else {
                None
            };
            if unholy {
                let (placed, complete) = place_unholy_base_units(
                    sim,
                    &slot.owner_name,
                    waypoint.rx,
                    waypoint.ry,
                    bounds,
                    rules,
                    resolved_terrain,
                    overlay_registry,
                );
                spawned_mcvs += placed;
                if !complete {
                    // The callback fails (0x005CB523..0x005CB52B).
                    continue;
                }
            } else if session.options.bases {
                // Ares 3.0 1001F0B0 (hook 0x5D7048): a WithConst country's
                // house starts with a construction yard instead.
                let yard: Option<bool> = if ares {
                    ares_place_starting_construction_yard(
                        sim,
                        &slot.owner_name,
                        bounds,
                        rules,
                        resolved_terrain,
                        overlay_registry,
                    )
                } else {
                    None
                };
                if let Some(placed) = yard {
                    if !placed {
                        // The callback fails (0x5D70DB).
                        continue;
                    }
                    spawned_mcvs += 1;
                } else {
                    // Battle5D7062 skips Unit construction and its RNG when505310
                    // finds no Owner-compatible BaseUnit. The House still exists.
                    // Ares 3.0 MPGameMode_SpawnBaseUnit_BaseUnit 10023C80 (hook
                    // 0x5D705E) keeps that and only logs.
                    let Some(mcv_type) = launch_mcv_type_for_country(
                        LaunchCountry::from_country_index(u32::from(country.0)),
                        rules,
                    ) else {
                        continue;
                    };
                    if place_starting_mcv(
                        sim,
                        mcv_type,
                        &slot.owner_name,
                        waypoint.rx,
                        waypoint.ry,
                        bounds,
                        rules,
                        resolved_terrain,
                        overlay_registry,
                    )
                    .is_some()
                    {
                        spawned_mcvs += 1;
                    } else {
                        log::warn!(
                            "Failed to seed session MCV '{}' for {} at waypoint {} ({},{})",
                            mcv_type,
                            slot.owner_name,
                            waypoint.index,
                            waypoint.rx,
                            waypoint.ry
                        );
                        continue;
                    }
                }
            }
            if ares {
                ares_settle_starting_construction_yards(
                    sim,
                    &slot.owner_name,
                    saved_spawn_cell,
                    rules,
                );
            }
        }

        if spawned_mcvs > 0 {
            log::info!(
                "Seeded {} session skirmish MCV(s) for {} active slot(s)",
                spawned_mcvs,
                slots.len()
            );
        }
    }

    seed_starting_extra_units_with_overlay_registry(
        sim,
        &slots,
        rules,
        resolved_terrain,
        bounds,
        session.options.unit_count,
        map_data
            .special_flags
            .initial_veteran
            .unwrap_or(rules.initial_veteran),
        overlay_registry,
    );

    // The selected-mode starting-force orchestrator advances Scenario RNG once
    // after the complete house pass, including zero-budget runs.
    let _ = sim.scenario_rng.next_range_u32_inclusive(0, 0xffff);

    apply_launch_shroud_option(sim, crate::sim::world::UninitContext::new(rules));

    SkirmishLaunchApplyResult {
        spawned_mcvs,
        active_slots: slots.len(),
    }
}

/// Project the already-resolved start table and base centers. This boundary is
/// intentionally incapable of drawing: the selected-mode chooser completed in
/// the pre-Fill plan, while starting Technos and the force tail remain later.
pub(super) fn project_pre_fill_start_assignment(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    start_assignment: &NativeStartAssignment,
) {
    // Session start-slot -> house table: filled after the random-assignment
    // draws, before tick 0 — lockstep state (hashed + serialized).
    let mut table = Vec::new();
    for (start_idx, slot_idx) in start_assignment.start_table.iter().enumerate() {
        let Some(slot_idx) = *slot_idx else {
            continue;
        };
        let Some(slot) = slots.get(slot_idx) else {
            continue;
        };
        table.push((start_idx as u32, sim.interner.intern(&slot.owner_name)));
    }
    sim.session.install_start_slot_houses(table);
    assign_launch_base_centers(sim, slots, &start_assignment.placements);
}

/// `Read_Scenario`'s `Shroud=no` start (`0x006849D0..0x006849E7`): with
/// Rules+0x14AE clear, `RevealEntireMap(NULL)` explores the whole map for the
/// current house ([`Simulation::reveal_entire_map`]). AI houses keep their own
/// unexplored planes; no sight, gap pass or map-clear byte is written.
///
/// RESIDUAL: native reveals after `Post_Map_Init` (`0x00684990`), so after
/// the startup crates, AI credits and alliances that VERA's
/// `finalize_scenario_post_map` runs later. The reveal reads none of them,
/// and a noncampaign CellPUT already discovered every object for the current
/// house (`discover_by_player`), so no effect is known.
pub fn apply_launch_shroud_option(
    sim: &mut Simulation,
    context: crate::sim::world::UninitContext<'_>,
) {
    if sim.session.game_options().shroud {
        return;
    }
    sim.reveal_entire_map(None, context);
}

fn assign_launch_base_centers(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    assignments: &[(usize, Waypoint)],
) {
    for (slot_idx, waypoint) in assignments {
        let Some(slot) = slots.get(*slot_idx) else {
            continue;
        };
        // The House edge (+0x577C) keeps the constructor's North (0x004F5C79)
        // until a yard's DetermineEdge (0x0050DB00); a start does not set it.
        if let Some(house) = crate::sim::house_state::house_state_for_owner_mut(
            &mut sim.houses,
            &slot.owner_name,
            &sim.interner,
        ) {
            house.set_base_center((waypoint.rx, waypoint.ry));
        }
    }
}

pub fn normalized_launch_slots(session: &SkirmishLaunchSession) -> Vec<NormalizedSkirmishSlot> {
    // CreateHouses687F66..84 compares signed DWORD node+53; equal colors
    // retain source order, normalized by the launch owner for local-first LAN
    // nodes. 688137 writes the House index back to each node.
    // Internal owner keys must stay distinct even for duplicate handles or
    // handles colliding with generated AI/fixed Houses (688092..6880A4).
    let mut used_names: Vec<String> = ["Neutral", "Special"]
        .into_iter()
        .map(normalize_house_name)
        .collect();
    used_names.extend(
        (1..=session.opponents.len())
            .map(|index| normalize_house_name(&format!("Computer{index}"))),
    );
    let mut humans: Vec<_> = session.humans_with_house_leaders().collect();
    humans.sort_by_key(|(index, human, _)| {
        (human.slot.color_index, human.house_source_order(*index))
    });
    let mut slots: Vec<NormalizedSkirmishSlot> =
        Vec::with_capacity(humans.len() + session.opponents.len());
    // VERA shared control: a player who shares a start joins its leader's
    // House instead of building one. Every human keeps the source its place
    // in this order gives it.
    let mut members = Vec::new();
    for (source, (index, human, leader)) in humans.into_iter().enumerate() {
        let controller = LaunchController {
            human_index: index,
            source: i8::try_from(source).expect("at most 96 human nodes"),
        };
        if leader != index {
            members.push((leader, controller));
            continue;
        }
        let mut owner_name = human.name.clone();
        let mut suffix = 0;
        while used_names.contains(&normalize_house_name(&owner_name)) {
            owner_name = if suffix == 0 {
                "Player".into()
            } else {
                format!("Player{suffix}")
            };
            suffix += 1;
        }
        used_names.push(normalize_house_name(&owner_name));
        slots.push(NormalizedSkirmishSlot {
            driver_policy: None,
            owner_name,
            country: if human.is_observer() {
                LaunchCountry::America
            } else {
                human.slot.country
            },
            color_index: human.slot.color_index,
            start_position: human.slot.start_position,
            team: human.slot.team,
            is_human: true,
            observer: human.is_observer(),
            controllers: vec![controller],
            handle: Some(human.name.clone()),
            difficulty: HouseDifficulty::Normal,
            commander: None,
        });
    }
    for (leader, controller) in members {
        slots
            .iter_mut()
            .find(|slot| slot.source_human_index() == Some(leader))
            .expect("a shared start's leader builds its House")
            .controllers
            .push(controller);
    }
    for (index, opponent) in session.opponents.iter().enumerate() {
        slots.push(NormalizedSkirmishSlot {
            driver_policy: opponent.driver_policy,
            owner_name: crate::skirmish_launch::computer_house_name(index),
            country: opponent.country,
            color_index: opponent.color_index,
            start_position: opponent.start_position,
            team: opponent.team,
            is_human: false,
            observer: false,
            controllers: Vec::new(),
            handle: None,
            difficulty: HouseDifficulty::from_native(opponent.difficulty.as_i32())
                .expect("native AI difficulty"),
            commander: None,
        });
    }
    slots
}

pub fn populate_special_houses(sim: &mut Simulation, house_roster: &HouseRoster, rules: &RuleSet) {
    for special_name in ["Neutral", "Special"] {
        let definition = house_roster
            .houses
            .iter()
            .find(|house| house.name.eq_ignore_ascii_case(special_name));
        let name = definition.map_or(special_name, |house| house.name.as_str());
        let name_id = sim.interner.intern(name);
        let country_name = definition
            .and_then(|house| house.country.as_deref())
            .unwrap_or(special_name);
        let country_id = Some(sim.interner.intern(country_name));
        let declared_side = definition.and_then(|house| house.side.as_deref());
        let side_idx = crate::sim::house_state::resolve_house_side_index(
            rules,
            Some(country_name),
            declared_side,
            crate::sim::house_state::side_index_from_name(declared_side),
        );
        // Original gamemd CreateHouses6882C2..68837E constructs this fixed
        // tail without participant SetCreditsAndColor4FCE00. House ctor
        // 4F58D8 leaves cash0; launch credits belong to participants.
        // RESIDUAL (invisible): the constructor's TechLevel is 1 (4F56C3),
        // not the session's; neither house ever builds.
        let mut house = HouseState::new(
            name_id,
            side_idx,
            country_id,
            false,
            0,
            sim.session.game_options().tech_level,
        );
        // The House constructor builds its whole Supers vector.
        crate::sim::superweapon::construct_house_supers(sim, rules, name_id, false);
        // House4F62BA copies its HouseType's ID. These fixed-tail Houses do
        // not pass the generated participant-name writers below.
        house.initialize_scenario_name(
            rules
                .scenario_country_name(Some(country_name))
                .unwrap_or(country_name),
        );
        // Stock Neutral/Special are MultiplayPassive, which keeps them out of
        // defeat evaluation and out of the game-over alive scan. `country_name`
        // is always concrete here — the roster's `Country=` when the map named
        // one, otherwise the special house's own `[Countries]` entry, which is
        // what this house is being built as.
        house.resolve_multiplay_passive(rules, Some(country_name));
        house.project_country_mults(rules, &sim.interner);
        sim.houses.register(house);
    }
}

pub fn populate_launch_houses(
    sim: &mut Simulation,
    slots: &[NormalizedSkirmishSlot],
    rules: &RuleSet,
) {
    let human_count = slots.iter().filter(|slot| slot.is_human).count();
    // Resolved once: the catalog and these switches are the same for every
    // slot, and the base plan is built before the loop.
    let feedback = rules.commander_feedback();
    // The slot ordinal seeds the commander pick, so a launch whose assignment
    // table names a commander of the wrong side still ends up with one
    // commander per house instead of the same stand-in three times.
    for (slot_index, slot) in slots.iter().enumerate() {
        let name_id = sim.interner.intern(&slot.owner_name);
        let country_name = rules
            .country_name(slot.country.index())
            .expect("accepted launch Country must exist in the canonical Rules registry");
        let country_id = sim.interner.intern(country_name);
        let side_index =
            crate::sim::house_state::resolve_house_side_index(rules, Some(country_name), None, 0);
        let mut house = HouseState::new(
            name_id,
            side_index,
            Some(country_id),
            slot.is_human,
            sim.session.game_options().starting_credits,
            sim.session.game_options().tech_level,
        );
        // The House constructor builds its whole Supers vector.
        crate::sim::superweapon::construct_house_supers(sim, rules, name_id, false);
        // CreateHouses overwrites the constructor name independently of the
        // display handle: GameMode 4 gives a human its node's name narrowed
        // to 20 bytes (0x687FCE..0x688043), other modes "<human player>"
        // (0x68804A..0x688092); every computer gets "Computer"
        // (0x688210..0x688252). Every computer keeps the same native name;
        // its internal key stays distinct, so FromName can select the first
        // array entry.
        // RESIDUAL: the node name's wide-to-narrow conversion (0x7B7F90) is
        // taken as the handle's first 20 characters; WOL nicknames are ASCII.
        let wol_handle = slot
            .handle
            .as_deref()
            .filter(|_| {
                slot.is_human
                    && sim.session.game_mode()
                        == crate::sim::scenario_session::ScenarioGameMode::Wol
            })
            .map(|handle| handle.chars().take(20).collect::<String>());
        house.initialize_scenario_name(match (&wol_handle, slot.is_human) {
            (Some(handle), _) => handle,
            (None, true) => "<human player>",
            (None, false) => "Computer",
        });
        // The same writers give a human its own node's handle and every
        // computer TXT_COMPUTER as its UI name, in every mode.
        if slot.is_human {
            if let Some(handle) = &slot.handle {
                house.set_ui_name(crate::sim::house_state::HouseUiName::handle(handle));
            }
        } else {
            house.set_ui_name(crate::sim::house_state::HouseUiName::Computer);
        }
        // `ScenarioClass::Create_Houses` passes every launch house through
        // `HouseClass::SetDifficulty` (humans with 1, `0x00688132`; computer
        // slots with their own, `0x006882B9`); Neutral and Special are never
        // passed and keep the constructor's values.
        // CreateHouses688293..6882B9 counts every human node, including
        // observers, before applying Rules[AI].CompEasyBonus to positive AI
        // difficulty values. The unchanged House owner applies its biases.
        let difficulty = if !slot.is_human && human_count > 1 && rules.general.comp_easy_bonus {
            HouseDifficulty::from_native((slot.difficulty as i32 - 1).max(0))
                .expect("native AI difficulty")
        } else {
            slot.difficulty
        };
        house.set_difficulty(
            difficulty,
            &rules.general,
            rules.country_difficulty_biases(country_name),
            sim.session.rule_policy(),
            sim.houses.order().len() as i32,
            sim.session.binary_frame() as i32,
        );
        // ScenarioClass::Create_Houses leaves generated human CurrentIQ at
        // the constructor value zero and stamps generated computer slots with
        // Rules.MaxIQLevels in non-campaign sessions.
        if !slot.is_human {
            house.stamp_computer_iq(rules.general.max_iq_levels);
            // `0x006881F5..0x00688209`: a computer slot's team marks the
            // Scenario, which turns Computer_Paranoid off.
            if slot.team != LaunchTeam::None {
                sim.session.set_computer_slot_has_team();
            }
        }
        if !slot.is_human
            && let Some(policy) = slot.driver_policy
        {
            house.make_command_driven(policy);
        }
        house.resolve_multiplay_passive(rules, Some(country_name));
        house.project_country_mults(rules, &sim.interner);
        // Product extension: a computer house may carry a commander
        // personality, but only when something asked for one. The launch slot's
        // own `commander` wins; `[CommanderFeedback] Assign.<owner_name>=` is
        // the test seam underneath it. With neither, the house stays `None` and
        // its AI is retail's own — no trigger deny, no weights, no re-pacing.
        //
        // `[CommanderFeedback] Personalities=no` removes the feature for every
        // house at once: a map that ships preset AI (tower defence, a scripted
        // mission) writes it and keeps the native skirmish brain.
        if !slot.is_human && feedback.personalities {
            let requested = slot
                .commander
                .as_deref()
                .or_else(|| feedback.assigned(&slot.owner_name));
            let personality = crate::rules::personality::personality_for_house(
                rules.personalities(),
                country_name,
                requested,
                slot_index,
            );
            house.set_personality(personality);
            if let Some(definition) = personality
                .and_then(|index| rules.personalities().get(index))
                .filter(|definition| !definition.reserved)
            {
                // The strategy tilt is the second half of a commander: the
                // same person who talks must also fight differently.
                house.apply_commander_strategy(
                    definition,
                    &rules.general,
                    sim.houses.order().len() as i32,
                    sim.session.binary_frame() as i32,
                );
            }
        }
        sim.houses.register(house);
    }
}

/// Bit pattern of the `0.01` double at `0x007E3808` read by
/// `ScenarioClass__Post_Map_Init @ 0x00686A52` (`FMUL qword [0x007E3808]`).
const NATIVE_AI_CM_PERCENT_MULTIPLIER_BITS: u64 = 0x3F84_7AE1_47AE_147B;

/// gamemd-derived AI opening grant: `ftol(MultiplayerAICM[diff] * 0.01 * money)`.
///
/// Active YR `ScenarioClass__Post_Map_Init @ 0x00686A52..0x00686A6B`:
/// `FILD MultiplayerAICM[house+0x184]; FMUL qword [0x007E3808] (0.01);
/// FIMUL money; CALL Math__ftol @ 0x007C5F00`. The process control word is
/// `0x0E7F` (RC = chop, PC = 53): `WinMain @ 0x006BBFC1` calls
/// `_controlfp(0x300, 0x300)` (`_RC_CHOP`) and `0x007C5EE4` captures the word
/// at `0x00822D80` for `ftol`. Both multiplies therefore truncate toward zero at
/// 53 bits and the final conversion truncates; plain f64 round-to-nearest
/// differs (e.g. `3 * 0.01 * 100` chops to 2, not 3). `Math__ftol` returns the
/// low dword of `FISTP qword`, so the i64 result is narrowed with `as i32`; an
/// out-of-range operand stores the x87 integer indefinite (low dword 0).
pub fn native_ai_opening_grant(coefficient: i32, money: i32) -> i32 {
    let percent = X87Chop53::load_f64(NativeF64Bits::from_bits(
        NATIVE_AI_CM_PERCENT_MULTIPLIER_BITS,
    ))
    .expect("0.01 is a finite normal double");
    let scaled = X87Chop53::mul(X87Chop53::load_i32(coefficient), percent);
    let product = X87Chop53::mul(scaled, X87Chop53::load_i32(money));
    X87Chop53::ftol_i32_low_masked(product)
}

/// Apply the generated skirmish AI opening-credit grant at the Post_Map_Init handoff.
///
/// Active YR `ScenarioClass__Post_Map_Init @ 0x00686890`, loop `0x006869BE..
/// 0x00686A7E`: for each house with `+0x1EC == 0` (not human) whose HouseType
/// `+0x1A6 == 0` (not `MultiplayPassive`), it calls the house secondary vslot
/// `+0x18` (`0x004F6990`: `ftol(storage_total * factor + credits)`, storage is
/// empty for generated Battle houses so this is the current balance), then
/// computes [`native_ai_opening_grant`] and passes the result to
/// `HouseClass__Add_Credits @ 0x004F9950`. Stock `MultiplayerAICM=400,0,0`
/// therefore gives a Hard AI 5x its balance and leaves Normal/Easy unchanged.
///
/// Sequencing: `Generate_Starting_Units @ 0x005D6D80` is called earlier in the
/// same function (`0x00686937`), so the balance multiplied here already carries
/// the leftover starting-unit budget credited by `seed_starting_extra_units`
/// (pure integer arithmetic on the native side: `CDQ/SAR/IDIV` budget thirds
/// and integer cost subtraction, no x87 involvement).
pub fn apply_skirmish_ai_opening_credits(houses: &mut crate::sim::houses::Houses, rules: &RuleSet) {
    for house in houses.values_mut() {
        if house.is_human() || house.multiplay_passive() {
            continue;
        }
        // External controllers opt in to the Hard grant. Read the same
        // post-starting-unit wallet as the stock 0x00686A52 calculation.
        let difficulty = match house.driver_policy() {
            Some(policy) if policy.native_hard_grant => HouseDifficulty::Hard,
            Some(_) => continue,
            None => house.difficulty(),
        };
        let coefficient = rules
            .general
            .multiplayer_ai_cm
            .get(difficulty.table_index())
            .copied()
            .unwrap_or(0);
        let grant = native_ai_opening_grant(coefficient, house.economy().available_money());
        house.economy_mut().add_credits(grant);
    }
}

/// The launch diplomacy graph for explicit teams and same-role alliances.
///
/// A map House's `Allies=` takes no part. `HouseClass::Read_INI @ 0x005009B0`,
/// its only reader on an authored map, runs only when `Session.GameMode` is 0
/// (campaign; `0x0068776E..0x006877B9`), and `ScenarioClass::Create_Houses
/// @ 0x00687F10` builds Neutral and Special with no allies.
pub fn launch_alliance_map(
    slots: &[NormalizedSkirmishSlot],
    policy: crate::rules::rule_policy::RulePolicy,
) -> crate::map::houses::HouseAllianceMap {
    let mut alliances = crate::map::houses::HouseAllianceMap::new();
    for slot in slots {
        alliances
            .entry(normalize_house_name(&slot.owner_name))
            .or_default();
    }
    for left in slots {
        let LaunchTeam::Team(team) = left.team else {
            continue;
        };
        for right in slots {
            if left.owner_name == right.owner_name || right.team != LaunchTeam::Team(team) {
                continue;
            }
            let left_key = normalize_house_name(&left.owner_name);
            let right_key = normalize_house_name(&right.owner_name);
            alliances
                .entry(left_key.clone())
                .or_default()
                .insert(right_key.clone());
            alliances.entry(right_key).or_default().insert(left_key);
        }
    }
    if policy.allies_starting_houses_by_role() {
        for left in slots {
            for right in slots {
                if left.owner_name == right.owner_name || left.is_human != right.is_human {
                    continue;
                }
                let left_key = normalize_house_name(&left.owner_name);
                let right_key = normalize_house_name(&right.owner_name);
                alliances
                    .entry(left_key.clone())
                    .or_default()
                    .insert(right_key.clone());
                alliances.entry(right_key).or_default().insert(left_key);
            }
        }
    }
    alliances
}
