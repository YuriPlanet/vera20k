//! Where an entity is drawn: the isometric projection plus every visual-only
//! vertical offset that sits on top of it.
//!
//! ## Why this module exists
//!
//! `Position` used to carry two cached `f32` screen coordinates, and five
//! places in `sim/` wrote them — three of those writing a *visual* height lift
//! that has no simulation meaning. Screen placement is a render concern, so it
//! lives here, and `sim/` no longer writes screen coordinates at all.
//!
//! Nothing is cached. The projection is a handful of multiplies and every
//! input is authoritative sim state, so deriving on read is both cheaper than
//! the invalidation it replaces and immune to the stale-cache class of bug
//! (position mutated by a pass that forgot to refresh).
//!
//! ## The height lift
//!
//! Active YR applies the height lift **once** through `Tactical__AdjustForZ`
//! (`0x006D20E0`) as part of tactical projection:
//!
//! ```text
//! screen_y -= ftol(Z_leptons * k + (Z_leptons >= 728 ? 1 : 0) + 0.5)
//! ```
//!
//! Startup writer `0x006D1BDD` stores `k` with exact f64 bits
//! `0x3FC25E5374344960`; the shared native-x87 substrate owns that value and
//! the 53-bit, truncate-toward-zero operation order.
//!
//! Buildings on VERA's extended terrain keep that native conversion for
//! relative height, but align their ground baseline to the terrain's 15px
//! levels. [`building_terrain_correction`] is shared by their live, attached
//! animation, placement and fogged drawing paths.
//!
//! ### What this replaced
//!
//! Until this landed the lift was applied in two layers — the air movement pass
//! wrote `screen_y` with it, and then every app draw path subtracted it again —
//! so airborne units were lifted **twice**, at an effective 0.12 px/lepton,
//! while parachutes and scripted missiles got a single 0.06. Two different
//! scales for one quantity, and neither was gamemd's. At YR's stock
//! `FlightLevel=1500` an aircraft now sits 216px up instead of ~180px, and a
//! paradrop or missile roughly 2.4× higher than before.
//!
//! Frequency: every airborne unit in every match, for as long as it is
//! airborne. Kirovs, Harriers, Rocketeers, Nighthawks, every jumpjet, every
//! paradrop and every missile in flight.
//!
//! ## Two anchors, half a tile apart
//!
//! [`screen_position`] is the entity anchor: the projection of the object's own
//! coordinate, which lands on the centre of the cell's diamond. That is where
//! gamemd draws every class — units, infantry, aircraft, animations,
//! projectiles — and it is half a tile below the row a cell's tile art starts
//! on (`map::terrain::iso_to_screen`).
//!
//! Buildings are the single exception, and [`BUILDING_ART_LIFT_PX`] is that
//! exception's whole content.
//!
//! ## Dependency rules
//! - Part of render/ — reads sim/ state read-only and writes none of it.
//! - sim/ NEVER depends on render/, so nothing here may be called from sim/.

use crate::map::entities::EntityCategory;
use crate::sim::components::Position;

/// What placing an object on the screen reads of it: a live object, or its
/// record in the presentation frame (`app::presentation::frame`).
pub trait Placed {
    fn placed_position(&self) -> &Position;
    fn placed_category(&self) -> EntityCategory;
}

impl Placed for crate::sim::game_entity::GameEntity {
    fn placed_position(&self) -> &Position {
        &self.position
    }

    fn placed_category(&self) -> EntityCategory {
        self.category()
    }
}

/// Presentation-only extension of Tactical::AdjustForZ @ 0x006D20E0 for
/// buildings on high terrain. Its native scale agrees with 15px terrain
/// levels through level 14, but gives 538px instead of 540px at level 36.
/// Shift the ground baseline, keeping exact-Z offsets and simulation intact;
/// otherwise the TMP depth surface hides flat parts of the building.
pub(crate) fn building_terrain_correction(level: i32) -> i32 {
    if !(15..=i32::from(i8::MAX)).contains(&level) {
        return 0;
    }
    level * crate::render::native_z::HEIGHT_LEVEL_PX
        - crate::util::native_x87::adjust_for_z_standard(
            level * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
        )
}

