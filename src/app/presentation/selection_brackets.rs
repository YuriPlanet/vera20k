//! Isometric 3D selection bracket lines for buildings.
//!
//! When a building is selected, draws white bracket stub lines at the 3 visible
//! corners of its isometric bounding box (at roof level). Each corner has 3 short
//! lines radiating outward along the 3 isometric axes (X, Y, Z).
//!
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use crate::app::AppState;
use crate::app::input::commands::preferred_local_owner_name;
use crate::app::presentation::instances::in_view;
use crate::map::entities::EntityCategory;
use crate::render::batch::SpriteInstance;
use crate::render::shroud_buffer::ShroudBuffer;
use crate::sim::vision::FogState;

/// Height= multiplier: 1 art.ini Height unit = 15 screen pixels (HeightFactor * AdjustForZ).
const HEIGHT_PX: f32 = 15.0;

/// A bracket endpoint: where it lands on screen, and how far above the ground
/// plane it sits.
///
/// The height is carried separately from the screen point because it is what
/// makes a bracket pixel's depth recoverable. gamemd gives each line endpoint
/// the depth `14 - AdjustForZ(z)` while its screen Y has already had
/// `AdjustForZ(z)` subtracted, so the two terms cancel and every pixel of the
/// box — roof corners included — ends up carrying the depth of the ground
/// corner beneath it. Adding `height` back to the plotted row reproduces that
/// ground row exactly, which is why a vertical edge comes out at one constant
/// depth along its whole length.
#[derive(Clone, Copy)]
struct Corner {
    pt: ScreenPt,
    height: f32,
}

/// Maps a bracket pixel's ground row to the same depth axis the sprite bodies
/// sort and stamp on, so the two are directly comparable in the depth test.
#[derive(Clone, Copy)]
struct BracketDepthCtx {
    origin_y: f32,
    world_height: f32,
    cell_z: u8,
}

impl BracketDepthCtx {
    fn from_state(state: &AppState, cell_z: u8) -> Self {
        let (origin_y, world_height) = state
            .match_state
            .match_presentation
            .terrain_grid
            .as_ref()
            .map(|g| (g.origin_y, g.world_height))
            .unwrap_or((0.0, 1.0));
        Self {
            origin_y,
            world_height,
            cell_z,
        }
    }

    /// Depth for a pixel plotted at `row` that stands `height` px above ground.
    fn depth_at(&self, row: i32, height: f32) -> f32 {
        crate::app::presentation::instances::compute_sprite_depth_params(
            self.origin_y,
            self.world_height,
            row as f32 + height,
            self.cell_z,
        )
    }
}

/// Bracket line color — solid white, fully opaque.
const BRACKET_COLOR: [f32; 4] = [1.0, 1.0, 1.0, 1.0];
const ABUFFER_NEUTRAL: u8 = 0x7F;
const ABUFFER_BLACK: u8 = 0x00;

struct BracketPixelFilter<'a> {
    shroud: Option<&'a ShroudBuffer>,
    camera_x: f32,
    camera_y: f32,
    enabled: bool,
}

impl<'a> BracketPixelFilter<'a> {
    fn from_state(state: &'a AppState) -> Self {
        Self {
            shroud: state.match_state.match_presentation.shroud_buffer.as_ref(),
            camera_x: state.match_state.input.camera_x,
            camera_y: state.match_state.input.camera_y,
            enabled: !state.match_state.sandbox_full_visibility,
        }
    }

    fn tint_for_pixel(&self, x: i32, y: i32) -> Option<[f32; 3]> {
        if !self.enabled {
            return Some([BRACKET_COLOR[0], BRACKET_COLOR[1], BRACKET_COLOR[2]]);
        }
        let abuf = self
            .shroud
            .and_then(|s| s.sample_world(x as f32, y as f32, self.camera_x, self.camera_y))
            .unwrap_or(ABUFFER_NEUTRAL);
        match abuf {
            ABUFFER_BLACK => None,
            ABUFFER_NEUTRAL => Some([BRACKET_COLOR[0], BRACKET_COLOR[1], BRACKET_COLOR[2]]),
            value => {
                let scale = (value as f32 / ABUFFER_NEUTRAL as f32).clamp(0.0, 1.0);
                Some([
                    BRACKET_COLOR[0] * scale,
                    BRACKET_COLOR[1] * scale,
                    BRACKET_COLOR[2] * scale,
                ])
            }
        }
    }
}

