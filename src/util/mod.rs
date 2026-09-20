//! Shared utilities used across the engine.
//!
//! Contains helpers that don't belong to any specific game module:
//! config loading, fixed-point math wrappers, color conversion, rectangles.
//!
//! ## Dependency rules
//! - util/ has NO dependencies on other game modules.
//! - Any module may depend on util/.

pub mod base64;
pub mod config;
pub mod direction;
pub mod direction_tables;
pub mod facing_table;
pub mod fixed_math;
pub mod flh_transform;
pub mod fnv;
pub mod ini_writer;
pub mod lcw;
pub mod legacy_crt_rng;
pub mod lepton;
pub mod logging;
pub mod lzo;
pub mod native_string;
pub mod native_trig;
pub mod native_x87;
pub mod pixel_conversion;
pub mod read_helpers;
pub(crate) mod retail_pointer_sort;
pub(crate) mod sha256;
pub mod single_instance;
pub mod version;
// pub mod rect;
// pub mod color;

pub(crate) mod native_file_time;

pub mod native_file_name;
