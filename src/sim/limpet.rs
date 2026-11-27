//! Tiberian Sun limpets (docs/ts-limpets.md), ported from Firestorm 2.0
//! GAME.EXE; gamemd has none of this code. A shot whose warhead has a
//! nonzero `LimpetFactor=` attaches its firer's house to the target instead
//! of launching a bullet, and the firer is deleted. A limpeted vehicle moves
//! slower and the houses that limpeted it see what it sees, until a heal or a
//! service depot's repair takes the limpets off.
//!
//! Firestorm 2.0 addresses below are of that executable, not of gamemd.
//! Nothing here runs unless a rules or map INI sets `LimpetFactor=`, so a
//! retail Yuri's Revenge game draws no RNG and hashes as before.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation, UninitContext};
use crate::util::native_x87::{MaskedX87Chop53 as X, NativeF32Bits, NativeF64Bits};

/// The limpets on one Techno: Firestorm 2.0 TechnoClass+0x214 (LimpetMask)
/// and +0x218 (the speed factor). Nothing until the first attach, and
/// nothing again once a heal or a depot clears them.
///
/// Firestorm keeps one bit per house, `1 << (House->ID & 31)`, which aliases
/// houses past 32; VERA keeps the houses themselves, so every house a game
/// can hold has its own place. The factor is the last attacher's: each
/// attach overwrites it (`0x00630261`). Firestorm puts the mask in the sync
/// CRC (`Object_CRC 0x00638F4E`) and not the factor; both are hashed here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct Limpets(Option<Box<Attached>>);

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct Attached {
    /// The limpeting houses; never empty.
    houses: BTreeSet<InternedId>,
    /// `(float)((100 - LimpetFactor) * 0.01)`, [`speed_factor`].
    speed_factor: NativeF32Bits,
}

impl Limpets {
    /// Whether any house limpeted the object: Firestorm's `LimpetMask != 0`.
    pub(crate) fn is_attached(&self) -> bool {
        self.0.is_some()
    }

    /// Whether `house`'s bit is set.
    pub(crate) fn attached_by(&self, house: InternedId) -> bool {
        self.0
            .as_ref()
            .is_some_and(|attached| attached.houses.contains(&house))
    }

    /// `viewers` joined by each of `clients` whose house limpeted the
    /// object, still ascending ([`sight_viewers`]).
    fn with_viewers(
        &self,
        clients: &[InternedId],
        viewers: Arc<[InternedId]>,
    ) -> Arc<[InternedId]> {
        let Some(attached) = &self.0 else {
            return viewers;
        };
        let mut joined = viewers.to_vec();
        joined.extend(
            clients
                .iter()
                .copied()
                .filter(|client| attached.houses.contains(client) && !viewers.contains(client)),
        );
        if joined.len() == viewers.len() {
            return viewers;
        }
        joined.sort_unstable();
        joined.dedup();
        joined.into()
    }

    /// `TechnoClass::Fire_At`'s limpet arm on the target, Firestorm 2.0
    /// `0x00630213..0x00630261`: `house`'s bit joins the mask and the factor
    /// becomes `limpet_factor`'s. The caller has tested the bit
    /// (`0x0063022B`).
    fn attach(&mut self, house: InternedId, limpet_factor: i32) {
        let speed_factor = speed_factor(limpet_factor);
        match &mut self.0 {
            Some(attached) => {
                attached.houses.insert(house);
                attached.speed_factor = speed_factor;
            }
            None => {
                self.0 = Some(Box::new(Attached {
                    houses: BTreeSet::from([house]),
                    speed_factor,
                }));
            }
        }
    }

    /// Mask and factor zeroed: a heal (`TechnoClass::Take_Damage`
    /// `0x006326EE`) or a depot repair (`TechnoClass::Receive_Message`
    /// `0x0062ABE5`). Factor 0.0 is never read while the mask is zero.
    fn clear(&mut self) {
        self.0 = None;
    }
}

/// Whether `house` limpeted `entity`, so its map sees through it: the sight
/// gates of `world::shroud_refresh` admit it beside the owner's reveal
/// houses ([`sight_viewers`]). The speed byte
/// ([`FootSpeedState::limpeted`](crate::sim::components::FootSpeedState::limpeted)),
/// which every writer of the limpets keeps for every category, spares an
/// object without limpets the read of its seldom part.
pub(crate) fn seen_through_by(entity: &GameEntity, house: InternedId) -> bool {
    entity.foot_speed.limpeted() && entity.seldom.limpets.attached_by(house)
}

/// `viewers`, the client maps a reveal of `entity` writes (ascending), joined
/// by each of `clients` whose house limpeted it, still ascending. Firestorm
/// 2.0 `TechnoClass::Look @ 0x00638160` reveals for PlayerPtr in place of
/// the owner when PlayerPtr's bit is set (`0x0063829A..0x006382B8`), so a
/// limpeting player sees through the object and its allies do not.
pub(crate) fn sight_viewers(
    entity: &GameEntity,
    clients: &[InternedId],
    viewers: Arc<[InternedId]>,
) -> Arc<[InternedId]> {
    if !entity.foot_speed.limpeted() {
        return viewers;
    }
    entity.seldom.limpets.with_viewers(clients, viewers)
}

