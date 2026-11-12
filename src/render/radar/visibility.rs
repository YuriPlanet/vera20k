//! Native `TechnoClass` / `BuildingClass` radar-registration visibility.
//!
//! These are client-local presentation predicates: they read the current
//! player, that player's shroud/sensor plane, and fresh mutable MapClass bounds.

use crate::map::entities::EntityCategory;
use crate::render::snapshot::SnapshotObject;
use crate::rules::object_type::Ability;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::veterancy::has_weapon_ability;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;

use super::helpers::{parse_foundation_size, world_to_minimap_pixel};
use super::tracker::{RadarObjectUpdate, RadarProjectionFacts};

pub(super) fn radar_owner_is_human_player(
    owner: InternedId,
    local_owner: InternedId,
    houses: &crate::sim::houses::Houses,
    game_mode_nonzero: bool,
) -> bool {
    if game_mode_nonzero {
        owner == local_owner
    } else {
        houses
            .get(&owner)
            .is_some_and(|house| house.is_human() || house.player_control())
    }
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
    if entity.category() == EntityCategory::Structure {
        true
    } else if entity.category() == EntityCategory::Aircraft {
        entity.ammo().is_none_or(|ammo| (ammo.current() as i8) < 0)
    } else {
        entity.low_bridge_tube_state.is_none()
    }
}

fn radar_fresh_mode_one_membership(
    entity: &impl crate::render::snapshot::SnapshotObject,
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
    // ObjectClass virtual `GetCoords`.
    let [x, y] = entity.center_xy();
    crate::sim::cell_rect::cell_is_in_playfield_leptons(
        (x, y, 0),
        Some(bounds),
        Some(resolved_terrain),
    )
}

fn radar_get_height_leptons(
    entity: &impl crate::render::snapshot::SnapshotObject,
    resolved_terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> i32 {
    // ObjectClass virtual `GetCoords`.
    let [x, y] = entity.center_xy();
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
            i32::from(entity.position().z() as i8)
                .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
        });
    // `object_world_z_leptons(entity, None)`.
    let mut height = entity.world_z_leptons_off_map().wrapping_sub(ground);
    if entity.on_bridge() {
        height = height.wrapping_sub(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
    }
    height
}

/// `MapClass::IsShrouded @ 0x00586360` for `viewer` at `point`
/// ([`crate::sim::vision::point_is_shrouded`]), which raises the point by
/// its height before testing the shroud lattice. A fixture without terrain
/// tests the flat cell.
fn radar_point_shrouded(
    fog: &FogState,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    viewer: InternedId,
    point: crate::sim::components::DriveCoord,
) -> bool {
    match cells {
        Some(cells) => crate::sim::vision::point_is_shrouded(fog, cells, viewer, point),
        None => radar_fog_cell_from_leptons(point.x, point.y)
            .is_none_or(|(rx, ry)| !fog.is_cell_revealed(viewer, rx, ry)),
    }
}

/// RenderCellPixel's shroud test (`0x00655D06..0x00655D5B`): IsShrouded
/// at a radar Cell's centre raised to its floor (`0x00578080`), which is
/// [`crate::sim::vision::cell_is_shrouded`]. A fixture without terrain
/// tests the flat cell.
pub(super) fn radar_cell_shrouded(
    fog: &FogState,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    viewer: InternedId,
    cell: (u16, u16),
) -> bool {
    match cells {
        Some(cells) => crate::sim::vision::cell_is_shrouded(fog, cells, viewer, cell),
        None => !fog.is_cell_revealed(viewer, cell.0, cell.1),
    }
}

/// `0x00457117..0x0045714B`: IsShrouded at the Location (`+0x9C`), then at
/// the Location plus `(dimension - 1)` cells along each foundation axis
/// (`0x0045EC90` width, `0x0045ECA0` height), at the Location's Z.
fn radar_building_both_corners_shrouded(
    fog: &FogState,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    local_owner: InternedId,
    location: crate::sim::components::DriveCoord,
    foundation: (u32, u32),
) -> bool {
    let far = crate::sim::components::DriveCoord {
        x: location
            .x
            .wrapping_add((foundation.0 as i32).wrapping_sub(1).wrapping_mul(256)),
        y: location
            .y
            .wrapping_add((foundation.1 as i32).wrapping_sub(1).wrapping_mul(256)),
        z: location.z,
    };
    radar_point_shrouded(fog, cells, local_owner, location)
        && radar_point_shrouded(fog, cells, local_owner, far)
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
    /// The current player's house asks (`PlayerPtr->IsAlliedWith(Owner)`,
    /// `0x0070D394`, `HouseClass::IsAlliedWith @ 0x004F9A50`).
    pub allied_with_current_player: bool,
    pub height_leptons: i32,
    pub veteran_radar_invisible: bool,
}

