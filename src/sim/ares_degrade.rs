//! Ares low-power degrading: `HouseClass_Update_DamageDelay @ 100247A0`
//! (hook `0x004F8B08`, inside `HouseClass::Update @ 0x004F8440` after team
//! creation and before the player's EVA block).
//!
//! Each time the House's `DamageDelayTimer` expires, a degrading House
//! (`Degrades=` of its country, else not `MultiplayPassive=`) with
//! `[General] Degrade.Enabled=yes` and low power damages every building in
//! its Buildings vector (House `+0x68`) whose health ratio is above its
//! `Degrade.Percentage` (else the RulesExt one, else `ConditionYellow=`) by
//! its `Degrade.Amount` (else `Degrade.AmountConsumer` for a `Power=` drain,
//! `Degrade.AmountNormal` otherwise), through `ReceiveDamage` with
//! `C4Warhead=`, no attacker, defenses applied.
//!
//! The timer: the House constructor starts it at its construction frame
//! (`0x004F5A6E..0x004F5A82`, `ftol(DamageDelay * 900)`) and the hook
//! restarts it at each expiry with `_ftol2(DamageDelay * 900)`. Every VERA
//! House is constructed at frame 0 and `HouseClass::Update` reaches the hook
//! for every House on every frame, so the timer expires exactly at each
//! positive multiple of that duration (every frame for a duration below 1).
//! VERA derives the expiry from the frame instead of storing the timer.
//!
//! RESIDUALS:
//! - HouseExt `Degrades=` (the map's house section, read by the House
//!   extension's `LoadFromINIFile` in `10021A70`) is not read; a House takes
//!   its country's answer. Trigger: a map sets it. Retail sets neither key.
//! - Native walks the Buildings vector with an end pointer taken once; a
//!   building the damage destroys shifts that vector under the walk. VERA
//!   walks a copy and skips buildings that are gone.

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::native_x87::{NativeF64Bits, X87Chop53, X87Ordering};

/// `DamageDelay * 900` truncated: the timer's duration in frames.
fn damage_delay_frames(rules: &RuleSet) -> i32 {
    X87Chop53::load_f64(NativeF64Bits::from_bits(
        f64::from(rules.general.damage_delay_minutes).to_bits(),
    ))
    .map(|minutes| {
        X87Chop53::mul(
            minutes,
            X87Chop53::load_f64(NativeF64Bits::from_bits(900.0f64.to_bits()))
                .expect("900.0 is finite"),
        )
    })
    .map_or(0, X87Chop53::ftol_i32_low_masked)
}

/// Whether the House's `DamageDelayTimer` expires on `frame` (module doc).
fn damage_delay_expires(frame: i32, duration: i32) -> bool {
    duration < 1 || (frame > 0 && frame % duration == 0)
}