/// Check if an entity is visible to the local player for overlay purposes.
fn is_visible(
    local_owner: Option<crate::sim::intern::InternedId>,
    fog: &FogState,
    pos: &crate::sim::components::Position,
    entity_owner: crate::sim::intern::InternedId,
    ignore_visibility: bool,
) -> bool {
    if ignore_visibility {
        return true;
    }
    let Some(owner) = local_owner else {
        return true;
    };
    if owner == entity_owner {
        return true;
    }
    fog.is_cell_revealed(owner, pos.rx, pos.ry) && !fog.is_cell_gap_covered(owner, pos.rx, pos.ry)
}

/// Compute the 8 corners of a building's isometric bounding box in screen space.
///
/// Returns `(ground_corners, roof_corners)` where each is `[FL, FR, BL, BR]`.
///
/// `sx`/`sy` must be the **plain entity anchor** — the projection of the
/// building's own coordinate, i.e. the diamond centre of its north-west
/// footprint cell. It is deliberately *not* the building's art anchor: gamemd
/// builds this cuboid from the foundation-centre coordinate (`GetCoords`), on a
/// draw path that reads the object coordinate directly and never consults the
/// render-coordinate override the art takes. Applying that override here would
/// float the whole box half a tile above the footprint it encloses.
fn compute_box_corners(
    sx: f32,
    sy: f32,
    fw: f32,
    fh: f32,
    z_screen: f32,
) -> ([ScreenPt; 4], [ScreenPt; 4]) {
    // From Location (leptons): the foundation centre sits ((fw-1)*128, (fh-1)*128)
    // from the north-west cell centre. Projected through the isometric transform
    // (dx_px = 30*(dx-dy)/256, dy_px = 15*(dx+dy)/256) that is:
    //   cx = sx + (fw - fh)*15
    //   cy = sy + (fw + fh)*7.5 - 15
    // The trailing -15 is the `-1` on each axis of that lepton offset, NOT a
    // correction between the tile and entity frames.
    let cx = sx + (fw - fh) * 15.0;
    let cy = sy + (fw + fh) * 7.5 - 15.0;

    // 4 ground corners relative to foundation center.
    // From gamemd projection: screen_dx = 30*(dx-dy)/256, screen_dy = 15*(dx+dy)/256
    // where dx = ±hw, dy = ±hh in leptons (hw = fw*128, hh = fh*128).
    // Simplifies to: screen offsets use (fw, fh) cells directly.
    let ground = [
        ScreenPt {
            x: cx - (fw + fh) * 15.0,
            y: cy + (fh - fw) * 7.5,
        }, // FL
        ScreenPt {
            x: cx + (fw - fh) * 15.0,
            y: cy + (fw + fh) * 7.5,
        }, // FR
        ScreenPt {
            x: cx + (fh - fw) * 15.0,
            y: cy - (fw + fh) * 7.5,
        }, // BL
        ScreenPt {
            x: cx + (fw + fh) * 15.0,
            y: cy - (fh - fw) * 7.5,
        }, // BR
    ];
    // Roof corners = ground corners shifted up by z_screen.
    let roof = [
        ScreenPt {
            x: ground[0].x,
            y: ground[0].y - z_screen,
        }, // FL roof
        ScreenPt {
            x: ground[1].x,
            y: ground[1].y - z_screen,
        }, // FR roof
        ScreenPt {
            x: ground[2].x,
            y: ground[2].y - z_screen,
        }, // BL roof
        ScreenPt {
            x: ground[3].x,
            y: ground[3].y - z_screen,
        }, // BR roof
    ];
    (ground, roof)
}

