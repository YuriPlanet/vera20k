//! Minimap click inverse and camera-window presentation adapters.

use crate::render::batch::SpriteInstance;

use super::minimap::MinimapRenderer;
use super::projection::minimap_screen_point_to_camera_top_left;
use super::viewport::{
    NativeRadarScreenGeometry, native_content_boundary_outline_instances,
    native_viewport_outline_instances, native_viewport_rect,
};

/// Exact result of the native radar click's
/// `MapClass::Get_CellClass -> CellClass::Get_Center_Coords` tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeRadarCameraTarget {
    pub(crate) cell: (u16, u16),
    pub(crate) world_leptons: (i32, i32, i32),
    pub(crate) tracker_hit: Option<u64>,
}

/// The signed-word clamp in the live RadarClass input owner
/// `0x00653D7A..0x00653E66`.
///
/// The four corrections deliberately are not collapsed to `clamp`: native
/// adjusts both packed cell words by the complete delta, in this exact order,
/// and each write wraps as i16 before the next signed comparison.
fn clamp_native_radar_click_cell(
    cell: (i16, i16),
    map_size: (i32, i32),
    tactical_size: (i32, i32),
) -> (i16, i16) {
    let horizontal_cells = tactical_size.0 / 60;
    let horizontal_half = horizontal_cells.wrapping_add(2) / 2;
    let difference_limit = map_size.0.wrapping_sub(horizontal_half).wrapping_sub(1);
    let vertical_cells = tactical_size.1 / 60;
    let minimum_sum = vertical_cells.wrapping_add(map_size.0).wrapping_add(1);
    let maximum_sum = map_size
        .1
        .wrapping_mul(2)
        .wrapping_sub(vertical_cells)
        .wrapping_add(map_size.0)
        .wrapping_sub(1);
    let (mut x, mut y) = cell;

    let y_minus_x = i32::from(y).wrapping_sub(i32::from(x));
    if y_minus_x > difference_limit {
        let delta = y_minus_x.wrapping_sub(difference_limit) as i16;
        y = y.wrapping_sub(delta);
        x = x.wrapping_add(delta);
    }

    let x_minus_y = i32::from(x).wrapping_sub(i32::from(y));
    if x_minus_y > difference_limit.wrapping_sub(1) {
        let delta = x_minus_y.wrapping_sub(difference_limit).wrapping_add(1) as i16;
        y = y.wrapping_add(delta);
        x = x.wrapping_sub(delta);
    }

    let sum = i32::from(x).wrapping_add(i32::from(y));
    if sum < minimum_sum {
        let delta = minimum_sum
            .wrapping_sub(i32::from(y))
            .wrapping_sub(i32::from(x)) as i16;
        x = x.wrapping_add(delta);
        y = y.wrapping_add(delta);
    }

    let sum = i32::from(x).wrapping_add(i32::from(y));
    if sum > maximum_sum {
        let delta = sum.wrapping_sub(maximum_sum) as i16;
        x = x.wrapping_sub(delta);
        y = y.wrapping_sub(delta);
    }
    (x, y)
}

fn signed_tracker_or_inverse_cell(
    surface: super::geometry::NativeRadarSurfaceGeometry,
    pixel: (i32, i32),
    tracker_cell: Option<(i16, i16)>,
) -> (i16, i16) {
    tracker_cell.unwrap_or_else(|| surface.surface_pixel_to_signed_cell(pixel))
}

fn tracker_object_signed_cell(
    tracker: &super::tracker::RetainedRadarTracker,
    simulation: &crate::sim::world::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    pixel: (i32, i32),
) -> Option<(u64, (i16, i16))> {
    let stable_id = tracker.object_at_pixel(pixel.0, pixel.1)?;
    // Original6567EF..656828 dispatches virtual+4C with a null requester,
    // then truncates signed XY toward zero and packs each low word. A moving
    // Foot's navigation head can precede its physical Object+48 cell. Do not
    // perform a Map lookup here: the handler clamps these words first.
    let point = simulation
        .navigation_target_coordinate(
            crate::sim::components::NavTargetRef::object(stable_id),
            None,
            rules,
        )
        .ok()?;
    Some((
        stable_id,
        (
            crate::util::lepton::lepton_to_cell_packed(point.x),
            crate::util::lepton::lepton_to_cell_packed(point.y),
        ),
    ))
}

