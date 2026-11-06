//! Hospital44B939..44B9CD plays HealCrateSound before removing its cargo head.
use std::cell::Cell;

use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags};
use crate::sim::passenger::{BoardingArm, board_passenger};
use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation, SoundBoundary};

#[derive(Default)]
struct Heard {
    hospital: u64,
    patient: u64,
    delivered: Cell<usize>,
    heals: Cell<usize>,
}

impl SoundBoundary for Heard {
    fn consume(&self, sim: &Simulation, _: &RuleSet, events: &[SimSoundEvent]) {
        for event in &events[self.delivered.get()..] {
            if let SimSoundEvent::VocAt {
                sound_id,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
                ..
            } = event
                && sound_id.eq_ignore_ascii_case("HealCrate")
            {
                let hospital = sim.entities().get(self.hospital).unwrap();
                assert_eq!(
                    hospital.passenger_role.cargo().unwrap().passengers(),
                    [self.patient],
                    "native7509E0 precedes hospital Cargo.RemoveFirst473430"
                );
                let patient = sim.entities().get(self.patient).unwrap();
                assert!(patient.lifecycle.in_limbo());
                assert_eq!(sound_id, "HealCrate", "fixed SOUNDMD identity");
                assert_eq!((*rx, *ry), (hospital.position.rx(), hospital.position.ry()));
                assert_eq!(
                    (*sub_x, *sub_y),
                    (hospital.position.sub_x(), hospital.position.sub_y())
                );
                assert_eq!(*world_z_leptons, 0);
                self.heals.set(self.heals.get() + 1);
            }
        }
        self.delivered.set(events.len());
    }

    fn delivered(&self) -> usize {
        self.delivered.get()
    }
}

#[test]
fn retail_hospital_heal_sound_is_delivered_before_the_patient_leaves_cargo() {
    // Retail Tech Hospitals only grant passive self-heal. Author the active
    // Hospital arm and InitialAmmo through the actual scenario/type readers.
    let patch = IniFile::from_str(
        "[General]\nIRepairRate=0\nIRepairStep=1\n\
         [CATHOSP]\nHospital=yes\nAmmo=2\nInitialAmmo=2\n",
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
    else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x44b939);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern("Americans");
    sim.session.set_current_house_for_test(Some(owner));
    let hospital = sim
        .spawn_object("CATHOSP", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let patient = sim
        .spawn_object("E1", "Americans", 14, 10, 0, &rules)
        .unwrap();
    sim.spawn_object("NAPOWR", "Russians", 25, 25, 0, &rules)
        .unwrap();
    for _ in 0..3 {
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    let strength = rules.object("E1").unwrap().strength;
    let ap = sim.interner.intern("AP");
    sim.commit_direct_damage_receiver(
        &rules,
        None,
        EntityDamageEvent::direct_receiver(
            patient,
            2,
            0,
            0,
            None,
            ap,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
        FrameEffects::empty(),
    );
    assert_eq!(
        sim.entities().get(patient).unwrap().health.current(),
        strength - 2
    );
    let building = sim.entities().get(hospital).unwrap();
    assert_eq!(building.ammo().unwrap().current(), 2);
    // The real admission/contact/arrival owners prepare the hospital patient;
    // movement to the building is outside this sound-order regression.
    for message in [
        RadioMessage::CanEnter,
        RadioMessage::Hello,
        RadioMessage::DockNow,
    ] {
        assert_eq!(
            radio::transmit(
                &mut sim,
                patient,
                hospital,
                message,
                RadioPayload::default(),
                &rules,
                FrameEffects::empty()
            ),
            RadioResponse::Roger
        );
    }
    assert!(board_passenger(
        &mut sim,
        &rules,
        patient,
        hospital,
        BoardingArm::InfantryBuilding,
        None,
        FrameEffects::empty()
    ));
    let now = sim.session.binary_frame();
    assert!(sim.substrate.mission_commence_exact(hospital, now).unwrap());
    let _ = sim.take_sound_events_for_test();
    let heard = Heard {
        hospital,
        patient,
        ..Default::default()
    };
    let effects = FrameEffects::for_sound(&heard);
    let _ = super::building_dock::mission_repair(&mut sim, &rules, hospital, None, effects);
    // First tick repairs one point without completing; the next completes.
    for (health, cues, cargo) in [(strength - 1, 0, 1), (strength, 1, 0)] {
        let _ = super::building_dock::mission_repair(&mut sim, &rules, hospital, None, effects);
        effects.flush_sound(&sim, &rules);
        let building = sim.entities().get(hospital).unwrap();
        assert_eq!(
            sim.entities().get(patient).unwrap().health.current(),
            health
        );
        assert_eq!(heard.heals.get(), cues);
        assert_eq!(building.passenger_role.passenger_count(), cargo);
    }
}