#[derive(Clone, Copy)]
struct ScreenPt {
    x: f32,
    y: f32,
}

#[derive(Debug, Default)]
pub(crate) struct SelectionBracketInstances {
    pub back: Vec<SpriteInstance>,
    pub front_first: Vec<SpriteInstance>,
    pub front: Vec<SpriteInstance>,
}

/// Compute the quarter-point 25% from `a` toward `b`: (3a + b) / 4.
fn quarter_point(a: ScreenPt, b: ScreenPt) -> ScreenPt {
    ScreenPt {
        x: ((a.x * 3.0 + b.x) * 0.25).trunc(),
        y: ((a.y * 3.0 + b.y) * 0.25).trunc(),
    }
}

fn screen_i32(v: f32) -> i32 {
    v.trunc() as i32
}

fn emit_pixel(instances: &mut Vec<SpriteInstance>, x: i32, y: i32, tint: [f32; 3], depth: f32) {
    instances.push(SpriteInstance {
        position: [x as f32, y as f32],
        size: [1.0, 1.0],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        tint,
        alpha: BRACKET_COLOR[3],
        depth,
        ..Default::default()
    });
}

/// Emit 1px-wide line segments as pixel-stepping SpriteInstance quads.
///
/// Matches the visible part of gamemd's surface-line contract for ordinary
/// bracket segments: integer endpoints, start pixel included, final endpoint
/// excluded, Bresenham-style x/y stepping. The optional filter handles the
/// post-shroud final-front bracket redraw's ABuffer pixel predicate.
fn emit_line(
    instances: &mut Vec<SpriteInstance>,
    a: Corner,
    b: Corner,
    ctx: &BracketDepthCtx,
    filter: Option<&BracketPixelFilter<'_>>,
) {
    // gamemd reorders every segment left-to-right before it plots, then steps x
    // unconditionally upward and stops one short of the far end. So the
    // SMALLER-X endpoint is always the one that gets a pixel and the larger-x
    // one is always dropped, whichever way the caller handed the segment over.
    // Walking in the caller's order instead slides the whole run one pixel
    // every time a stub points leftward. Ties keep the caller's order, matching
    // the `JLE` that guards the swap.
    let (a, b) = if screen_i32(a.pt.x) <= screen_i32(b.pt.x) {
        (a, b)
    } else {
        (b, a)
    };

    let mut x = screen_i32(a.pt.x);
    let mut y = screen_i32(a.pt.y);
    let end_x = screen_i32(b.pt.x);
    let end_y = screen_i32(b.pt.y);
    if x == end_x && y == end_y {
        return;
    }

    let dx = (end_x - x).abs();
    let dy = -(end_y - y).abs();
    let sx = if x < end_x { 1 } else { -1 };
    let sy = if y < end_y { 1 } else { -1 };
    let mut err = dx + dy;
    // Bresenham reaches the excluded far endpoint after max(|dx|,|dy|) plotted
    // pixels, so that count is the parameter the two endpoint heights blend
    // over. On a vertical edge the height gained cancels the row lost, which is
    // what keeps the whole edge at its ground corner's depth.
    let steps: f32 = dx.max(-dy).max(1) as f32;
    let mut step: f32 = 0.0;

    while x != end_x || y != end_y {
        let tint = match filter {
            Some(f) => f.tint_for_pixel(x, y),
            None => Some([BRACKET_COLOR[0], BRACKET_COLOR[1], BRACKET_COLOR[2]]),
        };
        if let Some(tint) = tint {
            let height: f32 = a.height + (b.height - a.height) * (step / steps);
            emit_pixel(instances, x, y, tint, ctx.depth_at(y, height));
        }
        let e2 = err * 2;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
        step += 1.0;
    }
}

