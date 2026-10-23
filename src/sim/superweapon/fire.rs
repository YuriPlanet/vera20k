//! Firing a superweapon: `HouseClass::Fire_SW @ 0x004FAE50`, which the
//! SPECIAL_PLACE event (`EventClass::Execute 0x004C78D6`) calls for a
//! player's click and the computer's launchers call directly;
//! `SuperClass::ClickFire @ 0x006CB920`; the dispatch of
//! `SuperClass::Launch @ 0x006CC390`; and the computer houses' alert
//! (`HouseClass__Super_Defense_Alert @ 0x004FAF00`).
//!
//! Fire_SW clicks the Super, then hands it and the cell to every house in
//! reverse HouseClass::Array order, whether or not the click launched. The
//! Super index Fire_SW takes is the type's `[SuperWeaponTypes]` index, the
//! command's type.
//!
//! A `PostClick=` type (retail ChronoWarpSpecial) pairs with the Super at its
//! `PreDependent=` index: Fire_SW copies that Super's map cell before the
//! click (`0x004FAE6C..0x004FAE8F`) and afterwards clears its charge
//! (`SuperClass::SetReadiness @ 0x006CB820`) and releases its anim
//! (`SuperClass::StopPreclickAnim @ 0x006CB830`), which restarts its
//! recharge. Every house holds one Super per `[SuperWeaponTypes]` entry from
//! its constructor (`0x004F620E..0x004F6290`); the scenario bootstrap owns
//! that construction. A click never constructs missing instance state.
//!
//! RESIDUALS:
//! - A `PostClick=` type whose `PreDependent=` is unset or past the list
//!   (constructor -1) reads outside the Supers vector natively; VERA pairs
//!   nothing. Dormant in retail.
//! - Authored charge-drain uses the native state/timer branch. Fresh House
//!   construction clears FirestormActive+1FA; imported nonzero native House
//!   states have no represented producer. Installed stock types do not enable
//!   UseChargeDrain. Ordinary one-time consumption uses the retained instance.

#[cfg(test)]
#[path = "fire_tests.rs"]
mod tests;

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::{SuperWeaponKind, SuperWeaponType};
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{SimSoundEvent, Simulation};

impl Simulation {
    /// `HouseClass::Fire_SW @ 0x004FAE50` for `owner`'s Super of type
    /// `sw_type_id` at `cell`. Returns whether ClickFire called Launch, a
    /// VERA answer: native Fire_SW returns 1 either way.
    /// Manual commands and AI share this transaction; `automatic` selects
    /// the extension target requirements before any charge is consumed.
    pub(crate) fn fire_super_weapon(
        &mut self,
        rules: &RuleSet,
        owner: InternedId,
        sw_type_id: InternedId,
        cell: (u16, u16),
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
        automatic: bool,
    ) -> bool {
        self.fire_super_weapon_selected(
            rules,
            owner,
            sw_type_id,
            cell,
            overlay_registry,
            frame_effects,
            automatic,
            None,
        )
    }

