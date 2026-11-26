//! Native factory delivery owns its product before AutoPlay does.
//! Do not issue a Stop/Move/Attack/attachment while contact 0 names a war
//! factory (or its native tether still holds), even when the native event
//! actor gate already accepts the unit.
//! No timer grants control, and no direct queue/tether/door change is made.
use std::collections::{BTreeMap, BTreeSet};
use crate::sim::{command::Command, control::brain::Outbox};
use super::AssistBrain;

#[derive(Debug, Default)]
pub(super) struct Director {
    pending: BTreeMap<u64, (u64,u64)>, // actor -> (producer, first observation)
    warned: BTreeSet<u64>,
    veto_reports: BTreeSet<u64>,
}

pub(super) fn observe(brain: &mut AssistBrain, tick: u64) {
    if tick % 600 == 0 {
        log::info!("auto play: factory_handoff tick={tick} pending={}",brain.census.factory_handoffs.len());
    }
    let state = &mut brain.factory_delivery;
    let done: Vec<_> = state.pending.iter().filter(|(actor,_)|
        !brain.census.factory_handoffs.contains_key(*actor)).map(|(a,v)| (*a,*v)).collect();
    for (actor,(factory,since)) in done {
        // A missing link is not proof of successful movement (the unit or
        // producer could have died). Log the observed end, not a made-up receipt.
        let live = brain.census.units.iter().any(|u| u.entity_id==actor && u.orderable);
        log::info!("auto play: factory_handoff_end tick={tick} actor={actor} factory={factory} elapsed={} orderable={live}",tick.saturating_sub(since));
        state.pending.remove(&actor);
        state.warned.remove(&actor);
        state.veto_reports.remove(&actor);
    }
    for (&actor,&factory) in &brain.census.factory_handoffs {
        let (_, since) = *state.pending.entry(actor).or_insert((factory,tick));
        if tick.saturating_sub(since)>=450 && state.warned.insert(actor) {
            log::info!("auto play: factory_handoff_stalled tick={tick} actor={actor} factory={factory} elapsed={}",tick.saturating_sub(since));
        }
    }
}

/// Direct commands currently used by AssistBrain are mostly building or
/// infantry toggles. Also cover the ordinary actor-bearing command forms so
/// a future tactical pass cannot evade the delivery guard by using raw clicks.
fn command_actor(cmd: &Command) -> Option<u64> {
    match cmd {
        Command::Move {entity_id,..} | Command::MoveToObject {entity_id,..}
        | Command::Stop {entity_id} | Command::AttackMove {entity_id,..}
        | Command::Guard {entity_id,..} | Command::DeployMcv {entity_id}
        | Command::ToggleInfantryDeploy {entity_id} | Command::Scatter {entity_id}
        | Command::MegaMission {entity_id,..} | Command::ArchiveObject {entity_id,..} => Some(*entity_id),
        Command::Attack {attacker_id,..} | Command::ForceAttackCell {attacker_id,..}
        | Command::PlantC4 {attacker_id,..} => Some(*attacker_id),
        Command::CaptureBuilding {engineer_id,..} => Some(*engineer_id),
        Command::UnloadPassengers {transport_id} => Some(*transport_id),
        _ => None,
    }
}