fn entity_terrain_correction(entity: &(impl Placed + ?Sized)) -> i32 {
    if entity.placed_category() == EntityCategory::Structure {
        building_terrain_correction(i32::from(entity.placed_position().z() as i8))
    } else {
        0
    }
}

/// Upward screen lift of an entity's exact Location Z, in pixels.
///
/// Positive lifts the entity toward the top of the screen (screen Y decreases).
///
/// The exact Z is total world Z, the terrain level included. An object
/// without one already projects its stored level; only the high-terrain
/// building correction is added here.
///
/// The native height term reproduces gamemd's `AdjustForZ` exactly, quantisation
/// included: the startup scale, the 728-lepton correction, and the `+ 0.5`
/// truncate that rounds it to a whole pixel. The engine's own
/// blitter is integer-pixel, so a climbing aircraft steps a pixel at a time
/// there too.
///
/// gamemd-derived: active YR `ObjectClass__GetRenderCoords` at `0x0041BE00`
/// delegates to virtual `GetCoords`; the InfantryClass receiver reaches
/// `ObjectClass__GetCoords` at `0x005F65A0`, which copies exact XYZ and adds no
/// walking sine bob. Grounded objects therefore have zero additional lift.
pub fn height_lift_px(entity: &(impl Placed + ?Sized)) -> f32 {
    let z = entity.placed_position().exact_z_leptons().unwrap_or(0);
    // Standard AdjustForZ(0) is exactly 0, including its +0.5/chop. This
    // common ground-level case need not enter the x87 evaluator.
    let lift = if z == 0 {
        0
    } else {
        crate::util::native_x87::adjust_for_z_standard(z)
    };
    (lift + entity_terrain_correction(entity)) as f32
}

/// Where this entity is drawn, in world-space screen pixels.
///
/// This is the one place that answers the question. Everything that draws an
/// entity, brackets it, hangs a health bar over it or anchors an effect to it
/// goes through here, so they cannot drift apart.
///
/// A building's *art* is the single exception, and it is a strict addition on
/// top of this answer rather than a second one: see [`building_art_anchor`].
pub fn screen_position(entity: &(impl Placed + ?Sized)) -> (f32, f32) {
    let (sx, sy) = if entity.placed_position().exact_z_leptons().is_some() {
        z_free_screen_position(entity.placed_position())
    } else {
        ground_screen_position(entity.placed_position())
    };
    (sx, sy - height_lift_px(entity))
}

/// [`screen_position`] of `entity`'s coordinate moved `distance` leptons along
/// `facing`, at the same Z: the full-direction step Walk and Fly take
/// ([`crate::util::native_trig::facing_step_world_xy`]), then the same
/// projection.
pub fn facing_step_screen_position(
    entity: &(impl Placed + ?Sized),
    facing: u16,
    distance: i32,
) -> (f32, f32) {
    let location = crate::sim::movement::ground_pose::position_world_xy(entity.placed_position());
    let [x, y] = crate::util::native_trig::facing_step_world_xy(location, facing, distance);
    let z = entity
        .placed_position()
        .exact_z_leptons()
        .unwrap_or_else(|| {
            i32::from(entity.placed_position().z() as i8)
                * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS
        });
    let (sx, sy) = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
    (sx, sy - entity_terrain_correction(entity) as f32)
}

/// Fly Draw_Point4CF830 returns X0 and this screen-row delta. It is separate
/// from physical XYZ projection; Aircraft414613 and Foot4DAF6D each add it.
///
/// `no_wobbles` is the value original 4CF8B1 branches on. Retail reads
/// `TechnoType+C95` (`IsDropship`) there; a Phobos build feeds the resolved
/// `FlyNoWobbles` value instead, so the flag and not the type field decides.
pub(crate) fn fly_draw_point_y(
    frame: u32,
    no_wobbles: bool,
    taking_off: bool,
    landing: bool,
    height: i32,
    landing_base: i32,
) -> i32 {
    if no_wobbles || taking_off || landing || height <= landing_base {
        return 0;
    }
    use crate::util::native_x87::{NativeF64Bits, X87Chop53 as X};
    //4CF8D8 uses signed IDIV20; the qword argument store precedes table sine.
    let angle = X::mul(
        X::load_i32((frame as i32) % 20),
        X::load_f64(NativeF64Bits::from_bits(0x3fd4_1b2f_769c_f0e0)).unwrap(),
    );
    let angle = X::load_f64(X::store_f64(angle).unwrap()).unwrap();
    let sine = crate::util::retail_trig::TrigTable::embedded().sin_from_table(angle);
    let scaled = X::mul(
        sine,
        X::load_f64(NativeF64Bits::from_bits(0x3ff8_0000_0000_0000)).unwrap(),
    );
    X::ftol_i32_low_masked(X::add(scaled, X::load_f64(NativeF64Bits::HALF).unwrap()))
}