    pub(crate) fn fire_super_weapon_selected(
        &mut self,
        rules: &RuleSet,
        owner: InternedId,
        sw_type_id: InternedId,
        cell: (u16, u16),
        overlay_registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
        automatic: bool,
        selection: Option<super::PaidSuperWeaponSelection>,
    ) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        super::ai_fire::observe(super::ai_fire::AiFireEvent::Fire(sw_type_id, cell));
        let Some(sw) = rules.super_weapon(self.interner.resolve(sw_type_id)) else {
            return false;
        };
        if super_instance(self, owner, sw_type_id).is_none() {
            return false;
        }
        // Refuse extension identities before any paired-state, charge, wallet,
        // RNG or presentation writes, including manually forged rule records.
        #[cfg(not(feature = "ares"))]
        {
            let _ = automatic;
            if selection.is_some()
                || matches!(
                    sw.kind,
                    SuperWeaponKind::GenericWarhead | SuperWeaponKind::UnitDelivery
                )
            {
                return false;
            }
        }
        // Burn a well-formed explicit source request's ID even if admission
        // later refuses (e.g. funds changed while queued). Otherwise an old
        // cancel/final for that refused request could match a future retry.
        // This is sequence metadata only: no wallet, charge or pair is touched.
        #[cfg(feature = "ares")]
        let requested_source = selection.filter(|_| sw.pre_click && !sw.post_click);
        #[cfg(feature = "ares")]
        if let Some(request) = requested_source {
            let Some(next) = self.next_paid_super_weapon_selection(rules, owner, sw_type_id) else {
                return false;
            };
            if request.source != next.source
                || request.destination != next.destination
                || request.sequence < next.sequence
            {
                return false;
            }
            super_instance(self, owner, sw_type_id)
                .expect("retained source")
                .paid_sequence = request.sequence;
        }
        // Optional extension preflight precedes paired-Super writes, effects,
        // defense alerts and charge consumption. Stock-only commands retain
        // their native behavior, including native refused-click side effects.
        #[cfg(any(feature = "ares", feature = "phobos"))]
        if !super::super_weapon_available(self, rules, owner, &sw.id) {
            return false;
        }
        #[cfg(feature = "ares")]
        if super::targeting::check(self, rules, owner, sw, cell, automatic).is_err() {
            return false;
        }
        #[cfg(feature = "ares")]
        if sw.kind == SuperWeaponKind::UnitDelivery
            && !super::unit_delivery::admits(self, rules, owner, sw, cell)
        {
            return false;
        }
        let paired = sw
            .post_click
            .then(|| super::pre_dependent(rules, sw))
            .flatten()
            .and_then(|(name, pre)| Some((self.interner.get(name)?, pre)));
        #[cfg(feature = "ares")]
        let paid_source = super::two_stage::destination_for_source(self, rules, sw_type_id, sw);
        #[cfg(feature = "ares")]
        let paid_final = paired.filter(|_| super::paid_pair_source(rules, sw).is_some());
        #[cfg(feature = "ares")]
        if paid_source.is_none()
            && paid_final.is_none()
            && !super::money::configuration_supported(sw)
        {
            return false;
        }
        #[cfg(feature = "ares")]
        if let Some((source, source_sw)) = paid_final {
            let Some(ticket) = selection else {
                return false;
            };
            if ticket.source != source
                || ticket.destination != sw_type_id
                || self.paid_super_weapon_selection(owner) != Some(ticket)
                || !source_sw.pre_click
                || source_sw.post_click
            {
                return false;
            }
            if !super::two_stage::source_available(self, rules, owner, source, source_sw) {
                Self::cancel_paid_super_weapon_selection(
                    &mut self.substrate,
                    &mut self.sound_events,
                    &mut self.super_weapons,
                    rules,
                    owner,
                    ticket,
                );
                return false;
            }
        } else if selection.is_some() && paid_source.is_none() {
            return false;
        }
        #[cfg(feature = "ares")]
        let source_ticket = if paid_source.is_some() {
            if !super::two_stage::source_available(self, rules, owner, sw_type_id, sw) {
                return false;
            }
            let ticket = requested_source
                .or_else(|| self.next_paid_super_weapon_selection(rules, owner, sw_type_id));
            let Some(ticket) = ticket else {
                return false;
            };
            Some(ticket)
        } else {
            None
        };
        // PostClick takes SOURCE settings (Ares SWTypeExt::Launch), even when
        // the destination has a different fee or grant. No second wallet exists.
        #[cfg(feature = "ares")]
        let payment = paid_final.unwrap_or((sw_type_id, sw));
        #[cfg(feature = "ares")]
        let guarded = paid_source.is_some() || paid_final.is_some() || sw.money_amount != 0;
        // Explicit extension only: all admission precedes paired-Super writes.
        // Stock zero-cost Fire_SW retains its native refused-click tail.
        #[cfg(feature = "ares")]
        if guarded
            && (!(if paid_source.is_some() || paid_final.is_some() {
                super::targeting::on_map(self, cell)
            } else {
                super::money::valid_target(self, sw, cell)
            }) || !click_fire_admission(self, owner, sw_type_id, sw)
                || !super::money::admit(
                    &mut self.sound_events,
                    &self.houses,
                    owner,
                    payment.0,
                    payment.1,
                ))
        {
            return false;
        }
        #[cfg(feature = "ares")]
        if paid_source.is_some() {
            Self::retire_paid_selections(
                &mut self.substrate,
                &mut self.sound_events,
                &mut self.super_weapons,
                rules,
                owner,
            );
        }
        if let Some((pre_type, _)) = paired {
            let Some(pre) = super_instance(self, owner, pre_type) else {
                return false;
            };
            let cell = pre.chrono_cell();
            super_instance(self, owner, sw_type_id)
                .expect("retained fired Super")
                .chrono_cell = cell;
        }
        #[cfg(feature = "ares")]
        let click_payment = paid_source.is_none().then_some(payment);
        #[cfg(feature = "ares")]
        let selecting_paid_source = paid_source.is_some();
        #[cfg(not(feature = "ares"))]
        let (click_payment, selecting_paid_source) = (None, false);
        let called_launch = click_fire(
            self,
            rules,
            owner,
            sw_type_id,
            sw,
            cell,
            overlay_registry,
            frame_effects,
            click_payment,
            selecting_paid_source,
        );
        #[cfg(feature = "ares")]
        if !called_launch && guarded {
            return false;
        }
        #[cfg(feature = "ares")]
        if let Some(ticket) = source_ticket {
            let instance = super_instance(self, owner, sw_type_id).expect("retained source");
            instance.paid_sequence = ticket.sequence;
            instance.paid_selection = Some(ticket);
        }
        if let Some((pre_type, pre_sw)) = paired {
            #[cfg(feature = "ares")]
            if paid_final.is_some() {
                let frame = self.session.binary_frame();
                let source = super_instance(self, owner, pre_type).expect("retained paired source");
                source.paid_selection = None;
                source.finish_click_fire(pre_sw, frame);
            }
            // `0x004FAEA6..0x004FAEC1`.
            super_instance(self, owner, pre_type)
                .expect("retained paired Super")
                .is_ready = false;
            Self::release_super_anim(
                &mut self.substrate,
                &mut self.super_weapons,
                owner,
                pre_type,
            );
            let frame = self.session.binary_frame();
            super_instance(self, owner, pre_type)
                .expect("retained paired Super")
                .stop_preclick(pre_sw, frame);
        }
        #[cfg(feature = "ares")]
        if called_launch && paid_source.is_none() && paid_final.is_none() {
            Self::retire_paid_selections(
                &mut self.substrate,
                &mut self.sound_events,
                &mut self.super_weapons,
                rules,
                owner,
            );
        }
        // `0x004FAEC6..0x004FAEF2`: every house, last to first.
        let houses: Vec<InternedId> = self.houses.order().iter().rev().copied().collect();
        for house in houses {
            alert_super_weapon_defense(
                &self.session,
                &mut self.scenario_rng,
                &self.substrate,
                &mut self.houses,
                &self.resolved_terrain,
                rules,
                house,
                sw,
                cell,
            );
        }
        called_launch
    }
}

