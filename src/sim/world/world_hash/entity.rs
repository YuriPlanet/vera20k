//! Per-object state-hash folds: one entity's components in fold order, its
//! mission component and leaf, and its locomotor (with the piggybacked one
//! and the slope transition state).

use std::hash::{Hash, Hasher};

use crate::sim::game_entity::GameEntity;
use crate::util::state_hasher::StateHasher;

/// Fold the `MissionCom` mission component into the state hash.
///
/// `MissionCom` intentionally does not derive `Hash`: every lossless selector,
/// raw latch/state dword, and signed timer dword is folded explicitly in the
/// snapshot schema order. Current compatibility projections remain ordinary
/// named writers until the production authority crosswalk is complete.
///
/// `owed_visits` are the `+0xC4` increments a sleeping object has not taken
/// yet (`world::idle_sleep`); the hash reads the counter as settled.
fn hash_mission_com(
    mission: &crate::sim::mission::MissionCom,
    owed_visits: u32,
    hasher: &mut impl Hasher,
) {
    mission.current().raw().hash(hasher);
    mission.suspended().raw().hash(hasher);
    mission.queued().raw().hash(hasher);
    mission.movement_bypass_latch().hash(hasher);
    mission.handler_state().hash(hasher);
    mission.mission_start_frame().hash(hasher);
    mission.ai_counter().wrapping_add(owed_visits).hash(hasher);
    mission.dispatch_timer().start_frame().hash(hasher);
    mission.dispatch_timer().delay().hash(hasher);
}

fn hash_mission_leaf(leaf: &crate::sim::mission::MissionLeafState, hasher: &mut impl Hasher) {
    if let Some(unit) = leaf.as_unit() {
        0u8.hash(hasher);
        unit.deployed().hash(hasher);
        unit.deploy_begin_active().hash(hasher);
        unit.deploy_reverse_active().hash(hasher);
        unit.tracker_byte_18().hash(hasher);
        unit.tracker_byte_19().hash(hasher);
    } else if let Some(infantry) = leaf.as_infantry() {
        1u8.hash(hasher);
        infantry.firing_sequence_latch().hash(hasher);
        infantry.doing().hash(hasher);
        infantry.pending_deploy().hash(hasher);
        // Infantry ctor517AC2 starts6E8 at2. Water/land DoAction51D8B8
        // retains0/1 before admission, affecting later sound requests.
        // Like the other sparse native bytes, the constructor adds no bytes.
        if infantry.water_state() != 2 {
            b"infantry-water-state-6e8".hash(hasher);
            infantry.water_state().hash(hasher);
        }
    } else if let Some(aircraft) = leaf.as_aircraft() {
        2u8.hash(hasher);
        aircraft.action_latch().hash(hasher);
        aircraft.transition_ready_latch().hash(hasher);
        // Ctor 0x00413D74 starts +0x6D3 at 5, which adds no bytes; Overfly
        // reads it to come round again.
        if aircraft.paradrop_passes() != 5 {
            b"aircraft-paradrop-passes-6d3".hash(hasher);
            aircraft.paradrop_passes().hash(hasher);
        }
    } else if let Some(building) = leaf.as_building() {
        3u8.hash(hasher);
        building.ready_latch().hash(hasher);
        b"building-repair-progress-620".hash(hasher);
        building.repair_progress().hash(hasher);
        // Constructor -1 adds no bytes, preserving earlier streams.
        if building.firing_super_weapon() != -1 {
            b"building-firing-super-weapon-5f8".hash(hasher);
            building.firing_super_weapon().hash(hasher);
        }
        // Only Superweapon Limit sets it; None adds no bytes.
        if let Some(cell) = building.own_nuke_target() {
            b"building-own-nuke-target-vera".hash(hasher);
            cell.hash(hasher);
        }
    }
    // Infantry already folds the same owned byte above. Native default0
    // retains the prior Unit/Aircraft hash stream; loaded nonzero Foot68D
    // can change a Foot handler and must contribute independently.
    if leaf.as_infantry().is_none() && leaf.foot_firing_sequence_latch() != 0 {
        b"foot-firing-sequence-v1".hash(hasher);
        leaf.foot_firing_sequence_latch().hash(hasher);
    }
    if leaf.foot_idle_entry_latch() != 0 {
        b"foot-idle-entry-6b3".hash(hasher);
        leaf.foot_idle_entry_latch().hash(hasher);
    }
}

