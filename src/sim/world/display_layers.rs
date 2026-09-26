//! DisplayClass's five persistent LayerClass vectors (8A0360, stride 24).
//!
//! Display membership is independent of Logic membership and cell occupation.
//! Crate effects read Ground directly, so rebuilding or fully sorting these
//! lists in presentation changes simulation state. Only the ordered vectors
//! are serialized/hashed; the ID-to-layer index is derived and never iterated
//! to decide gameplay. The Ground members' kept sort keys (`ground_keys`) are
//! derived too. Native comparisons: crate_ground_membership.json.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::ground_keys::{GroundKeys, GroundSortKeys, ReadContext};
use crate::sim::touch_log::Touched;

/// Native layer index; -1 (no display) is represented by `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DisplayLayer(u8);

impl DisplayLayer {
    pub(crate) const SURFACE: Self = Self(1);
    pub(crate) const GROUND: Self = Self(2);
    pub(crate) const AIR: Self = Self(3);
    pub(crate) const TOP: Self = Self(4);

    pub(crate) fn from_index(index: u8) -> Option<Self> {
        (index < 5).then_some(Self(index))
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct DisplayLayers {
    layers: [Vec<u64>; 5],
    /// Rebuilt from the vectors on deserialize; updated only by submit/remove.
    registered: HashMap<u64, DisplayLayer>,
    /// What the Ground members' key reads kept, in lockstep with Ground.
    ground_keys: GroundKeys,
}

impl DisplayLayers {
    pub(crate) fn members(&self, layer: DisplayLayer) -> &[u64] {
        &self.layers[usize::from(layer.0)]
    }

    pub(crate) fn layer_of(&self, id: u64) -> Option<DisplayLayer> {
        self.registered.get(&id).copied()
    }

    pub(crate) fn ordered_ids(&self) -> impl Iterator<Item = &u64> {
        self.layers.iter().flatten()
    }

    /// Display4A9720 removes any old registration before querying/inserting the
    /// selected layer. Ground551A90 inserts before the first strictly greater
    /// key; equal keys stay ahead of a newly submitted object. Other layers
    /// append (5519B0). Allocation failure leaves the object unregistered.
    pub(crate) fn submit(
        &mut self,
        id: u64,
        layer: Option<DisplayLayer>,
        keys: &impl GroundSortKeys,
    ) -> bool {
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
        let index = if layer == DisplayLayer::GROUND {
            // Compare5F6220 reads new, then existing. GetYSort is pure, so
            // the new object's key is read once for the whole scan.
            let (new_key, kept) = keys.read(id);
            let index = (0..members.len())
                .find(|&index| self.ground_keys.key(index, members[index], keys) > new_key)
                .unwrap_or(members.len());
            self.ground_keys.insert(index, id, new_key, kept);
            index
        } else {
            members.len()
        };
        members.insert(index, id);
        self.registered.insert(id, layer);
        debug_assert_eq!(
            self.ground_keys.len(),
            self.members(DisplayLayer::GROUND).len()
        );
        true
    }

    /// Display4A9770: first-match stable erase in the cached layer, then scan
    /// all layers if that lookup fails. The normal owner cannot produce a
    /// stale cache; the fallback also has an original-executable witness.
    pub(crate) fn remove(&mut self, id: u64) -> bool {
        let Some(layer) = self.registered.remove(&id) else {
            return false;
        };
        let cached = usize::from(layer.0);
        let ground = usize::from(DisplayLayer::GROUND.0);
        if let Some(index) = self.layers[cached].iter().position(|&member| member == id) {
            self.layers[cached].remove(index);
            if cached == ground {
                self.ground_keys.remove(index, id);
            }
        } else {
            for (layer, members) in self.layers.iter_mut().enumerate() {
                if let Some(index) = members.iter().position(|&member| member == id) {
                    members.remove(index);
                    if layer == ground {
                        self.ground_keys.remove(index, id);
                    }
                }
            }
        }
        true
    }

    /// Forget the kept Ground key reads whose inputs may have changed (see
    /// `GroundKeys::forget_changed`).
    pub(crate) fn forget_changed_ground_keys(
        &mut self,
        context: ReadContext,
        touched: impl IntoIterator<Item = Touched>,
    ) {
        self.ground_keys.forget_changed(context, touched);
    }

    /// Layer551A30, called at MainTick55DBC8 before Logic55DC9E: one
    /// left-to-right adjacent pass, not a stable full sort.
    pub(crate) fn sort_ground_pass(&mut self, keys: &impl GroundSortKeys) {
        let members = &mut self.layers[usize::from(DisplayLayer::GROUND.0)];
        let Some(&first) = members.first() else {
            return;
        };
        // GetYSort is pure: after a swap the carried member stays on the left
        // of the next comparison, so its key is reused rather than reread.
        let mut left_key = self.ground_keys.key(0, first, keys);
        for right in 1..members.len() {
            let right_key = self.ground_keys.key(right, members[right], keys);
            if right_key < left_key {
                members.swap(right - 1, right);
                self.ground_keys.swap(right - 1, right);
            } else {
                left_key = right_key;
            }
        }
    }

    /// Whether the kept Ground keys move in lockstep with the Ground members.
    #[cfg(test)]
    pub(crate) fn ground_keys_aligned(&self) -> bool {
        self.ground_keys
            .aligned_with(&self.layers[usize::from(DisplayLayer::GROUND.0)])
    }

    pub(crate) fn fold_hash(&self, hasher: &mut impl Hasher) {
        self.layers.hash(hasher);
    }

    /// Bounded pre184 replay projection: animations were not yet registered.
    /// This cannot reconstruct an old Ground order changed by intervening anims.
    #[cfg(test)]
    pub(crate) fn fold_hash_excluding(
        &self,
        hasher: &mut impl Hasher,
        exclude: impl Fn(u64) -> bool,
    ) {
        let layers: [Vec<u64>; 5] = std::array::from_fn(|layer| {
            self.layers[layer]
                .iter()
                .copied()
                .filter(|id| !exclude(*id))
                .collect()
        });
        layers.hash(hasher);
    }
}

impl Serialize for DisplayLayers {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.layers.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DisplayLayers {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let layers = <[Vec<u64>; 5]>::deserialize(deserializer)?;
        let mut registered = HashMap::new();
        for (layer, members) in layers.iter().enumerate() {
            for &id in members {
                if id == 0 || registered.insert(id, DisplayLayer(layer as u8)).is_some() {
                    return Err(serde::de::Error::custom(
                        "null or duplicate display identity",
                    ));
                }
            }
        }
        let ground_keys = GroundKeys::unknown(&layers[usize::from(DisplayLayer::GROUND.0)]);
        Ok(Self {
            layers,
            registered,
            ground_keys,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn original_registration_removal_and_adjacent_sort_sequences() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/crate_ground_membership.json"
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
                let key = |id: u64| {
                    let xyz = coords[id as usize - 1];
                    xyz[0].wrapping_add(xyz[1])
                };
                match step["op"].as_str().unwrap() {
                    "submit" => {
                        display.submit(id, Some(DisplayLayer::GROUND), &key);
                    }
                    "remove" => {
                        display.remove(id);
                    }
                    "sort" => display.sort_ground_pass(&key),
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
                let layers: Vec<Vec<u64>> = display
                    .layers
                    .iter()
                    .map(|members| members.iter().map(|id| id - 1).collect())
                    .collect();
                let registered: Vec<i32> = (1..=coords.len() as u64)
                    .map(|id| display.layer_of(id).map_or(-1, |layer| i32::from(layer.0)))
                    .collect();
                assert_eq!(
                    serde_json::json!(layers),
                    observed["layers"],
                    "{name}: {step}"
                );
                assert_eq!(
                    serde_json::json!(registered),
                    observed["registered"],
                    "{name}: {step}"
                );
                assert!(display.ground_keys_aligned(), "{name}: {step}");
            }
        }
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
            assert!(display.submit(id, Some(layer), &|_| 0));
        }
        let bytes = bincode::serialize(&display).unwrap();
        let mut restored: DisplayLayers = bincode::deserialize(&bytes).unwrap();
        assert_eq!(
            restored.ordered_ids().copied().collect::<Vec<_>>(),
            [3, 1, 4, 2]
        );
        assert!(restored.remove(3));
        assert!(restored.submit(2, Some(DisplayLayer::GROUND), &|_| 0));
        assert_eq!(restored.members(DisplayLayer::GROUND), [1, 2]);
        assert!(restored.members(DisplayLayer::TOP).is_empty());
        assert!(!restored.submit(1, None, &|_| panic!("no layer has no sort getter")));
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
