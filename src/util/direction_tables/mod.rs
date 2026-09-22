//! Facing / direction lookup-table substrate — pure, read-only, deterministic
//! services for the gamemd "which-way / where-next" table family (cell-delta,
//! lepton-delta, facing↔direction quantization, DRAGON 32-way frame). Tables are
//! gamemd-exact, proven by exact-equality tests; no shadow→invert.
//!
//! Ownership (F14): the 8-way vocabulary and 8-bit quantization are OWNED by
//! `util::direction` and re-exported/delegated here (`CELL_DELTAS`,
//! `cell_delta`, `dir_from_facing8`); this family ADDS the 16-bit facing
//! forms, lepton deltas, DRAGON frames, and muzzle rotation. Facing→movement
//! vectors (sin/cos) live in `util::facing_table`. One authority per
//! conversion — new helpers must delegate, not re-derive.
//!
//! Foundation slice (S1–S4): canonical sim-facing tables. Drive-track tables (S5)
//! and consumer cutovers (S6+) are later slices.
//!
//! ## Dependency rules
//! - Part of util/ (map-, rules-, and sim-independent). No render/ui/audio/net.

pub mod cell;
pub mod dragon;
pub mod lepton;
mod native_angle;
mod native_angle_table;
pub mod quantize;

pub use cell::{CELL_DELTAS, cell_delta, cell_delta_unchecked};
pub use dragon::{DRAGON_FRAME_TABLE, dragon_frame_index};
pub use lepton::{LEPTON_DELTAS, lepton_delta, lepton_to_cell};
pub use native_angle::{facing8_from_delta, facing16_from_delta};
pub(crate) use native_angle::native_atan2_f32;
pub(crate) use native_angle::facing16_between;
pub use quantize::{
    dir_from_facing8, dir_from_facing16, facing8_to_16, muzzle_anim_index_8way, opposite_dir,
    step32_from_facing16,
};