/// The limpets a deploy or an undeploy carries from `from` to the object it
/// makes, `to`, before that object's Unlimbo: Firestorm 2.0 `UnitClass::
/// Try_To_Deploy` copies mask and factor to the new building
/// (`0x00650D9E..0x00650DB6`), `BuildingClass::Mission_Deconstruction` back
/// to the new unit (`0x004309E1..0x004309F3`). Firestorm copies them just
/// after its Unlimbo (vt+0xC8), whose Look then reveals for the owner alone;
/// the limpeting player sees through the new object from its next Look.
/// VERA copies them first, so its Unlimbo reveal already includes that
/// player, and the sight it gives is the sight it later takes back.
pub(crate) fn carry(from: &GameEntity, to: &mut GameEntity) {
    to.seldom.limpets = from.seldom.limpets.clone();
    to.foot_speed.sync_limpets(&to.seldom.limpets);
}

/// The limpet's speed factor, Firestorm 2.0 `0x00630241..0x00630261`:
/// `100 - LimpetFactor` as a 32-bit subtraction, zero-extended into a qword
/// for `FILD`, times the double 0.01 (`0x006CBD78`), stored as a float. 35
/// gives `0x3F266666` (0.64999998), and a negative factor speeds the vehicle
/// up as in Firestorm.
///
/// VERA deviation: a `LimpetFactor=` above 100 gives 0 and stops the vehicle.
/// Firestorm wraps the subtraction to about 4.29e9 there and drives the
/// vehicle at an absurd speed; outside data must not break the game.
pub(crate) fn speed_factor(limpet_factor: i32) -> NativeF32Bits {
    if limpet_factor > 100 {
        return NativeF32Bits::POSITIVE_ZERO;
    }
    // At most 100 - i32::MIN, which a double holds exactly, as FILD does.
    let remaining = (100 - i64::from(limpet_factor)) as f64;
    X::store_f32_masked_chop(X::mul(
        X::load_f64(NativeF64Bits::from_bits(remaining.to_bits())),
        X::load_f64(NativeF64Bits::from_bits(0.01_f64.to_bits())),
    ))
}

/// The max speed a Techno's movers read (gamemd vt+0x38C, Firestorm 2.0
/// vt+0x2A8) for a `type_speed` in leptons per frame (TechnoType+0x678):
/// Firestorm's `UnitClass::Get_Max_Speed @ 0x00659710` truncates a limpeted
/// Unit's speed by its factor (`FILD`, `FMUL dword`, `__ftol`,
/// `0x00659719..0x00659731`). Only UnitClass overrides the slot, so
/// Infantry, Aircraft and Buildings keep the type speed. Speed 20 at
/// 0.64999998 is 12, not 13.
///
/// Its readers are gamemd's vt+0x38C callers, which match Firestorm's:
/// `FootClass::GetCurrentSpeed` (`0x004DB1C3`; Firestorm `0x004A5D57`) before
/// the house and crate stage, the Drive track brake (`0x004B10AC`; Firestorm
/// `0x0047E847`) and the base-defense threat score (`0x004D985F`,
/// `0x004D986E`; Firestorm `0x004A4C84`, `0x004A4C93`).
///
/// The limpets live in the object's seldom part; the byte
/// [`FootSpeedState::limpeted`] spares every other speed query that read.
///
/// [`FootSpeedState::limpeted`]: crate::sim::components::FootSpeedState::limpeted
pub(crate) fn max_speed(entity: &GameEntity, type_speed: i32) -> i32 {
    if !entity.foot_speed.limpeted() {
        return type_speed;
    }
    match &entity.seldom.limpets.0 {
        Some(attached) if entity.category() == crate::map::entities::EntityCategory::Unit => {
            X::ftol_i32_low_masked(X::mul(
                X::load_i32(type_speed),
                X::load_f32(attached.speed_factor),
            ))
        }
        _ => type_speed,
    }
}

