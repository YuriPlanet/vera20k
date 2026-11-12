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
//! - A player's Detonate: the lone selected carrier of a human player's bomb
//!   answers Detonate over itself where `[CombatDamage]
//!   CanDetonateTimeBomb=`/`CanDetonateDeathBomb=` allow its kind, both no in
//!   retail ([`Simulation::bomb_detonate_action`]); the click sends DETONATE
//!   with the bomb's house, whose Execute sets it off
//!   ([`Simulation::execute_detonate_event`]).
//! - Who sees it (`BombListClass::UpdateAll @ 0x00438BF0`, every frame before
//!   the object AI): every 46th call, and the second call after an attach, the
//!   carrier's `+0x68` becomes "the local player owns the planter's house or a
//!   BombSight detector within range". It gates the clock over the carrier
//!   (`TechnoClass::DrawExtras`, BOMBCURS.SHP) and the Engineer's DisarmBomb
//!   cursor. Each bomb here keeps the answer for every house (`seen_by`); the
//!   app reads the local player's.
//! - The ticking loop (`BombTickingSound=`, same UpdateAll) plays at the
//!   carrier for every player while it is out of limbo. The bomb keeps the
//!   sound selected at attach; the app drives it under `ticking_sound_owner`.
//!
//! Ares builds (`docs/ares-ivan-bomb-abductor.md`): `PlantBomb` (Ares.dll
//! 10055AE0) remembers the planting weapon on its bomb, and every hook reads
//! that weapon's `IvanBomb.*` keys over the Rules ones: any Techno plants,
//! a CellSpread warhead plants on every Techno around its target
//! ([`Simulation::ares_ivan_bomb_detonation`]), the fuse, cues, damage,
//! warhead and hut bridge come from the weapon, a death bomb has no fuse, an
//! undetachable bomb refuses the Engineer, a sold carrier's bomb goes off,
//! a captured building keeps its bomb, a deploying or undeploying carrier
//! hands it on, a cloaking planter keeps its credit, and the clock is drawn
//! from `IvanBomb.Image`.
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
//!   writer, so only timed bombs exist in stock builds; Ares PlantBomb writes
//!   it.
//! - Campaign visibility: `HouseClass::IsHumanPlayer @ 0x0050B6F0` also admits
//!   a house with PlayerControl (`+0x1ED`) in a campaign; the app tests the
//!   local player's house only, as for the other gated sounds. Trigger: a
//!   bomb planted by, or seen by a detector of, a player-controlled allied
//!   campaign house. Effect: its clock stays hidden. Frequency: rare.

use crate::sim::world::FrameEffects;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
#[cfg(feature = "ares")]
use crate::rules::ares_weapon::AresIvanBomb;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::combat::world_receiver::{
    ReceiverRun, apply_area_damage, apply_area_damage_in_run,
};
use crate::sim::combat::{DeathEffects, UnderAttackEvent};
use crate::sim::components::DriveCoord;
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::intern::StringInterner;
use crate::sim::movement::ground_pose::{object_get_coords, object_location};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::FrameChannel;
use crate::sim::world::ObjectSubstrate;
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bomb {
    /// `+0x24`: the Infantry that planted it, until its pointer expires.
    planter: Option<u64>,
    /// `+0x28`: the planter's house at the attach.
    planter_house: InternedId,
    /// `+0x34`: the attach frame.
    start_frame: i32,
    /// `+0x38`: `start_frame + IvanTimedDelay`.
    end_frame: i32,
    /// `+0x50`: `BombClass`'s constructor (`0x0043860F`) and the inlined
    /// constructor in `BombListClass::Attach` (`0x00438EFD`) copy the current
    /// Rules `BombTickingSound`. `BombClass::Load @ 0x00438B40` retains it.
    #[serde(default)]
    ticking_sound: Option<String>,
    /// The houses whose player sees it, bit `n` for
    /// `Houses::order()[n]`: native's per-client BombVisible
    /// (the carrier's `+0x68`) for every house at once. Written only by
    /// `bomb_list_update`; saved like `+0x68`, but not hashed.
    #[serde(default)]
    seen_by: u64,
    /// Ares `WeaponTypeExt::BombExt`: the weapon whose `PlantBomb`
    /// (10055AE0) planted it, whose `IvanBomb.*` settings every bomb hook
    /// reads. Saved like the extension map; native's CRC omits it.
    #[cfg(feature = "ares")]
    #[serde(default)]
    weapon: Option<InternedId>,
    /// `+0x30`, the kind: a death bomb (1) has no fuse (`IsTimerExpired`)
    /// and shows the clock's last frame. Only Ares PlantBomb writes it;
    /// `BombClass::CRC` omits it.
    #[cfg(feature = "ares")]
    #[serde(default)]
    death_bomb: bool,
}

impl Hash for Bomb {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // `BombClass::CRC @ 0x00438A90` omits the retained sound. `seen_by`
        // is presentation on its own cadence, which a load restarts.
        self.planter.hash(state);
        self.planter_house.hash(state);
        self.start_frame.hash(state);
        self.end_frame.hash(state);
    }
}

