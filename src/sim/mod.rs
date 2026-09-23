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
//! - sim/ never depends on render/, ui/, sidebar/, audio/ or net/.
//! - Commands enter and outputs leave through simulation APIs; presentation
//!   consumes state without becoming a gameplay authority.

// --- Core types: entity storage, components, commands, RNG, interning ---
pub mod anim_class;
#[cfg(test)]
pub(crate) mod arena_fixture;
pub(crate) mod building_art;
pub(crate) mod base_plan;
pub(crate) mod base_plan_generation;
pub mod capture_manager;
pub mod cloak_disguise;
pub mod command;
pub mod components;
#[cfg(test)]
pub(crate) mod health_ratio_fixture;
pub mod crates; // scenario-start crate placement (Post_Map_Init step 3)
pub mod credit_income; // oil-derrick ProduceCash + Floating Disc money drain (object-loop money)
pub(crate) mod conversion_health;
pub(crate) mod crew_survival; // building SpawnSurvivors and vehicle crew escape
pub mod economy; // per-house wallet/storage/statistics value-type (production+economy substrate)
pub mod entity_store;
pub(crate) mod estimated_health;
pub mod game_entity;
pub mod intern;
pub(crate) mod lifecycle_request;
pub(crate) mod light_sources;
pub mod multiplayer_checksum;
pub(crate) mod native_identity;
pub(crate) mod radiation_light;
pub mod rng;
pub(crate) mod scenario_bootstrap;
pub(crate) mod scenario_post_map;
pub mod scenario_session; // app->sim launch descriptor (per-match seed pipeline)
pub(crate) mod score;
pub mod sensor_lifecycle;
pub mod temporal;
pub mod timer; // signed frame-anchored countdown primitive
pub mod type_handle_table; // InternedId -> TypeHandle, one-hop entity->type resolution

// --- Pure read-only deterministic engine-data services (gamemd-exact lookup tables) ---
pub mod substrate; // direction/facing tables; no render/ui/audio/net dep

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
pub mod pathfinding; // A* search, zone connectivity, terrain costs, path smoothing
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
pub mod gate_runtime;
pub mod infantry;
pub(crate) mod mcv_deploy;

// --- Persistent cell occupancy ---
pub mod cell_kernel;
pub mod cell_rect;
pub mod find_nearby_cell;
pub mod occupancy;

// --- Map/cell substrate (read-only services over the canonical cell store) ---
pub mod map; // bridge topology service (first member of the map/cell-substrate workstream)

// --- Mutable per-cell overlay state (ore density, wall damage, bridge frames) ---
pub mod cell_neighbors;
pub mod overlay_grid;

// --- Mutable per-cell smudge state (craters, scorches, pre-placed map decals) ---
pub mod smudge_grid;

// --- Per-cell radiation field (sites, spread/decay; damage applied by combat) ---
pub mod radiation;

// --- Passengers, transport, slaves ---
pub mod parity_digest;
pub mod passenger;
pub(crate) mod slave_deposit;
pub mod slave_miner;
pub mod spawn_manager;
mod spawn_manager_tests;
pub mod transport_unload;

// --- Economy, map resources ---
pub mod ore_growth;
pub(crate) mod ore_twinkle;
pub mod radar;
pub mod rocking;
pub mod terrain_object;
pub mod terrain_spawn;
pub mod tiberium;
pub(crate) mod tiberium_germinate;

// --- Per-match settings, per-player state ---
pub mod game_options;
pub mod house_state;
pub(crate) mod house_strategy;
pub mod house_tracking;

// --- Trigger runtime (map trigger evaluation during gameplay) ---
pub mod team_script_vm;
pub mod trigger_runtime;

// --- AI, replay, selection, debug ---
pub mod ai;
pub(crate) mod ai_buildable;
pub mod debug_event_log;
pub(crate) mod naval_base_placement;
pub mod replay;
#[cfg(test)]
mod replay_determinism_tests;
pub mod runtime;
pub mod selection;
// GPU-independent HVA frame-count catalog shared with the renderer's atlas seeding (F09).
pub mod voxel_frame_catalog;

// --- Snapshot serialization (mid-match save/load) ---
pub mod snapshot;
#[cfg(test)]
mod stable_identity_tests;

// --- Particle systems (visual + damage particle effects: smoke, gas, fire) ---
pub mod particles;

#[cfg(test)]
#[path = "deploy_tests.rs"]
mod deploy_tests;
