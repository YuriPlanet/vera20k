//! Shared native surface line primitives. Presentation pixels, never simulation state.
//!
//! Line_In_Bounds7BC2B0 is shared by LineTrail, action and rally rows.
//! XSurface7BA610 and DSurface4C0750 share the integer pixel walk; the latter
//! walks a pattern in caller direction after arranging points left to right.
//! Native execution: tools/procedural_drawing_oracle/{rally,action_lines}.py.

/// Logical pixels are unzoomed screen coordinates; ZOrigin remains an explicit
/// native input and never becomes a second depth buffer.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SurfaceLineViewport {
    pub camera: [i32; 2],
    pub clip: [i32; 4],
    pub z_origin_y: i32,
    pub zoom: f32,
}

/// Shared DSurface operation after a presentation producer has resolved its
/// geometry and color. LaserDraw and EBolt feed the same ordered pixel owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceLineBlend {
    /// 4BDF00 pre-scales its RGB by the retained float then admits any >7.
    Add([u8; 3]),
    /// 4BFD30 receives an already packed surface word.
    Replace(u16),
    /// DSurface4BC750's paired antialiased stores; callers represented here
    /// set all RGB channels and never write Z (Game456DD8, Laser550655/880).
    Antialiased {
        rgb: [u8; 3],
        intensity: crate::util::native_x87::NativeF32Bits,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceLine {
    pub from: [i32; 2],
    pub to: [i32; 2],
    pub z_adjust: [i32; 2],
    pub blend: SurfaceLineBlend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceDraw {
    Line(SurfaceLine),
    /// XSurface7BB350: packed opaque stores, without A/Z admission.
    Ellipse {
        center: [i32; 2],
        radius: [i32; 2],
        word: u16,
    },
}

#[path = "surface_line_aa.rs"]
mod antialiased;
#[cfg(test)]
pub(crate) use antialiased::CUBIC_TABLE;
pub(crate) use antialiased::antialiased_line;

/// Original XSurface::DrawEllipse7BB350. Two midpoint walks start at the
/// vertical and horizontal axes independently; each updates before storing.
/// Stores are opaque, including pixels in shroud or behind existing objects.
pub(crate) fn ellipse(
    center: [i32; 2],
    radius: [i32; 2],
    clip: [i32; 4],
    mut emit: impl FnMut([i32; 2]),
) {
    let mut pixel = |[x, y]: [i32; 2]| {
        if x >= clip[0] && x < clip[0] + clip[2] && y >= clip[1] && y < clip[1] + clip[3] {
            emit([x, y]);
        }
    };
    let [cx, cy] = center;
    let [rx, ry] = radius;
    let rx2 = rx.wrapping_mul(rx);
    let ry2 = ry.wrapping_mul(ry);
    pixel([cx, cy.wrapping_add(ry)]);
    pixel([cx, cy.wrapping_sub(ry)]);
    let mut x: i32 = 0;
    let mut y = ry;
    let mut dx: i32 = 0;
    let mut dy = rx2.wrapping_mul(ry).wrapping_mul(2);
    let mut decision = (rx2 / 4).wrapping_sub(rx2.wrapping_mul(ry));
    loop {
        decision = decision.wrapping_add(dx).wrapping_add(ry2);
        if decision >= 0 {
            dy = dy.wrapping_sub(rx2.wrapping_mul(2));
            decision = decision.wrapping_sub(dy);
            y = y.wrapping_sub(1);
        }
        dx = dx.wrapping_add(ry2.wrapping_mul(2));
        x = x.wrapping_add(1);
        if dx >= dy {
            break;
        }
        pixel([cx.wrapping_add(x), cy.wrapping_add(y)]);
        pixel([cx.wrapping_sub(x), cy.wrapping_sub(y)]);
        pixel([cx.wrapping_add(x), cy.wrapping_sub(y)]);
        pixel([cx.wrapping_sub(x), cy.wrapping_add(y)]);
    }
    pixel([cx.wrapping_add(rx), cy]);
    pixel([cx.wrapping_sub(rx), cy]);
    x = rx;
    y = 0;
    dx = ry2.wrapping_mul(rx).wrapping_mul(2);
    dy = 0;
    decision = (ry2 / 4).wrapping_sub(ry2.wrapping_mul(rx));
    loop {
        decision = decision.wrapping_add(dy).wrapping_add(rx2);
        if decision >= 0 {
            dx = dx.wrapping_sub(ry2.wrapping_mul(2));
            decision = decision.wrapping_sub(dx);
            x = x.wrapping_sub(1);
        }
        dy = dy.wrapping_add(rx2.wrapping_mul(2));
        y = y.wrapping_add(1);
        if dy > dx {
            break;
        }
        pixel([cx.wrapping_add(x), cy.wrapping_add(y)]);
        pixel([cx.wrapping_sub(x), cy.wrapping_sub(y)]);
        pixel([cx.wrapping_add(x), cy.wrapping_sub(y)]);
        pixel([cx.wrapping_sub(x), cy.wrapping_add(y)]);
    }
}

/// Original7BC2B0 mutates endpoints only on success. Callers such as rally
/// deliberately reuse those clipped endpoints for their next offset row.
/// Nearest-f64 retains a one-pixel edge residual versus native chop rounding;
/// `rally.json::clipping_rounding_controls` preserves the observed cases.
pub(crate) fn clip_line(a: &mut [i32; 2], b: &mut [i32; 2], clip: [i32; 4]) -> bool {
    let left = f64::from(clip[0]);
    let top = f64::from(clip[1]);
    let right = f64::from(clip[0] + clip[2]);
    let bottom = f64::from(clip[1] + clip[3]);
    let mut p = a.map(f64::from);
    let mut q = b.map(f64::from);
    let xy = (q[0] - p[0]) / (q[1] - p[1]);
    let yx = (q[1] - p[1]) / (q[0] - p[0]);
    let code = |v: [f64; 2]| -> u8 {
        (if v[0] < left {
            1
        } else if v[0] >= right {
            2
        } else {
            0
        }) | (if v[1] < top {
            8
        } else if v[1] >= bottom {
            4
        } else {
            0
        })
    };
    loop {
        let pc = code(p);
        let qc = code(q);
        if pc | qc == 0 {
            *a = p.map(|v| v as i32);
            *b = q.map(|v| v as i32);
            return true;
        }
        if pc & qc != 0 {
            return false;
        }
        let c = if pc != 0 { pc } else { qc };
        let v = if c & 8 != 0 {
            [(top - p[1]) * xy + p[0], top]
        } else if c & 4 != 0 {
            [(bottom - 1.0 - p[1]) * xy + p[0], bottom - 1.0]
        } else if c & 2 != 0 {
            [right - 1.0, (right - 1.0 - p[0]) * yx + p[1]]
        } else {
            [left, (left - p[0]) * yx + p[1]]
        };
        if c == pc {
            p = v;
        } else {
            q = v;
        }
    }
}

/// Patterned pixel walk from DSurface4C0750. Endpoints must already be clipped.
/// Axis-aligned lines include the final endpoint; diagonals exclude it.
/// The callback receives lit pixels in native store order. The original leaf's
/// ABuffer predicate selects pass zero for nonzero samples, pass one for zero.
/// Neither pass reads or writes Z; the caller partitions these same stores.
pub(crate) fn patterned_line(
    mut from: [i32; 2],
    mut to: [i32; 2],
    pattern: &[u8; 16],
    mut phase: i32,
    mut emit: impl FnMut([i32; 2]),
) {
    let mut direction = 1;
    if from[0] > to[0] {
        let length = (from[0] - to[0]).max((from[1] - to[1]).abs() + 1);
        phase = phase.wrapping_add(length) % 16;
        direction = -1;
        std::mem::swap(&mut from, &mut to);
    }
    walk_line(from, to, |point| {
        phase = phase.rem_euclid(16);
        if pattern[phase as usize] != 0 {
            emit(point);
        }
        phase += direction;
    });
}

/// XSurface7BA610's clipped solid raster, emitted as disjoint rectangles.
/// Axis-aligned lines include both endpoints; diagonal lines exclude the
/// last endpoint after arranging X in ascending order. Consecutive stores
/// along the major axis are batched without changing coverage. The radar's
/// rectangle edges therefore remain four rectangles, not per-pixel quads.
pub(crate) fn solid_line(mut from: [i32; 2], mut to: [i32; 2], mut emit: impl FnMut([i32; 4])) {
    if from[0] > to[0] {
        std::mem::swap(&mut from, &mut to);
    }
    let dx = to[0].wrapping_sub(from[0]);
    let dy = to[1].wrapping_sub(from[1]).wrapping_abs();
    if dx == 0 || dy == 0 {
        emit([
            from[0],
            from[1].min(to[1]),
            dx.wrapping_add(1),
            dy.wrapping_add(1),
        ]);
        return;
    }
    let horizontal = dx > dy;
    let mut span: Option<[i32; 4]> = None;
    walk_line(from, to, |[x, y]| {
        if let Some(rect) = span.as_mut() {
            if horizontal && y == rect[1] && x == rect[0] + rect[2] {
                rect[2] += 1;
                return;
            }
            if !horizontal && x == rect[0] {
                rect[1] = rect[1].min(y);
                rect[3] += 1;
                return;
            }
            emit(*rect);
        }
        span = Some([x, y, 1, 1]);
    });
    if let Some(rect) = span {
        emit(rect);
    }
}

/// The common strict-error step in7BA610/4C0750. Points are already clipped
/// and sorted by X. It emits in original store order, including descending Y.
fn walk_line(mut from: [i32; 2], to: [i32; 2], mut emit: impl FnMut([i32; 2])) {
    let dx = to[0] - from[0];
    let dy = (to[1] - from[1]).abs();
    let y_step = if from[1] > to[1] { -1 } else { 1 };
    let count = dx.max(dy) + i32::from(dx == 0 || dy == 0);
    let mut error = 2 * dx.min(dy) - dx.max(dy);
    for _ in 0..count {
        emit(from);
        if dx > dy {
            if error > 0 {
                from[1] += y_step;
                error -= 2 * dx;
            }
            from[0] += 1;
            error += 2 * dy;
        } else {
            if error > 0 {
                from[0] += 1;
                error -= 2 * dy;
            }
            from[1] += y_step;
            error += 2 * dx;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LinePixel {
    pub point: [i32; 2],
    pub z: u16,
    /// Additive4BDF00 advances A only with Y; its X stays at the clipped
    /// start. Packed4BFD30 and LineTrail4BEAC0 advance A with both axes.
    pub clipped_start_x: i32,
}

/// Shared clipped three-axis walk of DSurface4BEAC0,4BDF00,4BFD30.
/// Each consumer keeps its own admission/color operation. Z is sampled as an
/// unsigned16 word and never mutated by these consumers. Executed original
/// pixels: projectile_oracle/line_trail and spatial_oracle/building_prism.
/// Common endpoint ordering, clipping and clipped-Z interpolation of
/// DSurface4BEAC0/4BC750/4BDF00/4BFD30. No raster or material decisions here.
fn clipped_z_line(
    from: [i32; 2],
    to: [i32; 2],
    z_adjust: [i32; 2],
    clip: [i32; 4],
) -> Option<([i32; 2], [i32; 2], i32, i32)> {
    if clip[2] <= 0 || clip[3] <= 0 {
        return None;
    }
    let mut a = [from[0].wrapping_add(clip[0]), from[1].wrapping_add(clip[1])];
    let mut b = [to[0].wrapping_add(clip[0]), to[1].wrapping_add(clip[1])];
    let mut za = z_adjust[0];
    let mut zb = z_adjust[1];
    if a[0] > b[0] {
        std::mem::swap(&mut a, &mut b);
        std::mem::swap(&mut za, &mut zb);
    }
    let original_a = a;
    let original_b = b;
    if !clip_line(&mut a, &mut b, clip) {
        return None;
    }
    // Original4C1B50 stores x²+y² then uses the existing retail table owner,
    // not host sqrt. Its two-coordinate scalar equals the zero-Z helper.
    let length = |a: [i32; 2], b: [i32; 2]| {
        crate::util::native_x87::distance_3d_leptons([a[0], a[1], 0], [b[0], b[1], 0])
    };
    let full_length = if a != original_a || b != original_b {
        length(original_a, original_b)
    } else {
        0
    };
    let clipped_adjust = |distance: i32, delta: i32| {
        use crate::util::native_x87::X87Chop53 as X;
        let ratio = X::div(X::load_i32(distance), X::load_i32(full_length))
            .expect("nonzero clipped line length");
        X::ftol_i64(X::mul(ratio, X::load_i32(delta))).expect("finite clipped line adjustment")
            as i32
    };
    let original_za = za;
    let original_zb = zb;
    if b != original_b {
        let delta = clipped_adjust(length(original_a, b), original_za.wrapping_sub(original_zb));
        zb = if original_za < original_zb {
            original_za.wrapping_add(delta.wrapping_abs())
        } else {
            original_za.wrapping_sub(delta.wrapping_abs())
        };
    }
    if a != original_a {
        let delta = clipped_adjust(length(a, original_b), original_za.wrapping_sub(original_zb));
        za = if original_zb > original_za {
            original_zb.wrapping_sub(delta.wrapping_abs())
        } else {
            original_zb.wrapping_add(delta.wrapping_abs())
        };
    }
    Some((a, b, za, zb))
}

pub(crate) fn rasterize_z_clipped(
    from: [i32; 2],
    to: [i32; 2],
    z_adjust: [i32; 2],
    clip: [i32; 4],
    z_origin_y: i32,
    mut emit: impl FnMut(LinePixel),
) {
    let Some((a, b, za, zb)) = clipped_z_line(from, to, z_adjust, clip) else {
        return;
    };
    let dx = b[0] - a[0];
    let dy = (b[1] - a[1]).abs();
    let dz = zb.wrapping_sub(za).wrapping_abs();
    let y_step = if b[1] < a[1] { -1 } else { 1 };
    let z_step = if zb < za { -1 } else { 1 };
    let dominant = if dz > dx && dz > dy {
        2
    } else if dx > dy {
        0
    } else {
        1
    };
    let lengths = [dx, dy, dz];
    let count = lengths[dominant];
    let mut errors = [-count; 3];
    let mut point = a;
    let mut adjustment = za;
    for _ in 0..count {
        for axis in 0..3 {
            if axis != dominant {
                errors[axis] += 2 * lengths[axis];
            }
        }
        // 4BEF03/4BF186/4BF3EE narrow before unsigned comparison, unlike SHP.
        let z = (crate::render::native_z::DEFAULT_Z.wrapping_add(z_origin_y) as u16 as i32)
            .wrapping_sub(point[1])
            .wrapping_sub(clip[1])
            .wrapping_add(adjustment) as u16;
        emit(LinePixel {
            point,
            z,
            clipped_start_x: a[0],
        });
        for (axis, error) in errors.iter_mut().enumerate() {
            if axis == dominant || *error > 0 {
                match axis {
                    0 => point[0] += 1,
                    1 => point[1] += y_step,
                    _ => adjustment += z_step,
                }
                if axis != dominant {
                    *error -= 2 * count;
                }
            }
        }
    }
}