/// Emit a bracket stub: a 25% line from corner `a` toward `b`.
fn emit_stub(
    instances: &mut Vec<SpriteInstance>,
    a: Corner,
    b: Corner,
    ctx: &BracketDepthCtx,
    filter: Option<&BracketPixelFilter<'_>>,
) {
    let qp = Corner {
        pt: quarter_point(a.pt, b.pt),
        // The stub's far end stands a quarter of the way up the edge, so its
        // height blends on the same (3a + b) / 4 ratio as its position.
        height: (a.height * 3.0 + b.height) * 0.25,
    };
    emit_line(instances, a, qp, ctx, filter);
}

/// Build bracket instances for all selected buildings.
pub(crate) fn build_selection_bracket_instances(
    state: &AppState,
    sw: f32,
    sh: f32,
) -> SelectionBracketInstances {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return SelectionBracketInstances::default();
    };
    let local_owner = preferred_local_owner_name(state);
    let local_owner_id = local_owner.as_deref().and_then(|n| sim.interner.get(n));
    let ignore_visibility = state.match_state.sandbox_full_visibility;
    let cam_x = state.match_state.input.camera_x;
    let cam_y = state.match_state.input.camera_y;
    let final_front_filter = BracketPixelFilter::from_state(state);
    let mut out = SelectionBracketInstances::default();

    for e in sim.entities().values() {
        if e.category() != EntityCategory::Structure || !e.selected {
            continue;
        }
        let type_str = sim.interner.resolve(e.type_ref());
        if !is_visible(
            local_owner_id,
            &sim.fog,
            &e.position,
            e.owner(),
            ignore_visibility,
        ) {
            continue;
        }

        let (sx, sy) = crate::render::locomotor_visual::screen_position(e);

        // Look up foundation and Height from rules/art.
        let obj = state.rules().and_then(|r| r.object(type_str));
        let (fw_u, fh_u) = obj
            .map(|o| crate::rules::foundation::foundation_dimensions(&o.foundation))
            .unwrap_or((2, 2));
        let fw = fw_u as f32;
        let fh = fh_u as f32;

        let art_key: &str = obj
            .map(|o| {
                let img = o.image.as_str();
                if img.is_empty() { o.id.as_str() } else { img }
            })
            .unwrap_or(type_str);
        let art_height: f32 = state
            .rules()
            .and_then(|rules| rules.art().get(art_key))
            .map(|entry| entry.height as f32)
            .unwrap_or(2.0);
        let z_screen = art_height * HEIGHT_PX;

        // Compute 8 corners.
        let (g, r) = compute_box_corners(sx, sy, fw, fh, z_screen);
        // g = [FL, FR, BL, BR] ground, r = [FL, FR, BL, BR] roof

        // Viewport cull: bounding box of all roof corners (ground is below roof).
        let min_x = r[0].x.min(r[1].x).min(r[2].x).min(r[3].x);
        let max_x = r[0].x.max(r[1].x).max(r[2].x).max(r[3].x);
        let min_y = r[0].y.min(r[1].y).min(r[2].y).min(r[3].y);
        let max_y = g[0].y.max(g[1].y).max(g[2].y).max(g[3].y); // ground is lower
        if !in_view(
            min_x,
            min_y,
            max_x - min_x,
            max_y - min_y,
            cam_x,
            cam_y,
            sw,
            sh,
            60.0,
        ) {
            continue;
        }
        // --- 12 edges of the isometric bounding box ---
        // Indices: FL=0, FR=1, BL=2, BR=3
        let ctx = BracketDepthCtx::from_state(state, e.position.z);
        let gc: [Corner; 4] = [
            Corner {
                pt: g[0],
                height: 0.0,
            },
            Corner {
                pt: g[1],
                height: 0.0,
            },
            Corner {
                pt: g[2],
                height: 0.0,
            },
            Corner {
                pt: g[3],
                height: 0.0,
            },
        ];
        let rc: [Corner; 4] = [
            Corner {
                pt: r[0],
                height: z_screen,
            },
            Corner {
                pt: r[1],
                height: z_screen,
            },
            Corner {
                pt: r[2],
                height: z_screen,
            },
            Corner {
                pt: r[3],
                height: z_screen,
            },
        ];
        let front = Some(&final_front_filter);

        // --- Three corner brackets, roof level only ---
        //
        // WHAT A PLAYER SEES, which is what this reproduces: a selected building
        // carries three corner marks, each radiating along the three isometric
        // axes, and nothing anywhere near the floor. Reported from live play and
        // confirmed from a screenshot 2026-08-05. The near (FR) roof corner is
        // bare.
        //
        // DRIFT, deliberate and recorded. gamemd's draw code emits more than
        // this: `TechnoClass__DrawBracketCorner` puts a stub at BOTH ends of
        // each edge it is given, its nine callsites are nine of the cuboid's
        // twelve edges, and `DrawExtras` inlines the remaining three as
        // single-ended stubs — 21 stubs in all, seven of the eight corners, six
        // of them anchored on the ground plane. Those ground-anchored stubs are
        // not visible in play, and the mechanism that removes them has not been
        // established. The leading hypothesis is per-pixel depth: bracket lines
        // are Z-tested by `Surface__DrawLine_ABufModulated_ZClipped` and
        // building bodies write Z, so the ground marks would lose against the
        // art standing over them. That is a hypothesis. An implementation of it
        // (the depth stamp in `presentation/render/draw_passes.rs`) did not hide them,
        // which is evidence the hypothesis is at least incomplete.
        //
        // So this emits the observed subset directly rather than the full set
        // plus an occlusion pass that does not yet work. Restoring the other
        // twelve stubs is a one-commit revert once the removal mechanism is
        // understood; until then, do not "fix" this by adding them back on the
        // strength of the callsite list alone.
        //
        // Corner arms, three each:
        //   BL roof — vertical down, toward FL roof, toward BR roof
        //   FL roof — vertical down, toward BL roof, toward FR roof
        //   BR roof — vertical down, toward BL roof, toward FR roof

        // Submitted before the object bodies, where the SHP merge can occlude
        // them — gamemd's DrawBehind phase.
        emit_stub(&mut out.back, rc[2], gc[2], &ctx, None); // BL roof, vertical
        emit_stub(&mut out.back, rc[2], rc[0], &ctx, None); // BL roof -> FL roof
        emit_stub(&mut out.back, rc[2], rc[3], &ctx, None); // BL roof -> BR roof
        emit_stub(&mut out.back, rc[0], rc[2], &ctx, None); // FL roof -> BL roof
        emit_stub(&mut out.back, rc[3], rc[2], &ctx, None); // BR roof -> BL roof

        // gamemd's first object pass submits these before the body draw and its
        // DrawExtras phase submits them again, so each goes into both buckets;
        // only the second submission carries the post-shroud ABuffer predicate.
        emit_stub(&mut out.front_first, rc[0], gc[0], &ctx, None); // FL roof, vertical
        emit_stub(&mut out.front, rc[0], gc[0], &ctx, front);
        emit_stub(&mut out.front_first, rc[0], rc[1], &ctx, None); // FL roof -> FR roof
        emit_stub(&mut out.front, rc[0], rc[1], &ctx, front);
        emit_stub(&mut out.front_first, rc[3], gc[3], &ctx, None); // BR roof, vertical
        emit_stub(&mut out.front, rc[3], gc[3], &ctx, front);
        emit_stub(&mut out.front_first, rc[3], rc[1], &ctx, None); // BR roof -> FR roof
        emit_stub(&mut out.front, rc[3], rc[1], &ctx, front);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{BracketDepthCtx, Corner, ScreenPt, emit_line};
    use crate::render::batch::SpriteInstance;

    fn test_ctx() -> BracketDepthCtx {
        BracketDepthCtx {
            origin_y: 0.0,
            world_height: 1000.0,
            cell_z: 0,
        }
    }

    fn ground(pt: ScreenPt) -> Corner {
        Corner { pt, height: 0.0 }
    }

    fn line_positions(a: ScreenPt, b: ScreenPt) -> Vec<(i32, i32)> {
        let mut instances: Vec<SpriteInstance> = Vec::new();
        emit_line(&mut instances, ground(a), ground(b), &test_ctx(), None);
        instances
            .iter()
            .map(|i| (i.position[0] as i32, i.position[1] as i32))
            .collect()
    }

    #[test]
    fn emit_line_excludes_final_endpoint() {
        let pts = line_positions(ScreenPt { x: 0.0, y: 0.0 }, ScreenPt { x: 3.0, y: 0.0 });
        assert_eq!(pts, vec![(0, 0), (1, 0), (2, 0)]);
    }

    /// A segment handed over right-to-left plots the SAME pixels as the same
    /// segment handed over left-to-right.
    ///
    /// gamemd canonicalises every segment before plotting and then steps x
    /// unconditionally upward, stopping one short of the far end, so the
    /// smaller-x endpoint is always the one that survives regardless of the
    /// order the caller used. Walking in caller order instead — which is what
    /// this test used to assert — slides the whole run one pixel every time a
    /// stub points leftward, and roughly half of the box's stubs do.
    #[test]
    fn emit_line_canonicalises_right_to_left_segments() {
        let leftward = line_positions(ScreenPt { x: 3.0, y: 0.0 }, ScreenPt { x: 0.0, y: 0.0 });
        assert_eq!(leftward, vec![(0, 0), (1, 0), (2, 0)]);
        let rightward = line_positions(ScreenPt { x: 0.0, y: 0.0 }, ScreenPt { x: 3.0, y: 0.0 });
        assert_eq!(
            leftward, rightward,
            "the plotted run must not depend on which end the caller passed first"
        );
    }

    /// A vertical box edge must come out at one single depth along its whole
    /// length. Each pixel climbs one row and gains one pixel of height, and the
    /// two cancel — the same cancellation gamemd gets from pairing a
    /// `14 - AdjustForZ(z)` endpoint depth with a screen Y that already had
    /// `AdjustForZ(z)` taken off it. If this ever splits, roof-corner marks
    /// start testing against the wrong row and clip against their own building.
    #[test]
    fn vertical_edge_pixels_all_carry_the_ground_corner_depth() {
        let mut instances: Vec<SpriteInstance> = Vec::new();
        let base = ScreenPt { x: 10.0, y: 100.0 };
        emit_line(
            &mut instances,
            Corner {
                pt: base,
                height: 0.0,
            },
            Corner {
                pt: ScreenPt { x: 10.0, y: 80.0 },
                height: 20.0,
            },
            &test_ctx(),
            None,
        );
        assert_eq!(instances.len(), 20, "one pixel per row climbed");
        let ground_depth = test_ctx().depth_at(100, 0.0);
        for (i, inst) in instances.iter().enumerate() {
            assert!(
                (inst.depth - ground_depth).abs() < 1e-6,
                "pixel {i} at row {} drifted to {} (ground corner is {ground_depth})",
                inst.position[1],
                inst.depth,
            );
        }
    }

    /// A ground edge is the control case: no height anywhere, so depth has to
    /// track the plotted row and nothing else. Rows are taken from the middle
    /// of the world so the depth axis is not sitting on its own clamp.
    #[test]
    fn ground_edge_depth_tracks_the_row() {
        let mut instances: Vec<SpriteInstance> = Vec::new();
        emit_line(
            &mut instances,
            ground(ScreenPt { x: 0.0, y: 400.0 }),
            ground(ScreenPt { x: 0.0, y: 404.0 }),
            &test_ctx(),
            None,
        );
        let depths: Vec<f32> = instances.iter().map(|i| i.depth).collect();
        assert_eq!(depths.len(), 4);
        for pair in depths.windows(2) {
            assert!(pair[1] < pair[0], "moving south must move nearer: {pair:?}");
        }
    }

    #[test]
    fn emit_line_truncates_float_endpoints_toward_zero() {
        let pts = line_positions(ScreenPt { x: -1.8, y: 0.9 }, ScreenPt { x: 1.8, y: 0.9 });
        assert_eq!(pts, vec![(-1, 0), (0, 0)]);
    }
}
