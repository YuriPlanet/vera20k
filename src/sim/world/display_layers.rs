//! DisplayClass's five persistent LayerClass vectors (8A0360, stride 24).
//!
//! Display membership is independent of Logic membership and cell occupation.
//! Crate effects read Ground membership directly. Ground is kept as an
//! unordered set (a sim-scale deviation, see `DisplayLayers::submit`); the
//! picture sorts it. Only the vectors are serialized/hashed, without the local
//! anims only this game draws (a LAN or online MoveFlash); the ID-to-layer and
//! Ground index maps are derived and never iterated to decide gameplay.
//! Native membership comparisons: crate_ground_membership.json.

use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

use crate::util::lookup_hash::LookupMap;

/// Native layer index; -1 (no display) is represented by `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayLayer(u8);

impl DisplayLayer {
    /// Layer 0, below the ground: a Tunnel locomotor travelling between
    /// cells (`TunnelLocomotionClass::In_Which_Layer @ 0x0072A1A0`).
    pub const UNDERGROUND: Self = Self(0);
    pub const SURFACE: Self = Self(1);
    pub const GROUND: Self = Self(2);
    pub const AIR: Self = Self(3);
    pub const TOP: Self = Self(4);

    /// Base Object5F4260 after its virtual IsInAir query. Class-specific
    /// locomotor/height overrides stay with their callers.
    pub(crate) fn for_object(
        in_air: bool,
        height: impl FnOnce() -> i32,
        cruise_height: i32,
    ) -> Self {
        if !in_air {
            return Self::GROUND;
        }
        let height = height();
        if height == 0 {
            Self::GROUND
        } else if height < cruise_height {
            Self::AIR
        } else {
            Self::TOP
        }
    }

    pub fn from_index(index: u8) -> Option<Self> {
        (index < 5).then_some(Self(index))
    }

    /// The native layer number vt+0x78 returns.
    pub fn index(self) -> u8 {
        self.0
    }
}

#[derive(Debug, Default, Clone)]
pub struct DisplayLayers {
    layers: [Vec<u64>; 5],
    /// Rebuilt from the vectors on deserialize; updated only by submit/remove.
    registered: LookupMap<u64, DisplayLayer>,
    /// Ground member -> its index in the Ground vector. Derived like
    /// `registered`, so a Ground removal swaps instead of shifting.
    ground_index: LookupMap<u64, u32>,
    /// The local anims on Ground (`crate::sim::anim_class::is_local_anim_id`),
    /// in submission order. Kept apart so that a swap on their removal never
    /// moves a shared member: every game holds the shared Ground vector in
    /// the same order. Never saved or hashed.
    local_ground: Vec<u64>,
}

impl DisplayLayers {
    /// `layer`'s members. Ground is a set in no particular order (see
    /// [`Self::submit`]) and leaves out its local anims
    /// ([`Self::local_ground`]); the other layers keep their submission order.
    pub fn members(&self, layer: DisplayLayer) -> &[u64] {
        &self.layers[usize::from(layer.0)]
    }

    /// The local anims on Ground, in submission order.
    pub fn local_ground(&self) -> &[u64] {
        &self.local_ground
    }

    /// Each layer's members, Ground followed by its local anims.
    fn walk(&self) -> impl Iterator<Item = (DisplayLayer, &[u64])> {
        let ground = usize::from(DisplayLayer::GROUND.0);
        self.layers
            .iter()
            .enumerate()
            .flat_map(move |(index, members)| {
                let local: &[u64] = if index == ground {
                    &self.local_ground
                } else {
                    &[]
                };
                [members.as_slice(), local].map(|slice| (DisplayLayer(index as u8), slice))
            })
    }

    pub fn layer_of(&self, id: u64) -> Option<DisplayLayer> {
        self.registered.get(&id).copied()
    }

    /// Every member, layer by layer, each layer as [`Self::members`] holds it.
    pub fn ordered_ids(&self) -> impl Iterator<Item = &u64> {
        self.walk().flat_map(|(_, members)| members)
    }

