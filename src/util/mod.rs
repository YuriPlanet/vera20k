//! Shared utilities used across the engine.
//!
//! Contains helpers that don't belong to any specific game module:
//! config loading, fixed-point math wrappers, color conversion, rectangles.
//!
//! ## Dependency rules
//! - util/ has NO dependencies on other game modules.
//! - Any module may depend on util/.

pub mod base64;
pub mod case_insensitive_path;
pub mod config;
pub mod direction;
pub mod direction_tables;
pub mod fixed_math;
pub mod flh_transform;
pub mod fnv;
pub mod ini_writer;
pub mod ini_roots;
pub mod lcw;
pub mod legacy_crt_rng;
pub mod lepton;
pub mod logging;
pub(crate) mod lookup_hash;
pub mod lzo;
pub(crate) mod native_ballistics;
pub mod native_color;
pub(crate) mod native_coord;
pub(crate) mod native_heap;
pub(crate) mod native_random;
pub mod native_string;
pub mod native_trig;
pub mod native_x87;
pub(crate) mod native_quaternion;
pub mod pixel_conversion;
pub mod read_helpers;
pub mod retail_pointer_sort;
pub mod retail_trig;
pub(crate) mod sha1;
pub mod sha256;
pub mod single_instance;
pub mod version;
pub mod rect;
pub mod save_codec;
// pub mod color;

pub mod native_file_time;

pub mod native_file_name;

pub mod native_affine;
