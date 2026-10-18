//! Persistent Boris designation: Tactical6D47FB -> Techno705860 -> Surface4BEAC0.
//! Current manager coordinates are supplied by sim; all clock/RNG/geometry here
//! is presentation-only. Executed controls: procedural_drawing_oracle/airstrike_beam.
use crate::render::surface_line::{
    SurfaceDraw, SurfaceLine, SurfaceLineBlend, SurfaceLineViewport,
};
use crate::sim::components::DriveCoord;
use crate::sim::rng::MainRngDraws;

///6D482D..6D48C0 uses the shared original sine/cosine lookup, not host libm.
fn wobble(mut target: DriveCoord, wall_ms: u32) -> DriveCoord {
    use crate::map::retail_trig::TrigTable;
    use crate::util::native_x87::{NativeF64Bits, X87Chop53 as X};
    let t = X::load_i32((wall_ms >> 7) as i32);
    let scale = X::load_i32(15);
    let rate = X::load_f64(NativeF64Bits::from_bits(0x3fd7ae147ae147ae)).unwrap();
    let table = TrigTable::embedded();
    let dx = X::ftol_i32_low_masked(X::add(
        X::load_i32(target.x),
        X::mul(table.cos_from_table(X::mul(t, rate)), scale),
    ));
    let dz = X::ftol_i32_low_masked(X::add(
        X::load_i32(target.z),
        X::mul(table.sin_from_table(t), scale),
    ));
    target.x = dx;
    target.z = dz;
    target
}

pub(super) fn build(
    from: DriveCoord,
    target: DriveCoord,
    wall_ms: u32,
    main: &MainRngDraws,
    view: SurfaceLineViewport,
) -> Vec<SurfaceDraw> {
    geometry(from, wobble(target, wall_ms), main.ranged(190, 270), view)
}

///705860 requests one Main ranged value, even if clipping/A/Z hides the beam.
///The color is red/orange independent of Flare.IsHouseColor and LaserTargetColor.
fn geometry(
    from: DriveCoord,
    to: DriveCoord,
    random: i32,
    view: SurfaceLineViewport,
) -> Vec<SurfaceDraw> {
    use crate::util::native_x87::adjust_for_z_standard;
    let project = |p: DriveCoord| {
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(p.x, p.y, p.z);
        [x as i32 - view.camera[0], y as i32 - view.camera[1]]
    };
    let start = project(from);
    let end = project(to);
    let rgb = if random <= 255 {
        [random as u8, 0, 0]
    } else {
        [
            255,
            (55 * (random - 256) / 7) as u8,
            (15 * (random - 256) / 7) as u8,
        ]
    };
    let mut draws = Vec::with_capacity(16);
    //70597E: ordinary XSurface lines clip against the whole surface, with no A/Z.
    if end[1] < start[1] {
        let [x, y] = end;
        for rect in [
            [x - 2, y + 1, 4, 1],
            [x - 2, y, 4, 1],
            [x - 1, y - 1, 1, 4],
            [x, y - 1, 1, 4],
        ] {
            draws.push(SurfaceDraw::OpaqueRect { rect, word: 0xF800 });
        }
    }
    let delta = [end[0].wrapping_sub(start[0]), end[1].wrapping_sub(start[1])];
    let axis = if delta[0].wrapping_abs() > delta[1].wrapping_abs() {
        1
    } else {
        0
    };
    let mut previous = start;
    let mut previous_z = (-32i32).wrapping_sub(adjust_for_z_standard(from.z));
    let delta_z = (-32i32)
        .wrapping_sub(adjust_for_z_standard(to.z))
        .wrapping_sub(previous_z);
    for i in 1i32..=4 {
        let next: [i32; 2] =
            std::array::from_fn(|axis| start[axis].wrapping_add(delta[axis].wrapping_mul(i) / 4));
        //705B15/705C57: native accumulates each i*deltaZ/4 onto the PREVIOUS Z.
        let next_z = previous_z.wrapping_add(delta_z.wrapping_mul(i) / 4);
        for (offset, strength) in [
            (0, 255 - 255 * (i - 1) / 4),
            (1, 100 - 25 * (i - 1)),
            (-1, 100 - 25 * (i - 1)),
        ] {
            let mut a = previous;
            let mut b = next;
            a[axis] += offset;
            b[axis] += offset;
            draws.push(SurfaceDraw::Line(SurfaceLine {
                from: a,
                to: b,
                z_adjust: [previous_z, next_z],
                blend: SurfaceLineBlend::Mix { rgb, strength },
            }));
        }
        previous = next;
        previous_z = next_z;
    }
    draws
}

/// Per-composite calls retain Tactical's object order before the Main RNG
/// phase. They are discarded with the UI build, never saved simulation state.
#[derive(Clone, Copy)]
pub(crate) enum TacticalLineCall {
    Opaque { rect: [i32; 4], word: u16 },
    Designation([DriveCoord; 2]),
}

/// The existing action-line producer emits opaque white-texture rectangles.
/// At logical zoom1 their resolved rectangles and RGB565 words can feed the
/// same surface command stream as the following translucent beam. Geometry
/// and action-line admission remain with target_lines, not another producer.
pub(crate) fn drain_opaque(
    calls: &mut Vec<TacticalLineCall>,
    instances: &mut Vec<crate::render::batch::SpriteInstance>,
    camera: [i32; 2],
) {
    for instance in instances.drain(..) {
        debug_assert_eq!(instance.alpha, 1.);
        let rect = [
            instance.position[0] as i32 - camera[0],
            instance.position[1] as i32 - camera[1],
            instance.size[0] as i32,
            instance.size[1] as i32,
        ];
        let word = crate::render::native_surface_format::RGB565
            .pack_rgb8(instance.tint.map(|v| (v * 255.).round() as u8));
        calls.push(TacticalLineCall::Opaque { rect, word });
    }
}

pub(crate) fn composite(
    calls: &[TacticalLineCall],
    wall_ms: u32,
    main: &MainRngDraws,
    view: SurfaceLineViewport,
) -> Vec<SurfaceDraw> {
    let mut draws = Vec::new();
    for call in calls {
        match *call {
            TacticalLineCall::Opaque { rect, word } => {
                draws.push(SurfaceDraw::OpaqueRect { rect, word })
            }
            TacticalLineCall::Designation([from, to]) => {
                draws.extend(build(from, to, wall_ms, main, view))
            }
        }
    }
    draws
}

#[cfg(test)]
#[path = "airstrike_lines_tests.rs"]
mod tests;