    /// [`Self::ordered_ids`] with each member's layer, for a caller that
    /// would otherwise look every member up in [`Self::layer_of`].
    pub fn members_in_order(&self) -> impl Iterator<Item = (DisplayLayer, u64)> + '_ {
        self.walk()
            .flat_map(|(layer, members)| members.iter().map(move |&id| (layer, id)))
    }

    /// Display4A9720 removes any old registration before querying/inserting the
    /// selected layer. Other layers append (5519B0). Allocation failure leaves
    /// the object unregistered.
    ///
    /// Sim-scale deviation: Ground551A90 inserts before the first member
    /// whose GetYSort key is strictly greater, and Layer551A30 bubbles the
    /// vector one adjacent pass each frame (MainTick55DBC8). Here Ground is an
    /// unordered set: a submit appends and a removal swaps the last member
    /// into the hole. Only the picture orders Ground by key
    /// (`Simulation::ground_draw_order`), on the presentation side; the
    /// simulation reads Ground in this deterministic set order: crate radius
    /// effects, whose per-object writes are independent of each other, the
    /// network checksum, and RefreshTechnoShroud4ADFF0
    /// (`refresh_techno_shroud`), which reveals each member's sight in this
    /// order instead of roughly GetYSort order and, when a reveal removes a
    /// member, skips the member swapped into its place rather than the next
    /// one. Dropping the ordered insert and pass took 6-7% off the main
    /// thread's time in a `--release` sim-bench at 20,000 units and 30
    /// houses.
    pub fn submit(&mut self, id: u64, layer: Option<DisplayLayer>) -> bool {
        if id == 0 {
            return false;
        }
        self.remove(id);
        let Some(layer) = layer else {
            return false;
        };
        let members = &mut self.layers[usize::from(layer.0)];
        if members.try_reserve(1).is_err() || self.registered.try_reserve(1).is_err() {
            return false;
        }
        if layer == DisplayLayer::GROUND && crate::sim::anim_class::is_local_anim_id(id) {
            if self.local_ground.try_reserve(1).is_err() {
                return false;
            }
            self.local_ground.push(id);
        } else {
            if layer == DisplayLayer::GROUND {
                if self.ground_index.try_reserve(1).is_err() {
                    return false;
                }
                self.ground_index.insert(id, members.len() as u32);
            }
            members.push(id);
        }
        self.registered.insert(id, layer);
        true
    }

    /// Display4A9770: erase from the cached layer, then scan all layers if
    /// that lookup fails. The normal owner cannot produce a stale cache; the
    /// fallback also has an original-executable witness. Ground swaps its
    /// last member into the hole ([`Self::submit`]); other layers keep their
    /// order.
    pub fn remove(&mut self, id: u64) -> bool {
        let Some(layer) = self.registered.remove(&id) else {
            return false;
        };
        if !self.erase(usize::from(layer.0), id) {
            for layer in 0..self.layers.len() {
                self.erase(layer, id);
            }
        }
        true
    }

    /// Erase `id` from `layer`'s vector; whether it was there.
    fn erase(&mut self, layer: usize, id: u64) -> bool {
        if layer == usize::from(DisplayLayer::GROUND.0) {
            if crate::sim::anim_class::is_local_anim_id(id) {
                let Some(index) = self.local_ground.iter().position(|&member| member == id) else {
                    return false;
                };
                self.local_ground.remove(index);
                return true;
            }
            let Some(index) = self.ground_index.remove(&id) else {
                return false;
            };
            let members = &mut self.layers[layer];
            members.swap_remove(index as usize);
            if let Some(&moved) = members.get(index as usize) {
                self.ground_index.insert(moved, index);
            }
            return true;
        }
        let members = &mut self.layers[layer];
        let Some(index) = position_of(members, id) else {
            return false;
        };
        members.remove(index);
        true
    }

    /// The layers as the hash and saves see them: without the local anims
    /// (`crate::sim::anim_class::is_local_anim_id`), which only this game
    /// draws. Borrowed as they are when none is a member.
    fn shared_layers(&self) -> std::borrow::Cow<'_, [Vec<u64>; 5]> {
        use crate::sim::anim_class::is_local_anim_id;
        if !self.layers.iter().flatten().any(|&id| is_local_anim_id(id)) {
            return std::borrow::Cow::Borrowed(&self.layers);
        }
        std::borrow::Cow::Owned(self.layers.clone().map(|mut members| {
            members.retain(|&id| !is_local_anim_id(id));
            members
        }))
    }

    pub(crate) fn fold_hash(&self, hasher: &mut impl Hasher) {
        self.shared_layers().hash(hasher);
    }
}