pub(super) fn filter(brain: &mut AssistBrain, tick: u64, out: &mut Outbox) {
    let pending = &brain.census.factory_handoffs;
    if pending.is_empty() { return; }
    let mut vetoed = BTreeSet::new();
    out.orders.retain(|o| {
        if pending.contains_key(&o.actor) { vetoed.insert(o.actor); false } else { true }
    });
    out.attachments.retain(|a| {
        if pending.contains_key(&a.actor) { vetoed.insert(a.actor); false } else { true }
    });
    out.commands.retain(|c| {
        if let Some(id) = command_actor(c).filter(|id| pending.contains_key(id)) {
            vetoed.insert(id); false
        } else { true }
    });
    for actor in vetoed {
        brain.tracked.cancel_for_safety(actor);
        brain.safety.forget(actor);
        // No emergency Stop: that is itself an order to a product!
        if brain.factory_delivery.veto_reports.insert(actor) {
            log::info!("auto play: factory_handoff_veto tick={tick} actor={actor}");
        }
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::control::{Goal,observation::Observation};

    #[test]
    fn contact_and_tether_each_keep_the_product_out_of_tactical_control() {
        let rules=RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=FACTORY\n\
             [FACTORY]\nStrength=1000\nWeaponsFactory=yes\nFactory=UnitType\nFoundation=3x3\n\
             [TANK]\nStrength=300\nSpeed=5\nPrimary=Gun\nArmor=heavy\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [Gun]\nDamage=50\nROF=30\nRange=5\nWarhead=AP\nProjectile=Bullet\n\
             [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [Bullet]\nAG=yes\nAA=no\nInviso=yes\n"
        )).unwrap();
        let mut sim=crate::sim::arena_fixture::rival_arena_known(&rules,0xA482);
        sim.intern_rule_type_ids(&rules);sim.resolve_type_handles(&rules);
        let owner=sim.interner.intern("Americans");
        let factory=sim.spawn_object("FACTORY","Americans",10,10,255,&rules).unwrap();
        let tank=sim.spawn_object("TANK","Americans",16,11,255,&rules).unwrap();
        let other=sim.spawn_object("TANK","Americans",18,11,255,&rules).unwrap();
        {
            let entity=sim.substrate.entities.get_mut(tank).unwrap();
            entity.seldom.radio_contacts.insert(factory);
            entity.seldom.tether.set_for_test(Some(factory));
        }
        let mut brain=AssistBrain::new(0xA482);
        for phase in 0..3 {
            if phase==1 {sim.substrate.entities.get_mut(tank).unwrap().seldom.radio_contacts.clear_all();}
            if phase==2 {sim.substrate.entities.get_mut(tank).unwrap().seldom.tether.set_for_test(None);}
            let obs=Observation::new(&sim,owner);
            assert!(obs.own_entities().any(|e|e.entity_id==tank && e.orderable));
            brain.rebuild_census(&rules,&obs);
            observe(&mut brain,phase*600);
            let protected=phase<2;
            assert_eq!(obs.own_factory_handoff(&rules,tank),protected.then_some(factory));
            assert_eq!(brain.census.factory_handoffs.contains_key(&tank),protected);
            let unit=brain.census.units.iter().find(|u|u.entity_id==tank).unwrap();
            assert_eq!(unit.controllable(),!protected);
            assert!(!protected || !brain.census.available(unit));
            assert_eq!(brain.census.limbo,0);
            let mut out=Outbox::default();
            let goal=Goal::Move {cell:(16,20)};
            assert_eq!(super::super::orders::issue(&brain,&mut out,&rules,&obs,tank,goal),!protected);
            // Simulate an accidental bypass of the ordinary order entrance.
            out.want(tank,0,Goal::Stop);
            out.want(other,0,goal);
            out.command(Command::Stop {entity_id:tank});
            out.resolve();
            filter(&mut brain,phase*600,&mut out);
            assert_eq!(out.orders.iter().any(|o|o.actor==tank),!protected);
            assert_eq!(out.commands.iter().any(|c|command_actor(c)==Some(tank)),!protected);
            assert!(out.orders.iter().any(|o|o.actor==other));
        }
    }

    #[test]
    fn native_retail_tank_delivery_keeps_control_until_the_link_releases() {
        let Some(retail)=crate::rules::retail_ini_fixture::retail_battle_rules() else {return;};
        let rules=retail.rules;
        let mut sim=crate::sim::arena_fixture::rival_arena_with_size(&rules,0xA483,48);
        sim.intern_rule_type_ids(&rules);sim.resolve_type_handles(&rules);
        let owner=sim.interner.intern("Americans");
        sim.session.set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
        sim.session.set_current_house_for_test(Some(owner));
        let grid=sim.path_grid_snapshot().map(|g|(*g).clone()).unwrap();
        let factory=sim.spawn_object("GAWEAP","Americans",20,20,0,&rules).unwrap();
        sim.spawn_object("NAPOWR","Russians",14,30,0,&rules).unwrap();
        let tank=sim.construct_object_limbo_at_height("MTNK","Americans",0,0,0,0,&rules,
            &mut crate::sim::world::FrameEffects::default()).unwrap();
        assert!(matches!(crate::sim::production::exit_produced_object(&mut sim,&rules,factory,tank,(0,0),None,
            &mut crate::sim::world::FrameEffects::default()),crate::sim::ai_base_building::BuildingExit::Placed));
        let mut brain=AssistBrain::new(0xA483);
        let mut held=0;
        let mut released=false;
        for tick in 0..600 {
            {
                let obs=Observation::new(&sim,owner);
                brain.rebuild_census(&rules,&obs);
                if obs.own_factory_handoff(&rules,tank).is_none() {
                    assert!(held>0,"native link must be observed during delivery");
                    assert!(brain.census.units.iter().any(|u|u.entity_id==tank && u.controllable()));
                    released=true;break;
                }
                held+=1;
                let mut out=Outbox::default();
                out.want(tank,0,Goal::Move {cell:(28,30)});out.resolve();
                out.command(Command::Stop {entity_id:tank});
                filter(&mut brain,tick,&mut out);
                assert!(out.orders.is_empty() && out.commands.is_empty());
            }
            let commands=sim.take_due_commands();
            sim.advance_tick(&commands,Some(&rules),Some(&grid),None,33);
        }
        assert!(released,"native unload should finish without assistant interruption");
    }
}