impl Simulation {
    /// `100247A0` for one House; see the module doc.
    pub(crate) fn ares_degrade_buildings(
        &mut self,
        owner: InternedId,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let frame = self.session.binary_frame() as i32;
        if !damage_delay_expires(frame, damage_delay_frames(rules)) {
            return;
        }
        let Some(house) = self.houses.get(&owner) else {
            return;
        };
        let degrades = rules
            .country_degrades(self.interner.resolve(house.house_type_id()))
            .unwrap_or(!house.multiplay_passive());
        let globals = rules.ares_building_globals();
        // HasLowPower, inlined: a drain above the output.
        let low_power = self
            .power_states
            .get(&owner)
            .map(|power| power.effective_totals())
            .is_some_and(|(output, drain)| drain != 0 && output < drain);
        if !degrades || !globals.degrade_enabled || !low_power {
            return;
        }
        let default_percentage = globals
            .degrade_percentage
            .unwrap_or(NativeF64Bits::from_bits(
                rules.general.condition_yellow.to_bits(),
            ));
        let c4 = self.interner.intern(&rules.bridge_warheads.c4_name);
        let buildings = house.base_projection().buildings().to_vec();
        for id in buildings {
            let Some(building) = self.substrate.entities.get(id) else {
                continue;
            };
            let Some(object) = self.object_type(building.type_ref(), rules) else {
                continue;
            };
            let ext = object.building_type_ext();
            // BuildingType PowerDrain (`+0xEE4`), the negated `Power=`.
            let consumer = crate::sim::power_system::native_building_power_drain(object.power) != 0;
            let damage = ext.degrade_amount.unwrap_or(if consumer {
                globals.degrade_amount_consumer
            } else {
                globals.degrade_amount_normal
            });
            let percentage = ext.degrade_percentage.unwrap_or(default_percentage);
            let ratio = X87Chop53::div(
                X87Chop53::load_i32(building.health.current()),
                X87Chop53::load_i32(object.strength),
            );
            let above = match (X87Chop53::load_f64(percentage), ratio) {
                (Ok(percentage), Ok(ratio)) => {
                    X87Chop53::compare(percentage, ratio) == X87Ordering::Less
                }
                _ => false,
            };
            if damage > 0 && above {
                let event = EntityDamageEvent::direct_receiver(
                    id,
                    damage,
                    0,
                    RAD_NO_ATTACKER,
                    None,
                    c4,
                    ReceiverCallFlags {
                        ignore_defenses: false,
                        arg6: false,
                    },
                );
                self.commit_direct_damage_receiver(rules, registry, event, frame_effects);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::house_state::HouseState;

    /// On a DamageDelay expiry a low-power House's consumer above
    /// `Degrade.Percentage` takes `Degrade.AmountConsumer`; one at the
    /// percentage, any between expiries and a `Degrades=no` country's are
    /// spared.
    #[test]
    fn low_power_degrades_consumers_above_the_percentage() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nDamageDelay=.1\nDegrade.Enabled=yes\nDegrade.AmountConsumer=10\n\
             Degrade.Percentage=50%\n[CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n\
             [Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [BuildingTypes]\n0=LAB\n[LAB]\nStrength=1000\nPower=-50\n\
             [Countries]\n0=Calm\n[Calm]\nDegrades=no\n",
        ))
        .unwrap();
        let mut sim = Simulation::with_seed(0x4F8B08);
        let owner = sim.interner.intern("Owner");
        sim.houses
            .register(HouseState::new(owner, 0, None, false, 0, 10));
        let calm = sim.interner.intern("Calm");
        let calm_owner = sim.interner.intern("CalmOwner");
        sim.houses
            .register(HouseState::new(calm_owner, 0, Some(calm), false, 0, 10));
        let lab = sim
            .spawn_object_at_height("LAB", "Owner", 10, 10, 0, 0, &rules)
            .unwrap();
        let calm_lab = sim
            .spawn_object_at_height("LAB", "CalmOwner", 20, 10, 0, 0, &rules)
            .unwrap();
        let half = sim
            .spawn_object_at_height("LAB", "Owner", 14, 10, 0, 0, &rules)
            .unwrap();
        sim.substrate
            .entities
            .get_mut(half)
            .unwrap()
            .health
            .set_for_test(500);
        assert!(
            sim.houses[&owner]
                .base_projection()
                .buildings()
                .contains(&lab)
        );
        for house in [owner, calm_owner] {
            let power = sim.power_states_mut_for_test().entry(house).or_default();
            power.set_total_output_for_test(0);
            power.set_total_drain_for_test(100);
        }
        let health =
            |sim: &Simulation, id| sim.substrate.entities.get(id).unwrap().health.current();
        // `.1` widened from binary32, times 900: 90 frames.
        for (frame, expected) in [(89, 1000), (90, 990), (91, 990), (180, 980)] {
            sim.session.set_binary_frame_for_test(frame);
            for house in [owner, calm_owner] {
                sim.ares_degrade_buildings(house, &rules, None, FrameEffects::default());
            }
            assert_eq!(health(&sim, lab), expected, "frame {frame}");
            assert_eq!(health(&sim, half), 500, "frame {frame}");
            assert_eq!(health(&sim, calm_lab), 1000, "frame {frame}");
        }
    }

    /// Constructed at frame 0 and restarted at each expiry: positive
    /// multiples of the duration, or every frame below one.
    #[test]
    fn the_damage_delay_expires_on_each_multiple() {
        assert!(!damage_delay_expires(0, 900));
        assert!(!damage_delay_expires(899, 900));
        assert!(damage_delay_expires(900, 900));
        assert!(damage_delay_expires(1800, 900));
        assert!(damage_delay_expires(0, 0));
        assert!(damage_delay_expires(7, -3));
    }
}
