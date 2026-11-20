//! `Simulation` construction and rule binding: constructors, type-handle
//! resolution, the RNG stream accessors and entity and object store access.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use crate::map::resolved_terrain::RealCellBridgeFlags0x1180;
use crate::map::resolved_terrain::SharedCellDummy;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::pathfinding::terrain_speed;
use crate::sim::production::ProductionState;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::team_script_vm::TeamScriptVm;
use crate::sim::trigger_runtime::TriggerRuntime;
use crate::sim::vision::FogState;

use super::ChangedLooks;
#[cfg(any(test, feature = "test-support"))]
use super::DEFAULT_SIM_SEED;
use super::FrameChannel;
use super::LoadObjectLifecycle;
use super::ObjectSubstrate;
use super::Simulation;
use super::SyncedWorld;
use super::TypeTechLevels;
use super::disk_laser;
use super::display_layers;
use super::every_frame;
use super::planning;

impl Simulation {
    /// Create a new empty simulation with the test fixtures' seed. Test builds
    /// only: a launch constructs through `ScenarioDescriptor`
    /// (`Simulation::from_descriptor`, `ScenarioBootstrapRng::into_simulation`),
    /// so no match can start on the default seed (AT-3).
    #[cfg(any(test, feature = "test-support"))]
    pub fn new() -> Self {
        Self::with_seed(DEFAULT_SIM_SEED)
    }

    /// Bind the interner's type handles ([`StringInterner::bind_types`]) and
    /// build the pre-resolved rule handles beside them. Call once at sim init
    /// AFTER `intern_rule_type_ids`, and again after load once the RuleSet is
    /// available. Idempotent: the warhead names are already interned on every
    /// re-resolution path, so serialized interner state cannot grow or reorder
    /// (a save made before MoveFlash's name was interned here gains it at the
    /// end on load).
    ///
    /// MoveFlash (Rules+0xBB4) enters the interner here, after the rules
    /// types, as it does in every game of a match: a LAN or online flash only
    /// looks its name up (`spawn_move_flash`), since a name only the clicking
    /// game added would renumber that game's later names.
    ///
    /// [`StringInterner::bind_types`]: crate::sim::intern::StringInterner::bind_types
    pub fn resolve_type_handles(&mut self, rules: &RuleSet) {
        self.interner.bind_types(rules);
        self.rule_handles = Some(crate::sim::type_handle_table::ResolvedRuleHandles::resolve(
            rules,
            &mut self.interner,
        ));
        if !rules.general.move_flash.name.is_empty() {
            self.interner.intern(&rules.general.move_flash.name);
        }
    }

    /// Intern every rules type id (infantry, vehicle, aircraft, building,
    /// `[SuperWeaponTypes]`) so `interner.get(type_id)` succeeds for any type
    /// this ruleset references, and a command naming one carries the same id
    /// in every world built from these rules: the Chrono Warp is clicked
    /// without ever being granted. Moved from `RuleSet::intern_all_ids` (F04):
    /// interning is sim-side work over rules-owned canonical names.
    pub fn intern_rule_type_ids(&mut self, rules: &RuleSet) {
        for id in rules
            .infantry_ids
            .iter()
            .chain(&rules.vehicle_ids)
            .chain(&rules.aircraft_ids)
            .chain(&rules.building_ids)
            .chain(&rules.super_weapon_order)
        {
            self.interner.intern(id);
        }
    }

    /// Resolve the rules-owned ordered AnimType lists in this world's interner.
    /// These IDs are derived, never portable between independently constructed
    /// worlds. Native ReadGeneral owns the list names/order; both map binding
    /// and snapshot restoration project those names through this one writer.
    pub fn resolve_rule_animation_lists(&mut self, rules: &RuleSet) {
        self.bridge_explosions = rules
            .bridge_rules
            .explosions
            .iter()
            .map(|name| self.interner.intern(name))
            .collect();
        self.metallic_debris = rules
            .general
            .metallic_debris
            .iter()
            .map(|name| self.interner.intern(name))
            .collect();
    }

    /// Pre-resolved rule handles for combat comparisons.
    ///
    /// # Panics
    /// Panics if `resolve_type_handles` has not run (init and load both call it
    /// before any combat tick).
    pub fn rule_handles(&self) -> crate::sim::type_handle_table::ResolvedRuleHandles {
        self.rule_handles.expect(
            "Simulation::resolve_type_handles must run at sim init \
             before combat reads warhead handles",
        )
    }