/// The index of `id` in `members`, the first match `iter().position` finds:
/// blocks of 32 are compared at once, then the matching block member by
/// member.
fn position_of(members: &[u64], id: u64) -> Option<usize> {
    let (blocks, tail) = members.as_chunks::<32>();
    for (block_index, block) in blocks.iter().enumerate() {
        if block
            .iter()
            .fold(false, |found, &member| found | (member == id))
        {
            let index = block.iter().position(|&member| member == id)?;
            return Some(block_index * 32 + index);
        }
    }
    let index = tail.iter().position(|&member| member == id)?;
    Some(blocks.len() * 32 + index)
}

impl Serialize for DisplayLayers {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.shared_layers().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DisplayLayers {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let layers = <[Vec<u64>; 5]>::deserialize(deserializer)?;
        if crate::util::save_codec::layout::describing() {
            return Ok(Self::default());
        }
        let mut registered = LookupMap::default();
        for (layer, members) in layers.iter().enumerate() {
            for &id in members {
                if id == 0 || registered.insert(id, DisplayLayer(layer as u8)).is_some() {
                    return Err(serde::de::Error::custom(
                        "null or duplicate display identity",
                    ));
                }
            }
        }
        let ground_index = layers[usize::from(DisplayLayer::GROUND.0)]
            .iter()
            .enumerate()
            .map(|(index, &id)| (id, index as u32))
            .collect();
        Ok(Self {
            layers,
            registered,
            ground_index,
            local_ground: Vec::new(),
        })
    }
}

#[cfg(all(test, tests_sim_world_c_o))]
mod tests {
    use super::*;

