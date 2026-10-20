//! Retained DiskLaserClass shots, independently of their copied LaserDraws.
//!
//! Active YR: ctor4A7A30, Fire4A71A0, AI4A7340, Detach4A7900,
//! scalar destructor4A7C90 (equivalent body4A7B00). Logic55B5A1 visits
//! vector8A0208 backwards before LaserUpdate and the live object walk.
//! Native instructions and executable controls: tools/spatial_oracle/disk_laser.md.
//! No Logic/Display registration, native Create_ID or private deletion queue.

use super::{FrameEffects, SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::combat::TargetKind;
use crate::sim::combat::fire_coord::{FireSource, effect_target_coordinate, fire_coordinate};
use crate::sim::combat::laser::{LaserColor, construct};
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::{object_get_coords, target_get_coords};
use crate::sim::projectile::ProjectileCoord;
use crate::util::native_x87::NativeF32Bits;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

/// Original4A6CF0 initializes8A0180 via its literal angles and retail trig.
/// The final two angles are235/257 degrees: this is not a regular16-gon.
/// Consumed original table controls are in disk_laser.json:ring_init.
const RING: [[i32; 2]; 16] = [
    [0, -240],
    [89, -222],
    [169, -169],
    [220, -93],
    [240, 0],
    [222, 89],
    [169, 169],
    [93, 220],
    [0, 240],
    [-89, 222],
    [-169, 169],
    [-220, 93],
    [-240, 0],
    [-222, -89],
    [-137, -196],
    [-54, -233],
];

#[derive(Debug, Clone, Copy, Default, Hash, Serialize, Deserialize)]
struct DiskLaser {
    source: Option<u64>,
    target: Option<TargetKind>,
    weapon: Option<InternedId>,
    countdown: i32,
    start: u8,
    step: i32,
    damage: i32,
}

/// Shared stable IDs are strictly increasing. Thus key order is the original
/// vector's birth order; removal is stable without a second ordered registry.
#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct DiskLasers {
    shots: BTreeMap<u64, DiskLaser>,
}

impl DiskLasers {
    pub(super) fn hash_state(&self, hasher: &mut impl Hasher) {
        // Empty ownership introduces no fold into existing unrelated scenes.
        if !self.shots.is_empty() {
            b"DiskLaserClass4A7340".hash(hasher);
            self.shots.len().hash(hasher);
            for (id, shot) in &self.shots {
                id.hash(hasher);
                shot.hash(hasher);
            }
        }
    }
}

/// Copied diagnostics of the one authoritative shot body. Reading these facts
/// does not advance its AI, query coordinates or change RNG/deletion state.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct DiskLaserObservation {
    pub id: u64,
    pub source: Option<u64>,
    pub target: Option<TargetKind>,
    pub weapon: Option<InternedId>,
    pub countdown: i32,
    pub start: u8,
    pub step: i32,
    pub damage: i32,
}

