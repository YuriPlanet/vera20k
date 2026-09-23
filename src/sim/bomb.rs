//! The Crazy Ivan's time bomb: `BombClass` and `BombListClass`.
//!
//! Native owner: `BombListClass` (the live list at `0x0087F5D8`) and its
//! `BombClass` records; the carrier holds its bomb at `ObjectClass+0x38`.
//! Here the carrier's `GameEntity::bomb` is the record and `Simulation::bombs`
//! the list; only this module writes either.
//!
//! - Planting: the IvanBomb arm of `BulletClass::DetonateAtCoord`
//!   (`0x00469343`, on both deliveries: the stock bombs are Inviso) calls
//!   `BombListClass::Attach @ 0x00438E70`. The firer must be Infantry and the
//!   target a Techno without a bomb; alliance, `Bombable=`, buildings and
//!   health are not tested. The bomb keeps its planter, the planter's house
//!   and its fuse, `end = attach frame + IvanTimedDelay` (a plain 32-bit add).
//!   The shot itself deals no damage.
//! - Fuse: the carrier's `TechnoClass::AI_Update` (`0x006FA6F5`) sets it off
//!   while it is not in limbo once `Frame > end` (`IsTimerExpired @
//!   0x00438A70`, signed): IvanTimedDelay + 1 frames after the attach. Frames
//!   in limbo do not extend it.
//! - Blast (`BombClass::Detonate @ 0x00438720`): the record goes first; a
//!   carrier in limbo is silent. Otherwise Apply_area_damage at the carrier's
//!   Location with IvanDamage and IvanWarhead, the planter as the source (none
//!   once its pointer expired) and no source house; then the warhead's
//!   explosion anim; then, for a `BridgeRepairHut=` carrier, the hut's bridge
//!   collapse.
//! - The carrier's death (`TechnoClass::ReceiveDamage` `0x00702672`, after its
//!   death weapon) sets its bomb off. A dying Crazy Ivan's `Explodes=` death
//!   weapon is his own bomber, so it plants a bomb on him that goes off at once.
//! - Silent removal (`BombClass::Defuse @ 0x004389B0`): an Engineer's
//!   BombDisarm hit (`0x004699C4`), the carrier's UnInit (`0x005F65F3`: sold,
//!   crushed, erased), a building changing hands unless `CanBeOccupied=`
//!   (`BuildingClass::ChangeOwner` `0x00448277`), the destructor
//!   (`0x005F3BA6`).
//! - The planter's pointer expiry nulls it (`BombListClass::PointerGotInvalid
//!   @ 0x00439150`); its bombs still go off, credited to no one.
//! - Fire legality (`TechnoClass::GetFireError` `0x006FCB8D`, `0x006FCBAD`): a
//!   BombDisarm weapon needs a bombed target, an IvanBomb weapon an unbombed
//!   one.
//! - Who sees it (`BombListClass::UpdateAll @ 0x00438BF0`, every frame before
//!   the object AI): every 46th call, and the second call after an attach, the
//!   carrier's `+0x68` becomes "the local player owns the planter's house or a
//!   BombSight detector within range". It gates the clock over the carrier
//!   (`TechnoClass::DrawExtras`, BOMBCURS.SHP) and the Engineer's DisarmBomb
//!   cursor. Each bomb here keeps the answer for every house (`seen_by`); the
//!   app reads the local player's.
//! - The ticking loop (`BombTickingSound=`, same UpdateAll) plays at the
//!   carrier for every player while it is out of limbo; the app drives it
//!   under `ticking_sound_owner`.
//!
//! Scenario draws: none of its own; the blast's receivers and anim draw in
//! their owners.
//!
//! Evidence: `tools/spatial_oracle/bomb_class.py` runs the original Attach,
//! IsTimerExpired, GetClockFrame, Detonate, Defuse, UpdateAll, the AI_Update
//! fuse check, the two DetonateAtCoord arms and the GetFireError gates.
//!
//! RESIDUALS:
//! - The death bomb (kind `+0x30 = 1`: no fuse, clock frame 12) has no native
//!   writer, so only timed bombs exist here.
//! - The player's DETONATE action (`EventClass` `0x004C7823`, behind
//!   `CanDetonateTimeBomb=`/`CanDetonateDeathBomb=`, both off in stock) is not
//!   represented.
//! - Campaign visibility: `HouseClass::IsHumanPlayer @ 0x0050B6F0` also admits
//!   a house with PlayerControl (`+0x1ED`) in a campaign; the app tests the
//!   local player's house only, as for the other gated sounds. Trigger: a
//!   bomb planted by, or seen by a detector of, a player-controlled allied
//!   campaign house. Effect: its clock stays hidden. Frequency: rare.

