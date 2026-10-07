//! Match runtime owner (F12): the per-frame simulation advance transaction,
//! the local frame pacer, and the scenario exit cascade.

pub(crate) mod eva_producers;
pub(crate) mod frame_pacer;
pub(crate) mod scenario_exit;
pub(crate) mod sim_tick;
pub(crate) mod sound_dispatch;
pub(crate) mod state;
mod super_selection;

pub(crate) mod restore;
pub(crate) mod startup;
