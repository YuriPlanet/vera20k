//! Native `TechnoClass` / `BuildingClass` radar-registration visibility.
//!
//! These are client-local presentation predicates: they read the current
//! player, that player's shroud/sensor plane, and fresh mutable MapClass bounds.

use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::veterancy::VeterancyRank;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;

use super::minimap_helpers::{parse_foundation_size, world_to_minimap_pixel};
use super::radar_tracker::{RadarObjectUpdate, RadarProjectionFacts};

pub(super) fn radar_owner_is_human_player(
    owner: InternedId,
    local_owner: InternedId,
    houses: &BTreeMap<InternedId, crate::sim::house_state::HouseState>,
    game_mode_nonzero: bool,
) -> bool {
    if game_mode_nonzero {
        owner == local_owner
    } else {
        houses
            .get(&owner)
            .is_some_and(|house| house.is_human || house.player_control)
    }
}

/// ObjectClass virtual `GetCoords` used by radar visibility and radar clicks.
/// `BuildingClass::GetCoords @ 0x00447AC0` projects the stored foundation-origin
/// coordinate to its geometric centre; mobiles inherit the raw Object result.
pub(super) fn radar_object_get_coords_leptons(
    entity: &crate::sim::game_entity::GameEntity,
) -> (i32, i32) {
    let (mut x, mut y) = radar_raw_coord_leptons(entity);
    if entity.category == EntityCategory::Structure {
        let (width, height) = crate::rules::foundation::foundation_dimensions(&entity.foundation);
        x = x.wrapping_add(
            i32::from(width)
                .wrapping_sub(1)
                .wrapping_mul(crate::util::lepton::CELL_CENTER_LEPTON_I32),
        );
        y = y.wrapping_add(
            i32::from(height)
                .wrapping_sub(1)
                .wrapping_mul(crate::util::lepton::CELL_CENTER_LEPTON_I32),
        );
    }
    (x, y)
}

fn radar_raw_coord_leptons(entity: &crate::sim::game_entity::GameEntity) -> (i32, i32) {
    let x = i32::from(entity.position.rx)
        .wrapping_mul(crate::util::lepton::LEPTONS_PER_CELL_I32)
        .wrapping_add(entity.position.sub_x.to_num::<i32>());
    let y = i32::from(entity.position.ry)
        .wrapping_mul(crate::util::lepton::LEPTONS_PER_CELL_I32)
        .wrapping_add(entity.position.sub_y.to_num::<i32>());
    (x, y)
}

fn radar_packed_cell_from_leptons(x: i32, y: i32) -> (i32, i32) {
    // Coord2Cell uses signed division and then narrows to signed words.
    (
        i32::from((x / crate::util::lepton::LEPTONS_PER_CELL_I32) as i16),
        i32::from((y / crate::util::lepton::LEPTONS_PER_CELL_I32) as i16),
    )
}

fn radar_fog_cell_from_leptons(x: i32, y: i32) -> Option<(u16, u16)> {
    let (x, y) = radar_packed_cell_from_leptons(x, y);
    (x >= 0 && y >= 0).then_some((x as u16, y as u16))
}

fn enemy_sensed_prefilter(entity: &crate::sim::game_entity::GameEntity) -> bool {
    if entity.category == EntityCategory::Structure {
        true
    } else if entity.category == EntityCategory::Aircraft {
        entity
            .aircraft_ammo
            .as_ref()
            .is_none_or(|ammo| (ammo.current as i8) < 0)
    } else {
        entity.low_bridge_tube_state.is_none()
    }
}

