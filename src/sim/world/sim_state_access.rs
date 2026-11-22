//! `Simulation` state access: RNG views and load-process retention, read
//! accessors, session and map installs, and the test-support mutators.

use std::collections::BTreeMap;

#[cfg(any(test, feature = "test-support"))]
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::RealCellBridgeFlags0x1180;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::resolved_terrain::SavedCellLandValues;
use crate::map::resolved_terrain::SharedCellDummy;
#[cfg(any(test, feature = "test-support"))]
use crate::rules::ruleset::RuleSet;
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::house_diplomacy::HouseAlliances;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::native_identity::NativeUniqueIdCursor;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::overlay_grid::OverlayGrid;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::overlay_grid::recalc_overlay_passability;
use crate::sim::pathfinding::terrain_speed;
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::sim::power_system::PowerState;
use crate::sim::production::ProductionState;
use crate::sim::rng::MainRng;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::smudge_grid::SmudgeGrid;
use crate::sim::team_script_vm::TeamScriptVm;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::trigger_runtime::TriggerEffect;
use crate::sim::trigger_runtime::TriggerRuntime;
use crate::sim::vision::FogState;

#[cfg(any(test, feature = "test-support"))]
use super::DeferredDeleteQueue;
#[cfg(any(test, feature = "test-support"))]
use super::ObjectSubstrate;
use super::SimFireEvent;
use super::SimSoundEvent;
use super::Simulation;
use super::SimulationRngState;
use super::SimulationRngViews;
use super::SnapshotProcessState;
#[cfg(any(test, feature = "test-support"))]
use super::display_layers;

