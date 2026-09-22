//! Body-rocking simulation.
//!
//! Implements the spring-damper that drives `RockingState::angle_*` toward zero
//! each tick. Drive/Ship slope interpolation is owned separately by locomotor
//! runtime payload state.
//!
//! # DEAD SUBSYSTEM (GSI-08.14) — no producer and no consumer
//!
//! [`tick`] runs every frame from `World::advance_tick` Phase 2.5, but nothing
//! in the build writes an impulse into it and nothing reads the angles back out:
//!
//! - **No producer.** [`apply_rocker_impulse`] has no caller outside this
//!   module's tests. gamemd reaches `TechnoClass::ApplyRocker` (vtable `+0x3D8`
//!   → `0x0070B280`) from exactly four places, established this session by a
//!   `CALL [reg+0x3D8]` census over all 1.16M instructions in the image:
//!   `BulletClass::DetonateAtCoord @ 0x004699A1` (`DirectRocker=`, zero stock
//!   authors), `Apply_area_damage @ 0x00489DFF`/`0x00489E3E` (`Rocker=`, 18
//!   stock warheads), and `ParasiteClass::AI @ 0x0062A21C` (a parasite bite
//!   on a non-Infantry victim, force `1.5`; its lateral-sign RNG draw at
//!   `0x0062A17F` is taken in `combat/parasite.rs`).
//!   `FootClass::ReceiveEMP @ 0x004DECF0` writes the velocities directly
//!   instead. **Firing never rocks the body** — there is no
//!   `ApplyRocker` site in `Fire_At`, and none in any crush or deploy path.
//! - **No consumer.** `grep` over `render/` and `app/` finds no read of
//!   `GameEntity::rocking`, so even a correct impulse would tilt nothing, and
//!   the self-destruct hook the production caller passes is
//!   [`NoopSelfDestruct`].
//!
//! - Trigger: any `Rocker=yes` warhead detonating near a vehicle — that is most
//!   tank and artillery shells in the game.
//! - Player effect: vehicles do not lurch when shells land beside them. The
//!   whole "shot lands next to the Rhino and the Rhino rocks" reading is
//!   missing.
//! - Frequency: continuous in any engagement.
//! - Downstream risk: wiring the producer alone would add per-entity
//!   deterministic state and move the pinned replay hash while changing nothing
//!   a player can see, so the producer and the renderer belong in one slice. See
//!   the verified DRIFT list on [`apply_rocker_impulse`] for what the impulse
//!   maths must also change when that slice lands.
//!
//! ## Dependency rules
//! - Part of sim/ — depends only on sim/components, sim/entity_store, map,
//!   rules, util/fixed_math. Never on render/, ui/, audio/, net/.

pub mod impulse;
pub mod rocking_system;
pub mod self_destruct;

#[cfg(test)]
#[path = "rocking_tests.rs"]
mod rocking_tests;

pub use impulse::apply_rocker_impulse;
pub use rocking_system::tick;
pub use self_destruct::{NoopSelfDestruct, SelfDestructHook, check_and_fire};