/// The whole upward lift [`screen_position`] draws `entity` with, in pixels:
/// `AdjustForZ` of its exact Location Z, or of its stored level. A draw that
/// cancels its lift for depth or per-pixel Z
/// cancels this, including the high-terrain correction, as the building
/// draw's `NormalZAdjust - AdjustForZ(Z)` seed does (`BuildingClass_DrawBody`).
pub fn screen_lift_px(entity: &(impl Placed + ?Sized)) -> i32 {
    let native = match entity.placed_position().exact_z_leptons() {
        Some(z) => crate::util::native_x87::adjust_for_z_standard(z),
        None => crate::util::native_x87::adjust_for_z_standard(
            i32::from(entity.placed_position().z() as i8)
                * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
        ),
    };
    native + entity_terrain_correction(entity)
}

/// How far up a building's art sits from the entity anchor, in screen pixels.
///
/// gamemd gives buildings their own answer to "where do I draw": of every
/// class, only `BuildingClass` overrides that virtual, and its override takes
/// half a cell off **both** coordinate axes — a pure `-128, -128` lepton step,
/// applied before the projection. Equal steps on the two axes cancel
/// horizontally and come to exactly half a tile up, moving the anchor from the
/// centre of the north-west footprint cell to that cell's tile row. There is no
/// X term and there must never be one.
///
/// This is the *whole* difference between a building's draw point and every
/// other class's, and it is only correct on top of an entity anchor that sits
/// on the cell's diamond centre — which is what
/// [`crate::util::lepton::lepton_to_screen`] returns. Applying this shift while
/// the entity anchor was still on the tile row double-counted it and left every
/// building floating half a tile above its foundation; that was tried and
/// reverted before the anchor was moved. The two belong together.
///
/// It applies to the building's **art** only — body, bib, overlay anims, voxel
/// turret, and the depth those sort on. It does not apply to selection
/// brackets, health pips, occupant pips or sensor rings: gamemd builds those
/// from the foundation-centre coordinate (the `GetCoords` virtual), reached on
/// a path that never consults the render-coordinate override, so they belong on
/// the plain entity anchor.
pub const BUILDING_ART_LIFT_PX: f32 = crate::map::terrain::TILE_HEIGHT / 2.0;

/// A building's art anchor, given its plain entity anchor.
///
/// One owner for [`BUILDING_ART_LIFT_PX`] so every draw of a building's art
/// lands on the same point.
pub fn building_art_anchor(sx: f32, sy: f32) -> (f32, f32) {
    (sx, sy - BUILDING_ART_LIFT_PX)
}

/// The isometric projection alone, with no height lift applied.
///
/// For callers that place something on the ground under an entity rather than
/// on the entity itself, and for entities that have no height state at all.
pub fn ground_screen_position(position: &crate::sim::components::Position) -> (f32, f32) {
    crate::util::lepton::lepton_to_screen(
        position.rx(),
        position.ry(),
        position.sub_x(),
        position.sub_y(),
        position.z(),
    )
}

