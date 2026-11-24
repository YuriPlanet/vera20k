//! Phobos `DropCrate=`: a techno that leaves a fixed crate on the cell it is
//! destroyed on.
//!
//! ```ini
//! DropCrate=          ; Nullable<Powerup>, unset   Ext/TechnoType/Body.cpp:1446
//! ```
//!
//! `TechnoExt::GetDropCrateIndex` (`Ext/Techno/Body.Internal.cpp:275-299`)
//! answers the type's `[Powerups]` slot. The two destruction hooks,
//! `TechnoClass::RegisterDestruction` (`0x702E4E`) and the Chronoshift path
//! (`0x70337D`), then call `TechnoExt::TryToCreateCrate` (`Ext/Techno/Hooks.cpp:667-703`):
//!
//! ```cpp
//! const int nSelectedPowerup = TechnoExt::GetDropCrateIndex(pVictim);
//! if (nSelectedPowerup >= 0)
//!     TechnoExt::TryToCreateCrate(pVictim->Location, static_cast<Powerup>(...));
//! ```
//!
//! `TryToCreateCrate` (`Ext/Techno/Body.cpp:579-656`) converts the location to
//! its cell and scans outward in rings of `currentRange = 0..10` — the upper
//! line `y - r` left to right, the lower line `y + r`, then the left and right
//! columns between them — calling `MapClass::PlacePowerupCrate` at each cell
//! until one accepts. VERA's [`crate::sim::crates::place_crate`] is that
//! 0x0056BEC0 callee, so the scan is the only new body here.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;

use super::world::{FrameEffects, Simulation};

/// `TechnoExt::TryToCreateCrate`'s ring limit (`Ext/Techno/Body.cpp:579`).
const DROP_CRATE_CELL_RANGE: i16 = 10;

impl Simulation {
    /// `TechnoExt::GetDropCrateIndex` plus `TryToCreateCrate`, run from the
    /// destruction transaction (`TechnoClass::RegisterDestruction`, hook
    /// `0x702E4E`, `Ext/Techno/Hooks.cpp:667-686`).
    pub(crate) fn drop_crate_on_destruction(
        &mut self,
        victim_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some(registry) = registry else {
            return;
        };
        let Some((type_ref, cell, instance)) =
            self.substrate.entities.get(victim_id).map(|victim| {
                let location =
                    crate::sim::movement::ground_pose::position_world_coord(&victim.position);
                (
                    victim.type_ref(),
                    (
                        crate::util::lepton::lepton_to_cell_packed(location.x),
                        crate::util::lepton::lepton_to_cell_packed(location.y),
                    ),
                    (victim.phobos_drop_crate(), victim.phobos_drop_crate_type()),
                )
            })
        else {
            return;
        };
        let type_slot = self
            .interner
            .object_type(type_ref, rules)
            .and_then(|object| object.phobos_techno_extras.drop_crate)
            .map(|slot| slot as i32);
        let Some(powerup) = drop_crate_index(instance.0, instance.1, type_slot) else {
            return;
        };
        try_to_create_crate(self, rules, registry, cell, powerup as u8, effects);
    }

    /// `TActionExt::SetDropCrate` (`Ext/TAction/Body.cpp:690-720`), action
    /// 600: every techno carrying a tag whose trigger chain lists a trigger
    /// typed `trigger_type_id` takes `DropCrate = value`; a value of `1` also
    /// stores the action's `Param3` as `DropCrateType`.
    pub(crate) fn phobos_set_drop_crate(
        &mut self,
        trigger_type_id: &str,
        value: i32,
        crate_type: i32,
    ) {
        let mut ordinal = 0;
        while ordinal < self.substrate.entities.techno_registry_len() {
            let Some(id) = self.substrate.entities.techno_registry_at(ordinal) else {
                ordinal += 1;
                continue;
            };
            ordinal += 1;
            // `pTechno->AttachedTag` then the tag's `FirstTrigger` chain.
            let Some(tag) = self.trigger_runtime.object_tag(id) else {
                continue;
            };
            if !self
                .trigger_runtime
                .tag_carries_trigger_type(tag, trigger_type_id)
            {
                continue;
            }
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.set_phobos_drop_crate(value);
                if value == 1 {
                    entity.set_phobos_drop_crate_type(crate_type);
                }
            }
        }
    }
}

/// `TechnoExt::GetDropCrateIndex` (`Ext/Techno/Body.Internal.cpp:275-299`):
/// an instance override wins whenever it is set (`>= 0`); only `1` answers the
/// stored slot, anything else answers "none". Without an override the type
/// key's slot is the answer.
fn drop_crate_index(instance: i32, instance_type: i32, type_slot: Option<i32>) -> Option<i32> {
    if instance >= 0 {
        if instance == 1 && instance_type >= 0 {
            return Some(instance_type);
        }
        return None;
    }
    type_slot
}

/// `TechnoExt::TryToCreateCrate`'s outward scan
/// (`Ext/Techno/Body.cpp:585-650`): range `0..10`, each ring's upper line,
/// lower line, left column then right column, first accepted cell wins.
fn try_to_create_crate(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    center: (i16, i16),
    powerup: u8,
    effects: &mut FrameEffects<'_>,
) {
    for range in 0..DROP_CRATE_CELL_RANGE {
        let mut place = |x: i16, y: i16| -> bool {
            match crate::sim::crates::place_crate(
                sim,
                rules,
                registry,
                (x, y),
                Some(powerup),
                effects,
            ) {
                Ok(placed) => placed,
                Err(error) => {
                    log::error!("DropCrate placement failed: {error}");
                    false
                }
            }
        };
        for i in -range..=range {
            if place(center.0.wrapping_add(i), center.1.wrapping_sub(range)) {
                return;
            }
        }
        for i in -range..=range {
            if place(center.0.wrapping_add(i), center.1.wrapping_add(range)) {
                return;
            }
        }
        for j in (-range + 1)..range {
            if place(center.0.wrapping_sub(range), center.1.wrapping_add(j)) {
                return;
            }
        }
        for j in (-range + 1)..range {
            if place(center.0.wrapping_add(range), center.1.wrapping_add(j)) {
                return;
            }
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
#[path = "phobos_drop_crate_tests.rs"]
mod tests;
