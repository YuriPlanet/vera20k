//! Ares 3.0 `Type=Firestorm` (`src/Misc/SWTypes/Firewall.cpp`, SW_Firewall,
//! handler index 3): a charge-drain power that raises its owner's firestorm
//! walls (`sim::ares_firestorm`) while it drains.
//!
//! Installed Ares: Activate `0x100715C0`, Deactivate `0x100715F0`,
//! LoadFromINI `0x10071620` (Action=None, UseChargeDrain and no radar event,
//! every pass); it has no Initialize, so no AI targeting mode. The
//! charge-drain ClickFire (`0x10035130`), drain expiry (`0x10035730`), loss
//! (`0x10035610`) and hold (`0x10035750`) owners in `fire`/`mod` call these.
//!
//! Both edges set the House's tech-tree recheck (House+0x1FC) for the
//! player's own click: natively the local player's House only, which VERA
//! projects as a human-controlled one (the drain expiry passes true for
//! every House, the loss and hold false).
//!
//! Ledger: no RNG draws, timer writes or detach calls of its own.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

/// SW_Firewall::Activate `0x100715C0`: SetFirestormState(true) of the
/// Super's owner. Always activates.
pub(super) fn activate(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    is_player: bool,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    sim.ares_set_firestorm_state(owner, true, rules, registry, effects);
    if is_player {
        sim.houses.request_house_tech_tree_recheck(owner);
    }
}

/// SW_Firewall::Deactivate `0x100715F0`: SetFirestormState(false).
pub(super) fn deactivate(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    is_player: bool,
    effects: FrameEffects<'_>,
) {
    sim.ares_set_firestorm_state(owner, false, rules, effects.overlay_registry(), effects);
    if is_player {
        sim.houses.request_house_tech_tree_recheck(owner);
    }
}
