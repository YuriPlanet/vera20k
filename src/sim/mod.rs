//! Deterministic game state, object lifecycle and ordered simulation systems.
//!
//! Positions commonly use fixed-point arithmetic. Mechanisms whose native
//! semantics require floating point retain their documented numeric behavior
//! (for example `bounce` and `voxel_anim`); deterministic behavior depends on
//! preserving each mechanism's operations, widths and RNG order.
//!
//! RuleSet supplies authored rules/art data. Native hardcoded rules and
//! constants carry evidence near their implementation; see AGENTS.md.
//!
//! ## Key types
//! - `world::Simulation` owns mutable gameplay state and object lifecycle.
//! - `runtime::SimRuntime` binds that state to match rules and map resources.
//! - `game_entity::GameEntity` holds one techno's components and runtime state.
//! - Systems include movement, combat, harvesting, production and pathfinding.
//!
//! ## Dependency rules
//! - sim/ may depend on rules/, map/ and util/.
//! - sim/ never depends on render/, ui/, audio/ or net/.
//! - Commands enter and outputs leave through simulation APIs; presentation
//!   consumes state without becoming a gameplay authority.

// --- Core types: entity storage, components, commands, RNG, interning ---
pub(crate) mod ai_base_building;
pub(crate) mod ai_base_defense;
pub(crate) mod ai_base_site;
pub mod ammo;
pub mod anim_class;
pub(crate) mod object_fall;
#[cfg(any(test, feature = "test-support"))]
pub mod arena_fixture;
pub(crate) mod build_site;
pub mod building_art;
pub(crate) mod building_upgrades;
pub(crate) mod building_power;
pub(crate) mod building_entry;
pub mod building_construction;
pub(crate) mod base_plan;
pub(crate) mod base_plan_generation;
pub mod bomb;
pub(crate) mod c4_plant;
pub mod campaign_carry_over; // the record 0x008A38E0 one campaign mission hands to the next start
pub mod capture_manager;
pub mod cloak_disguise;
pub mod command;
pub mod command_queue;
pub mod components;
#[cfg(any(test, feature = "test-support"))]
pub mod health_ratio_fixture;
pub mod crates; // scenario-start crate placement (Post_Map_Init step 3)
pub mod credit_income; // oil-derrick ProduceCash + Floating Disc money drain (object-loop money)
pub(crate) mod conversion_health;
pub(crate) mod crew_survival; // building SpawnSurvivors and vehicle crew escape
#[cfg(feature = "ares")]
pub(crate) mod vehicle_theft;
pub mod economy; // per-house wallet/storage/statistics value-type (production+economy substrate)
#[cfg(feature = "ares")]
pub(crate) mod factory_owners;
#[cfg(feature = "telemetry")]
pub mod frame_profile;
#[cfg(feature = "telemetry")]
pub mod telemetry;
#[cfg(feature = "debug-tools")]
pub mod cheats;
#[cfg(feature = "debug-tools")]
pub mod debug_event_log;
pub mod dirty_cells;
pub mod entity_store;
pub(crate) mod estimated_health;
pub(crate) mod flashing;
pub mod game_entity;
pub(crate) mod house_threat;
pub(crate) mod hunter_seeker;
pub mod intern;
pub(crate) mod kamikaze; // missiles out of their launcher's control (0x00ABC5F8)
pub(crate) mod lifecycle_request;
pub(crate) mod locomotor_lift; // the Magnetron's IsLocomotor= lift (ImbueLocomotor 0x00710000)
pub(crate) mod powered_unit; // Robot Tanks offline without a PowersUnit= building (Deactivate 0x0070FC90)
pub mod light_sources;
pub mod multiplayer_checksum;
pub mod native_identity;
#[cfg_attr(
    all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"),
    path = "parallel_web.rs"
)]
pub(crate) mod parallel; // order-preserving work over scoped threads
#[cfg(all(test, feature = "wasm", not(target_arch = "wasm32")))]
#[path = "parallel_web.rs"]
mod parallel_web_tests;
pub mod rng;
pub mod scenario_bootstrap;
pub mod scenario_post_map;
pub(crate) mod secret_lab;
pub mod scenario_session; // app->sim launch descriptor (per-match seed pipeline)
pub mod score;
pub mod sensor_lifecycle;
pub mod spy_infiltration; // BuildingClass::Infiltrate (a spy's effects on the building it enters)
pub(crate) mod stage;
pub use stage::StageClass;
pub(crate) mod state_hasher;
pub mod temporal;
pub(crate) mod tesla_charging;
pub mod timer; // signed frame-anchored countdown primitive
pub(crate) mod touch_log; // which stored objects were handed out mutably, per reader
pub mod type_handle_table; // InternedId -> TypeHandle, one-hop entity->type resolution

