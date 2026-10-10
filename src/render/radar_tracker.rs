//! Retained live-radar object tracker.
//!
//! This is presentation state, deliberately not simulation state. Active YR's
//! registered byte (`TechnoClass+0x423`) and discovery/visibility decisions are
//! local-client facts: they read `g_PlayerPtr` and that client's shroud. The
//! tracker is also recreated by `FUN_00655990` and on load. Serializing or
//! world-hashing it would therefore make a local view cache part of lockstep.

use std::collections::{BTreeMap, BTreeSet};

use crate::map::houses::HouseColorMap;
use crate::rules::house_colors::HouseColorRamps;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::vision::FogState;

use super::minimap_helpers::owner_dot_color;
use super::native_radar_surface::NativeRadarSurfaceGeometry;
use super::radar_visibility::{RadarRegistrationVisibilityFacts, radar_owner_is_human_player};

const TRACKER_BUCKET_COUNT: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RadarTrackerEntry {
    pub stable_id: u64,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RadarProjectionFacts {
    pub native_surface: Option<NativeRadarSurfaceGeometry>,
    pub world_origin_x: f32,
    pub world_origin_y: f32,
    pub world_width: f32,
    pub world_height: f32,
    pub map_offset_x: f32,
    pub map_offset_y: f32,
    pub map_pixel_w: f32,
    pub map_pixel_h: f32,
}

impl RadarProjectionFacts {
    pub fn cell_axis_scale(self) -> f32 {
        if let Some(surface) = self.native_surface {
            return surface.zoom();
        }
        // Mapless/headless adapter retained for tests without MapClass data.
        let x_scale =
            self.map_pixel_w / self.world_width.max(1.0) * (crate::map::terrain::TILE_WIDTH * 0.5);
        let y_scale = self.map_pixel_h / self.world_height.max(1.0)
            * (crate::map::terrain::TILE_HEIGHT * 0.5);
        x_scale.abs().min(y_scale.abs()).max(f32::EPSILON)
    }

    fn pixel_to_cell(self, x: i32, y: i32) -> Option<(u16, u16)> {
        if let Some(surface) = self.native_surface {
            return surface.surface_pixel_to_cell((x, y));
        }
        let normalized_x = (x as f32 - self.map_offset_x) / self.map_pixel_w.max(1.0);
        let normalized_y = (y as f32 - self.map_offset_y) / self.map_pixel_h.max(1.0);
        let screen_x = self.world_origin_x + normalized_x * self.world_width;
        let screen_y = self.world_origin_y + normalized_y * self.world_height;
        // Inverse of iso_to_screen: sx=(rx-ry)*TILE_WIDTH/2 and
        // sy=(rx+ry)*TILE_HEIGHT/2.
        let rx = screen_x / crate::map::terrain::TILE_WIDTH
            + screen_y / crate::map::terrain::TILE_HEIGHT;
        let ry = screen_y / crate::map::terrain::TILE_HEIGHT
            - screen_x / crate::map::terrain::TILE_WIDTH;
        let rx = rx.round() as i32;
        let ry = ry.round() as i32;
        (rx >= 0 && ry >= 0 && rx <= i32::from(u16::MAX) && ry <= i32::from(u16::MAX))
            .then_some((rx as u16, ry as u16))
    }
}

pub(super) fn radar_entity_owner_color(
    entity: &crate::sim::game_entity::GameEntity,
    interner: Option<&crate::sim::intern::StringInterner>,
    house_colors: &HouseColorMap,
    ramps: &HouseColorRamps,
    observer: Option<InternedId>,
    owner_allied_with_observer: bool,
) -> [u8; 4] {
    use crate::map::entities::EntityCategory;
    // Radar655F48 asks the class's +D0(0) only when +C4 says disguised.
    // Unit7465F0 and Infantry5226C0 preserve the owner's directional alliance
    // with the current viewer; selection and disguise sensors are not inputs.
    let color_owner = match entity.disguise.as_ref().filter(|d| d.is_disguised()) {
        None => Some(entity.owner()),
        Some(_)
            if owner_allied_with_observer
                && matches!(
                    entity.category,
                    EntityCategory::Unit | EntityCategory::Infantry
                ) =>
        {
            Some(entity.owner())
        }
        Some(disguise) => match entity.category {
            EntityCategory::Unit => disguise.house(),
            // The Spy leaf deliberately differs: NULL disguiseHouse returns
            // CurrentHouse, whereas a Mirage preserves NULL.
            EntityCategory::Infantry => disguise.house().or(observer),
            // Building/Aircraft retain Techno41BE70: XOR EAX,EAX; RET4.
            EntityCategory::Structure | EntityCategory::Aircraft => None,
        },
    };
    let Some(color_owner) = color_owner else {
        // 655FD6..655FE5 finds literal LightGrey but copies ColorScheme+330
        // directly. Constructor68C769..68C7DC writes palette index16 there;
        // the admitted pixel suffix stores WORD0010, not that scheme's RGB.
        // Executed source/pixel controls: spatial_oracle/mirage_disguise.json.
        let rgb = super::native_surface_format::RGB565.unpack_rgb8(16);
        return super::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
            .quantize_rgba8([rgb[0], rgb[1], rgb[2], 255]);
    };
    let owner_str = interner.map_or("", |interner| interner.resolve(color_owner));
    // Building and mobile tracker entries share RenderCellPixel's owner/remap
    // path. Khaki is terrain-only.
    owner_dot_color(owner_str, house_colors, ramps)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn radar_pixel_candidate_eligible(
    entry: RadarTrackerEntry,
    entities: &crate::sim::entity_store::EntityStore,
    houses: &BTreeMap<InternedId, crate::sim::house_state::HouseState>,
    local_owner: Option<InternedId>,
    fog: &FogState,
    full_visibility: bool,
    game_mode_nonzero: bool,
    rules: Option<&RuleSet>,
    interner: Option<&crate::sim::intern::StringInterner>,
    projection: RadarProjectionFacts,
) -> bool {
    let Some(entity) = entities.get(entry.stable_id) else {
        return false;
    };
    let type_str = interner.map_or("", |i| i.resolve(entity.type_ref()));
    let object = rules.and_then(|rules| rules.object(type_str));

    if let Some(local_owner) = local_owner {
        let friendly = entity.owner() == local_owner
            || interner
                .is_some_and(|interner| fog.is_friendly_id(local_owner, entity.owner(), interner));
        if !full_visibility {
            let (rx, ry) = projection
                .pixel_to_cell(entry.x, entry.y)
                .unwrap_or((entity.position.rx, entity.position.ry));
            let owner_is_human =
                radar_owner_is_human_player(entity.owner(), local_owner, houses, game_mode_nonzero);
            if !fog.is_cell_revealed(local_owner, rx, ry) && !owner_is_human {
                return false;
            }
        }
        // RenderCellPixel @ 0x00655DFF tests RadarInvisible before the later
        // Insignificant/RadarVisible branch. RadarVisible cannot rescue a
        // hostile RadarInvisible object.
        if object.is_some_and(|object| object.radar_invisible) && !friendly {
            return false;
        }
    }

    if object.is_some_and(|object| object.insignificant && !object.radar_visible)
        && !houses
            .get(&entity.owner())
            .is_some_and(|house| !house.multiplay_passive)
    {
        return false;
    }

    // Negative native evidence: this live pixel gate does not read gap cover,
    // current cloak state, Invisible=, or sensor state.
    true
}

#[derive(Debug, Clone)]
struct RadarObjectCache {
    cached_x: i32,
    cached_y: i32,
    registered: bool,
    discovered: bool,
    owner: InternedId,
    local_front: bool,
    pixels: Vec<(i32, i32)>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct RadarObjectUpdate {
    pub stable_id: u64,
    pub owner: InternedId,
    pub origin: (i32, i32),
    pub event_source_cell: Option<(u16, u16)>,
    pub enemy_sensed_prefilter: bool,
    pub foundation: Option<(u32, u32)>,
    pub radar_scale: f32,
    pub discovery_observed: bool,
    pub visibility: RadarRegistrationVisibilityFacts,
    pub local_front: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RadarSensedPresentationEvent {
    pub stable_id: u64,
    pub out_code: u8,
    pub cell: (u16, u16),
}

/// Client-local equivalent of RadarClass+0x1258 plus the per-object cached
/// `+0x208/+0x20C`, registered `+0x423`, and discovery facts needed to update
/// it. Bucket order is observable because `RenderCellPixel @ 0x00655C50`
/// selects the first eligible exact-coordinate entry.
#[derive(Debug, Clone)]
pub(super) struct RetainedRadarTracker {
    buckets: Vec<Vec<RadarTrackerEntry>>,
    objects: BTreeMap<u64, RadarObjectCache>,
    action40_building_tail_pending: bool,
}

impl Default for RetainedRadarTracker {
    fn default() -> Self {
        Self {
            buckets: vec![Vec::new(); TRACKER_BUCKET_COUNT],
            objects: BTreeMap::new(),
            action40_building_tail_pending: false,
        }
    }
}

impl RetainedRadarTracker {
    /// `FUN_00655990` recreates tracker storage and clears every Techno's
    /// `+0x423`, but does not clear discovery. The action's immediate forced
    /// tail repopulates Buildings in reverse BuildingClass array order; mobiles
    /// remain clear until their later ordinary `TechnoClass+0x4A0` visit.
    pub fn reset_for_action40(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        for cache in self.objects.values_mut() {
            cache.registered = false;
            cache.pixels.clear();
        }
        self.action40_building_tail_pending = true;
    }

    /// Load/view replacement reconstructs all local presentation facts from
    /// the restored world and fog, matching radar-storage recreation.
    pub fn reset_for_load_or_view(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
        self.objects.clear();
        self.action40_building_tail_pending = false;
    }

    pub fn take_action40_building_tail_pending(&mut self) -> bool {
        std::mem::take(&mut self.action40_building_tail_pending)
    }

    /// Remove objects which no longer exist or have reached an unregistering
    /// lifecycle state. Objects merely absent from LogicClass are retained:
    /// native radar registration is independent of LogicVector membership.
    pub fn remove_absent_or_ineligible(
        &mut self,
        entities: &crate::sim::entity_store::EntityStore,
    ) {
        let stale: Vec<u64> = self
            .objects
            .keys()
            .copied()
            .filter(|&stable_id| {
                entities.get(stable_id).is_none_or(|entity| {
                    entity.lifecycle.in_limbo || !entity.lifecycle.object_alive
                })
            })
            .collect();
        for stable_id in stale {
            self.unregister(stable_id);
            self.objects.remove(&stable_id);
        }
    }

    /// Presentation-frame equivalent of `TechnoClass+0x4A0 @ 0x0070D990`.
    /// The exact native order is remove OLD, write cached coordinates, then add
    /// NEW. Buildings retain their old cached coordinate on ordinary param=0;
    /// action 40 passes param=1 and therefore forces recomputation.
    pub fn update_object(
        &mut self,
        update: RadarObjectUpdate,
        force_building_coord: bool,
    ) -> Option<RadarSensedPresentationEvent> {
        let existing = self.objects.get(&update.stable_id);
        let old_origin = existing.map(|cache| (cache.cached_x, cache.cached_y));
        let origin = if update.foundation.is_some() && !force_building_coord {
            old_origin.unwrap_or(update.origin)
        } else {
            update.origin
        };
        let was_registered = existing.is_some_and(|cache| cache.registered);
        let was_discovered = existing.is_some_and(|cache| cache.discovered);
        let discovered = was_discovered || update.discovery_observed;
        let visibility = update.visibility.evaluate(discovered);
        let visible = visibility.visible;
        let registration_identity_changed = existing.is_some_and(|cache| {
            cache.owner != update.owner || cache.local_front != update.local_front
        });

        if was_registered
            && (old_origin != Some(origin) || !visible || registration_identity_changed)
        {
            self.unregister(update.stable_id);
        }

        let registered_after_remove = self
            .objects
            .get(&update.stable_id)
            .is_some_and(|cache| cache.registered);
        self.objects
            .entry(update.stable_id)
            .and_modify(|cache| {
                cache.cached_x = origin.0;
                cache.cached_y = origin.1;
                cache.discovered = discovered;
                cache.owner = update.owner;
                cache.local_front = update.local_front;
            })
            .or_insert(RadarObjectCache {
                cached_x: origin.0,
                cached_y: origin.1,
                registered: false,
                discovered,
                owner: update.owner,
                local_front: update.local_front,
                pixels: Vec::new(),
            });

        if visible && !registered_after_remove {
            let offsets = update.foundation.map_or_else(
                || vec![(0, 0)],
                |(width, height)| radar_foundation_brush(width, height, update.radar_scale),
            );
            let pixels: Vec<(i32, i32)> = offsets
                .into_iter()
                .map(|(x, y)| (origin.0.wrapping_add(x), origin.1.wrapping_add(y)))
                .collect();
            self.register(update.stable_id, &pixels, update.local_front);
        }
        if visibility.out_code != 0 && update.enemy_sensed_prefilter {
            update
                .event_source_cell
                .map(|cell| RadarSensedPresentationEvent {
                    stable_id: update.stable_id,
                    out_code: visibility.out_code,
                    cell,
                })
        } else {
            None
        }
    }

    /// AddObjectToTracker @ 0x00655560: reject an exact duplicate, insert the
    /// local player's object at bucket front, append every non-local object.
    fn add(&mut self, entry: RadarTrackerEntry, local_front: bool) -> bool {
        let bucket = &mut self.buckets[tracker_bucket(entry.x, entry.y)];
        if bucket.iter().any(|candidate| *candidate == entry) {
            return false;
        }
        if local_front {
            bucket.insert(0, entry);
        } else {
            bucket.push(entry);
        }
        true
    }

    fn register(&mut self, stable_id: u64, pixels: &[(i32, i32)], local_front: bool) {
        let mut inserted = Vec::with_capacity(pixels.len());
        for &(x, y) in pixels {
            let entry = RadarTrackerEntry { stable_id, x, y };
            if self.add(entry, local_front) {
                inserted.push((x, y));
            }
        }
        if let Some(cache) = self.objects.get_mut(&stable_id) {
            cache.registered = true;
            for pixel in inserted {
                if !cache.pixels.contains(&pixel) {
                    cache.pixels.push(pixel);
                }
            }
        }
    }

    /// RemoveObjectFromTracker @ 0x00655740 shifts the remaining vector over
    /// the removed entry. `Vec::remove` preserves that exact relative order.
    fn unregister(&mut self, stable_id: u64) {
        let pixels = self
            .objects
            .get(&stable_id)
            .map(|cache| cache.pixels.clone())
            .unwrap_or_default();
        for (x, y) in pixels {
            let bucket = &mut self.buckets[tracker_bucket(x, y)];
            if let Some(index) = bucket
                .iter()
                .position(|entry| entry.stable_id == stable_id && entry.x == x && entry.y == y)
            {
                bucket.remove(index);
            }
        }
        if let Some(cache) = self.objects.get_mut(&stable_id) {
            cache.registered = false;
            cache.pixels.clear();
        }
    }

    /// RenderCellPixel's forward bucket scan, once for each occupied exact
    /// coordinate. An ineligible earlier entry does not block a later one.
    pub fn visible_winners(
        &self,
        mut eligible: impl FnMut(RadarTrackerEntry) -> bool,
    ) -> Vec<RadarTrackerEntry> {
        let mut chosen = BTreeSet::new();
        let mut winners = Vec::new();
        for bucket in &self.buckets {
            for &entry in bucket {
                let coordinate = (entry.x, entry.y);
                if chosen.contains(&coordinate) || !eligible(entry) {
                    continue;
                }
                chosen.insert(coordinate);
                winners.push(entry);
            }
        }
        winners
    }

    /// `RadarClass::GetObjectAtRadarPixel @ 0x00656750` scans the selected
    /// bucket from back to front and returns the first exact-coordinate object.
    /// This is deliberately the reverse of `RenderCellPixel`'s forward scan.
    pub fn object_at_pixel(&self, x: i32, y: i32) -> Option<u64> {
        self.buckets[tracker_bucket(x, y)]
            .iter()
            .rev()
            .find(|entry| entry.x == x && entry.y == y)
            .map(|entry| entry.stable_id)
    }

    #[cfg(test)]
    pub(super) fn is_registered(&self, stable_id: u64) -> bool {
        self.objects
            .get(&stable_id)
            .is_some_and(|cache| cache.registered)
    }

    #[cfg(test)]
    fn entries_at(&self, x: i32, y: i32) -> Vec<u64> {
        self.buckets[tracker_bucket(x, y)]
            .iter()
            .filter(|entry| entry.x == x && entry.y == y)
            .map(|entry| entry.stable_id)
            .collect()
    }
}

fn tracker_bucket(x: i32, y: i32) -> usize {
    x.wrapping_sub(y.wrapping_mul(5)) as u32 as usize & 0xff
}

/// Building radar brush constructor @ 0x006563B0. Native scales the foundation
/// width/height by radar zoom, rounds with +0.5, applies the 1-vs-2 minimum,
/// then emits this ordered isometric diamond rather than an occupancy square.
pub(super) fn radar_foundation_brush(
    foundation_width: u32,
    foundation_height: u32,
    radar_scale: f32,
) -> Vec<(i32, i32)> {
    let scaled_dimension = |cells: u32| {
        let rounded = (cells as f32 * radar_scale + 0.5).floor() as i32;
        rounded.max(if cells <= 1 { 1 } else { 2 })
    };
    let width = scaled_dimension(foundation_width);
    let height = scaled_dimension(foundation_height);
    let mut pixels = Vec::new();
    for row in 0..width.wrapping_add(height).wrapping_sub(1) {
        let start = if row < height {
            -row
        } else {
            row.wrapping_sub(height.wrapping_mul(2)).wrapping_add(2)
        };
        let end = if row < width {
            row
        } else {
            width.wrapping_mul(2).wrapping_sub(row).wrapping_sub(2)
        };
        for x in start..=end {
            pixels.push((x, row));
        }
    }
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::radar_visibility::RadarMobileVisibilityFacts;
    use crate::sim::intern::test_intern;

    fn visible_mobile_facts() -> RadarMobileVisibilityFacts {
        RadarMobileVisibilityFacts {
            type_invisible: false,
            sinking: false,
            object_alive: true,
            in_limbo: false,
            owner_is_human_player: false,
            fresh_in_playfield: true,
            shrouded: false,
            cloak_state: 0,
            has_sensor: false,
            allied_with_current_player: false,
            height_leptons: 0,
            veteran_radar_invisible: false,
        }
    }

    fn update(stable_id: u64, local_front: bool) -> RadarObjectUpdate {
        RadarObjectUpdate {
            stable_id,
            owner: test_intern(if local_front { "Local" } else { "Enemy" }),
            origin: (40, 60),
            event_source_cell: Some((10, 20)),
            enemy_sensed_prefilter: true,
            foundation: None,
            radar_scale: 1.0,
            discovery_observed: true,
            visibility: RadarRegistrationVisibilityFacts::Mobile(visible_mobile_facts()),
            local_front,
        }
    }

    #[test]
    fn radar_tracker_local_front_beats_enemy_despite_opposite_stable_ids() {
        let mut tracker = RetainedRadarTracker::default();
        tracker.update_object(update(1, false), false);
        tracker.update_object(update(99, true), false);
        assert_eq!(tracker.entries_at(40, 60), vec![99, 1]);
        assert_eq!(tracker.visible_winners(|_| true)[0].stable_id, 99);
    }

    #[test]
    fn radar_tracker_duplicate_insert_is_rejected_and_removal_preserves_order() {
        let mut tracker = RetainedRadarTracker::default();
        tracker.update_object(update(1, false), false);
        tracker.register(1, &[(40, 60)], false);
        tracker.update_object(update(2, false), false);
        tracker.update_object(update(3, false), false);
        assert_eq!(tracker.entries_at(40, 60), vec![1, 2, 3]);
        tracker.unregister(2);
        assert_eq!(tracker.entries_at(40, 60), vec![1, 3]);
    }

    #[test]
    fn radar_tracker_move_hidden_and_newly_visible_follow_remove_write_add_order() {
        let mut tracker = RetainedRadarTracker::default();
        tracker.update_object(update(1, false), false);
        let mut moved = update(1, false);
        moved.origin = (41, 61);
        tracker.update_object(moved, false);
        assert!(tracker.entries_at(40, 60).is_empty());
        assert_eq!(tracker.entries_at(41, 61), vec![1]);

        moved.visibility = RadarRegistrationVisibilityFacts::Mobile(RadarMobileVisibilityFacts {
            shrouded: true,
            ..visible_mobile_facts()
        });
        tracker.update_object(moved, false);
        assert!(!tracker.is_registered(1));
        moved.visibility = RadarRegistrationVisibilityFacts::Mobile(visible_mobile_facts());
        tracker.update_object(moved, false);
        assert!(tracker.is_registered(1));
        assert_eq!(tracker.entries_at(41, 61), vec![1]);
    }

    #[test]
    fn radar_tracker_consumes_nonzero_visibility_outcode_as_local_event() {
        let mut tracker = RetainedRadarTracker::default();
        let mut sensed = update(1, false);
        sensed.visibility = RadarRegistrationVisibilityFacts::Mobile(RadarMobileVisibilityFacts {
            cloak_state: 2,
            has_sensor: true,
            ..visible_mobile_facts()
        });
        assert_eq!(
            tracker.update_object(sensed, false),
            Some(RadarSensedPresentationEvent {
                stable_id: 1,
                out_code: 1,
                cell: (10, 20),
            })
        );
        sensed.visibility = RadarRegistrationVisibilityFacts::Mobile(RadarMobileVisibilityFacts {
            cloak_state: 2,
            has_sensor: true,
            allied_with_current_player: true,
            ..visible_mobile_facts()
        });
        assert_eq!(tracker.update_object(sensed, false), None);
        sensed.visibility = RadarRegistrationVisibilityFacts::Mobile(RadarMobileVisibilityFacts {
            cloak_state: 2,
            has_sensor: true,
            allied_with_current_player: false,
            ..visible_mobile_facts()
        });
        sensed.enemy_sensed_prefilter = false;
        assert_eq!(
            tracker.update_object(sensed, false),
            None,
            "native Foot/+0x684 prefilter rejects before CreateRadarEvent"
        );
    }

    #[test]
    fn radar_tracker_building_brush_is_native_irregular_diamond() {
        assert_eq!(
            radar_foundation_brush(3, 3, 1.0),
            vec![
                (0, 0),
                (-1, 1),
                (0, 1),
                (1, 1),
                (-2, 2),
                (-1, 2),
                (0, 2),
                (1, 2),
                (2, 2),
                (-1, 3),
                (0, 3),
                (1, 3),
                (0, 4),
            ]
        );
    }

    #[test]
    fn radar_tracker_action40_immediately_repopulates_building_not_mobile() {
        let mut tracker = RetainedRadarTracker::default();
        tracker.update_object(update(1, false), false);
        let mut building = update(2, false);
        building.foundation = Some((2, 2));
        tracker.update_object(building, false);
        tracker.reset_for_action40();
        assert!(!tracker.is_registered(1));
        assert!(!tracker.is_registered(2));
        assert!(tracker.take_action40_building_tail_pending());

        // FUN_006E21E0's reverse-Building vtable+0x4A0(1) tail is immediate.
        tracker.update_object(building, true);
        assert!(tracker.is_registered(2));
        assert!(!tracker.is_registered(1));
        // The mobile returns only at its later ordinary Techno +0x4A0 visit.
        tracker.update_object(update(1, false), false);
        assert!(tracker.is_registered(1));
    }

    #[test]
    fn radar_tracker_two_identical_action40_resets_both_rebuild() {
        let mut tracker = RetainedRadarTracker::default();
        let mut building = update(2, false);
        building.foundation = Some((2, 2));
        tracker.update_object(building, false);
        for _ in 0..2 {
            tracker.reset_for_action40();
            assert!(tracker.take_action40_building_tail_pending());
            assert!(!tracker.is_registered(2));
            tracker.update_object(building, true);
            assert!(tracker.is_registered(2));
            assert_eq!(tracker.entries_at(40, 60).len(), 1);
        }
    }

    #[test]
    fn radar_tracker_action40_building_tail_uses_reverse_creation_order() {
        let mut tracker = RetainedRadarTracker::default();
        let mut older = update(1, false);
        older.foundation = Some((1, 1));
        let mut newer = update(2, false);
        newer.foundation = Some((1, 1));
        tracker.reset_for_action40();
        assert!(tracker.take_action40_building_tail_pending());
        // BuildingClass array insertion follows monotonic Abstract stable
        // creation IDs in Rust; FUN_006E21E0 walks that array backward.
        tracker.update_object(newer, true);
        tracker.update_object(older, true);
        assert_eq!(tracker.entries_at(40, 60), vec![2, 1]);
    }

    #[test]
    fn radar_click_lookup_reverse_scans_exact_coordinate_bucket() {
        let mut tracker = RetainedRadarTracker::default();
        tracker.update_object(update(1, false), false);
        tracker.update_object(update(2, false), false);
        assert_eq!(tracker.entries_at(40, 60), vec![1, 2]);
        assert_eq!(tracker.object_at_pixel(40, 60), Some(2));
        assert_eq!(tracker.object_at_pixel(41, 60), None);
    }
}
