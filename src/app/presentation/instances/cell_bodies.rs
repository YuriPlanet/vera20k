//! The tactical cell body sweep: high-bridge and ordinary overlay bodies in
//! one native order.
//!
//! `Cell_ContentRendering @ 0x006D6D10` walks the view's cells twice: every
//! body, then every shadow. The body sweep calls
//! `CellClass::DrawOverlay_Body @ 0x0047F6A0` (`0x006D7001`) for each in-bounds
//! cell (`0x00568300`) whose overlay is not -1, high bridge or ordinary overlay
//! alike. Diagonals run in descending `X + Y` (`0x006D7023`), each in ascending
//! X (`0x006D7006`/`0x006D700F`). A body is a `0x4E00` SHP draw whose opaque
//! leaf `0x004990E0` keeps the first accepted pixel at equal depth (strict `<`,
//! `0x004991EA..0x004991EC`), so this order decides those ties. Executed
//! traversal: `tools/spatial_oracle/bridge_shadow_render.json` (`traversal`).

use std::cmp::Reverse;

use crate::render::batch::SpriteInstance;
use crate::render::tactical_draw_plan::RenderZPolicy;

/// A cell's place in both native cell sweeps: descending diagonal, then
/// ascending X.
pub(crate) fn cell_sweep_key(rx: u16, ry: u16) -> (Reverse<u32>, u16) {
    (Reverse(u32::from(rx) + u32::from(ry)), rx)
}

/// The atlas a body samples. Every body draws through the same SHP pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellBodyAtlas {
    /// Ordinary overlays (`OverlayAtlas`).
    Overlay,
    /// High-bridge bodies (`BridgeAtlas`).
    Bridge,
}

/// How one uploaded body instance draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellBodyDraw {
    pub atlas: CellBodyAtlas,
    pub render_z: RenderZPolicy,
}

/// One cell's body, before the sweep orders it.
pub(crate) struct CellBody {
    pub cell: (u16, u16),
    pub draw: CellBodyDraw,
    pub instance: SpriteInstance,
}

/// Append `bodies` in sweep order, each instance's draw beside it.
///
/// Valid native overlays, including sloped resources and veins, all use Z.
/// Unregistered compatibility records retain their old no-depth painting
/// order; they have no native OverlayType from which to select a draw policy.
pub(crate) fn submit_cell_body_sweep(
    mut bodies: Vec<CellBody>,
    instances: &mut Vec<SpriteInstance>,
    draws: &mut Vec<CellBodyDraw>,
) {
    // One body per cell, so no two keys tie.
    bodies.sort_unstable_by_key(|body| cell_sweep_key(body.cell.0, body.cell.1));
    let unshaped = |body: &&CellBody| body.draw.render_z == RenderZPolicy::None;
    let painted = bodies.iter().rev().filter(unshaped);
    let swept = bodies.iter().filter(|body| !unshaped(body));
    for body in painted.chain(swept) {
        instances.push(body.instance);
        draws.push(body.draw);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(cell: (u16, u16), atlas: CellBodyAtlas, render_z: RenderZPolicy) -> CellBody {
        CellBody {
            cell,
            draw: CellBodyDraw { atlas, render_z },
            instance: SpriteInstance {
                position: [f32::from(cell.0), f32::from(cell.1)],
                ..Default::default()
            },
        }
    }

    fn cells(instances: &[SpriteInstance]) -> Vec<[u16; 2]> {
        instances
            .iter()
            .map(|instance| instance.position.map(|v| v as u16))
            .collect()
    }

    /// The executed `0x006D6D10` traversal calls the body function per cell,
    /// whatever its overlay, so ordinary and high-bridge bodies interleave.
    #[test]
    fn mixed_bodies_follow_the_native_body_traversal() {
        let packet = crate::test_fixture::json("tools/spatial_oracle/bridge_shadow_render.json");
        let expected: Vec<[u16; 2]> = packet["traversal"]["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["piece"] == "body")
            .map(|call| std::array::from_fn(|i| call["coords"][i].as_u64().unwrap() as u16))
            .collect();
        assert_eq!(expected.len(), 9);
        // Builders emit ordinary cells in index order and bridges apart.
        let mut ordered = expected.clone();
        ordered.sort_by_key(|&[x, y]| (y, x));
        let bodies = ordered
            .iter()
            .enumerate()
            .map(|(i, &[x, y])| {
                let atlas = if i % 2 == 0 {
                    CellBodyAtlas::Bridge
                } else {
                    CellBodyAtlas::Overlay
                };
                body((x, y), atlas, RenderZPolicy::ReadWrite)
            })
            .collect();
        let (mut instances, mut draws) = (Vec::new(), Vec::new());
        submit_cell_body_sweep(bodies, &mut instances, &mut draws);
        assert_eq!(cells(&instances), expected);
        assert_eq!(draws.len(), instances.len());
        assert!(
            draws.windows(2).any(|pair| pair[0].atlas != pair[1].atlas),
            "one sweep, not one batch per atlas"
        );
    }

    #[test]
    fn unshaped_bodies_paint_back_to_front_before_the_sweep() {
        use CellBodyAtlas::{Bridge, Overlay};
        use RenderZPolicy::{None, ReadWrite};
        let (mut instances, mut draws) = (Vec::new(), Vec::new());
        submit_cell_body_sweep(
            vec![
                body((4, 4), Overlay, None),
                body((5, 5), Overlay, ReadWrite),
                body((6, 6), Overlay, None),
                body((3, 3), Bridge, ReadWrite),
            ],
            &mut instances,
            &mut draws,
        );
        assert_eq!(cells(&instances), [[4, 4], [6, 6], [5, 5], [3, 3]]);
        assert_eq!(
            draws.iter().map(|draw| draw.render_z).collect::<Vec<_>>(),
            [None, None, ReadWrite, ReadWrite]
        );
    }
}
