//! BuildingLightClass435820/4361D0/436BE0/436A00, RTTI19, VT7E3AD0.
//! Independent ObjectClass child of Building+600. The global8B4190 vector
//! retains construction order; D8 is assigned from its constructor index and
//! never recomputed after removal. No RNG or timer writes. Parent Limbo deletes
//! immediately; an orphan's AI uses ObjectUnInit5F65F0/deferred destruction.
use super::display_layers::DisplayLayer;
use super::{Simulation, UninitContext};
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::{RuleSet, SpotlightRules};
use crate::sim::movement::{ground_pose, locomotor::MovementLayer};
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53 as X, X87Value};
use crate::util::retail_trig::TrigTable;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct Spotlight {
    stable_id: u64,
    native_id: i32,
    coord: [i32; 3],
    pivot: [i32; 3],
    endpoint: [i32; 3],
    angle: NativeF64Bits,
    rate: NativeF64Bits,
    increasing: bool,
    mode: i32,
    target: Option<u64>,
    parent: Option<u64>,
    alive: bool,
    #[serde(skip)]
    in_logic_vector: bool,
}
impl Spotlight {
    pub(crate) fn stable_id(&self) -> u64 {
        self.stable_id
    }
    pub(super) fn in_logic_vector(&self) -> bool {
        self.in_logic_vector
    }
    /// LogicClass membership: RegisterObject sets it (`0x0055BAC6`),
    /// UnregisterObject clears it (`0x0055BB27`). Only the logic vector's
    /// owner writes it, and a load rebuilds it from the saved vector.
    pub(super) fn mark_logic_membership(&mut self, member: bool) {
        self.in_logic_vector = member;
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SpotlightStore {
    objects: BTreeMap<u64, Spotlight>,
    order: Vec<u64>,
}
impl SpotlightStore {
    pub(super) fn get(&self, id: u64) -> Option<&Spotlight> {
        self.objects.get(&id)
    }
    pub(super) fn get_mut(&mut self, id: u64) -> Option<&mut Spotlight> {
        self.objects.get_mut(&id)
    }
    pub(super) fn contains_key(&self, id: u64) -> bool {
        self.objects.contains_key(&id)
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&u64, &Spotlight)> {
        self.objects.iter()
    }
    pub(super) fn values_mut(&mut self) -> impl Iterator<Item = &mut Spotlight> {
        self.objects.values_mut()
    }
    pub(super) fn hash_state(&self, h: &mut impl Hasher) {
        if !self.order.is_empty() {
            b"building-light-objects-v1".hash(h);
            self.order.hash(h);
            // The derived +98 flag is rebuilt from Logic after load.
            for id in &self.order {
                let mut light = self.objects[id].clone();
                light.in_logic_vector = false;
                light.hash(h);
            }
        }
    }
}
fn d(value: NativeF64Bits) -> X87Value {
    X::load_f64(value).expect("finite spotlight input")
}
fn save(value: X87Value) -> NativeF64Bits {
    X::store_f64(value).expect("finite spotlight state")
}
fn integer(value: X87Value) -> i32 {
    X::ftol_i32_low_masked(value)
}
fn raw(entity: &crate::sim::game_entity::GameEntity) -> [i32; 3] {
    let c = ground_pose::position_world_coord(&entity.position);
    [c.x, c.y, c.z]
}
fn coords(entity: &crate::sim::game_entity::GameEntity, sim: &Simulation) -> [i32; 3] {
    let c = ground_pose::object_get_coords(entity, sim.resolved_terrain.as_ref());
    [c.x, c.y, c.z]
}
fn initial_endpoint(parent: [i32; 3], facing: u16, radius: i32) -> [i32; 3] {
    //435988..435AA9: signed facing minus3FFF; double spill before table calls.
    let a = save(X::mul(
        X::load_i32(i32::from(facing as i16) - 0x3fff),
        d(NativeF64Bits::from_bits(0xbf19222d989f5e57)),
    ));
    let t = TrigTable::embedded();
    [
        integer(X::add(
            X::load_i32(parent[0]),
            X::mul(t.cos_from_table(d(a)), X::load_i32(radius)),
        )),
        integer(X::sub(
            X::load_i32(parent[1]),
            X::mul(t.sin_from_table(d(a)), X::load_i32(radius)),
        )),
        parent[2],
    ]
}
fn rotate(endpoint: [i32; 3], pivot: [i32; 3], angle: NativeF64Bits) -> Option<[i32; 3]> {
    use crate::util::native_affine as a;
    //436384/436534 spills angle to float, then shared MatrixRotateZ5AF1A0
    //and TransformPoint5AFB80, vector floats, add pivot and ftol.
    let angle = X::load_f32(X::store_f32(d(angle)).ok()?).ok()?;
    let t = TrigTable::embedded();
    let sin = a::store(t.sin_from_table(angle))?;
    let cos = a::store(t.cos_from_table(angle))?;
    let m = a::rotate_z(glam::Mat4::IDENTITY, sin, cos);
    let mut point = [0.0; 3];
    for i in 0..3 {
        point[i] = a::store(X::load_i32(endpoint[i].wrapping_sub(pivot[i])))?;
    }
    let p = a::transform_point(m, glam::Vec3::from_array(point))?.to_array();
    let mut result = [0; 3];
    for i in 0..3 {
        result[i] = integer(X::add(X::load_i32(pivot[i]), a::load(p[i])?));
    }
    Some(result)
}
fn advance_angle(light: &mut Spotlight, rules: SpotlightRules) {
    light.angle = save(X::add(d(light.angle), d(light.rate)));
    let angle = d(light.angle);
    let rate = d(light.rate);
    let half = X::mul(
        d(rules.angle),
        d(NativeF64Bits::from_bits(0x3fe0000000000000)),
    );
    use crate::util::native_x87::X87Ordering::{Greater, Less};
    if light.increasing {
        if X::compare(angle, half) == Greater {
            light.rate = save(X::sub(rate, d(rules.acceleration)));
            if X::compare(d(light.rate), X::load_i32(0)) == Less {
                light.rate = NativeF64Bits::from_bits(0);
                light.increasing = false;
            }
        } else if X::compare(rate, d(rules.speed)) == Less {
            light.rate = save(X::add(d(rules.acceleration), rate));
        }
    } else if X::compare(angle, X::neg(half)) == Less {
        light.rate = save(X::add(d(rules.acceleration), rate));
        if X::compare(d(light.rate), X::load_i32(0)) == Greater {
            light.rate = NativeF64Bits::from_bits(0);
            light.increasing = true;
        }
    } else if X::compare(X::neg(d(rules.speed)), rate) == Less {
        light.rate = save(X::sub(rate, d(rules.acceleration)));
    }
}
fn bucket(parent: [i32; 3], light: [i32; 3], rules: SpotlightRules) -> i32 {
    let dist = crate::util::native_x87::distance_3d_leptons(parent, light);
    if dist < rules.location_radius {
        return 0;
    }
    //436E80: signed divide, no clamp. Reject native division-by-zero authored
    //state deterministically instead of crashing the simulation process.
    dist.wrapping_sub(rules.location_radius)
        .checked_div(rules.movement_radius.wrapping_sub(rules.location_radius) / 10)
        .unwrap_or(0)
}

/// Read-only435BE0 draw inputs, derived from the independent child pose.
#[derive(Debug, Clone, Copy)]
pub struct BuildingSpotlight {
    pub coord: [i32; 3],
    pub glow_stage: u8,
    pub beam_origin: [i32; 3],
    pub beam_edges: Option<[[i32; 3]; 2]>,
    pub beam_z_adjust: [i32; 2],
    pub beam_amount: i32,
}

impl Simulation {
    /// BuildingLightDraw435BE0 uses the child's coordinates, the parent's
    /// virtual center/operational state, and the retained rules. No draw query
    /// mutates gameplay, RNG or timers. Native +6E7's represented constructor
    /// state is false; foreign native save bytes are not imported by this engine.
    pub fn building_spotlight(&self, id: u64, rules: &RuleSet) -> Option<BuildingSpotlight> {
        let light = self.substrate.spotlights.get(id)?;
        let parent = light.parent.and_then(|p| self.substrate.entities.get(p))?;
        if !light.alive
            || light.mode == 0
            || !parent.lifecycle.object_alive()
            || parent.category() != EntityCategory::Structure
            || self.building_operational_state(parent.stable_id(), rules) != Some(true)
        {
            return None;
        }
        let p = coords(parent, self);
        let coord = light.coord;
        let params = rules.general.spotlight;
        let distance = crate::util::native_x87::distance_3d_leptons(p, coord);
        let b = bucket(p, coord, params);
        let stage = if light.mode == 3 && distance > params.location_radius {
            80_i32.wrapping_add(b).clamp(0, 89)
        } else {
            80
        };
        let width = params.radius.wrapping_add(integer(X::mul(
            X::load_i32(b),
            d(NativeF64Bits::from_bits(0x4017e4b17e4b17e4)),
        )));
        let beam_edges = if distance >= width && distance > 0 && width >= 0 {
            let ratio = X::div(X::load_i32(width), X::load_i32(distance)).unwrap();
            let angle = save(
                crate::util::retail_trig::AcosTable::retail()
                    .asin_lookup(ratio)
                    .1,
            );
            Some([
                rotate(coord, p, angle)?,
                rotate(coord, p, save(X::neg(d(angle))))?,
            ])
        } else {
            None
        };
        Some(BuildingSpotlight {
            coord,
            glow_stage: stage as u8,
            beam_origin: [p[0], p[1], p[2].wrapping_add(430)],
            beam_edges,
            beam_z_adjust: [
                crate::util::native_x87::adjust_for_z_standard(p[2].wrapping_add(400))
                    .wrapping_neg(),
                crate::util::native_x87::adjust_for_z_standard(coord[2].wrapping_add(250))
                    .wrapping_neg(),
            ],
            beam_amount: 75_i32.wrapping_sub(6_i32.wrapping_mul(b)),
        })
    }

    pub(super) fn spotlight_alive(&self, id: u64) -> Option<bool> {
        self.substrate.spotlights.get(id).map(|l| l.alive)
    }
    pub(super) fn finish_spotlight_retirement(&mut self, id: u64) {
        self.conceal_spotlight_logic(id);
        if let Some(light) = self.substrate.spotlights.get_mut(id) {
            light.alive = false;
        }
    }
    pub(crate) fn validate_restored_spotlights(&self) -> Result<(), (u64, &'static str)> {
        let mut order = std::collections::BTreeSet::new();
        for &id in &self.substrate.spotlights.order {
            if !order.insert(id) || !self.substrate.spotlights.contains_key(id) {
                return Err((id, "invalid construction vector"));
            }
        }
        for (&id, l) in self.substrate.spotlights.iter() {
            if !order.contains(&id) {
                return Err((id, "missing construction vector member"));
            }
            if l.parent
                .is_some_and(|p| !self.substrate.entities.contains(p))
                || l.target
                    .is_some_and(|p| !self.substrate.entities.contains(p))
            {
                return Err((id, "unmatched parent or target pointer"));
            }
            if !l.alive && self.substrate.logic.as_slice().contains(&id) {
                return Err((id, "retired spotlight remains in Logic"));
            }
        }
        for entity in self.substrate.entities.values() {
            if entity
                .building_light
                .light()
                .is_some_and(|id| !self.substrate.spotlights.contains_key(id))
            {
                return Err((entity.stable_id(), "unmatched owned spotlight pointer"));
            }
        }
        Ok(())
    }

    pub(super) fn construct_spotlight(&mut self, parent_id: u64, rules: Option<&RuleSet>) {
        let Some(parent) = self.substrate.entities.get(parent_id).filter(|e| {
            e.category() == EntityCategory::Structure
                && rules
                    .and_then(|r| self.object_type(e.type_ref(), r))
                    .map_or(e.building_light.spotlight_capable(), |o| o.has_spotlight)
                && e.building_light.light().is_none()
        }) else {
            return;
        };
        let p = raw(parent);
        let facing = parent.body_facing_current(self.session.binary_frame());
        let params = rules.map_or_else(SpotlightRules::default, |r| r.general.spotlight);
        let coord = initial_endpoint(p, facing, params.location_radius);
        // ObjectUnlimbo5F4F34 queries the coordinate before entry1AC. This
        // class inherits4264C0 (always0) and GetType4369E0 (null), so it
        // has no terrain placement rejection/height rewrite or cell footprint.
        if !self.object_placement_scope_active() {
            if let Some(terrain) = self.resolved_terrain.as_ref() {
                crate::map::resolved_terrain::NativeCellQuery::canonical(terrain)
                    .lookup_world(coord[0], coord[1]);
            } else {
                self.effective_shared_cell_dummy()
                    .stamp_coord(coord[0] / 256, coord[1] / 256);
            }
        }
        let pivot = initial_endpoint(p, facing, params.movement_radius.wrapping_neg());
        let id = self.allocate_stable_id();
        let native_id = self.next_native_runtime_id();
        let light = Spotlight {
            stable_id: id,
            native_id,
            coord,
            pivot,
            endpoint: coord,
            angle: NativeF64Bits::from_bits(0),
            rate: NativeF64Bits::from_bits(0),
            increasing: self.substrate.spotlights.order.len() % 2 != 0,
            mode: 1,
            target: None,
            parent: Some(parent_id),
            alive: true,
            in_logic_vector: false,
        };
        self.substrate.spotlights.objects.insert(id, light);
        self.substrate.spotlights.order.push(id);
        self.substrate
            .entities
            .get_mut(parent_id)
            .unwrap()
            .building_light
            .attach(id);
        self.submit_object_display(id, DisplayLayer::AIR, rules);
        self.register_spotlight_logic(id);
    }
    pub(super) fn destroy_spotlight_for_parent(&mut self, parent: u64, context: UninitContext<'_>) {
        if let Some(id) = self
            .substrate
            .entities
            .get_mut(parent)
            .and_then(|e| e.building_light.take())
        {
            self.destroy_spotlight(id, context);
        }
    }
    pub(super) fn destroy_spotlight(&mut self, id: u64, context: UninitContext<'_>) {
        let Some(parent) = self.substrate.spotlights.get(id).map(|s| s.parent) else {
            return;
        };
        self.notify_pointer_expired(id, context);
        self.conceal_spotlight_logic(id);
        self.substrate.spotlights.objects.remove(&id);
        self.substrate.spotlights.order.retain(|&p| p != id);
        self.substrate
            .pending_delete
            .remove(super::substrate::DeferredDeleteTicket::Object(id));
        if let Some(parent) = parent.and_then(|p| self.substrate.entities.get_mut(p)) {
            if parent.building_light.light() == Some(id) {
                parent.building_light.take();
            }
        }
    }
    pub(super) fn spotlights_pointer_expired(&mut self, id: u64) {
        //436A00 always nulls matching parent/target, regardless of control flag.
        for light in self.substrate.spotlights.values_mut() {
            if light.parent == Some(id) {
                light.parent = None;
            }
            if light.target == Some(id) {
                light.target = None;
            }
        }
    }
    fn spotlight_candidates(
        &self,
        coord: [i32; 3],
        owner: crate::sim::intern::InternedId,
    ) -> Vec<u64> {
        let cell = [
            crate::util::lepton::lepton_to_cell_packed(coord[0]),
            crate::util::lepton::lepton_to_cell_packed(coord[1]),
        ];
        let mut result = Vec::new();
        for dx in -1_i16..=1 {
            for dy in -1_i16..=1 {
                let requested = (cell[0].wrapping_add(dx), cell[1].wrapping_add(dy));
                let actual = if let Some(terrain) = self.resolved_terrain.as_ref() {
                    let query = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
                    let identity = query.lookup(requested);
                    if identity == crate::map::cell_index::NativeCellIdentity::Dummy {
                        continue;
                    }
                    query.coord(identity)
                } else {
                    self.effective_shared_cell_dummy()
                        .stamp_coord(i32::from(requested.0), i32::from(requested.1));
                    requested
                };
                let xy = (actual.0 as u16, actual.1 as u16);
                let mut current = self
                    .substrate
                    .occupancy
                    .get(xy.0, xy.1)
                    .and_then(|l| l.first_on_layer(MovementLayer::Ground));
                while let Some(id) = current {
                    if let Some(e) = self.substrate.entities.get(id)
                        && matches!(
                            e.category(),
                            EntityCategory::Unit | EntityCategory::Infantry
                        )
                        && !crate::sim::combat::combat_weapon::is_ally_by_object(
                            Some(self.house_alliances.as_map()),
                            &self.interner,
                            owner,
                            e.owner(),
                        )
                    {
                        result.push(id);
                    }
                    current = self
                        .substrate
                        .occupancy
                        .get(xy.0, xy.1)
                        .and_then(|l| l.next_on_layer(MovementLayer::Ground, id));
                }
            }
        }
        result
    }
    pub(crate) fn spotlight_object_coord(&self, id: u64) -> Option<[i32; 3]> {
        self.substrate.spotlights.get(id).map(|l| l.coord)
    }

    pub(crate) fn set_spotlight_behavior(&mut self, id: u64, mode: i32) {
        let Some(light) = self.substrate.spotlights.get_mut(id) else {
            return;
        };
        light.angle = NativeF64Bits::from_bits(0);
        light.mode = mode;
        if mode != 3 {
            return;
        }
        let coord = light.coord;
        let parent = light.parent;
        let Some(owner) = parent
            .and_then(|p| self.substrate.entities.get(p))
            .map(|e| e.owner())
        else {
            return;
        };
        let mut best = 9_999_999;
        let mut target = None;
        for id in self.spotlight_candidates(coord, owner) {
            let dist = crate::util::native_x87::distance_3d_leptons(
                coord,
                coords(self.substrate.entities.get(id).unwrap(), self),
            );
            if dist < best {
                best = dist;
                target = Some(id);
            }
        }
        if target.is_some() {
            self.substrate.spotlights.get_mut(id).unwrap().target = target;
        }
    }
    pub(super) fn visit_spotlight(&mut self, id: u64, rules: &RuleSet, context: UninitContext<'_>) {
        let Some(mut light) = self.substrate.spotlights.get(id).cloned() else {
            return;
        };
        let Some(parent) = light
            .parent
            .and_then(|p| self.substrate.entities.get(p))
            .filter(|p| p.lifecycle.object_alive())
        else {
            self.uninit_with_context(id, context);
            return;
        };
        let parent_id = parent.stable_id();
        let parent_raw = raw(parent);
        let parent_coord = coords(parent, self);
        let owner = parent.owner();
        let params = rules.general.spotlight;
        // Authored exponent overflow can survive the native floating reader.
        // Motion's finite numeric owner cannot represent it: retain the last
        // drawable pose instead of panicking or clamping the retained INI.
        // Parent expiry above still follows the ordinary cleanup path.
        if [
            params.speed,
            params.acceleration,
            params.angle,
            light.angle,
            light.rate,
        ]
        .into_iter()
        .any(|value| !f64::from_bits(value.bits()).is_finite())
        {
            return;
        }
        match light.mode {
            1 => {
                advance_angle(&mut light, params);
                let Some(coord) = rotate(light.endpoint, light.pivot, light.angle) else {
                    return;
                };
                light.coord = coord;
            }
            2 => {
                let angle = X::add(X::mul(d(params.speed), X::load_i32(4)), d(light.angle));
                let tau = d(NativeF64Bits::from_bits(0x401921fb54442d18));
                light.angle = save(
                    if X::compare(angle, tau) == crate::util::native_x87::X87Ordering::Greater {
                        X::sub(angle, tau)
                    } else {
                        angle
                    },
                );
                let Some(coord) = rotate(light.endpoint, parent_raw, light.angle) else {
                    return;
                };
                light.coord = coord;
            }
            3 => {
                if let Some(target) = light
                    .target
                    .and_then(|t| self.substrate.entities.get(t))
                    .filter(|t| t.lifecycle.object_alive())
                    .filter(|t| {
                        crate::util::native_x87::distance_3d_leptons(parent_coord, coords(t, self))
                            < params.movement_radius
                    })
                {
                    light.coord = crate::util::native_coord::interpolate(
                        light.coord,
                        raw(target),
                        X::load_f32(NativeF32Bits::from_bits(0x3e800000)).unwrap(),
                    );
                } else {
                    //436321 resets mode then submits uninitialized stack XYZ.
                    //Keep last coordinates for this transition; native undefined
                    //stack contents are not a portable gameplay contract.
                    light.mode = 1;
                    light.angle = NativeF64Bits::from_bits(0);
                }
            }
            _ => {}
        }
        let scan = light.mode == 1
            && self.object_has_trigger_tag(parent_id)
            && self.building_operational_state(parent_id, rules) == Some(true);
        let coord = light.coord;
        self.substrate.spotlights.objects.insert(id, light);
        if scan {
            let b = bucket(parent_coord, coord, params);
            let threshold = params.radius.wrapping_add(30).wrapping_add(integer(X::mul(
                X::load_i32(b),
                d(NativeF64Bits::from_bits(0x4017e4b17e4b17e4)),
            )));
            let seen = self
                .spotlight_candidates(coord, owner)
                .into_iter()
                .any(|candidate| {
                    crate::util::native_x87::distance_3d_leptons(
                        coord,
                        coords(self.substrate.entities.get(candidate).unwrap(), self),
                    ) < threshold
                });
            if seen {
                self.dispatch_object_tag_event(parent_id, 35, Some(parent_id), None, context);
            }
        }
    }
}

#[cfg(test)]
#[path = "spotlight_tests.rs"]
mod tests;