/// Planar X/Y projection before any Z term is applied.
///
/// An exact world Z is already the complete native coordinate, so combining it
/// with the coarse terrain level would apply ground height twice. Keeping this
/// row separate also lets the draw-key path recover native's Z-free sort row by
/// adding [`height_lift_px`] back to the drawn row.
fn z_free_screen_position(position: &crate::sim::components::Position) -> (f32, f32) {
    crate::util::lepton::lepton_to_screen(
        position.rx(),
        position.ry(),
        position.sub_x(),
        position.sub_y(),
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn high_terrain_building_anchor_matches_its_tile_and_depth_lift() {
        let mut building = GameEntity::test_default_of_category(
            1,
            "GASPYSAT",
            "Neutral",
            189,
            258,
            EntityCategory::Structure,
        );
        for (level, exact_z) in [(14, Some(1456)), (36, Some(3744)), (36, None)] {
            building.position.set_z_for_test(level);
            building.position.set_exact_z_leptons_for_test(exact_z);
            let (x, y) = screen_position(&building);
            let lift = screen_lift_px(&building);
            let (tile_x, tile_y) = crate::map::terrain::iso_to_screen(189, 258, level);
            assert_eq!(building_art_anchor(x, y), (tile_x + 30.0, tile_y));
            assert_eq!(
                y + lift as f32,
                z_free_screen_position(&building.position).1
            );
        }
        building.position.set_exact_z_leptons_for_test(Some(3796));
        assert_eq!(
            screen_lift_px(&building) - 540,
            crate::util::native_x87::adjust_for_z_standard(3796)
                - crate::util::native_x87::adjust_for_z_standard(3744),
            "retain the native relative lift above the high plateau"
        );
        let mut aircraft =
            GameEntity::test_default_of_category(2, "ZEP", "A", 189, 258, EntityCategory::Aircraft);
        aircraft.position.set_z_for_test(36);
        aircraft.position.set_exact_z_leptons_for_test(Some(3744));
        assert_eq!(
            screen_lift_px(&aircraft),
            crate::util::native_x87::adjust_for_z_standard(3744),
            "aircraft retain their native lift"
        );
    }

    #[test]
    fn fly_draw_point_matches_original_signed_frames_and_phase_gates() {
        let ordinary = [0, 0, 1, 1, 1, 2, 1, 1, 1, 0, 0, 0, 0, 0, 0, -1, 0, 0, 0, 0];
        for (frame, expected) in ordinary.into_iter().enumerate() {
            assert_eq!(
                fly_draw_point_y(frame as u32, false, false, false, 60, 0),
                expected
            );
        }
        // Original IDIV/table/ftol controls; negative peaks are asymmetric.
        for (frame, expected) in [
            (-1, 0),
            (-5, 0),
            (-15, 1),
            (-19, 0),
            (-20, 0),
            (i32::MIN, 0),
            (i32::MIN + 1, 0),
            (i32::MAX, 1),
            (0, 0),
        ] {
            assert_eq!(
                fly_draw_point_y(frame as u32, false, false, false, 1500, 0),
                expected
            );
        }
        for (no_wobbles, takeoff, landing, height, base, expected) in [
            (true, false, false, 1500, 0, 0),
            (false, true, false, 1500, 0, 0),
            (false, false, true, 1500, 0, 0),
            (false, false, false, 0, 0, 0),
            (false, false, false, 100, 100, 0),
            (false, false, false, 99, 100, 0),
            (false, false, false, 101, 100, 2),
        ] {
            assert_eq!(
                fly_draw_point_y(5, no_wobbles, takeoff, landing, height, base),
                expected
            );
        }
    }

    // -- F14 boundary tests: sim-side callers moved here so `sim/` never
    // names `crate::render`. Coverage preserved verbatim. --

    /// A draw that cancels its lift cancels what [`screen_position`] applied:
    /// the drawn row plus [`screen_lift_px`] is the Z-free row, for a building
    /// on a ramp (exact Location Z 52) and for one at a stored level.
    #[test]
    fn the_cancelled_lift_is_the_drawn_lift() {
        let building = |exact_z: Option<i32>, level: u8| {
            let mut entity = GameEntity::test_default_of_category(
                1,
                "GAPOWR",
                "Americans",
                5,
                5,
                EntityCategory::Structure,
            );
            entity.position.set_exact_z_leptons_for_test(exact_z);
            entity.position.set_z_for_test(level);
            entity
        };
        for entity in [building(Some(52), 0), building(None, 2)] {
            let (_, drawn) = screen_position(&entity);
            let (_, z_free) = z_free_screen_position(&entity.position);
            assert_eq!(drawn + screen_lift_px(&entity) as f32, z_free);
        }
        assert_eq!(
            screen_lift_px(&building(Some(52), 0)),
            crate::util::native_x87::adjust_for_z_standard(52)
        );
    }
}
