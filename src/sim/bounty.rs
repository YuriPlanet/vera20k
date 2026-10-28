//! Ares bounty admission and payment at RecordKill702E64 ->1004B100.
//! Source: installed Ares payout10043230..10043395 and display10044490.
//! HouseTracking and Economy retain ownership of counts and money.

use crate::{
    map::{entities::EntityCategory, houses::HouseAllianceMap},
    rules::ruleset::RuleSet,
    sim::{
        combat::veterancy::VeterancyRank,
        entity_store::EntityStore,
        houses::Houses,
        intern::StringInterner,
        projectile::ProjectileCoord,
        world::{FrameEffects, Simulation},
    },
    util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits},
};

/// A consumed signed credit label, before the client's shroud/visual-character
/// checks. Display rejection cannot undo the wallet or requeue the amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BountyText {
    pub entity_id: u64,
    pub amount: i32,
    /// Unraised virtual+48 point: IsShrouded586360 reads its height too.
    pub coord: ProjectileCoord,
    pub z_offset: i32,
}

/// TechnoExt+A8/AC, cleared by constructor10042DFE/10042E04. This queued
/// presentation survives saves and in-place conversion, but is not gameplay CRC.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BountyDisplayState {
    pending: i32,
    deadline: i32,
}

impl BountyDisplayState {
    fn queue(&mut self, amount: i32) {
        self.pending = self.pending.wrapping_add(amount);
    }

    ///10044497..100444E7 consumes the sum before checking presentation
    /// visibility. The absolute signed deadline is not a CdTimer.
    fn take_ready(&mut self, frame: i32, force: bool, delay: NativeF64Bits) -> Option<i32> {
        if self.pending == 0 || (!force && frame < self.deadline) {
            return None;
        }
        let amount = std::mem::take(&mut self.pending);
        //100444C2..100444E7: FTOL(delay * binary64(-900)), then SUB32
        // from the current Scenario frame. Reuse the shared numeric kernel.
        let offset = X87::ftol_i32_low_masked(X87::mul(
            X87::load_f64(delay),
            X87::load_f64(NativeF64Bits::from_bits(0xc08c_2000_0000_0000)),
        ));
        self.deadline = frame.wrapping_sub(offset);
        Some(amount)
    }
}

impl Simulation {
    /// Ares TechnoExt::DisplayCredits10044490. ChangeOwner7014D5 and
    /// Remove6F6AC9 force the same owner; Update1004B658 observes its deadline.
    pub(crate) fn flush_bounty_display(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
        force: bool,
    ) {
        let frame = self.session.binary_frame() as i32;
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        let Some(amount) =
            actor
                .bounty_display
                .take_ready(frame, force, rules.bounty_rules().display_delay())
        else {
            return;
        };
        let actor = self
            .substrate
            .entities
            .get(id)
            .expect("borrowed actor exists");
        //10044531/10044568: virtual+48 is the foundation centre for a
        // building; its lift is the shared native Height*104 dimension.
        let coord = crate::sim::movement::ground_pose::object_get_coords(
            actor,
            self.resolved_terrain.as_ref(),
        );
        let lift = if actor.category() == EntityCategory::Structure {
            let Some(object) = self.interner.object_type(actor.type_ref(), rules) else {
                return;
            };
            rules.building_dimensions(object)[2]
        } else {
            256
        };
        effects.emit_bounty_text(
            self,
            rules,
            BountyText {
                entity_id: id,
                amount,
                coord: ProjectileCoord {
                    x: coord.x,
                    y: coord.y,
                    z: coord.z,
                },
                z_offset: lift,
            },
        );
    }
}