impl Simulation {
    pub fn disk_laser_observations(
        &self,
    ) -> impl DoubleEndedIterator<Item = DiskLaserObservation> + '_ {
        self.disk_lasers
            .shots
            .iter()
            .map(|(&id, shot)| DiskLaserObservation {
                id,
                source: shot.source,
                target: shot.target,
                weapon: shot.weapon,
                countdown: shot.countdown,
                start: shot.start,
                step: shot.step,
                damage: shot.damage,
            })
    }

    pub(crate) fn has_disk_laser(&self, id: u64) -> bool {
        self.disk_lasers.shots.contains_key(&id)
    }

    /// Ctor4A7A30 calls Abstract410170, which leaves UniqueID=-1. It never
    /// calls Create_ID410230/68BCB0 or spends Scenario+214. Reserve only the
    /// independent Rust handle, before FireAt's GetROF continuation.
    pub(crate) fn construct_disk_laser(&mut self) -> u64 {
        let id = self.allocate_stable_id();
        assert!(
            self.disk_lasers
                .shots
                .insert(id, DiskLaser::default())
                .is_none()
        );
        id
    }

    /// Fire4A71A0. NULL input queues the unchanged constructor body; valid
    /// pointers retain current raw GetCoords and initialize the ring cursor.
    pub(crate) fn fire_disk_laser(
        &mut self,
        id: u64,
        source: Option<u64>,
        target: Option<TargetKind>,
        weapon: Option<InternedId>,
        damage: i32,
    ) {
        let (Some(source), Some(target), Some(weapon)) = (source, target, weapon) else {
            self.disk_lasers
                .shots
                .get_mut(&id)
                .expect("constructed DiskLaser")
                .countdown = -1;
            self.substrate.pending_delete.queue_disk_laser(id);
            return;
        };
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(NativeCellQuery::canonical);
        let target_coord =
            target_get_coords(target, self, cells.as_ref()).expect("retained Disk target");
        let source_coord = object_get_coords(
            self.substrate
                .entities
                .get(source)
                .expect("retained Disk source"),
            self.resolved_terrain.as_ref(),
        );
        let facing = crate::util::direction_tables::facing16_between(
            [source_coord.x, source_coord.y],
            [target_coord.x, target_coord.y],
        );
        let start = (((((u32::from(facing) >> 11) + 1) >> 1) & 15) + 8) & 15;
        *self
            .disk_lasers
            .shots
            .get_mut(&id)
            .expect("constructed DiskLaser") = DiskLaser {
            source: Some(source),
            target: Some(target),
            weapon: Some(weapon),
            countdown: 0,
            start: start as u8,
            step: 0,
            damage,
        };
    }

    /// DispatchPointerExpired725977..725993 calls Detach4A7900 in reverse
    /// birth order. Matching either pointer marks -1 and appends, including
    /// duplicates. Fields and previously created LaserDraws remain intact.
    pub(super) fn detach_disk_lasers(&mut self, expired_id: u64) {
        for (&id, shot) in self.disk_lasers.shots.iter_mut().rev() {
            if shot.source == Some(expired_id)
                || shot.target == Some(TargetKind::Entity(expired_id))
            {
                shot.countdown = -1;
                self.substrate.pending_delete.queue_disk_laser(id);
            }
        }
    }

    /// The scalar destructor4A7C90 stable-removes8A0208 and calls4101F0,
    /// whose whole body only resets four interfaces. No Object UnInit,
    /// pointer-expiry broadcast, Logic/Display removal or LaserDraw deletion.
    pub(super) fn destroy_disk_laser(&mut self, id: u64) {
        self.disk_lasers
            .shots
            .remove(&id)
            .expect("queued DiskLaser");
    }

    /// Whole selected AI4A7340. Countdowns precede all pointer queries.
    /// Native55B5A1 captures the reverse walk before newborn object-AI shots;
    /// an increasing-ID birth during a nested receiver also waits next pass.
    pub(super) fn tick_disk_lasers(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let mut cursor = None;
        let mut bridge_changed = false;
        loop {
            let next = match cursor {
                None => self.disk_lasers.shots.last_key_value(),
                Some(id) => self.disk_lasers.shots.range(..id).next_back(),
            };
            let Some((&id, &shot)) = next else { break };
            cursor = Some(id);
            if shot.countdown < 0 {
                self.disk_lasers.shots.get_mut(&id).unwrap().countdown = -1;
                self.substrate.pending_delete.queue_disk_laser(id);
                continue;
            }
            if shot.countdown > 0 {
                self.disk_lasers.shots.get_mut(&id).unwrap().countdown -= 1;
                continue;
            }
            let source_id = shot.source.expect("armed Disk source");
            let target = shot.target.expect("armed Disk target");
            let weapon_id = shot.weapon.expect("armed Disk weapon");
            let weapon = rules
                .weapon(self.interner.resolve(weapon_id))
                .expect("retained Disk weapon");
            let source = self
                .substrate
                .entities
                .get(source_id)
                .expect("live Disk source before Detach");
            let mut from = object_get_coords(source, self.resolved_terrain.as_ref());
            let cells = self
                .resolved_terrain
                .as_ref()
                .map(NativeCellQuery::canonical);
            let to = target_get_coords(target, self, cells.as_ref())
                .expect("live Disk target before Detach");
            if crate::sim::movement::air_movement::is_high_flying(
                source,
                self.resolved_terrain.as_ref(),
                Some((rules, &self.interner)),
            ) {
                from.z = to.z;
            }
            let mut distance = crate::util::native_x87::distance_3d_leptons(
                [from.x, from.y, from.z],
                [to.x, to.y, to.z],
            );
            if let TargetKind::Entity(target_id) = target
                && let Some(building) = self.substrate.entities.get(target_id)
                && building.category() == EntityCategory::Structure
            {
                let (width, height) = crate::rules::foundation::foundation_dimensions(
                    building.type_facts.foundation(),
                );
                distance = distance
                    .wrapping_sub((i32::from(width) + i32::from(height)) * 64)
                    .max(0);
            }
            if distance > weapon.range_leptons || source.crash.is_crashing() {
                self.disk_lasers.shots.get_mut(&id).unwrap().countdown = -1;
                self.substrate.pending_delete.queue_disk_laser(id);
                continue;
            }
            let obj = self
                .interner
                .object_type(source.type_ref(), rules)
                .expect("Disk source type");
            let muzzle = fire_coordinate(
                self,
                rules,
                &FireSource::of_entity(source),
                obj,
                0,
                (source.weapon_burst.index() & 1) as u8,
                Default::default(),
            )
            .coord;
            let color = if weapon.is_house_color {
                LaserColor::HouseWithHalfOuter {
                    owner: source.owner(),
                    spread: weapon.laser_outer_spread,
                }
            } else {
                LaserColor::Explicit {
                    inner: weapon.laser_inner_color,
                    outer: weapon.laser_outer_color,
                    spread: weapon.laser_outer_spread,
                }
            };
            let a = (i32::from(shot.start) + shot.step) & 15;
            let b = (i32::from(shot.start) - shot.step + 16) & 15;
            let point = |index: i32| {
                let [x, y] = RING[(index & 15) as usize];
                ProjectileCoord::new(muzzle.x.wrapping_add(x), muzzle.y.wrapping_add(y), muzzle.z)
            };
            let first = point(a);
            if a == b && shot.step != 0 {
                let impact = effect_target_coordinate(self, rules, target.projectile_target(self))
                    .expect("Disk effect target");
                construct(
                    self,
                    first,
                    impact,
                    0,
                    weapon.laser_duration,
                    color,
                    NativeF32Bits::POSITIVE_ZERO,
                );
                let warhead_name = weapon.warhead.as_deref().expect("Disk damage warhead");
                let warhead = rules.warhead(warhead_name).expect("Disk damage warhead");
                let warhead_id = self.interner.intern(warhead_name);
                bridge_changed |= crate::sim::combat::world_receiver::apply_area_damage(
                    self,
                    rules,
                    registry,
                    impact,
                    shot.damage,
                    warhead,
                    (source_id, None, warhead_id),
                    effects,
                );
                if let Some(source) = self.substrate.entities.get(source_id)
                    && let Some(report) = crate::sim::combat::world_fire::per_shot_report(
                        weapon,
                        false,
                        source.techno_ctor_random_word(),
                    )
                {
                    self.sound_events.push(SimSoundEvent::voc_at_world_coord(
                        report.to_owned(),
                        AnimWorldCoord {
                            x: first.x,
                            y: first.y,
                            z: first.z,
                        },
                    ));
                    effects.flush_sound(self, Some(rules));
                }
                self.disk_lasers.shots.get_mut(&id).unwrap().countdown = -1;
            } else {
                if shot.step == 0
                    && let Some(sound) = rules.general.disk_laser_charge_up()
                {
                    self.sound_events.push(SimSoundEvent::voc_at_world_coord(
                        sound.to_owned(),
                        AnimWorldCoord {
                            x: first.x,
                            y: first.y,
                            z: first.z,
                        },
                    ));
                    effects.flush_sound(self, Some(rules));
                }
                let duration = 8 - shot.step;
                construct(
                    self,
                    first,
                    point(a + 1),
                    0,
                    duration,
                    color,
                    NativeF32Bits::from_bits(0x3F000000),
                );
                construct(
                    self,
                    point(b),
                    point(b - 1),
                    0,
                    duration,
                    color,
                    NativeF32Bits::from_bits(0x3F000000),
                );
                let retained = self.disk_lasers.shots.get_mut(&id).unwrap();
                retained.countdown = 1;
                retained.step += 1;
            }
        }
        bridge_changed
    }

    /// Save4A7C10 persists the40-byte native body; Load4A7B90 swizzles its
    /// pointers. Validate through the owner without exposing mutable storage.
    /// Negative countdown never dereferences stale post-Detach pointers.
    pub(crate) fn validate_disk_lasers_after_restore(
        &self,
        rules: Option<&RuleSet>,
    ) -> Result<(), (u64, &'static str)> {
        if self.disk_lasers.shots.is_empty() {
            return Ok(());
        }
        let logic_ids: BTreeSet<_> = self.substrate.logic.as_slice().iter().copied().collect();
        for (&id, shot) in &self.disk_lasers.shots {
            if logic_ids.contains(&id) || self.display_layers().layer_of(id).is_some() {
                return Err((id, "DiskLaser registered in Logic or Display"));
            }
            if shot.start > 15 || !(0..=8).contains(&shot.step) || shot.countdown > 1 {
                return Err((id, "invalid ring cursor or countdown"));
            }
            if let Some(weapon) = shot.weapon {
                let Some(name) = self.interner.try_resolve(weapon) else {
                    return Err((id, "missing weapon identity"));
                };
                if rules.is_some_and(|rules| rules.weapon(name).is_none()) {
                    return Err((id, "missing retained weapon"));
                }
            }
            if shot.countdown >= 0 {
                if !shot
                    .source
                    .is_some_and(|source| self.substrate.entities.get(source).is_some())
                {
                    return Err((id, "missing active source"));
                }
                if !shot.target.is_some_and(|target| match target {
                    TargetKind::Entity(target) => {
                        self.substrate.entities.get(target).is_some()
                            || self.terrain_objects.objects().contains_key(&target)
                    }
                    TargetKind::Cell(_, _) => true,
                }) {
                    return Err((id, "missing active target"));
                }
                if shot.weapon.is_none() {
                    return Err((id, "missing active weapon"));
                }
            }
        }
        Ok(())
    }
}