use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::{object_center_coord, position_world_coord};
use crate::sim::world::{SimSoundEvent, Simulation};

/// `BombListClass` (`0x0087F5D8`): the carriers of the live bombs and the
/// BombVisible countdown. Nothing here is saved: the records live on their
/// carriers, and a load starts from `Default` (`BombListClass::Clear`) and
/// rebuilds the carriers ([`Simulation::rebuild_bomb_carriers`]).
#[derive(Debug, Clone)]
pub(crate) struct BombList {
    /// An index of the objects whose `GameEntity::bomb` is set, which is the
    /// source of truth. This module's writers (attach, defuse, blast) change
    /// both together; debug builds check the two agree at every BombVisible
    /// refresh.
    carriers: BTreeSet<u64>,
    /// `+0x30`: UpdateAll calls left before the next BombVisible refresh.
    /// `BombListClass::Clear @ 0x00439110` sets 45 when a scenario starts and
    /// before a load, and the list's Save does not write it.
    visibility_countdown: i32,
}

/// `BombListClass::Clear`'s countdown, also UpdateAll's refresh period less one.
const CLEARED_VISIBILITY_COUNTDOWN: i32 = 45;

impl Default for BombList {
    fn default() -> Self {
        Self {
            carriers: BTreeSet::new(),
            visibility_countdown: CLEARED_VISIBILITY_COUNTDOWN,
        }
    }
}

/// One armed bomb (`BombClass`), held by its carrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bomb {
    /// `+0x24`: the Infantry that planted it, until its pointer expires.
    planter: Option<u64>,
    /// `+0x28`: the planter's house at the attach.
    planter_house: InternedId,
    /// `+0x34`: the attach frame.
    start_frame: i32,
    /// `+0x38`: `start_frame + IvanTimedDelay`.
    end_frame: i32,
    /// The houses whose player sees it, bit `n` for
    /// `ScenarioSession::house_order[n]`: native's per-client BombVisible
    /// (the carrier's `+0x68`) for every house at once. Written only by
    /// `bomb_list_update`; saved like `+0x68`, but not hashed.
    #[serde(default)]
    seen_by: u64,
}

impl Hash for Bomb {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // `seen_by` is presentation on its own cadence, which a load restarts.
        self.planter.hash(state);
        self.planter_house.hash(state);
        self.start_frame.hash(state);
        self.end_frame.hash(state);
    }
}

/// The loop-handle owner of a carrier's ticking sound: native gives each bomb
/// its own VocHandle (`BombClass+0x3C`), apart from the carrier's own sounds,
/// so a tag bit keeps the key apart from every stable object id.
const TICKING_SOUND_TAG: u64 = 1 << 62;

pub(crate) const fn ticking_sound_owner(carrier: u64) -> u64 {
    carrier | TICKING_SOUND_TAG
}

pub(crate) const fn ticking_sound_carrier(owner: u64) -> Option<u64> {
    if owner & TICKING_SOUND_TAG != 0 {
        Some(owner & !TICKING_SOUND_TAG)
    } else {
        None
    }
}

/// A house's bit in `Bomb::seen_by`: its place in the house order (none past 64).
fn house_bit(house_order: &[InternedId], house: InternedId) -> u64 {
    house_order
        .iter()
        .position(|&candidate| candidate == house)
        .filter(|&index| index < 64)
        .map_or(0, |index| 1 << index)
}

/// UpdateAll's range test (`0x00438D9C..0x00438E0B`): each axis `detector -
/// carrier` (`SUB`), FILD'ed and summed as `(dx*dx + dy*dy) + dz*dz`, `FSTP
/// double`, `Sqrt_Approx @ 0x004CAC40`, the truncating ftol, then `distance <
/// BombSight << 8` (`JL`). For a BombSight of 1 to 63 cells that is exactly
/// `d² < range²`, computed here in integers: the chopped float and the table
/// root never round up, and `range²` (a multiple of 2^16 that fits the float)
/// starts a table bucket whose root is exact. `native_update_all_corpus` pins
/// the edges. A range of zero or less sees nothing.
fn within_bomb_sight(detector: DriveCoord, carrier: DriveCoord, range: i32) -> bool {
    let axis = |a: i32, b: i32| i128::from(a.wrapping_sub(b));
    let squared = axis(detector.x, carrier.x).pow(2)
        + axis(detector.y, carrier.y).pow(2)
        + axis(detector.z, carrier.z).pow(2);
    range > 0 && squared < i128::from(range).pow(2)
}

