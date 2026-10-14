//! Live487A10 bridge damage/repair receivers. Both passes resume against current links.
use super::*;
use crate::map::entities::EntityCategory;
use crate::sim::bridge_state::occupants::{self, BridgeOccupantHost};
use crate::sim::combat::{
    EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TerrainDamageEvent,
};
use crate::sim::movement::{at_coord::AtCoordQuery, locomotor::MovementLayer};
use crate::sim::occupancy::CellObjectMember;

#[path = "bridge_occupant_admission.rs"]
mod admission;

pub(super) fn apply(
    live: &mut LivePublication<'_>,
    selected: Cell,
    damage_mode: u8,
) -> Result<(), String> {
    occupants::recheck_occupants(&mut Occupants { live }, selected, damage_mode)
}

#[derive(Clone, Copy)]
struct Member {
    object: CellObjectMember,
    list_cell: CellCoord,
}

struct Occupants<'a, 'world> {
    live: &'a mut LivePublication<'world>,
}

impl Occupants<'_, '_> {
    fn members(
        &self,
        p: CellCoord,
        layer: MovementLayer,
    ) -> impl Iterator<Item = CellObjectMember> + '_ {
        self.live.sim.cell_objects((p.0 as u16, p.1 as u16), layer)
    }
}

impl BridgeOccupantHost for Occupants<'_, '_> {
    type Cell = Cell;
    type Object = Member;
    type Error = String;
    fn coord(&self, cell: Cell) -> CellCoord {
        self.live.coord(cell)
    }
    fn lookup(&mut self, p: CellCoord) -> Cell {
        self.live.lookup(p)
    }
    fn ground_head(&self, cell: Cell) -> Option<Member> {
        if cell == Cell::Dummy {
            return None;
        }
        let p = self.live.coord(cell);
        self.members(p, MovementLayer::Ground)
            .next()
            .map(|object| Member {
                object,
                list_cell: p,
            })
    }
    fn next_object(&self, member: Member) -> Option<Member> {
        // For an object moved by a synchronous receiver, its Next now belongs
        // to its new list. Removed/unmarked objects have no surviving link.
        let (cell, _) = self.live.sim.cell_object_list_location(member.object)?;
        self.live
            .sim
            .next_cell_object(member.object)
            .map(|object| Member {
                object,
                list_cell: (cell.0 as i16, cell.1 as i16),
            })
    }

    fn is_foot(&self, member: Member) -> bool {
        match member.object {
            CellObjectMember::Entity(id) => self
                .live
                .sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| e.category() != EntityCategory::Structure),
            CellObjectMember::Terrain(_) => false,
        }
    }
    fn is_techno(&self, member: Member) -> bool {
        matches!(member.object, CellObjectMember::Entity(_))
    }
    fn type_is_jumpjet(&mut self, member: Member) -> Result<bool, String> {
        let CellObjectMember::Entity(id) = member.object else {
            return Err("bridge JumpJet type receiver is not a Techno".into());
        };
        let entity = self
            .live
            .sim
            .substrate
            .entities
            .get(id)
            .ok_or("bridge JumpJet type receiver has retired object")?;
        self.live
            .sim
            .object_type(entity.type_ref(), self.live.rules)
            .map(|kind| kind.jumpjet)
            .ok_or("bridge JumpJet receiver has no type".into())
    }
    fn admission(&mut self, member: Member, cell: Cell) -> Result<i32, String> {
        admission::impassable(self.live, member.object, cell).map(|hard| if hard { 7 } else { 0 })
    }
    fn abstract_kind(&mut self, member: Member) -> Result<i32, String> {
        match member.object {
            CellObjectMember::Terrain(_) => Ok(36),
            CellObjectMember::Entity(id) => self
                .live
                .sim
                .substrate
                .entities
                .get(id)
                .map(|e| match e.category() {
                    EntityCategory::Unit => 1,
                    EntityCategory::Aircraft => 2,
                    EntityCategory::Structure => 6,
                    EntityCategory::Infantry => 15,
                })
                .ok_or("bridge occupant kind receiver has retired object".into()),
        }
    }
    fn current_health(&self, member: Member) -> i32 {
        match member.object {
            CellObjectMember::Entity(id) => self
                .live
                .sim
                .substrate
                .entities
                .get(id)
                .map_or(0, |e| e.health.current()),
            CellObjectMember::Terrain(id) => self
                .live
                .sim
                .production
                .terrain_objects
                .get(&id)
                .map_or(0, |e| e.health),
        }
    }
    fn receive_damage(&mut self, member: Member, damage: i32) -> Result<(), String> {
        let warhead = self
            .live
            .sim
            .interner
            .intern(&self.live.rules.bridge_warheads.c4_name);
        match member.object {
            CellObjectMember::Entity(id) => {
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
                self.live.sim.commit_direct_damage_receiver(
                    self.live.rules,
                    self.live.registry,
                    event,
                    self.live.frame_effects,
                );
            }
            CellObjectMember::Terrain(id) => {
                let event = TerrainDamageEvent {
                    stable_id: id,
                    rx: member.list_cell.0 as u16,
                    ry: member.list_cell.1 as u16,
                    damage,
                    distance_leptons: 0,
                    warhead_ref: warhead,
                    near_center_ic_isolation_eligible: false,
                };
                self.live.sim.commit_direct_terrain_damage_receiver(
                    self.live.rules,
                    self.live.registry,
                    event,
                    self.live.frame_effects,
                );
            }
        }
        Ok(())
    }
    fn ground_probe(&mut self, cell: Cell) -> Result<[i32; 3], String> {
        let p = self.live.coord(cell);
        let xy = [i32::from(p.0) * 256 + 128, i32::from(p.1) * 256 + 128];
        let (level, slope) = match cell {
            Cell::Real(index) => {
                let c = &self.live.terrain().cells()[index];
                (c.level, c.slope_type)
            }
            Cell::Dummy => {
                let c = self.live.terrain().shared_cell_dummy().snapshot();
                (c.level as u8, c.slope_type)
            }
        };
        let z = crate::util::lepton::ground_height_leptons(level, slope, xy[0], xy[1])
            .map_err(|e| format!("bridge occupant ground slope: {e:?}"))?;
        Ok([xy[0], xy[1], z])
    }
    fn is_at_coord(&mut self, member: Member, probe: [i32; 3]) -> Result<bool, String> {
        let CellObjectMember::Entity(id) = member.object else {
            return Ok(false);
        };
        let e = self
            .live
            .sim
            .substrate
            .entities
            .get(id)
            .ok_or("bridge occupant IsAtCoord retired object")?;
        if e.locomotor.is_none() {
            return Err("bridge occupant Foot has no active locomotor".into());
        }
        Ok(AtCoordQuery::from_entity(e).is_some_and(|q| {
            q.matches(crate::sim::components::DriveCoord {
                x: probe[0],
                y: probe[1],
                z: probe[2],
            })
        }))
    }
}
