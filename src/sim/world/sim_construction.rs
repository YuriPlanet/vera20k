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
use crate::util::forked::Forked;

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
        *self.rule_handles = Some(crate::sim::type_handle_table::ResolvedRuleHandles::resolve(
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
        *self.bridge_explosions = rules
            .bridge_rules
            .explosions
            .iter()
            .map(|name| self.interner.intern(name))
            .collect();
        *self.metallic_debris = rules
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
        *self.rule_handles
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
        *simulation.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        *simulation.state_hash_interval = None;
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
            path_batch: Forked::new(None),
            idle_sleep: Default::default(),
            scan_batch: Forked::new(None),
            effect_anims: Default::default(),
            turn_streams: Default::default(),
            interner: Forked::new(crate::sim::intern::StringInterner::new()),
            type_tech_levels: Forked::new(TypeTechLevels::default()),
            rule_handles: Forked::new(None),
            production: Forked::new(ProductionState::default()),
            terrain_objects: Default::default(),
            ore_growth_config: Forked::new(crate::sim::ore_growth::OreGrowthConfig::disabled()),
            ore_growth_state: Forked::new(crate::sim::ore_growth::OreGrowthState::new(0, 0)),
            session: Forked::new(session),
            #[cfg(feature = "debug-tools")]
            debug_tools: Default::default(),
            frame_profiling: Forked::new(false),
            frame_profile: Default::default(),
            object_placement_scope_depth: Forked::new(0),
            buildup_bypass: Forked::new(false),
            #[cfg(feature = "phobos")]
            reflected_warhead: Forked::new(None),
            ares_made_up_rules: Forked::new(None),
            scenario_rng: Forked::new(SimRng::new(seed)),
            main_rng: Forked::new(SimRng::new(seed)),
            mapgen_rng: Forked::new(SimRng::new(0)),
            native_unique_ids: Forked::new(None),
            authored_tiberium_value_total: Forked::new(None),
            post_load_particle_system_constructed: Default::default(),
            native_map_tubes: Forked::new(crate::map::tubes::NativeMapTubes::default()),
            load_objects: Forked::new(LoadObjectLifecycle::default()),
            fog: Forked::new(FogState::default()),
            house_alliances: Default::default(),
            substrate: ObjectSubstrate::new(),
            lifecycle_outputs: Forked::new(FrameChannel::default()),
            unit_voice_visit_interest: Default::default(),
            mission_spawned_entities: Forked::new(false),
            ice_zones_stale: Forked::new(false),
            ownership_changed_since_output: Forked::new(false),
            frame_overlay_updates: Forked::new(Vec::new()),
            frame_overlay_removals: Forked::new(Vec::new()),
            terminal_score_snapshot: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            lifecycle_test_events: Forked::new(Vec::new()),
            #[cfg(test)]
            receiver_fixture: Forked::new(None),
            trigger_effects: Forked::new(Vec::new()),
            revision: Forked::new(0),
            #[cfg(any(test, feature = "test-support"))]
            frame_step_trace: Forked::new(Vec::new()),
            #[cfg(any(test, feature = "test-support"))]
            house_ai_activation_order_test_trace: Forked::new(Vec::new()),
            sound_events: Forked::new(FrameChannel::default()),
            lighting_sources: Forked::new(crate::sim::light_sources::LightingSources::default()),
            fire_events: Forked::new(FrameChannel::default()),
            combat_light_requests: Forked::new(FrameChannel::default()),
            screen_shake_updates: Forked::new(FrameChannel::default()),
            projectiles: Forked::new(crate::sim::projectile::ProjectileStore::new()),
            #[cfg(feature = "phobos")]
            warhead_anim_runtime: Default::default(),
            waves: Forked::new(crate::sim::wave::WaveStore::new()),
            active_wave_links: Forked::new(BTreeMap::new()),
            object_custom_sounds: Forked::new(BTreeMap::new()),
            #[cfg(any(test, feature = "test-support"))]
            bale_events: Forked::new(Vec::new()),
            #[cfg(any(test, feature = "test-support"))]
            bunker_wall_events: Forked::new(Vec::new()),
            team_script_vm: Forked::new(TeamScriptVm::default()),
            houses: Forked::new(crate::sim::houses::Houses::default()),
            zone_grid: Forked::new(None),
            path_grid: Forked::new(None),
            structure_navigation_cells: Forked::new(BTreeSet::new()),
            resolved_terrain: Forked::new(None),
            shared_cell_dummy: Forked::new(SharedCellDummy::fresh()),
            native_local_cell_cursor: Forked::new(None),
            native_allocated_cell_cursor: Forked::new(None),
            real_cell_bridge_flags_0x1180: Forked::new(RealCellBridgeFlags0x1180::default()),
            dynamic_terrain_cells: Default::default(),
            ice_cracks: Forked::new(Vec::new()),
            terrain_dent: Forked::new(None),
            bridge_state: Forked::new(None),
            pending_bridge_collapses: Default::default(),
            overlay_grid: Forked::new(None),
            crate_authority: Forked::new(crate::sim::crates::CrateAuthority::default()),
            smudge_grid: Forked::new(None),
            radiation: Forked::new(crate::sim::radiation::RadiationState::default()),
            bombs: Forked::new(crate::sim::bomb::BombList::default()),
            kamikaze: Forked::new(crate::sim::kamikaze::KamikazeTracker::default()),
            disk_lasers: Forked::new(disk_laser::DiskLasers::default()),
            playfield_bounds: Forked::new(None),
            playfield_size_height: Forked::new(None),
            playfield_revision: Forked::new(0),
            bridge_explosions: Forked::new(Vec::new()),
            metallic_debris: Forked::new(Vec::new()),
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
            ion_blasts: Forked::new(Vec::new()),
            alpha_shapes: Default::default(),
            terrain_speed_config: Forked::new(terrain_speed::TerrainSpeedConfig::default()),
            input_delay_ticks: Forked::new(u64::from(
                crate::util::config::default_input_delay_ticks(),
            )),
            quit_requested: Forked::new(false),
            executed_exit_owner: Forked::new(None),
            departed_senders: Forked::new(Vec::new()),
            connection_lost: Forked::new(false),
            frame_sync_checksum: Forked::new(None),
            state_hash_interval: every_frame(),
            pending_commands: Default::default(),
            local_commands: Default::default(),
            dispatch_reserved_slots: Default::default(),
            frame_command_admissions: Forked::new(Vec::new()),
            frame_continuation: Forked::new(None),
            planning: Forked::new(planning::PlanningState::default()),
            replaying_recorded_commands: Forked::new(false),
            trigger_runtime: Forked::new(TriggerRuntime::default()),
            ext_modules: Default::default(),
        };
        debug_assert_eq!(out.scenario_rng.state(), out.main_rng.state());
        // Authoritative map bounds are session state, known at construction —
        // vision must never run against a zero-dim first-tick window. Fixture
        // sims built without a descriptor (zero bounds) keep the lazy
        // derivation as a fallback inside the vision recompute.
        *out.fog = FogState::new(out.session.map_width(), out.session.map_height());
        out
    }

    // --- Scenario stream (gamemd Scenario->Random @ Scen+0x218) ---
    // Keep accessors distinct even though several return the same stream today:
    // the intent name is the per-consumer routing record and the grep/audit anchor.
    // Consumers that borrow `scenario_rng` directly beside other fields: infantry
    // sub-cell rotation and paradrop sub-cell; destruction smudge, survivor and
    // debris and the smudge type pick; the overlay/wall damage roll; ore growth
    // and spread (queue, direction, variant, TIBTRE); the building damage-fire
    // type and start frame; a Smoke system's children's AI
    // (`ParticleSystemStore::system_and_rng_mut`).
    pub(crate) fn bridge_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // bridge collapse/debris/explosion
    /// Particle/smoke/gas/fire lifetime/offset/dir/insert draws: Scenario's,
    /// or while presentation particles are made or run
    /// (`Simulation::present_particles`), their own stream, so the shared one
    /// stays as the other games have it.
    pub(crate) fn particle_rng(&mut self) -> &mut SimRng {
        self.particle_stream()
    }
    /// [`Self::particle_rng`]'s stream, not yet taken for writing, for a
    /// consumer that draws only on some visits (the Smoke AI).
    pub(crate) fn particle_stream(&mut self) -> &mut Forked<SimRng> {
        if self.substrate.particle_systems.presenting() {
            self.substrate.particle_systems.presented_rng()
        } else {
            &mut self.scenario_rng
        }
    }
    pub(crate) fn superweapon_rng(&mut self) -> &mut SimRng {
        &mut self.scenario_rng
    } // lightning-storm scatter/bolt
    /// Capture the process-continuity Scenario cursor when gameplay returns to
    /// the frontend. The app stores this clone until the next successful Start
    /// reseeds the gameplay pair.
    pub fn clone_scenario_rng(&self) -> SimRng {
        (*self.scenario_rng).clone()
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
    ///   state only gameplay reads (the triggers and the pointer-expiry index,
    ///   both refusing reads in debug builds like the entity store, the zone
    ///   and path grids, the team script VM, the movement pass's kept
    ///   products, the reveal pass's answers, the Houses' spatial threat
    ///   grids, the AI's base reservations and the structures' navigation
    ///   cells);
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
        let mut fog = std::mem::take(&mut *self.fog);
        let entities = std::mem::replace(&mut *self.substrate.entities, EntityStore::detached());
        // In the scale benchmark's battle the triggers took half the copy's
        // time and the expiry index a seventh.
        let trigger_runtime = std::mem::replace(
            &mut *self.trigger_runtime,
            crate::sim::trigger_runtime::TriggerRuntime::detached(),
        );
        let expiry_listeners = std::mem::replace(
            &mut *self.expiry_listeners,
            super::expiry_listeners::ExpiryListeners::detached(),
        );
        let zone_grid = self.zone_grid.take();
        let path_grid = self.path_grid.take();
        let team_script_vm = std::mem::take(&mut *self.team_script_vm);
        let movement_pass_cache = std::mem::take(&mut *self.movement_pass_cache);
        let reveal_viewers = std::mem::take(&mut *self.reveal_viewers);
        // CellClass +0xDC, one word per reserved cell: a city map's thousands
        // of buildings reserve tens of thousands, which cloned took most of
        // the copy. Only the AI's base planning reads them.
        let base_reservations = std::mem::take(&mut *self.substrate.base_reservations);
        let structure_navigation_cells = std::mem::take(&mut *self.structure_navigation_cells);
        // Each House's spatial threat grid (a large map's is 66,564 dwords,
        // thirty Houses about 8 MB) is AI state only gameplay reads.
        let spatial_threats: Vec<_> = self
            .houses
            .values_mut()
            .map(HouseState::take_spatial_threat)
            .collect();
        // The other large bodies the copy carries but no draw writes: a House
        // array and the substrate stores whose clone ran on the main thread's
        // critical path. Each is taken out and cloned on the pool below; the
        // placeholders are empty and cost no clone here.
        let houses = std::mem::take(&mut *self.houses);
        let production = std::mem::take(&mut *self.production);
        let logic = std::mem::take(&mut *self.substrate.logic);
        let display = std::mem::take(&mut *self.substrate.display);
        let voxel_anims = std::mem::take(&mut *self.substrate.voxel_anims);
        let spotlights = std::mem::take(&mut *self.substrate.spotlights);
        let particle_systems = std::mem::take(&mut *self.substrate.particle_systems);
        let pending_delete = std::mem::take(&mut *self.substrate.pending_delete);
        #[cfg(any(test, feature = "test-support"))]
        let test_recorders = (
            std::mem::take(&mut *self.lifecycle_test_events),
            std::mem::take(&mut *self.frame_step_trace),
            std::mem::take(&mut *self.house_ai_activation_order_test_trace),
        );
        // What it syncs. The previous copy's other parts are freed on the
        // pool, and the syncs run there beside the clone of the rest.
        let mut synced = SyncedWorld::take(self);
        let (mut theirs, old_fog) = reuse.map_or_else(Default::default, |mut old| {
            let theirs = SyncedWorld::take(&mut old);
            let old_fog = std::mem::take(&mut *old.fog);
            crate::sim::parallel::drop_on_pool(old);
            (theirs, old_fog)
        });
        // The Anim store, which took half the clone of the rest in the scale
        // benchmark's battle, is cloned beside it.
        let anims = std::mem::take(&mut *self.substrate.anims);
        let mut copy = None;
        let mut fog_copy = None;
        let mut anims_copy = None;
        let mut houses_copy = None;
        let mut production_copy = None;
        let mut logic_copy = None;
        let mut display_copy = None;
        let mut voxel_copy = None;
        let mut spotlights_copy = None;
        let mut particle_copy = None;
        let mut pending_copy = None;
        let this = &*self;
        let fog_ref = &mut fog;
        let anims_ref = &anims;
        let houses_ref = &houses;
        let production_ref = &production;
        let logic_ref = &logic;
        let display_ref = &display;
        let voxel_ref = &voxel_anims;
        let spotlights_ref = &spotlights;
        let particle_ref = &particle_systems;
        let pending_ref = &pending_delete;
        // The parts share only reads, so they run at once: the syncs, the
        // viewer's fog, the anims, the House array and the substrate stores
        // on the pool, the clone of the small rest here.
        crate::sim::parallel::alongside(
            || synced.sync_into(&mut theirs),
            || {
                crate::sim::parallel::alongside(
                    || {
                        houses_copy = Some(houses_ref.presentation_copy());
                        production_copy = Some(production_ref.clone());
                        logic_copy = Some(logic_ref.clone());
                        display_copy = Some(display_ref.clone());
                        voxel_copy = Some(voxel_ref.clone());
                        spotlights_copy = Some(spotlights_ref.clone());
                        particle_copy = Some(particle_ref.clone());
                        pending_copy = Some(pending_ref.clone());
                    },
                    || {
                        crate::sim::parallel::alongside(
                            || {
                                fog_copy = Some(fog_ref.presentation_copy(viewer, old_fog));
                                anims_copy = Some(anims_ref.clone());
                            },
                            || copy = Some(this.clone()),
                        )
                    },
                )
            },
        );
        // Put the live world's House bodies back before restoring their
        // threat grids and before its sync writes into them.
        *self.houses = houses;
        for (house, grid) in self.houses.values_mut().zip(spatial_threats) {
            house.restore_spatial_threat(grid);
        }
        let mut copy = copy.expect("cloned beside the syncs");
        let fog_copy = fog_copy.expect("fog copied beside the clone");
        *self.production = production;
        *self.substrate.logic = logic;
        *self.substrate.display = display;
        *self.substrate.voxel_anims = voxel_anims;
        *self.substrate.spotlights = spotlights;
        *self.substrate.particle_systems = particle_systems;
        *self.substrate.pending_delete = pending_delete;
        synced.put(self);
        *copy.houses = houses_copy.expect("houses cloned beside the rest");
        *copy.production = production_copy.expect("production cloned beside the rest");
        *copy.substrate.logic = logic_copy.expect("logic order cloned beside the rest");
        *copy.substrate.display = display_copy.expect("display layers cloned beside the rest");
        *copy.substrate.voxel_anims = voxel_copy.expect("voxel anims cloned beside the rest");
        *copy.substrate.spotlights = spotlights_copy.expect("spotlights cloned beside the rest");
        *copy.substrate.particle_systems =
            particle_copy.expect("particle systems cloned beside the rest");
        *copy.substrate.pending_delete =
            pending_copy.expect("pending deletes cloned beside the rest");
        theirs.put(&mut copy);
        *copy.fog = fog_copy;
        *copy.substrate.anims = anims_copy.expect("anims cloned beside the rest");
        *copy.house_alliances = self
            .house_alliances
            .detached(&self.interner, &copy.interner);
        *copy.shared_cell_dummy = copy.shared_cell_dummy.fork_query_identity();
        *self.fog = fog;
        *self.substrate.anims = anims;
        *self.substrate.entities = entities;
        *self.trigger_runtime = trigger_runtime;
        *self.expiry_listeners = expiry_listeners;
        *self.zone_grid = zone_grid;
        *self.path_grid = path_grid;
        *self.team_script_vm = team_script_vm;
        *self.movement_pass_cache = movement_pass_cache;
        *self.reveal_viewers = reveal_viewers;
        *self.substrate.base_reservations = base_reservations;
        *self.structure_navigation_cells = structure_navigation_cells;
        #[cfg(any(test, feature = "test-support"))]
        {
            (
                *self.lifecycle_test_events,
                *self.frame_step_trace,
                *self.house_ai_activation_order_test_trace,
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