/// An immutable boundary projection of the bomb owner. The carrier index and
/// carried records are read independently; neither is reconstructed from the
/// other. This value owns no gameplay state and does not sample visibility.
#[derive(Debug, Serialize)]
pub struct BombObservation {
    pub carriers: Vec<u64>,
    pub records: Vec<BombRecordObservation>,
}

#[derive(Debug, Serialize)]
pub struct BombRecordObservation {
    pub carrier_id: u64,
    pub planter_id: Option<u64>,
    pub planter_house: String,
    pub start_frame: i32,
    pub end_frame: i32,
}

/// The loop-handle owner of a carrier's ticking sound: native gives each bomb
/// its own VocHandle (`BombClass+0x3C`), apart from the carrier's own sounds,
/// so a tag bit keeps the key apart from every stable object id.
const TICKING_SOUND_TAG: u64 = 1 << 62;

pub const fn ticking_sound_owner(carrier: u64) -> u64 {
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

    /// `+0x28`: the planter's house at the attach.
    pub(crate) fn house(&self) -> InternedId {
        self.planter_house
    }

    /// `BombClass::GetType @ 0x004389F0`, the kind `+0x30`: a death bomb (1).
    fn is_death_bomb(&self) -> bool {
        #[cfg(feature = "ares")]
        return self.death_bomb;
        #[cfg(not(feature = "ares"))]
        false
    }

    /// `BombClass::IsTimerExpired @ 0x00438A70`: a timed bomb (kind 0)
    /// once `Frame > end`; a death bomb never.
    pub(crate) fn expired(&self, frame: i32) -> bool {
        #[cfg(feature = "ares")]
        if self.death_bomb {
            return false;
        }
        frame > self.end_frame
    }

    /// The planting weapon's Ares `IvanBomb.*` settings, or the
    /// constructor's for a bomb without one.
    #[cfg(feature = "ares")]
    fn ares_settings<'r>(&self, interner: &StringInterner, rules: &'r RuleSet) -> &'r AresIvanBomb {
        ivan_bomb_settings(interner, self.weapon, rules)
    }

    /// Ares BombClass_GetCurrentFrame (Ares.dll 100568C0, hook 0x438A00), in
    /// place of GetClockFrame, for an image of `frames` frames: frame 0 for
    /// fewer than two; a death bomb's last frame; else `frames / 2` stages of
    /// two frames, the stage `elapsed / (delay / stages)` capped at the last
    /// and its second frame in the blink half of `2 * rate`. A rate below 1
    /// counts `elapsed / (delay / frames)` over single frames, capped at the
    /// last stage's index. `None` where native divides by zero.
    #[cfg(feature = "ares")]
    fn ares_clock_frame(&self, frame: i32, frames: i32, delay: i32, rate: i32) -> Option<i32> {
        if frames < 2 {
            return Some(0);
        }
        if self.death_bomb {
            return Some(frames - 1);
        }
        let elapsed = frame.wrapping_sub(self.start_frame);
        let stages = frames / 2;
        let last = stages - 1;
        if rate < 1 {
            return Some(elapsed.checked_div(delay / (stages * 2))?.min(last));
        }
        let stage = elapsed.checked_div(delay / stages)?.min(last);
        let blink = frame.checked_rem(rate.wrapping_mul(2))? >= rate;
        Some(stage.wrapping_mul(2) + i32::from(blink))
    }

    /// `BombClass::GetClockFrame @ 0x00438A00`: the clock art's frame, the
    /// hand (`2 * elapsed / (IvanTimedDelay / 6)`) plus the blink in its global
    /// phase, at most 11. `None` where native divides by zero.
    pub fn clock_frame(&self, frame: i32, delay: i32, flicker_rate: i32) -> Option<i32> {
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
    /// The planter, or `RAD_NO_ATTACKER` once its pointer expired.
    source: u64,
    /// The carrier's Location, which the blast, its anim and the hut's
    /// bridge all use (`0x00438775..0x0043879F`).
    location: ProjectileCoord,
    bridge_hut: bool,
    /// The blast's damage and warhead (`0x00438799`, `0x00438843`):
    /// `IvanDamage`/`IvanWarhead`, or under Ares the planting weapon's
    /// `IvanBomb.Damage`/`IvanBomb.Warhead` over them
    /// (BombClass_Detonate1/2, Ares.dll 10056770/100567F0). `None` blasts
    /// nothing.
    damage: i32,
    warhead: Option<String>,
}

impl Simulation {
    /// Read retained BombClass inputs and BombList membership without running
    /// Attach, UpdateAll, a clock query, expiry or any other gameplay callback.
    /// Each independent collection stops at `limit`; callers request one more
    /// than their budget and reject excess instead of publishing truncation.
    pub fn bomb_observation(&self, limit: usize) -> BombObservation {
        BombObservation {
            carriers: self.bombs.carriers.iter().copied().take(limit).collect(),
            records: self
                .substrate
                .entities
                .iter_sorted()
                .filter_map(|(carrier_id, entity)| {
                    entity.bomb.as_ref().map(|bomb| BombRecordObservation {
                        carrier_id,
                        planter_id: bomb.planter,
                        planter_house: self.interner.resolve(bomb.planter_house).to_owned(),
                        start_frame: bomb.start_frame,
                        end_frame: bomb.end_frame,
                    })
                })
                .take(limit)
                .collect(),
        }
    }