/// `SuperClass::ClickFire @ 0x006CB920` without charge drain: an admitted
/// Super launches unless a Lightning Storm finds one raging or counting
/// down (`0x006CBAA7`) or a Psychic Dominator finds one active
/// (`0x006CBAD6`), refusals that keep the charge and tell the owner's
/// player ([`SimSoundEvent::LightningStormRefused`],
/// [`SimSoundEvent::PsychicDominatorRefused`]); then readiness and
/// recharge as [`super::SuperWeaponInstance::finish_click_fire`]. Returns
/// whether it called Launch, whatever the case then did: VERA's answer, as
/// native returns 0 after Launch outside its one-time arm (`0x006CBB5A`).
fn click_fire(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
    payment: Option<(InternedId, &SuperWeaponType)>,
    selecting_paid_source: bool,
) -> bool {
    if sw.use_charge_drain {
        // ClickFire6CB934..6CBA7A precedes ordinary ready/running/one-time
        // admission. House control is independent of local launch output.
        let Some(house) = sim.houses.get(&owner) else {
            return false;
        };
        let human = house.is_controlled_by_human(sim.session.game_mode_nonzero());
        let called_launch = if human {
            let frame = sim.session.binary_frame();
            let Some(instance) = super_instance(sim, owner, sw_type_id) else {
                return false;
            };
            instance.click_charge_drain(sw, frame, rules.general.charge_to_drain_ratio)
        } else {
            // HouseCtor4F573A clears +1FA; no enabled fresh writer has been
            // established. Native6CBA52 tests that byte, without human timer
            // conversion. Do not manufacture a Firestorm effect/toggle here.
            true
        };
        if called_launch {
            launch(
                sim,
                rules,
                owner,
                sw_type_id,
                sw,
                cell,
                overlay_registry,
                frame_effects,
            );
        }
        return called_launch;
    }
    // Paid launches passed this same owner before the outer transaction writes.
    #[cfg(feature = "ares")]
    let needs_native_admission = sw.money_amount == 0;
    #[cfg(not(feature = "ares"))]
    let needs_native_admission = true;
    #[cfg(not(feature = "ares"))]
    let _ = (payment, selecting_paid_source);
    if needs_native_admission && !click_fire_admission(sim, owner, sw_type_id, sw) {
        return false;
    }
    #[cfg(feature = "ares")]
    if sw.kind == SuperWeaponKind::GenericWarhead
        && !super::generic_warhead::can_launch(sim, rules, sw, cell)
    {
        return false;
    }
    #[cfg(feature = "ares")]
    if let Some((money_type, money_sw)) = payment
        && money_sw.money_amount != 0
        && !super::money::transact(
            &mut sim.sound_events,
            &mut sim.houses,
            owner,
            money_type,
            money_sw,
        )
    {
        return false;
    }
    launch(
        sim,
        rules,
        owner,
        sw_type_id,
        sw,
        cell,
        overlay_registry,
        frame_effects,
    );
    // `0x006CBBDE..0x006CBC34`: a type that is neither pairs no anim.
    let one_time = super_instance(sim, owner, sw_type_id).is_some_and(|instance| instance.one_time);
    //6CBB1B returns before placement-Anim retirement/recharge for one-time.
    if !one_time && !sw.manual_control && !sw.pre_click && !sw.post_click {
        Simulation::release_super_anim(
            &mut sim.substrate,
            &mut sim.super_weapons,
            owner,
            sw_type_id,
        );
    }
    let frame = sim.session.binary_frame();
    if let Some(instance) = sim.super_weapons.instance_mut(&owner, &sw_type_id) {
        #[cfg(feature = "ares")]
        if selecting_paid_source {
            // Selection neither consumes a one-time grant nor starts recharge.
            instance.is_ready = false;
            return true;
        }
        instance.finish_click_fire(sw, frame);
    }
    true
}

