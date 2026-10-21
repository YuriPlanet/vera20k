//! Hospital arm of Building::MissionRepairAndProduce44B780,44B870..44BA79.
//! Original Steam gamemd.exe.
//! Instruction-level reading. It uses the
//! building's existing StageClass, Techno RepairTick and ExitObject owners.
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::passenger::{DepartureRoute, depart_cargo_head};
use crate::sim::radio::{self, RadioMessage, RadioResponse};
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};
use crate::util::native_x87::{
    MaskedX87Chop53 as X87, MaskedX87Ordering as Ordering, NativeF64Bits,
};

pub(super) fn mission_repair(
    sim: &mut Simulation,
    rules: &RuleSet,
    hospital: u64,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> i32 {
    let now = sim.session.binary_frame();
    //44BA44 reads the live BuildingType+684 Ammo, not an instance cache.
    let type_ammo = sim
        .substrate
        .entities
        .get(hospital)
        .and_then(|building| sim.object_type(building.type_ref(), rules))
        .map_or(-1, |object| object.ammo);
    let Some(building) = sim.substrate.entities.get_mut(hospital) else {
        return 1;
    };
    let state = building.mission.handler_state();
    if state == 0 {
        //44BA0B: status2, ready0, progress0, stage rate/increment1.
        building.mission.set_handler_state(2);
        building.mission_leaf.set_building_ready_latch(0);
        building
            .mission_leaf
            .start_building_repair_progress(now as i32);
        if let Some(ammo) = building.ammo_mut()
            && type_ammo != -1
            && ammo.add(-1) < 0
        {
            ammo.empty();
        }
    } else if state == 2 {
        let progress = building
            .mission_leaf
            .advance_building_repair_progress(now as i32);
        //44B8E6: compare the unrounded double rate*900 with the signed
        //Stage value, including the native unordered branch.
        let threshold = X87::mul(
            X87::load_f64(NativeF64Bits::from_bits(
                rules.general.infantry_repair_rate.to_bits(),
            )),
            X87::load_i32(900),
        );
        if X87::compare(threshold, X87::load_i32(progress)) == Ordering::Greater {
            return 1;
        }
        building.mission_leaf.set_building_ready_latch(0);
        building.mission_leaf.reset_building_repair_progress();
        let response =
            radio::transmit_to_contact(sim, hospital, RadioMessage::RepairTick, rules, effects);
        if matches!(
            response,
            RadioResponse::Negatory
                | RadioResponse::InsufficientFunds
                | RadioResponse::RepairComplete
        ) {
            if response == RadioResponse::RepairComplete
                && let Some(building) = sim.substrate.entities.get(hospital)
                && sim.house_is_human_player(building.owner())
            {
                //44B939/50B6F0: player-house gate covers radar/EVA AND HealCrateSound;
                //EVA itself is further gated by the app's radar admission.
                sim.sound_events.emit(SimSoundEvent::UnitRepaired {
                    owner: building.owner(),
                    radar: crate::sim::radar::RadarEventRequest::new(
                        crate::sim::radar::RadarEventType::UnitRepaired,
                        building.position.rx(),
                        building.position.ry(),
                    ),
                });
                if let Some(sound_id) = rules.crate_rules.heal_crate_sound.clone() {
                    let coordinate =
                        crate::sim::movement::ground_pose::position_world_coord(&building.position);
                    sim.sound_events.emit(SimSoundEvent::voc_at_world_coord(
                        sound_id,
                        crate::sim::anim_class::AnimWorldCoord {
                            x: coordinate.x,
                            y: coordinate.y,
                            z: coordinate.z,
                        },
                    ));
                }
            }
            //44B9A8/44B9E0: pop first, call the existing443C60 exit,
            //ignore its return. A blocked exit is not an absorber retry.
            let _ = depart_cargo_head(
                sim,
                rules,
                registry,
                hospital,
                DepartureRoute::Hospital,
                |sim, patient| {
                    let _ = crate::sim::production::exit_produced_object(
                        sim, rules, hospital, patient, registry, effects,
                    );
                    Ok(())
                },
                effects,
            );
            if response != RadioResponse::InsufficientFunds {
                let _ = sim.mission_queue_exact(
                    hospital,
                    MissionId::from_known(MissionType::Guard),
                    0,
                    now,
                    &LiveReadyInputProvider { rules },
                );
            }
        }
        return 1;
    }
    //44C819: initial/other states use this mission's configured Rate;
    //subsequent state2 visits return literal1, without random draws.
    sim.substrate.entities.get(hospital).map_or(1, |building| {
        rules.mission_control.rate_frames(
            building
                .mission
                .current()
                .known()
                .unwrap_or(MissionType::Repair),
        )
    })
}