    /// The live bombs' carriers (`BombListClass`'s list).
    pub fn bomb_carriers(&self) -> &BTreeSet<u64> {
        &self.bombs.carriers
    }

    /// The objects carrying a bomb, by stable id.
    fn carried_bombs(substrate: &ObjectSubstrate) -> impl Iterator<Item = u64> + '_ {
        substrate
            .entities
            .iter_sorted()
            .filter(|(_, entity)| entity.bomb.is_some())
            .map(|(id, _)| id)
    }

    /// Rebuild the carrier index from the carried records after a load.
    pub(crate) fn rebuild_bomb_carriers(substrate: &ObjectSubstrate, bombs: &mut BombList) {
        bombs.carriers = Self::carried_bombs(substrate).collect();
    }

    /// Whether the player owning `house` sees the carrier's bomb: its
    /// BombVisible (`+0x68`) as of the last refresh.
    pub fn bomb_seen_by(&self, carrier: u64, house: InternedId) -> bool {
        self.substrate
            .entities
            .get(carrier)
            .and_then(|entity| entity.bomb.as_ref())
            .is_some_and(|bomb| bomb.seen_by & house_bit(&self.houses.order(), house) != 0)
    }

    /// Where the carrier's ticking loop plays: its Location (`+0x9C`), while it
    /// carries a bomb out of limbo (`0x00438C84..0x00438CFE`); `None` stops it.
    pub fn bomb_ticking_coord(&self, carrier: u64) -> Option<AnimWorldCoord> {
        let entity = self.substrate.entities.get(carrier)?;
        (entity.bomb.is_some() && !entity.lifecycle.in_limbo())
            .then(|| Self::movement_sound_world(entity))
    }

    /// The cue captured by this bomb at attach. `BombListClass::UpdateAll`
    /// reads the retained `+0x50` at `0x00438C7C`, including after a load.
    pub fn bomb_ticking_sound(&self, carrier: u64) -> Option<&str> {
        self.substrate
            .entities
            .get(carrier)?
            .bomb
            .as_ref()?
            .ticking_sound
            .as_deref()
    }

    /// `BombListClass::UpdateAll @ 0x00438BF0`, once per frame from
    /// `LogicClass::PerTickUpdate` (`0x0055B4E1`), after ore growth and before
    /// the Teams and the object vector. Its first pass purges spent records,
    /// which VERA drops as they go off or are defused; its second drives the
    /// ticking loop, which the app derives from the carriers, and hard-stops
    /// it in limbo. Then comes BombVisible (`0x00438D04..0x00438E4C`).
    pub(crate) fn bomb_list_update(
        interner: &StringInterner,
        substrate: &mut ObjectSubstrate,
        houses: &Houses,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        bombs: &mut BombList,
        sound_events: &mut FrameChannel<SimSoundEvent>,
        rules: &RuleSet,
    ) {
        // 0x00438C7C..0x00438C99: a valid retained cue in limbo calls
        // VocHandle::StopAndClear405D40 before the visibility countdown.
        for &carrier in &bombs.carriers {
            let Some(entity) = substrate.entities.get(carrier) else {
                continue;
            };
            if entity.lifecycle.in_limbo()
                && entity
                    .bomb
                    .as_ref()
                    .is_some_and(|bomb| bomb.ticking_sound.is_some())
            {
                sound_events.emit(SimSoundEvent::AnimationStopped {
                    anim_id: ticking_sound_owner(carrier),
                    stop_sound_id: None,
                    world: Self::movement_sound_world(entity),
                });
            }
        }
        if bombs.visibility_countdown > 0 {
            bombs.visibility_countdown -= 1;
            return;
        }
        bombs.visibility_countdown = CLEARED_VISIBILITY_COUNTDOWN;
        debug_assert!(
            Self::carried_bombs(substrate).eq(bombs.carriers.iter().copied()),
            "the carrier index disagrees with the carried bombs"
        );
        if bombs.carriers.is_empty() {
            return;
        }
        let order = &houses.order();
        // The detectors (`BombList+0x1C`): `TechnoClass::Unlimbo` adds an
        // object whose type has a nonzero BombSight (`0x006F6D70`), `Limbo`
        // takes it out (`0x006F6B7A`). Coordinates are GetCoords (`vt+0x48`).
        let mut detectors = Vec::new();
        for entity in substrate.entities.values() {
            if entity.lifecycle.in_limbo() || !entity.lifecycle.object_alive() {
                continue;
            }
            let Some(object) = interner.object_type(entity.type_ref(), rules) else {
                continue;
            };
            if object.bomb_sight != 0 {
                detectors.push((
                    house_bit(order, entity.owner()),
                    object_get_coords(entity, resolved_terrain.as_ref()),
                    object.bomb_sight.wrapping_shl(8),
                ));
            }
        }
        let mut refreshed = Vec::with_capacity(bombs.carriers.len());
        for &carrier in &bombs.carriers {
            let Some(entity) = substrate.entities.get(carrier) else {
                continue;
            };
            let Some(bomb) = entity.bomb.as_ref() else {
                continue;
            };
            let center = object_get_coords(entity, resolved_terrain.as_ref());
            let mut seen_by = house_bit(order, bomb.planter_house);
            for &(house, detector, range) in &detectors {
                if seen_by & house == 0 && within_bomb_sight(detector, center, range) {
                    seen_by |= house;
                }
            }
            refreshed.push((carrier, seen_by));
        }
        for (carrier, seen_by) in refreshed {
            if let Some(bomb) = substrate
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
    ///
    /// Ares builds reach it only through `PlantBomb` (Ares.dll 10055AE0,
    /// [`Self::ares_ivan_bomb_detonation`]), with `weapon` the planting
    /// weapon: BombListClass_Plant_AllTechnos (10056A50, hook 0x438E86)
    /// admits any Techno planter, BombListClass_Plant_AttachSound (10056AA0,
    /// hook 0x438FD7) skips the stock cue, and PlantBomb then rewrites the
    /// fuse and ticking cue from the weapon's `IvanBomb.Delay` and
    /// `IvanBomb.TickingSound`, makes it a death bomb by `IvanBomb.DeathBomb`
    /// (`DeathBombOnAllies` for a victim the planter's house counts an ally,
    /// `HouseClass::IsAlliedWith` inlined) and plays `IvanBomb.AttachSound`
    /// (else BombAttachSound) under the same player gate.
    pub(crate) fn bomb_attach(
        &mut self,
        planter: u64,
        target: Option<u64>,
        weapon: Option<InternedId>,
        rules: &RuleSet,
    ) {
        #[cfg(not(feature = "ares"))]
        let _ = weapon;
        // An object UnInit has passed reads as the null pointer its expiry
        // left natively.
        let substrate = &self.substrate;
        let live = |id| {
            substrate
                .entities
                .get(id)
                .filter(|entity| entity.lifecycle.object_alive())
        };
        let Some(planter_entity) = live(planter) else {
            return;
        };
        #[cfg(not(feature = "ares"))]
        if planter_entity.category() != EntityCategory::Infantry {
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
        let position = carrier.position;
        let world_z_leptons = crate::sim::movement::ground_pose::object_world_z_leptons(
            carrier,
            self.resolved_terrain.as_ref(),
        );
        let start_frame = self.session.binary_frame() as i32;
        #[cfg(feature = "ares")]
        let settings = ivan_bomb_settings(&self.interner, weapon, rules);
        #[cfg(feature = "ares")]
        let death_bomb =
            if self
                .house_alliances
                .is_allied(&self.interner, planter_house, carrier.owner())
            {
                settings.death_bomb_on_allies
            } else {
                settings.death_bomb
            };
        // `BombListClass::Attach`'s inlined constructor (`0x00438EFD`):
        // capture the bound cue only after the attach gates admit the bomb.
        // Keep presentation names outside the gameplay interner so a sound
        // change cannot shift IDs allocated for later gameplay state.
        #[cfg(not(feature = "ares"))]
        let (ticking_sound, delay, attach_sound) = (
            rules.general.bomb_ticking_sound.clone(),
            rules.combat_damage.ivan_timed_delay,
            rules.general.bomb_attach_sound.clone(),
        );
        #[cfg(feature = "ares")]
        let (ticking_sound, delay, attach_sound) = (
            settings
                .ticking_sound
                .clone()
                .unwrap_or_else(|| rules.general.bomb_ticking_sound.clone()),
            settings
                .delay
                .unwrap_or(rules.combat_damage.ivan_timed_delay),
            settings
                .attach_sound
                .clone()
                .unwrap_or_else(|| rules.general.bomb_attach_sound.clone()),
        );
        let end_frame = start_frame.wrapping_add(delay);
        let bomb = Bomb {
            planter: Some(planter),
            planter_house,
            start_frame,
            end_frame,
            ticking_sound,
            seen_by: 0,
            #[cfg(feature = "ares")]
            weapon,
            #[cfg(feature = "ares")]
            death_bomb,
        };
        // Native Attach438E70 constructs a fresh BombClass/VocHandle. The
        // app reuses the carrier key, so retire any previous bomb's binding
        // before installing the new one, including a silent replacement.
        self.sound_events.emit(SimSoundEvent::ObjectSoundDetached {
            owner: ticking_sound_owner(target),
        });
        if let Some(carrier) = self.substrate.entities.get_mut(target) {
            carrier.bomb = Some(Box::new(bomb));
        }
        self.bombs.carriers.insert(target);
        // 0x00438FCA: the carrier is seen from the second UpdateAll on.
        self.bombs.visibility_countdown = 1;
        log::info!(
            "BOMB {target} planted by {planter} ({}), fuse {start_frame}..{}",
            self.interner.resolve(planter_house),
            end_frame,
        );
        // 0x00438FD7..0x0043901D: BombAttachSound at the target, only for the
        // player who owns the planter (+0x21C).
        if let Some(sound_id) = attach_sound {
            self.sound_events.emit(SimSoundEvent::VocAt {
                sound_id,
                audible_to: Some([planter_house, planter_house]),
                rx: position.rx(),
                ry: position.ry(),
                sub_x: position.sub_x(),
                sub_y: position.sub_y(),
                world_z_leptons,
            });
        }
    }

    /// Ares BulletClass_DetonateAt_IvanBombs (Ares.dll 10056AF0, hook
    /// 0x46934D, resuming past the arm at 0x469AA4) in place of the stock
    /// IvanBomb arm: with a live bullet owner and an installed weapon (else a
    /// logged warning and nothing), a warhead `CellSpread` under 0.5 plants
    /// on a Techno target alone; a wider one plants on every Techno the
    /// cell-spread walk (CellSpreadIterator 10056550, `(int)CellSpread`
    /// rings around the cell of the target's GetCoords, or the bullet's own
    /// when it has none) finds, except the owner and those already carrying
    /// one. Each plant is `PlantBomb` ([`Self::bomb_attach`]).
    #[cfg(feature = "ares")]
    pub(crate) fn ares_ivan_bomb_detonation(
        &mut self,
        detonation: &crate::sim::projectile::ProjectileDetonation,
        rules: &RuleSet,
    ) {
        use crate::sim::projectile::ProjectileTarget;
        let owner = detonation.source_id;
        if !self.substrate.entities.contains(owner) {
            return;
        }
        let Some(weapon) = detonation.payload.installed_weapon() else {
            log::warn!("IvanBomb bullet without attached WeaponType.");
            return;
        };
        let Some(warhead) = rules.warhead(self.interner.resolve(detonation.payload.warhead)) else {
            return;
        };
        if warhead.cell_spread_f64 < 0.5 {
            if let ProjectileTarget::Entity(target) = detonation.target {
                self.bomb_attach(owner, Some(target), Some(weapon), rules);
            }
            return;
        }
        // GetCoords (vt+0x48) of the target, or of the bullet itself; VERA
        // reads the bullet's detonation point for that one.
        let target = match detonation.target {
            ProjectileTarget::Entity(id) | ProjectileTarget::Terrain(id) => {
                Some(crate::sim::combat::TargetKind::Entity(id))
            }
            ProjectileTarget::Cell { rx, ry } => Some(crate::sim::combat::TargetKind::Cell(rx, ry)),
            ProjectileTarget::None | ProjectileTarget::DummyCell => None,
        };
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        let center = target
            .and_then(|target| {
                crate::sim::movement::ground_pose::target_get_coords(target, self, cells.as_ref())
            })
            .unwrap_or(crate::sim::components::DriveCoord {
                x: detonation.impact.x,
                y: detonation.impact.y,
                z: detonation.impact.z,
            });
        let mut victims = Vec::new();
        self.ares_cell_spread_technos(
            crate::sim::movement::coord_cell(center),
            warhead.cell_spread_f64 as i32 as u32,
            |id| victims.push(id),
        );
        for victim in victims {
            // The walk's own test; PlantBomb repeats the bomb one.
            if victim != owner {
                self.bomb_attach(owner, Some(victim), Some(weapon), rules);
            }
        }
    }

    /// Whether an Engineer may defuse the carrier's bomb: always in stock;
    /// under Ares unless its weapon says `IvanBomb.Detachable=no`
    /// (TechnoClass_CanFire_IvanBomb 10056C60, hook 0x6FCBAD;
    /// InfantryClass_GetActionOnObject2 10056C00, hook 0x51E488).
    pub fn bomb_detachable(&self, carrier: u64, rules: &RuleSet) -> bool {
        #[cfg(feature = "ares")]
        if let Some(bomb) = self
            .substrate
            .entities
            .get(carrier)
            .and_then(|entity| entity.bomb.as_ref())
        {
            return bomb.ares_settings(&self.interner, rules).detachable;
        }
        let _ = (carrier, rules);
        true
    }

    /// The clock frame over the carrier for an image of `frames` frames:
    /// `BombClass::GetClockFrame @ 0x00438A00` ([`Bomb::clock_frame`]), or
    /// under Ares BombClass_GetCurrentFrame (100568C0) with the planting
    /// weapon's `IvanBomb.Delay`/`IvanBomb.FlickerRate` over the Rules ones.
    /// `None` without a bomb or where native divides by zero.
    pub fn bomb_clock_frame(&self, carrier: u64, rules: &RuleSet, frames: usize) -> Option<i32> {
        let bomb = self.substrate.entities.get(carrier)?.bomb.as_ref()?;
        let frame = self.session.binary_frame() as i32;
        let (delay, rate) = (
            rules.combat_damage.ivan_timed_delay,
            rules.combat_damage.ivan_icon_flicker_rate,
        );
        #[cfg(feature = "ares")]
        {
            let settings = bomb.ares_settings(&self.interner, rules);
            // TechnoClass_DrawExtras reads the SHP header's frame count, a
            // short.
            let frames = i32::from(i16::try_from(frames).unwrap_or(i16::MAX));
            bomb.ares_clock_frame(
                frame,
                frames,
                settings.delay.unwrap_or(delay),
                settings.flicker_rate.unwrap_or(rate),
            )
        }
        #[cfg(not(feature = "ares"))]
        {
            let _ = frames;
            bomb.clock_frame(frame, delay, rate)
        }
    }

    /// The carrier bomb's Ares `IvanBomb.Image` candidates, in pass order
    /// (TechnoClass_DrawExtras_IvanBombImage 10056CC0, hook 0x6F523C, draws
    /// the last that loads in place of BOMBCURS.SHP).
    #[cfg(feature = "ares")]
    pub fn bomb_images<'r>(&self, carrier: u64, rules: &'r RuleSet) -> &'r [String] {
        self.substrate
            .entities
            .get(carrier)
            .and_then(|entity| entity.bomb.as_ref())
            .map_or(&[], |bomb| &bomb.ares_settings(&self.interner, rules).image)
    }

    /// Ares `WeaponTypeExt::OnSell` (10055A80) from the sale hooks
    /// BuildingClass_Sell_DetonateNoBuildup (10056AD0, hook 0x4471D5),
    /// BuildingClass_Mi_Selling_DetonatePostBuildup (10056AB0, hook 0x44A1FF)
    /// and FootClass_Sell_Detonate (10056AD0, hook 0x4D9F7B): the sold
    /// carrier's bomb goes off ([`Self::bomb_detonate`]) when its weapon has
    /// `IvanBomb.DetonateOnSell` (the default). Returns whether the carrier
    /// is still alive afterwards.
    #[cfg(feature = "ares")]
    pub(crate) fn bomb_detonate_on_sell(
        &mut self,
        carrier: u64,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let sells = self
            .substrate
            .entities
            .get(carrier)
            .and_then(|entity| entity.bomb.as_ref())
            .is_some_and(|bomb| bomb.ares_settings(&self.interner, rules).detonate_on_sell);
        if sells {
            self.bomb_detonate(carrier, rules, overlay_registry, frame_effects);
        }
        self.substrate
            .entities
            .get(carrier)
            .is_some_and(|entity| entity.lifecycle.object_alive())
    }

    /// Ares TransferIvanBomb, inlined in 10045A80 (UnitClass_Deploy_Transfer,
    /// hook 0x739956; BuildingClass_Mi_Selling_Transfer, hook 0x44A03C): a
    /// deploying or undeploying object's bomb moves to the object it becomes
    /// when that one carries none, with its BombVisible (`+0x68`). The
    /// record keeps its fuse; the app restarts its loop on the new carrier.
    #[cfg(feature = "ares")]
    pub(crate) fn transfer_ivan_bomb(&mut self, from: u64, to: u64) {
        if self
            .substrate
            .entities
            .get(to)
            .is_none_or(|entity| entity.bomb.is_some())
        {
            return;
        }
        let Some(bomb) = self
            .substrate
            .entities
            .get_mut(from)
            .and_then(|entity| entity.bomb.take())
        else {
            return;
        };
        self.bombs.carriers.remove(&from);
        self.sound_events.emit(SimSoundEvent::ObjectSoundDetached {
            owner: ticking_sound_owner(from),
        });
        if let Some(entity) = self.substrate.entities.get_mut(to) {
            entity.bomb = Some(bomb);
        }
        self.bombs.carriers.insert(to);
        log::info!("BOMB moved from {from} to {to}");
    }

    /// `BombClass::Defuse @ 0x004389B0`: remove the bomb and detach its cue
    /// through VocHandle405FD0 (`0x004389D6`), leaving the current clip's tail.
    pub(crate) fn bomb_defuse(
        substrate: &mut ObjectSubstrate,
        bombs: &mut BombList,
        sound_events: &mut FrameChannel<SimSoundEvent>,
        carrier: u64,
    ) {
        if bombs.carriers.remove(&carrier)
            && let Some(entity) = substrate.entities.get_mut(carrier)
        {
            entity.bomb = None;
            sound_events.emit(SimSoundEvent::ObjectSoundDetached {
                owner: ticking_sound_owner(carrier),
            });
        }
    }

    /// `BombListClass::PointerGotInvalid @ 0x00439150`: bombs planted by the
    /// expired object lose their source.
    pub(crate) fn bomb_planter_expired(
        substrate: &mut ObjectSubstrate,
        bombs: &BombList,
        expired: u64,
    ) {
        for &carrier in &bombs.carriers {
            if let Some(bomb) = substrate
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
    fn take_bomb_blast(
        interner: &StringInterner,
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        resolved_terrain: &Option<ResolvedTerrainGrid>,
        bombs: &mut BombList,
        sound_events: &mut FrameChannel<SimSoundEvent>,
        carrier: u64,
        rules: &RuleSet,
    ) -> Option<BombBlast> {
        let entity = substrate.entities.get_mut(carrier)?;
        let bomb = entity.bomb.take()?;
        bombs.carriers.remove(&carrier);
        let entity = substrate.entities.get(carrier)?;
        if entity.lifecycle.in_limbo() {
            log::info!("BOMB {carrier} went off in limbo, silently");
            // The limbo branch joins Detonate's VocHandle405FD0 tail
            // (0x00438759 -> 0x00438989..0x00438995).
            sound_events.emit(SimSoundEvent::ObjectSoundDetached {
                owner: ticking_sound_owner(carrier),
            });
            return None;
        }
        log::info!(
            "BOMB {carrier} goes off at frame {} (source {:?})",
            session.binary_frame(),
            bomb.planter,
        );
        let bridge_hut = entity.category() == EntityCategory::Structure
            && interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| object.bridge_repair_hut);
        // Ares BombClass_Detonate1/2/3 (10056770, 100567F0, 10056870; hooks
        // 0x438799, 0x438843, 0x438879): the weapon's damage and warhead
        // over the Rules ones, and `IvanBomb.DestroysBridges=no` skips the
        // hut's bridge (to 0x438989).
        #[cfg(feature = "ares")]
        let (damage, warhead, bridge_hut) = {
            let settings = bomb.ares_settings(interner, rules);
            (
                settings.damage.unwrap_or(rules.combat_damage.ivan_damage),
                settings
                    .warhead
                    .clone()
                    .unwrap_or_else(|| rules.combat_damage.ivan_warhead.clone()),
                bridge_hut && settings.destroys_bridges,
            )
        };
        #[cfg(not(feature = "ares"))]
        let (damage, warhead) = (
            rules.combat_damage.ivan_damage,
            rules.combat_damage.ivan_warhead.clone(),
        );
        let location = object_location(entity, resolved_terrain.as_ref());
        Some(BombBlast {
            source: bomb.planter.unwrap_or(crate::sim::combat::RAD_NO_ATTACKER),
            location: ProjectileCoord::new(location.x, location.y, location.z),
            bridge_hut,
            damage,
            warhead,
        })
    }

    /// The fuse check in `TechnoClass::AI_Update` (`0x006FA6F5..0x006FA717`),
    /// in the carrier's own object visit. The blast runs outside any damage
    /// transaction ([`apply_area_damage`]). Returns whether a bridge changed.
    pub(crate) fn bomb_fuse_step(
        &mut self,
        carrier: u64,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(carrier) else {
            return false;
        };
        let expired = entity
            .bomb
            .as_ref()
            .is_some_and(|bomb| bomb.expired(self.session.binary_frame() as i32));
        if !expired || entity.lifecycle.in_limbo() {
            return false;
        }
        self.bomb_detonate(carrier, rules, overlay_registry, frame_effects)
    }

    /// The Detonate rung of `TechnoClass::What_Action_OnObject`
    /// (`0x006FFF8D..0x00700003`), past the lone-selection (`[0xA8ECC8] == 1`)
    /// and self tests its callers make: `carrier` holds a bomb of a human
    /// player's house (`HouseClass::IsHumanPlayer @ 0x0050B6F0`), and Rules
    /// lets that kind go off on a click: a death bomb `CanDetonateDeathBomb=`,
    /// a timed one `CanDetonateTimeBomb=`. Retail sets both to no.
    ///
    /// Ares 3.0 answers it at the entry of What_Action instead
    /// (`TechnoClass_GetActionOnObject_IvanBombsA @ 10056D30`, hook
    /// `0x006FFEC0`, ahead of the berserk and warp exits; `IvanBombsB @
    /// 10056D70`, hook `0x006FFF9E`, skips the stock rung):
    /// `TechnoExt::CanDetonateAttachedBomb @ 100433A0` reads the planting
    /// weapon's `IvanBomb.CanDetonateTimeBomb`/`.CanDetonateDeathBomb` over the
    /// Rules keys.
    pub fn bomb_detonate_action(&self, carrier: u64, rules: &RuleSet) -> bool {
        let Some(bomb) = self
            .substrate
            .entities
            .get(carrier)
            .and_then(|entity| entity.bomb.as_ref())
        else {
            return false;
        };
        let combat = &rules.combat_damage;
        if !self.house_is_human_player(bomb.planter_house) {
            return false;
        }
        #[cfg(feature = "ares")]
        return bomb.ares_settings(&self.interner, rules).can_detonate(
            bomb.is_death_bomb(),
            combat.can_detonate_time_bomb,
            combat.can_detonate_death_bomb,
        );
        #[cfg(not(feature = "ares"))]
        if bomb.is_death_bomb() {
            combat.can_detonate_death_bomb
        } else {
            combat.can_detonate_time_bomb
        }
    }

    /// The DETONATE arm of `EventClass::Execute @ 0x004C6CB0`
    /// (`0x004C7823..0x004C7850`): the event's object as a Techno
    /// (`TargetClass::As_Techno`) drops its planning tokens
    /// (`TechnoClass::ClearPlanningTokens`, [`Self::planning_detach_actor`]),
    /// then its bomb, if any, goes off ([`Self::bomb_detonate`]). The event's
    /// house and the carrier's owner are not asked. Answers whether a bomb
    /// went off.
    pub(crate) fn execute_detonate_event(
        &mut self,
        carrier: u64,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        // Every entity in the store is a Techno.
        let Some(entity) = self.substrate.entities.get(carrier) else {
            return false;
        };
        let armed = entity.bomb.is_some();
        self.planning_detach_actor(carrier);
        if !armed {
            return false;
        }
        self.bomb_detonate(carrier, rules, overlay_registry, frame_effects);
        true
    }

    /// `BombClass::Detonate @ 0x00438720` outside any damage transaction
    /// ([`apply_area_damage`]): the fuse, a player's DETONATE event and
    /// Ares's sale detonation. Returns whether a bridge changed.
    pub(crate) fn bomb_detonate(
        &mut self,
        carrier: u64,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(blast) = Self::take_bomb_blast(
            &self.interner,
            &self.session,
            &mut self.substrate,
            &self.resolved_terrain,
            &mut self.bombs,
            &mut self.sound_events,
            carrier,
            rules,
        ) else {
            return false;
        };
        let Some(warhead) = blast.warhead(rules) else {
            self.sound_events.emit(SimSoundEvent::ObjectSoundDetached {
                owner: ticking_sound_owner(carrier),
            });
            return false;
        };
        let origin = blast.origin(&mut self.interner, warhead);
        let receipt = apply_area_damage(
            self,
            rules,
            overlay_registry,
            blast.location,
            blast.damage,
            warhead,
            true,
            origin,
            frame_effects,
        );
        receipt.bridge_state_changed
            | self.bomb_blast_tail(
                &blast,
                carrier,
                warhead,
                rules,
                overlay_registry,
                receipt.crit_active,
                frame_effects,
            )
    }

    /// A dying carrier's bomb (`TechnoClass::ReceiveDamage` `0x0070267F`,
    /// after its death weapon): the blast's receivers nest in the death's
    /// transaction ([`apply_area_damage_in_run`]), and their effects and pings
    /// go back to it. `None` when the carrier sets off nothing.
    pub(crate) fn bomb_detonate_in_run(
        &mut self,
        run: &mut ReceiverRun,
        carrier: u64,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
    ) -> Option<(DeathEffects, Vec<UnderAttackEvent>)> {
        let blast = Self::take_bomb_blast(
            &self.interner,
            &self.session,
            &mut self.substrate,
            &self.resolved_terrain,
            &mut self.bombs,
            &mut self.sound_events,
            carrier,
            rules,
        )?;
        let Some(warhead) = blast.warhead(rules) else {
            self.sound_events.emit(SimSoundEvent::ObjectSoundDetached {
                owner: ticking_sound_owner(carrier),
            });
            return None;
        };
        let origin = blast.origin(&mut self.interner, warhead);
        let (mut effects, pings, crit_active, _area_result) = apply_area_damage_in_run(
            self,
            run,
            rules,
            overlay_registry,
            blast.location,
            blast.damage,
            warhead,
            true,
            origin,
        );
        effects.bridge_state_changed |= self.bomb_blast_tail(
            &blast,
            carrier,
            warhead,
            rules,
            overlay_registry,
            crit_active,
            run.effects(),
        );
        Some((effects, pings))
    }

    /// `BombClass::Detonate` after its Apply_area_damage (`0x004387A3`): the
    /// IvanWarhead's explosion anim at the carrier's Location, chosen by that
    /// cell's current land (`0x0043880A..0x00438852`), then a
    /// `BridgeRepairHut=` carrier's bridge (`0x0043896A`/`0x00438982`), then
    /// detach the cue (`0x00438995`, VocHandle405FD0).
    /// Returns whether a bridge changed.
    #[allow(clippy::too_many_arguments)]
    fn bomb_blast_tail(
        &mut self,
        blast: &BombBlast,
        carrier: u64,
        warhead: &WarheadType,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        crit_active: bool,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let land = crate::sim::combat::detonation_anim::land_at(self, blast.location);
        if let Some(effect) = crate::sim::combat::detonation_anim::effect(
            self,
            rules,
            warhead,
            blast.damage,
            land,
            blast.location,
            blast.location,
            crit_active,
        ) {
            crate::sim::world::damage_consequences::admit_explosion_effect(self, rules, effect);
        }
        let bridge_changed = if blast.bridge_hut {
            let (rx, ry, ..) = crate::sim::combat::projectile_impact_cell(blast.location);
            crate::sim::world::bridge_orchestrator::dispatch_bridge_collapse_from_hut_with_overlay_registry(
                self,
                rules,
                (rx, ry),
                overlay_registry,
                frame_effects,
            )
        } else {
            false
        };
        self.sound_events.emit(SimSoundEvent::ObjectSoundDetached {
            owner: ticking_sound_owner(carrier),
        });
        bridge_changed
    }
}

/// The Ares `IvanBomb.*` settings of `weapon`, or the constructor's
/// (`WeaponTypeExt::BombExt` of a bomb no weapon planted).
#[cfg(feature = "ares")]
fn ivan_bomb_settings<'r>(
    interner: &StringInterner,
    weapon: Option<InternedId>,
    rules: &'r RuleSet,
) -> &'r AresIvanBomb {
    static CONSTRUCTED: AresIvanBomb = AresIvanBomb::CONSTRUCTED;
    weapon
        .and_then(|weapon| rules.weapon(interner.resolve(weapon)))
        .map_or(&CONSTRUCTED, |weapon| &weapon.ares_ivan_bomb)
}

impl BombBlast {
    /// The blast's warhead (`[CombatDamage] IvanWarhead=`, `Rules+0xFC8`, or
    /// Ares's `IvanBomb.Warhead`); without one a bomb's record still goes but
    /// nothing blasts.
    fn warhead<'r>(&self, rules: &'r RuleSet) -> Option<&'r WarheadType> {
        self.warhead.as_deref().and_then(|name| rules.warhead(name))
    }

    /// Apply_area_damage's source: the planter and no house.
    fn origin(
        &self,
        interner: &mut crate::sim::intern::StringInterner,
        warhead: &WarheadType,
    ) -> (u64, Option<InternedId>, InternedId) {
        (self.source, None, interner.intern(&warhead.id))
    }
}

#[cfg(test)]
#[path = "bomb_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "bomb_detonate_tests.rs"]
pub(crate) mod detonate_tests;