fn radar_fresh_mode_one_membership(
    entity: &crate::sim::game_entity::GameEntity,
    playfield_bounds: Option<crate::map::playfield::PlayfieldBounds>,
    resolved_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> bool {
    let Some(bounds) = playfield_bounds else {
        // Presentation-only/headless fixtures without an installed MapClass
        // authority preserve their historical admission behavior.
        return true;
    };
    let Some(resolved_terrain) = resolved_terrain else {
        // A configured LocalSize without its CellClass level/slope authority
        // cannot be replaced by a rectangular or geometry-only approximation.
        return false;
    };
    let (x, y) = radar_object_get_coords_leptons(entity);
    crate::sim::cell_rect::cell_is_in_playfield_leptons(
        (x, y, 0),
        Some(bounds),
        Some(resolved_terrain),
    )
}

fn radar_get_height_leptons(
    entity: &crate::sim::game_entity::GameEntity,
    resolved_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> i32 {
    let (x, y) = radar_object_get_coords_leptons(entity);
    let (rx, ry) = radar_packed_cell_from_leptons(x, y);
    let ground = resolved_terrain
        .and_then(|terrain| {
            (rx >= 0 && ry >= 0)
                .then(|| terrain.cell(rx as u16, ry as u16))
                .flatten()
        })
        .and_then(|cell| {
            crate::util::lepton::ground_height_leptons(cell.level, cell.slope_type, x, y).ok()
        })
        .unwrap_or_else(|| {
            i32::from(entity.position.z as i8)
                .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
        });
    let mut height = crate::sim::movement::ground_pose::object_world_z_leptons(entity, None)
        .wrapping_sub(ground);
    if entity.on_bridge {
        height = height.wrapping_sub(crate::sim::map::bridge_topology::BRIDGE_DECK_HEIGHT_LEPTONS);
    }
    height
}

fn radar_building_both_corners_shrouded(
    fog: &FogState,
    local_owner: InternedId,
    entity: &crate::sim::game_entity::GameEntity,
    foundation: (u32, u32),
) -> bool {
    let origin_shrouded =
        !fog.is_cell_revealed(local_owner, entity.position.rx, entity.position.ry);
    let far_x = i32::from(entity.position.rx).wrapping_add((foundation.0 as i32).wrapping_sub(1));
    let far_y = i32::from(entity.position.ry).wrapping_add((foundation.1 as i32).wrapping_sub(1));
    let far_shrouded =
        far_x < 0 || far_y < 0 || !fog.is_cell_revealed(local_owner, far_x as u16, far_y as u16);
    origin_shrouded && far_shrouded
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RadarVisibilityResult {
    pub visible: bool,
    pub out_code: u8,
}

impl RadarVisibilityResult {
    const HIDDEN: Self = Self {
        visible: false,
        out_code: 0,
    };
    const VISIBLE: Self = Self {
        visible: true,
        out_code: 0,
    };
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RadarMobileVisibilityFacts {
    pub type_invisible: bool,
    pub sinking: bool,
    pub object_alive: bool,
    pub in_limbo: bool,
    pub owner_is_human_player: bool,
    pub fresh_in_playfield: bool,
    pub shrouded: bool,
    pub cloak_state: i32,
    pub has_sensor: bool,
    pub allied_with_current_player: bool,
    pub height_leptons: i32,
    pub veteran_radar_invisible: bool,
}

/// Mobile virtual +0x324 @ 0x0070D1D0, in native branch order.
pub(super) fn radar_mobile_registration_visibility(
    facts: RadarMobileVisibilityFacts,
    discovered: bool,
) -> RadarVisibilityResult {
    if facts.type_invisible || facts.sinking || !facts.object_alive || facts.in_limbo {
        return RadarVisibilityResult::HIDDEN;
    }
    if facts.owner_is_human_player {
        return RadarVisibilityResult {
            visible: discovered,
            out_code: 0,
        };
    }
    if !facts.fresh_in_playfield {
        return RadarVisibilityResult::HIDDEN;
    }
    let needs_sensor = facts.cloak_state == 2
        || facts.height_leptons < -20
        || facts.veteran_radar_invisible
        || facts.shrouded;
    if !needs_sensor {
        return RadarVisibilityResult::VISIBLE;
    }
    if !facts.has_sensor {
        return RadarVisibilityResult::HIDDEN;
    }
    let out_code =
        if !facts.allied_with_current_player && !facts.veteran_radar_invisible && !facts.shrouded {
            if facts.height_leptons < -20 { 2 } else { 1 }
        } else {
            0
        };
    RadarVisibilityResult {
        visible: true,
        out_code,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RadarBuildingVisibilityFacts {
    pub type_invisible: bool,
    pub in_limbo: bool,
    pub owner_is_human_player: bool,
    pub fresh_in_playfield: bool,
    pub both_corners_shrouded: bool,
    pub cloak_state: i32,
    /// BuildingClass+0x6ED. No Rust production writer exists yet.
    pub building_cloak_stage: u8,
    /// BuildingClass+0x6E7. No Rust production writer exists yet.
    pub building_cloak_flag: bool,
    pub has_sensor: bool,
    pub allied_with_current_player: bool,
}

/// Building override +0x324 @ 0x00457020, distinct from the mobile body.
pub(super) fn radar_building_registration_visibility(
    facts: RadarBuildingVisibilityFacts,
    discovered: bool,
) -> RadarVisibilityResult {
    if facts.type_invisible || facts.in_limbo {
        return RadarVisibilityResult::HIDDEN;
    }
    if facts.owner_is_human_player {
        return RadarVisibilityResult {
            visible: discovered,
            out_code: 0,
        };
    }
    if !facts.fresh_in_playfield {
        return RadarVisibilityResult::HIDDEN;
    }
    if facts.cloak_state != 2
        && facts.building_cloak_stage != 15
        && !facts.building_cloak_flag
        && !facts.both_corners_shrouded
    {
        return RadarVisibilityResult::VISIBLE;
    }
    if !facts.has_sensor {
        return RadarVisibilityResult::HIDDEN;
    }
    let out_code = u8::from(
        !facts.allied_with_current_player
            && !facts.building_cloak_flag
            && !facts.both_corners_shrouded,
    );
    RadarVisibilityResult {
        visible: true,
        out_code,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum RadarRegistrationVisibilityFacts {
    Mobile(RadarMobileVisibilityFacts),
    Building(RadarBuildingVisibilityFacts),
}

impl RadarRegistrationVisibilityFacts {
    pub(super) fn evaluate(self, discovered: bool) -> RadarVisibilityResult {
        match self {
            Self::Mobile(facts) => radar_mobile_registration_visibility(facts, discovered),
            Self::Building(facts) => radar_building_registration_visibility(facts, discovered),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_radar_object_update(
    entity: &crate::sim::game_entity::GameEntity,
    houses: &BTreeMap<InternedId, crate::sim::house_state::HouseState>,
    local_owner: Option<InternedId>,
    fog: &FogState,
    full_visibility: bool,
    game_mode_nonzero: bool,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    projection: RadarProjectionFacts,
    playfield_bounds: Option<crate::map::playfield::PlayfieldBounds>,
    resolved_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> RadarObjectUpdate {
    let type_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
    let object = rules.and_then(|rules| rules.object(type_str));
    let (raw_x, raw_y) = radar_raw_coord_leptons(entity);
    let origin = projection.native_surface.map_or_else(
        || {
            let (screen_x, screen_y) = super::locomotor_visual::screen_position(entity);
            let screen_y = screen_y - crate::map::terrain::TILE_HEIGHT / 2.0;
            let (x, y) = world_to_minimap_pixel(
                screen_x,
                screen_y,
                projection.world_origin_x,
                projection.world_origin_y,
                projection.world_width,
                projection.world_height,
                projection.map_offset_x,
                projection.map_offset_y,
                projection.map_pixel_w,
                projection.map_pixel_h,
            );
            (x as i32, y as i32)
        },
        // TechnoClass+0x4A0 passes clamp=0 to FUN_006557F0.
        |surface| surface.world_to_surface_pixel(raw_x, raw_y, false),
    );
    let foundation = (entity.category == EntityCategory::Structure)
        .then(|| parse_foundation_size(&entity.foundation));
    let owner_is_human_player = local_owner.is_none_or(|local_owner| {
        radar_owner_is_human_player(entity.owner(), local_owner, houses, game_mode_nonzero)
    });
    let (coord_x, coord_y) = radar_object_get_coords_leptons(entity);
    let current_cell = radar_fog_cell_from_leptons(coord_x, coord_y);
    // The type-5 call at `0x0070DA95..0x0070DAD7` converts raw Object+0x9C,
    // not BuildingClass's centre-adjusted +0x324 coordinate.
    let event_source_cell = radar_fog_cell_from_leptons(raw_x, raw_y);
    // `TechnoClass::IdleAnimDispatch @ 0x0070DA79..0x0070DAD7` rejects a
    // FootClass object while its signed `+0x684` byte is nonnegative. For
    // Unit/Infantry that byte is the active TubeClass index; Aircraft uses the
    // same inherited byte as its finite current-ammo value. Buildings do not
    // carry the FootClass flag and bypass this prefilter.
    let enemy_sensed_prefilter = enemy_sensed_prefilter(entity);
    let discovery_observed = full_visibility
        || local_owner.is_none()
        || local_owner.is_some_and(|local_owner| {
            current_cell.is_some_and(|(rx, ry)| fog.is_cell_revealed(local_owner, rx, ry))
        });
    let shrouded = !full_visibility
        && local_owner.is_some_and(|local_owner| {
            current_cell.is_none_or(|(rx, ry)| !fog.is_cell_revealed(local_owner, rx, ry))
        });
    let both_corners_shrouded = !full_visibility
        && local_owner.is_some_and(|local_owner| {
            foundation.is_some_and(|foundation| {
                radar_building_both_corners_shrouded(fog, local_owner, entity, foundation)
            })
        });
    let has_sensor = full_visibility
        || local_owner.is_none()
        || local_owner.is_some_and(|local_owner| {
            current_cell.is_some_and(|(rx, ry)| fog.has_sensor_for_house(local_owner, rx, ry))
        });
    let allied_with_current_player = full_visibility
        || local_owner.is_none()
        || local_owner.is_some_and(|local_owner| {
            entity.owner() == local_owner
                || interner.is_some_and(|interner| {
                    fog.is_friendly_id(local_owner, entity.owner(), interner)
                })
        });
    let effective_type_invisible =
        object.is_some_and(|object| object.invisible || object.invisible_in_game);
    let fresh_in_playfield =
        radar_fresh_mode_one_membership(entity, playfield_bounds, resolved_terrain);
    let cloak_state = entity.cloak.as_ref().map_or(0, |cloak| cloak.state);
    let veteran_radar_invisible = object.is_some_and(|object| {
        match crate::sim::combat::veterancy::rank_of(entity.veterancy_raw) {
            VeterancyRank::Rookie => false,
            VeterancyRank::Veteran => object.veteran_radar_invisible,
            VeterancyRank::Elite => object.veteran_radar_invisible || object.elite_radar_invisible,
        }
    });
    let visibility = if foundation.is_some() {
        RadarRegistrationVisibilityFacts::Building(RadarBuildingVisibilityFacts {
            type_invisible: effective_type_invisible,
            in_limbo: entity.lifecycle.in_limbo,
            owner_is_human_player,
            fresh_in_playfield,
            both_corners_shrouded,
            cloak_state,
            // Exact predicate fields are explicit; neutral production facts do
            // not pretend the absent BuildingClass cloak writers are closed.
            building_cloak_stage: 0,
            building_cloak_flag: false,
            has_sensor,
            allied_with_current_player,
        })
    } else {
        RadarRegistrationVisibilityFacts::Mobile(RadarMobileVisibilityFacts {
            type_invisible: effective_type_invisible,
            sinking: entity.sinking.is_active(),
            object_alive: entity.lifecycle.object_alive,
            in_limbo: entity.lifecycle.in_limbo,
            owner_is_human_player,
            fresh_in_playfield,
            shrouded,
            cloak_state,
            has_sensor,
            allied_with_current_player,
            height_leptons: radar_get_height_leptons(entity, resolved_terrain),
            veteran_radar_invisible,
        })
    };

    RadarObjectUpdate {
        stable_id: entity.stable_id(),
        owner: entity.owner(),
        origin,
        event_source_cell,
        enemy_sensed_prefilter,
        foundation,
        radar_scale: projection.cell_axis_scale(),
        discovery_observed,
        visibility,
        // AddObjectToTracker @ 0x00655560 compares directly to g_PlayerPtr,
        // narrower than IsHumanPlayer's single-player shroud exception.
        local_front: local_owner == Some(entity.owner()),
    }
}

#[cfg(test)]
#[path = "radar_visibility_lifecycle_tests.rs"]
mod lifecycle_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::map::tube_facts::TubeId;
    use crate::sim::components::DriveCoord;
    use crate::sim::docking::aircraft_dock::AircraftAmmo;
    use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;

    fn mobile() -> RadarMobileVisibilityFacts {
        RadarMobileVisibilityFacts {
            type_invisible: false,
            sinking: false,
            object_alive: true,
            in_limbo: false,
            owner_is_human_player: false,
            fresh_in_playfield: true,
            shrouded: false,
            cloak_state: 0,
            has_sensor: false,
            allied_with_current_player: false,
            height_leptons: 0,
            veteran_radar_invisible: false,
        }
    }

    fn building() -> RadarBuildingVisibilityFacts {
        RadarBuildingVisibilityFacts {
            type_invisible: false,
            in_limbo: false,
            owner_is_human_player: false,
            fresh_in_playfield: true,
            both_corners_shrouded: false,
            cloak_state: 0,
            building_cloak_stage: 0,
            building_cloak_flag: false,
            has_sensor: false,
            allied_with_current_player: false,
        }
    }

    #[test]
    fn enemy_sensed_prefilter_maps_native_foot_signed_684() {
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "Soviet", 4, 5);
        assert!(enemy_sensed_prefilter(&entity));
        entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
            tube_id: TubeId(2),
            cursor: 0,
            target: DriveCoord { x: 0, y: 0, z: 0 },
        });
        assert!(!enemy_sensed_prefilter(&entity));

        entity.category = EntityCategory::Aircraft;
        entity.low_bridge_tube_state = None;
        entity.aircraft_ammo = Some(AircraftAmmo::new(3));
        assert!(!enemy_sensed_prefilter(&entity));
        entity.aircraft_ammo = None;
        assert!(
            enemy_sensed_prefilter(&entity),
            "Ammo=-1 keeps the signed byte negative"
        );

        entity.category = EntityCategory::Structure;
        entity.aircraft_ammo = Some(AircraftAmmo::new(3));
        assert!(
            enemy_sensed_prefilter(&entity),
            "Building lacks AbstractFlags IsFoot"
        );
    }

    #[test]
    fn enemy_sensed_source_uses_raw_building_anchor_not_visibility_center() {
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "BLDG", "Enemy", 4, 5);
        entity.category = EntityCategory::Structure;
        entity.foundation = "3x2".to_string();
        let raw = radar_raw_coord_leptons(&entity);
        let visibility = radar_object_get_coords_leptons(&entity);
        assert_eq!(radar_fog_cell_from_leptons(raw.0, raw.1), Some((4, 5)));
        assert_ne!(
            radar_fog_cell_from_leptons(visibility.0, visibility.1),
            Some((4, 5)),
            "BuildingClass +0x324 may center its query, but 0x70DAD7 packs Object+0x9C"
        );
    }

    #[test]
    fn radar_object_get_coords_centralizes_mobile_and_building_foundation_centres() {
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "BLDG", "Enemy", 10, 20);
        entity.position.sub_x = crate::util::fixed_math::SimFixed::from_num(200);
        entity.position.sub_y = crate::util::fixed_math::SimFixed::from_num(33);
        let raw = (10 * 256 + 200, 20 * 256 + 33);

        assert_eq!(
            radar_object_get_coords_leptons(&entity),
            raw,
            "mobile GetCoords is raw"
        );
        entity.category = EntityCategory::Structure;
        for (foundation, expected) in [
            ("1x1", raw),
            ("2x2", (raw.0 + 128, raw.1 + 128)),
            ("4x3", (raw.0 + 384, raw.1 + 256)),
            ("", raw),
            ("not-a-native-foundation", raw),
        ] {
            entity.foundation = foundation.to_string();
            assert_eq!(
                radar_object_get_coords_leptons(&entity),
                expected,
                "foundation {foundation:?}"
            );
        }
    }

    #[test]
    fn radar_object_update_uses_native_world_projection_shared_with_cell_events() {
        let surface =
            crate::render::native_radar_surface::NativeRadarSurfaceGeometry::from_raw_rect(
                0, 0, 300, 180,
            )
            .expect("wide generated surface");
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "Enemy", 120, 30);
        entity.position.sub_x = crate::util::fixed_math::SimFixed::from_num(0);
        entity.position.sub_y = crate::util::fixed_math::SimFixed::from_num(0);
        let mut projection = visibility_projection();
        projection.native_surface = Some(surface);
        let update = build_radar_object_update(
            &entity,
            &BTreeMap::new(),
            None,
            &FogState::default(),
            true,
            false,
            None,
            None,
            projection,
            None,
            None,
        );
        let local = surface.cell_to_surface_pixel((120, 30));
        assert_eq!(update.origin, local, "Techno+0x4A0 uses FUN_006557F0");
        assert_eq!(update.event_source_cell, Some((120, 30)));
        assert_eq!(
            surface.surface_to_aperture_pixel(update.origin),
            surface.surface_to_aperture_pixel(local),
            "object and type-5 source receive one identical final copy transform"
        );
    }

    #[test]
    fn sinking_owner_clears_retained_radar_registration_before_hull_cleanup() {
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=TEST\n[TEST]\nNaval=yes\nStrength=800\n",
        ))
        .unwrap();
        let mut sim = crate::sim::world::Simulation::new();
        let id = sim.allocate_stable_id();
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(id, "TEST", "Americans", 4, 5);
        entity.type_ref = sim.interner.intern("TEST");
        entity.owner = sim.interner.intern("Americans");
        entity.category = EntityCategory::Unit;
        entity.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(entity);
        let update = |sim: &crate::sim::world::Simulation| {
            build_radar_object_update(
                sim.entities().get(id).unwrap(),
                &sim.houses,
                None,
                &sim.fog,
                true,
                false,
                Some(&rules),
                Some(&sim.interner),
                visibility_projection(),
                None,
                None,
            )
        };
        let mut tracker = super::super::radar_tracker::RetainedRadarTracker::default();
        tracker.update_object(update(&sim), false);
        assert!(tracker.is_registered(id));
        sim.begin_ship_sinking(id, &rules);
        assert!(sim.entities().get(id).unwrap().lifecycle.object_alive);
        tracker.update_object(update(&sim), false);
        assert!(
            !tracker.is_registered(id),
            "Techno70D1D0 reads +3CD before discovery/owner gates"
        );
    }

    fn flat_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            base_speed_costs: crate::map::resolved_terrain::TEST_OPEN_SPEED_COSTS,
            ..crate::map::resolved_terrain::test_smudge_cell(rx, ry)
        }
    }

    pub(super) fn flat_terrain(side: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(side, side, flat_cell)
    }

    pub(super) fn visibility_projection() -> RadarProjectionFacts {
        RadarProjectionFacts {
            native_surface: None,
            world_origin_x: 0.0,
            world_origin_y: 0.0,
            world_width: 1.0,
            world_height: 1.0,
            map_offset_x: 0.0,
            map_offset_y: 0.0,
            map_pixel_w: 1.0,
            map_pixel_h: 1.0,
        }
    }

    #[test]
    fn radar_visibility_mobile_ctor_gates_precede_human_discovery() {
        for facts in [
            RadarMobileVisibilityFacts {
                type_invisible: true,
                owner_is_human_player: true,
                ..mobile()
            },
            RadarMobileVisibilityFacts {
                sinking: true,
                owner_is_human_player: true,
                ..mobile()
            },
            RadarMobileVisibilityFacts {
                object_alive: false,
                owner_is_human_player: true,
                ..mobile()
            },
            RadarMobileVisibilityFacts {
                in_limbo: true,
                owner_is_human_player: true,
                ..mobile()
            },
        ] {
            assert_eq!(
                radar_mobile_registration_visibility(facts, true),
                RadarVisibilityResult::HIDDEN
            );
        }
    }

    #[test]
    fn radar_visibility_human_owner_returns_cached_discovery_before_fresh_queries() {
        let facts = RadarMobileVisibilityFacts {
            owner_is_human_player: true,
            fresh_in_playfield: false,
            shrouded: true,
            cloak_state: 2,
            has_sensor: false,
            ..mobile()
        };
        assert!(!radar_mobile_registration_visibility(facts, false).visible);
        assert_eq!(
            radar_mobile_registration_visibility(facts, true),
            RadarVisibilityResult::VISIBLE
        );
    }

    #[test]
    fn radar_visibility_mobile_cloak_sensor_alliance_and_height_match_outcode() {
        let cloaked = RadarMobileVisibilityFacts {
            cloak_state: 2,
            ..mobile()
        };
        assert_eq!(
            radar_mobile_registration_visibility(cloaked, false),
            RadarVisibilityResult::HIDDEN
        );
        assert_eq!(
            radar_mobile_registration_visibility(
                RadarMobileVisibilityFacts {
                    has_sensor: true,
                    ..cloaked
                },
                false,
            ),
            RadarVisibilityResult {
                visible: true,
                out_code: 1,
            }
        );
        assert_eq!(
            radar_mobile_registration_visibility(
                RadarMobileVisibilityFacts {
                    has_sensor: true,
                    allied_with_current_player: true,
                    ..cloaked
                },
                false,
            )
            .out_code,
            0
        );
        assert_eq!(
            radar_mobile_registration_visibility(
                RadarMobileVisibilityFacts {
                    height_leptons: -21,
                    has_sensor: true,
                    ..mobile()
                },
                false,
            )
            .out_code,
            2
        );
        assert!(
            radar_mobile_registration_visibility(
                RadarMobileVisibilityFacts {
                    height_leptons: -20,
                    ..mobile()
                },
                false,
            )
            .visible
        );
    }

    #[test]
    fn radar_visibility_ai_shroud_and_veteran_radar_invisible_require_sensor() {
        for facts in [
            RadarMobileVisibilityFacts {
                shrouded: true,
                ..mobile()
            },
            RadarMobileVisibilityFacts {
                veteran_radar_invisible: true,
                ..mobile()
            },
        ] {
            assert!(!radar_mobile_registration_visibility(facts, false).visible);
            let sensed = radar_mobile_registration_visibility(
                RadarMobileVisibilityFacts {
                    has_sensor: true,
                    ..facts
                },
                false,
            );
            assert!(sensed.visible);
            assert_eq!(sensed.out_code, 0);
        }
    }

    #[test]
    fn radar_visibility_building_two_corner_and_cloak_contract_is_distinct() {
        let both_shrouded = RadarBuildingVisibilityFacts {
            both_corners_shrouded: true,
            ..building()
        };
        assert!(!radar_building_registration_visibility(both_shrouded, false).visible);
        assert_eq!(
            radar_building_registration_visibility(
                RadarBuildingVisibilityFacts {
                    has_sensor: true,
                    ..both_shrouded
                },
                false,
            ),
            RadarVisibilityResult::VISIBLE
        );
        assert_eq!(
            radar_building_registration_visibility(
                RadarBuildingVisibilityFacts {
                    cloak_state: 2,
                    has_sensor: true,
                    ..building()
                },
                false,
            )
            .out_code,
            1
        );
        // A visible far corner admits the building, while a mobile in shroud
        // at that same origin still requires a sensor.
        assert!(radar_building_registration_visibility(building(), false).visible);
        assert!(
            !radar_mobile_registration_visibility(
                RadarMobileVisibilityFacts {
                    shrouded: true,
                    ..mobile()
                },
                false,
            )
            .visible
        );
    }

    #[test]
    fn radar_visibility_building_human_and_type_lifecycle_order() {
        let human = RadarBuildingVisibilityFacts {
            owner_is_human_player: true,
            fresh_in_playfield: false,
            both_corners_shrouded: true,
            ..building()
        };
        assert!(radar_building_registration_visibility(human, true).visible);
        assert!(!radar_building_registration_visibility(human, false).visible);
        assert!(
            !radar_building_registration_visibility(
                RadarBuildingVisibilityFacts {
                    type_invisible: true,
                    ..human
                },
                true,
            )
            .visible
        );
        assert!(
            !radar_building_registration_visibility(
                RadarBuildingVisibilityFacts {
                    in_limbo: true,
                    ..human
                },
                true,
            )
            .visible
        );
    }

    #[test]
    fn radar_visibility_fresh_mode_one_ignores_stale_stored_membership() {
        let bounds = crate::map::playfield::PlayfieldBounds {
            base: 16,
            off_fc: 2,
            off_100: 2,
            off_104: 12,
            off_108: 8,
        };
        let terrain = flat_terrain(32);
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "DOT", "Enemy", 12, 10);
        entity.in_playfield = false;
        assert!(radar_fresh_mode_one_membership(
            &entity,
            Some(bounds),
            Some(&terrain),
        ));
        entity.in_playfield = true;
        entity.position.rx = 2;
        entity.position.ry = 2;
        assert!(!radar_fresh_mode_one_membership(
            &entity,
            Some(bounds),
            Some(&terrain),
        ));
        assert!(!radar_fresh_mode_one_membership(
            &entity,
            Some(bounds),
            None,
        ));
    }

    #[test]
    fn radar_visibility_building_shroud_samples_origin_and_far_corner() {
        let local = crate::sim::intern::test_intern("Local");
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "BLDG", "Enemy", 4, 4);
        entity.foundation = "3x2".to_string();
        let mut fog = FogState::default();
        fog.mark_visible_for_owner(local, 6, 5);
        assert!(!radar_building_both_corners_shrouded(
            &fog,
            local,
            &entity,
            (3, 2),
        ));
        let fog = FogState::default();
        assert!(radar_building_both_corners_shrouded(
            &fog,
            local,
            &entity,
            (3, 2),
        ));
    }

    #[test]
    fn radar_visibility_invisible_in_game_is_effective_type_invisibility() {
        let local = crate::sim::intern::test_intern("Local");
        let enemy = crate::sim::intern::test_intern("Enemy");
        crate::sim::intern::test_intern("DOT");
        let interner = crate::sim::intern::test_interner();
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=DOT\n[DOT]\nStrength=100\nInvisibleInGame=yes\n",
        ))
        .expect("visibility fixture rules");
        let mut houses = BTreeMap::new();
        houses.insert(
            enemy,
            crate::sim::house_state::HouseState::new(enemy, 0, None, false, 0, 10),
        );
        let mut fog = FogState::default();
        fog.mark_visible_for_owner(local, 5, 5);
        let mut entity = crate::sim::game_entity::GameEntity::test_default(1, "DOT", "Enemy", 5, 5);
        entity.lifecycle.in_limbo = false;
        let update = build_radar_object_update(
            &entity,
            &houses,
            Some(local),
            &fog,
            false,
            true,
            Some(&rules),
            Some(&interner),
            visibility_projection(),
            None,
            None,
        );
        assert!(!update.visibility.evaluate(false).visible);
    }
}