fn native_camera_cell_from_signed(
    signed: (i16, i16),
    map_size: (i32, i32),
    tactical_size: (i32, i32),
) -> (u16, u16) {
    let clamped = clamp_native_radar_click_cell(signed, map_size, tactical_size);
    (clamped.0 as u16, clamped.1 as u16)
}

fn native_camera_target_from_signed(
    signed: (i16, i16),
    map_size: (i32, i32),
    tactical_size: (i32, i32),
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
) -> Option<NativeRadarCameraTarget> {
    let (rx, ry) = native_camera_cell_from_signed(signed, map_size, tactical_size);
    let cell = terrain.cell(rx, ry)?;
    // `CellClass::Get_Center_Coords @ 0x00480A30` reads the cell object's
    // signed packed coordinate, forms X/Y at subcell (128,128), and asks
    // `0x0047B3A0` for ground Z. It does not add bridge-deck height.
    let point = crate::sim::cell_rect::CellRef::Real(cell).center_coordinate()?;
    Some(NativeRadarCameraTarget {
        cell: (cell.rx, cell.ry),
        world_leptons: (point.x, point.y, point.z),
        tracker_hit: None,
    })
}

impl MinimapRenderer {
    /// Return true only inside the generated primary when native authority is
    /// installed. Native `0x006539D0` rejects the centered letterbox margins.
    pub fn contains_screen_point_in_rect(
        &self,
        screen_x: f32,
        screen_y: f32,
        rect_x: f32,
        rect_y: f32,
        rect_w: f32,
        rect_h: f32,
    ) -> bool {
        if let Some(surface) = self.native_radar_surface {
            return NativeRadarScreenGeometry::new(surface, [rect_x, rect_y, rect_w, rect_h])
                .screen_to_surface_pixel((screen_x, screen_y))
                .is_some();
        }
        if self.playfield_bounds.is_some() {
            return false;
        }
        screen_x >= rect_x
            && screen_x < rect_x + rect_w
            && screen_y >= rect_y
            && screen_y < rect_y + rect_h
    }

    /// Screen-space generated-primary rectangle. `None` under configured
    /// MapClass authority means fail closed; only mapless fixtures retain the
    /// full-aperture adapter.
    pub(crate) fn content_screen_rect_in_rect(
        &self,
        rect_x: f32,
        rect_y: f32,
        rect_w: f32,
        rect_h: f32,
    ) -> Option<[f32; 4]> {
        self.native_radar_surface
            .map(|surface| {
                NativeRadarScreenGeometry::new(surface, [rect_x, rect_y, rect_w, rect_h])
                    .content_rect()
            })
            .or_else(|| {
                self.playfield_bounds
                    .is_none()
                    .then_some([rect_x, rect_y, rect_w, rect_h])
            })
    }

    /// `RadarClass::GetObjectAtRadarPixel @ 0x00656750`: screen to generated
    /// pixel, reverse tracker lookup, then exact x87 cell inverse fallback.
    pub(crate) fn native_click_target_in_rect(
        &self,
        screen_x: f32,
        screen_y: f32,
        rect_x: f32,
        rect_y: f32,
        rect_w: f32,
        rect_h: f32,
        simulation: &crate::sim::world::Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
        map_size: (i32, i32),
        tactical_size: (i32, i32),
    ) -> Option<NativeRadarCameraTarget> {
        let surface = self.native_radar_surface?;
        let pixel = NativeRadarScreenGeometry::new(surface, [rect_x, rect_y, rect_w, rect_h])
            .screen_to_surface_pixel((screen_x, screen_y))?;
        let hit = tracker_object_signed_cell(&self.radar_tracker, simulation, rules, pixel);
        let signed = signed_tracker_or_inverse_cell(surface, pixel, hit.map(|(_, cell)| cell));
        let mut target =
            native_camera_target_from_signed(signed, map_size, tactical_size, terrain)?;
        target.tracker_hit = hit.map(|(id, _)| id);
        Some(target)
    }

