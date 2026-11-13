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
    /// Ares SPECIAL_PLACE hook10038E90: UseAITargeting replaces the event
    /// cell with the shared automatic selector. Script producers still call
    /// Fire_SW directly and retain their explicit location.
    pub(crate) fn place_super_weapon(
        &mut self,
        rules: &RuleSet,
        owner: InternedId,
        sw_type_id: InternedId,
        cell: (u16, u16),
        overlays: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        if rules.ares_super_weapons() && rules.super_weapon_uses_ai_targeting(self.interner.resolve(sw_type_id)) {
            let launched = super::ai_fire::fire_extension_automatic(
                self, rules, owner, sw_type_id, overlays, effects, true,
            );
            if !launched {
                // Ares SPECIAL_PLACE10038ECA..10038EE0 prints CannotFire
                // after selector failure. The client owns the PlayerPtr gate.
                self.sound_events
                    .emit(SimSoundEvent::AresSuperWeaponRefused {
                        owner,
                        sw_type: sw_type_id,
                    });
            }
            return launched;
        }
        self.fire_super_weapon(rules, owner, sw_type_id, cell, overlays, effects, false)
    }

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
            if automatic {
                super::SuperWeaponTargetSource::Automatic
            } else {
                super::SuperWeaponTargetSource::Manual
            },
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
        target_source: super::SuperWeaponTargetSource,
        selection: Option<super::PaidSuperWeaponSelection>,
    ) -> bool {
        // Whether Ares plays in this game, read once: without it only the
        // stock Fire_SW statements below run, in the stock order.
        let ares = rules.ares_super_weapons();
        let automatic = target_source != super::SuperWeaponTargetSource::Manual;
        #[cfg(any(test, feature = "test-support"))]
        super::ai_fire::observe(super::ai_fire::AiFireEvent::Fire(sw_type_id, cell));
        let Some(sw) = rules.super_weapon(self.interner.resolve(sw_type_id)) else {
            return false;
        };
        if super_instance(self, owner, sw_type_id).is_none() {
            return false;
        }
        // Without Ares, refuse extension identities before any paired-state,
        // charge, wallet, RNG or presentation writes, including manually
        // forged rule records.
        if !ares && (selection.is_some() || sw.kind.is_extension()) {
            return false;
        }
        // Burn a well-formed explicit source request's ID even if admission
        // later refuses (e.g. funds changed while queued). Otherwise an old
        // cancel/final for that refused request could match a future retry.
        // This is sequence metadata only: no wallet, charge or pair is touched.
        let requested_source = selection.filter(|_| ares && sw.pre_click && !sw.post_click);
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
        if (ares || cfg!(feature = "phobos"))
            && !super::super_weapon_available(self, rules, owner, &sw.id)
        {
            return false;
        }
        if ares
            && target_source != super::SuperWeaponTargetSource::SelectedAutomatic
            && super::targeting::check(self, rules, owner, sw, cell, automatic).is_err()
        {
            return false;
        }
        if ares
            && sw.kind == SuperWeaponKind::UnitDelivery
            && !super::unit_delivery::admits(self, rules, owner, sw, cell)
        {
            return false;
        }
        // Ares hook 4FAE72 (10034C90) pairs a handler's PostClick type with
        // the House's last Super instead of its PreDependent.
        let post_click_source = |sim: &Self| {
            if ares && sw.kind.has_ares_handler() {
                super::ares_post_click_source(sim, rules, owner)
            } else {
                super::pre_dependent(rules, sw)
            }
        };
        let paired = sw
            .post_click
            .then(|| post_click_source(self))
            .flatten()
            .and_then(|(name, pre)| Some((self.interner.get(name)?, pre)));
        // The paid two-stage bindings are Ares': without it they hold their
        // neutral values and every test on them below falls through.
        let paid_source = if ares {
            super::two_stage::destination_for_source(self, rules, sw_type_id, sw)
        } else {
            None
        };
        let paid_final = paired.filter(|_| ares && super::paid_pair_source(rules, sw).is_some());
        if ares
            && paid_source.is_none()
            && paid_final.is_none()
            && !super::money::configuration_supported(sw)
        {
            return false;
        }
        // Without Ares `paid_final` is `None` and a `selection` was refused
        // above, so neither arm runs.
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
        let payment = paid_final.unwrap_or((sw_type_id, sw));
        let guarded =
            ares && (paid_source.is_some() || paid_final.is_some() || sw.money_amount != 0);
        // Explicit extension only: all admission precedes paired-Super writes.
        // Stock zero-cost Fire_SW retains its native refused-click tail.
        if guarded
            && (!(if paid_source.is_some() || paid_final.is_some() {
                super::targeting::on_map(self, cell)
            } else {
                super::money::valid_target(self, sw, cell)
            }) || !click_fire_admission(self, owner, sw_type_id, sw, ares)
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
        let click_payment = (ares && paid_source.is_none()).then_some(payment);
        let selecting_paid_source = paid_source.is_some();
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
        if !called_launch && guarded {
            return false;
        }
        if let Some(ticket) = source_ticket {
            let instance = super_instance(self, owner, sw_type_id).expect("retained source");
            instance.paid_sequence = ticket.sequence;
            instance.paid_selection = Some(ticket);
        }
        if let Some((pre_type, pre_sw)) = paired {
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
        if ares && called_launch && paid_source.is_none() && paid_final.is_none() {
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
        // Ares 3.0 HouseClass_SWDefendAgainst_Aborted 10034C70 (hook
        // 0x004FAF2A): a Super still charged (`+0x6F`) did not fire, and no
        // house is alerted (no draw).
        let houses = if ares
            && self
                .super_weapons
                .get(&owner)
                .and_then(|supers| supers.get(&sw_type_id))
                .is_some_and(|instance| instance.is_ready())
        {
            Vec::new()
        } else {
            houses
        };
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
    let ares = rules.ares_super_weapons();
    if sw.use_charge_drain {
        // ClickFire6CB934..6CBA7A precedes ordinary ready/running/one-time
        // admission. House control is independent of local launch output.
        let Some(house) = sim.houses.get(&owner) else {
            return false;
        };
        let human = house.is_controlled_by_human(sim.session.game_mode_nonzero());
        // Ares ClickFire10035130 drops the computer's FirestormActive branch:
        // every House toggles with the type's GetChargeToDrainRatio.
        let toggles = ares || human;
        let was_draining = ares
            && super_instance(sim, owner, sw_type_id)
                .is_some_and(|instance| instance.charge_drain_state() == 2);
        let called_launch = if toggles {
            let frame = sim.session.binary_frame();
            let ratio = super::charge_to_drain_ratio(rules, sw);
            let Some(instance) = super_instance(sim, owner, sw_type_id) else {
                return false;
            };
            instance.click_charge_drain(sw, frame, ratio, rules.ares_super_weapons())
        } else {
            // HouseCtor4F573A clears +1FA; no enabled fresh writer has been
            // established. Native6CBA52 tests that byte, without human timer
            // conversion. Do not manufacture a Firestorm effect/toggle here.
            let _ = human;
            true
        };
        // 10035130's draining arm calls SWTypeExt::Deactivate after the timer.
        if was_draining && !called_launch {
            super::deactivate_ares_handler(
                sim,
                rules,
                owner,
                sw_type_id,
                sw,
                human,
                frame_effects.with_overlay_registry(overlay_registry),
            );
        }
        if called_launch {
            let effect_launched = launch(
                sim,
                rules,
                owner,
                sw_type_id,
                sw,
                cell,
                overlay_registry,
                frame_effects,
            );
            // Launch reaches the common SWTypeExt::Launch10032ED0 for a new
            // type here too. Its money step never settles: Fire_SW refuses a
            // charge-drain type with Money.Amount before this click.
            if ares && effect_launched && sw.kind.has_ares_handler() {
                super::ares_presentation::on_launch(sim, rules, owner, sw_type_id, cell);
                super::raise_ares_activation_events(
                    sim,
                    rules,
                    owner,
                    sw_type_id,
                    cell,
                    overlay_registry,
                    frame_effects,
                );
            }
        }
        return called_launch;
    }
    // Paid launches passed this same owner before the outer transaction writes.
    let needs_native_admission = !ares || sw.money_amount == 0;
    if needs_native_admission && !click_fire_admission(sim, owner, sw_type_id, sw, ares) {
        return false;
    }
    if ares
        && sw.kind == SuperWeaponKind::GenericWarhead
        && !super::generic_warhead::can_launch(sim, rules, sw, cell)
    {
        return false;
    }
    // Without Ares no type has a handler, and Fire_SW passes no `payment`
    // and never selects a paid source.
    let extension_effect = ares && sw.kind.has_ares_handler();
    if let Some((money_type, money_sw)) = payment
        && !extension_effect
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
    let effect_launched = launch(
        sim,
        rules,
        owner,
        sw_type_id,
        sw,
        cell,
        overlay_registry,
        frame_effects,
    );
    // Installed Ares SWTypeExt::Launch10032ED0 calls Activate first and only
    // reaches money10032FDF after success. In particular a HunterSeeker with
    // no usable launch building still spends its charge, but never its fee.
    // Keep one wallet/settlement owner shared with the other extension kinds.
    if extension_effect && effect_launched {
        // 10032FDF: skipped when the handler's Flags hold NoMoney.
        if let Some((money_type, money_sw)) = payment
            && money_sw.money_amount != 0
            && !sw
                .kind
                .ares_flags()
                .has(crate::rules::superweapon_type::AresSuperWeaponFlags::NO_MONEY)
        {
            super::money::transact(
                &mut sim.sound_events,
                &mut sim.houses,
                owner,
                money_type,
                money_sw,
            );
        }
        // Common Launch10032FDF..10033242 settles first, then constructs
        // animation/sound and finally removes the queued Ready announcement.
        super::ares_presentation::on_launch(sim, rules, owner, sw_type_id, cell);
        // Activate10032020 raises its trigger events once Launch returned.
        super::raise_ares_activation_events(
            sim,
            rules,
            owner,
            sw_type_id,
            cell,
            overlay_registry,
            frame_effects,
        );
    }
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
    ares: bool,
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
    // Installed Ares asks the handler's AbortFire (10072AB0) instead.
    if ares && sw.kind == SuperWeaponKind::LightningStorm {
        return !super::lightning_storm::ares_abort_fire(sim, owner, sw_type_id);
    }
    if sw.kind == SuperWeaponKind::LightningStorm && super::lightning_storm::has_deferment(sim) {
        sim.sound_events
            .emit(SimSoundEvent::LightningStormRefused { owner });
        return false;
    }
    // Installed Ares asks the handler's AbortFire (10070780) instead.
    if ares && sw.kind == SuperWeaponKind::PsychicDominator {
        return !super::psychic_dominator::ares_abort_fire(sim, owner, sw_type_id);
    }
    if sw.kind == SuperWeaponKind::PsychicDominator && super::psychic_dominator::active(sim) {
        sim.sound_events
            .emit(SimSoundEvent::PsychicDominatorRefused { owner });
        return false;
    }
    true
}

/// `SuperClass::Launch @ 0x006CC390`: the case of the type's `Type=`
/// (`+0xB4`, jump table `0x006CDE44`). Stock Launch returns nothing; the
/// extension Activate result gates successful-effect bookkeeping and money.
fn launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw_type_id: InternedId,
    sw: &SuperWeaponType,
    (rx, ry): (u16, u16),
    overlay_registry: Option<&OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    use super::paradrop::ParaDropKind;
    let ares = rules.ares_super_weapons();
    // Hook 6CC390 (100355C0) -> SWTypeExt::Activate (10032020): a
    // redirected Type's handler Activate runs first; an accepted one ends
    // Launch through the common tail, a refused one falls through to the
    // stock case, which then reports no Ares activation.
    let redirect_refused = if ares && sw.kind.is_ares_redirect() {
        let activated = match sw.kind {
            SuperWeaponKind::ChronoSphere => {
                super::chronosphere::ares_launch_source(
                    sim,
                    rules,
                    owner,
                    sw_type_id,
                    sw,
                    (rx, ry),
                );
                true
            }
            SuperWeaponKind::ChronoWarp => super::chronosphere::ares_launch_warp(
                sim,
                rules,
                owner,
                sw_type_id,
                (rx, ry),
                overlay_registry,
                frame_effects,
            ),
            SuperWeaponKind::MultiMissile => {
                super::nuke::ares_launch(sim, rules, owner, sw_type_id, sw, (rx, ry))
            }
            SuperWeaponKind::PsychicDominator => {
                super::psychic_dominator::ares_launch(sim, rules, owner, sw_type_id, sw, (rx, ry));
                true
            }
            SuperWeaponKind::SpyPlane => {
                super::spy_plane::ares_launch(
                    sim,
                    rules,
                    owner,
                    sw_type_id,
                    sw,
                    (rx, ry),
                    frame_effects,
                );
                true
            }
            SuperWeaponKind::LightningStorm => {
                super::lightning_storm::ares_launch(sim, rules, owner, sw_type_id, sw, (rx, ry))
            }
            SuperWeaponKind::ParaDrop | SuperWeaponKind::AmerParaDrop => {
                super::paradrop::ares_launch(
                    sim,
                    rules,
                    owner,
                    sw_type_id,
                    sw,
                    (rx, ry),
                    frame_effects,
                )
            }
            SuperWeaponKind::GeneticConverter => {
                super::genetic_converter::ares_launch(
                    sim,
                    rules,
                    owner,
                    sw_type_id,
                    sw,
                    (rx, ry),
                    overlay_registry,
                    frame_effects,
                );
                true
            }
            _ => false,
        };
        if activated {
            super::finish_ares_launch(sim, rules, owner, sw_type_id, (rx, ry));
            return true;
        }
        true
    } else {
        false
    };
    // Every native stock case except ChronoWarp begins with rawReady+6F
    // (ForceShield6CD072..6CD077; case4 starts6CC4B2 with radar production).
    // Drain ClickFire can reach Launch after Reset retained state1 but cleared
    // rawReady. Keep the earlier timer transition while skipping case effects.
    let native_kind = sw.kind.native_index();
    if (0..12).contains(&native_kind)
        && native_kind != 4
        && !super::is_charged(sim, owner, sw_type_id)
    {
        return false;
    }
    match sw.kind {
        // Launch6CC3A3..6CC3AC: the constructor's -1 exits without case
        // effects. ClickFire's ordinary/one-time finish still runs afterwards.
        SuperWeaponKind::NoEffect => {}
        // Stock Launch has no case for an Ares type: without Ares these do
        // nothing, and Fire_SW has refused them before the click.
        SuperWeaponKind::GenericWarhead
        | SuperWeaponKind::UnitDelivery
        | SuperWeaponKind::HunterSeeker
        | SuperWeaponKind::Protect
        | SuperWeaponKind::Battery
        | SuperWeaponKind::SonarPulse
        | SuperWeaponKind::EMPulse
        | SuperWeaponKind::Firestorm
        | SuperWeaponKind::DropPod
            if !ares => {}
        SuperWeaponKind::HunterSeeker => {
            let activated = super::hunter_seeker::launch(
                sim,
                rules,
                owner,
                sw_type_id,
                sw,
                overlay_registry,
                frame_effects,
            );
            if activated {
                super::finish_ares_launch(sim, rules, owner, sw_type_id, (rx, ry));
            }
            return activated;
        }
        // SW_DropPod::Activate (10070E40) refuses only an empty type list.
        SuperWeaponKind::DropPod => {
            let activated = super::drop_pod::launch(
                sim,
                rules,
                owner,
                sw,
                (rx, ry),
                overlay_registry,
                frame_effects,
            );
            if activated {
                super::finish_ares_launch(sim, rules, owner, sw_type_id, (rx, ry));
            }
            return activated;
        }
        SuperWeaponKind::UnitDelivery => {
            let activated = super::unit_delivery::launch(
                sim,
                rules,
                owner,
                sw_type_id,
                sw,
                (rx, ry),
                overlay_registry,
                frame_effects,
            );
            if activated {
                super::finish_ares_launch(sim, rules, owner, sw_type_id, (rx, ry));
            }
            return activated;
        }
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
        SuperWeaponKind::GenericWarhead => {
            let activated = super::generic_warhead::launch(
                sim,
                rules,
                owner,
                sw,
                (rx, ry),
                overlay_registry,
                frame_effects,
            );
            if activated {
                super::finish_ares_launch(sim, rules, owner, sw_type_id, (rx, ry));
            }
            return activated;
        }
        SuperWeaponKind::Protect => {
            super::protect::launch(
                sim,
                rules,
                owner,
                sw_type_id,
                sw,
                (rx, ry),
                overlay_registry,
                frame_effects,
            );
            super::finish_ares_launch(sim, rules, owner, sw_type_id, (rx, ry));
            return true;
        }
        SuperWeaponKind::SonarPulse => {
            super::sonar_pulse::launch(sim, rules, owner, sw, (rx, ry));
            super::finish_ares_launch(sim, rules, owner, sw_type_id, (rx, ry));
            return true;
        }
        SuperWeaponKind::Battery => {
            super::battery::activate(sim, rules, owner, sw);
            super::finish_ares_launch(sim, rules, owner, sw_type_id, (rx, ry));
            return true;
        }
        SuperWeaponKind::Firestorm => {
            // ClickFire's isPlayer (Fire_SW passes `this == PlayerPtr`),
            // projected as a human-controlled House.
            let is_player = sim
                .houses
                .get(&owner)
                .is_some_and(|house| house.is_controlled_by_human(sim.session.game_mode_nonzero()));
            super::firewall::activate(
                sim,
                rules,
                owner,
                is_player,
                overlay_registry,
                frame_effects,
            );
            super::finish_ares_launch(sim, rules, owner, sw_type_id, (rx, ry));
            return true;
        }
        SuperWeaponKind::EMPulse => {
            super::emp_pulse::launch(
                sim,
                rules,
                owner,
                sw_type_id,
                sw,
                (rx, ry),
                overlay_registry,
                frame_effects,
            );
            super::finish_ares_launch(sim, rules, owner, sw_type_id, (rx, ry));
            return true;
        }
    }
    if redirect_refused {
        return false;
    }
    true
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