impl Bomb {
    /// `+0x24`: the planter, until its pointer expires.
    pub(crate) fn planter(&self) -> Option<u64> {
        self.planter
    }

    /// `BombClass::IsTimerExpired @ 0x00438A70` for a timed bomb.
    pub(crate) fn expired(&self, frame: i32) -> bool {
        frame > self.end_frame
    }

    /// `BombClass::GetClockFrame @ 0x00438A00`: the clock art's frame, the
    /// hand (`2 * elapsed / (IvanTimedDelay / 6)`) plus the blink in its global
    /// phase, at most 11. `None` where native divides by zero.
    pub(crate) fn clock_frame(&self, frame: i32, delay: i32, flicker_rate: i32) -> Option<i32> {
        let hand = frame
            .wrapping_sub(self.start_frame)
            .checked_div(delay / 6)?
            .wrapping_mul(2);
        let blink = frame.checked_rem(flicker_rate.wrapping_mul(2))? >= flicker_rate;
        Some((hand + i32::from(blink)).min(11))
    }
}

/// A detonation's inputs, read before the record goes (`0x00438741..0x0043879F`).
pub(crate) struct BombBlast {
    pub(crate) carrier: u64,
    /// The planter, or `RAD_NO_ATTACKER` once its pointer expired.
    pub(crate) source: u64,
    pub(crate) bridge_hut: bool,
}

impl Simulation {
    /// The live bombs' carriers (`BombListClass`'s list).
    pub(crate) fn bomb_carriers(&self) -> &BTreeSet<u64> {
        &self.bombs.carriers
    }