impl Simulation {
    /// `TechnoClass::Fire_At`'s limpet arm, Firestorm 2.0
    /// `0x006301EB..0x0063032A`: true when it took the shot, which then
    /// launches nothing. In Firestorm it is the first thing Fire_At does once
    /// it has a weapon and a target.
    ///
    /// It takes a shot whose warhead's `LimpetFactor=` is nonzero (a `TEST`
    /// and `JBE`, so negatives count) at a Techno (`0x00405DF0`). When the
    /// firer's house has limpeted the target already it does nothing else
    /// (`0x0063022B`): no sound and no delete, so the firer tries again on
    /// its next shot. Otherwise:
    /// - the house joins the target's limpets ([`Limpets::attach`]), and the
    ///   maps that see through the target change with it
    ///   ([`Self::change_limpets`]);
    /// - the weapon's report plays without a position, at full volume
    ///   (`0x006302B3..0x006302DE`, `Sound_Effect @ 0x00664970`), picked by
    ///   the firer's constructor word as every report is
    ///   ([`super::combat::world_fire::per_shot_report`]); no RNG is drawn;
    /// - the firer is deleted (vt+0xE4 at `0x0063031C`): UnInit, with no
    ///   death, explosion, survivors or kill credit.
    ///
    /// No bullet, damage, ammo or rearm. Not ported: the `FacingClass::
    /// Set_ROT` pair on the firer's own facings (`0x00630267..0x006302AE`),
    /// dead because the firer is deleted, the debug log line
    /// (`0x00630310`), and TEvent 55 (`0x006302F8`), whose number is a
    /// different gamemd event (docs/ts-limpets.md).
    pub(crate) fn limpet_fire_at(
        &mut self,
        rules: &RuleSet,
        firer: u64,
        target: u64,
        weapon: &WeaponType,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let Some(limpet_factor) = rules
            .weapon_warhead(weapon)
            .map(|warhead| warhead.limpet_factor)
            .filter(|&factor| factor != 0)
        else {
            return false;
        };
        let entities = &self.substrate.entities;
        let (Some(firer_entity), Some(target_entity)) = (entities.get(firer), entities.get(target))
        else {
            return false;
        };
        let house = firer_entity.owner();
        if target_entity.seldom.limpets.attached_by(house) {
            return true;
        }
        let sequence = firer_entity.techno_ctor_random_word();
        self.change_limpets(target, rules, effects, |limpets| {
            limpets.attach(house, limpet_factor);
        });
        if let Some(report) = super::combat::world_fire::per_shot_report(weapon, false, sequence) {
            self.sound_events.emit(SimSoundEvent::VocCentered {
                sound_id: rules.sound_name(report),
            });
        }
        self.uninit_with_context(firer, UninitContext::new(rules).with_effects(effects));
        true
    }

    /// Take every limpet off `id`: a heal, Firestorm 2.0 `TechnoClass::
    /// Take_Damage @ 0x006325A0`, which on `*damage < 0` (`0x006325C5`)
    /// zeroes mask and factor before any other receiver work
    /// (`0x006326E1..0x006326F4`), or a depot repair, `TechnoClass::
    /// Receive_Message` RADIO_REPAIR (26, `0x0062ABE1..0x0062ABF0`), before
    /// its health test, so a full-health vehicle is cleared too. Both then
    /// reset the facings' ROT to the type's, which nothing changed, so that
    /// is not ported.
    pub(crate) fn clear_limpets(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.foot_speed.limpeted())
        {
            self.change_limpets(id, rules, effects, Limpets::clear);
        }
    }

    /// Firestorm 2.0 `UnitClass::AI` after its visceroid AI
    /// (`0x0064E1FA..0x0064E221`): a Limpet Drone's (`IsLimpetDrone=`,
    /// UnitType+0x4A2) Stage of 10 or more goes back to 0. Its Unlimbo starts
    /// the Stage at rate 1 (`world::lifecycle::reveal`) and the common
    /// Techno step (`TechnoClass::AI` `0x0062EAF2..0x0062EB30`, before this
    /// in the same AI) adds one a frame, so its body, which the Unit SHP draw
    /// (`0x00652EE0`) takes from the Stage in place of a facing
    /// (`0x00652F2D`, `0x0065377F`), loops ten frames. The Stage is read
    /// first, so no other Unit looks up its type here.
    pub(crate) fn wrap_drone_stage(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.native_stage().value() < 10
            || entity.category() != crate::map::entities::EntityCategory::Unit
            || !self
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| object.is_limpet_drone)
        {
            return;
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.set_native_stage_value(0);
        }
    }

    /// Change `id`'s limpets between a release and a reveal of its sight, so
    /// each map a limpeting house holds is given and taken back the same
    /// sight (`world::shroud_refresh`, the house gate). VERA-only: Firestorm
    /// keeps the local player's map alone and never takes sight back, so its
    /// `TechnoClass::Look @ 0x00638160` just reveals on its next call.
    fn change_limpets(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
        change: impl FnOnce(&mut Limpets),
    ) {
        let config = self.sight_reveal_config(rules);
        let sees = self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| crate::sim::vision::holds_sight(entity, &config));
        let mut context = UninitContext::new(rules).with_effects(effects);
        if sees {
            self.release_object_sight_with_context(id, context.reborrow());
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            change(&mut entity.seldom.limpets);
            entity.foot_speed.sync_limpets(&entity.seldom.limpets);
        }
        if sees {
            self.update_object_sight_with_context(id, 0, None, context);
        }
    }
}

#[cfg(all(test, tests_sim_h_o))]
#[path = "limpet_tests.rs"]
mod tests;
