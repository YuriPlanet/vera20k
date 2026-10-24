//! `HoverLocomotionClass::Draw_Matrix @ 0x00513F40` and its Shadow_Matrix
//! (`0x005142A0`) through the production Unit draw selectors.
use super::*;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::LocomotorState;

/// `slope01.tem`'s ramp, rising toward +X.
const RAMP: u8 = 1;
const CELL: (u16, u16) = (10, 10);

fn ramp_terrain() -> ResolvedTerrainGrid {
    let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
    terrain
        .cell_mut_for_test(CELL.0, CELL.1)
        .unwrap()
        .slope_type = RAMP;
    terrain
}

/// A powered Hover Foot at the ramp's centre, `height` leptons above its
/// surface.
fn hover_on_ramp(terrain: &ResolvedTerrainGrid, type_id: &str, height: i32) -> GameEntity {
    let mut entity = GameEntity::test_default(1, type_id, "Americans", CELL.0, CELL.1);
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Hover));
    let centre = DriveCoord {
        x: i32::from(CELL.0) * 256 + 128,
        y: i32::from(CELL.1) * 256 + 128,
        z: 0,
    };
    let ground = crate::sim::movement::ground_pose::query_ground_height(
        &NativeCellQuery::isolated(terrain),
        centre,
    )
    .unwrap();
    entity.position.put_location(DriveCoord {
        z: ground + height,
        ..centre
    });
    entity
}

fn body_slope(
    terrain: &ResolvedTerrainGrid,
    entity: &GameEntity,
    angles: [f32; 2],
) -> UnitRenderSlopeState {
    unit_render_slope_state(
        Some(terrain),
        Some(&NativeCellQuery::isolated(terrain)),
        entity,
        0,
        EntityDrawBand::Ground,
        angles,
    )
}

/// Is_Powered (`0x00516C70`) holds through the powered byte or any height,
/// and either one draws the level facing matrix (`0x0051407F..0x00514092`).
/// Only an unpowered Hover resting on its cell takes the ramp
/// (`0x00513F93..0x0051405C`), read through the draw's query. The shadow
/// (base `0x0055A7D0`) lies on the ramp throughout.
#[test]
fn a_powered_hover_draws_level_and_only_an_unpowered_one_takes_its_cell_slope() {
    let terrain = ramp_terrain();
    let cells = NativeCellQuery::isolated(&terrain);
    let height = |entity: &GameEntity| {
        crate::sim::movement::air_movement::queried_flight_height(entity, Some(&cells))
    };

    let resting = hover_on_ramp(&terrain, "SAPC", 0);
    assert_eq!(height(&resting), 0);
    let slope = body_slope(&terrain, &resting, [0.; 2]);
    assert_eq!(slope, UnitRenderSlopeState::Stable(0), "powered byte");
    assert_eq!(unit_shadow_slope(Some(&cells), &resting, slope), RAMP);
    // A terrain replacement must change the shadow even while the powered
    // body's level matrix and immutable initial template remain unchanged.
    let mut replacement = ramp_terrain();
    replacement
        .cell_mut_for_test(CELL.0, CELL.1)
        .unwrap()
        .slope_type = 4;
    assert_eq!(
        unit_shadow_slope(
            Some(&NativeCellQuery::isolated(&replacement)),
            &resting,
            slope
        ),
        4
    );
    assert!(ground_pose_with_angles(&resting, slope, 0, [0.; 2]).is_none());
    assert!(turret_images_cached(&resting, slope));

    let mut aloft = hover_on_ramp(&terrain, "SAPC", 1);
    aloft.locomotor.as_mut().unwrap().power_off();
    assert_eq!(height(&aloft), 1);
    assert_eq!(
        body_slope(&terrain, &aloft, [0.; 2]),
        UnitRenderSlopeState::Stable(0),
        "an unpowered Hover still above its surface"
    );

    let mut settled = hover_on_ramp(&terrain, "SAPC", 0);
    settled.locomotor.as_mut().unwrap().power_off();
    let slope = body_slope(&terrain, &settled, [0.; 2]);
    assert_eq!(slope, UnitRenderSlopeState::Stable(RAMP));
    assert_eq!(unit_shadow_slope(Some(&cells), &settled, slope), RAMP);
}

