//! Retained native voxel-library slots. File probes are deferred until asset
//! binding; references copy an allocation generation, never a future type name.
//! Native: ObjectType LoadVoxel5F8110, Building45FA90, VoxelAnim74B050/74B810.
//! Native copies can outlive source frees. VERA's established retired-reference
//! policy resolves those generations to None, never to a replacement by name.
//! This deliberately avoids reproducing native use-after-free/double-free output.
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) enum LibraryKind {
    Vxl,
    Hva,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct LibraryRequest {
    pub kind: LibraryKind,
    pub image: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
enum Condition {
    False,
    True,
    Present(LibraryRequest),
    Valid(LibraryRequest),
    Not(u32),
    And(u32, u32),
    Or(u32, u32),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
enum Reference {
    Null,
    Allocation(u32),
    Select(u32, u32, u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct Allocation {
    request: LibraryRequest,
    retired: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct ModelPair {
    pub vxl: u32,
    pub hva: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct ModelGunSlots {
    pub turret: ModelPair,
    pub barrel: ModelPair,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct ModelSlots {
    pub main: ModelPair,
    pub auxiliary: ModelPair,
    pub barrel: ModelPair,
    pub indexed: Vec<ModelGunSlots>,
}

impl ModelSlots {
    fn references_mut(&mut self) -> impl Iterator<Item = &mut u32> {
        [&mut self.main, &mut self.auxiliary, &mut self.barrel]
            .into_iter()
            .chain(
                self.indexed
                    .iter_mut()
                    .flat_map(|gun| [&mut gun.turret, &mut gun.barrel]),
            )
            .flat_map(|pair| [&mut pair.vxl, &mut pair.hva])
    }
}

/// The fields already read when TechnoType71609C calls the shared loader.
#[derive(Debug, Clone)]
pub(crate) struct GenericLoadInput {
    pub image: String,
    pub unit: bool,
    pub apc: bool,
    pub turret: bool,
    pub turret_count: i32,
    pub gattling: bool,
    pub no_spawn_alt: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct VoxelModelPlan {
    conditions: Vec<Condition>,
    references: Vec<Reference>,
    allocations: Vec<Allocation>,
    #[serde(skip)]
    condition_index: BTreeMap<Condition, u32>,
    #[serde(skip)]
    reference_index: BTreeMap<Reference, u32>,
}

impl PartialEq for VoxelModelPlan {
    fn eq(&self, other: &Self) -> bool {
        self.conditions == other.conditions
            && self.references == other.references
            && self.allocations == other.allocations
    }
}
impl Eq for VoxelModelPlan {}
impl std::hash::Hash for VoxelModelPlan {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.conditions.hash(h);
        self.references.hash(h);
        self.allocations.hash(h);
    }
}

impl Default for VoxelModelPlan {
    fn default() -> Self {
        Self {
            conditions: vec![Condition::False, Condition::True],
            references: vec![Reference::Null],
            allocations: Vec::new(),
            condition_index: BTreeMap::new(),
            reference_index: BTreeMap::new(),
        }
    }
}

impl VoxelModelPlan {
    fn condition(&mut self, value: Condition) -> u32 {
        if self.condition_index.len() != self.conditions.len() {
            self.condition_index = self
                .conditions
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, v)| (v, i as u32))
                .collect();
        }
        if let Some(&id) = self.condition_index.get(&value) {
            return id;
        }
        let id = u32::try_from(self.conditions.len()).expect("voxel condition identity space");
        self.conditions.push(value.clone());
        self.condition_index.insert(value, id);
        id
    }
    fn reference(&mut self, value: Reference) -> u32 {
        if self.reference_index.len() != self.references.len() {
            self.reference_index = self
                .references
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, v)| (v, i as u32))
                .collect();
        }
        if let Some(&id) = self.reference_index.get(&value) {
            return id;
        }
        let id = u32::try_from(self.references.len()).expect("voxel reference identity space");
        self.references.push(value.clone());
        self.reference_index.insert(value, id);
        id
    }
    fn not(&mut self, a: u32) -> u32 {
        match self.conditions[a as usize] {
            Condition::False => 1,
            Condition::True => 0,
            Condition::Not(x) => x,
            _ => self.condition(Condition::Not(a)),
        }
    }
    fn terms(&self, id: u32, and: bool, out: &mut BTreeSet<u32>) {
        match self.conditions[id as usize] {
            Condition::And(a, b) if and => {
                self.terms(a, and, out);
                self.terms(b, and, out);
            }
            Condition::Or(a, b) if !and => {
                self.terms(a, and, out);
                self.terms(b, and, out);
            }
            _ => {
                out.insert(id);
            }
        }
    }
    fn combine(&mut self, a: u32, b: u32, and: bool) -> u32 {
        let mut terms = BTreeSet::new();
        self.terms(a, and, &mut terms);
        self.terms(b, and, &mut terms);
        let annihilator = if and { 0 } else { 1 };
        let identity = 1 - annihilator;
        if terms.contains(&annihilator) {
            return annihilator;
        }
        terms.remove(&identity);
        if terms.iter().any(|id|matches!(self.conditions[*id as usize],Condition::Not(other) if terms.contains(&other))){return annihilator;}
        // Absorption avoids retaining repeated guarded retirements forever.
        let redundant: Vec<_> = terms
            .iter()
            .copied()
            .filter(|id| {
                let mut inner = BTreeSet::new();
                self.terms(*id, !and, &mut inner);
                inner.len() > 1 && inner.iter().any(|term| terms.contains(term))
            })
            .collect();
        for id in redundant {
            terms.remove(&id);
        }
        let mut result = identity;
        for term in terms {
            result = if result == identity {
                term
            } else {
                self.condition(if and {
                    Condition::And(result, term)
                } else {
                    Condition::Or(result, term)
                })
            };
        }
        result
    }
    fn and(&mut self, a: u32, b: u32) -> u32 {
        self.combine(a, b, true)
    }
    fn or(&mut self, a: u32, b: u32) -> u32 {
        self.combine(a, b, false)
    }
    fn select(&mut self, condition: u32, yes: u32, no: u32) -> u32 {
        if condition == 1 || yes == no {
            return yes;
        }
        if condition == 0 {
            return no;
        }
        let yes = self.assuming(yes, condition, true);
        let no = self.assuming(no, condition, false);
        if yes == no {
            yes
        } else {
            self.reference(Reference::Select(condition, yes, no))
        }
    }
    fn assuming(&mut self, reference: u32, condition: u32, value: bool) -> u32 {
        match self.references[reference as usize].clone() {
            Reference::Select(c, y, n) if c == condition => {
                self.assuming(if value { y } else { n }, condition, value)
            }
            Reference::Select(c, y, n) => {
                let y = self.assuming(y, condition, value);
                let n = self.assuming(n, condition, value);
                if y == n {
                    y
                } else {
                    self.reference(Reference::Select(c, y, n))
                }
            }
            _ => reference,
        }
    }
    fn probe(&mut self, kind: LibraryKind, image: &str, valid: bool) -> u32 {
        let request = LibraryRequest {
            kind,
            image: image.to_ascii_uppercase(),
        };
        self.condition(if valid {
            Condition::Valid(request)
        } else {
            Condition::Present(request)
        })
    }
    fn allocate(&mut self, kind: LibraryKind, image: &str) -> u32 {
        let id = u32::try_from(self.allocations.len()).expect("voxel allocation identity space");
        self.allocations.push(Allocation {
            request: LibraryRequest {
                kind,
                image: image.to_ascii_uppercase(),
            },
            retired: 0,
        });
        self.reference(Reference::Allocation(id))
    }
    fn retire(&mut self, reference: u32, guard: u32) {
        if guard == 0 {
            return;
        }
        match self.references[reference as usize].clone() {
            Reference::Null => {}
            Reference::Allocation(id) => {
                let retired = self.allocations[id as usize].retired;
                self.allocations[id as usize].retired = self.or(retired, guard);
            }
            Reference::Select(c, yes, no) => {
                let yes_guard = self.and(guard, c);
                let nc = self.not(c);
                let no_guard = self.and(guard, nc);
                self.retire(yes, yes_guard);
                self.retire(no, no_guard);
            }
        }
    }
    fn clear(&mut self, pair: &mut ModelPair, guard: u32) {
        self.retire(pair.vxl, guard);
        self.retire(pair.hva, guard);
        pair.vxl = self.select(guard, 0, pair.vxl);
        pair.hva = self.select(guard, 0, pair.hva);
    }
    fn replace_pair(
        &mut self,
        pair: &mut ModelPair,
        image: &str,
        guard: u32,
        free_old: bool,
    ) -> u32 {
        if free_old {
            self.retire(pair.vxl, guard);
            self.retire(pair.hva, guard);
        }
        let vxl = self.allocate(LibraryKind::Vxl, image);
        let hva = self.allocate(LibraryKind::Hva, image);
        pair.vxl = self.select(guard, vxl, pair.vxl);
        pair.hva = self.select(guard, hva, pair.hva);
        let vxl_valid = self.probe(LibraryKind::Vxl, image, true);
        let hva_valid = self.probe(LibraryKind::Hva, image, true);
        self.and(vxl_valid, hva_valid)
    }
    fn optional_pair(
        &mut self,
        pair: &mut ModelPair,
        image: &str,
        guard: u32,
        free_old: bool,
    ) -> u32 {
        let present = self.probe(LibraryKind::Vxl, image, false);
        let attempted = self.and(guard, present);
        let valid = self.replace_pair(pair, image, attempted, free_old);
        let invalid = self.not(valid);
        self.and(attempted, invalid)
    }

    /// ObjectType5F8110, including the no-Turret prefix before indexed parts.
    pub(crate) fn load_generic(&mut self, slots: &mut ModelSlots, input: &GenericLoadInput) {
        //5F8169 and the common failure cleanup cover both presence arms:
        // every complete generic call retires its previous main libraries.
        self.retire(slots.main.vxl, 1);
        self.retire(slots.main.hva, 1);
        // No old main survives either native outcome: a present file replaces
        // it at5F8169, and an absent/invalid file reaches the failure cleanup.
        // Clear it before the conditional load so repeated reads cannot retain
        // an impossible absent-file fallback (or its retirement predicates).
        slots.main = ModelPair::default();
        let present = self.probe(LibraryKind::Vxl, &input.image, false);
        let valid = self.replace_pair(&mut slots.main, &input.image, present, true);
        let success = self.and(present, valid);
        let mut failed = self.not(success);
        let indexed = input.unit && input.turret_count > 0 && !input.gattling;
        // Ares ObjectTypeClass_Load3DArt_NoSpawnAlt1 (10041280, hook
        // 0x5F8277, resuming at the stock Turret test 0x5F8287): a unit
        // type's NoSpawnAlt loads <Image>WO into TechnoTypeExt's own slot
        // whatever its Turret=, and a missing or unreadable pair fails the
        // whole load (LoadVoxel 1003F4E0 with missingOk false). The unit draw
        // reads that pair by name (`unit_atlas_variants`), so only the
        // outcome joins the failure flag here.
        #[cfg(feature = "ares")]
        if input.unit && input.no_spawn_alt {
            let image = format!("{}WO", input.image);
            let present = self.probe(LibraryKind::Vxl, &image, false);
            let vxl_valid = self.probe(LibraryKind::Vxl, &image, true);
            let hva_valid = self.probe(LibraryKind::Hva, &image, true);
            let valid = self.and(vxl_valid, hva_valid);
            let loaded = self.and(present, valid);
            let bad = self.not(loaded);
            failed = self.or(failed, bad);
        }
        if input.unit && !input.turret {
            //5F82C4: APC is an exact stored-ID comparison; W precedes WO.
            if input.apc {
                let e =
                    self.optional_pair(&mut slots.auxiliary, &format!("{}W", input.image), 1, true);
                failed = self.or(failed, e);
            }
            // Ares ObjectTypeClass_Load3DArt_NoSpawnAlt2 (100413A0, hook
            // 0x5F848C) skips this stock WO load to the barrel tests at
            // 0x5F8844; the pair is loaded above instead.
            if input.no_spawn_alt && !cfg!(feature = "ares") {
                let e = self.optional_pair(
                    &mut slots.auxiliary,
                    &format!("{}WO", input.image),
                    1,
                    true,
                );
                failed = self.or(failed, e);
            }
        } else if indexed {
            // Original arrays hold18 entries; larger counts address other native
            // storage. Omit unsafe alias writes under the existing indexed policy.
            let count = usize::try_from(input.turret_count).unwrap_or(0).min(18);
            slots
                .indexed
                .resize_with(slots.indexed.len().max(count), ModelGunSlots::default);
            let mut running = 1;
            for i in 0..count {
                let name = crate::rules::voxel_frame_catalog::voxel_gun_basename(
                    &input.image,
                    crate::rules::voxel_frame_catalog::VxlLayer::Turret,
                    i as i32,
                );
                let present = self.probe(LibraryKind::Vxl, &name, false);
                let attempted = self.and(running, present);
                let valid = self.replace_pair(&mut slots.indexed[i].turret, &name, attempted, true);
                let success = self.and(present, valid);
                let bad = self.not(success);
                let error = self.and(running, bad);
                failed = self.or(failed, error);
                running = self.and(running, success);
            }
            let mut running = 1;
            for i in 0..count {
                let name = crate::rules::voxel_frame_catalog::voxel_gun_basename(
                    &input.image,
                    crate::rules::voxel_frame_catalog::VxlLayer::Barrel,
                    i as i32,
                );
                let error = self.optional_pair(&mut slots.indexed[i].barrel, &name, running, false);
                failed = self.or(failed, error);
                let okay = self.not(error);
                running = self.and(running, okay);
            }
        } else {
            let e = self.optional_pair(
                &mut slots.auxiliary,
                &format!("{}TUR", input.image),
                1,
                true,
            );
            failed = self.or(failed, e);
            let e = self.optional_pair(&mut slots.barrel, &format!("{}BARL", input.image), 1, true);
            failed = self.or(failed, e);
        }
        self.clear(&mut slots.main, failed);
        if indexed {
            for gun in &mut slots.indexed {
                self.clear(&mut gun.turret, failed);
                self.clear(&mut gun.barrel, failed);
            }
        } else {
            self.clear(&mut slots.auxiliary, failed);
        }
    }

    /// Building45FA90: an absent VXL preserves both previous library pointers.
    pub(crate) fn load_building(
        &mut self,
        slots: &mut ModelSlots,
        turret: Option<&str>,
        barrel: Option<&str>,
    ) {
        if let Some(image) = turret {
            self.optional_pair(&mut slots.auxiliary, image, 1, true);
        }
        if let Some(image) = barrel {
            self.optional_pair(&mut slots.barrel, image, 1, true);
        }
    }
    /// VoxelAnim74B59D..5DB replaces only B0, without a presence test or HVA.
    pub(crate) fn load_debris(&mut self, slots: &mut ModelSlots, image: &str) {
        self.retire(slots.main.vxl, 1);
        slots.main.vxl = self.allocate(LibraryKind::Vxl, image);
    }
    /// VoxelAnim74B4D0..55B copies the selected pair with no fallback/free.
    pub(crate) fn copy_debris(&mut self, slots: &mut ModelSlots, source: Option<ModelPair>) {
        slots.main = source.unwrap_or_default();
    }

    pub(crate) fn validate(&self) -> bool {
        if self.conditions.first() != Some(&Condition::False)
            || self.conditions.get(1) != Some(&Condition::True)
            || self.references.first() != Some(&Reference::Null)
        {
            return false;
        }
        let Ok(conditions) = u32::try_from(self.conditions.len()) else {
            return false;
        };
        if u32::try_from(self.references.len()).is_err()
            || u32::try_from(self.allocations.len()).is_err()
        {
            return false;
        }
        if self
            .requests()
            .any(|request| request.image.contains('\0') || request.image.len() > 512)
        {
            return false;
        }
        self.conditions.iter().enumerate().all(|(i, c)| match c {
            Condition::Not(a) => *a < i as u32,
            Condition::And(a, b) | Condition::Or(a, b) => *a < i as u32 && *b < i as u32,
            _ => true,
        }) && self.references.iter().enumerate().all(|(i, r)| match r {
            Reference::Null => true,
            Reference::Allocation(a) => (*a as usize) < self.allocations.len(),
            Reference::Select(c, y, n) => *c < conditions && *y < i as u32 && *n < i as u32,
        }) && self.allocations.iter().all(|a| a.retired < conditions)
    }

    pub(crate) fn valid_slots(&self, slots: &ModelSlots) -> bool {
        fn valid(plan: &VoxelModelPlan, id: u32, kind: LibraryKind) -> bool {
            match plan.references.get(id as usize) {
                Some(Reference::Null) => true,
                Some(Reference::Allocation(a)) => plan
                    .allocations
                    .get(*a as usize)
                    .is_some_and(|a| a.request.kind == kind),
                Some(Reference::Select(_, y, n)) => valid(plan, *y, kind) && valid(plan, *n, kind),
                None => false,
            }
        }
        slots.indexed.len() <= 18
            && [&slots.main, &slots.auxiliary, &slots.barrel]
                .into_iter()
                .chain(
                    slots
                        .indexed
                        .iter()
                        .flat_map(|gun| [&gun.turret, &gun.barrel]),
                )
                .all(|pair| {
                    valid(self, pair.vxl, LibraryKind::Vxl)
                        && valid(self, pair.hva, LibraryKind::Hva)
                })
    }

    /// Retain only current slot alternatives and their retirement predicates.
    pub(crate) fn compact<'a>(&mut self, slots: impl Iterator<Item = &'a mut ModelSlots>) {
        let old = std::mem::take(self);
        let mut copies = PlanCopies::default();
        for slots in slots {
            for reference in slots.references_mut() {
                *reference = old.copy_reference(*reference, self, &mut copies);
            }
        }
    }
    fn copy_condition(&self, id: u32, new: &mut Self, copies: &mut PlanCopies) -> u32 {
        if let Some(&out) = copies.conditions.get(&id) {
            return out;
        }
        let out = match &self.conditions[id as usize] {
            Condition::False => 0,
            Condition::True => 1,
            Condition::Not(a) => {
                let a = self.copy_condition(*a, new, copies);
                new.not(a)
            }
            Condition::And(a, b) => {
                let a = self.copy_condition(*a, new, copies);
                let b = self.copy_condition(*b, new, copies);
                new.and(a, b)
            }
            Condition::Or(a, b) => {
                let a = self.copy_condition(*a, new, copies);
                let b = self.copy_condition(*b, new, copies);
                new.or(a, b)
            }
            value => new.condition(value.clone()),
        };
        copies.conditions.insert(id, out);
        out
    }
    fn copy_reference(&self, id: u32, new: &mut Self, copies: &mut PlanCopies) -> u32 {
        if let Some(&out) = copies.references.get(&id) {
            return out;
        }
        let out = match self.references[id as usize] {
            Reference::Null => 0,
            Reference::Allocation(id) => {
                if self.allocations[id as usize].retired == 1 {
                    0
                } else {
                    let allocation = if let Some(&mapped) = copies.allocations.get(&id) {
                        mapped
                    } else {
                        let a = &self.allocations[id as usize];
                        let retired = self.copy_condition(a.retired, new, copies);
                        let mapped = u32::try_from(new.allocations.len())
                            .expect("voxel allocation identity space");
                        new.allocations.push(Allocation {
                            request: a.request.clone(),
                            retired,
                        });
                        copies.allocations.insert(id, mapped);
                        mapped
                    };
                    new.reference(Reference::Allocation(allocation))
                }
            }
            Reference::Select(c, y, n) => {
                let c = self.copy_condition(c, new, copies);
                let y = self.copy_reference(y, new, copies);
                let n = self.copy_reference(n, new, copies);
                new.select(c, y, n)
            }
        };
        copies.references.insert(id, out);
        out
    }
    #[cfg(test)]
    pub(crate) fn node_count(&self) -> usize {
        self.conditions.len() + self.references.len() + self.allocations.len()
    }

    pub(crate) fn requests(&self) -> impl Iterator<Item = &LibraryRequest> {
        self.allocations
            .iter()
            .map(|allocation| &allocation.request)
            .chain(
                self.conditions
                    .iter()
                    .filter_map(|condition| match condition {
                        Condition::Present(request) | Condition::Valid(request) => Some(request),
                        _ => None,
                    }),
            )
    }

    pub(crate) fn resolve<'a>(
        &'a self,
        mut probe: impl FnMut(&LibraryRequest, bool) -> bool,
    ) -> ResolvedPlan<'a> {
        let mut conditions: Vec<bool> = Vec::with_capacity(self.conditions.len());
        for c in &self.conditions {
            let value = match c {
                Condition::False => false,
                Condition::True => true,
                Condition::Present(r) => probe(r, false),
                Condition::Valid(r) => probe(r, true),
                Condition::Not(a) => !conditions[*a as usize],
                Condition::And(a, b) => conditions[*a as usize] && conditions[*b as usize],
                Condition::Or(a, b) => conditions[*a as usize] || conditions[*b as usize],
            };
            conditions.push(value);
        }
        ResolvedPlan {
            plan: self,
            conditions,
        }
    }
}
#[derive(Default)]
struct PlanCopies {
    conditions: HashMap<u32, u32>,
    references: HashMap<u32, u32>,
    allocations: HashMap<u32, u32>,
}
pub(crate) struct ResolvedPlan<'a> {
    plan: &'a VoxelModelPlan,
    conditions: Vec<bool>,
}
impl ResolvedPlan<'_> {
    pub(crate) fn library(&self, mut reference: u32) -> Option<&LibraryRequest> {
        loop {
            match self.plan.references[reference as usize] {
                Reference::Null => return None,
                Reference::Allocation(id) => {
                    let allocation = &self.plan.allocations[id as usize];
                    return (!self.conditions[allocation.retired as usize])
                        .then_some(&allocation.request);
                }
                Reference::Select(c, y, n) => {
                    reference = if self.conditions[c as usize] { y } else { n }
                }
            }
        }
    }
}