// --- Pure read-only deterministic engine-data services (gamemd-exact lookup tables) ---

// --- Subsystem folders (multi-file subsystems with internal mod.rs) ---
pub mod bounce;
pub mod combat; // targeting, weapons, AOE, fire gates, damage resolution
pub mod miner; // harvester state machine, dock sequences, ore delivery
pub mod production; // build queue, placement, economy, tech tree, selling
pub mod voxel_anim;
pub mod world; // Simulation orchestrator, command dispatch, spawn, hash

// --- Mission scheduler substrate + radio contact RPC vocabulary ---
pub mod mission; // MissionType, MissionTimer, MissionControl (INI table)
pub mod radio; // RadioMessage / RadioResponse / RadioPayload

// --- Movement: ground pathing, speed ramping, cell transitions,
//     special locomotors, drive tracks, turret rotation ---
pub mod movement;
pub mod pathfinding; // A* search, zone connectivity, terrain speed, path smoothing
pub mod psychic_detection;
pub mod projectile; // serialized BulletClass-style flight state and detonation handoff
pub mod wave; // serialized WaveClass-style display registrations and lifetime

// --- Docking: repair depots and airfield landing pads ---
pub mod docking;

// --- Aircraft mission state machines (attack runs, guard, RTB, idle) ---
pub mod aircraft;

// --- Vision, fog of war, power ---
pub mod house_eva; // HouseClass::Update EVA advice timers (funds nag, low power)
pub mod power_system;
pub mod superweapon;
pub mod vision;

// --- Animation, building overlays, bridge state ---
pub mod animation;
pub mod bridge_specs;
pub mod bridge_state;

// --- Infantry deploy-fire state machine ---
pub mod deploy;
pub(crate) mod door;
pub mod gate_runtime;
pub mod infantry;
pub(crate) mod mcv_deploy;
pub(crate) mod unit_simple_deploy;

// --- Persistent cell occupancy ---
pub mod cell_kernel;
pub(crate) mod cell_map;
pub mod cell_rect;
pub mod find_nearby_cell;
pub mod occupancy;

// --- Map/cell substrate (read-only services over the canonical cell store) ---
pub mod map; // bridge topology service (first member of the map/cell-substrate workstream)

// --- Mutable per-cell overlay state (ore density, wall damage, bridge frames) ---
pub mod cell_neighbors;
pub mod overlay_grid;
pub(crate) mod overlay_mark_host; // the live Simulation's host of OverlayClass::Mark

// --- Mutable per-cell smudge state (craters, scorches, pre-placed map decals) ---
pub mod smudge_grid;

// --- Per-cell radiation field (sites, spread/decay; damage applied by combat) ---
pub mod radiation;

// --- Phobos Shields: per-object shield points, absorption and recovery ---
#[cfg(feature = "phobos")]
pub mod shield;
#[cfg(feature = "phobos")]
pub mod digital_display;

// --- Passengers, transport, slaves ---
pub mod parity_digest;
pub mod passenger;
pub(crate) mod slave_deposit;
pub mod slave_manager;
pub mod spawn_manager;
pub mod transport_unload;

// --- Economy, map resources ---
pub mod ore_growth;
pub(crate) mod ore_twinkle;
pub mod radar;
pub mod rocking;
/// Native ripple table shared by IonBlast forces and presentation.
pub mod ion_blast_ripple;
pub mod terrain_object;
pub mod terrain_spawn;
pub mod tiberium;
pub mod tiberium_germinate;

// --- Per-match settings, per-player state ---
pub mod game_options;
pub(crate) mod house_diplomacy; // ally graph: install, MakeAlly/MakeEnemy, Computer_Paranoid
pub mod house_state;
pub mod houses;
pub(crate) mod house_strategy;
pub mod house_tracking;

// --- Trigger runtime (map trigger evaluation during gameplay) ---
pub mod team_script_vm;
pub mod trigger_runtime;

// --- AI, replay, selection, debug ---
pub(crate) mod ai_build_need;
pub mod ai_buildable;
pub(crate) mod ai_team_creation;
pub(crate) mod ai_unit_choice;
pub(crate) mod naval_base_placement;
pub mod replay;
#[cfg(test)]
mod replay_determinism_tests;
pub mod runtime;
pub mod selection;

// --- Snapshot serialization (mid-match save/load) ---
pub mod snapshot;
#[cfg(test)]
mod stable_identity_tests;

// --- Particle systems (visual + damage particle effects: smoke, gas, fire) ---
pub mod particles;

#[cfg(test)]
#[path = "deploy_tests.rs"]
mod deploy_tests;

pub(crate) mod airstrike;
