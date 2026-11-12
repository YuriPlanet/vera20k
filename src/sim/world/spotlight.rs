//! BuildingLightClass435820/4361D0/436BE0/436A00, RTTI19, VT7E3AD0.
//! Members: Constructor435820, ScalarDeletingDestructor4370C0, Draw_It435BE0,
//! AI4361D0, GetType4369E0, GetLayer4369F0, PointerExpired436A00,
//! SetBehaviour436BE0, DistanceToIntensity436E80, Limbo437030, Unlimbo437050.
//! RecalcArcPositions436A40 and the plain Destructor435B50 have no call, jump
//! or data reference in the exe, so the game never runs them.
//! Independent ObjectClass child of Building+600. The global8B4190 vector
//! retains construction order; D8 is its constructor index's parity
//! (435B0F..435B3F), never recomputed after removal. No RNG or timer writes.
//! Native parent Limbo (Building445D75..445D81, which the building destructor
//! reaches at 43BD6E) calls the light's vt+0xF8, ObjectUnInit5F65F0 (VT slot
//! 7E3BC8): expiry broadcast, Limbo437030, Object+90 cleared and a deferred
//! delete, so the light stays in 8B4190 until that frame's drain (725C70)
//! runs 4370C0; an orphan's AI (4368E1) retires the same way. gamemd deletes
//! a light directly only at scenario teardown (5345A2).
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
/// BuildingLightClass::Constructor @ 0x00435820's BeamCoords/PivotCoords.
/// The unreferenced RecalcArcPositions @ 0x00436A40 repeats this body.
fn initial_endpoint(parent: [i32; 3], facing: u16, radius: i32) -> [i32; 3] {
    //435988..435AA9: signed facing minus3FFF; double spill before table calls.
    endpoint_for_facing(
        parent,
        facing,
        radius,
        NativeF64Bits::from_bits(0xbf19222d989f5e57),
    )
}
fn endpoint_for_facing(
    parent: [i32; 3],
    facing: u16,
    radius: i32,
    scale: NativeF64Bits,
) -> [i32; 3] {
    let a = save(X::mul(
        X::load_i32(i32::from(facing as i16) - 0x3fff),
        d(scale),
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
/// BuildingLightClass::DistanceToIntensity @ 0x00436E80; AI4361D0 inlines it.
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
    pub glow_flags: u32,
    pub beam_origin: [i32; 3],
    pub beam_edges: Option<[[i32; 3]; 2]>,
    pub beam_z_adjust: [i32; 2],
    pub beam_amount: i32,
}

impl Simulation {
    /// BuildingLightDraw435BE0 uses the child's coordinates, the parent's
    /// virtual center/operational state, and the retained rules. No draw query
    /// mutates gameplay, RNG or timers. `viewer` is the player whose map draws
    /// it: a parent the FoggedObject constructor (`0x004D10D5`) fogged on that
    /// map (`+0x6E7`) draws nothing (`0x00435C44..0x00435C4C`).
    pub fn building_spotlight(
        &self,
        id: u64,
        viewer: Option<crate::sim::intern::InternedId>,
        rules: &RuleSet,
    ) -> Option<BuildingSpotlight> {
        let light = self.substrate.spotlights.get(id)?;
        let parent = light.parent.and_then(|p| self.substrate.entities.get(p))?;
        if !light.alive
            || light.mode == 0
            || !parent.lifecycle.object_alive()
            || (parent.category() != EntityCategory::Structure && !rules.ares())
            || (parent.category() == EntityCategory::Structure
                && self.building_operational_state(parent.stable_id(), rules) != Some(true))
            || viewer.is_some_and(|viewer| self.fog.is_object_fogged(viewer, parent.stable_id()))
        {
            return None;
        }
        #[cfg(feature = "ares")]
        if parent
            .cloak
            .as_ref()
            .is_some_and(|cloak| cloak.state() >= 2)
            || parent.deactivated.is_offline()
            || parent.is_warped_out()
            || crate::sim::movement::air_movement::current_fly_height(
                parent,
                self.resolved_terrain.as_ref(),
            ) < -10
        {
            // BuildingLightDraw_Start1004EB42..1004EB96 (hook 0x435BFA) hides the beam,
            // retaining its child through cloak, EMP and underground travel.
            return None;
        }
        let (start_height, middle_height, edge_height, glow_flags) = (430, 400, 250, 0);
        // Ares BuildingLightClass_Draw_430/_400/_250 (1004EAA0, 1004EA70,
        // 1004EA40; hooks 0x436072, 0x4360D8, 0x4360FF) offset the three
        // heights from `Spotlight.StartHeight`; Draw_Spotlight (1004EAC0,
        // hook 0x435CD3) sets the colour flags.
        #[cfg(feature = "ares")]
        let (start_height, middle_height, edge_height, glow_flags) =
            self.entity_object_type(parent, rules).map_or(
                (start_height, middle_height, edge_height, glow_flags),
                |object| {
                    let options = object.spotlight_rules();
                    let height = options.start_height();
                    (
                        height.max(0),
                        height.wrapping_sub(30).max(400),
                        height.wrapping_sub(180).max(250),
                        options.glow_flags(),
                    )
                },
            );
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
            glow_flags,
            beam_origin: [p[0], p[1], p[2].wrapping_add(start_height)],
            beam_edges,
            beam_z_adjust: [
                crate::util::native_x87::adjust_for_z_standard(p[2].wrapping_add(middle_height))
                    .wrapping_neg(),
                crate::util::native_x87::adjust_for_z_standard(coord[2].wrapping_add(edge_height))
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
                .building
                .building_light
                .light()
                .is_some_and(|id| !self.substrate.spotlights.contains_key(id))
            {
                return Err((entity.stable_id(), "unmatched owned spotlight pointer"));
            }
        }
        Ok(())
    }

    /// BuildingClass::Unlimbo @ 0x00441163..0x00441190 constructs one for a
    /// HasSpotlight type (BuildingLightClass::Constructor @ 0x00435820).
    pub(crate) fn construct_spotlight(&mut self, parent_id: u64, rules: &RuleSet) {
        let Some(parent) = self.substrate.entities.get(parent_id).filter(|e| {
            (e.category() == EntityCategory::Structure || (rules.ares() && e.category().is_foot()))
                && self
                    .entity_object_type(e, rules)
                    .map_or(e.building.building_light.spotlight_capable(), |o| {
                        o.has_spotlight
                    })
                && e.building.building_light.light().is_none()
        }) else {
            return;
        };
        let p = raw(parent);
        let facing = parent.body_facing_current(self.session.binary_frame());
        let params = rules.general.spotlight;
        let coord = initial_endpoint(p, facing, params.location_radius);
        // ObjectUnlimbo5F4F34 queries the coordinate before entry1AC. This
        // class inherits4264C0 (always0) and GetType @ 0x004369E0 (null), so it
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
        let id = self.substrate.allocate_stable_id();
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
            .building
            .building_light
            .attach(id);
        Self::submit_object_display(
            &self.interner,
            &self.terrain_objects,
            &mut self.substrate,
            id,
            // BuildingLightClass::GetLayer @ 0x004369F0 returns Air (3).
            DisplayLayer::AIR,
            rules,
        );
        // BuildingLightClass::Unlimbo @ 0x00437050 registers it with Logic;
        // Limbo @ 0x00437030 is `conceal_spotlight_logic`.
        self.register_spotlight_logic(id);
    }
    /// BuildingClass::Limbo @ 0x00445D75..0x00445D81, in its not-yet-in-Limbo
    /// block (0x004458C6): the light's vt+0xF8, ObjectClass::UnInit @
    /// 0x005F65F0. The expiry broadcast clears Building+0x600
    /// (BuildingClass::PointerExpired @ 0x0044E93A); the light leaves Display
    /// and Logic, dies and keeps its 8B4190 slot until the drain deletes it.
    /// A Foot's light is Ares' own and keeps its direct delete. So does a
    /// building's in an Ares build: its hook 0x441163 skips the stock
    /// construction that stores Building+0x600 and Ares owns the light, so
    /// whether this stock arm still runs there is not established (Ares'
    /// 1004EDE0 was not read). RESIDUAL for Ares builds only: the light is
    /// deleted at once, which decides the sweep direction of a light
    /// constructed before that frame's drain.
    pub(super) fn uninit_spotlight_for_parent_limbo(
        &mut self,
        parent: u64,
        context: UninitContext<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(parent) else {
            return;
        };
        match entity.building.building_light.light() {
            Some(id)
                if entity.category() == EntityCategory::Structure && !context.rules().ares() =>
            {
                self.uninit_with_context(id, context)
            }
            Some(_) => self.destroy_spotlight_for_parent(parent, context),
            None => {}
        }
    }
    /// The direct delete of a parent's light: Ares' Deactivate and type
    /// conversion, and a parent destroyed without its Limbo.
    pub(crate) fn destroy_spotlight_for_parent(&mut self, parent: u64, context: UninitContext<'_>) {
        if let Some(id) = self
            .substrate
            .entities
            .get_mut_if(parent, |e| e.building.building_light.light().is_some())
            .and_then(|e| e.building.building_light.take())
        {
            self.destroy_spotlight(id, context);
        }
    }
    /// BuildingLightClass::ScalarDeletingDestructor @ 0x004370C0 (it inlines
    /// the unreferenced Destructor @ 0x00435B50): Limbo and Logic unregister
    /// (nothing for a light UnInit already limboed), removal from the 8B4190
    /// vector, then ObjectClass's destructor. The drain calls it for a dead
    /// queued light (0x00725D7A).
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
        // Ares BuildingLightClass_SDDTOR (1004EBE0, hook 0x4370C0) clears
        // TechnoExt::Spotlight; BuildingLightClass_CTOR (1004EA20, hook
        // 0x435820) set it. The parent's one owned pointer is both fields.
        if let Some(parent) = parent.and_then(|p| self.substrate.entities.get_mut(p)) {
            if parent.building.building_light.light() == Some(id) {
                parent.building.building_light.take();
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
                            Some(&self.house_alliances),
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

    /// BuildingLightClass::SetBehaviour @ 0x00436BE0.
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
    /// BuildingLightClass::AI @ 0x004361D0.
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
        let mut lost_target = false;
        match light.mode {
            1 => {
                #[cfg(feature = "ares")]
                let params = if parent.category().is_foot() {
                    // BuildingLight_AI1004EC10..1004EDD2: both endpoint
                    // and pivot follow the current Foot facing, not its old
                    // construction cell; the native angle bound becomes0.
                    if let Some(object) = self.entity_object_type(parent, rules) {
                        let options = object.spotlight_rules();
                        let frame = self.session.binary_frame();
                        let facing = if options.follows_secondary() {
                            parent
                                .barrel_facing
                                .as_ref()
                                .map_or(0, |facing| facing.current(frame))
                        } else {
                            parent.body_facing_current(frame)
                        };
                        let endpoint = endpoint_for_facing(
                            parent_raw,
                            facing,
                            options.distance(),
                            NativeF64Bits::from_bits(0xbf1922147658a371),
                        );
                        light.endpoint = endpoint;
                        light.pivot = endpoint;
                    }
                    SpotlightRules {
                        angle: NativeF64Bits::POSITIVE_ZERO,
                        ..params
                    }
                } else {
                    params
                };
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
                    lost_target = true;
                }
            }
            _ => {}
        }
        let scan = (light.mode == 1 || lost_target)
            && self.object_has_trigger_tag(parent_id)
            && (self.building_operational_state(parent_id, rules) == Some(true)
                || (rules.ares() && parent.category().is_foot()));
        let coord = light.coord;
        self.substrate.spotlights.objects.insert(id, light);
        if lost_target {
            //436321: SetBehaviour(1), so the scan below sees behaviour 1.
            self.set_spotlight_behavior(id, 1);
        }
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
                // Ares 3.0 BuildingLightClass_Update_Trigger (10050550, hook
                // 0x4368C9): then 66 (in spotlight) on the same object while
                // it is alive.
                #[cfg(feature = "ares")]
                if self
                    .substrate
                    .entities
                    .get(parent_id)
                    .is_some_and(|parent| parent.lifecycle.object_alive())
                {
                    self.dispatch_object_tag_event(parent_id, 66, Some(parent_id), None, context);
                }
            }
        }
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// BuildingLightClass::PointerExpired @ 0x00436A00 for every light.
    /// `ares` is [`RuleSet::ares`](crate::rules::ruleset::RuleSet::ares).
    pub(super) fn spotlights_pointer_expired(&mut self, id: u64, removed: bool, ares: bool) {
        //436A00 clears the target for either control. Ares1004EBC0 at
        //436A25 skips only the parent clear on DetachAll(false), allowing
        //the beam to reappear after its parent uncloaks. Real removal clears it.
        let clear_parent = removed || !ares;
        for light in self.spotlights.values_mut() {
            if clear_parent && light.parent == Some(id) {
                light.parent = None;
            }
            if light.target == Some(id) {
                light.target = None;
            }
        }
    }
}

#[cfg(test)]
#[path = "spotlight_tests.rs"]
mod tests;