    /// Resolve an ordinary input point from the installed tracker, including
    /// reverse-hit overlap authority. This reads no raw simulation coordinate.
    pub(crate) fn native_object_click_point_in_rect(
        &self,
        id: u64,
        rect: [f32; 4],
    ) -> Option<(f32, f32)> {
        let geometry = NativeRadarScreenGeometry::new(self.native_radar_surface?, rect);
        let pixel = self.radar_tracker.clickable_pixel(id, |pixel| {
            geometry.surface_pixel_screen_point(pixel).is_some()
        })?;
        geometry.surface_pixel_screen_point(pixel)
    }

    pub(crate) const fn has_playfield_authority(&self) -> bool {
        self.playfield_bounds.is_some()
    }

    /// Mapless/headless adapter retained only when no MapClass authority exists.
    pub fn camera_top_left_for_screen_point_in_rect(
        &self,
        screen_x: f32,
        screen_y: f32,
        screen_w: f32,
        screen_h: f32,
        rect_x: f32,
        rect_y: f32,
        rect_w: f32,
        rect_h: f32,
    ) -> (f32, f32) {
        minimap_screen_point_to_camera_top_left(
            screen_x,
            screen_y,
            screen_w,
            screen_h,
            rect_x,
            rect_y,
            rect_w,
            rect_h,
            self.world_origin_x,
            self.world_origin_y,
            self.world_width,
            self.world_height,
            self.map_offset_x,
            self.map_offset_y,
            self.map_pixel_w,
            self.map_pixel_h,
        )
    }

    /// Build the native camera-window rectangle after the generated content
    /// copy. The active side's exact sidebar text color is supplied by app
    /// composition; each edge is one generated-surface pixel thick and clips
    /// against the complete retained sidebar surface, not the radar aperture.
    pub fn build_viewport_rect_in_rect(
        &mut self,
        camera_x: f32,
        camera_y: f32,
        center_cell: (u16, u16),
        tactical_width: i32,
        tactical_height: i32,
        rect_x: f32,
        rect_y: f32,
        rect_w: f32,
        rect_h: f32,
        sidebar_surface: [f32; 4],
        sidebar_color: [f32; 3],
    ) -> Vec<SpriteInstance> {
        let Some(surface) = self.native_radar_surface else {
            // No rectangular fallback is permitted once MapClass authority is
            // configured. Mapless renderer fixtures have no native geometry.
            return Vec::new();
        };
        let rect = native_viewport_rect(surface, center_cell, tactical_width, tactical_height);
        let transition = self.viewport_state.update(rect);
        let _native_old_border_visits = transition.dirty_previous_border;
        native_viewport_outline_instances(
            (camera_x, camera_y),
            NativeRadarScreenGeometry::new(surface, [rect_x, rect_y, rect_w, rect_h]),
            transition.current,
            sidebar_surface,
            sidebar_color,
        )
    }

