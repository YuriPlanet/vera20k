//! Locomotor power: the flag, the edges that drive it, and its one effect.
//!
//! ## The native shape
//!
//! Power is a plain byte on the locomotor instance. `Power_On` writes 1,
//! `Power_Off` writes 0, `Is_Powered` reads it back. Both setters then
//! re-dispatch to one more vtable slot; that re-dispatch has **no verified
//! effect** and is deliberately not modelled. Neither setter stops movement or
//! clears a destination — powering off is not a stop.
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
//! | deploy begins | off | the deploy command |
//! | undeploy completes | on | the deploy state machine |
//! | a destination is accepted (command-time adapter locomotors) | on | the move-command entry |
//! | bunker sell/death release4593A0 | on | `docking::bunker_link::release_sell_destroy` |
//! | bunker normal release4595C0 | on | `docking::bunker_link::release_normal` |
//!
//! Both bunker calls dispatch ILoco+58 (Power_On55A8F0), before Force_Track
//! and the caller's separate speed write. The building+2E4 link gates them;
//! its only non-null producer is bunker installation. The old harvester label
//! on4595C0 does not make it a refinery release path.
//!
//! Walk, Drive and Ship orders keep the byte: the ordinary Unit741970 ->
//! Drive4AFD40/Ship69F450 setter leaves a powered-off locomotor powered off
//! (tools/spatial_oracle/track_order_path power rows), and Infantry51AA40 has
//! no PowerOn. The remaining command-time adapter locomotors still power on
//! when they accept a destination. A deploy-powered-off unit is powered on by
//! its undeploy, and a bunkered one by its release.
//!
//! ## The one observable effect
//!
//! **Hover.** An unpowered hover locomotor stops producing lift and sinks to the
//! ground; every other family ignores the flag entirely today. That asymmetry is
//! native, not a shortcut.
//!
//! ## Frequency: UNCHECKED
//!
//! How often a stock skirmish actually reaches the powered-off state is **not
//! established**. EMP-driven power is not modelled here at all, and with it out
//! of scope the only producer wired is deploy-begin — which pairs with an
//! undeploy that powers straight back on. No pass has traced a stock sequence
//! that leaves a *hover* unit unpowered long enough to be seen sinking, so the
//! player-visible reach of this slice is unquantified. It is modelled because
//! the flag is real deterministic state that deploy flips, not because a
//! symptom demanded it.
//!
//! ## Explicitly out of scope
//!
//! EMP-drives-power; the Fly family's power-off RNG draws; anything reading ion
//! sensitivity, ion storms or the special-flags ion path; any lightning-storm to
//! locomotor-power coupling; and the setters' re-dispatch artefact.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sibling movement state only.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

#[cfg(test)]
mod tests {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::LocomotorState;

    #[test]
    fn a_fresh_locomotor_is_powered() {
        for kind in [
            LocomotorKind::Drive,
            LocomotorKind::Hover,
            LocomotorKind::Walk,
            LocomotorKind::Teleport,
        ] {
            assert!(
                LocomotorState::for_test_kind(kind).is_powered(),
                "{kind:?} must start powered — unpowered is a state something \
                 has to actively put a unit into"
            );
        }
    }

    #[test]
    fn power_off_then_on_round_trips() {
        let mut state = LocomotorState::for_test_kind(LocomotorKind::Hover);
        state.power_off();
        assert!(!state.is_powered());
        state.power_on();
        assert!(state.is_powered());
    }

    /// The deploy pair, end to end through the production state machine:
    /// beginning a deploy powers the locomotor off, and the undeploy completing
    /// powers it back on. These are the only two edges wired that can *reach*
    /// the powered-off state, so if this test stops holding the flag is inert.
    #[test]
    fn undeploy_completing_powers_the_locomotor_back_on() {
        use crate::sim::deploy::{DeployPhase, tick_deploy_state};
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        let mut entities = EntityStore::default();
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Hover);
        loco.power_off();
        entity.locomotor = Some(loco);
        entity.deploy_state = Some(DeployPhase::Undeploying { ticks_remaining: 1 });
        entities.insert(entity);

        tick_deploy_state(&mut entities);

        let entity = entities.get(1).expect("entity");
        assert_eq!(entity.deploy_state, None, "undeploy completed");
        assert!(
            entity.locomotor.as_ref().expect("locomotor").is_powered(),
            "undeploy completing must power the locomotor back on"
        );
    }

    /// Powering off is not a stop: the native setters touch the flag and nothing
    /// else, so a destination already installed survives.
    #[test]
    fn powering_off_does_not_disturb_the_rest_of_the_locomotor() {
        let mut state = LocomotorState::for_test_kind(LocomotorKind::Drive);
        let before = state.clone();
        state.power_off();

        assert_eq!(state.kind, before.kind);
        assert_eq!(state.slot, before.slot);
        assert_eq!(state.layer, before.layer);
        assert_eq!(state.phase, before.phase);
        assert_eq!(state.altitude, before.altitude);
        assert_eq!(state.piggyback, before.piggyback);
    }
}