/// Either angle at the double 0.005 gate (`0x00513F53..0x00513F7F`) keys the
/// draw -1 and tilts the level facing matrix, even for an unpowered Hover
/// resting on a ramp; binary32 0.005 lies just under that gate. The tilt is
/// the shared ground pose with no slope blend, at slope 0.
#[test]
fn a_rocking_hover_tilts_its_level_facing_matrix_through_the_ground_pose() {
    let terrain = ramp_terrain();
    let mut hover = hover_on_ramp(&terrain, "ROBO", 0);
    hover.locomotor.as_mut().unwrap().power_off();
    for (angles, tilted) in [
        ([0.005, -0.005], false),
        ([0.25, 0.], true),
        ([0., -0.5], true),
    ] {
        let mut rocking = crate::sim::rocking::RockingState::default();
        rocking.set_angles_native_for_test(angles);
        hover.rocking = Some(rocking);
        let slope = body_slope(&terrain, &hover, angles);
        let pose = ground_pose_with_angles(&hover, slope, 0, angles);
        assert_eq!(pose.is_some(), tilted, "{angles:?}");
        assert_eq!(turret_images_cached(&hover, slope), !tilted, "{angles:?}");
        assert_eq!(
            unit_shadow_slope(Some(&NativeCellQuery::isolated(&terrain)), &hover, slope),
            RAMP
        );
        if let Some(pose) = pose {
            assert_eq!(slope, UnitRenderSlopeState::Stable(0), "{angles:?}");
            assert_eq!(pose.angles, angles);
            assert!(pose.slope_blend.is_none());
        } else {
            assert_eq!(slope, UnitRenderSlopeState::Stable(RAMP), "{angles:?}");
        }
    }
}

/// Hover's rocking arm makes two products, `(lift * facing) * frame`
/// (`0x00514262`, `0x00514270`), from the same lift, frame and
/// `0x0055A730` facing calls as Drive's three-product arm, whose slope-0
/// identity (`0x004B01A9`) is the only difference. On every executed Drive
/// slope-0 tilt row the two-product form reproduces Drive's native bits, and
/// so does the shared owner a rocking Hover draws through.
#[test]
fn hover_rocking_matrix_is_the_shared_ground_owner_at_slope_zero() {
    use crate::map::voxel_pose::{
        ground_locomotor_matrix, voxel_body_facing, voxel_tilt_applies, voxel_tilt_frame,
    };
    use crate::render::vxl_raster::ship_pose_tests::{native_drive_corpus, native_ground_pose};
    let bits = |matrix: glam::Mat4| {
        (0..3)
            .flat_map(|row| (0..4).map(move |col| matrix.col(col)[row].to_bits()))
            .collect::<Vec<_>>()
    };
    let product = |left, right| crate::util::native_affine::matrix_product(left, right).unwrap();
    let mut compared = 0;
    for row in native_drive_corpus()["cases"].as_array().unwrap() {
        let input = &row["input"];
        let pose = native_ground_pose(input);
        if input["slope"] != 0 || pose.slope_blend.is_some() || !voxel_tilt_applies(pose.angles) {
            continue;
        }
        let half_sizes: [f32; 2] = std::array::from_fn(|i| {
            f64::from_bits(
                u64::from_str_radix(row["drive"]["half_size_bits"][i].as_str().unwrap(), 16)
                    .unwrap(),
            ) as f32
        });
        let step = crate::render::vxl_raster::voxel_facing_step(pose.body_facing);
        let (lift, frame) = voxel_tilt_frame(pose.angles, half_sizes);
        let hover = product(product(lift, voxel_body_facing(step)), frame);
        let name = input["name"].as_str().unwrap();
        assert_eq!(
            serde_json::json!(bits(hover)),
            row["drive"]["matrix_bits"],
            "{name}"
        );
        assert_eq!(
            bits(ground_locomotor_matrix(
                0,
                None,
                step,
                pose.angles,
                half_sizes
            )),
            bits(hover),
            "{name}"
        );
        compared += 1;
    }
    assert_eq!(compared, 4);
}

/// The retail Hover users, through the production `Locomotor=` reader. ROBO
/// has a turret, so its rocking draws through the per-part ground pose, not a
/// composite.
#[test]
fn retail_hover_types_are_the_four_amphibious_hovers() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut hovers: Vec<&str> = battle
        .rules
        .all_objects()
        .filter(|object| object.locomotor == LocomotorKind::Hover)
        .map(|object| object.id.as_str())
        .collect();
    hovers.sort_unstable();
    assert_eq!(hovers, ["LCRF", "ROBO", "SAPC", "YHVR"]);
    assert!(battle.rules.object("ROBO").unwrap().has_turret);
}
