//! Ares SW_HunterSeeker::Activate (0x10072850) and its building callback
//! (0x10072190..0x10072372), read from the installed Ares instructions.

use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::object_type::ObjectCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponType;
use crate::sim::components::DriveCoord;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::hunter_seeker::HunterSeekerOrigin;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, PlacementEvidence, Simulation};

pub(super) fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type: InternedId,
    sw: &SuperWeaponType,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(settings) = sw.hunter_seeker.as_ref() else {
        return false;
    };
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    let unit = settings
        .unit_type
        .as_deref()
        .or_else(|| rules.side_hunter_seeker(house.side_index()));
    let Some(unit) = unit.and_then(|name| rules.object_in_category(ObjectCategory::Vehicle, name))
    else {
        return false;
    };
    let allowed = if settings.buildings.is_empty() {
        rules.hunter_seeker_buildings()
    } else {
        &settings.buildings
    };
    // Native house Building vector order, not an entity/category scan. Failed
    // placement does not spend MaxCount: only the callback's successes count.
    let buildings = house.base_projection().buildings().to_vec();
    let owner_name = sim.interner.resolve(owner).to_owned();
    let mut successes = 0_i32;
    for building in buildings {
        if settings.max_count >= 0 && successes >= settings.max_count {
            break;
        }
        let Some(building) = sim.substrate.entities.get(building) else {
            continue;
        };
        let Some(ty) = sim.interner.object_type(building.type_ref(), rules) else {
            continue;
        };
        if !allowed.iter().any(|name| name == &ty.id) {
            continue;
        }
        // GetCoords (vt+0x48 at100721ED): a building's foundation centre.
        let [at_x, at_y] = crate::sim::movement::ground_pose::object_center_xy(building);
        // FNPC56DC20: Foot, zone -1, Normal, 1x1, no occupancy/height/overlay
        // filter, bridge cells allowed. This owner supplies native frame order.
        let Some((x, y)) = sim.nearby_passable_cell(
            ((at_x / 256) as i16 as i32, (at_y / 256) as i16 as i32),
            NearbyArgs::plain(
                PassabilityArgs {
                    speed_type: SpeedType::Foot,
                    required_zone_id: None,
                    movement_zone: MovementZone::Normal,
                    bridge_aware_zone: false,
                },
                NearbyFootprint::SINGLE,
            ),
        ) else {
            continue;
        };
        if (x, y) == (0, 0) || !super::targeting::on_map(sim, (x, y)) {
            continue;
        }
        let Some(id) = sim.construct_resolved_object_limbo_at_height(
            unit,
            &owner_name,
            x,
            y,
            64,
            0,
            rules,
            effects,
        ) else {
            continue;
        };
        sim.substrate
            .entities
            .get_mut(id)
            .expect("constructed hunter")
            .set_hunter_seeker_origin(HunterSeekerOrigin { owner, sw_type });
        // Cell2Coord, deliberately Z=0: Put owns floor/layer admission. Do not
        // replace this with UnitDelivery's finished-unit placement policy.
        if sim
            .reveal_constructed_object_at_coord_with_overlay_context(
                id,
                DriveCoord::cell(x, y, 0),
                64,
                PlacementEvidence::EvaluateMark,
                rules,
                registry,
                effects,
            )
            .is_none()
        {
            sim.discard_constructed_limbo(id, rules, effects);
            continue;
        }
        sim.acquire_hunter_seeker_target(id, rules, registry, effects);
        // 100722F5 loads Area Guard (0xB); a non-harvester (UnitType+0xE0E,
        // `Harvester=`) switches to Attack at1007230C; vt+0x1E8 at10072323.
        let mission = if unit.harvester {
            MissionType::AreaGuard
        } else {
            MissionType::Attack
        };
        let frame = sim.session.binary_frame();
        let _ = sim.mission_queue_exact(
            id,
            MissionId::from_known(mission),
            0,
            frame,
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
        let _ = sim.substrate.mission_commence_exact(id, frame);
        successes += 1;
    }
    if successes == 0 {
        return false;
    }
    true
}