/// One entity's components, in the order the state hash folds them.
/// `frame` is the binary frame the pending timers measure against.
/// `owed_visits` are the visits its sleep still owes it (`world::idle_sleep`).
pub(super) fn hash_entity_owed(
    entity: &GameEntity,
    frame: u32,
    owed_visits: u32,
    hasher: &mut StateHasher,
) {
    entity.stable_id().hash(hasher);
    if entity.production_start_price() != 0 {
        // Techno+300 survives factory release and can suppress FreeUnit at
        // Grand_Opening446AF2. Native ComputeCRC70C3C6 also consumes it.
        b"production-start-price".hash(hasher);
        entity.production_start_price().hash(hasher);
    }
    if entity.is_useless() {
        // Native Techno+3D0 affects a later Repair-depot sale (`0x0044C4BC`).
        b"techno-useless-v1".hash(hasher);
    }
    // Shared count presence and the raw Techno reload clock affect future
    // firing/reload, including a signed -1 reached from InitialAmmo=-2.
    b"techno-ammo-reload-v1".hash(hasher);
    entity.ammo().is_some().hash(hasher);
    if let Some(ammo) = entity.ammo() {
        if entity.category() == crate::map::entities::EntityCategory::Aircraft {
            b"aircraft-ammo-v170".hash(hasher);
            ammo.hash(hasher);
        } else {
            ammo.current().hash(hasher);
        }
    }
    entity.ammo_reload_timer().hash(hasher);
    // GSI-09.01: the `BuildingClass+0x6D0/+0x6D8` ProduceCash
    // timer and the `TechnoClass+0x1CC/+0x1D0` drain link pair.
    // All three drive future wallet writes, so a divergence here
    // desyncs credits; every object carries them.
    entity.produce_cash.timer().hash(hasher);
    // Ares' upgrade timers fold once any was started, so other buildings
    // hash as earlier schemas did.
    if entity.seldom.ares.cash_upgrade_timers != <[crate::sim::timer::CdTimer; 3]>::default() {
        b"ares-produce-cash-upgrades-v1".hash(hasher);
        entity.seldom.ares.cash_upgrade_timers.hash(hasher);
    }
    entity.drain.target().hash(hasher);
    entity.drain.draining_me().hash(hasher);
    // ParasiteClass (Foot+69C) of dogs and drones, the victim's
    // Foot+694/+698 links, and Foot+6A0 once it has been armed.
    // `limbo_reselect` (+432) is deliberately absent: it is set
    // only for the local player's selection, so peers differ.
    let parasite = &entity.parasite;
    if let Some(class) = parasite.class() {
        b"parasite-v193".hash(hasher);
        class.hash(hasher);
    }
    if parasite.eater().is_some() || parasite.launch_lock() != 0 {
        b"parasite-victim-v193".hash(hasher);
        parasite.eater().hash(hasher);
        parasite.launch_lock().hash(hasher);
    }
    if parasite.paralysis().duration() != 0 {
        0x6a0_u32.hash(hasher);
        parasite.paralysis().hash(hasher);
    }
    // Foot+5C4..+5D1, an attack-move order: its scan, resume and
    // pointer expiry (`sim::mission::mega_mission`).
    if entity.seldom.mega_mission != Default::default() {
        0x5c4_u32.hash(hasher);
        entity.seldom.mega_mission.hash(hasher);
    }
    if entity.building.has_been_captured.is_set() {
        // Building+6E3: doubles the survivor divisor, widens the
        // survivor roll and skips the engineer roll at death.
        0x6e3_u32.hash(hasher);
    }
    if entity.building.no_survivor.is_set() {
        // Building+6E0 (CRC `0x00454407`): no survivor leaves the building
        // that outlived its killing blow.
        0x6e0_u32.hash(hasher);
    }
    if entity.is_nominal_survivor() {
        //Infantry CRC521CE5..521CEE includes its retained6D9 byte.
        0x6d9_u32.hash(hasher);
    }
    if let Some(production) = entity.secret_lab_fallback() {
        b"building-secret-production-v382".hash(hasher);
        production.hash(hasher);
    }
    // Ares BuildingExt+0x50: only GenerateOnCapture rolls a placed lab again.
    if entity.ares_secret_lab_placed() {
        b"ares-secret-lab-placed-v1".hash(hasher);
    }
    if entity.object_group() != -1 {
        // gamemd.exe TechnoClass::ComputeCRC70C270 reads +214 at70C280.
        // Replay state includes it; native MP checksum64DAB0 is separate and
        // omits local control groups, as in the original executable. A
        // network game's local group slot is deliberately absent: peers differ.
        b"techno-object-group-v1".hash(hasher);
        entity.object_group().hash(hasher);
    }
    if entity.unit_follower().is_some() || entity.is_unit_follower() {
        // UnitClass::ComputeCRC744640 includes the +6C8 peer identity and
        // independent +6D0 byte at744678..7446AB (gamemd.exe instructions).
        b"unit-follower-v1".hash(hasher);
        entity.unit_follower().hash(hasher);
        entity.is_unit_follower().hash(hasher);
    }
    if entity.techno_ctor_random_word() != 0 {
        b"techno-constructor-v1".hash(hasher);
        entity.techno_ctor_random_word().hash(hasher);
        // Preserve the previous no-upgrade hash stream.
        Option::<u8>::None.hash(hasher);
    }
    if entity.building_upgrades().count() != 0
        || entity
            .building_upgrades()
            .types()
            .iter()
            .any(Option::is_some)
    {
        b"building-installed-upgrades-v458".hash(hasher);
        entity.building_upgrades().hash(hasher);
    }
    // One retained native+F8/+FC/+100/+108/+10C/+110 clock, shared
    // by Infantry, harvest/unload and Building animation receivers.
    b"techno-stage-v264".hash(hasher);
    entity.native_stage().hash(hasher);
    entity.native_crush_immunity().hash(hasher);
    b"building-body-v277".hash(hasher);
    entity.hash_building_body(hasher);
    entity.hash_building_health_sample(hasher);
    if let Some(pending) = entity.pending_entry() {
        b"foot-pending-entry-500".hash(hasher);
        pending.hash(hasher);
    }
    if entity.seldom.building_entry.seeking() {
        b"foot-building-entry-68f-690".hash(hasher);
        entity.seldom.building_entry.hash(hasher);
    }
    let latches = entity.house_tracking_latches();
    #[cfg(feature = "phobos")]
    {
        b"phobos-techno-placement-v1".hash(hasher);
        entity.seldom.phobos_placement.hash(hasher);
        if let Some(shield) = &entity.seldom.shield {
            b"phobos-shield-v1".hash(hasher);
            shield.hash(hasher);
        }
        entity
            .seldom
            .wh_anim_remaining_creation_interval
            .hash_state(hasher);
    }
    if entity.tracking_facts().naval_combatant() {
        // Retained across reload: later Remove_Tracking must use the same
        // Naval/Passengers admission as construction, even while in limbo.
        b"naval-tracking-eligibility-v1".hash(hasher);
    }
    entity.tracking_facts().hash_retained_inputs(hasher);
    if let Some(multiplier) = entity.seldom.ares.factory_plant_multiplier {
        b"ares-plant-multiplier-v1".hash(hasher);
        multiplier.hash(hasher);
    }
    if latches != Default::default() {
        b"house-tracking-infantry-latches-v1".hash(hasher);
        latches.hash(hasher);
    }
    entity.native_unique_id().hash(hasher);
    if let Some(manager) = entity.slave_manager.as_ref() {
        b"slave-manager-v209".hash(hasher);
        manager.hash(hasher);
    }
    if entity.seldom.slave.owner().is_some() || !entity.seldom.slave.cargo().is_empty() {
        b"slave-v209".hash(hasher);
        entity.seldom.slave.owner().hash(hasher);
        entity.seldom.slave.cargo().len().hash(hasher);
        for bale in entity.seldom.slave.cargo() {
            bale.tiberium_type.0.hash(hasher);
            (bale.resource_type as u8).hash(hasher);
            bale.value.hash(hasher);
        }
    }
    entity.air_spatial_bucket().hash(hasher);
    entity.air_spatial_enter_order().hash(hasher);
    // Foot+560/+564 are independent retained native Cells. Constructor
    // NativeNull contributes no bytes, like the other sparse native
    // fields; either non-null cache independently affects continuation.
    if entity.air_tracker_cell() != (0, 0) {
        b"foot-air-tracker-cell-v289".hash(hasher);
        entity.air_tracker_cell().hash(hasher);
    }
    if entity.air_slot_cell() != (0, 0) {
        b"foot-air-slot-cell-v289".hash(hasher);
        entity.air_slot_cell().hash(hasher);
    }
    // Independent lifecycle axes and deterministic Rust bookkeeping.
    // Keep this order fixed: it is part of the lockstep hash contract.
    entity.lifecycle.object_alive().hash(hasher);
    entity.lifecycle.in_limbo().hash(hasher);
    entity.lifecycle.cell_marked().hash(hasher);
    entity.death.is_dying().hash(hasher);
    entity
        .seldom
        .removal_facts
        .dirty_rect_eligible()
        .hash(hasher);
    entity.seldom.kill_record.is_recorded().hash(hasher);
    // TechnoClass+0x3D5 is mutable admission state, not a derived
    // position query: ordinary movement is promote-only while
    // teleport and Set_Clipped_LocalSize own exact demotions.
    entity.in_playfield().hash(hasher);
    entity.move_sound.is_active().hash(hasher);
    entity.crash.is_crashing().hash(hasher);
    entity.crash.seen().hash(hasher);
    if !entity.sinking.is_default() {
        b"techno-sinking-v1".hash(hasher);
        entity.sinking.hash(hasher);
    }
    entity.is_mission_only().hash(hasher);
    if entity.seldom.on_carryall.get() {
        b"foot-on-carryall-v1".hash(hasher);
    }
    entity.move_sound.countdown().hash(hasher);
    entity.position.rx().hash(hasher);
    entity.position.ry().hash(hasher);
    entity.position.z().hash(hasher);
    // Most objects have no exact coordinate-Z override. Preserve their
    // pre-v62 hash stream while making TubeMovement's signed lepton Z
    // authoritative whenever it is present.
    if let Some(exact_z_leptons) = entity.position.exact_z_leptons() {
        b"exact-object-z-leptons-v1".hash(hasher);
        exact_z_leptons.hash(hasher);
    }
    entity.position.sub_x().hash(hasher);
    entity.position.sub_y().hash(hasher);
    // The body FacingClass (`+0x388`), each Techno's one heading.
    entity.body_facing.hash(hasher);
    // The barrel elevation (`+0x370`), tagged, once an Unlimbo moved
    // it off its constructor value.
    if !entity.barrel_elevation_is_constructed() {
        b"barrel-elevation-v1".hash(hasher);
        entity.barrel_elevation().hash(hasher);
    }
    entity.body_frame_counter.get().hash(hasher);
    if entity.unit_firing_counter.get() != -1 {
        b"unit-firing-counter-v1".hash(hasher);
        entity.unit_firing_counter.hash(hasher);
    }
    if entity.unit_death_counter.running() {
        b"unit-death-counter-v1".hash(hasher);
        entity.unit_death_counter.hash(hasher);
    }
    if !entity.building.tesla_charge.is_default() {
        b"building-tesla-charge-v1".hash(hasher);
        entity.building.tesla_charge.hash(hasher);
    }
    if entity.seldom.tesla_charger.is_active() {
        b"foot-tesla-charger-v1".hash(hasher);
    }
    // Building+6E6 is retained transition state, independent of HP.
    if entity.building_art.wall_tower_variant() != -1 {
        b"building-wall-tower-variant-v1".hash(hasher);
        entity.building_art.wall_tower_variant().hash(hasher);
    }
    if entity.building_art.damage_state_active() {
        b"building-damage-state-v1".hash(hasher);
    }
    if entity.building_art.actually_placed()
        || entity.building_art.anim_slots().iter().any(Option::is_some)
        || (entity.building_art.anim_effect_replay().iter().any(|v| *v))
    {
        b"building-anim-slots-v1".hash(hasher);
        entity.building_art.actually_placed().hash(hasher);
        entity.building_art.anim_slots().hash(hasher);
        entity.building_art.anim_effect_replay().hash(hasher);
    }
    if *entity.building_art.storage() != Default::default() {
        b"building-storage-v1".hash(hasher);
        entity.building_art.storage().hash(hasher);
    }
    if entity.building.building_last_operational.is_operational() {
        b"building-operational-v1".hash(hasher);
    }
    if !entity.building_art.stuff_enabled() {
        b"building-stuff-disabled-v1".hash(hasher);
    }
    if entity.category() == crate::map::entities::EntityCategory::Structure
        && !entity.building_online()
    {
        b"building-online-latch-off-v1".hash(hasher);
    }
    if entity.building.building_online.ares_toggled_off() {
        b"ares-building-toggled-off-v1".hash(hasher);
    }
    if entity.building_art.free_units_done() {
        b"ares-building-free-units-done-v1".hash(hasher);
    }
    if entity.building.building_has_engineer.is_set() {
        b"building-has-engineer-v1".hash(hasher);
    }
    // Native Building ComputeCRC4542E0 includes the retained +53C value,
    // not merely the -1 loss gate. Keep the constructor-zero stream stable.
    if entity.category() == crate::map::entities::EntityCategory::Structure
        && entity.building_loss_index() != 0
    {
        b"building-loss-index-v1".hash(hasher);
        entity.building_loss_index().hash(hasher);
    }
    if !entity.unit_delivery_provides_base_space() {
        b"unit-delivery-no-base-v1".hash(hasher);
    }
    if entity.ares_chronoshifting() {
        b"ares-building-chronoshift-v1".hash(hasher);
    }
    if let Some(origin) = entity.ares_super_weapon() {
        b"hunter-seeker-source-v1".hash(hasher);
        origin.hash(hasher);
    }
    if let Some(target) = entity.ares_super_target() {
        b"ares-super-target-v1".hash(hasher);
        target.hash(hasher);
    }
    if let Some(pod) = entity.ares_drop_pod() {
        b"ares-drop-pod-v1".hash(hasher);
        pod.hash(hasher);
    }
    if let Some(alt) = entity.ares_alt_occupation() {
        b"ares-alt-occupation-v1".hash(hasher);
        alt.hash(hasher);
    }
    // Building+662 and Techno+1C8 (`sim::powered_unit`), folded only
    // while set.
    if entity.building.building_powered_center.is_counted() {
        b"building-powered-center-v1".hash(hasher);
    }
    if entity.deactivated.is_offline() {
        b"techno-deactivated-v1".hash(hasher);
    }
    entity.hash_emp_state(hasher);
    if let Some(source) = entity.seldom.ares.powered_by {
        b"ares-powered-by-v1".hash(hasher);
        source.hash(hasher);
    }
    if !entity.seldom.ares.radar_jam.is_default() {
        b"ares-radar-jam-v1".hash(hasher);
        entity.seldom.ares.radar_jam.hash(hasher);
    }
    if !entity.seldom.ares.sensor_array_counter.is_zero() {
        b"ares-sensor-array-counter-v1".hash(hasher);
        entity.seldom.ares.sensor_array_counter.hash(hasher);
    }
    if !entity.seldom.ares.ares_dock_reload_timers.is_empty() {
        b"ares-dock-reload-timers-v1".hash(hasher);
        entity.seldom.ares.ares_dock_reload_timers.hash(hasher);
    }
    if !entity.seldom.ares.ares_firestorm_wall.is_zero() {
        b"ares-firestorm-wall-frame-v1".hash(hasher);
        entity.seldom.ares.ares_firestorm_wall.hash(hasher);
    }
    if entity.seldom.ares.sonar_timer != crate::sim::timer::CdTimer::default() {
        b"ares-sonar-timer-v1".hash(hasher);
        entity.seldom.ares.sonar_timer.hash(hasher);
    }
    if entity.seldom.ares.ares_disable_weapons != crate::sim::timer::CdTimer::default() {
        b"ares-disable-weapons-timer-v1".hash(hasher);
        entity.seldom.ares.ares_disable_weapons.hash(hasher);
    }
    if entity.seldom.ares.ares_self_heal_combat != crate::sim::timer::CdTimer::default() {
        b"ares-self-heal-combat-timer-v1".hash(hasher);
        entity.seldom.ares.ares_self_heal_combat.hash(hasher);
    }
    entity.hash_attach_effects(hasher);
    #[cfg(feature = "phobos")]
    entity.hash_phobos_attach_effects(hasher);
    // Infantry gameplay reads its class-owned Doing/Stage fields.
    // A restored presentation component is not a second state owner.
    if entity.category() != crate::map::entities::EntityCategory::Infantry
        && let Some(animation) = entity.animation.as_ref()
    {
        b"entity-animation-v1".hash(hasher);
        animation.sequence().hash(hasher);
        animation.frame_index().hash(hasher);
        animation.elapsed_frames().hash(hasher);
        animation.finished().hash(hasher);
    }
    entity.owner().hash(hasher);
    if let Some(country) = entity.original_factory_country() {
        b"ares-original-factory-country-v1".hash(hasher);
        country.hash(hasher);
    }
    if entity.ares_wave_weapon_slot() != 0 {
        b"ares-wave-weapon-slot-v1".hash(hasher);
        entity.ares_wave_weapon_slot().hash(hasher);
    }
    // Techno+14C survives ChangeOwner and controls an AI building capture's
    // automatic sale. Preserve the untouched-owner stream, but distinguish
    // objects transferred from different constructor houses.
    if entity.initial_owner() != entity.owner() {
        b"techno-initial-owner-v1".hash(hasher);
        entity.initial_owner().hash(hasher);
    }
    entity.health.current().hash(hasher);
    entity.estimated_health.get().hash(hasher);
    entity.type_ref().hash(hasher);
    (entity.category() as u8).hash(hasher);
    // Keep the stock name encoding used before Foundation became an enum.
    // Custom values still fold their full dimensions, cells and exit outline.
    let foundation = entity.seldom.type_facts.foundation();
    match foundation {
        crate::rules::foundation::Foundation::Stock(_) => foundation.name().hash(hasher),
        _ => foundation.hash(hasher),
    }
    entity
        .seldom
        .type_facts
        .building_hidden_occupancy()
        .hash(hasher);
    entity
        .seldom
        .type_facts
        .base_reservation_spacing()
        .hash(hasher);
    if entity.seldom.type_facts.build_const_eligible() {
        b"naval-build-const-entity-v1".hash(hasher);
        entity.seldom.type_facts.build_const_eligible().hash(hasher);
    }
    entity.seldom.type_facts.base_plan_type_index().hash(hasher);
    entity.seldom.type_facts.base_plan_is_defense().hash(hasher);
    entity
        .seldom
        .type_facts
        .base_plan_has_undeploy_target()
        .hash(hasher);
    // The rank is sampled from the raw accumulator; its fold keeps its
    // place in the stream. The accumulator itself is folded too, so
    // two objects one kill apart inside the same rank are distinct.
    entity.veterancy.rank().hash(hasher);
    entity.veterancy.raw().bits().hash(hasher);
    entity.veterancy.rank_cache().hash(hasher);
    // The flash countdown (+F0) is not folded: TechnoClass::Flash's gate and
    // the MegaMission event's flash read the current player, so lockstep
    // peers hold different values, and no gameplay decision reads it.
    entity
        .seldom
        .crate_multipliers
        .crate_armor()
        .bits()
        .hash(hasher);
    if entity.armor_multiplier() != entity.seldom.crate_multipliers.crate_armor() {
        b"attached-armor-v1".hash(hasher);
        entity.armor_multiplier().bits().hash(hasher);
    }
    // Folded only once a Firepower crate changed it, so the legacy
    // hash stream of every untouched object is preserved.
    if entity.seldom.crate_multipliers.crate_firepower()
        != crate::util::native_x87::NativeF64Bits::ONE
    {
        b"firepower-mult-v1".hash(hasher);
        entity
            .seldom
            .crate_multipliers
            .crate_firepower()
            .bits()
            .hash(hasher);
    }
    if entity.firepower_multiplier() != entity.seldom.crate_multipliers.crate_firepower() {
        b"attached-firepower-v1".hash(hasher);
        entity.firepower_multiplier().bits().hash(hasher);
    }
    entity.berserk.hash(hasher);
    entity
        .seldom
        .ai_sell_repair
        .was_attacked_by_enemy()
        .hash(hasher);
    if entity.category() == crate::map::entities::EntityCategory::Structure {
        entity.seldom.ai_sell_repair.ai_sellable().hash(hasher);
    }
    if entity.category() == crate::map::entities::EntityCategory::Structure {
        entity.building.repairing.is_on().hash(hasher);
        entity.seldom.ai_sell_repair.ai_repairable().hash(hasher);
    }
    let ai_placement_timer = entity.building.ai_placement_timer.timer();
    if ai_placement_timer != crate::sim::timer::CdTimer::default() {
        b"ai-placement-timer-v1".hash(hasher);
        ai_placement_timer.hash(hasher);
    }
    b"base-defense-response-v1".hash(hasher);
    entity.seldom.base_defense_response.hash(hasher);
    entity.mover_facts.regular_crusher().hash(hasher);
    entity.mover_facts.drive_accelerates().hash(hasher);
    entity.damage_fires.is_active().hash(hasher);
    entity.damage_fires.anim_ids().hash(hasher);
    entity.seldom.sight.range().hash(hasher);
    entity.seldom.sight_refresh_timers.hash(hasher);
    if entity.building.gap_generator != crate::sim::world::GapGeneratorRuntime::default() {
        b"gap-operational-v144".hash(hasher);
        entity.building.gap_generator.hash(hasher);
    }

    {
        // Preserve schema160's original four-field order. Deriving
        // the fold from the expanded struct would alter every prior
        // projection, including constructor-default Foot states.
        let path = entity.navigation.path_runtime();
        path.movement_timer().hash(hasher);
        path.blocked_timer().hash(hasher);
        path.path_blocked().hash(hasher);
        path.retries_left().hash(hasher);
        if path.scold_latch_raw() != 0 {
            0x68a_u32.hash(hasher);
            path.scold_latch_raw().hash(hasher);
        }
    }
    // Foot+530 is retained across type changes and saved restores.
    // Preserve the constructor-zero historical projection; nonzero
    // values (including negative zero) change future path decisions.
    let coefficient = entity.navigation.path_threat_coefficient();
    if coefficient.bits() != 0 {
        0x530_u32.hash(hasher);
        coefficient.hash(hasher);
    }
    // Techno+508 is a retained contribution, not recomputed live
    // ThreatPosed. Some(0) differs from constructor-uninitialized None:
    // a later removal must not read an invented constructor value.
    if let Some(value) = entity.cached_spatial_threat() {
        b"techno-spatial-threat-v1".hash(hasher);
        value.hash(hasher);
    }
    entity.navigation.neighbor_state().hash(hasher);
    entity.navigation.path_replay().hash(hasher);
    entity.navigation.nav_com_aux().hash(hasher);
    entity.navigation.nav_com().hash(hasher);
    entity.navigation.suspended_nav_com().hash(hasher);
    entity.navigation.nav_queue().hash(hasher);
    // Foot+5AC/+6B1 hash only when used, so established goldens without a
    // navigation list keep their values.
    if !entity.navigation.nav_list().is_empty() || entity.navigation.nav_list_loops() {
        0x5ac_u32.hash(hasher);
        entity.navigation.nav_list().hash(hasher);
        entity.navigation.nav_list_loops().hash(hasher);
    }
    entity.navigation.pending_destination_setter().hash(hasher);

    entity.foot_speed.native_applied_fraction().hash(hasher);
    if entity.foot_speed.crush_slowdown() {
        0x6b5_u32.hash(hasher);
        true.hash(hasher);
    }
    if entity.flight_attitude != Default::default() {
        0x2e8_u32.hash(hasher);
        entity.flight_attitude.hash(hasher);
    }
    entity.foot_speed.crate_multiplier().hash(hasher);
    entity.foot_occupation_enabled().hash(hasher);
    entity.lift.swap_active().hash(hasher);
    // The Magnetron's links and drop (Techno+2AC/+2B0/+427/+428) and
    // the warp or drop credit house (Techno+42C) are clear outside a
    // lift or a warp; Foot+6AE stays up after any Jumpjet touchdown
    // until an END clears it. Tagged like +0x1F8 below.
    if entity.lift.may_end() {
        0x6ae_u32.hash(hasher);
    }
    if let Some(target) = entity.lift.target() {
        0x2ac_u32.hash(hasher);
        target.hash(hasher);
    }
    if let Some(source) = entity.lift.source() {
        0x2b0_u32.hash(hasher);
        source.hash(hasher);
    }
    if entity.seldom.drop_credit.being_manipulated() {
        0x427_u32.hash(hasher);
    }
    if let Some(firer) = entity.seldom.drop_credit.manipulated_by() {
        0x428_u32.hash(hasher);
        firer.hash(hasher);
    }
    if let Some(house) = entity.seldom.drop_credit.warped_by_house() {
        0x42c_u32.hash(hasher);
        house.hash(hasher);
    }
    // Techno+0x1F8 is up only between the Teleporter arm's can't-end
    // branch and the next Unit setter call; the tag keeps every
    // established stream unchanged while it is clear.
    if entity.seldom.setter_force_reassign.is_raised() {
        0x1f8_u32.hash(hasher);
    }
    // Techno+0x284 stays zero until a blocked Chronosphere landing
    // writes it; tagged like +0x1F8.
    if entity.chrono_warp_delay() != 0 {
        0x284_u32.hash(hasher);
        entity.chrono_warp_delay().hash(hasher);
    }

    if let Some(ref loco) = entity.locomotor {
        1u8.hash(hasher);
        hash_locomotor(loco, hasher);
        // Mission readiness inputs are NOT hashed: they are derived at
        // the gate from state already hashed above (and from position and
        // facing, likewise hashed). Hashing a derived predicate would pin a
        // projection of the same state twice, and it cannot diverge
        // independently of its inputs.
    } else {
        0u8.hash(hasher);
    }

    entity.on_bridge().hash(hasher);
    entity
        .seldom
        .runtime_bridge_transition
        .pending_mismatch()
        .hash(hasher);
    entity
        .building
        .building_light
        .spotlight_capable()
        .hash(hasher);
    entity.building.building_light.light().hash(hasher);
    entity.low_bridge_tube_state.hash(hasher);
    entity.stealth_ability.granted().hash(hasher);
    // The Ares attached cloak, folded only while set, so a stock game's
    // stream stays as it was.
    if entity.cloak_attached() {
        b"ares-attached-stealth-v1".hash(hasher);
    }
    if let Some(cloak) = entity.cloak.as_ref() {
        1u8.hash(hasher);
        cloak.state().hash(hasher);
        cloak.depth().hash(hasher);
        cloak.step_delta().hash(hasher);
        cloak.step_timer().timer().start_frame().hash(hasher);
        cloak.step_timer().speed().hash(hasher);
        cloak.step_timer().timer().duration().hash(hasher);
        cloak.recloak_delay().start_frame().hash(hasher);
        cloak.recloak_delay().duration().hash(hasher);
        cloak.building_stage().hash(hasher);
    } else {
        0u8.hash(hasher);
    }
    if let Some(disguise) = entity.seldom.disguise.as_ref() {
        1u8.hash(hasher);
        disguise.hash_state(hasher);
    } else {
        0u8.hash(hasher);
    }

    if let Some(ref inv) = entity.invulnerability {
        1u8.hash(hasher);
        inv.hash_state(hasher);
    } else {
        0u8.hash(hasher);
    }

    if let Some(target) = entity.attack_target.target() {
        1u8.hash(hasher);

        target.hash(hasher);
    } else {
        0u8.hash(hasher);
    }
    entity.seldom.rearm_timer.start_frame().hash(hasher);
    entity.seldom.rearm_timer.duration().hash(hasher);
    entity.seldom.weapon_burst.hash(hasher);
    entity.building.pending_building_fire.pending().hash(hasher);
    if entity.building.prism_support_count.count() != 0 {
        b"prism-support-count-v1".hash(hasher);
        entity.building.prism_support_count.count().hash(hasher);
    }
    entity.seldom.ares.ares_prism.hash_state(hasher);
    // One native weapon owner replaces last-shot/transport copies.
    // The turret index and saved charge duration only drive drawing.
    entity.current_weapon_number().hash(hasher);

    // Slot-indexed fold: capacity + each slot's Option (null holes and
    // pad positions are hash-relevant). Replaces the old len + ordered-id
    // fold — an intended one-time re-baseline at this behavior boundary.
    entity.seldom.radio_contacts.hash_fold(hasher);
    // Dock-entered flag (+0x418 analogue). Intended one-time re-baseline
    // at this behavior boundary alongside the slot-folded contacts.
    match entity.seldom.tether.partner() {
        Some(sid) => {
            1u8.hash(hasher);
            sid.hash(hasher);
        }
        None => 0u8.hash(hasher),
    }

    entity.hash_capture_infantry_type(hasher);
    match entity.building.pending_c4_detonation.pending() {
        Some(pending) => {
            true.hash(hasher);
            pending.timer().remaining(frame as i32).hash(hasher);
            pending.source_entity_id().hash(hasher);
        }
        None => false.hash(hasher),
    }
    entity.seldom.ares.display_production_to.hash_state(hasher);
    entity.seldom.idle_wander.hash_state(hasher);
    entity.seldom.ares.ares_raid.hash_state(hasher);
    entity
        .seldom
        .ares
        .ares_initial_payload
        .hash_state(entity.category(), hasher);
    entity.bunker.occupant().hash(hasher);
    // Reciprocal link + install machine are authoritative lifecycle state.
    entity.bunker.link().hash(hasher);
    entity.building.bunker_runtime.hash(hasher);
    if let Some(gate) = entity.building.building_gate {
        1u8.hash(hasher);
        gate.hash_state(hasher);
    } else {
        0u8.hash(hasher);
    }
    // Shared Techno Door gameplay state is retained and folded once.
    entity.hash_door_state(hasher);

    // Unit+68C remains the deployment authority. The former MCV-only
    // turn observation now hashes once in its retained locomotor class.
    if entity.seldom.mcv_deploy_pending.is_pending() {
        0x4d435644u32.hash(hasher);
        entity.seldom.mcv_deploy_pending.is_pending().hash(hasher);
    }
    entity.hash_deploy_attachment(hasher);
    entity.hash_behind_anim(hasher);

    if let Some(infantry) = entity.infantry {
        1u8.hash(hasher);
        // Infantry5220F0's retained6D8 changes target admission. Preserve
        // established streams until this separate latch actually rises.
        if infantry.is_psycho() {
            0x36D8_u32.hash(hasher);
        }
        infantry.fear_level().hash(hasher);
        infantry.is_prone().hash(hasher);
        // The idle-fidget countdown gates a scenario-RNG draw, so a
        // divergence here becomes a divergence of every later draw.
        // Folded in every schema variant rather than behind a gate: the
        // Scenario RNG itself is folded before either gate, and the two
        // provenance fixtures both hold infantry eligible on their first
        // tick, so their legacy probes already move with the draws this
        // timer schedules. Gating it would hide the field without
        // buying those probes back.
        infantry.idle_action_timer().hash(hasher);
        // Infantry+6DC is written only by the failed-path receiver
        // 0x51DAF0; the tagged extension keeps every established
        // stream unchanged until a receiver actually stores 1.
        if infantry.cell_entry_blocked() {
            0x36DC_u32.hash(hasher);
        }
    } else {
        0u8.hash(hasher);
    }

    if let Some(ref miner) = entity.miner {
        1u8.hash(hasher);
        // The FSM cursor retired from this block at the substate-
        // authority flip: it is MissionCom.handler_state, folded by
        // `hash_mission_com` (and the pre-v29 reconstruction).
        (miner.kind() as u8).hash(hasher);
        (miner.cargo().len() as u16).hash(hasher);
        for bale in miner.cargo() {
            bale.tiberium_type.0.hash(hasher);
            (bale.resource_type as u8).hash(hasher);
            bale.value.hash(hasher);
        }
        let retired_dock_fold = false;
        if retired_dock_fold {
            // home_refinery: None.
            None::<u64>.hash(hasher);
        }
        // The retired return-order refinery; nothing writes it.
        miner.reserved_refinery().hash(hasher);
        let native_ore_field = true;
        if !native_ore_field {
            // The retired target_ore_cell (None) and harvest_timer
            // (unarmed).
            None::<(u16, u16)>.hash(hasher);
            crate::sim::mission::MissionTimer::default().hash(hasher);
        }
        if native_ore_field {
            // Unit+0x6D1/+0x6D2; the shared Stage is folded above.
            miner.unload_active().hash(hasher);
            miner.harvesting().hash(hasher);
        }
        if retired_dock_fold {
            // dock_queued false, dock_phase Approach (discriminant 0),
            // dock_pivot_facing None.
            false.hash(hasher);
            0_isize.hash(hasher);
            None::<crate::sim::movement::FacingClass>.hash(hasher);
        }
    } else {
        0u8.hash(hasher);
    }

    // Passenger/transport state.
    let role = entity.passenger_role();
    match role.cargo() {
        Some(cargo) => {
            1u8.hash(hasher);
            cargo.capacity().hash(hasher);
            (cargo.passengers().len() as u32).hash(hasher);
            for &pid in cargo.passengers() {
                pid.hash(hasher);
            }
            debug_assert_eq!(cargo.passenger_sizes().len(), cargo.passengers().len());
            for &passenger_size in cargo.passenger_sizes() {
                passenger_size.to_bits().hash(hasher);
            }
            cargo.total_size().hash(hasher);
            // A Foot can own cargo while riding another transport (Unit
            // PerCell 0x0073A730..0x0073A77C). Only append a live parent
            // relation, preserving ordinary outside-cargo hash controls.
            if let Some(link) = role.carrier_link() {
                3u8.hash(hasher);
                link.transport_id().hash(hasher);
                link.open_topped().hash(hasher);
                // `+0x11C`, folded only when set (see the arm below).
                if role.transporter() == Some(link.transport_id()) {
                    4u8.hash(hasher);
                }
            }
            // `UnitClass+0x6E4` keep-count, written only by a
            // transport's own Unload state 0. Folded only when set so
            // objects that never unloaded (every pinned harness
            // object) keep their v-schema bytes.
            if entity.seldom.transport_unload_keep_count.count() != 0 {
                entity
                    .seldom
                    .transport_unload_keep_count
                    .count()
                    .hash(hasher);
            }
        }
        None => {
            if let Some(link) = role.carrier_link() {
                3u8.hash(hasher);
                link.transport_id().hash(hasher);
                // `+0x82` and `+0x11C`, folded only when set so cargo that
                // CargoClass::AddPassenger loaded keeps its historical hash.
                if link.open_topped() {
                    1u8.hash(hasher);
                }
                if role.transporter() == Some(link.transport_id()) {
                    4u8.hash(hasher);
                }
            } else {
                0u8.hash(hasher);
            }
        }
    }
    // `+0x11C` naming another hull than the carrier relation, or none: a
    // soldier an aircraft unloaded keeps it (`PassengerRole::transporter`).
    if let Some(transporter) = role.transporter()
        && role.carrier_link().map(|link| link.transport_id()) != Some(transporter)
    {
        5u8.hash(hasher);
        transporter.hash(hasher);
    }
    // BuildingCtor43B8DC..43B907 and GetCount4581F0: independent vector,
    // not Cargo+114. The unique empty/zero constructor state adds no bytes,
    // preserving unrelated replay controls. A nonzero signed +69C still
    // contributes even when pointer expiry has emptied the vector.
    let occupants = role.occupants();
    if entity.category() == crate::map::entities::EntityCategory::Structure
        && (!occupants.is_empty() || occupants.fire_index() != 0)
    {
        b"building-occupants-694-69c".hash(hasher);
        occupants.count().hash(hasher);
        occupants.ids().hash(hasher);
        occupants.fire_index().hash(hasher);
    }
    // Spawn-manager pool: slot states, timers and targets are
    // deterministic sim state that no other field covers. (Native
    // folds only the manager-level fields into its CRC and leaves the
    // per-slot machine uncovered; VERA folds the whole thing, which is
    // strictly stricter and cannot mask a divergence.)
    //
    // Deliberately folded ONLY when present — no absent-case tag byte.
    // Every object in the game carries these two fields, so an
    // unconditional tag would move every committed baseline, including
    // the legacy provenance probes, for fixtures that contain no
    // spawner unit at all.
    //
    // Honest limitation: this block is not self-delimiting. The leading
    // 1u8/2u8 tags separate the two fields from each other, but nothing
    // in this hasher marks where one entity's contribution ends, so an
    // omitted-field encoding is not provably distinct from some other
    // field's bytes further along the stream. That is a property of the
    // whole per-entity hasher, not of this block; every neighbouring
    // conditional field has it too. It is not reachable here — a live
    // pool always folds a tag plus its spawn type and mode — but the
    // invariant is "no known aliasing", not "aliasing is impossible".
    entity.seldom.airstrike.hash_state(hasher);
    entity.airstrike_tint.hash_state(hasher);
    if entity.seldom.type_facts.spawned_non_missile_aircraft() {
        b"spawned-non-missile-aircraft".hash(hasher);
    }
    if let Some(ref manager) = entity.spawn_manager {
        1u8.hash(hasher);
        manager.hash(hasher);
    }
    if let Some(owner_id) = entity.seldom.spawn_owner_id.owner() {
        2u8.hash(hasher);
        owner_id.hash(hasher);
    }
    // CaptureManager capacity and MCNode order gate future fire/mission
    // decisions. As with SpawnManager, absent managers add no bytes so
    // worlds without a mind-control controller keep legacy hashes.
    if let Some(ref manager) = entity.capture_manager {
        3u8.hash(hasher);
        manager.hash(hasher);
    }
    if entity.seldom.mind_control != crate::sim::capture_manager::MindControlLink::default() {
        // The victim's MindControlledBy (+2C0) and ring (+2C8).
        0x2c0_u32.hash(hasher);
        entity.seldom.mind_control.hash(hasher);
    }
    // TemporalImUsing (+274) and TemporalTargetingMe (+278). Objects
    // with neither fold nothing, so worlds without a Temporal firer
    // keep their hashes.
    if entity.temporal != crate::sim::temporal::TemporalState::default() {
        0x274_u32.hash(hasher);
        entity.temporal.hash(hasher);
    }
    // A carried bomb (+38); an unbombed object folds nothing.
    if let Some(bomb) = &entity.bomb {
        0x38_u32.hash(hasher);
        bomb.hash(hasher);
    }
    // Gattling stage, value and report latch (+140, +144, +4B8); an
    // object that never spun folds nothing.
    if entity.gattling != crate::sim::combat::gattling::GattlingState::default() {
        0x140_u32.hash(hasher);
        entity.gattling.hash(hasher);
    }
    // Turret rotation-sound latch (`+0x49C`): unarmed suppresses another
    // playback request until idle, even if no source exists. No other
    // folded field reproduces that history. The armed default folds
    // nothing, so a building that never turned keeps its prior hash stream.
    if !entity.turret_rotate_latch.is_armed() {
        b"turret-rotate-latch-v1".hash(hasher);
    }
    // Techno+4F8, the deploy-voice suppression, is left out: only voice
    // playback reads it, and a deploy click clears it on the clicking
    // client alone (ClickedMission -> VoiceDeploy708E00).
    // Retired slot: the entity homing model had no production creator
    // and is gone; its constant absent tag keeps pinned hashes stable.
    0u8.hash(hasher);
    // Turret facing (`+0x3A0`) — Hash-derived, all primitive fields
    // contribute.
    if let Some(barrel) = entity.barrel_facing.as_ref() {
        1u8.hash(hasher);
        barrel.hash(hasher);
    } else {
        0u8.hash(hasher);
    }

    // Body rocking's four authoritative binary32 fields. A sub-fixed-quantum
    // rate can select a different damping branch on the next visit, so the
    // presentation's fixed getters cannot stand in for retained state.
    // Drive/Ship slope state is hashed with its locomotor payload below.
    if let Some(ref r) = entity.rocking {
        1u8.hash(hasher);
        r.native_bits().hash(hasher);
    } else {
        0u8.hash(hasher);
    }

    hash_mission_com(&entity.mission, owed_visits, hasher);
    hash_mission_leaf(&entity.mission_leaf, hasher);
    if let Some(post) = entity.seldom.patrol_post.cell() {
        b"foot-patrol-post-v1".hash(hasher);
        post.hash(hasher);
    }
    entity.seldom.removal_facts.occupier().hash(hasher);
    entity.passive_scan.timer().hash(hasher);
    // Passive-acquire bookkeeping. `passively_acquired_target` gates
    // the stale-target drop and the off-mission clear, so a
    // divergence here changes future targets; the scan-frame stamp
    // rides along in the same block.
    entity.passive_scan.last_scan_frame().hash(hasher);
    entity.passive_scan.acquired().hash(hasher);
    entity.hash_distributed_fire(hasher);
    // Foot688 changes the next scan's topology/radius. Constructor
    // false preserves earlier streams; the retained true state is an
    // independent schema255 contribution, never inferred from TarCom.
    if entity.foot_retarget_after_stop() {
        0x004D_9920_u32.hash(hasher);
        true.hash(hasher);
    }
    match entity.seldom.suspended_attack_target.target() {
        Some(target) => {
            1u8.hash(hasher);
            target.hash(hasher);
        }
        None => 0u8.hash(hasher),
    }
    if let Some(anim) = entity.parachute_animation() {
        b"parachute-animation-v1".hash(hasher);
        anim.hash(hasher);
    }
    // Preserve the idle hash while retaining every nonzero fall word.
    // Landing retains the rate/+84/+8F for a later DropAsBomb.
    if entity.is_falling_down()
        || entity.fall_rate() != 0
        || entity.is_parachuted()
        || entity.was_dropped_as_bomb()
    {
        1u8.hash(hasher);
        entity.is_falling_down().hash(hasher);
        entity.fall_rate().hash(hasher);
        entity.is_parachuted().hash(hasher);
        entity.was_dropped_as_bomb().hash(hasher);
    } else {
        0u8.hash(hasher);
    }

    // Preserve the constructor-NULL representation while hashing the actual
    // retained system identity, which gates FireAt/GetROF and future AI draws.
    entity
        .seldom
        .spark_particle_system
        .system()
        .unwrap_or(0)
        .hash(hasher);
    // ReceiveDamage damage-Smoke `+0x310` identity. This gates later
    // spawn/RNG and remains set while a marked system drains.
    entity.seldom.damage_smoke_system_id.system().hash(hasher);
    // The Railgun `+0x314` link gates the next railgun shot. Hashed only
    // when set, so objects that never fired a railgun hash as before.
    if let Some(system) = entity.seldom.railgun_particle_system.system() {
        system.hash(hasher);
    }
    // The Fire `+0x304` link gates the next fire shot, hashed the same way.
    if let Some(system) = entity.seldom.fire_particle_system.system() {
        (0x304_u16, system).hash(hasher);
    }
    // A Building's natural `+0x30C` link gates the cloak fade-in's rebuild
    // and its destroy-smoke `+0x320` link gates that pick; both hashed only
    // when set, as no retail type sets either key.
    if let Some(system) = entity.building.natural_particle_system.system() {
        (0x30C_u16, system).hash(hasher);
    }
    if let Some(system) = entity.building.destroy_smoke_particle_system.system() {
        (0x320_u16, system).hash(hasher);
    }
}

