//! Game data definitions parsed from rules.ini and art.ini.
//!
//! rules.ini is the heart of Red Alert 2 — it defines EVERY unit type, building type,
//! weapon, warhead, projectile, speed value, cost, build time, and prerequisite chain.
//! The sim/ module is essentially a rules.ini interpreter.
//!
//! ## Key types
//! - `RuleSet` — master lookup table for all game data, loaded once at startup
//! - `ObjectType` — defines a game object (unit/building/aircraft) with shared properties
//! - `WeaponType` — defines a weapon (damage, range, rate of fire, projectile, warhead)
//! - `WarheadType` — defines damage spread and armor effectiveness (Verses)
//!
//! ## Dependency rules
//! - rules/ depends on: assets/ (reads INI files extracted from .mix archives)
//! - rules/ is depended on by: sim/, map/, render/, sidebar/
//! - rules/ does NOT depend on: sim/, render/, ui/, sidebar/, audio/, net/

pub mod animation_sequence;
pub mod art_data;
pub mod bridge_warheads;
pub mod color_add;
pub mod color_scheme;
pub mod combat_damage;
pub mod crate_rules;
pub mod effect_asset_catalog;
pub mod error;
pub mod flh;
pub mod foundation;
pub mod house_colors;
pub mod infantry_sequence;
pub mod ini_enum;
pub mod ini_parser;
pub mod ini_value;
pub mod native_processing;
pub mod jumpjet_params;
pub mod locomotor_type;
pub mod missile_spawn;
pub mod mission_data;
pub mod object_type;
pub mod overlay_types;
pub mod particle_system_type;
pub mod particle_type;
pub mod powerups;
pub(crate) mod process_owner;
pub mod projectile_type;
pub mod radar_event_config;
pub mod ruleset;
pub mod shp_vehicle_sequence;
pub mod smudge_type;
pub mod sound_ini;
pub mod superweapon_type;
pub mod team_ai_ini;
pub mod terrain_asset_catalog;
pub mod terrain_object_type;
pub mod terrain_rules;
pub mod tiberium_type;
pub mod voxel_anim_type;
pub mod warhead_type;
pub mod weapon_type;

#[cfg(test)]
mod path_delay_rules_tests;