    /// Build the generated-content boundary that native submits after the
    /// camera window at `RadarClass::Update @ 0x00657669..0x006576A2`.
    /// Absence of the generated MapClass-owned surface fails closed; there is
    /// no legacy rectangular substitute on the parity path.
    pub fn build_content_boundary_in_rect(
        &self,
        camera_x: f32,
        camera_y: f32,
        rect_x: f32,
        rect_y: f32,
        rect_w: f32,
        rect_h: f32,
        sidebar_surface: [f32; 4],
        sidebar_color: [f32; 3],
    ) -> Vec<SpriteInstance> {
        let Some(surface) = self.native_radar_surface else {
            return Vec::new();
        };
        native_content_boundary_outline_instances(
            (camera_x, camera_y),
            NativeRadarScreenGeometry::new(surface, [rect_x, rect_y, rect_w, rect_h]),
            surface,
            sidebar_surface,
            sidebar_color,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::radar::geometry::NativeRadarSurfaceGeometry;
    use crate::render::radar::tracker::{RadarObjectUpdate, RetainedRadarTracker};
    use crate::render::radar::visibility::{
        RadarMobileVisibilityFacts, RadarRegistrationVisibilityFacts,
    };
    use crate::sim::intern::test_intern;

    fn tracker_update(stable_id: u64, foundation: Option<(u32, u32)>) -> RadarObjectUpdate {
        RadarObjectUpdate {
            stable_id,
            owner: test_intern("Enemy"),
            origin: (40, 60),
            event_source_cell: None,
            enemy_sensed_prefilter: true,
            foundation,
            radar_scale: 1.0,
            discovery_observed: true,
            visibility: RadarRegistrationVisibilityFacts::Mobile(RadarMobileVisibilityFacts {
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
            }),
            local_front: false,
        }
    }

    fn terrain_cell(rx: u16, ry: u16) -> crate::map::resolved_terrain::ResolvedTerrainCell {
        crate::map::resolved_terrain::ResolvedTerrainCell {
            ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
        }
    }

    fn click_terrain(width: u16, height: u16) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(width, height, terrain_cell)
    }

    #[test]
    fn native_click_clamp_preserves_signed_words_and_strict_branch_equality() {
        let map = (100, 100);
        let tactical = (632, 570);
        assert_eq!(
            clamp_native_radar_click_cell((50, 143), map, tactical),
            (50, 143)
        );
        assert_eq!(
            clamp_native_radar_click_cell((50, 144), map, tactical),
            (51, 143)
        );
        assert_eq!(
            clamp_native_radar_click_cell((142, 50), map, tactical),
            (142, 50)
        );
        assert_eq!(
            clamp_native_radar_click_cell((143, 50), map, tactical),
            (142, 51)
        );
        assert_eq!(
            clamp_native_radar_click_cell((55, 55), map, tactical),
            (55, 55)
        );
        assert_eq!(
            clamp_native_radar_click_cell((54, 55), map, tactical),
            (55, 56)
        );
        assert_eq!(
            clamp_native_radar_click_cell((145, 145), map, tactical),
            (145, 145)
        );
        assert_eq!(
            clamp_native_radar_click_cell((145, 146), map, tactical),
            (144, 145)
        );
    }

    #[test]
    fn native_click_target_samples_all_active_slopes_at_cell_center() {
        let mut terrain = click_terrain(64, 64);
        let expected_contributions = [
            0, 52, 52, 52, 52, 0, 0, 0, 0, 104, 104, 104, 104, 104, 104, 104, 104, 52, 52, 52, 52,
        ];
        for (slope, contribution) in expected_contributions.into_iter().enumerate() {
            let cell = terrain.cell_mut(55, 55).unwrap();
            cell.level = 2;
            cell.slope_type = slope as u8;
            let target =
                native_camera_target_from_signed((55, 55), (100, 100), (632, 570), &terrain)
                    .unwrap();
            assert_eq!(target.cell, (55, 55));
            assert_eq!(target.world_leptons, (14_208, 14_208, 208 + contribution));
        }
    }

    #[test]
    fn native_click_target_uses_ground_slope_for_bridge_and_fails_closed_without_cell() {
        let mut terrain = click_terrain(64, 64);
        let cell = terrain.cell_mut(63, 63).unwrap();
        cell.level = 3;
        cell.slope_type = 1;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_deck_level = 7;
        let target =
            native_camera_target_from_signed((63, 63), (100, 100), (632, 570), &terrain).unwrap();
        assert_eq!(
            target.cell,
            (63, 63),
            "last real grid cell remains available"
        );
        assert_eq!(target.world_leptons, (16_256, 16_256, 364));

        assert_eq!(
            native_camera_target_from_signed((64, 64), (100, 100), (632, 570), &terrain,),
            None,
            "this caller still fails closed instead of routing through the existing shared dummy",
        );
        terrain.test_set_native_allocated_cells(&[]);
        assert_eq!(
            native_camera_target_from_signed((63, 63), (100, 100), (632, 570), &terrain,),
            None,
            "a Size-diamond hole is not a rectangular/level-zero fallback",
        );
    }

    #[test]
    fn native_click_tracker_and_empty_hit_share_the_same_cellclass_center() {
        let terrain = click_terrain(64, 64);
        let mut tracker = RetainedRadarTracker::default();
        tracker.update_object(tracker_update(1, None), false);
        let mut simulation = crate::sim::world::Simulation::new();
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "Enemy", 55, 55);
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        simulation.entities_mut().insert(entity);
        let rules = crate::rules::ruleset::RuleSet::from_ini(
            &crate::rules::ini_parser::IniFile::from_str("[General]\n"),
        )
        .unwrap();
        let tracker_signed = tracker_object_signed_cell(&tracker, &simulation, &rules, (40, 60))
            .unwrap()
            .1;
        assert_eq!(tracker_signed, (55, 55));
        let tracker_target =
            native_camera_target_from_signed(tracker_signed, (100, 100), (632, 570), &terrain);
        let empty_target =
            native_camera_target_from_signed((55, 55), (100, 100), (632, 570), &terrain);
        assert_eq!(tracker_target, empty_target);
    }