impl Simulation {
    /// Observe all three logical RNG objects without exposing mutation.
    pub fn rng_views(&self) -> SimulationRngViews<'_> {
        SimulationRngViews {
            scenario: self.scenario_rng.logical_view(),
            main: self.main_rng.logical_state(),
            mapgen: self.mapgen_rng.logical_view(),
        }
    }

    /// Summarise this tick for cross-engine parity comparison.
    ///
    /// Read-only and side-effect free: the caller decides whether to record it, so
    /// enabling capture cannot perturb the run being measured. The RNG cursor comes from
    /// the main stream, which is the one the original engine exposes globally.
    pub fn parity_digest(&self) -> crate::sim::parity_digest::ParityDigest {
        let views = self.rng_views();
        crate::sim::parity_digest::ParityDigest::capture(
            self.session.tick(),
            self.entities(),
            &self.houses,
            views.main.index_a,
            views.main.index_b,
            views.scenario.index_a,
            views.scenario.index_b,
        )
    }

    /// Capture all three complete logical RNG objects as immutable evidence.
    pub fn rng_state(&self) -> SimulationRngState {
        SimulationRngState {
            scenario: self.scenario_rng.logical_state(),
            main: self.main_rng.logical_state(),
            mapgen: self.mapgen_rng.logical_state(),
        }
    }

    /// LoadContent67E730 does not restore process Main/MapGen. ScenarioLoad
    /// For stock saves, 689470 -> post-read683560 has already reset Scenario to Seed(0).
    /// The candidate stages the one process Main cursor: draws made while the
    /// load is prepared advance it, and the commit moves it into that cell
    /// (`install_fresh_process_main`). Stock loading keeps the live seed and
    /// MapGen or initializes them to zero in a cold shell. Custom sessions
    /// resume the saved seed and MapGen.
    pub(crate) fn retain_process_state_for_load(&mut self, process: SnapshotProcessState<'_>) {
        let retain_continuation = self.retains_snapshot_continuation();
        match process {
            SnapshotProcessState::Live(live) => {
                if !retain_continuation {
                    self.session.record_random_seed(live.session.seed());
                    self.mapgen_rng = live.mapgen_rng.clone();
                    self.substrate
                        .raw_cell_occupation
                        .retain_process_dummy_from(&live.substrate.raw_cell_occupation);
                }
                self.main_rng = live.main_rng.clone();
                self.bind_shared_cell_dummy(live.effective_shared_cell_dummy());
            }
            SnapshotProcessState::Cold(main) => {
                if !retain_continuation {
                    self.session.record_random_seed(0);
                    self.mapgen_rng = SimRng::new(0);
                }
                self.main_rng = main.checkpoint();
            }
        }
    }

    /// Replay reconstructs a fresh launch, then restores its captured startup
    /// clocks/cursors. This is deliberately separate from native player Load,
    /// whose Scenario RNG reset and retained process cursors remain unchanged.
    pub fn restore_replay_startup(
        &mut self,
        session: crate::sim::scenario_session::ScenarioSession,
        scenario: SimRng,
        main: SimRng,
        mapgen: SimRng,
    ) -> Result<(), String> {
        if self.session.tick() != 0
            || self.session.binary_frame() != 0
            || session.tick() != 0
            || session.binary_frame() != 0
        {
            return Err("replay startup must precede the first ordinary frame".into());
        }
        if !self.session.started() {
            return Err(
                "replay startup requires the completed loader's started publication".into(),
            );
        }
        if session.seed() != self.session.seed() || session.map_name() != self.session.map_name() {
            return Err("replay startup seed/map differs from its constructed world".into());
        }
        if !scenario.valid_checkpoint() || !main.valid_checkpoint() || !mapgen.valid_checkpoint() {
            return Err("invalid replay RNG checkpoint".into());
        }
        if session
            .current_house()
            .is_some_and(|owner| !self.houses.contains_key(&owner))
        {
            return Err("replay startup House is absent from the constructed world".into());
        }
        self.session = session;
        // ScenarioSession.started is process state, deliberately serde-skipped.
        // Retain the fresh loader's StartScenario683E75 publication: Tag6E4F60
        // consumes it when adjusting the active trigger traversal cursors.
        self.session.publish_started();
        self.scenario_rng = scenario;
        self.main_rng = main;
        self.mapgen_rng = mapgen;
        Ok(())
    }

    /// Init_Random_Number_System52FC20. The caller supplies the resolved
    /// clock/override seed. Both Scenario and this simulation's Main are
    /// reseeded; MapGen is untouched.
    /// Executed full-state controls:
    /// tools/rmg_oracle/vectors/rng.json::menu_rng_cases.
    pub fn initialize_random_number_system(&mut self, seed: u32) {
        self.session.record_random_seed(u64::from(seed));
        self.scenario_rng = SimRng::new(u64::from(seed));
        self.main_rng = SimRng::new(u64::from(seed));
    }

    /// Apply the successful load's native MapClass Resize reconstruction to
    /// every modeled field of the fixed fallback CellClass.
    ///
    /// `MouseClass::Load` dispatch at `0x005BE150` routes restored dimensions through
    /// `MapClass::Resize @ 0x00565C10`, whose unconditional call to
    /// `CellClass::Constructor @ 0x0047BBF0` reconstructs the fixed dummy in
    /// place. Player loads reset a detached candidate before fallible restoration,
    /// so a rejected transactional load cannot mutate the running world.
    pub(crate) fn reconstruct_cellclass_dummy_for_map_resize(&mut self) {
        self.effective_shared_cell_dummy()
            .reconstruct_for_map_resize();
        self.trigger_runtime.reconstruct_dummy_for_map_resize();
        self.substrate
            .base_reservations
            .reconstruct_dummy_for_map_resize();
        self.substrate
            .raw_cell_occupation
            .reconstruct_dummy_for_map_resize();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn retain_player_load_process_for_test(&mut self, process: SnapshotProcessState<'_>) {
        self.retain_process_state_for_load(process);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn reconstruct_map_dummy_for_test(&mut self) {
        self.reconstruct_cellclass_dummy_for_map_resize();
    }

    /// Adopt the process-global CellClass identity already bound to a load's
    /// resolved grid, before that grid is cloned into Simulation.
    pub fn bind_shared_cell_dummy(&mut self, shared_cell_dummy: SharedCellDummy) {
        self.shared_cell_dummy = shared_cell_dummy.clone();
        if let Some(terrain) = self.resolved_terrain.as_mut() {
            terrain.bind_shared_cell_dummy(shared_cell_dummy);
        }
    }

    /// Rebind a successfully prepared map to the retained process selector.
    /// The map owner keeps its catalogs on the same lazy table identity.
    pub fn bind_tile_variant_selector(
        &mut self,
        selector: crate::map::tile_variant_selector::TileVariantSelector,
    ) {
        if let Some(terrain) = self.resolved_terrain.as_mut() {
            terrain.bind_tile_variant_selector(selector);
        }
    }

    // Read access for the client crate. In-crate code changes these slots only
    // through each sub-owner's operations.
    pub fn interner(&self) -> &crate::sim::intern::StringInterner {
        &self.interner
    }

    pub fn production(&self) -> &ProductionState {
        &self.production
    }

    pub fn session(&self) -> &ScenarioSession {
        &self.session
    }

    pub fn scenario_rng(&self) -> &SimRng {
        &self.scenario_rng
    }

    pub fn main_rng(&self) -> &SimRng {
        &self.main_rng
    }

    pub fn mapgen_rng(&self) -> &SimRng {
        &self.mapgen_rng
    }

    pub fn native_unique_ids(&self) -> Option<&crate::sim::native_identity::NativeUniqueIdCursor> {
        self.native_unique_ids.as_ref()
    }

    pub fn fog(&self) -> &FogState {
        &self.fog
    }

    pub fn house_alliances(&self) -> &HouseAlliances {
        &self.house_alliances
    }

    pub fn sound_events(&self) -> &[SimSoundEvent] {
        self.sound_events.as_slice()
    }

    pub fn lighting_sources(&self) -> &crate::sim::light_sources::LightingSources {
        &self.lighting_sources
    }

    pub fn fire_events(&self) -> &[SimFireEvent] {
        self.fire_events.as_slice()
    }

    pub fn projectiles(&self) -> &crate::sim::projectile::ProjectileStore {
        &self.projectiles
    }

    pub fn waves(&self) -> &crate::sim::wave::WaveStore {
        &self.waves
    }

    pub fn team_script_vm(&self) -> &TeamScriptVm {
        &self.team_script_vm
    }

    pub fn houses(&self) -> &crate::sim::houses::Houses {
        &self.houses
    }

    /// Trigger-runtime read access for external observers. Every mutation
    /// stays with the trigger owners; see [`TriggerRuntime`]'s view
    /// accessors.
    pub fn trigger_runtime(&self) -> &TriggerRuntime {
        &self.trigger_runtime
    }

    pub fn zone_grid(&self) -> Option<&ZoneGrid> {
        self.zone_grid.as_ref()
    }

    pub fn resolved_terrain(&self) -> Option<&ResolvedTerrainGrid> {
        self.resolved_terrain.as_ref()
    }

    pub fn real_cell_bridge_flags_0x1180(&self) -> &RealCellBridgeFlags0x1180 {
        &self.real_cell_bridge_flags_0x1180
    }

    pub fn bridge_state(&self) -> Option<&BridgeRuntimeState> {
        self.bridge_state.as_ref()
    }

    pub fn overlay_grid(&self) -> Option<&crate::sim::overlay_grid::OverlayGrid> {
        self.overlay_grid.as_ref()
    }

    pub fn crate_authority(&self) -> &crate::sim::crates::CrateAuthority {
        &self.crate_authority
    }

    pub(crate) fn crate_slot_updates(&mut self) -> crate::sim::crates::CrateSlotUpdates<'_> {
        crate::sim::crates::CrateSlotUpdates::new(&mut self.crate_authority, &mut self.scenario_rng)
    }

    pub(in crate::sim) fn reset_crates_after_user_load(
        &mut self,
        saved_land: &SavedCellLandValues,
    ) -> Result<(), crate::sim::snapshot::SnapshotRestoreError> {
        let overlays = self.overlay_grid.as_mut().ok_or(
            crate::sim::snapshot::SnapshotRestoreError::MissingMapAuthorityComponent {
                component: "OverlayGrid",
            },
        )?;
        self.crate_authority.reset_coordinates_after_user_load();
        overlays.reset_crate_fields_after_user_load(saved_land.allocated_cells());
        Ok(())
    }

    pub fn smudge_grid(&self) -> Option<&crate::sim::smudge_grid::SmudgeGrid> {
        self.smudge_grid.as_ref()
    }

    pub fn radiation(&self) -> &crate::sim::radiation::RadiationState {
        &self.radiation
    }

    pub fn playfield_bounds(&self) -> Option<crate::sim::cell_rect::PlayfieldBounds> {
        self.playfield_bounds
    }

    pub fn playfield_size_height(&self) -> Option<i32> {
        self.playfield_size_height
    }

    /// Advances on every frame step and every debug-trainer write. Between
    /// frames the world changes otherwise only through command queues,
    /// interning and per-input selection effects, none of which reach house,
    /// production, power or superweapon state; a presentation cache of values
    /// derived from those holds while this and the rules serial are unchanged.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Mark a write that a presentation cache keyed on [`Self::revision`]
    /// must see.
    pub(crate) fn note_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn playfield_revision(&self) -> u64 {
        self.playfield_revision
    }

    pub fn bridge_explosions(&self) -> &[InternedId] {
        &self.bridge_explosions
    }

    pub fn metallic_debris(&self) -> &[InternedId] {
        &self.metallic_debris
    }

    pub fn radar_terrain_dirty_cells(&self) -> &[(u16, u16)] {
        self.dirty_cells.radar()
    }

    pub fn radar_terrain_dirty_generation(&self) -> u64 {
        self.dirty_cells.radar_generation()
    }

    pub fn power_states(&self) -> &BTreeMap<InternedId, PowerState> {
        self.power_states.as_map()
    }

    pub fn super_weapons(
        &self,
    ) -> &BTreeMap<InternedId, BTreeMap<InternedId, crate::sim::superweapon::SuperWeaponInstance>>
    {
        self.super_weapons.as_map()
    }

    pub fn lightning_storm(&self) -> &crate::sim::superweapon::lightning_storm::LightningStorm {
        &self.lightning_storm
    }

    pub fn psychic_dominator(
        &self,
    ) -> &crate::sim::superweapon::psychic_dominator::PsychicDominatorState {
        &self.psychic_dominator
    }

    pub fn terrain_speed_config(&self) -> &terrain_speed::TerrainSpeedConfig {
        &self.terrain_speed_config
    }

    /// Wire the cliff/slope coefficients from `[General]` into the live
    /// config; it otherwise holds the compiled vanilla defaults.
    pub(crate) fn install_terrain_speed_rules(
        &mut self,
        general: &crate::rules::ruleset::GeneralRules,
    ) {
        self.terrain_speed_config = terrain_speed::TerrainSpeedConfig::from_general(
            general.tracked_uphill,
            general.tracked_downhill,
            general.wheeled_uphill,
            general.wheeled_downhill,
        );
    }

    /// The authored `Full_Init`'s `InitCellAttributes(0)` value-only total,
    /// recorded once by the authored map install.
    pub(crate) fn record_authored_tiberium_value_total(&mut self, total: i32) {
        self.authored_tiberium_value_total = Some(total);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn authored_tiberium_value_total(&self) -> Option<i32> {
        self.authored_tiberium_value_total
    }

    /// A mission or production step placed new objects this frame; the frame
    /// tail folds it into the tick's `spawned_entities`.
    pub(crate) fn note_mission_spawned_entities(&mut self) {
        self.mission_spawned_entities = true;
    }

    /// Every Techno+324 Wave link, by owner.
    pub(crate) fn active_wave_links(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.active_wave_links
            .iter()
            .map(|(&owner, &wave)| (owner, wave))
    }

    /// Objects holding a custom sound loop.
    pub(crate) fn custom_sound_objects(&self) -> impl Iterator<Item = u64> + '_ {
        self.object_custom_sounds.keys().copied()
    }

    pub fn input_delay_ticks(&self) -> u64 {
        self.input_delay_ticks
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn base_reservations(&self) -> &crate::sim::cell_rect::CellReservationGrid {
        &self.substrate.base_reservations
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn pending_delete(&self) -> &DeferredDeleteQueue {
        &self.substrate.pending_delete
    }

    pub fn air_slots(&self) -> &crate::sim::occupancy::AirSlotGrid {
        &self.substrate.air_slots
    }

    pub fn next_stable_object_id(&self) -> u64 {
        self.substrate.next_stable_object_id.peek()
    }

    // App and harness entry points into sub-owners. Each runs the sub-owner's
    // own operation; the client never holds a mutable sub-owner.

    /// Stock mode install for synthetic and diagnostic scenarios.
    pub fn install_session_mode(&mut self, mode: crate::sim::scenario_session::ScenarioGameMode) {
        let class = self.session.multiplayer_mode_class();
        self.install_session_admission(mode, class, mode.stock_rule_policy(class));
    }

    /// Install the live choices from startup admission before map and House
    /// initialization. This keeps construction rules separate for native replays.
    pub fn install_session_admission(
        &mut self,
        mode: crate::sim::scenario_session::ScenarioGameMode,
        class: Option<crate::skirmish_modes::MpModeClass>,
        rule_policy: crate::rules::rule_policy::RulePolicy,
    ) {
        if mode.is_network() && !self.session.execution_topology().is_lockstep() {
            // VERA network adaptation: Init_Random_Number_System52FC20 seeds
            // Scenario/Main, leaving process MapGen alone. MapGen598030 also
            // supplies bridge repair's overlay variants, so a shared match
            // must not inherit each client's unrelated RMG/repair history.
            // Rejoin/watch use this same fresh start before replaying history.
            // On a generated map this also replaces the post-RMG cursor the
            // bootstrap installed (Generate598960 leaves MapGen advanced, and
            // gamemd's repairs continue from it), so a generated network map's
            // repair variants differ from gamemd's for the same map seed. The
            // match seed is the one value every peer, rejoiner and watcher
            // already holds. Truncated to 32 bits as for Scenario/Main, it gives
            // MapGen their seeded state, from which it advances on its own.
            self.mapgen_rng = SimRng::new(self.session.seed());
        }
        self.session.install_admission(mode, class, rule_policy);
    }

    /// The accepted network setup's MaxAhead ([`ScenarioSession::install_network_timing`]).
    pub fn install_network_timing(&mut self, max_ahead_frames: i32) {
        self.session.install_network_timing(max_ahead_frames);
    }

    /// Admit explicit grants after House construction and before publication.
    pub fn install_control_grants(
        &mut self,
        grants: Vec<crate::sim::control_authority::ControlGrant>,
    ) -> Result<(), crate::sim::control_authority::ControlGrantError> {
        self.session.install_control_grants(&self.houses, grants)
    }

    /// Lookup for a source in the launch roster and a registered House.
    /// Execution and input validate live sender eligibility before querying.
    pub fn control_scope_for_source(
        &self,
        source: i8,
        house: InternedId,
    ) -> crate::sim::control_authority::ControlScope {
        self.session
            .control_scope_for_source(&self.houses, source, house)
    }

    /// The WOL start's tournament value and clan lists
    /// ([`ScenarioSession::install_wol_start`]).
    pub fn install_wol_start(&mut self, tournament: i32, squad_lists: [Vec<String>; 2]) {
        self.session.install_wol_start(tournament, squad_lists);
    }

    /// The map's waypoints ([`ScenarioSession::install_waypoints`]).
    pub fn install_waypoints(
        &mut self,
        waypoints: &std::collections::HashMap<u32, crate::map::waypoints::Waypoint>,
    ) {
        self.session.install_waypoints(waypoints);
    }

    /// StartScenario683E75 and a committed LoadGame67E6C4 publish A8ED5C=1
    /// ([`ScenarioSession::publish_started`]).
    pub fn publish_scenario_started(&mut self) {
        self.session.publish_started();
    }

    /// MainGame48CEBF and the restart clear A8ED5C ([`ScenarioSession::retire_started`]).
    pub fn retire_scenario_started(&mut self) {
        self.session.retire_started();
    }

    /// BattleControlTerminated686570 on shell return
    /// ([`ScenarioSession::reset_campaign_mission_counter_on_shell_exit`]).
    pub fn reset_campaign_mission_counter_on_shell_exit(&mut self) {
        self.session.reset_campaign_mission_counter_on_shell_exit();
    }

    /// MapSelect5AE1FA/5AE0B8 publishes the selected stage before EvadeSave4C6180.
    pub fn set_campaign_map_stage(&mut self, stage: u16) {
        self.session.set_campaign_map_stage(stage);
    }

    /// The outgoing scene's live ambient Rules writes, returned once it stops
    /// ([`ScenarioLightingState::retire_ambient_change_inputs`]).
    pub fn retire_ambient_change_inputs(
        &mut self,
    ) -> [Option<crate::util::native_x87::NativeF64Bits>; 2] {
        self.session.lighting_mut().retire_ambient_change_inputs()
    }

    /// Harness registration of a house in array order ([`Houses::register`]).
    pub fn register_house(&mut self, house: HouseState) {
        self.houses.register(house);
    }

    /// Harness exemption of `house` from the defeat scan
    /// ([`HouseState::make_multiplay_passive`]); false when it is unknown.
    pub fn make_house_multiplay_passive(&mut self, house: InternedId) -> bool {
        let Some(state) = self.houses.get_mut(&house) else {
            return false;
        };
        state.make_multiplay_passive();
        true
    }

    /// The authored loader runs the final InitCellAttributes sweep on the live
    /// map grid outside the simulation, then installs it again through
    /// [`Self::install_resolved_terrain_for_new_map`].
    pub(crate) fn take_resolved_terrain_for_load(&mut self) -> Option<ResolvedTerrainGrid> {
        self.resolved_terrain.take()
    }

    /// Install the bridge record vector a load builds from the map grid's
    /// CellClass state, or its rebuild after a crate `OverlayClass::Mark` batch.
    pub(crate) fn install_bridge_runtime(&mut self, bridges: BridgeRuntimeState) {
        self.bridge_state = Some(bridges);
    }

    /// Install the map's `[Smudge]` grid and mark the cells it seeded dirty.
    pub(crate) fn install_smudge_grid(&mut self, smudges: SmudgeGrid) {
        self.smudge_grid = Some(smudges);
        Self::flush_smudge_dirty(&mut self.smudge_grid, &mut self.dirty_cells);
    }

    /// Install the native unique-id cursor a scenario prefix plan staged,
    /// before terrain Fill constructs the first map object.
    pub(crate) fn install_native_unique_id_cursor(&mut self, cursor: NativeUniqueIdCursor) {
        self.native_unique_ids = Some(cursor);
    }

    // Client-crate fixtures for operations that take their owners.

    #[cfg(any(test, feature = "test-support"))]
    pub fn bomb_defuse_for_test(&mut self, carrier: u64) {
        Self::bomb_defuse(
            &mut self.substrate,
            &mut self.bombs,
            &mut self.sound_events,
            carrier,
        );
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn bomb_list_update_for_test(&mut self, rules: &RuleSet) {
        Self::bomb_list_update(
            &self.interner,
            &mut self.substrate,
            &self.houses,
            &self.resolved_terrain,
            &mut self.bombs,
            &mut self.sound_events,
            rules,
        );
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn light_building_for_test(&mut self, id: u64, rules: &RuleSet) {
        self.light_building(id, rules);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn initialize_campaign_current_house_for_test(
        &mut self,
        ini: &crate::rules::ini_parser::IniFile,
    ) {
        crate::sim::scenario_bootstrap::initialize_campaign_current_house(
            &mut self.session,
            &mut self.houses,
            ini,
        );
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn bomb_attach_for_test(&mut self, planter: u64, target: Option<u64>, rules: &RuleSet) {
        self.bomb_attach(planter, target, None, rules);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn suspend_production_for_test(
        &mut self,
        owner: &str,
        category: crate::sim::production::ProductionCategory,
    ) -> bool {
        crate::sim::production::suspend_production(
            &self.interner,
            &mut self.production,
            &self.session,
            owner,
            category,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_trigger_variable_for_test(&mut self, global: bool, index: i32, set: bool) -> bool {
        Self::set_trigger_variable(
            &self.session,
            &mut self.scenario_rng,
            &mut self.trigger_runtime,
            global,
            index,
            set,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn submit_object_display_for_test(&mut self, id: u64, layer: display_layers::DisplayLayer) {
        Self::submit_object_display(&mut self.substrate, id, layer);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn apply_runtime_bridge_mark_stamp_for_test(
        &mut self,
        stamp: crate::map::bridge_facts::BridgeFlagStamp,
        family: crate::map::bridge_facts::BridgeStampFamily,
    ) {
        Self::apply_runtime_bridge_mark_stamp(
            &mut self.resolved_terrain,
            &mut self.real_cell_bridge_flags_0x1180,
            stamp,
            family,
        );
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn pause_scenario_for_test(&mut self, duration: i32) -> Vec<TriggerEffect> {
        self.session.pause_scenario(duration)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn expire_scenario_pause_for_test(&mut self) -> Vec<TriggerEffect> {
        self.session.expire_scenario_pause()
    }

    // Client-crate fixtures; in-crate tests use the fields.

    #[cfg(any(test, feature = "test-support"))]
    pub fn session_mut_for_test(&mut self) -> &mut ScenarioSession {
        &mut self.session
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn fog_mut_for_test(&mut self) -> &mut FogState {
        &mut self.fog
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn substrate_mut_for_test(&mut self) -> &mut ObjectSubstrate {
        &mut self.substrate
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn overlay_grid_mut_for_test(
        &mut self,
    ) -> &mut Option<crate::sim::overlay_grid::OverlayGrid> {
        &mut self.overlay_grid
    }

    /// One live overlay cell for a fixture, over an empty grid sized to the
    /// terrain when none is installed yet.
    #[cfg(any(test, feature = "test-support"))]
    pub fn overlay_cell_mut_for_test(
        &mut self,
        rx: u16,
        ry: u16,
    ) -> &mut crate::sim::overlay_grid::OverlayCell {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .expect("an overlay fixture needs terrain");
        let (width, height) = (terrain.width(), terrain.height());
        self.overlay_grid
            .get_or_insert_with(|| OverlayGrid::new(width, height))
            .cell_mut_for_test(rx, ry)
    }

    /// One overlay placed for a fixture with the Land, blocking and Zone it
    /// projects onto the Cell ([`recalc_overlay_passability`]), over an empty
    /// grid sized to the terrain when none is installed yet.
    #[cfg(any(test, feature = "test-support"))]
    pub fn place_overlay_for_test(
        &mut self,
        cell: (u16, u16),
        overlay_id: u8,
        data: u8,
        registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    ) {
        let terrain = self
            .resolved_terrain
            .as_mut()
            .expect("an overlay fixture needs terrain");
        let grid = self
            .overlay_grid
            .get_or_insert_with(|| OverlayGrid::new(terrain.width(), terrain.height()));
        grid.place_overlay(cell.0, cell.1, overlay_id, data);
        recalc_overlay_passability(grid, terrain, registry, cell.0, cell.1);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn houses_mut_for_test(&mut self) -> &mut crate::sim::houses::Houses {
        &mut self.houses
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn resolved_terrain_mut_for_test(&mut self) -> &mut Option<ResolvedTerrainGrid> {
        &mut self.resolved_terrain
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn house_alliances_mut_for_test(&mut self) -> &mut HouseAllianceMap {
        self.house_alliances.as_map_mut_for_test()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn scenario_rng_mut_for_test(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn super_weapons_mut_for_test(
        &mut self,
    ) -> &mut BTreeMap<InternedId, BTreeMap<InternedId, crate::sim::superweapon::SuperWeaponInstance>>
    {
        self.super_weapons.map_mut_for_test()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn take_sound_events_for_test(&mut self) -> Vec<SimSoundEvent> {
        self.sound_events.take_for_test()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn crate_authority_mut_for_test(&mut self) -> &mut crate::sim::crates::CrateAuthority {
        &mut self.crate_authority
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn radiation_mut_for_test(&mut self) -> &mut crate::sim::radiation::RadiationState {
        &mut self.radiation
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn power_states_mut_for_test(&mut self) -> &mut BTreeMap<InternedId, PowerState> {
        self.power_states.map_mut_for_test()
    }

    /// The power fixture's assessment of every House from the live buildings.
    #[cfg(any(test, feature = "test-support"))]
    pub fn assess_power_for_test(&mut self, rules: &RuleSet) {
        crate::sim::power_system::tick_power_states(
            &mut self.power_states,
            &mut self.substrate.entities,
            rules,
            &self.interner,
            self.session.binary_frame(),
        );
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn lighting_sources_mut_for_test(
        &mut self,
    ) -> &mut crate::sim::light_sources::LightingSources {
        &mut self.lighting_sources
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn bridge_state_mut_for_test(&mut self) -> &mut Option<BridgeRuntimeState> {
        &mut self.bridge_state
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn playfield_bounds_mut_for_test(
        &mut self,
    ) -> &mut Option<crate::sim::cell_rect::PlayfieldBounds> {
        &mut self.playfield_bounds
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_playfield_bounds_for_test(
        &mut self,
        bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
    ) {
        self.playfield_bounds = bounds;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_playfield_size_height_for_test(&mut self, height: Option<i32>) {
        self.playfield_size_height = height;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn playfield_size_height_mut_for_test(&mut self) -> &mut Option<i32> {
        &mut self.playfield_size_height
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_rule_handles_for_test(
        &mut self,
        handles: Option<crate::sim::type_handle_table::ResolvedRuleHandles>,
    ) {
        self.rule_handles = handles;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_quit_requested_for_test(&mut self, requested: bool) {
        self.quit_requested = requested;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_connection_lost_for_test(&mut self, lost: bool) {
        self.connection_lost = lost;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_terrain_speed_config_for_test(&mut self, config: terrain_speed::TerrainSpeedConfig) {
        self.terrain_speed_config = config;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn link_active_wave_for_test(&mut self, owner: u64, wave: u64) {
        self.active_wave_links.insert(owner, wave);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn waves_mut_for_test(&mut self) -> &mut crate::sim::wave::WaveStore {
        &mut self.waves
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn lightning_storm_mut_for_test(
        &mut self,
    ) -> &mut crate::sim::superweapon::lightning_storm::LightningStorm {
        &mut self.lightning_storm
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn projectiles_mut_for_test(&mut self) -> &mut crate::sim::projectile::ProjectileStore {
        &mut self.projectiles
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn main_rng_mut_for_test(&mut self) -> &mut SimRng {
        &mut self.main_rng
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn interner_mut_for_test(&mut self) -> &mut crate::sim::intern::StringInterner {
        &mut self.interner
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn psychic_dominator_mut_for_test(
        &mut self,
    ) -> &mut crate::sim::superweapon::psychic_dominator::PsychicDominatorState {
        &mut self.psychic_dominator
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn production_mut_for_test(&mut self) -> &mut ProductionState {
        &mut self.production
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn native_unique_ids_mut_for_test(
        &mut self,
    ) -> &mut Option<crate::sim::native_identity::NativeUniqueIdCursor> {
        &mut self.native_unique_ids
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn mapgen_rng_mut_for_test(&mut self) -> &mut SimRng {
        &mut self.mapgen_rng
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn real_cell_bridge_flags_0x1180_mut_for_test(&mut self) -> &mut RealCellBridgeFlags0x1180 {
        &mut self.real_cell_bridge_flags_0x1180
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn metallic_debris_mut_for_test(&mut self) -> &mut Vec<InternedId> {
        &mut self.metallic_debris
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_fire_events_for_test(&mut self) {
        self.fire_events.clear_for_test();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn bridge_explosions_mut_for_test(&mut self) -> &mut Vec<InternedId> {
        &mut self.bridge_explosions
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn base_reservations_mut_for_test(
        &mut self,
    ) -> &mut crate::sim::cell_rect::CellReservationGrid {
        &mut self.substrate.base_reservations
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_next_stable_object_id_for_test(&mut self, next: u64) {
        self.substrate.next_stable_object_id.set_for_test(next);
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn anims_mut_for_test(&mut self) -> &mut crate::sim::anim_class::AnimStore {
        &mut self.substrate.anims
    }

    /// Saved isometric-tile replacements, reapplied on restore.
    pub fn dynamic_terrain_cells(&self) -> &crate::map::resolved_terrain::DynamicTerrainCells {
        &self.dynamic_terrain_cells
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn dynamic_terrain_cells_mut_for_test(
        &mut self,
    ) -> &mut crate::map::resolved_terrain::DynamicTerrainCells {
        &mut self.dynamic_terrain_cells
    }

    /// Install a newly constructed map grid and capture the real CellClass
    /// `0x1180` value authority after row-major OverlayPack marking.
    pub fn install_resolved_terrain_for_new_map(&mut self, resolved_terrain: ResolvedTerrainGrid) {
        let grid = resolved_terrain.cell_grid();
        self.houses.configure_cell_grid(grid);
        self.terrain_objects.configure_cell_grid(grid);
        self.bind_shared_cell_dummy(resolved_terrain.shared_cell_dummy());
        self.real_cell_bridge_flags_0x1180 =
            resolved_terrain.capture_real_cell_bridge_flags_0x1180();
        self.dynamic_terrain_cells.clear();
        self.resolved_terrain = Some(resolved_terrain);
        // A fresh grid restarts its mutation epoch; drop any plane keyed on the old one.
        self.movement_pass_cache = Default::default();
    }

    /// Apply one live runtime setter and update only the allocated real-cell
    /// serialized values it changed. Missing slots remain owned solely by the
    /// process dummy and are intentionally absent from Scenario payload.
    #[cfg(any(test, feature = "test-support"))]
    pub fn apply_runtime_bridge_flag_stamp(
        &mut self,
        stamp: crate::map::bridge_facts::BridgeFlagStamp,
    ) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        if !terrain.bridge_flag_authority_matches_shape(&self.real_cell_bridge_flags_0x1180) {
            self.real_cell_bridge_flags_0x1180 = terrain.capture_real_cell_bridge_flags_0x1180();
        }
        let updates = self
            .resolved_terrain
            .as_mut()
            .expect("terrain presence checked before runtime bridge setter")
            .apply_runtime_bridge_flag_stamp(stamp);
        for (index, flags) in updates {
            self.real_cell_bridge_flags_0x1180
                .set_allocated_cell(index, flags);
        }
    }

    /// High-bridge anchor construction uses the same literal setter as bridge
    /// damage/repair, but it additionally creates the family/anchor relations
    /// from which Rust derives live bridge topology.
    pub(crate) fn apply_runtime_bridge_mark_stamp(
        resolved_terrain: &mut Option<ResolvedTerrainGrid>,
        real_cell_bridge_flags_0x1180: &mut RealCellBridgeFlags0x1180,
        stamp: crate::map::bridge_facts::BridgeFlagStamp,
        family: crate::map::bridge_facts::BridgeStampFamily,
    ) {
        let Some(terrain) = resolved_terrain.as_ref() else {
            return;
        };
        if !terrain.bridge_flag_authority_matches_shape(real_cell_bridge_flags_0x1180) {
            *real_cell_bridge_flags_0x1180 = terrain.capture_real_cell_bridge_flags_0x1180();
        }
        let updates = resolved_terrain
            .as_mut()
            .expect("terrain presence checked before runtime bridge Mark setter")
            .apply_runtime_bridge_mark_stamp(stamp, family);
        for (index, flags) in updates {
            real_cell_bridge_flags_0x1180.set_allocated_cell(index, flags);
        }
    }

    /// Synthetic fixtures may assign a detached grid directly. Production
    /// construction binds both owners, but gameplay must still read the live
    /// handle attached to the actual CellClass table it queried.
    pub fn effective_shared_cell_dummy(&self) -> SharedCellDummy {
        self.resolved_terrain
            .as_ref()
            .map(ResolvedTerrainGrid::shared_cell_dummy)
            .unwrap_or_else(|| self.shared_cell_dummy.clone())
    }

    /// Install the Scenario cursor advanced by the pre-IsoMapPack Fill pass.
    /// Called only by sim's opaque bootstrap owner; Main remains independently
    /// owned by the terrain variant selector.
    pub(in crate::sim) fn install_terrain_load_advanced_scenario_rng(
        &mut self,
        scenario_rng: SimRng,
    ) {
        self.scenario_rng = scenario_rng;
    }

    /// Install the Main cursor advanced by one-time terrain variant-table
    /// generation. Called only by sim's opaque bootstrap owner; Scenario
    /// remains independently owned by the Fill pass.
    pub(in crate::sim) fn install_variant_advanced_main_rng(&mut self, main_rng: SimRng) {
        self.main_rng = main_rng;
    }

    /// Bind a successful fresh or saved scenario to the retained process Main
    /// cell: the process cell takes this simulation's staged cursor, and the
    /// simulation continues from that same cursor in its own generator.
    /// App orchestration must first cancel the outgoing playback callbacks.
    ///
    /// Residual (sim-scale deviation, docs/sim-thread.md): gamemd's one Main
    /// RNG serves the logic's sounds and the interface alike; here the
    /// simulation thread and the app each draw their own copy from the
    /// install on. Main is outside saves and the state hash, so only which
    /// random voice or variation plays can differ from gamemd.
    pub fn install_fresh_process_main(&mut self, process: &MainRng) {
        // Two streams from one cursor is the intent: from here the app and
        // the simulation each draw their own.
        process.install(self.main_rng.clone());
    }

    /// Install the process MapGen cursor prepared by the bootstrap owner.
    pub(in crate::sim) fn install_generated_mapgen_rng(&mut self, mapgen_rng: SimRng) {
        self.mapgen_rng = mapgen_rng;
    }

    /// Publish a completed RMG run into the resident process cursor. Original
    /// Generate598960 advances MapGen in place and returns without restoring
    /// it; preview OK/Cancel leave it unchanged (59664C,59951F..599527).
    pub fn install_mapgen_continuation(
        &mut self,
        continuation: crate::rng_continuation::MapGenRngContinuation,
    ) {
        self.install_generated_mapgen_rng(SimRng::from_mapgen_continuation(continuation));
    }

    /// Carry the resident process cursor through a fresh Scenario replacement.
    /// Original52FC20 reseeds Scenario/Main only. The outgoing world remains
    /// inactive throughout loading, so this opaque snapshot has no competing
    /// draw authority and the fresh bootstrap consumes it exactly once.
    pub fn mapgen_continuation_for_fresh_load(
        &self,
    ) -> crate::rng_continuation::MapGenRngContinuation {
        self.mapgen_rng.mapgen_continuation()
    }

    /// Campaign ClearScene/SetDefaults retain these Scenario flag bits. The
    /// inactive loader can carry their existing owners' draw-free projection
    /// through map reads without creating a second mutable flags word.
    /// Original683610/686B20; cold Scenario ctor6833F3 word0x8088.
    pub fn retained_campaign_special_flags(&self) -> crate::map::basic::SpecialFlagsSection {
        crate::map::basic::SpecialFlagsSection {
            inert: Some(self.session.no_damage()),
            tiberium_grows: Some(self.session.tiberium_grows_flag()),
            tiberium_spreads: Some(self.session.tiberium_spreads_flag()),
            destroyable_bridges: Some(
                self.bridge_state
                    .as_ref()
                    .is_none_or(|state| state.is_destroyable()),
            ),
            harvester_immune: Some(self.session.harvester_immune()),
            ..Default::default()
        }
    }
}
