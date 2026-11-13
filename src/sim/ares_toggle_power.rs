//! Ares computer power balancing: `HouseExt::ExtData::UpdateTogglePower @
//! 100228A0`, run at the head of `HouseClass::Update @ 0x004F8440`
//! (`HouseClass_Update_TogglePower @ 10024B40`, hook `0x004F8440`).
//!
//! With `[General] TogglePowerAllowed=yes`, a positive `TogglePowerDelay`
//! and an `[IQ] TogglePower` from 0 to the House's CurrentIQ (`+0x24C`), a
//! computer House that owns buildings, is not being drained (`+0x577B`) and
//! is outside a power blackout (`+0x2A4`) balances its power on every frame
//! that is a multiple of the delay. It ranks its `TogglePower=` consumers
//! whose StuffEnabled (`+0x6EA`) equals its low-power state by expendability
//! (a superweapon building's drain doubled, a non-`IsBaseDefense=` one's by
//! 15/10, else the drain; ties by UniqueID): with low power it switches the
//! most expendable off until the surplus is not negative, otherwise it
//! switches the least expendable back on while the surplus covers them.
//!
//! Scenario draws: none. Each switch is `GoOffline @ 0x00452360` /
//! `GoOnline @ 0x00452260` ([`Simulation::go_building_offline`],
//! [`Simulation::go_building_online`]), which invalidate the House power.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::power_system::native_building_power_drain;
use crate::sim::world::{FrameEffects, Simulation};

impl Simulation {
    /// `100228A0` for one House; see the module doc.
    pub(crate) fn ares_update_toggle_power(
        &mut self,
        owner: InternedId,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let globals = rules.ares_building_globals();
        let frame = self.session.binary_frame();
        let Some(house) = self.houses.get(&owner) else {
            return;
        };
        let buildings = house.base_projection().buildings();
        let Some(power) = self.power_states.get(&owner) else {
            return;
        };
        if !globals.toggle_power_allowed
            || globals.toggle_power_delay <= 0
            || globals.toggle_power_iq < 0
            || globals.toggle_power_iq > house.current_iq()
            || buildings.is_empty()
            || power.has_drained_power_source()
            || house.is_controlled_by_human(self.session.game_mode_nonzero())
            || power.blackout_remaining(frame) > 0
        {
            return;
        }
        if (frame as i32) % globals.toggle_power_delay != 0 {
            return;
        }
        let (output, drain) = power.effective_totals();
        // HasLowPower, inlined: a drain above the output.
        let low_power = drain != 0 && drain > output;
        // (expendability, UniqueID, id, drain), sorted least expendable
        // first; the UniqueID tie-break makes the order total.
        let mut candidates: Vec<(i32, i32, u64, i32)> = buildings
            .iter()
            .filter_map(|&id| {
                let building = self.substrate.entities.get(id)?;
                if building.category() != EntityCategory::Structure {
                    return None;
                }
                let object = self.object_type(building.type_ref(), rules)?;
                let drain = native_building_power_drain(object.power);
                if !object.toggle_power
                    || drain <= 0
                    || building.building_art.stuff_enabled() != low_power
                {
                    return None;
                }
                let expendability =
                    if object.super_weapon.is_some() || object.super_weapon2.is_some() {
                        drain.wrapping_mul(2)
                    } else if !object.is_base_defense {
                        drain.wrapping_mul(15) / 10
                    } else {
                        drain
                    };
                Some((expendability, building.native_unique_id(), id, drain))
            })
            .collect();
        candidates.sort_unstable();
        let mut surplus = output.wrapping_sub(drain);
        if low_power {
            for &(_, _, id, drain) in candidates.iter().rev() {
                surplus = surplus.wrapping_add(drain);
                self.go_building_offline(id, rules, effects);
                if surplus >= 0 {
                    break;
                }
            }
        } else {
            for &(_, _, id, drain) in &candidates {
                let left = surplus.wrapping_sub(drain);
                if left >= 0 {
                    self.go_building_online(id, rules, effects);
                    surplus = left;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::house_state::HouseState;
    use crate::sim::world::{FrameEffects, Simulation};

    /// With low power the most expendable consumer (a non-base-defense
    /// drain counts 15/10) goes offline until the surplus is not negative;
    /// with power to spare it comes back, and only on multiples of the delay.
    #[test]
    fn computer_houses_switch_expendable_consumers_off_and_back_on() {
        let _ares = crate::rules::ares_switch::read_ares_way(true);
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nTogglePowerAllowed=yes\nTogglePowerDelay=45\n[IQ]\nTogglePower=0\n\
             [BuildingTypes]\n0=WALLGUN\n1=LAB\n\
             [WALLGUN]\nStrength=500\nPower=-50\nIsBaseDefense=yes\n\
             [LAB]\nStrength=500\nPower=-50\n",
        ))
        .unwrap();
        let mut sim = Simulation::with_seed(0x4F8440);
        let owner = sim.interner.intern("Computer");
        sim.houses
            .register(HouseState::new(owner, 0, None, false, 0, 10));
        let gun = sim
            .spawn_object_at_height("WALLGUN", "Computer", 10, 10, 0, 0, &rules)
            .unwrap();
        let lab = sim
            .spawn_object_at_height("LAB", "Computer", 14, 10, 0, 0, &rules)
            .unwrap();
        let online =
            |sim: &Simulation, id| sim.substrate.entities.get(id).unwrap().building_online();
        let balance = |sim: &mut Simulation, frame, output| {
            let power = sim.power_states_mut_for_test().entry(owner).or_default();
            power.set_total_output_for_test(output);
            power.set_total_drain_for_test(100);
            sim.session.set_binary_frame_for_test(frame);
            sim.ares_update_toggle_power(owner, &rules, FrameEffects::default());
        };
        balance(&mut sim, 44, 50);
        assert!(online(&sim, gun) && online(&sim, lab), "not a multiple");
        balance(&mut sim, 45, 50);
        assert!(online(&sim, gun) && !online(&sim, lab));
        balance(&mut sim, 90, 200);
        assert!(online(&sim, gun) && online(&sim, lab));
    }
}