    /// The objects carrying a bomb, by stable id.
    fn carried_bombs(&self) -> impl Iterator<Item = u64> + '_ {
        self.substrate
            .entities
            .iter_sorted()
            .filter(|(_, entity)| entity.bomb.is_some())
            .map(|(id, _)| id)
    }

    /// Rebuild the carrier index from the carried records after a load.
    pub(crate) fn rebuild_bomb_carriers(&mut self) {
        self.bombs.carriers = self.carried_bombs().collect();
    }

    /// Whether the player owning `house` sees the carrier's bomb: its
    /// BombVisible (`+0x68`) as of the last refresh.
    pub(crate) fn bomb_seen_by(&self, carrier: u64, house: InternedId) -> bool {
        self.substrate
            .entities
            .get(carrier)
            .and_then(|entity| entity.bomb)
            .is_some_and(|bomb| bomb.seen_by & house_bit(&self.session.house_order, house) != 0)
    }

    /// Where the carrier's ticking loop plays: its Location (`+0x9C`), while it
    /// carries a bomb out of limbo (`0x00438C84..0x00438CFE`); `None` stops it.
    pub(crate) fn bomb_ticking_coord(&self, carrier: u64) -> Option<AnimWorldCoord> {
        let entity = self.substrate.entities.get(carrier)?;
        (entity.bomb.is_some() && !entity.lifecycle.in_limbo)
            .then(|| Self::movement_sound_world(entity))
    }

    /// `BombListClass::UpdateAll @ 0x00438BF0`, once per frame from
    /// `LogicClass::PerTickUpdate` (`0x0055B4E1`), after ore growth and before
    /// the Teams and the object vector. Its first pass purges spent records,
    /// which VERA drops as they go off or are defused; its second drives the
    /// ticking loop, which the app derives from the carriers. What is left is
    /// the BombVisible refresh (`0x00438D04..0x00438E4C`).
    pub(crate) fn bomb_list_update(&mut self, rules: &RuleSet) {
        if self.bombs.visibility_countdown > 0 {
            self.bombs.visibility_countdown -= 1;
            return;
        }
        self.bombs.visibility_countdown = CLEARED_VISIBILITY_COUNTDOWN;
        debug_assert!(
            self.carried_bombs().eq(self.bombs.carriers.iter().copied()),
            "the carrier index disagrees with the carried bombs"
        );
        if self.bombs.carriers.is_empty() {
            return;
        }
        let order = &self.session.house_order;
        // The detectors (`BombList+0x1C`): `TechnoClass::Unlimbo` adds an
        // object whose type has a nonzero BombSight (`0x006F6D70`), `Limbo`
        // takes it out (`0x006F6B7A`). Coordinates are GetCoords (`vt+0x48`).
        let mut detectors = Vec::new();
        for entity in self.substrate.entities.values() {
            if entity.lifecycle.in_limbo || !entity.lifecycle.object_alive {
                continue;
            }
            let Some(object) = rules.object(self.interner.resolve(entity.type_ref())) else {
                continue;
            };
            if object.bomb_sight != 0 {
                detectors.push((
                    house_bit(order, entity.owner()),
                    object_center_coord(entity, object),
                    object.bomb_sight.wrapping_shl(8),
                ));
            }
        }
        let mut refreshed = Vec::with_capacity(self.bombs.carriers.len());
        for &carrier in &self.bombs.carriers {
            let Some(entity) = self.substrate.entities.get(carrier) else {
                continue;
            };
            let Some(bomb) = entity.bomb else {
                continue;
            };
            let center = rules
                .object(self.interner.resolve(entity.type_ref()))
                .map_or_else(
                    || position_world_coord(&entity.position),
                    |object| object_center_coord(entity, object),
                );
            let mut seen_by = house_bit(order, bomb.planter_house);
            for &(house, detector, range) in &detectors {
                if seen_by & house == 0 && within_bomb_sight(detector, center, range) {
                    seen_by |= house;
                }
            }
            refreshed.push((carrier, seen_by));
        }
        for (carrier, seen_by) in refreshed {
            if let Some(bomb) = self
                .substrate
                .entities
                .get_mut(carrier)
                .and_then(|entity| entity.bomb.as_mut())
                && bomb.seen_by != seen_by
            {
                log::info!(
                    "BOMB {carrier} seen by {:#x} (was {:#x})",
                    seen_by,
                    bomb.seen_by
                );
                bomb.seen_by = seen_by;
            }
        }
    }

    /// The IvanBomb arm of `BulletClass::DetonateAtCoord` (`0x00469343`) and
    /// `BombListClass::Attach @ 0x00438E70`: the bullet's owner plants a bomb
    /// on its target when the owner is Infantry and the target carries none.
    pub(crate) fn bomb_attach(&mut self, planter: u64, target: Option<u64>, rules: &RuleSet) {
        // An object UnInit has passed reads as the null pointer its expiry
        // left natively.
        let live = |id| {
            self.substrate
                .entities
                .get(id)
                .filter(|entity| entity.lifecycle.object_alive)
        };
        let Some(planter_entity) = live(planter) else {
            return;
        };
        if planter_entity.category != EntityCategory::Infantry {
            return;
        }
        let planter_house = planter_entity.owner();
        let Some(target) = target else {
            return;
        };
        let Some(carrier) = live(target) else {
            return;
        };
        if carrier.bomb.is_some() {
            return;
        }
        let position = carrier.position.clone();
        let world_z_leptons =
            crate::sim::combat::object_world_z_leptons(carrier, self.resolved_terrain.as_ref());
        let start_frame = self.session.binary_frame as i32;
        let bomb = Bomb {
            planter: Some(planter),
            planter_house,
            start_frame,
            end_frame: start_frame.wrapping_add(rules.combat_damage.ivan_timed_delay),
            seen_by: 0,
        };
        if let Some(carrier) = self.substrate.entities.get_mut(target) {
            carrier.bomb = Some(bomb);
        }
        self.bombs.carriers.insert(target);
        // 0x00438FCA: the carrier is seen from the second UpdateAll on.
        self.bombs.visibility_countdown = 1;
        log::info!(
            "BOMB {target} planted by {planter} ({}), fuse {start_frame}..{}",
            self.interner.resolve(planter_house),
            bomb.end_frame,
        );
        // 0x00438FD7..0x0043901D: BombAttachSound at the target, only for the
        // player who owns the planter (+0x21C).
        if let Some(sound_id) = rules.general.bomb_attach_sound.clone() {
            self.sound_events.push(SimSoundEvent::VocAt {
                sound_id,
                audible_to: Some([planter_house, planter_house]),
                rx: position.rx,
                ry: position.ry,
                sub_x: position.sub_x,
                sub_y: position.sub_y,
                world_z_leptons,
            });
        }
    }

    /// `BombClass::Defuse @ 0x004389B0`: the carrier's bomb is gone, silently.
    pub(crate) fn bomb_defuse(&mut self, carrier: u64) {
        if self.bombs.carriers.remove(&carrier)
            && let Some(entity) = self.substrate.entities.get_mut(carrier)
        {
            entity.bomb = None;
        }
    }

    /// `BombListClass::PointerGotInvalid @ 0x00439150`: bombs planted by the
    /// expired object lose their source.
    pub(crate) fn bomb_planter_expired(&mut self, expired: u64) {
        for &carrier in &self.bombs.carriers {
            if let Some(bomb) = self
                .substrate
                .entities
                .get_mut(carrier)
                .and_then(|entity| entity.bomb.as_mut())
                && bomb.planter == Some(expired)
            {
                bomb.planter = None;
            }
        }
    }

    /// `BombClass::Detonate @ 0x00438720` up to its blast: the record goes
    /// first; `None` when the carrier carries none, or silently when it is in
    /// limbo.
    pub(crate) fn take_bomb_blast(&mut self, carrier: u64, rules: &RuleSet) -> Option<BombBlast> {
        let entity = self.substrate.entities.get_mut(carrier)?;
        let bomb = entity.bomb.take()?;
        self.bombs.carriers.remove(&carrier);
        let entity = self.substrate.entities.get(carrier)?;
        if entity.lifecycle.in_limbo {
            log::info!("BOMB {carrier} went off in limbo, silently");
            return None;
        }
        log::info!(
            "BOMB {carrier} goes off at frame {} (source {:?})",
            self.session.binary_frame,
            bomb.planter,
        );
        let bridge_hut = entity.category == EntityCategory::Structure
            && rules
                .object(self.interner.resolve(entity.type_ref()))
                .is_some_and(|object| object.bridge_repair_hut);
        Some(BombBlast {
            carrier,
            source: bomb.planter.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
            bridge_hut,
        })
    }

    /// The fuse check in `TechnoClass::AI_Update` (`0x006FA6F5..0x006FA717`),
    /// in the carrier's own object visit.
    pub(crate) fn bomb_fuse_step(
        &mut self,
        carrier: u64,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(entity) = self.substrate.entities.get(carrier) else {
            return;
        };
        let expired = entity
            .bomb
            .is_some_and(|bomb| bomb.expired(self.session.binary_frame as i32));
        if !expired || entity.lifecycle.in_limbo {
            return;
        }
        if let Some(blast) = self.take_bomb_blast(carrier, rules) {
            self.bomb_blast(blast, rules, overlay_registry);
        }
    }

    /// The blast of `BombClass::Detonate` outside a damage transaction:
    /// Apply_area_damage (`0x004387A3`, its receivers committed in order), the
    /// explosion anim (`0x00438852`), then the hut's bridge (`0x0043896A`).
    pub(crate) fn bomb_blast(
        &mut self,
        blast: BombBlast,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(warhead_name) = rules.combat_damage.ivan_warhead.clone() else {
            return;
        };
        let Some(warhead) = rules.warhead(&warhead_name) else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(blast.carrier) else {
            return;
        };
        let position = entity.position.clone();
        let air_impact = crate::sim::combat::combat_aoe::air_impact_from_entity(
            entity,
            self.resolved_terrain.as_ref(),
        );
        let world_z_leptons =
            crate::sim::combat::object_world_z_leptons(entity, self.resolved_terrain.as_ref());
        let damage = rules.combat_damage.ivan_damage;
        let warhead_ref = self.interner.intern(&warhead_name);
        let aoe = crate::sim::combat::world_receiver::collect_area(
            self,
            rules,
            overlay_registry,
            (position.rx, position.ry),
            damage,
            warhead,
            (blast.source, None, warhead_ref),
            air_impact,
            i32::from(position.z),
        );
        self.commit_noncombat_aoe_receivers(rules, overlay_registry, &aoe.receivers);

        let mut explosions = Vec::new();
        let mut smudges = Vec::new();
        crate::sim::combat::emit_warhead_detonation_effects(
            warhead,
            damage,
            position.rx,
            position.ry,
            position.sub_x,
            position.sub_y,
            position.z,
            world_z_leptons,
            &mut self.interner,
            &mut explosions,
            &mut smudges,
        );
        for request in smudges {
            self.commit_smudge_request_inline(rules, overlay_registry, request);
        }
        for fx in &explosions {
            self.spawn_combat_explosion_anim(
                rules,
                fx.shp_name,
                fx.rx,
                fx.ry,
                fx.sub_x,
                fx.sub_y,
                fx.z,
            );
        }
        if blast.bridge_hut {
            crate::sim::world::bridge_orchestrator::dispatch_bridge_collapse_from_hut_with_overlay_registry(
                self,
                rules,
                (position.rx, position.ry),
                overlay_registry,
            );
        }
    }
}

#[cfg(test)]
#[path = "bomb_tests.rs"]
mod tests;