    /// The rule handles, or `None` before `resolve_type_handles` has run.
    pub(crate) fn resolved_rule_handles(
        &self,
    ) -> Option<crate::sim::type_handle_table::ResolvedRuleHandles> {
        self.rule_handles
    }

    /// Resolve an entity's type to its `ObjectType` through the interner's
    /// bound handles (`StringInterner::object_type`): two array indexes, no
    /// hashing, and the name path wherever the handles do not apply.
    #[inline]
    pub fn object_type<'r>(
        &self,
        type_ref: InternedId,
        rules: &'r RuleSet,
    ) -> Option<&'r ObjectType> {
        self.interner.object_type(type_ref, rules)
    }

    /// Create a new empty simulation with an explicit deterministic seed.
    /// Test/dev entry — u64 seeds wider than 32 bits keep their full value in
    /// `session.seed` (pinned harness baselines depend on it) even though the
    /// stream seeder consumes only 32 bits. Its frames hash their state for
    /// the TickResult only once [`Self::hash_state_every`] asks, or when they
    /// end the match: hashing the whole world after every frame is most of an
    /// unoptimized frame's time.
    pub fn with_seed(seed: u64) -> Self {
        Self::synthetic(
            seed,
            &crate::sim::scenario_session::ScenarioDescriptor::default(),
        )
    }

    /// [`Self::with_seed`] over a supplied descriptor, for dev tooling that
    /// needs launch fields such as the map extent without loading a map.
    pub fn synthetic(seed: u64, desc: &crate::sim::scenario_session::ScenarioDescriptor) -> Self {
        let mut session = ScenarioSession::from_descriptor(desc);
        session.record_random_seed(seed);
        let mut simulation = Self::construct(session);
        simulation.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        simulation.state_hash_interval = None;
        simulation
    }

    /// Construct a session simulation from an app-layer launch descriptor.
    /// The only entry real launches use; `new()`/`with_seed()` remain for
    /// tests and dev tooling.
    pub fn from_descriptor(desc: &crate::sim::scenario_session::ScenarioDescriptor) -> Self {
        Self::construct(ScenarioSession::from_descriptor(desc))
    }

    /// Shared constructor body: seed both gameplay streams identically from
    /// the session seed and construct fresh MapGen with native Seed(0) state.
    pub(super) fn construct(session: ScenarioSession) -> Self {
        let seed = session.seed();
        let mut out = Self {
            movement_pass_cache: Default::default(),
            reveal_viewers: Default::default(),
            civilian_house_memo: Default::default(),
            expiry_listeners: Default::default(),
            path_batch: None,
            idle_sleep: Default::default(),
            scan_batch: None,
            effect_anims: Default::default(),
            interner: crate::sim::intern::StringInterner::new(),
            type_tech_levels: TypeTechLevels::default(),
            rule_handles: None,
            production: ProductionState::default(),
            terrain_objects: Default::default(),
            ore_growth_config: crate::sim::ore_growth::OreGrowthConfig::disabled(),
            ore_growth_state: crate::sim::ore_growth::OreGrowthState::new(0, 0),
            session,
            #[cfg(feature = "debug-tools")]
            debug_tools: Default::default(),
            frame_profiling: false,
            frame_profile: Default::default(),
            object_placement_scope_depth: 0,
            buildup_bypass: false,
            #[cfg(feature = "phobos")]
            reflected_warhead: None,
            ares_made_up_rules: None,
            scenario_rng: SimRng::new(seed),
            main_rng: SimRng::new(seed),
            mapgen_rng: SimRng::new(0),
            native_unique_ids: None,
            authored_tiberium_value_total: None,
            post_load_particle_system_constructed: Default::default(),
            native_map_tubes: crate::map::tubes::NativeMapTubes::default(),
            load_objects: LoadObjectLifecycle::default(),
            fog: FogState::default(),
            house_alliances: Default::default(),
            substrate: ObjectSubstrate::new(),
            lifecycle_outputs: FrameChannel::default(),
            unit_voice_visit_interest: Default::default(),
            mission_spawned_entities: false,
            ownership_changed_since_output: false,
            frame_overlay_updates: Vec::new(),
            frame_overlay_removals: Vec::new(),
            terminal_score_snapshot: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            lifecycle_test_events: Vec::new(),
            #[cfg(test)]
            receiver_fixture: None,
            trigger_effects: Vec::new(),
            revision: 0,
            #[cfg(any(test, feature = "test-support"))]
            frame_step_trace: Vec::new(),
            #[cfg(any(test, feature = "test-support"))]
            house_ai_activation_order_test_trace: Vec::new(),
            sound_events: FrameChannel::default(),
            lighting_sources: crate::sim::light_sources::LightingSources::default(),
            fire_events: FrameChannel::default(),
            combat_light_requests: FrameChannel::default(),
            screen_shake_updates: FrameChannel::default(),
            projectiles: crate::sim::projectile::ProjectileStore::new(),
            #[cfg(feature = "phobos")]
            warhead_anim_runtime: Default::default(),
            waves: crate::sim::wave::WaveStore::new(),
            active_wave_links: BTreeMap::new(),
            object_custom_sounds: BTreeMap::new(),
            #[cfg(any(test, feature = "test-support"))]
            bale_events: Vec::new(),
            #[cfg(any(test, feature = "test-support"))]
            bunker_wall_events: Vec::new(),
            team_script_vm: TeamScriptVm::default(),
            houses: crate::sim::houses::Houses::default(),
            zone_grid: None,
            path_grid: None,
            structure_navigation_cells: BTreeSet::new(),
            resolved_terrain: None,
            shared_cell_dummy: SharedCellDummy::fresh(),
            native_local_cell_cursor: None,
            native_allocated_cell_cursor: None,
            real_cell_bridge_flags_0x1180: RealCellBridgeFlags0x1180::default(),
            dynamic_terrain_cells: Default::default(),
            bridge_state: None,
            pending_bridge_collapses: Default::default(),
            overlay_grid: None,
            crate_authority: crate::sim::crates::CrateAuthority::default(),
            smudge_grid: None,
            radiation: crate::sim::radiation::RadiationState::default(),
            bombs: crate::sim::bomb::BombList::default(),
            kamikaze: crate::sim::kamikaze::KamikazeTracker::default(),
            disk_lasers: disk_laser::DiskLasers::default(),
            playfield_bounds: None,
            playfield_size_height: None,
            playfield_revision: 0,
            bridge_explosions: Vec::new(),
            metallic_debris: Vec::new(),
            dirty_cells: Default::default(),
            power_states: Default::default(),
            building_upgrade_art_names: Default::default(),
            voxel_type_damage: Default::default(),
            super_weapons: Default::default(),
            lightning_storm: Default::default(),
            psychic_dominator: Default::default(),
            unit_deliveries: Default::default(),
            ares_chrono_warps: Default::default(),
            ares_firestorm: Default::default(),
            ares_tunnels: Default::default(),
            ion_blasts: Vec::new(),
            alpha_shapes: Default::default(),
            terrain_speed_config: terrain_speed::TerrainSpeedConfig::default(),
            input_delay_ticks: u64::from(crate::util::config::default_input_delay_ticks()),
            quit_requested: false,
            executed_exit_owner: None,
            departed_senders: Vec::new(),
            connection_lost: false,
            frame_sync_checksum: None,
            state_hash_interval: every_frame(),
            pending_commands: Default::default(),
            local_commands: Default::default(),
            dispatch_reserved_slots: Default::default(),
            frame_command_admissions: Vec::new(),
            frame_continuation: None,
            planning: planning::PlanningState::default(),
            replaying_recorded_commands: false,
            trigger_runtime: TriggerRuntime::default(),
        };
        debug_assert_eq!(out.scenario_rng.state(), out.main_rng.state());
        // Authoritative map bounds are session state, known at construction —
        // vision must never run against a zero-dim first-tick window. Fixture
        // sims built without a descriptor (zero bounds) keep the lazy
        // derivation as a fallback inside the vision recompute.
        out.fog = FogState::new(out.session.map_width(), out.session.map_height());
        out
    }

    // --- Scenario stream (gamemd Scenario->Random @ Scen+0x218) ---
    // Keep accessors distinct even though several return the same stream today:
    // the intent name is the per-consumer routing record and the grep/audit anchor.
    // Consumers that borrow `scenario_rng` directly beside other fields: infantry
    // sub-cell rotation and paradrop sub-cell; destruction smudge, survivor and
    // debris and the smudge type pick; the overlay/wall damage roll; ore growth
    // and spread (queue, direction, variant, TIBTRE); the building damage-fire
    // type and start frame.
    pub(crate) fn bridge_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // bridge collapse/debris/explosion
    pub(crate) fn particle_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // particle/smoke/gas/fire lifetime/offset/dir/insert
    pub(crate) fn superweapon_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // lightning-storm scatter/bolt
    /// Capture the process-continuity Scenario cursor when gameplay returns to
    /// the frontend. The app stores this clone until the next successful Start
    /// reseeds the gameplay pair.
    pub fn clone_scenario_rng(&self) -> SimRng {
        self.scenario_rng.clone()
    }

    // --- Main/global process stream (`main_rng`) ---
    // Its owning modules draw the same generator through `&mut`: terrain-load
    // variants (`terrain_load_draws`), selection_voice, Gattling Report, death
    // sounds and MoveSound. Local selection calls the domain operation; it must
    // not create a seeded copy that loses loading or live-consumer continuation.

    /// The occupancy grid (per-cell object lists). Read access for systems above sim/.
    pub fn occupancy(&self) -> &OccupancyGrid {
        &self.substrate.occupancy
    }

    /// Mutable occupancy access for the few above-sim callers that unmark cells.
    #[cfg(any(test, feature = "test-support"))]
    pub fn occupancy_mut(&mut self) -> &mut OccupancyGrid {
        &mut self.substrate.occupancy
    }

    /// The entity store. Read access for systems above sim/.
    pub fn entities(&self) -> &EntityStore {
        &self.substrate.entities
    }

    /// The objects that may draw differently since the last take, leaving
    /// none: for the presentation's atlas check, which then checks only
    /// those. The simulation never reads these logs.
    pub fn take_looks(&mut self) -> ChangedLooks {
        ChangedLooks {
            entities: self.substrate.entities.take_looks(),
            anims: self.substrate.anims.take_looks(),
        }
    }

    /// The copy of the world drawing reads while this one ticks elsewhere
    /// (docs/sim-thread.md). It shares no storage with this world:
    ///
    /// - it holds no objects (its entity store is [`EntityStore::detached`];
    ///   the presentation frame holds their draw records), and none of the
    ///   state only gameplay reads (the zone and path grids, the team script
    ///   VM, the movement pass's kept products, the reveal pass's answers,
    ///   the Houses' spatial threat grids, the AI's base reservations and the
    ///   structures' navigation cells);
    /// - its fog is only what `viewer`, the local player, sees
    ///   ([`FogState::presentation_copy`]);
    /// - its terrain, overlays, smudges, terrain objects, interner and
    ///   occupation grids are `reuse`'s, the last copy, brought up to this
    ///   world by copying only what changed since (`util::synced`); without
    ///   `reuse` they are copied whole;
    /// - its Dummy cell and its Main RNG are its own.
    pub fn presentation_copy(
        &mut self,
        viewer: Option<InternedId>,
        reuse: Option<Simulation>,
    ) -> Simulation {
        // What the copy leaves out.
        let mut fog = std::mem::take(&mut self.fog);
        let entities = std::mem::replace(&mut self.substrate.entities, EntityStore::detached());
        let zone_grid = self.zone_grid.take();
        let path_grid = self.path_grid.take();
        let team_script_vm = std::mem::take(&mut self.team_script_vm);
        let movement_pass_cache = std::mem::take(&mut self.movement_pass_cache);
        let reveal_viewers = std::mem::take(&mut self.reveal_viewers);
        // CellClass +0xDC, one word per reserved cell: a city map's thousands
        // of buildings reserve tens of thousands, which cloned took most of
        // the copy. Only the AI's base planning reads them.
        let base_reservations = std::mem::take(&mut self.substrate.base_reservations);
        let structure_navigation_cells = std::mem::take(&mut self.structure_navigation_cells);
        // Each House's spatial threat grid (a large map's is 66,564 dwords,
        // thirty Houses about 8 MB) is AI state only gameplay reads.
        let spatial_threats: Vec<_> = self
            .houses
            .values_mut()
            .map(HouseState::take_spatial_threat)
            .collect();
        #[cfg(any(test, feature = "test-support"))]
        let test_recorders = (
            std::mem::take(&mut self.lifecycle_test_events),
            std::mem::take(&mut self.frame_step_trace),
            std::mem::take(&mut self.house_ai_activation_order_test_trace),
        );
        // What it syncs. The previous copy's other parts are freed on the
        // pool, and the syncs run there beside the clone of the rest.
        let mut synced = SyncedWorld::take(self);
        let (mut theirs, old_fog) = reuse.map_or_else(Default::default, |mut old| {
            let theirs = SyncedWorld::take(&mut old);
            let old_fog = std::mem::take(&mut old.fog);
            crate::sim::parallel::drop_on_pool(old);
            (theirs, old_fog)
        });
        let mut copy = None;
        let mut fog_copy = None;
        let this = &*self;
        let fog_ref = &mut fog;
        // The three parts share only reads, so they run at once: the syncs
        // and the viewer's fog on the pool, the clone of the rest here.
        crate::sim::parallel::alongside(
            || synced.sync_into(&mut theirs),
            || {
                crate::sim::parallel::alongside(
                    || fog_copy = Some(fog_ref.presentation_copy(viewer, old_fog)),
                    || copy = Some(this.clone()),
                )
            },
        );
        for (house, grid) in self.houses.values_mut().zip(spatial_threats) {
            house.restore_spatial_threat(grid);
        }
        let mut copy = copy.expect("cloned beside the syncs");
        let fog_copy = fog_copy.expect("fog copied beside the clone");
        synced.put(self);
        theirs.put(&mut copy);
        copy.fog = fog_copy;
        copy.house_alliances = self
            .house_alliances
            .detached(&self.interner, &copy.interner);
        copy.shared_cell_dummy = copy.shared_cell_dummy.fork_query_identity();
        self.fog = fog;
        self.substrate.entities = entities;
        self.zone_grid = zone_grid;
        self.path_grid = path_grid;
        self.team_script_vm = team_script_vm;
        self.movement_pass_cache = movement_pass_cache;
        self.reveal_viewers = reveal_viewers;
        self.substrate.base_reservations = base_reservations;
        self.structure_navigation_cells = structure_navigation_cells;
        #[cfg(any(test, feature = "test-support"))]
        {
            (
                self.lifecycle_test_events,
                self.frame_step_trace,
                self.house_ai_activation_order_test_trace,
            ) = test_recorders;
        }
        copy
    }

    /// The live terrain objects and their indexes. Read access for systems
    /// above sim/.
    pub fn terrain_objects(&self) -> &crate::sim::terrain_object::TerrainObjectStore {
        &self.terrain_objects
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn terrain_objects_mut_for_test(
        &mut self,
    ) -> &mut crate::sim::terrain_object::TerrainObjectStore {
        &mut self.terrain_objects
    }

    /// Logic scheduling order, also consumed by the current radar pipeline.
    pub fn logic_order(&self) -> &[u64] {
        self.substrate.logic.as_slice()
    }

    /// Retained Display membership and order. Tactical6D8F19..6D95A9 walks
    /// these five vectors directly; it neither queries GetLayer nor re-sorts.
    pub fn display_layers(&self) -> &display_layers::DisplayLayers {
        &self.substrate.display
    }

    /// Client-crate fixtures; in-crate code uses the substrate's entity store.
    #[cfg(any(test, feature = "test-support"))]
    pub fn entities_mut_for_test(&mut self) -> &mut EntityStore {
        &mut self.substrate.entities
    }

    #[cfg(test)]
    pub(crate) fn ground_occupation_bits_for_test(&self, rx: u16, ry: u16) -> u8 {
        self.substrate.raw_cell_occupation.ground_bits(rx, ry)
    }

    /// Borrow one fixture actor, recording the same touch as a simulation writer.
    #[cfg(any(test, feature = "test-support"))]
    pub fn entity_mut_for_test(
        &mut self,
        stable_id: u64,
    ) -> Option<&mut crate::sim::game_entity::GameEntity> {
        self.substrate.entities.get_mut(stable_id)
    }

    /// Disjoint access to the entity store (mutable) and the interner (shared)
    /// for callers that need both at once.
    pub(in crate::sim) fn entities_mut_and_interner(
        &mut self,
    ) -> (&mut EntityStore, &crate::sim::intern::StringInterner) {
        (&mut self.substrate.entities, &self.interner)
    }

    /// Resolve an InternedId back to its display string.
    #[inline]
    pub fn resolve(&self, id: crate::sim::intern::InternedId) -> &str {
        self.interner.resolve(id)
    }

    /// Client-crate fixture for a pending radar batch.
    #[cfg(any(test, feature = "test-support"))]
    pub fn mark_radar_terrain_dirty_for_test(
        &mut self,
        cells: impl IntoIterator<Item = (u16, u16)>,
    ) {
        self.dirty_cells.mark_radar(cells);
    }
}