/// Pure admission apart from existing type-specific refusal feedback. Reused
/// before paid paired-Super writes; successful checks neither charge nor pay.
fn click_fire_admission(
    sim: &mut Simulation,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
) -> bool {
    if sw.use_charge_drain {
        return false;
    }
    let Some(instance) = super_instance(sim, owner, sw_type_id) else {
        return false;
    };
    if !instance.click_fire_admits() && !sw.post_click {
        return false;
    }
    if sw.kind == SuperWeaponKind::LightningStorm && super::lightning_storm::has_deferment(sim) {
        sim.sound_events
            .emit(SimSoundEvent::LightningStormRefused { owner });
        return false;
    }
    if sw.kind == SuperWeaponKind::PsychicDominator && super::psychic_dominator::active(sim) {
        sim.sound_events
            .emit(SimSoundEvent::PsychicDominatorRefused { owner });
        return false;
    }
    true
}

/// `SuperClass::Launch @ 0x006CC390`: the case of the type's `Type=`
/// (`+0xB4`, jump table `0x006CDE44`). Native returns nothing; each case's
/// own answer serves only its tests.
fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
    (rx, ry): (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    use super::paradrop::ParaDropKind;
    // Every native stock case except ChronoWarp begins with rawReady+6F
    // (ForceShield6CD072..6CD077; case4 starts6CC4B2 with radar production).
    // Drain ClickFire can reach Launch after Reset retained state1 but cleared
    // rawReady. Keep the earlier timer transition while skipping case effects.
    let native_kind = sw.kind.native_index();
    if (0..12).contains(&native_kind)
        && native_kind != 4
        && !super::is_charged(sim, owner, sw_type_id)
    {
        return;
    }
    match sw.kind {
        // Launch6CC3A3..6CC3AC: the constructor's -1 exits without case
        // effects. ClickFire's ordinary/one-time finish still runs afterwards.
        SuperWeaponKind::NoEffect => {}
        #[cfg(feature = "ares")]
        SuperWeaponKind::UnitDelivery => {
            if super::unit_delivery::launch(
                sim,
                rules,
                owner,
                sw_type_id,
                sw,
                (rx, ry),
                overlay_registry,
                frame_effects,
            ) {
                super::record_ares_launch(sim, rules, owner, sw_type_id);
            }
        }
        #[cfg(not(feature = "ares"))]
        SuperWeaponKind::GenericWarhead | SuperWeaponKind::UnitDelivery => {}
        SuperWeaponKind::MultiMissile => {
            super::nuke::launch(sim, rules, owner, sw_type_id, sw, (rx, ry));
        }
        SuperWeaponKind::LightningStorm => {
            super::lightning_storm::launch(sim, rules, owner, sw_type_id, (rx, ry));
        }
        SuperWeaponKind::IronCurtain => {
            super::iron_curtain::launch(
                sim,
                rules,
                owner,
                rx,
                ry,
                sw_type_id,
                overlay_registry,
                frame_effects,
            );
        }
        SuperWeaponKind::ForceShield => {
            super::force_shield::launch(sim, rules, owner, rx, ry, sw_type_id, frame_effects);
        }
        SuperWeaponKind::GeneticConverter => {
            super::genetic_converter::launch(
                sim,
                rules,
                owner,
                rx,
                ry,
                sw_type_id,
                overlay_registry,
                frame_effects,
            );
        }
        SuperWeaponKind::PsychicReveal => {
            super::psychic_reveal::launch(sim, rules, owner, rx, ry, sw_type_id);
        }
        SuperWeaponKind::ParaDrop => {
            super::paradrop::launch(
                sim,
                rules,
                owner,
                rx,
                ry,
                ParaDropKind::Generic,
                sw_type_id,
                frame_effects,
            );
        }
        SuperWeaponKind::AmerParaDrop => {
            super::paradrop::launch(
                sim,
                rules,
                owner,
                rx,
                ry,
                ParaDropKind::American,
                sw_type_id,
                frame_effects,
            );
        }
        SuperWeaponKind::ChronoSphere => {
            super::chronosphere::launch_source(sim, rules, owner, sw_type_id, (rx, ry));
        }
        SuperWeaponKind::ChronoWarp => {
            super::chronosphere::launch_warp(
                sim,
                rules,
                owner,
                sw_type_id,
                (rx, ry),
                overlay_registry,
                frame_effects,
            );
        }
        SuperWeaponKind::PsychicDominator => {
            super::psychic_dominator::launch(sim, rules, owner, sw_type_id, (rx, ry));
        }
        SuperWeaponKind::SpyPlane => {
            super::spy_plane::launch(sim, rules, owner, sw_type_id, (rx, ry), frame_effects);
        }
        #[cfg(feature = "ares")]
        SuperWeaponKind::GenericWarhead => {
            if super::generic_warhead::launch(
                sim,
                rules,
                owner,
                sw_type_id,
                sw,
                (rx, ry),
                overlay_registry,
                frame_effects,
            ) {
                super::record_ares_launch(sim, rules, owner, sw_type_id);
            }
        }
    }
}

