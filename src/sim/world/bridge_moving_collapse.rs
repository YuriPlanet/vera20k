//! Cell487720..487941: post-driver damage to neighboring Foot objects whose
//! active locomotor still targets the selected collapsed deck coordinate.
//! Source: original gamemd1cdd1180, full546B/162instructions. This is separate
//! from487A10: deck then ground, cached Next before damage, no admission7 gate.

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::components::DriveCoord;
use crate::sim::movement::at_coord::AtCoordQuery;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{CellObjectMember, RawCellKey};
use crate::sim::world::{FrameEffects, Simulation};

pub(super) fn apply(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    point: (i16, i16),
    effects: FrameEffects<'_>,
) {
    let (selected, probe) = {
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .expect("bridge damage terrain");
        let cells = NativeCellQuery::canonical(terrain);
        let selected = cells.lookup(point);
        let key = RawCellKey::from_native(terrain, selected);
        if sim
            .substrate
            .raw_cell_occupation
            .bits_at(key, MovementLayer::Bridge)
            == 0
        {
            return;
        }
        // Original captures centerXY, then Map578080 groundZ+89E7B4 once.
        // Use its existing world lookup, including signed/Dummy restamping;
        // the selected Cell's own ground fields are not a substitute.
        let [x, y] = crate::sim::cell_kernel::native_cell_own_xy(selected, &cells);
        let point = DriveCoord { x, y, z: 0 };
        let Ok(z) = crate::sim::movement::ground_pose::query_ground_height(&cells, point) else {
            return; // Unsupported raw slope, not an invented fallback height.
        };
        (
            selected,
            DriveCoord {
                x,
                y,
                z: z.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS),
            },
        )
    };
    for dx in -2i16..=2 {
        for dy in -2i16..=2 {
            let neighbor = {
                let terrain = sim
                    .resolved_terrain
                    .as_ref()
                    .expect("bridge damage terrain");
                //48778E rereads selectedCell+24 each iteration. A nested
                //lookup can stamp the same shared Dummy; retain its identity.
                let p = terrain.native_cell_coord(selected);
                terrain.native_cell_identity((p.0.wrapping_add(dx), p.1.wrapping_add(dy)))
            };
            if neighbor == selected {
                continue;
            }
            let NativeCellIdentity::Real(index) = neighbor else {
                // Shared Dummy object-list contents are not represented by
                //OccupancyGrid; no whole raw/Dummy closure is claimed.
                continue;
            };
            for layer in [MovementLayer::Bridge, MovementLayer::Ground] {
                let p = {
                    let cell = &sim.resolved_terrain.as_ref().unwrap().cells()[index];
                    (cell.rx, cell.ry)
                };
                let mut current = sim.cell_objects(p, layer).next();
                while let Some(member) = current {
                    //4877D9/487880 capture Object+30 before any receiver.
                    let next = sim.next_cell_object(member);
                    if let CellObjectMember::Entity(id) = member
                        && let Some(entity) = sim.entities().get(id)
                        && entity.category().is_foot()
                        && AtCoordQuery::from_entity(entity).is_some_and(|q| q.matches(probe))
                    {
                        let damage = entity.health.current();
                        let warhead = sim.interner.intern(&rules.bridge_warheads.c4_name);
                        let event = EntityDamageEvent::direct_receiver(
                            id,
                            damage,
                            0,
                            RAD_NO_ATTACKER,
                            None,
                            warhead,
                            ReceiverCallFlags {
                                ignore_defenses: true,
                                arg6: true,
                            },
                        );
                        sim.commit_direct_damage_receiver(rules, registry, event, effects);
                    }
                    current = next;
                }
            }
        }
    }
}