    #[test]
    #[ignore = "requires retail assets; ordinary moving AMCV radar hit must use its navigation cell"]
    fn retail_moving_radar_hit_uses_navigation_cell_before_camera_center() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::world::{FrameEffects, TickLane};
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

        let root = std::path::PathBuf::from(std::env::var_os("RA2_DIR").unwrap());
        let mut launch = crate::headless_scenario::battle_session(
            "GoldSt.mmx",
            &[(LaunchCountry::France, AiDifficulty::Easy)],
        );
        launch.local_player_mut().name = "VERA-OBSERVER".into();
        launch.local_slot_mut().start_position = LaunchStartPosition::Position(3);
        launch.opponents[0].start_position = LaunchStartPosition::Position(0);
        launch.options.unit_count = 0;
        let mut scene =
            crate::headless_scenario::load_session(&root, "GoldSt.mmx", 0x1234_5678, launch)
                .unwrap();
        let owner = scene.sim().session.current_house().unwrap();
        let id = scene
            .sim()
            .entities()
            .iter_sorted()
            .find(|(_, entity)| {
                entity.owner() == owner && scene.sim().resolve(entity.type_ref()) == "AMCV"
            })
            .map(|(id, _)| id)
            .unwrap();
        let command = CommandEnvelope::new(
            owner,
            scene.sim().session.tick() + 1,
            Command::Move {
                entity_id: id,
                target_rx: 111,
                target_ry: 27,
            },
        );
        let packet = crate::test_fixture::json("tools/input_oracle/radar_click.json");
        let row = |name: &str| {
            packet["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["input"]["name"] == name)
                .unwrap()
        };
        let mut moving_checked = false;
        for frame in 0..160 {
            scene
                .runtime
                .advance_frame(
                    if frame == 0 {
                        std::slice::from_ref(&command)
                    } else {
                        &[]
                    },
                    22,
                    TickLane::Ordinary,
                    FrameEffects::default(),
                )
                .unwrap();
            let sim = scene.sim();
            let entity = sim.entities().get(id).unwrap();
            let navigation = sim.foot_navigation_coordinate(id).unwrap();
            let navigation_cell = ((navigation.x / 256) as i16, (navigation.y / 256) as i16);
            if !moving_checked
                && (entity.position.rx(), entity.position.ry()) == (110, 27)
                && navigation_cell == (111, 27)
            {
                // Independent original Move-produced states; compare the event,
                // not the fixture's absolute frame or supplied tracker lifetime.
                assert_native_radar_camera_case(
                    sim,
                    &scene.runtime.resources.rules,
                    id,
                    row("moving_occupied"),
                );
                moving_checked = true;
            }
        }
        assert!(
            moving_checked,
            "ordinary Move must distinguish physical and navigation cells"
        );
        let stop = CommandEnvelope::new(
            owner,
            scene.sim().session.tick() + 1,
            Command::Stop { entity_id: id },
        );
        scene
            .runtime
            .advance_frame(&[stop], 22, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        for _ in 0..3 {
            assert_native_radar_camera_case(
                scene.sim(),
                &scene.runtime.resources.rules,
                id,
                row("stopped_occupied"),
            );
            scene
                .runtime
                .advance_frame(&[], 22, TickLane::Ordinary, FrameEffects::default())
                .unwrap();
        }
        assert_native_radar_camera_case(
            scene.sim(),
            &scene.runtime.resources.rules,
            id,
            row("empty_physical_ramp"),
        );
    }

    fn assert_native_radar_camera_case(
        sim: &crate::sim::world::Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        id: u64,
        row: &serde_json::Value,
    ) {
        let hash = sim.state_hash();
        let input = &row["input"];
        let calls = row["calls"].as_array().unwrap();
        let call = |name: &str| calls.iter().find(|call| call["name"] == name).unwrap();
        let pair = |value: &serde_json::Value| {
            (
                value[0].as_i64().unwrap() as i32,
                value[1].as_i64().unwrap() as i32,
            )
        };
        let pixel = pair(&row["pixel"]);
        let surface = NativeRadarSurfaceGeometry::from_raw_rect(
            input["column_base"].as_i64().unwrap() as i32,
            input["row_base"].as_i64().unwrap() as i32,
            input["image_rect"][2].as_i64().unwrap() as i32,
            input["image_rect"][3].as_i64().unwrap() as i32,
        )
        .unwrap();
        assert_eq!(
            surface.zoom().to_bits(),
            input["zoom_bits"].as_u64().unwrap() as u32
        );
        let mut tracker = RetainedRadarTracker::default();
        if input["tracked"] != false {
            let mut update = tracker_update(id, None);
            update.origin = pixel;
            tracker.update_object(update, false);
        }
        let hit = tracker_object_signed_cell(&tracker, sim, rules, pixel);
        let tracked = hit.map(|(_, cell)| cell);
        let signed = signed_tracker_or_inverse_cell(surface, pixel, tracked);
        let expected = pair(&call("radar_pick")["cell"]);
        assert_eq!(
            (i32::from(signed.0), i32::from(signed.1)),
            expected,
            "{}",
            input["name"]
        );
        let target = native_camera_target_from_signed(
            signed,
            pair(&input["map_size"]),
            pair(&input["viewport"]),
            sim.resolved_terrain.as_ref().unwrap(),
        )
        .unwrap();
        let xyz = &call("cell_center")["xyz"];
        assert_eq!(
            target.world_leptons,
            (
                xyz[0].as_i64().unwrap() as i32,
                xyz[1].as_i64().unwrap() as i32,
                xyz[2].as_i64().unwrap() as i32
            )
        );
        assert_eq!(call("set_view")["xyz"], *xyz);
        let (x, y, z) = target.world_leptons;
        let world = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
        let native_center = pair(&row["after"]["committed"]);
        // VERA's established world-pixel frame has the common row bias;
        // original6D6070/6D8B30 store native pixels without that frame origin.
        let bias = crate::util::lepton::absolute_leptons_to_screen(0, 0, 0);
        assert_eq!(
            world,
            (
                native_center.0 as f32 + bias.0,
                native_center.1 as f32 + bias.1
            )
        );
        let viewport = pair(&input["viewport"]);
        let top_left = crate::app::input::camera::tactical_camera_top_left(
            world,
            viewport.0 as f32,
            viewport.1 as f32,
            1.0,
        );
        let native_top_left = pair(&row["after"]["top_left"]);
        assert_eq!(
            top_left,
            (
                native_top_left.0 as f32 + bias.0,
                native_top_left.1 as f32 + bias.1
            )
        );
        assert_eq!(
            sim.state_hash(),
            hash,
            "radar/camera queries must not mutate simulation"
        );
    }

    #[test]
    fn native_click_cellclass_xyz_uses_the_full_6d6070_camera_projection() {
        // Cell (55,55), Level=2, slope 1 at center: 208 base + 52 slope.
        // Get_Center_Coords 0x00480A30 passes this complete XYZ to 0x006D6070;
        // the latter projects/AdjustForZ before writing current and desired to
        // one identical immediate top-left (represented by Rust's one point).
        let world = crate::util::lepton::absolute_leptons_to_screen(14_208, 14_208, 260);
        assert_eq!(world, (0.0, 1_643.0));
        let camera = crate::app::input::camera::tactical_camera_top_left(world, 856.0, 736.0, 1.0);
        assert_eq!(camera, (-428.0, 1_275.0));
        assert_eq!(((world.0 - camera.0), (world.1 - camera.1)), (428.0, 368.0),);

        let level_only = crate::util::lepton::absolute_leptons_to_screen(14_208, 14_208, 208);
        assert_eq!(level_only, (0.0, 1_650.0));
        assert_ne!(
            world, level_only,
            "the slope contribution cannot be reconstructed from Level"
        );
    }

    #[test]
    fn native_scaled_screen_adapter_keeps_signed_inverse_until_map_clamp() {
        let surface = NativeRadarSurfaceGeometry::from_raw_rect(30, 20, 300, 180).unwrap();
        let screen = NativeRadarScreenGeometry::new(surface, [100.0, 50.0, 280.0, 216.0]);
        let pixel = screen.screen_to_surface_pixel((100.0, 74.0)).unwrap();
        assert_eq!(pixel, (0, 0));
        let signed = signed_tracker_or_inverse_cell(surface, pixel, None);
        assert_eq!(signed, (-4, 25));
        assert_eq!(
            clamp_native_radar_click_cell(signed, (100, 100), (632, 570)),
            (85, 114)
        );
    }

    #[test]
    fn native_click_wide_and_tall_corners_and_edges_clamp_before_packing() {
        let fixtures = [
            (
                NativeRadarSurfaceGeometry::from_raw_rect(30, 20, 300, 180).unwrap(),
                [
                    (0, 0),
                    (139, 0),
                    (0, 82),
                    (139, 82),
                    (70, 0),
                    (70, 82),
                    (0, 41),
                    (139, 41),
                ],
                [
                    ((-4, 25), (85, 114)),
                    ((144, -123), (58, 141)),
                    ((83, 113), (83, 113)),
                    ((232, -35), (57, 140)),
                    ((70, -49), (132, 67)),
                    ((158, 38), (130, 66)),
                    ((39, 69), (41, 71)),
                    ((188, -79), (14, 97)),
                ],
            ),
            (
                NativeRadarSurfaceGeometry::from_raw_rect(30, 20, 180, 300).unwrap(),
                [
                    (0, 0),
                    (63, 0),
                    (0, 107),
                    (63, 107),
                    (32, 0),
                    (32, 107),
                    (0, 54),
                    (63, 54),
                ],
                [
                    ((-4, 25), (85, 114)),
                    ((83, -62), (119, 80)),
                    ((144, 174), (116, 146)),
                    ((231, 86), (151, 112)),
                    ((39, -18), (128, 71)),
                    ((188, 129), (161, 102)),
                    ((70, 100), (70, 100)),
                    ((158, 12), (104, 66)),
                ],
            ),
        ];
        for (surface, pixels, expected) in fixtures {
            let cells = pixels.map(|pixel| {
                let signed = signed_tracker_or_inverse_cell(surface, pixel, None);
                (
                    signed,
                    clamp_native_radar_click_cell(signed, (100, 100), (632, 570)),
                )
            });
            assert_eq!(cells, expected);
        }
    }

    #[test]
    fn clipped_radar_brush_searches_for_a_represented_reverse_hit() {
        let mut tracker = RetainedRadarTracker::default();
        let mut update = tracker_update(7, Some((3, 3)));
        update.origin = (-1, 0);
        tracker.update_object(update, false);
        let geometry = NativeRadarScreenGeometry::new(
            NativeRadarSurfaceGeometry::from_raw_rect(30, 20, 100, 100).unwrap(),
            [0.0, 0.0, 140.0, 108.0],
        );
        assert_eq!(tracker.clickable_pixel(7, |_| true), Some((-1, 0)));
        let pixel = tracker
            .clickable_pixel(7, |pixel| {
                geometry.surface_pixel_screen_point(pixel).is_some()
            })
            .expect("the clipped building still has an on-surface dot");
        assert_eq!(pixel, (0, 1));
        let point = geometry.surface_pixel_screen_point(pixel).unwrap();
        assert_eq!(geometry.screen_to_surface_pixel(point), Some(pixel));
        assert_eq!(tracker.object_at_pixel(pixel.0, pixel.1), Some(7));
    }

    #[test]
    fn native_tracker_reverse_hit_uses_building_navigation_for_camera_cell() {
        let mut tracker = RetainedRadarTracker::default();
        tracker.update_object(tracker_update(1, None), false);
        tracker.update_object(tracker_update(2, Some((4, 3))), false);
        assert_eq!(
            tracker.object_at_pixel(40, 60),
            Some(2),
            "native reverse bucket scan"
        );

        assert_eq!(
            tracker.clickable_pixel(1, |_| true),
            None,
            "the later building covers its only dot"
        );
        let pixel = tracker.clickable_pixel(2, |_| true).unwrap();
        assert_eq!(tracker.object_at_pixel(pixel.0, pixel.1), Some(2));
        let mut simulation = crate::sim::world::Simulation::new();
        let mobile = crate::sim::game_entity::GameEntity::test_default(1, "MTNK", "Enemy", 60, 60);
        simulation.entities_mut().insert(mobile);
        let mut building = crate::sim::game_entity::GameEntity::test_default_of_category(
            2,
            "BLDG",
            "Enemy",
            60,
            60,
            crate::map::entities::EntityCategory::Structure,
        );
        building
            .type_facts
            .set_foundation_for_test("4x3".to_string());
        building
            .position
            .set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(200));
        building
            .position
            .set_sub_y_for_test(crate::util::fixed_math::SimFixed::from_num(33));
        simulation.entities_mut().insert(building);
        simulation.interner = crate::sim::intern::test_interner();
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[BuildingTypes]\n0=BLDG\n[BLDG]\nFoundation=4x3\n",
            ))
            .unwrap();
        let signed = tracker_object_signed_cell(&tracker, &simulation, &rules, (40, 60))
            .map(|(_, cell)| cell);
        assert_eq!(
            signed,
            Some((62, 61)),
            "4x3 foundation centre truncates after subcell"
        );
        assert_eq!(
            native_camera_cell_from_signed(signed.unwrap(), (100, 100), (632, 570)),
            (62, 61),
            "the exact tracker GetCoords cell is the immediate camera centre"
        );

        let edge = simulation.entities_mut().get_mut(2).unwrap();
        edge.position.set_rx_for_test(u16::MAX);
        edge.position.set_ry_for_test(u16::MAX);
        assert_eq!(
            tracker_object_signed_cell(&tracker, &simulation, &rules, (40, 60))
                .map(|(_, cell)| cell),
            Some((1, 0)),
            "GetCoords converts to wrapping native signed cell words"
        );
    }
}
