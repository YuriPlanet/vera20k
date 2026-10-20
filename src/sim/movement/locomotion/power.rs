//! Locomotor power: the flag, the edges that drive it, and its one effect.
//!
//! ## The native shape
//!
//! Power is a plain byte on the locomotor instance (`+0xC` of the
//! interface). The base `Power_On` (`0x0055A8F0`) writes 1, the base
//! `Power_Off` (`0x0055A910`) writes 0 and `Is_Powered` (`0x0055A930`) reads
//! it back; both setters then re-dispatch ILoco+60, returning the new flag.
//! The base setters stop nothing and keep the destination. Every family
//! keeps the base Power_On; Hover and Fly override Power_Off
//! ([`Simulation::locomotor_power_off`](crate::sim::world::Simulation::locomotor_power_off)):
//! a powered Hover (its byte, or any height) spins down and a moving one
//! stops.
//!
//! The flag itself lives on [`LocomotorState`]; this module owns the
//! documentation of *when* it moves and what reads it, because that is the part
//! worth keeping in one place.
//!
//! ## The edges, and where they are wired
//!
//! Every one of these was read off a callsite whose receiver is the locomotor
//! interface on the host, not some other object's vtable:
//!
//! | edge | direction | wired in |
//! |---|---|---|
//! //! | bunker sell/death release4593A0 | on | `docking::bunker_link::release_sell_destroy` |
//! | bunker normal release4595C0 | on | `docking::bunker_link::release_normal` |
//! | Unit destination tail742F48, on a repair/bunker building's ground cell | on | `track_path::unit_destination_power_on` |
//! | depot MissionRepair44B780 pending admission / servicing / state1 release | on / off / on | `docking::building_dock::mission_repair` |
//! | Unit PerCell dock arrival73A52B | off | `movement::per_cell::unit_dock_now` |
//! | Deactivate70FD0B / Reactivate70FC15 (Robot Tanks) | off / on | `sim::powered_unit` |
//!
//! Both bunker calls dispatch ILoco+58 (Power_On55A8F0), before Force_Track
//! and the caller's separate speed write. The building+2E4 link gates them;
//! its only non-null producer is bunker installation. The old harvester label
//! on4595C0 does not make it a refinery release path.
//!
//! Ordinary Unit741970 orders on cells without a repair/bunker building keep
//! the byte (tools/spatial_oracle/track_order_path power rows). Its shared
//! tail powers on before Foot4D94B0 when Object5F6960's raw current cell has
//! a UnitRepair/Bunker building and lacks high-bridge flag0x100. Same-Nav,
//! deployment refusal and the accepted depot handshake return before that
//! tail. Executable controls: building_repair.depot_service.{json,md}.
//! Repair state0 admission failure restores power at44C808; state1's
//! NEED_MOVE failure restores an unpowered contact at44C61B. The same corpus
//! compares both fallbacks and their distance/moving/powered controls.
//! Infantry51AA40 has no PowerOn. A deploy-powered-off unit is powered on by
//! its undeploy, and a bunkered one by its release.
//!
//! ## The one observable effect
//!
//! **Hover.** An unpowered hover locomotor stops producing lift and sinks to the
//! ground; every other family ignores the flag entirely today. That asymmetry is
//! native, not a shortcut.
//!
//! ## Frequency
//!
//! A Robot Tank (stock ROBO, a Hover) is unpowered for as long as it is
//! offline (`sim::powered_unit`): every Robot Tank of a house whose Robot
//! Control Center loses power or is lost settles and spins down. EMP-driven
//! power is not modelled here. Deployment and ordinary depot servicing
//! produce the other flag changes; depot departure reaches the shared Unit
//! setter's current-cell power-on gate.
//!
//! ## Explicitly out of scope
//!
//! EMP-drives-power; the Fly family's power-off RNG draws; anything reading ion
//! sensitivity, ion storms or the special-flags ion path; any lightning-storm to
//! locomotor-power coupling.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sibling movement state only.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

impl crate::sim::world::Simulation {
    /// ILocomotion+0x5C, `Power_Off`, on `id`'s active locomotor: Hover's
    /// override (`0x00516BF0`, [`Self::hover_power_off`]) or the base
    /// (`0x0055A910`).
    ///
    /// RESIDUAL: Fly's override (`0x004CFD20`: a spin of `Random(20, 30)`
    /// steps and a stop while moving) is not ported. None of the callers
    /// reaches it: they are a Unit's dock and depot service and Deactivate,
    /// which the stock `PoweredUnit=` type (a Hover) alone reaches.
    pub(crate) fn locomotor_power_off(&mut self, id: u64) {
        if self.hover_power_off(id) {
            return;
        }
        if let Some(locomotor) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|entity| entity.locomotor.as_mut())
        {
            locomotor.power_off();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::LocomotorState;

    /// The base Power_Off is not a stop: it touches the flag and nothing
    /// else, so a destination already installed survives.
    #[test]
    fn powering_off_does_not_disturb_the_rest_of_the_locomotor() {
        let mut state = LocomotorState::for_test_kind(LocomotorKind::Drive);
        let before = state.clone();
        state.power_off();

        assert_eq!(state.active_kind(), before.active_kind());
        assert_eq!(state.layer(), before.layer());
        assert_eq!(state.piggyback(), before.piggyback());
    }
}