/// The retained Super constructed by the scenario House owner. Native valid
/// scenarios hold every type; incomplete external fixtures can safely miss it.
fn super_instance(
    sim: &mut Simulation,
    owner: InternedId,
    sw_type: InternedId,
) -> Option<&mut super::SuperWeaponInstance> {
    sim.super_weapons.instance_mut(&owner, &sw_type)
}

/// `HouseClass @ 0x004FAF00` for `house`, given the fired Super's type and
/// its cell: a computer house (not human, `+0x1EC`, nor MultiplayPassive,
/// HouseType `+0x1A6`) facing a type with `AIDefendAgainst=` (`+0xEC`)
/// measures from its base to the cell; within `AISuperDefenseDistance=`
/// (`Rules+0xEE4`) one Scenario draw `RandomRanged(0, 99)` (`0x0065C7E0`,
/// `0x004FB03D`) at most its difficulty's `AISuperDefenseProbability=`
/// (`Rules+0xEC8`, indexed by `+0x184`) alerts it: the cell it defends
/// (`+0x54F4`) becomes its first construction yard's (`+0x54`, the yard's
/// GetCoords `0x00447AC0` over 256, toward zero), else its base cell, and
/// the frame is stored (`+0x54FC`).
///
/// The base is the alternate centre (`+0x5494`) unless empty, else the
/// primary (`+0x5490`); both points are a cell's GetCoords (vt+0x48,
/// `0x00486840`) through the never-null map lookup (`0x005657A0`), an empty
/// base the origin (the zeroed static `0x00A8EFF8`), and the distance is
/// `CoordStruct::Distance3D @ 0x0041C380` of base minus cell.
fn alert_super_weapon_defense(
    session: &ScenarioSession,
    scenario_rng: &mut SimRng,
    substrate: &ObjectSubstrate,
    houses: &mut Houses,
    resolved_terrain: &Option<ResolvedTerrainGrid>,
    rules: &RuleSet,
    house_id: InternedId,
    sw: &SuperWeaponType,
    cell: (u16, u16),
) {
    let Some(house) = houses.get(&house_id) else {
        return;
    };
    if house.multiplay_passive() || house.is_human() || !sw.ai_defend_against {
        return;
    }
    let base = house.base_origin();
    let probability = house.difficulty_value(&rules.general.ai_super_defense_probability);
    let first_yard = house.build_const_order().first().copied();
    let terrain = resolved_terrain.as_ref();
    let cell_coords = |(x, y): (u16, u16)| {
        let coord = crate::sim::projectile::cell_ground_coord(terrain, x, y);
        [coord.x, coord.y, coord.z]
    };
    let base_coords = if base == (0, 0) {
        [0, 0, 0]
    } else {
        cell_coords(base)
    };
    let distance = crate::util::native_x87::distance_3d_leptons(base_coords, cell_coords(cell));
    if distance > rules.general.ai_super_defense_distance {
        return;
    }
    if scenario_rng.next_range_i32_inclusive(0, 99) > probability {
        return;
    }
    let defended = first_yard
        .and_then(|yard| substrate.entities.get(yard))
        .map(|yard| {
            let coords = crate::sim::movement::ground_pose::object_get_coords(
                yard,
                resolved_terrain.as_ref(),
            );
            ((coords.x / 256) as u16, (coords.y / 256) as u16)
        })
        .unwrap_or(base);
    let frame = session.binary_frame() as i32;
    if let Some(house) = houses.get_mut(&house_id) {
        house.alert_super_weapon_defense(defended, frame);
    }
}
