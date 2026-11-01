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
    use crate::util::native_x87::{NativeF64Bits, X87Chop53 as X};
    use crate::util::retail_trig::TrigTable;
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

/// Phobos `TechnoClass_DrawAirstrikeFlare_LineColor` (`0x007058F6`) inputs:
/// the drawing techno type's resolved `AirstrikeLineColor` and
/// `[AudioVisual] AirstrikeLineZAdjust`. Stock builds carry nothing.
#[derive(Clone, Copy)]
pub(crate) struct FlareStyle {
    #[cfg(feature = "phobos")]
    pub(crate) color: [u8; 3],
    #[cfg(feature = "phobos")]
    pub(crate) z_adjust: i32,
}

/// One designation's flare shade.
fn shade(main: &MainRngDraws) -> i32 {
    //705905 draws the stock 190..270 shade. Phobos's hook at 7058F6 resumes
    //at 705976, past that call, and draws its 745..1000 scale from the same
    //process Main stream instead.
    #[cfg(not(feature = "phobos"))]
    let random = main.ranged(190, 270);
    #[cfg(feature = "phobos")]
    let random = main.ranged(745, 1000);
    random
}

/// The flare shade each designating Techno drew at the last composite:
/// Techno705860 requests one per Main_Tick's composite, and a draw between
/// composites shows the same beam again.
#[derive(Debug, Default)]
pub(crate) struct AirstrikeShades {
    shades: Vec<(u64, i32)>,
}

impl AirstrikeShades {
    /// One composite's requests, in Tactical's designation order (after the
    /// Laser and Bolt requests, `0x006D48F1`).
    pub(crate) fn step(&mut self, calls: &[TacticalLineCall], main: &MainRngDraws) {
        self.shades.clear();
        for call in calls {
            if let TacticalLineCall::Designation { id, .. } = *call {
                self.shades.push((id, shade(main)));
            }
        }
    }

    fn get(&self, id: u64) -> Option<i32> {
        self.shades
            .iter()
            .find(|(held, _)| *held == id)
            .map(|&(_, shade)| shade)
    }
}

///705860 requests one Main ranged value, even if clipping/A/Z hides the beam.
///The stock color is red/orange independent of Flare.IsHouseColor and
///LaserTargetColor.
fn geometry(
    from: DriveCoord,
    to: DriveCoord,
    random: i32,
    style: FlareStyle,
    view: SurfaceLineViewport,
) -> Vec<SurfaceDraw> {
    use crate::util::native_x87::adjust_for_z_standard;
    let project = |p: DriveCoord| {
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(p.x, p.y, p.z);
        [x as i32 - view.camera[0], y as i32 - view.camera[1]]
    };
    let start = project(from);
    let end = project(to);
    let start_z = (-32i32).wrapping_sub(adjust_for_z_standard(from.z));
    let end_z = (-32i32).wrapping_sub(adjust_for_z_standard(to.z));
    //70597E draws the dot only when the target end is the higher one.
    #[cfg(not(feature = "phobos"))]
    let (rgb, dot_word, dot) = {
        let _ = style;
        let rgb = if random <= 255 {
            [random as u8, 0, 0]
        } else {
            [
                255,
                (55 * (random - 256) / 7) as u8,
                (15 * (random - 256) / 7) as u8,
            ]
        };
        (rgb, 0xF800, end[1] < start[1])
    };
    // Phobos `TechnoClass_DrawAirstrikeFlare_LineColor` gives the whole beam
    // one depth, `min(zSrc, zDest) + AirstrikeLineZAdjust`, scales each line
    // channel to `(BYTE)(base * RandomRanged(745,1000) / 1000.0)` and leaves
    // the dot the unscaled `RGB_To_Int(base)` word.
    // `TechnoClass_DrawAirstrikeFlare_DotColor` (`0x0070597A`) then always
    // draws the dot (`src/Ext/Techno/Hooks.Airstrike.cpp`).
    #[cfg(feature = "phobos")]
    let (start_z, end_z) = {
        let z = start_z.min(end_z).wrapping_add(style.z_adjust);
        (z, z)
    };
    #[cfg(feature = "phobos")]
    let (rgb, dot_word, dot) = {
        let percentage = f64::from(random) / 1000.0;
        (
            style
                .color
                .map(|channel| (f64::from(channel) * percentage) as u8),
            crate::render::native_surface_format::RGB565.pack_rgb8(style.color),
            true,
        )
    };
    let mut draws = Vec::with_capacity(16);
    //70597E: ordinary XSurface lines clip against the whole surface, with no A/Z.
    if dot {
        let [x, y] = end;
        for rect in [
            [x - 2, y + 1, 4, 1],
            [x - 2, y, 4, 1],
            [x - 1, y - 1, 1, 4],
            [x, y - 1, 1, 4],
        ] {
            draws.push(SurfaceDraw::OpaqueRect {
                rect,
                word: dot_word,
            });
        }
    }
    let delta = [end[0].wrapping_sub(start[0]), end[1].wrapping_sub(start[1])];
    let axis = if delta[0].wrapping_abs() > delta[1].wrapping_abs() {
        1
    } else {
        0
    };
    let mut previous = start;
    let mut previous_z = start_z;
    let delta_z = end_z.wrapping_sub(start_z);
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
    Opaque {
        rect: [i32; 4],
        word: u16,
    },
    Designation {
        /// The designating Techno.
        id: u64,
        coords: [DriveCoord; 2],
        style: FlareStyle,
    },
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

/// The action stores and designation beams, each beam with its last
/// composite's shade and its target wobbled by that composite's clock
/// (`0x006D482D`). A designation that has drawn no shade yet waits for the
/// next composite.
pub(crate) fn composite(
    calls: &[TacticalLineCall],
    wall_ms: u32,
    shades: &AirstrikeShades,
    view: SurfaceLineViewport,
) -> Vec<SurfaceDraw> {
    let mut draws = Vec::new();
    for call in calls {
        match *call {
            TacticalLineCall::Opaque { rect, word } => {
                draws.push(SurfaceDraw::OpaqueRect { rect, word })
            }
            TacticalLineCall::Designation {
                id,
                coords: [from, to],
                style,
            } => {
                if let Some(shade) = shades.get(id) {
                    draws.extend(geometry(from, wobble(to, wall_ms), shade, style, view));
                }
            }
        }
    }
    draws
}

#[cfg(test)]
#[path = "airstrike_lines_tests.rs"]
mod tests;
