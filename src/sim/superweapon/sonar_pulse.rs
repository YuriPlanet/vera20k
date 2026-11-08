//! Ares `Type=SonarPulse` (`src/Misc/SWTypes/SonarPulse.cpp`): decloak the
//! affected Technos and keep them from cloaking for `SonarPulse.Delay`.
//!
//! Installed Ares: Activate `0x100753A0`, Initialize `0x100756D0` (no radar
//! event, delay 60, AITargeting Stealth, enemies on water for every mask),
//! LoadFromINI `0x10075710` (`SonarPulse.Delay` via ReadInteger; Action None
//! for a whole-map pulse), GetRange `0x10075680` (the type's `SW.Range`,
//! else radius 10) and Flags `0x100708C0` (NoEvent: the common Launch makes
//! no radar event).
//!
//! Activate: with a negative WidthOrRange every Techno of TechnoClass::Array,
//! otherwise those on the area's cells ([`super::protect::area_technos`]),
//! each passing `SW.AffectsHouse`/`SW.AffectsTarget`, restarts its SonarTimer
//! (TechnoExt+0x44) with the longer of its time left and the delay, and an
//! object not uncloaked starts uncloaking silently (vt+0x45C with 1). A ranged
//! pulse then raises a type-13 radar event while `SW.CreateRadarEvent` holds.
//! Always activates. The SonarTimer keeps CloakDisallowed refusing cloak
//! (`sim::world::techno_ai_cloak`).
//!
//! Ledger: no RNG draws; timer writes of each SonarTimer and each started
//! uncloak; no detach calls.

use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::intern::InternedId;
use crate::sim::world::{SimSoundEvent, Simulation};

/// SW_SonarPulse::Activate `0x100753A0` for `owner`'s Super at `cell`.
pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
) {
    let Some(metadata) = rules.super_weapon_metadata(&sw.id) else {
        return;
    };
    let launch = metadata.launch();
    let targeting = metadata.targeting();
    let range = match launch.range() {
        (width, height) if width < 0.0 && height < 0 => (10.0, -1),
        range => range,
    };
    let technos: Vec<u64> = if range.0 < 0.0 {
        let entities = &sim.substrate.entities;
        (0..entities.techno_registry_len())
            .filter_map(|index| entities.techno_registry_at(index))
            .collect()
    } else {
        super::protect::area_technos(sim, range, cell)
    };
    let frame = sim.session.binary_frame() as i32;
    for id in technos {
        let Some(entity) = sim.substrate.entities.get(id) else {
            continue;
        };
        let relation = sim.ares_house_relation(owner, entity.owner());
        if !targeting.affects_house().intersects(relation)
            || !super::protect::techno_affected(sim, entity, targeting.affects_target())
        {
            continue;
        }
        let object = sim.interner.object_type(entity.type_ref(), rules);
        let speed = object.map_or(0, |object| object.cloaking_speed);
        let stages = object.map_or(rules.general.cloaking_stages, |object| {
            object.cloaking_stages(rules.general.cloaking_stages)
        });
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        let left = entity.sonar_timer.remaining(frame);
        entity
            .sonar_timer
            .start(frame, left.max(launch.sonar_delay()));
        if let Some(cloak) = entity.cloak.as_mut()
            && cloak.state() != 0
        {
            cloak.start_uncloaking(frame, speed, stages, true);
        }
    }
    if range.0 >= 0.0 && launch.radar_event() {
        sim.sound_events.emit(SimSoundEvent::RadarEvent {
            radar: crate::sim::radar::RadarEventRequest::new(
                crate::sim::radar::RadarEventType::ImpactSilent,
                cell.0,
                cell.1,
            ),
        });
    }
}