/// Mobile virtual +0x324 @ 0x0070D1D0, in native branch order. Its Fog of
/// War arm (`0x0070D350..0x0070D384`: scenario flag `0x1000` and a fogged
/// Location) never applies: the fog test it calls, `0x005865E0`, is
/// `xor al, al; ret 4` in YR, so no fact stands for it here.
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
    /// BuildingClass+0x6ED, the fade stage.
    pub building_cloak_stage: u8,
    /// BuildingClass+0x6E7, the FogOfWar freeze flag set by the fogged-object
    /// constructor (`0x004D10D5`): `freeze_building_in_fog` and `clean_fog`
    /// keep it as `FogState::is_object_fogged` for the local player.
    pub fogged: bool,
    pub has_sensor: bool,
    /// The current player's house asks (`PlayerPtr->IsAlliedWith(Owner)`,
    /// `0x00457196`, `HouseClass::IsAlliedWith @ 0x004F9A50`).
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
        && !facts.fogged
        && !facts.both_corners_shrouded
    {
        return RadarVisibilityResult::VISIBLE;
    }
    if !facts.has_sensor {
        return RadarVisibilityResult::HIDDEN;
    }
    let out_code = u8::from(
        !facts.allied_with_current_player && !facts.fogged && !facts.both_corners_shrouded,
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

pub(super) fn build_radar_object_update(
    entity: &impl crate::render::snapshot::SnapshotObject,
    houses: &crate::sim::houses::Houses,
    local_owner: Option<InternedId>,
    fog: &FogState,
    alliances: &crate::sim::house_diplomacy::HouseAlliances,
    full_visibility: bool,
    game_mode_nonzero: bool,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    projection: RadarProjectionFacts,
    playfield_bounds: Option<crate::map::playfield::PlayfieldBounds>,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> RadarObjectUpdate {
    let resolved_terrain = cells.map(crate::map::resolved_terrain::NativeCellQuery::terrain);
    let object = rules
        .zip(interner)
        .and_then(|(rules, interner)| interner.object_type(entity.type_ref(), rules));
    let [raw_x, raw_y] = crate::sim::movement::ground_pose::position_world_xy(entity.position());
    let origin = projection.native_surface.map_or_else(
        || {
            let (screen_x, screen_y) = entity.screen_position();
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
    let facts = entity.radar();
    let foundation = facts.foundation;
    let owner_is_human_player = local_owner.is_none_or(|local_owner| {
        radar_owner_is_human_player(entity.owner(), local_owner, houses, game_mode_nonzero)
    });
    let [coord_x, coord_y] = entity.center_xy();
    let current_cell = radar_fog_cell_from_leptons(coord_x, coord_y);
    // The type-5 call at `0x0070DA95..0x0070DAD7` converts raw Object+0x9C,
    // not BuildingClass's centre-adjusted +0x324 coordinate.
    let event_source_cell = radar_fog_cell_from_leptons(raw_x, raw_y);
    // `TechnoClass::IdleAnimDispatch @ 0x0070DA79..0x0070DAD7` rejects a
    // FootClass object while its signed `+0x684` byte is nonnegative. For
    // Unit/Infantry that byte is the active TubeClass index; Aircraft uses the
    // same inherited byte as its finite current-ammo value. Buildings do not
    // carry the FootClass flag and bypass this prefilter.
    let enemy_sensed_prefilter = facts.enemy_sensed_prefilter;
    let discovery_observed = full_visibility
        || local_owner.is_none()
        || local_owner.is_some_and(|local_owner| {
            current_cell.is_some_and(|(rx, ry)| fog.is_cell_revealed(local_owner, rx, ry))
        });
    // Both bodies test the Location (`+0x9C`), not GetCoords: mobile
    // `0x0070D30E..0x0070D34B`, Building `0x00457117..0x0045714B`.
    let location = facts.location();
    let shrouded = !full_visibility
        && local_owner
            .is_some_and(|local_owner| radar_point_shrouded(fog, cells, local_owner, location));
    let both_corners_shrouded = !full_visibility
        && local_owner.is_some_and(|local_owner| {
            foundation.is_some_and(|foundation| {
                radar_building_both_corners_shrouded(fog, cells, local_owner, location, foundation)
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
                    alliances.is_allied(interner, local_owner, entity.owner())
                })
        });
    let effective_type_invisible =
        object.is_some_and(|object| object.invisible || object.invisible_in_game);
    let fresh_in_playfield =
        radar_fresh_mode_one_membership(entity, playfield_bounds, resolved_terrain);
    let cloak_state = entity.cloak_state();
    let veteran_radar_invisible = object.is_some_and(|object| {
        has_weapon_ability(
            entity.veterancy().rank_kind(),
            object,
            Ability::RadarInvisible,
        )
    });
    let visibility = if foundation.is_some() {
        RadarRegistrationVisibilityFacts::Building(RadarBuildingVisibilityFacts {
            type_invisible: effective_type_invisible,
            in_limbo: entity.lifecycle().in_limbo(),
            owner_is_human_player,
            fresh_in_playfield,
            both_corners_shrouded,
            cloak_state,
            building_cloak_stage: entity.building_cloak_stage(),
            fogged: !full_visibility
                && local_owner.is_some_and(|local| fog.is_object_fogged(local, entity.stable_id())),
            has_sensor,
            allied_with_current_player,
        })
    } else {
        RadarRegistrationVisibilityFacts::Mobile(RadarMobileVisibilityFacts {
            type_invisible: effective_type_invisible,
            sinking: entity.sinking(),
            object_alive: entity.lifecycle().object_alive(),
            in_limbo: entity.lifecycle().in_limbo(),
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
        #[cfg(feature = "ares")]
        sensor_array_warn: object.is_none_or(|object| object.sensor_array_warn()),
        foundation,
        radar_scale: projection.cell_axis_scale(),
        discovery_observed,
        visibility,
        // AddObjectToTracker @ 0x00655560 compares directly to g_PlayerPtr,
        // narrower than IsHumanPlayer's single-player shroud exception.
        local_front: local_owner == Some(entity.owner()),
        surface_edge: projection.native_surface.map(|surface| {
            let mut edge = super::tracker::RadarSurfaceEdge {
                size: surface.generated_size(),
                in_playfield_origin: None,
            };
            if (!edge.contains(origin) || foundation.is_some())
                && radar_edge_cell_in_playfield(facts, cells, playfield_bounds)
            {
                edge.in_playfield_origin = Some(surface.world_to_surface_pixel(raw_x, raw_y, true));
            }
            edge
        }),
    }
}

/// `0x0070DB0A..0x0070DB21`: `MapClass::Is_Cell_In_Playfield @ 0x00578540`
/// (mode 1) on the object's GetCell (vt+0x1C4,
/// [`crate::sim::movement::ground_pose::entity_navigation_cell_identity`]). Without
/// the map the object stays off the radar, as it did before this step.
fn radar_edge_cell_in_playfield(
    facts: &RadarFacts,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    bounds: Option<crate::map::playfield::PlayfieldBounds>,
) -> bool {
    cells.is_some_and(|cells| {
        facts.navigation_cell.is_some_and(|cell| {
            crate::sim::cell_rect::retained_cell_is_in_playfield(cell, bounds, cells)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::map::tube_facts::TubeId;
    use crate::sim::ammo::TechnoAmmo;
    use crate::sim::components::DriveCoord;
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
            fogged: false,
            has_sensor: false,
            allied_with_current_player: false,
        }
    }

    #[test]
    fn enemy_sensed_prefilter_maps_native_foot_signed_684() {
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "Soviet", 4, 5);
        assert!(enemy_sensed_prefilter(&entity));
        entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState::for_test(
            TubeId(2),
            0,
            DriveCoord { x: 0, y: 0, z: 0 },
        ));
        assert!(!enemy_sensed_prefilter(&entity));

        entity.set_category_for_test(EntityCategory::Aircraft);
        entity.low_bridge_tube_state = None;
        entity.install_ammo_for_test(Some(TechnoAmmo::new(3)));
        assert!(!enemy_sensed_prefilter(&entity));
        entity.install_ammo_for_test(None);
        assert!(
            enemy_sensed_prefilter(&entity),
            "Ammo=-1 keeps the signed byte negative"
        );

        entity.set_category_for_test(EntityCategory::Structure);
        entity.install_ammo_for_test(Some(TechnoAmmo::new(3)));
        assert!(
            enemy_sensed_prefilter(&entity),
            "Building lacks AbstractFlags IsFoot"
        );
    }

    #[test]
    fn enemy_sensed_source_uses_raw_building_anchor_not_visibility_center() {
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "BLDG", "Enemy", 4, 5);
        entity.set_category_for_test(EntityCategory::Structure);
        entity
            .type_facts
            .set_foundation_for_test("3x2".to_string().into());
        let raw = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
        let visibility = crate::sim::movement::ground_pose::object_center_xy(&entity);
        assert_eq!(radar_fog_cell_from_leptons(raw[0], raw[1]), Some((4, 5)));
        assert_ne!(
            radar_fog_cell_from_leptons(visibility[0], visibility[1]),
            Some((4, 5)),
            "BuildingClass +0x324 may center its query, but 0x70DAD7 packs Object+0x9C"
        );
    }

    #[test]
    fn radar_object_update_uses_native_world_projection_shared_with_cell_events() {
        let surface = crate::render::radar::geometry::NativeRadarSurfaceGeometry::from_raw_rect(
            0, 0, 300, 180,
        )
        .expect("wide generated surface");
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "Enemy", 120, 30);
        entity
            .position
            .set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(0));
        entity
            .position
            .set_sub_y_for_test(crate::util::fixed_math::SimFixed::from_num(0));
        let mut projection = visibility_projection();
        projection.native_surface = Some(surface);
        let update = build_radar_object_update(
            &crate::app::presentation::frame::ObjectRecord::of_test(&entity),
            &crate::sim::houses::Houses::default(),
            None,
            &FogState::default(),
            &Default::default(),
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
        let id = sim.substrate_mut_for_test().allocate_stable_id();
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(id, "TEST", "Americans", 4, 5);
        entity.set_type_ref_for_test(sim.intern("TEST"));
        entity.set_owner_for_test(sim.intern("Americans"));
        entity.set_category_for_test(EntityCategory::Unit);
        entity.lifecycle.set_in_limbo_for_test(false);
        sim.entities_mut_for_test().insert(entity);
        let update = |sim: &crate::sim::world::Simulation| {
            build_radar_object_update(
                crate::app::presentation::frame::PresentationFrame::of_test(sim, &rules)
                    .object(id)
                    .unwrap(),
                sim.houses(),
                None,
                sim.fog(),
                sim.house_alliances(),
                true,
                false,
                Some(&rules),
                Some(sim.interner()),
                visibility_projection(),
                None,
                None,
            )
        };
        let mut tracker = super::super::tracker::RetainedRadarTracker::default();
        tracker.update_object(update(&sim), false);
        assert!(tracker.is_registered(id));
        sim.begin_ship_sinking(id, &rules, crate::sim::world::FrameEffects::default());
        assert!(sim.entities().get(id).unwrap().lifecycle.object_alive());
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
        entity.set_in_playfield(false);
        assert!(radar_fresh_mode_one_membership(
            &crate::app::presentation::frame::ObjectRecord::of_test(&entity),
            Some(bounds),
            Some(&terrain),
        ));
        entity.set_in_playfield(true);
        entity.position.set_rx_for_test(2);
        entity.position.set_ry_for_test(2);
        assert!(!radar_fresh_mode_one_membership(
            &crate::app::presentation::frame::ObjectRecord::of_test(&entity),
            Some(bounds),
            Some(&terrain),
        ));
        assert!(!radar_fresh_mode_one_membership(
            &crate::app::presentation::frame::ObjectRecord::of_test(&entity),
            Some(bounds),
            None,
        ));
    }

    /// A unit centred on the playfield's bottom row projects one pixel past
    /// the generated surface (`0x006557F0`, clamp 0). TechnoClass+0x4A0
    /// re-projects it with clamping while its cell is in the playfield
    /// (`0x0070DADC..0x0070DB64`), so its dot sits on the surface edge.
    #[test]
    fn a_unit_on_the_bottom_playfield_row_keeps_its_radar_dot_on_the_edge() {
        let bounds = crate::map::playfield::PlayfieldBounds {
            base: 16,
            off_fc: 2,
            off_100: 2,
            off_104: 12,
            off_108: 8,
        };
        let terrain = flat_terrain(32);
        let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(&terrain);
        let surface = crate::render::radar::geometry::NativeRadarSurfaceGeometry::from_playfield(
            &terrain, bounds,
        )
        .expect("flat playfield surface");
        let (rx, ry) = terrain
            .iter()
            .filter(|cell| {
                bounds.contains_height_aware_packed(
                    i32::from(cell.rx),
                    i32::from(cell.ry),
                    cell.level as i8,
                    cell.slope_type,
                )
            })
            .map(|cell| (cell.rx, cell.ry))
            .max_by_key(|&(rx, ry)| (rx + ry, rx))
            .expect("a playfield cell");
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "Enemy", rx, ry);
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        let mut projection = visibility_projection();
        projection.native_surface = Some(surface);
        let update = build_radar_object_update(
            &crate::app::presentation::frame::ObjectRecord::of_test_in(
                &entity,
                Some(&terrain),
                crate::rules::ruleset::RuleSet::empty_for_test(),
            ),
            &crate::sim::houses::Houses::default(),
            None,
            &FogState::default(),
            &Default::default(),
            true,
            false,
            None,
            None,
            projection,
            Some(bounds),
            Some(&cells),
        );
        let (width, height) = surface.generated_size();
        assert_eq!(update.origin.1, height, "one pixel past the surface");
        assert!((0..width).contains(&update.origin.0));
        let mut tracker = super::super::tracker::RetainedRadarTracker::default();
        tracker.update_object(update, false);
        assert_eq!(tracker.entries_at(update.origin.0, height - 1), vec![1]);
    }

    #[test]
    fn radar_visibility_building_shroud_samples_origin_and_far_corner() {
        let local = crate::sim::intern::test_intern("Local");
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "BLDG", "Enemy", 4, 4);
        entity
            .type_facts
            .set_foundation_for_test("3x2".to_string().into());
        let location = crate::sim::movement::ground_pose::object_location(&entity, None);
        let mut fog = FogState::default();
        fog.mark_visible_for_owner(local, 6, 5);
        assert!(!radar_building_both_corners_shrouded(
            &fog,
            None,
            local,
            location,
            (3, 2),
        ));
        let fog = FogState::default();
        assert!(radar_building_both_corners_shrouded(
            &fog,
            None,
            local,
            location,
            (3, 2),
        ));
    }

    /// `0x00457170`: an AI building the local player sees only as a fogged
    /// image (+0x6E7, set by the fogged-object constructor `0x004D10D5`)
    /// needs a sensor to register on the radar, like a shrouded one.
    #[test]
    fn radar_visibility_a_fogged_ai_building_needs_a_sensor() {
        let local = crate::sim::intern::test_intern("Local");
        let mut building =
            crate::sim::game_entity::GameEntity::test_default(2, "BLDG", "Enemy", 10, 10);
        building.set_category_for_test(EntityCategory::Structure);
        building
            .type_facts
            .set_foundation_for_test("2x2".to_string().into());
        building.lifecycle.set_in_limbo_for_test(false);
        let mut fog = FogState::default();
        fog.mark_visible_for_owner(local, 10, 10);
        let visible = |fog: &FogState| {
            build_radar_object_update(
                &crate::app::presentation::frame::ObjectRecord::of_test(&building),
                &crate::sim::houses::Houses::default(),
                Some(local),
                fog,
                &Default::default(),
                false,
                true,
                None,
                None,
                visibility_projection(),
                None,
                None,
            )
            .visibility
            .evaluate(false)
            .visible
        };
        assert!(visible(&fog));
        fog.set_object_fogged_for_test(local, 2, true);
        assert!(!visible(&fog));
    }

    /// `0x0070D30E..0x0070D34B` and `0x00457117..0x0045714B` ask
    /// `MapClass::IsShrouded @ 0x00586360` at the Location, which raises the
    /// point by its height. On level-4 ground an enemy at (10, 10) is tested
    /// at (8, 8), and a 2x2 building's far corner (11, 11) at (9, 9).
    #[test]
    fn radar_shroud_is_tested_at_the_height_raised_location() {
        let local = crate::sim::intern::test_intern("Local");
        let terrain =
            crate::map::resolved_terrain::test_grid(32, 32, |rx, ry| ResolvedTerrainCell {
                level: 4,
                ..flat_cell(rx, ry)
            });
        let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(&terrain);
        let visible = |entity: &crate::sim::game_entity::GameEntity, revealed: &[(u16, u16)]| {
            let mut fog = FogState::default();
            for &(rx, ry) in revealed {
                fog.mark_visible_for_owner(local, rx, ry);
            }
            build_radar_object_update(
                &crate::app::presentation::frame::ObjectRecord::of_test_in(
                    entity,
                    Some(&terrain),
                    crate::rules::ruleset::RuleSet::empty_for_test(),
                ),
                &crate::sim::houses::Houses::default(),
                Some(local),
                &fog,
                &Default::default(),
                false,
                true,
                None,
                None,
                visibility_projection(),
                None,
                Some(&cells),
            )
            .visibility
            .evaluate(false)
            .visible
        };

        let mut unit = crate::sim::game_entity::GameEntity::test_default(1, "DOT", "Enemy", 10, 10);
        // Standing on its ground: GetHeight (`vt+0x1C8`) is zero.
        unit.position.set_z_for_test(4);
        unit.lifecycle.set_in_limbo_for_test(false);
        assert!(
            !visible(&unit, &[(10, 10)]),
            "mapping its own cell leaves a raised unit shrouded"
        );
        assert!(visible(&unit, &[(8, 8)]));

        let mut building =
            crate::sim::game_entity::GameEntity::test_default(2, "BLDG", "Enemy", 10, 10);
        building.set_category_for_test(EntityCategory::Structure);
        building
            .type_facts
            .set_foundation_for_test("2x2".to_string().into());
        building.lifecycle.set_in_limbo_for_test(false);
        assert!(!visible(&building, &[(10, 10), (11, 11)]));
        assert!(visible(&building, &[(9, 9)]), "the raised far corner");
    }

    /// The radar asks from the current player's house whether an object's
    /// owner is its ally (`0x0070D394`): the owner's own one-way alliance with
    /// the player does not count.
    #[test]
    fn radar_alliance_is_the_current_players_ask() {
        let local = crate::sim::intern::test_intern("Local");
        let enemy = crate::sim::intern::test_intern("Enemy");
        crate::sim::intern::test_intern("DOT");
        let interner = crate::sim::intern::test_interner();
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=DOT\n[DOT]\nStrength=100\n",
        ))
        .expect("visibility fixture rules");
        let mut houses = crate::sim::houses::Houses::default();
        houses.register(crate::sim::house_state::HouseState::new(
            enemy, 0, None, false, 0, 10,
        ));
        let fog = FogState::default();
        let mut entity = crate::sim::game_entity::GameEntity::test_default(1, "DOT", "Enemy", 5, 5);
        entity.lifecycle.set_in_limbo_for_test(false);
        let allied = |asker: &str, other: &str| {
            let alliances = crate::map::houses::HouseAllianceMap::from([(
                asker.to_ascii_uppercase(),
                [other.to_ascii_uppercase()].into(),
            )]);
            let update = build_radar_object_update(
                &crate::app::presentation::frame::ObjectRecord::of_test(&entity),
                &houses,
                Some(local),
                &fog,
                &super::super::helpers::test_alliances(alliances),
                false,
                true,
                Some(&rules),
                Some(&interner),
                visibility_projection(),
                None,
                None,
            );
            match update.visibility {
                RadarRegistrationVisibilityFacts::Mobile(facts) => facts.allied_with_current_player,
                RadarRegistrationVisibilityFacts::Building(_) => unreachable!("DOT is a vehicle"),
            }
        };
        assert!(allied("Local", "Enemy"));
        assert!(!allied("Enemy", "Local"));
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
        let mut houses = crate::sim::houses::Houses::default();
        houses.register(crate::sim::house_state::HouseState::new(
            enemy, 0, None, false, 0, 10,
        ));
        let mut fog = FogState::default();
        fog.mark_visible_for_owner(local, 5, 5);
        let mut entity = crate::sim::game_entity::GameEntity::test_default(1, "DOT", "Enemy", 5, 5);
        entity.lifecycle.set_in_limbo_for_test(false);
        let update = build_radar_object_update(
            &crate::app::presentation::frame::ObjectRecord::of_test(&entity),
            &houses,
            Some(local),
            &fog,
            &Default::default(),
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

/// What the radar reads of one object that its record does not carry,
/// captured into the record (`app::presentation::frame_scene::SceneFacts`)
/// by the functions the radar asked of the object.
#[derive(Clone, Debug, Default)]
pub(crate) struct RadarFacts {
    /// [`enemy_sensed_prefilter`].
    enemy_sensed_prefilter: bool,
    /// A Structure's [`parse_foundation_size`] of its type's Foundation.
    foundation: Option<(u32, u32)>,
    /// `ground_pose::object_location` (Location `+0x9C`) on the map.
    location: [i32; 3],
    /// `ground_pose::entity_navigation_cell_identity` (GetCell, vt+0x1C4) on
    /// the map; `None` without the map or when it fails.
    navigation_cell: Option<crate::map::cell_index::NativeCellIdentity>,
    /// `radar_combat_flash.remaining` at the committed binary frame.
    pub(super) combat_flash_remaining: i32,
    /// `GameEntity::disguise_house(false, local player, allied)` for
    /// `allied` false and true.
    pub(super) disguise_house: [Option<InternedId>; 2],
    /// A radar click's cell words for it (`0x006567EF..0x00656828`):
    /// `Simulation::navigation_target_coordinate` with no requester,
    /// truncated and packed.
    pub(super) click_cell: Option<(i16, i16)>,
}

impl RadarFacts {
    /// Captured with the live world: `cells` is the capture's isolated
    /// query of the map, `observer` the local player.
    pub(crate) fn capture(
        entity: &crate::sim::game_entity::GameEntity,
        sim: &crate::sim::world::Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
        binary_frame: u32,
        observer: Option<InternedId>,
    ) -> Self {
        let terrain = sim.resolved_terrain();
        let location = crate::sim::movement::ground_pose::object_location(entity, terrain);
        Self {
            enemy_sensed_prefilter: enemy_sensed_prefilter(entity),
            foundation: (entity.category() == EntityCategory::Structure)
                .then(|| parse_foundation_size(entity.type_facts.foundation())),
            location: [location.x, location.y, location.z],
            navigation_cell: cells.and_then(|cells| {
                crate::sim::movement::ground_pose::entity_navigation_cell_identity(
                    entity,
                    cells,
                    Some((rules, sim.interner())),
                )
                .ok()
            }),
            combat_flash_remaining: entity.radar_combat_flash.remaining(binary_frame as i32),
            disguise_house: [false, true]
                .map(|allied| entity.disguise_house(false, observer, allied)),
            click_cell: sim
                .navigation_target_coordinate(
                    crate::sim::components::NavTargetRef::object(entity.stable_id()),
                    None,
                    rules,
                )
                .ok()
                .map(|point| {
                    (
                        crate::util::lepton::lepton_to_cell_packed(point.x),
                        crate::util::lepton::lepton_to_cell_packed(point.y),
                    )
                }),
        }
    }

    fn location(&self) -> crate::sim::components::DriveCoord {
        let [x, y, z] = self.location;
        crate::sim::components::DriveCoord { x, y, z }
    }
}

/// What the radar reads of the objects beside any one object's record
/// (`app::presentation::frame_scene::SceneFrame`).
#[derive(Clone, Debug, Default)]
pub(crate) struct RadarFrame {}
