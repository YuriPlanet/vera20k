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

use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::MovementLayer;

/// What is carrying this entity's visual height, if anything.
///
/// The variants are ordered by the precedence the sim tick used to establish by
/// write order: the ground pass ran first and the later passes overwrote it, so
/// the last pass to touch an entity won. Preserved here as an explicit `match`
/// rather than left implicit in pass ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeightSource {
    /// Object-level falling state — a paradropped unit under a parachute.
    Parachute,
    /// A scripted missile in flight.
    Rocket,
    /// An air-layer locomotor holding the unit up.
    AirLocomotor,
    /// On the ground, with no additional visual height.
    Ground,
}

fn height_source(entity: &GameEntity) -> HeightSource {
    if entity.parachute_state.is_some() {
        return HeightSource::Parachute;
    }
    if entity.rocket_state.is_some() {
        return HeightSource::Rocket;
    }
    let airborne = entity
        .locomotor
        .as_ref()
        .is_some_and(|loco| loco.layer == MovementLayer::Air && loco.kind != LocomotorKind::Rocket);
    if airborne {
        HeightSource::AirLocomotor
    } else {
        HeightSource::Ground
    }
}

/// An entity's total world Z in leptons — the VERA answer to the native
/// object's coordinate `+0xA4`.
///
/// This is the coordinate frame, not the render frame: it is what
/// `CenterViewCommandClass` and the follow camera read, so the `BuildingClass`
/// -128/-128 art shift must stay out of it (see [`building_art_anchor`]).
///
/// An exact object coordinate is already total world Z and replaces both terms
/// below; otherwise the coarse signed terrain level and the object's altitude
/// above it compose, exactly as [`screen_position`] composes their pixel forms.
pub fn world_z_leptons(entity: &GameEntity) -> i32 {
    if let Some(exact_z_leptons) = entity.position.exact_z_leptons {
        return exact_z_leptons;
    }
    // The level byte is signed everywhere else that reads it.
    i32::from(entity.position.z as i8) * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS
        + height_leptons(entity)
}

/// This entity's height above the ground, in leptons.
///
/// Whichever state is carrying it — see [`HeightSource`] for why the order is
/// what it is.
fn height_leptons(entity: &GameEntity) -> i32 {
    match height_source(entity) {
        HeightSource::Parachute => entity
            .parachute_state
            .as_ref()
            .map(|state| state.altitude.to_num::<i32>())
            .unwrap_or(0),
        HeightSource::Rocket => entity
            .rocket_state
            .as_ref()
            .map(|state| state.altitude.to_num::<i32>())
            .unwrap_or(0),
        HeightSource::AirLocomotor => entity
            .locomotor
            .as_ref()
            .map(|loco| loco.altitude.to_num::<i32>())
            .unwrap_or(0),
        HeightSource::Ground => 0,
    }
}

fn adjust_for_z_lift_px(leptons: i32) -> f32 {
    crate::util::native_x87::adjust_for_z_standard(leptons) as f32
}

/// Total upward screen lift for an entity, in pixels.
///
/// Positive lifts the entity toward the top of the screen (screen Y decreases).
///
/// An exact object coordinate is total world Z and therefore replaces every
/// decomposed height source below, including the coarse terrain level.
///
/// The height term reproduces gamemd's `AdjustForZ` exactly, quantisation
/// included: the startup scale, the 728-lepton correction, and the `+ 0.5`
/// truncate that rounds it to a whole pixel. The engine's own
/// blitter is integer-pixel, so a climbing aircraft steps a pixel at a time
/// there too.
///
/// gamemd-derived: active YR `ObjectClass__GetRenderCoords` at `0x0041BE00`
/// delegates to virtual `GetCoords`; the InfantryClass receiver reaches
/// `ObjectClass__GetCoords` at `0x005F65A0`, which copies exact XYZ and adds no
/// walking sine bob. Grounded objects therefore have zero additional lift.
pub fn height_lift_px(entity: &GameEntity) -> f32 {
    if let Some(exact_z_leptons) = entity.position.exact_z_leptons {
        return adjust_for_z_lift_px(exact_z_leptons);
    }
    adjust_for_z_lift_px(height_leptons(entity))
}