/// Caller has already run the concrete/base Tag prefixes and DontScore gate.
/// Payout owns no RNG, detach or gameplay timer; display owns only its deadline.
pub(crate) fn award(
    entities: &mut EntityStore,
    houses: &mut Houses,
    interner: &StringInterner,
    alliances: &HouseAllianceMap,
    rules: &RuleSet,
    victim_id: u64,
    attacker_id: u64,
) {
    let Some(attacker) = entities.get(attacker_id) else {
        return;
    };
    let Some(attacker_type) = interner.object_type(attacker.type_ref(), rules) else {
        return;
    };
    if !attacker_type.bounty_rules().enabled() {
        return;
    }
    let Some(victim) = entities.get(victim_id) else {
        return;
    };
    let attacker_owner = attacker.owner();
    let victim_owner = victim.owner();
    //10043244..10043273 asks the VICTIM's ally bits for the attacker's
    // index. Original kill XP makes the opposite one-way test afterward.
    if crate::map::houses::is_allied_with(
        alliances,
        interner.resolve(victim_owner),
        interner.resolve(attacker_owner),
    ) {
        return;
    }
    let Some(victim_country) = houses.get(&victim_owner).and_then(|house| house.country()) else {
        return;
    };
    if !rules.country_gives_bounty(interner.resolve(victim_country)) {
        return;
    }
    let Some(attacker_house) = houses.get(&attacker_owner) else {
        return;
    };
    let enablers = rules.bounty_rules().enablers();
    if !enablers.is_empty()
        && !enablers.iter().any(|name| {
            interner.get(name).is_some_and(|id| {
                //100432B0..100432C6 reads House+5554, the on-map counter,
                // independently of power, operational or tracked-owned state.
                attacker_house
                    .tracking()
                    .active_count(EntityCategory::Structure, id)
                    > 0
            })
        })
    {
        return;
    }
    let Some(victim_type) = interner.object_type(victim.type_ref(), rules) else {
        return;
    };
    let rank = match victim.veterancy.rank_kind() {
        VeterancyRank::Rookie => 0,
        VeterancyRank::Veteran => 1,
        VeterancyRank::Elite => 2,
    };
    let amount = victim_type.bounty_rules().values()[rank];
    if amount == 0 {
        return;
    }
    let display = attacker_type
        .bounty_rules()
        .display()
        .unwrap_or_else(|| rules.bounty_rules().display());
    //1004336B..1004338A redirects only the displayed sum for missiles.
    // Funds still go to this incoming attacker's live House.
    let display_actor = if attacker_type.missile_spawn {
        attacker.spawn_owner_id.owner().unwrap_or(attacker_id)
    } else {
        attacker_id
    };
    let Some(house) = houses.get_mut(&attacker_owner) else {
        return;
    };
    if amount > 0 {
        //10011E20 is the original Add_Credits4F9950 trampoline.
        house.economy_mut().add_credits(amount);
    } else {
        //100130C0 is Spend_Money4F9790, including its spending statistic.
        house.economy_mut().spend_money(amount.wrapping_neg());
    }
    if display && let Some(actor) = entities.get_mut(display_actor) {
        actor.bounty_display.queue(amount);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        rules::ini_parser::IniFile,
        sim::{
            combat::{KillCallback, KillSource},
            house_state::HouseState,
            world::UninitContext,
        },
    };
    use std::cell::RefCell;

    #[test]
    fn bounty_kill_admission_wallet_and_forced_display_use_existing_owners() {
        let rules = RuleSet::from_ini_with_fixed_art_for_test(
            &crate::sim::arena_fixture::clear_ground_ini(
                "[Countries]\n0=COUNTRY\n1=BLOCKED\n[COUNTRY]\nMultiplayPassive=yes\n\
                 [BLOCKED]\nGivesBounty=no\n[VehicleTypes]\n0=HUNTER\n1=TARGET\n2=PENALTY\n\
                 [BuildingTypes]\n0=BANK\n[General]\nBountyEnablers=BANK\n\
                 [AudioVisual]\nBountyDisplay=yes\n[HUNTER]\nStrength=100\nBounty=yes\n\
                 [TARGET]\nStrength=100\nCost=0\nBounty.Value=30\nBounty.EliteValue=70\n\
                 [PENALTY]\nStrength=100\nCost=0\nBounty.Value=-200\n[BANK]\nStrength=100\n",
            ),
            &IniFile::from_str("[BANK]\nFoundation=1x1\nHeight=1\n"),
        )
        .unwrap();
        let mut sim = Simulation::new();
        let country = sim.interner.intern("COUNTRY");
        let blocked = sim.interner.intern("BLOCKED");
        let owner = sim.interner.intern("Americans");
        let enemy = sim.interner.intern("Russians");
        sim.houses
            .register(HouseState::new(owner, 0, Some(country), true, 50, 10));
        sim.houses
            .register(HouseState::new(enemy, 0, Some(country), false, 0, 10));
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let hunter = sim
            .spawn_object("HUNTER", "Americans", 16, 16, 64, &rules)
            .unwrap();
        let victim = sim
            .spawn_object("TARGET", "Russians", 18, 16, 64, &rules)
            .unwrap();
        let penalty = sim
            .spawn_object("PENALTY", "Russians", 20, 16, 64, &rules)
            .unwrap();
        let record = |sim: &mut Simulation, id, source| {
            sim.record_the_kill(
                id,
                source,
                KillCallback::OwnerChange,
                UninitContext::with_rules(&rules),
            )
        };
        let source = KillSource::Techno(Some(hunter));
        record(&mut sim, victim, source);
        assert_eq!(
            sim.houses[&owner].economy().credits(),
            50,
            "missing enabler"
        );
        let bank = sim
            .spawn_object("BANK", "Americans", 10, 10, 64, &rules)
            .unwrap();
        sim.techno_limbo_with_rules(bank, &rules, None);
        record(&mut sim, victim, source);
        assert_eq!(
            sim.houses[&owner].economy().credits(),
            50,
            "owned but limbo is not on-map"
        );
        sim.spawn_object("BANK", "Americans", 10, 10, 64, &rules)
            .unwrap();
        sim.houses
            .get_mut(&enemy)
            .unwrap()
            .set_country_for_test(Some(blocked));
        record(&mut sim, victim, source);
        assert_eq!(
            sim.houses[&owner].economy().credits(),
            50,
            "victim Country gate"
        );
        sim.houses
            .get_mut(&enemy)
            .unwrap()
            .set_country_for_test(Some(country));
        sim.house_alliances_mut_for_test()
            .insert("RUSSIANS".into(), ["AMERICANS".into()].into());
        record(&mut sim, victim, source);
        assert_eq!(
            sim.houses[&owner].economy().credits(),
            50,
            "victim's one-way ally bit"
        );
        sim.house_alliances_mut_for_test().clear();
        sim.house_alliances_mut_for_test()
            .insert("AMERICANS".into(), ["RUSSIANS".into()].into());
        record(&mut sim, victim, source);
        assert_eq!(
            sim.houses[&owner].economy().credits(),
            80,
            "opposite ally bit does not block bounty"
        );
        let events = RefCell::new(Vec::new());
        let observe = |sim: &Simulation, _: &RuleSet, text: BountyText| {
            events
                .borrow_mut()
                .push((text, sim.entities().get(text.entity_id).unwrap().owner()));
        };
        let effects = FrameEffects::empty().with_bounty_text(&observe);
        sim.flush_bounty_display(hunter, &rules, effects, false);
        assert_eq!(events.borrow()[0].0.amount, 30);
        sim.session.set_binary_frame_for_test(1);
        sim.substrate
            .entities
            .get_mut(victim)
            .unwrap()
            .veterancy
            .set_elite();
        record(&mut sim, victim, source);
        sim.flush_bounty_display(hunter, &rules, effects, false);
        assert_eq!(events.borrow().len(), 1, "delay retains a later kill's sum");
        record(&mut sim, penalty, KillSource::House(Some(owner)));
        sim.substrate
            .entities
            .get_mut(penalty)
            .unwrap()
            .tracking_facts
            .set_dont_score_for_test(true);
        record(&mut sim, penalty, source);
        assert_eq!(
            sim.houses[&owner].economy().credits(),
            150,
            "E4 and DontScore skip the hook"
        );
        sim.substrate
            .entities
            .get_mut(penalty)
            .unwrap()
            .tracking_facts
            .set_dont_score_for_test(false);
        record(&mut sim, penalty, source);
        assert_eq!(sim.houses[&owner].economy().credits(), 0);
        assert_eq!(
            sim.houses[&owner].economy().spent_credits(),
            150,
            "negative value uses SpendMoney"
        );
        let state = sim
            .substrate
            .entities
            .get(hunter)
            .unwrap()
            .bounty_display
            .clone();
        let saved: BountyDisplayState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        assert_eq!(saved, state);
        let before = sim.state_hash();
        sim.change_owner_with_rules(hunter, enemy, &rules, None, effects);
        assert_eq!(
            events.borrow()[1].0.amount,
            -130,
            "display uses requested, not clipped payment"
        );
        assert_eq!(
            events.borrow()[1].1,
            owner,
            "forced display precedes owner swap"
        );
        assert_ne!(
            sim.state_hash(),
            before,
            "ownership still changes gameplay state"
        );
        let before = sim.state_hash();
        sim.flush_bounty_display(hunter, &rules, effects, true);
        assert_eq!(events.borrow().len(), 2, "consumed sum never repeats");
        assert_eq!(
            sim.state_hash(),
            before,
            "display state is outside gameplay CRC"
        );
    }
}