/// One locomotor object, field for field, and the object its piggyback slot
/// suspends through the same fold. [`LocomotorHashFields`] names every field
/// (its builder destructures the object), and `deny(unused_variables)` makes a
/// named but unfolded field a compile error; `locomotor_field_hash_tests`
/// checks that each one reaches the hash.
///
/// [`LocomotorHashFields`]: crate::sim::movement::locomotor::LocomotorHashFields
#[deny(unused_variables)]
fn hash_locomotor(
    loco: &crate::sim::movement::locomotor::LocomotorState,
    hasher: &mut impl Hasher,
) {
    let crate::sim::movement::locomotor::LocomotorHashFields {
        kind,
        powered,
        piggyback,
        runtime_payload,
        layer,
        balloon_hover,
        speed_type,
        movement_zone,
    } = loco.hash_fields();
    // The installed class is the kind of the bottom object, which this
    // fold reaches through the stash.
    (kind as u8).hash(hasher);
    powered.hash(hasher);
    (layer as u8).hash(hasher);
    balloon_hover.hash(hasher);
    speed_type.hash(hasher);
    movement_zone.hash(hasher);
    hash_locomotor_payload(runtime_payload, hasher);
    match piggyback {
        Some(stashed) => {
            1u8.hash(hasher);
            hash_locomotor(stashed, hasher);
        }
        None => 0u8.hash(hasher),
    }
}