    /// The native sequences' memberships: Ground is a set here
    /// ([`DisplayLayers::submit`]), so each layer is compared as one and the
    /// native sort pass changes nothing.
    #[test]
    fn original_registration_and_removal_sequences_keep_native_membership() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/crate_ground_membership.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 6);
        for row in rows {
            let case = &row["input"];
            let name = case["name"].as_str().unwrap();
            let mut coords: Vec<[i32; 3]> = case["actors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|actor| {
                    let delta: [i32; 3] = actor["delta"].as_array().map_or([0; 3], |v| {
                        std::array::from_fn(|i| v[i].as_i64().unwrap() as i32)
                    });
                    [2688 + delta[0], 2688 + delta[1], delta[2]]
                })
                .collect();
            let mut display = DisplayLayers::default();
            for (step, observed) in case["steps"]
                .as_array()
                .unwrap()
                .iter()
                .zip(row["after_steps"].as_array().unwrap())
            {
                let id = step["actor"].as_u64().map_or(0, |index| index + 1);
                match step["op"].as_str().unwrap() {
                    "submit" => {
                        display.submit(id, Some(DisplayLayer::GROUND));
                    }
                    "remove" => {
                        display.remove(id);
                    }
                    "sort" => {}
                    "coordinates" => {
                        coords[id as usize - 1] =
                            std::array::from_fn(|i| step["xyz"][i].as_i64().unwrap() as i32);
                    }
                    "cached_layer" => {
                        display.registered.insert(
                            id,
                            DisplayLayer::from_index(step["layer"].as_u64().unwrap() as u8)
                                .unwrap(),
                        );
                    }
                    op => panic!("unexpected {op}"),
                }
                let sorted = |members: Vec<u64>| {
                    let mut members = members;
                    members.sort_unstable();
                    members
                };
                let layers: Vec<Vec<u64>> = display
                    .layers
                    .iter()
                    .map(|members| sorted(members.iter().map(|id| id - 1).collect()))
                    .collect();
                let native: Vec<Vec<u64>> = observed["layers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|members| {
                        sorted(
                            members
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|id| id.as_u64().unwrap())
                                .collect(),
                        )
                    })
                    .collect();
                let registered: Vec<i32> = (1..=coords.len() as u64)
                    .map(|id| display.layer_of(id).map_or(-1, |layer| i32::from(layer.0)))
                    .collect();
                assert_eq!(layers, native, "{name}: {step}");
                assert_eq!(
                    serde_json::json!(registered),
                    observed["registered"],
                    "{name}: {step}"
                );
            }
        }
    }

    /// Air removal compares 32 members at a time: members at the blocks'
    /// edges and in the tail leave, and the rest keep their order.
    #[test]
    fn remove_finds_members_at_block_edges() {
        let mut display = DisplayLayers::default();
        for id in 1..=70 {
            assert!(display.submit(id, Some(DisplayLayer::AIR)));
        }
        // From the back, so each still sits at index id - 1: the tail's last
        // and first members, then the last and first of each block.
        let leaving = [70, 65, 64, 33, 32, 1];
        for id in leaving {
            assert!(display.remove(id));
        }
        let staying: Vec<u64> = (1..=70).filter(|id| !leaving.contains(id)).collect();
        assert_eq!(display.members(DisplayLayer::AIR), staying);
    }

    /// Ground removals swap, so the shared members' order depends on every
    /// removal before it: a local anim (one only this game draws) must not
    /// be among them, or two games would hold, hash and checksum their
    /// shared members in different orders.
    #[test]
    fn local_ground_anims_never_move_shared_members() {
        let local = 1 << 56;
        assert!(crate::sim::anim_class::is_local_anim_id(local));
        let mut here = DisplayLayers::default();
        let mut there = DisplayLayers::default();
        for id in [1, local, 2, 3] {
            here.submit(id, Some(DisplayLayer::GROUND));
        }
        for id in [1, 2, 3] {
            there.submit(id, Some(DisplayLayer::GROUND));
        }
        assert_eq!(here.local_ground(), [local]);
        here.remove(local);
        assert_eq!(here.members(DisplayLayer::GROUND), [1, 2, 3]);
        assert_eq!(
            here.members(DisplayLayer::GROUND),
            there.members(DisplayLayer::GROUND)
        );
    }

    #[test]
    fn restore_preserves_order_and_rebuilds_only_the_lookup() {
        let mut display = DisplayLayers::default();
        for (id, layer) in [
            (3, DisplayLayer::GROUND),
            (1, DisplayLayer::GROUND),
            (4, DisplayLayer::AIR),
            (2, DisplayLayer::TOP),
        ] {
            assert!(display.submit(id, Some(layer)));
        }
        let bytes = crate::util::save_codec::serialize(&display).unwrap();
        let mut restored: DisplayLayers = crate::util::save_codec::deserialize(&bytes).unwrap();
        assert_eq!(
            restored.ordered_ids().copied().collect::<Vec<_>>(),
            [3, 1, 4, 2]
        );
        assert!(restored.remove(3));
        assert!(restored.submit(2, Some(DisplayLayer::GROUND)));
        assert_eq!(restored.members(DisplayLayer::GROUND), [1, 2]);
        assert!(restored.members(DisplayLayer::TOP).is_empty());
        assert!(!restored.submit(1, None));
        assert_eq!(restored.members(DisplayLayer::GROUND), [2]);
        for malformed in [
            "[[],[],[1,1],[],[]]",
            "[[],[],[1],[],[1]]",
            "[[],[],[0],[],[]]",
        ] {
            assert!(serde_json::from_str::<DisplayLayers>(malformed).is_err());
        }
    }
}
