//! Docking systems — repair depot docks and airfield landing pads.

pub(crate) mod airfield_reload;
pub(crate) mod armory;
pub mod building_dock;
pub mod bunker_install;
pub mod bunker_link;
mod hospital;
#[cfg(all(test, tests_sim_c_g))]
mod hospital_tests;
#[cfg(all(test, tests_sim_c_g))]
mod armory_tests;