/// Where this entity is drawn, in world-space screen pixels.
///
/// This is the one place that answers the question. Everything that draws an
/// entity, brackets it, hangs a health bar over it or anchors an effect to it
/// goes through here, so they cannot drift apart.
///
/// A building's *art* is the single exception, and it is a strict addition on
/// top of this answer rather than a second one: see [`building_art_anchor`].
pub fn screen_position(entity: &GameEntity) -> (f32, f32) {
    let (sx, sy) = if entity.position.exact_z_leptons.is_some() {
        z_free_screen_position(&entity.position)
    } else {
        ground_screen_position(&entity.position)
    };
    (sx, sy - height_lift_px(entity))
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
/// One owner for [`BUILDING_ART_LIFT_PX`] so the placement ghost and the
/// building it previews cannot drift apart.
pub fn building_art_anchor(sx: f32, sy: f32) -> (f32, f32) {
    (sx, sy - BUILDING_ART_LIFT_PX)
}

/// The isometric projection alone, with no height lift applied.
///
/// For callers that place something on the ground under an entity rather than
/// on the entity itself, and for entities that have no height state at all.
pub fn ground_screen_position(position: &crate::sim::components::Position) -> (f32, f32) {
    crate::util::lepton::lepton_to_screen(
        position.rx,
        position.ry,
        position.sub_x,
        position.sub_y,
        position.z,
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
        position.rx,
        position.ry,
        position.sub_x,
        position.sub_y,
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::LocomotorState;
    use crate::util::fixed_math::SimFixed;

    fn air_unit(kind: LocomotorKind, altitude: i32) -> GameEntity {
        let mut entity = GameEntity::test_default(1, "BEAG", "Americans", 5, 5);
        entity.category = EntityCategory::Aircraft;
        let mut loco = LocomotorState::for_test_kind(kind);
        loco.layer = MovementLayer::Air;
        loco.altitude = SimFixed::from_num(altitude);
        entity.locomotor = Some(loco);
        entity
    }

    #[test]
    fn teleport_switch_draws_the_same_exact_owner_coordinate_as_navigation() {
        use crate::sim::movement::{rocket_movement, teleport_movement};
        use crate::sim::world::Simulation;
        for kind in [
            LocomotorKind::Fly,
            LocomotorKind::Hover,
            LocomotorKind::Rocket,
        ] {
            let mut sim = Simulation::new();
            let mut e = air_unit(kind, 125);
            e.position.z = 2;
            e.position.exact_z_leptons = Some(333);
            e.locomotor = Some(LocomotorState::for_test_kind(kind));
            e.locomotor.as_mut().unwrap().altitude = SimFixed::from_num(125);
            sim.substrate.entities.insert(e);
            if kind == LocomotorKind::Rocket {
                assert!(rocket_movement::attach_rocket_state(
                    &mut sim.substrate.entities,
                    1,
                    (5, 5),
                    (20, 5),
                    SimFixed::from_num(250),
                ));
                sim.substrate
                    .entities
                    .get_mut(1)
                    .unwrap()
                    .rocket_state
                    .as_mut()
                    .unwrap()
                    .altitude = SimFixed::from_num(125);
            }
            let before = world_z_leptons(sim.substrate.entities.get(1).unwrap());
            assert_eq!(before, 333);
            assert!(teleport_movement::issue_teleport_command(
                &mut sim.substrate.entities,
                1,
                (8, 9),
                &Default::default(),
                true,
                0,
            ));
            let e = sim.substrate.entities.get(1).unwrap();
            assert_eq!(world_z_leptons(e), before);
            assert_eq!(sim.foot_navigation_coordinate(1).unwrap().z, before);
            assert_eq!(height_lift_px(e), adjust_for_z_lift_px(before));
            teleport_movement::tick_teleport_movement(
                &mut sim.substrate.entities,
                &mut sim.substrate.occupancy,
                &[1],
                0,
                None,
                None,
            );
            let e = sim.substrate.entities.get(1).unwrap();
            assert_eq!(e.locomotor.as_ref().unwrap().active_kind(), kind);
            assert_eq!(world_z_leptons(e), 208);
            assert_eq!(sim.foot_navigation_coordinate(1).unwrap().z, 208);
            assert_eq!(height_lift_px(e), adjust_for_z_lift_px(208));
        }
    }

    /// The whole reason this half-tile keeps going wrong, pinned on one cell.
    ///
    /// Three layers share cell (10, 4) at ground level, and gamemd puts them in
    /// exactly this relation:
    ///
    /// ```text
    ///   layer                    row              relative to the tile box top
    ///   terrain tile / overlay   box top-left     (0, 0)
    ///   unit / infantry / anim   diamond centre   (+30, +15)
    ///   building art             box top edge     (+30,   0)
    /// ```
    ///
    /// A unit stands in the *middle* of its tile; a building's art starts on the
    /// same row the tile art does. Get either wrong and every unit on the map is
    /// drawn half a tile off the ground it walks on — which is what this pins
    /// against. The absolute numbers carry VERA's constant world-row bias (see
    /// `util::lepton::WORLD_ROW_BIAS_PX`); the *relation* is what matters and is
    /// what a player sees.
    #[test]
    fn the_three_world_layers_sit_where_gamemd_puts_them() {
        use crate::map::terrain::{self, TILE_HEIGHT, TILE_WIDTH};

        const RX: u16 = 10;
        const RY: u16 = 4;

        let (box_x, box_y) = terrain::iso_to_screen(RX, RY, 0);
        assert_eq!((box_x, box_y), (150.0, 225.0), "tile bounding-box top-left");

        let unit = GameEntity::test_default(1, "MTNK", "Americans", RX, RY);
        let (unit_x, unit_y) = screen_position(&unit);
        assert_eq!(
            (unit_x - box_x, unit_y - box_y),
            (TILE_WIDTH / 2.0, TILE_HEIGHT / 2.0),
            "a unit stands on its cell's diamond centre, not the box top"
        );

        let (art_x, art_y) = building_art_anchor(unit_x, unit_y);
        assert_eq!(
            (art_x - box_x, art_y - box_y),
            (TILE_WIDTH / 2.0, 0.0),
            "a building's art anchor drops back onto the tile row — and the \
             half-cell shift is Y-only, so X must not move"
        );
        assert_eq!(art_x, unit_x, "the building shift has no X term");
    }

    #[test]
    fn a_grounded_unit_draws_at_its_projection() {
        let entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        assert_eq!(
            screen_position(&entity),
            ground_screen_position(&entity.position),
            "nothing is holding this unit up, so nothing may lift it"
        );
    }

    /// The whole point of the module: one owner, so the lift cannot be applied
    /// twice. At YR's stock `FlightLevel=1500`, the native multiplier lifts a
    /// cruising aircraft by 216px, including the threshold correction.
    #[test]
    fn adjust_for_z_cruising_aircraft_sits_at_gamemds_height() {
        let entity = air_unit(LocomotorKind::Fly, 1500);
        let (_, ground_sy) = ground_screen_position(&entity.position);
        let (_, sy) = screen_position(&entity);
        assert_eq!(ground_sy - sy, 216.0);
    }

    /// Every kind of height goes through one scale. A parachute and an aircraft
    /// at the same altitude must be drawn at the same place — before this they
    /// differed by a factor of two.
    #[test]
    fn every_height_source_uses_the_same_scale() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::movement::parachute_descent::begin_parachute_descent;

        let aircraft = air_unit(LocomotorKind::Fly, 400);

        let mut entities = EntityStore::default();
        let mut infantry = GameEntity::test_default(1, "E1", "Americans", 5, 5);
        infantry.category = EntityCategory::Infantry;
        entities.insert(infantry);
        assert!(begin_parachute_descent(
            &mut entities,
            1,
            SimFixed::from_num(400)
        ));

        assert_eq!(
            height_lift_px(&aircraft),
            height_lift_px(entities.get(1).expect("entity")),
        );
    }

    /// The extra pixel above the threshold is gamemd's, not ours: one lepton
    /// either side of 728 must differ by more than the scale alone accounts for.
    #[test]
    fn adjust_for_z_extra_pixel_switches_on_at_the_threshold() {
        let below = height_lift_px(&air_unit(LocomotorKind::Fly, 727));
        let at = height_lift_px(&air_unit(LocomotorKind::Fly, 728));
        assert_eq!(below, 104.0, "trunc(727 * k + 0.5)");
        assert_eq!(at, 105.0, "trunc(728 * k + 1 + 0.5) — the threshold fires");
    }

    #[test]
    fn adjust_for_z_exact_signed_z_ignores_coarse_level_and_locomotor_altitude() {
        let mut entity = air_unit(LocomotorKind::Fly, 1500);
        entity.position.z = 9;
        entity.position.exact_z_leptons = Some(-400);

        let z_free = z_free_screen_position(&entity.position);
        let drawn = screen_position(&entity);
        assert_eq!(height_lift_px(&entity), -56.0);
        assert_eq!(drawn, (z_free.0, z_free.1 + 56.0));
        assert_eq!(drawn.1 + height_lift_px(&entity), z_free.1);
    }

    #[test]
    fn gsi_04_15_exact_z_threshold_bonus_starts_at_728() {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.position.exact_z_leptons = Some(727);
        assert_eq!(height_lift_px(&entity), 104.0);

        entity.position.exact_z_leptons = Some(728);
        assert_eq!(height_lift_px(&entity), 105.0);
    }

    #[test]
    fn gsi_13_02_building_lift_and_adjust_for_z_remain_separate_exact_steps() {
        let mut entity = GameEntity::test_default(1, "GAPOWR", "Americans", 5, 5);
        entity.category = EntityCategory::Structure;
        entity.position.exact_z_leptons = Some(728);

        let z_free = z_free_screen_position(&entity.position);
        let drawn = screen_position(&entity);
        assert_eq!(drawn, (z_free.0, z_free.1 - 105.0));
        assert_eq!(
            building_art_anchor(drawn.0, drawn.1),
            (drawn.0, drawn.1 - 15.0),
        );
    }

    #[test]
    fn height_does_not_move_an_entity_sideways() {
        let entity = air_unit(LocomotorKind::Jumpjet, 900);
        assert_eq!(
            screen_position(&entity).0,
            ground_screen_position(&entity.position).0,
            "the lift is vertical only"
        );
    }

    /// A ground locomotor's altitude is not a height source. Only the air layer
    /// lifts, which is what keeps a deployed or docked unit on the floor.
    #[test]
    fn a_ground_layer_locomotor_never_lifts() {
        let mut entity = air_unit(LocomotorKind::Drive, 1500);
        entity.category = EntityCategory::Unit;
        if let Some(loco) = entity.locomotor.as_mut() {
            loco.layer = MovementLayer::Ground;
        }
        assert_eq!(
            screen_position(&entity),
            ground_screen_position(&entity.position),
        );
    }

    /// Object-level falling wins over the locomotor, matching the write order
    /// the sim tick used to have: the parachute pass ran after the air pass.
    #[test]
    fn parachute_state_takes_precedence_over_an_air_locomotor() {
        use crate::sim::entity_store::EntityStore;
        use crate::sim::movement::parachute_descent::begin_parachute_descent;

        let mut entities = EntityStore::default();
        let mut entity = air_unit(LocomotorKind::Jumpjet, 1500);
        entity.category = EntityCategory::Infantry;
        entities.insert(entity);
        assert!(begin_parachute_descent(
            &mut entities,
            1,
            SimFixed::from_num(400)
        ));
        let entity = entities.get(1).expect("entity");

        let (_, ground_sy) = ground_screen_position(&entity.position);
        let (_, sy) = screen_position(&entity);
        // The parachute's 400 leptons, not the locomotor's 1500, and below the
        // extra-pixel threshold.
        assert_eq!(
            ground_sy - sy,
            57.0,
            "the parachute altitude must be the only height source"
        );
    }

    #[test]
    fn gsi_13_02_grounded_infantry_anchor_is_independent_of_wobble_phase() {
        let mut entity = GameEntity::test_default(1, "E1", "Americans", 5, 5);
        entity.category = EntityCategory::Infantry;
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Walk);
        loco.infantry_wobble_phase = 0.0;
        entity.locomotor = Some(loco);
        let resting = screen_position(&entity);

        if let Some(loco) = entity.locomotor.as_mut() {
            loco.infantry_wobble_phase = std::f32::consts::PI;
        }
        assert_eq!(
            screen_position(&entity),
            resting,
            "ObjectClass::GetCoords returns exact XYZ; phase cannot move the anchor",
        );
        assert_eq!(resting, ground_screen_position(&entity.position));
    }

    /// The slice's exit criterion, and the thing that keeps it from growing
    /// back: no file under `sim/` may assign a screen coordinate. Screen
    /// placement is derived here, so a write over there is by definition a
    /// second owner — which is how the height lift came to be applied twice.
    ///
    /// Deliberately NOT asserted: that `sim/` holds no `f32` at all. Legacy
    /// locomotor phase state may remain without becoming a render-coordinate
    /// offset; removing that state is outside this projection slice.
    #[test]
    fn sim_writes_no_screen_coordinates() {
        fn walk(dir: &std::path::Path, out: &mut Vec<(String, usize, String)>) {
            for entry in std::fs::read_dir(dir).expect("readable") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).expect("utf-8 source");
                    for (i, line) in text.lines().enumerate() {
                        if is_screen_coord_write(line) {
                            out.push((path.display().to_string(), i + 1, line.trim().to_string()));
                        }
                    }
                }
            }
        }

        let sim = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/sim");
        let mut writes = Vec::new();
        walk(&sim, &mut writes);
        assert!(
            writes.is_empty(),
            "sim/ must not assign screen coordinates — that is \
             render::locomotor_visual's job. Found:\n{}",
            writes
                .iter()
                .map(|(f, l, t)| format!("  {f}:{l}: {t}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn the_guard_recognises_a_write_when_it_sees_one() {
        // Guards that can never fire are worse than no guard, so exercise the
        // matcher against the shapes the old code actually used.
        for line in [
            "entity.position.screen_x = sx;",
            "self.screen_y = sy;",
            "entity.position.screen_y -= bob;",
            "        e.position.screen_x  =  nx;",
        ] {
            assert!(is_screen_coord_write(line), "missed: {line}");
        }
        for line in [
            "pub screen_x: f32,",
            "screen_x: position.screen_x + dx,",
            "if a.screen_y == b.screen_y {",
            "let dx = screen_x - start_x;",
            "pub fn begin_drag(&mut self, screen_x: f32, screen_y: f32) {",
        ] {
            assert!(!is_screen_coord_write(line), "false hit: {line}");
        }
    }

    /// An assignment to a `screen_x`/`screen_y` binding — plain `=` or any
    /// compound operator. A comparison is not a write, and neither is a
    /// `screen_x:` field declaration, struct initialiser or parameter.
    fn is_screen_coord_write(line: &str) -> bool {
        for name in ["screen_x", "screen_y"] {
            let mut from = 0;
            while let Some(at) = line[from..].find(name) {
                let after = from + at + name.len();
                let rest = line[after..].trim_start();
                let op = rest.trim_start_matches(['+', '-', '*', '/']);
                if op.starts_with('=') && !op.starts_with("==") {
                    return true;
                }
                from = after;
            }
        }
        false
    }

    #[test]
    fn a_vehicle_never_bobs() {
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
        entity.category = EntityCategory::Unit;
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Drive);
        loco.infantry_wobble_phase = std::f32::consts::PI;
        entity.locomotor = Some(loco);
        assert_eq!(height_lift_px(&entity), 0.0);
    }
    // -- F14 boundary tests: sim-side callers moved here so `sim/` never
    // names `crate::render`. Coverage preserved verbatim. --
    use crate::map::terrain;
    use crate::sim::components::{Health, MovementTarget};
    use crate::sim::entity_store::EntityStore;
    use crate::sim::intern::test_interner;
    use crate::sim::movement::tick_movement;
    use crate::util::fixed_math::SIM_ZERO;

    #[test]
    fn test_screen_coords_computed() {
        let e = GameEntity::new_at_frame_zero_for_test(
            1,
            30,
            40,
            2, // z=2 for elevation
            0,
            crate::sim::intern::test_intern("Americans"),
            Health { current: 100 },
            crate::sim::intern::test_intern("HTNK"),
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        // An entity is drawn on its cell's diamond centre, half a tile east and
        // half a tile south of that cell's tile corner.
        let (corner_sx, corner_sy) = terrain::iso_to_screen(30, 40, 2);
        let (sx, sy) = crate::render::locomotor_visual::screen_position(&e);
        assert!((sx - (corner_sx + terrain::TILE_WIDTH / 2.0)).abs() < 0.01);
        assert!((sy - (corner_sy + terrain::TILE_HEIGHT / 2.0)).abs() < 0.01);
    }

    #[test]
    fn test_tick_movement_updates_screen_position() {
        let mut entities = EntityStore::new();

        let path: Vec<(u16, u16)> = vec![(5, 5), (6, 5)];
        let mut e = GameEntity::test_default(1, "HTNK", "Americans", 5, 5);
        e.movement_target = Some(MovementTarget {
            path,
            path_layers: vec![MovementLayer::Ground; 2],
            next_index: 1,
            speed: SimFixed::from_num(1280), // 5 cells/sec in leptons.
            move_dir_x: SimFixed::from_num(256),
            move_dir_y: SIM_ZERO,
            move_dir_len: SimFixed::from_num(256),
            ..Default::default()
        });
        e.facing = 64;
        entities.insert(e);

        let mut lifecycle_requests = Vec::new();
        for _ in 0..3 {
            tick_movement(&mut entities, &mut test_interner(), &mut lifecycle_requests);
        }

        let entity = entities.get(1).expect("entity exists");
        // After moving, the unit is drawn on cell (6, 5)'s diamond centre — half a
        // tile east and half a tile south of that cell's tile corner.
        let (corner_sx, corner_sy): (f32, f32) = terrain::iso_to_screen(6, 5, 0);
        let (sx, sy) = crate::render::locomotor_visual::screen_position(entity);
        assert!((sx - (corner_sx + terrain::TILE_WIDTH / 2.0)).abs() < 1.0);
        assert!((sy - (corner_sy + terrain::TILE_HEIGHT / 2.0)).abs() < 1.0);
    }

    /// The spawn-path variant with explicit constants (moved from the sim
    /// spawn test): a unit at cell (30, 40, z=0) projects to
    ///   x = 30*(30-40) = -300,  y = 15*(30+40) + 15 + 15 = 1080
    /// (the trailing +15s are VERA's constant world-row bias and the
    /// half-tile drop from the tile row down to the diamond centre).
    #[test]
    fn spawned_unit_projects_to_diamond_centre_constants() {
        let e = GameEntity::new_at_frame_zero_for_test(
            1,
            30,
            40,
            0,
            64,
            crate::sim::intern::test_intern("Americans"),
            Health { current: 100 },
            crate::sim::intern::test_intern("HTNK"),
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        let (sx, sy) = screen_position(&e);
        assert!((sx - (-300.0)).abs() < 0.1);
        assert!((sy - 1080.0).abs() < 0.1);
    }
}