fn hash_locomotor_payload(
    payload: &crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload,
    hasher: &mut impl Hasher,
) {
    use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
    match payload {
        LocomotorRuntimePayload::Drive(state) => {
            0u8.hash(hasher);
            hash_slope_transition_state(state.slope(), hasher);
            state.retained().hash(hasher);
        }
        LocomotorRuntimePayload::Walk(state) => {
            1u8.hash(hasher);
            state.hash(hasher);
        }
        LocomotorRuntimePayload::Teleport(state) => {
            2u8.hash(hasher);
            state.hash(hasher);
        }
        LocomotorRuntimePayload::Rocket(state) => {
            4u8.hash(hasher);
            state.hash(hasher);
        }
        LocomotorRuntimePayload::Hover(state) => {
            6u8.hash(hasher);
            state.hash(hasher);
        }
        LocomotorRuntimePayload::Ship(state) => {
            8u8.hash(hasher);
            hash_slope_transition_state(state.slope(), hasher);
            state.retained().hash(hasher);
        }
        LocomotorRuntimePayload::Fly(state) => {
            9u8.hash(hasher);
            let (target_height, taking_off, landing) = state.height_hash_fields();
            target_height.hash(hasher);
            taking_off.hash(hasher);
            landing.hash(hasher);
            state.destination().hash(hasher);
            state.cruise_mode().hash(hasher);
            state.moving().hash(hasher);
            state.landing_effect_latched().hash(hasher);
            state.airport_bound().hash(hasher);
            state.fall_counter().hash(hasher);
            state.target_speed().bits().hash(hasher);
            state.current_speed().bits().hash(hasher);
            // Fly+53/+54 (Power_Off `0x004CFD20`), only while it spins: a
            // game without a spin keeps its hash.
            if let Some(rate) = state.spin() {
                rate.hash(hasher);
            }
        }
        LocomotorRuntimePayload::Jumpjet(state) => {
            10u8.hash(hasher);
            state.hash(hasher);
        }
        LocomotorRuntimePayload::Tunnel(state) => {
            11u8.hash(hasher);
            state.hash(hasher);
        }
    }
}

fn hash_slope_transition_state(
    state: &crate::sim::movement::slope_transition::SlopeTransitionState,
    hasher: &mut impl Hasher,
) {
    // Native Drive/Ship raw-block persistence includes these defined fields;
    // Load does not resample (`Save` 0x004AF800/0x0069EF10, shared raw writer
    // 0x0055AA60). Hash every defined field for active and stash.
    let (previous_slope, current_slope, start_frame, transition_total) = state.hash_fields();
    previous_slope.hash(hasher);
    current_slope.hash(hasher);
    start_frame.hash(hasher);
    transition_total.hash(hasher);
}
