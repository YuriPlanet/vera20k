//! Phobos warhead critical hits and the per-target warhead pass that carries
//! them.
//!
//! Native identity: `WarheadTypeExt::Detonate`'s early `TransactMoney` block
//! (`src/Ext/WarheadType/Detonate.cpp:102..111`), its `LaunchSW` block
//! (`:121..150`, `superweapon::launch_sw`) and its per-target
//! prefix and pass (`156..199`), `DetonateOnOneUnit`
//! (`201..275`), `GetCritChance`/`ApplyCrit` (`525..608`, `735..778`), called
//! from the `BulletClass_Detonate @ 0x46920B` and `MapClass_DamageArea
//! @ 0x489286` hooks. Both hooks run at the entry of those routines, before
//! the special/ordinary arm split and before the native area damage: the pass
//! visits its own `getCellSpreadItemsExt` collection, so it runs even for a
//! special arm (IvanBomb, MindControl, Temporal, Locomotor, ...) and before
//! SpawnShrapnel's draws.
//!
//! RESIDUALS:
//! - `PossibleCellSpreadDetonate` is reproduced member-for-member (see the gate
//!   in [`detonate`]), so a warhead setting only a member whose effect is not
//!   ported still draws the two buffers. Those unported arms are
//!   `IvanBomb.Detonate`'s fused-bomb detonation, `RemoveMindControl`'s silent
//!   flag, the shield respawn/self-healing timers and `BuildingSell`'s refund;
//!   the remaining members are ported and applied.
//! - `Detonate`'s `InterceptBullets` arm is not ported, so a warhead that
//!   sets it misses that arm. The other earlier per-house arms run here:
//!   `CreateGap` (`Simulation::apply_warhead_create_gap`), `Reveal`
//!   (`Detonate.cpp:82-100`, including its `Detonate.cpp:88` gate) and
//!   `SpawnsCrate.Types`' `RandomDouble`; `TransactMoney`
//!   (`apply_transact_money`) and `LaunchSW` (`superweapon::launch_sw`)
//!   are also ported.
//! - `CanAffectTarget`'s `AffectsAbovePercent`/`AffectsBelowPercent`,
//!   `AffectsVeterancy` and `AffectsInvokerOnly` gates are unread and pass.
//! - The pass's `getCellSpreadItemsExt` walk carries the warhead's
//!   `CellSpread.Cylinder` flag; in the helper that flag alters only the
//!   ground-Structure cell-center and aircraft distances.
//! - `Crit.Warhead`'s full detonation creates a projectile with no
//!   `Projectile=` type, so the default bullet type's `Cluster` is not read.
//! - `Crit.ExtraDamage.ApplyFirepowerMult` reads the owner's live multiplier;
//!   native uses the bullet's firepower captured at launch.
//! - Crit animations set the owner house but no invoker (VERA stores no anim
//!   invoker on this branch).
//! - `Crit.SuppressWhenIntercepted` is read but VERA tracks no intercepted
//!   bullet, so the bullet's `false` value never suppresses.
//! - `ApplyPerTargetEffectsOnDetonate=no`: native skips this prefix and runs
//!   `DetonateOnOneUnit` from `TechnoClass::ReceiveDamage` for every target
//!   that receives non-zero damage, with the attacker's house. VERA keeps the
//!   pass here on the same targets with per-target dice and no prefix draws,
//!   so the targets and the order against the receivers differ.
//! - Phobos gates its `MapClass_DamageArea` hook on a per-`WarheadTypeExt`
//!   latch, `InDamageArea`: `true` from the constructor
//!   (`Ext/WarheadType/Body.h:537`), set `false` right after the bullet hook's
//!   own `Detonate` (`Hooks.cpp:18..19`) so the native area damage the same
//!   bullet triggers does not run the pass a second time, and set back `true`
//!   at `BulletClass_Logics_Extras` (`0x469AA4`,
//!   `Ext/Bullet/Hooks.DetonateLogics.cpp:556`) for the detonations that
//!   follow. VERA has no per-type latch: every direct area call runs the pass,
//!   so a warhead detonated both by a bullet and by an `DamageArea` reached
//!   from that same detonation can visit its targets twice. Porting the latch
//!   would add cross-call mutable state to a type projection; no retail
//!   warhead is known to reach both paths.

use super::effects::DeathEffects;
use super::receiver_call::EntityDamageEvent;
use super::world_receiver::{
    ReceiverRun, apply_area_damage_in_run_with_target, commit_entities, commit_projectiles,
};
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::phobos_crit::{AffectedTarget, PhobosAbilityFlags};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::rules::warhead_type::WarheadType;
use crate::sim::combat::veterancy::{VeterancyRank, has_weapon_ability};
use crate::sim::credit_income::transact_money;
use crate::sim::intern::InternedId;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::Simulation;

/// The per-detonation crit state `WarheadTypeExt::Detonate` sets before its
/// per-target pass: the firer's chance once, and the dice buffer the whole
/// detonation shares unless `Crit.ApplyChancePerTarget` is set.
pub(crate) struct CritRoll {
    chance: f64,
    buffer: f64,
    per_target: bool,
    /// `ReturnWarhead_RandomBuffer`: drawn once per detonation unless
    /// `ReturnWarhead.ApplyChancePerTarget` is set.
    return_buffer: f64,
}

/// `WarheadTypeExt::Detonate`: the early `TransactMoney` block
/// (`Detonate.cpp:102..111`, [`apply_transact_money`]), then the per-target
/// prefix and pass (`Detonate.cpp:156..199`): `Crit_CurrentChance = GetCritChance(owner)`;
/// when the warhead has a per-target effect or a chance, the crit and
/// `ReturnWarhead` buffers and `Crit.ActiveChanceAnims`; then
/// `DetonateOnOneUnit` on every `getCellSpreadItemsExt` item of a cell-spread
/// warhead, or on `intended_target` (the bullet's target, or `DamageAreaTarget`)
/// within 64 leptons of `impact`. `source_house` is the hook's house; a direct
/// `DamageArea` without one takes the owner's (`0x489286`). `damage` is the
/// firing bullet's Health for the PenetratesTransport arm, 0 without a
/// bullet. Returns `Crit_Active`, which the damage-animation selection reads.
#[cfg(feature = "phobos")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn detonate(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    warhead_ref: InternedId,
    source_id: u64,
    source_house: Option<InternedId>,
    impact: ProjectileCoord,
    intended_target: Option<u64>,
    // The firing bullet's Health (`WarheadTypeExt::Detonate`'s `damage`); 0
    // for the pure `MapClass::DamageArea` call, which carries no bullet.
    damage: i32,
    // `bulletWasIntercepted`: the detonating bullet has the Intercepted
    // status, which `Crit.SuppressWhenIntercepted` can veto.
    intercepted: bool,
    effects: &mut DeathEffects,
) -> bool {
    let crit = &warhead.crit;
    let globals = rules.crit_globals();
    // `DetonateOnOneUnit`'s crit gate: `Crit_CurrentChance > 0 &&
    // (!Crit_SuppressWhenIntercepted.Get(global) || !bulletWasIntercepted)`.
    let suppress_crit = intercepted
        && crit
            .suppress_when_intercepted
            .unwrap_or(globals.suppress_when_intercepted());
    // `RemoveMindControl` can hand the detonation a different House for the
    // arms that follow (`Detonate.cpp:206-208`), so the local is mutable.
    #[cfg_attr(not(feature = "phobos"), allow(unused_mut))]
    let mut house = source_house.or_else(|| {
        world
            .substrate
            .entities
            .get(source_id)
            .map(|source| source.owner())
    });
    // `WarheadTypeExt::Detonate`'s CreateGap block (`Detonate.cpp:51-80`):
    // before Reveal, a positive radius conceals for enemies of the firing
    // house; a negative value reshrouds every non-allied house.
    #[cfg(feature = "phobos")]
    if warhead.create_gap != 0
        && let Some(owner) = house
    {
        let context = crate::sim::world::UninitContext::new(rules, overlay_registry);
        let _ = world.apply_warhead_create_gap(
            owner,
            [impact.x, impact.y, impact.z],
            warhead.create_gap,
            rules,
            context,
        );
    }
    // `WarheadTypeExt::Detonate`'s Reveal block (`Detonate.cpp:82-100`): a
    // positive radius reveals for the firing house on the client maps its
    // RevealArea gate admits, but only behind `Detonate.cpp:88`'s gate on the
    // current player; a negative value reveals the whole map for that house.
    // It runs before TransactMoney.
    #[cfg(feature = "phobos")]
    if warhead.reveal != 0
        && let Some(owner) = house
    {
        if warhead.reveal < 0 {
            world.reveal_map_for_owner(owner, rules);
        } else if world.warhead_reveal_admits_current_player(owner) {
            let config = world.sight_reveal_config(rules);
            let viewers = crate::sim::vision::direct_reveal_viewers(
                world.house_alliances.as_map(),
                owner,
                &config,
                &world.interner,
                &world.fog_viewers(),
            );
            if !viewers.is_empty() {
                let radius = warhead.reveal.clamp(0, i32::from(u16::MAX)) as u16;
                let cell = ((impact.x / 256) as u16, (impact.y / 256) as u16);
                let context = crate::sim::world::UninitContext::new(rules, overlay_registry);
                let notification = world.session.current_house();
                for final_pass in [false, true] {
                    let _ = world.reveal_area(
                        &viewers,
                        cell,
                        impact.z,
                        radius,
                        None,
                        final_pass,
                        &config,
                        context,
                        crate::sim::vision::MapRevealProjection::Transient,
                        notification,
                    );
                }
            }
        }
    }
    // `WarheadTypeExt::Detonate`'s `TransactMoney` block runs before the crit
    // prefix and regardless of whether this warhead can crit
    // (`Ext/WarheadType/Detonate.cpp:102-111`).
    apply_transact_money(
        world,
        rules,
        warhead,
        house,
        source_id,
        impact,
        run.effects(),
    );
    // `WarheadTypeExt::Detonate`'s SpawnsCrate block (`Detonate.cpp:113-118`):
    // after TransactMoney and before LaunchSW, one Scenario draw picks a
    // weighted crate and `PlacePowerupCrate` drops it at the impact cell.
    #[cfg(feature = "phobos")]
    apply_spawns_crate(
        world,
        rules,
        overlay_registry,
        warhead,
        house,
        impact,
        run.effects(),
    );
    // `WarheadTypeExt::Detonate`'s LaunchSW block (`Detonate.cpp:121-150`):
    // after the TransactMoney block and before the crit prefix.
    #[cfg(feature = "phobos")]
    crate::sim::superweapon::launch_sw::apply_on_detonation(
        world,
        rules,
        house,
        impact,
        warhead,
        overlay_registry,
        run.effects(),
    );
    let chance = get_crit_chance(world, rules, warhead, source_id);
    // `PossibleCellSpreadDetonate` (`Ext/WarheadType/Body.cpp`'s
    // "List all Warheads here that respect CellSpread", read by
    // `WarheadTypeExt::Detonate`): `RemoveDisguise`, `RemoveMindControl`,
    // `Shield.Break`/`Respawn.Duration`/`SelfHealing.Duration`/
    // `AttachTypes`/`RemoveTypes`/`RemoveAll`, `Convert.*`, the AttachEffect
    // lists, `ChangeOwner`, `BuildingSell`, `BuildingUndeploy`,
    // `ReverseEngineer`, `ReturnWarhead`, `PenetratesTransport.Level`,
    // `Taunt`, `Ammo` and `IvanBomb.Detonate`. That list, or a live
    // `Crit_CurrentChance`, is the only way into `DetonateOnOneUnit` and thus
    // into the pass's two buffer draws: a warhead whose every member is off
    // draws nothing.
    let attach = !warhead.attach_effect.is_empty();
    let penetrates = warhead.phobos_penetrates_transport.level > 0;
    let shield_modifiers = warhead.shield.has_per_target_modifier();
    #[cfg(feature = "phobos")]
    let convert_pairs = !warhead.phobos_convert_pairs.is_empty();
    #[cfg(not(feature = "phobos"))]
    let convert_pairs = false;
    #[cfg(feature = "phobos")]
    let return_warhead = warhead
        .phobos_return_warhead
        .warhead
        .as_deref()
        .is_some_and(|name| rules.warhead(name).is_some());
    #[cfg(not(feature = "phobos"))]
    let return_warhead = false;
    #[cfg(feature = "phobos")]
    let change_owner = warhead.phobos_change_owner;
    #[cfg(not(feature = "phobos"))]
    let change_owner = false;
    // The remaining members. `IvanBomb.Detonate` is one: its fused bombs are
    // not ported, but the member still draws the two buffers native makes.
    // `CanKill` and the `Prevent*Escape` arms are not members — they run in
    // ReceiveDamage and the death arms, so they never force this loop.
    #[cfg(feature = "phobos")]
    let warhead_extras = warhead.phobos_extras.has_per_target_effect()
        || warhead.ammo != 0
        || warhead.remove_disguise
        || warhead.reverse_engineer
        || warhead.taunt
        || warhead.phobos_ivan_bomb_detonate;
    #[cfg(not(feature = "phobos"))]
    let warhead_extras = false;
    if !(attach
        || penetrates
        || chance > 0.0
        || shield_modifiers
        || convert_pairs
        || return_warhead
        || change_owner
        || warhead_extras)
    {
        return false;
    }
    let on_detonate = warhead
        .apply_per_target_effects_on_detonate
        .unwrap_or(globals.apply_per_target_effects_on_detonate());
    let mut roll = CritRoll {
        chance,
        buffer: 0.0,
        per_target: true,
        return_buffer: 0.0,
    };
    if on_detonate {
        roll.per_target = crit.apply_chance_per_target(globals);
        if !roll.per_target {
            roll.buffer = world.scenario_rng.random_double();
        }
        // `ReturnWarhead_RandomBuffer`: drawn for every such detonation and
        // read by [`apply_return_warhead`]'s shared-dice arm.
        if !crit.return_warhead_apply_chance_per_target(globals) {
            roll.return_buffer = world.scenario_rng.random_double();
        }
        // One random pick per detonation, even when every target later fails a
        // gate: `CreateRandomAnim` draws before it constructs.
        if !crit.active_chance_anims().is_empty() && chance > 0.0 {
            let index = world.scenario_rng.next_range_i32_inclusive(
                0,
                i32::try_from(crit.active_chance_anims().len() - 1)
                    .expect("native anim count fits i32"),
            );
            let name = crit.active_chance_anims()[index as usize].clone();
            spawn_anim_at_world_coord(world, rules, &name, impact, house);
        }
    }
    let invoker = (source_id != 0).then_some(source_id);
    let mut active = false;
    for target_id in detonation_targets(world, rules, warhead, impact, intended_target) {
        if !can_detonate_on(world, rules, warhead, house, target_id, invoker) {
            continue;
        }
        // Phobos `WarheadTypeExt::ExtData::ApplyRemoveMindControl`
        // (`Detonate.cpp:207-208,477-486`): first, because it changes the
        // target's House for every arm below. The freed target keeps the owner
        // `FreeUnit` restored, and that House replaces the detonation's own.
        #[cfg(feature = "phobos")]
        if warhead.phobos_extras.remove_mind_control
            && let Some(controller) = world
                .substrate
                .entities
                .get(target_id)
                .and_then(|target| target.mind_control.controller())
        {
            let silent = warhead
                .phobos_extras
                .mind_control_silent(rules.phobos_globals.remove_mind_control_silent);
            world.free_unit_with_sound(
                controller,
                target_id,
                rules,
                overlay_registry,
                run.effects(),
                !silent,
            );
            house = world
                .substrate
                .entities
                .get(target_id)
                .map(|target| target.owner());
        }
        // Phobos `WarheadTypeExt::ExtData::ApplyOwnerChange`
        // (`Detonate.cpp:207-211,488-522`): after RemoveMindControl and
        // before ApplyConvert, the firing House takes the target; a
        // `SetAsMindControl` capture respects psionic immunity, an existing
        // mind-control link, and may ring the new capture.
        #[cfg(feature = "phobos")]
        if warhead.phobos_change_owner {
            apply_change_owner(
                world,
                rules,
                overlay_registry,
                warhead,
                house,
                target_id,
                run.effects(),
            );
        }
        // Phobos `WarheadTypeExt::ApplyConvert` (`Detonate.cpp:215,669-678`):
        // before the PenetratesTransport arm, a Foot target converts through
        // the first applicable pair.
        #[cfg(feature = "phobos")]
        world.phobos_apply_type_convert_pairs(
            target_id,
            &warhead.phobos_convert_pairs,
            house,
            rules,
            run.effects(),
        );
        // Phobos `WarheadTypeExt::ExtData::ApplyBuildingUndeploy`
        // (`Detonate.cpp:213-214,266-352`): after `RemoveMindControl`,
        // `ChangeOwner` and `ApplyConvert`, a hit building is sold, else
        // packed up into its `UndeploysInto=` unit.
        #[cfg(feature = "phobos")]
        if warhead.phobos_extras.building_sell || warhead.phobos_extras.building_undeploy {
            super::building_sell::apply(
                world,
                rules,
                overlay_registry,
                warhead,
                target_id,
                run.effects(),
            );
        }
        // `DetonateOnOneUnit`'s `ApplyAmmoModifier(pTarget)`
        // (`Detonate.cpp:221-222,923-932`): before RemoveDisguise, the delta
        // clamps into `0..type->Ammo`.
        #[cfg(feature = "phobos")]
        if warhead.ammo != 0 {
            let max_ammo = world
                .substrate
                .entities
                .get(target_id)
                .and_then(|target| world.interner.object_type(target.type_ref(), rules))
                .map_or(0, |object| object.ammo);
            if let Some(ammo) = world
                .substrate
                .entities
                .get_mut(target_id)
                .and_then(|target| target.ammo_mut())
            {
                ammo.apply_warhead_delta(warhead.ammo, max_ammo);
            }
        }
        // `DetonateOnOneUnit`'s `ApplyRemoveDisguise(pTarget)`
        // (`Detonate.cpp:224-225,466-475`): after the one-time Ammo arm and
        // before ReverseEngineer/ReturnWarhead. Infantry clears the `Disguised`
        // flag only; a Unit also clears the recorded identity.
        #[cfg(feature = "phobos")]
        if warhead.remove_disguise {
            let frame = world.session.binary_frame() as u32;
            if let Some(target) = world.substrate.entities.get_mut(target_id)
                && target
                    .disguise
                    .as_ref()
                    .is_some_and(|disguise| disguise.is_disguised())
            {
                if target.category() == EntityCategory::Unit {
                    target.disguise_mut(frame).clear_unit();
                } else {
                    target.disguise_mut(frame).clear_flag();
                }
            }
        }
        // `DetonateOnOneUnit`'s `ApplyReverseEngineer(pHouse, pTarget)`
        // (`Detonate.cpp:227-228,260-264`): after the one-time Ammo and
        // RemoveDisguise arms and before ReturnWarhead. A firing house that is
        // not `MultiplayPassive` records the victim's type through Ares'
        // HouseExt::ExtData::ReverseEngineer (10022360), which only exists
        // with Ares.
        #[cfg(all(feature = "phobos", feature = "ares"))]
        if warhead.reverse_engineer
            && let Some(owner) = house
            && !world
                .houses
                .get(&owner)
                .is_some_and(|house| house.multiplay_passive())
            && let Some(type_ref) = world
                .substrate
                .entities
                .get(target_id)
                .map(|target| target.type_ref())
            && let Some(victim_type) = world.object_type(type_ref, rules)
        {
            world.ares_reverse_engineer_house(owner, victim_type, rules);
        }
        // `DetonateOnOneUnit`'s `ApplyReturnWarhead(pHouse, pTarget, pOwner)`
        // (`Detonate.cpp:229-231`): after the displacement arms and before
        // `PenetratesTransport`.
        #[cfg(feature = "phobos")]
        super::phobos_return_warhead::apply(
            world,
            run,
            rules,
            overlay_registry,
            warhead,
            roll.return_buffer,
            on_detonate,
            house,
            source_id,
            target_id,
            effects,
        );
        // `DetonateOnOneUnit`'s PenetratesTransport arm (`Detonate.cpp:233`):
        // before the attach-effect and crit blocks, the shot reaches the
        // target's passengers. Shields resolve inside the passenger receiver.
        super::penetrates_transport::apply(
            world,
            run,
            rules,
            overlay_registry,
            warhead,
            warhead_ref,
            source_id,
            house,
            target_id,
            damage,
            impact,
            effects,
        );
        // `DetonateOnOneUnit`'s Taunt arm (`Detonate.cpp:236-237`): after the
        // penetrate/displacement effects and before the shield modifiers, a
        // hit target is ordered to `Mission::Attack` the firing techno.
        #[cfg(feature = "phobos")]
        if warhead.taunt && source_id != 0 {
            let _ = world.override_mission_on_damage_response(
                target_id,
                source_id,
                rules,
                overlay_registry,
                run.effects(),
            );
        }
        // `DetonateOnOneUnit`'s `IvanBombDetonate(pOwner, pTarget)` arm
        // (`Detonate.cpp:239-240`): right after the Taunt arm and before the
        // shield modifiers, the bombs the target carries have their fuse cut
        // to the current frame.
        #[cfg(feature = "phobos")]
        if warhead.phobos_ivan_bomb_detonate {
            world.phobos_ivan_bomb_detonate(
                target_id,
                (source_id != 0).then_some(source_id),
                warhead,
            );
        }
        // `DetonateOnOneUnit`'s `ApplyShieldModifiers(pTarget)`
        // (`Detonate.cpp:240`): after the PenetratesTransport arm and before
        // the attach-effect and crit blocks, so the receiver's damage hits
        // the removed, replaced or reconfigured shield.
        let shield_shots =
            crate::sim::shield::apply_shield_modifiers(world, rules, warhead, target_id);
        if !shield_shots.is_empty() {
            // `FireWeaponAtSelf`'s `WeaponTypeExt::DetonateAt` shot: the break
            // weapon detonates on the shield owner inside the per-target
            // pass, committed through this run.
            let detonations = world.shield_break_detonations(shield_shots, rules);
            let commit = commit_projectiles(world, run, &detonations, rules, overlay_registry);
            effects.append(commit.effects);
            effects
                .under_attack_events
                .extend(commit.under_attack_events);
        }
        if attach {
            world.apply_warhead_attach_effects(
                target_id,
                warhead_ref,
                invoker,
                house,
                rules,
                run.effects(),
                overlay_registry,
            );
        }
        if chance > 0.0
            && !suppress_crit
            && apply_one(
                world,
                run,
                rules,
                overlay_registry,
                warhead,
                warhead_ref,
                source_id,
                house,
                &roll,
                target_id,
                effects,
            )
        {
            active = true;
        }
    }
    active
}

/// Phobos `WarheadTypeExt::Detonate`'s `TransactMoney` block
/// (`Ext/WarheadType/Detonate.cpp:102-111`): `pHouse->TransactMoney(amount)`
/// on the decided house — the bullet owner's house, else `FirerHouse` for the
/// `BulletClass_Detonate @ 0x46920B` hook, or the passed source house for
/// `MapClass_DamageArea @ 0x489286`. The credits go to the firing house, not
/// the target's: Phobos documents the key as "warheads can now give credits to
/// its owner at impact". `HouseClass::TransactMoney` is the YRpp wrapper over
/// `Add_Credits @ 0x004F9950` for a positive amount and `Spend_Money
/// @ 0x004F9790` for a negative one.
///
/// With `TransactMoney.Display` the label is `FlyingStrings::AddMoneyString(
/// amount, nullptr, pHouse, Houses, coords, Offset)`: `nullptr` source, so no
/// visual-character test; `coords` is the firer's `Location` under `AtFirer`
/// when the firer is known, otherwise the impact point.
fn apply_transact_money(
    world: &mut Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    house: Option<InternedId>,
    firer_id: u64,
    impact: ProjectileCoord,
    effects: crate::sim::world::FrameEffects<'_>,
) {
    let settings = &warhead.transact_money;
    if settings.amount == 0 {
        return;
    }
    let Some(house) = house else {
        return;
    };
    transact_money(&mut world.houses, house, settings.amount);
    if !settings.display {
        return;
    }
    let coord = if settings.display_at_firer && firer_id != 0 {
        world
            .substrate
            .entities
            .get(firer_id)
            .map_or(impact, |firer| {
                let location = crate::sim::movement::ground_pose::object_location(
                    firer,
                    world.resolved_terrain.as_ref(),
                );
                ProjectileCoord::new(location.x, location.y, location.z)
            })
    } else {
        impact
    };
    effects.emit_bounty_text(
        world,
        rules,
        crate::sim::flying_text::BountyText {
            entity_id: None,
            amount: settings.amount,
            coord,
            z_offset: 0,
            display_to: Some(settings.display_houses),
            display_owner: Some(house),
            pixel_offset: settings.display_offset,
        },
    );
}

/// `DetonateOnOneUnit`'s target gate (`Detonate.cpp:203..207`): a live,
/// placed, non-sinking target not being warped out, then
/// `WarheadTypeExt::CanTargetHouse` and `CanAffectTarget`.
#[cfg(feature = "phobos")]
fn can_detonate_on(
    world: &Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    house: Option<InternedId>,
    target_id: u64,
    invoker: Option<u64>,
) -> bool {
    let Some(target) = world.substrate.entities.get(target_id) else {
        return false;
    };
    if target.lifecycle.in_limbo()
        || !target.lifecycle.object_alive()
        || target.health.current() <= 0
        || target.sinking.is_active()
        || target.is_warped_out()
    {
        return false;
    }
    if !crate::sim::shield::can_target_house(warhead, house, target.owner(), world) {
        return false;
    }
    can_affect_target(world, rules, warhead, target, invoker)
}

/// `WarheadTypeExt::CanAffectTarget` (`WarheadType/Body.cpp:37..57`): the
/// health, veterancy and invoker trigger gates, then with
/// `EffectsRequireVerses` the `AffectsAir`/`AffectsGround` split on `IsInAir`
/// (`0x005F6B90`) and a non-zero `GetWarheadVersusArmor`. Under Ares that is
/// the raw Verses; without it `GetTotalDamage(100, WH, armor, 0) / 100`.
#[cfg(feature = "phobos")]
pub(crate) fn can_affect_target(
    world: &Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    target: &crate::sim::game_entity::GameEntity,
    invoker: Option<u64>,
) -> bool {
    let object = world.interner.object_type(target.type_ref(), rules);
    if !super::warhead_triggers_allow(
        warhead,
        target,
        object,
        invoker,
        rules
            .phobos_globals
            .affects_invoker_only_ignore_invoker_state,
    ) {
        return false;
    }
    if !warhead.effects_require_verses {
        return true;
    }
    let in_air = crate::sim::movement::air_movement::is_high_flying(
        target,
        world.resolved_terrain.as_ref(),
        Some((rules, &world.interner)),
    );
    if (in_air && !warhead.affects_air) || (!in_air && !warhead.affects_ground) {
        return false;
    }
    let Some(target_type) = world.interner.object_type(target.type_ref(), rules) else {
        return false;
    };
    let armor = crate::sim::shield::warhead_versus_armor(target, rules, warhead, target_type);
    let verses = warhead.interaction(armor).multiplier();
    #[cfg(feature = "ares")]
    {
        f64::from_bits(verses.bits()) != 0.0
    }
    #[cfg(not(feature = "ares"))]
    {
        super::damage::kernel::apply_warhead_damage(
            100,
            warhead.cell_spread_f64,
            warhead.percent_at_max_f64,
            verses,
            0,
            world.session.no_damage(),
            rules.combat_damage.max_damage,
            warhead.apply_modifiers_on_negative_damage(rules),
        ) != 0
    }
}

/// `WarheadTypeExt::GetCritChance` (`Detonate.cpp:745..791`): zero for a
/// warhead that cannot crit, else its `Crit.Chance`, the `CRITCHANCE` rank
/// ability's `VeteranCritChance` and the AttachEffect fold.
#[cfg(feature = "phobos")]
fn get_crit_chance(
    world: &Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    owner_id: u64,
) -> f64 {
    if !warhead.crit.can_crit() {
        return 0.0;
    }
    let mut chance = warhead.crit.chance();
    if owner_id == 0 {
        return chance;
    }
    let Some(owner) = world.substrate.entities.get(owner_id) else {
        return chance;
    };
    let Some(object) = world.interner.object_type(owner.type_ref(), rules) else {
        return chance;
    };
    let rank = owner.veterancy.rank_kind();
    if has_phobos_ability(rank, object, PhobosAbilityFlags::CRIT_CHANCE) {
        let veteran = object
            .veteran_crit_chance()
            .unwrap_or(rules.crit_globals().veteran_crit_chance());
        chance *= veteran.max(0.0);
    }
    // Phobos `GetCritChance`'s AttachEffect fold: every active effect whose
    // block is more than the identity multiplies the chance and adds its
    // extra chance, gated by the Allow/Disallow warhead lists.
    if !owner.phobos_stat_multipliers().has_crit_modifiers {
        return chance;
    }
    let effects = rules.attach_effect_types().types();
    let mut extra_chance = 0.0;
    for (_, effect) in owner.phobos_attach_effects().active_effects(effects) {
        if !effect.has_crit_modifiers() {
            continue;
        }
        if !effect.crit_allow_warheads.is_empty()
            && !effect
                .crit_allow_warheads
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&warhead.id))
        {
            continue;
        }
        if effect
            .crit_disallow_warheads
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&warhead.id))
        {
            continue;
        }
        chance *= effect.crit_multiplier.max(0.0);
        extra_chance += effect.crit_extra_chance;
    }
    chance + extra_chance
}

#[cfg(feature = "phobos")]
pub(crate) fn has_phobos_ability(rank: VeterancyRank, object: &ObjectType, slot: usize) -> bool {
    match rank {
        VeterancyRank::Rookie => false,
        VeterancyRank::Veteran => object.phobos_abilities().has(slot, false),
        VeterancyRank::Elite => object.phobos_abilities().has(slot, true),
    }
}

/// One `ApplyCrit`: dice, immunity/health/house/cell/techno gates in native
/// order, then the crit's animations and damage.
#[cfg(feature = "phobos")]
#[allow(clippy::too_many_arguments)]
fn apply_one(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    warhead_ref: InternedId,
    source_id: u64,
    source_house: Option<InternedId>,
    roll: &CritRoll,
    target_id: u64,
    effects: &mut DeathEffects,
) -> bool {
    // `InApplyCrit`: a crit whose own damage detonates this same warhead
    // cannot crit again while the outer apply is running.
    if run.crit_warheads.contains(&warhead_ref) {
        return false;
    }
    let crit = &warhead.crit;
    // `ApplyChancePerTarget || !ApplyPerTargetEffectsOnDetonate` picks a fresh
    // draw; the shared buffer otherwise.
    let dice = if roll.per_target {
        world.scenario_rng.random_double()
    } else {
        roll.buffer
    };
    if roll.chance < dice {
        return false;
    }
    let Some(target) = world.substrate.entities.get(target_id) else {
        return false;
    };
    let target_object = world.interner.object_type(target.type_ref(), rules);
    if target_object.is_some_and(|object| object.immune_to_crit()) {
        return false;
    }
    if target_object.is_some_and(|object| {
        has_phobos_ability(
            target.veterancy.rank_kind(),
            object,
            PhobosAbilityFlags::CRIT_IMMUNE,
        )
    }) {
        return false;
    }
    if target.shield.as_ref().is_some_and(|state| {
        state.is_active()
            && rules
                .shield_type(state.type_name())
                .is_some_and(|ty| ty.immune_to_crit())
    }) {
        return false;
    }
    let strength = target_object.map_or(0, |object| object.strength);
    if !super::health_in_threshold(
        target.health.current(),
        strength,
        crit.affects_above_percent(),
        crit.affects_below_percent(),
    ) {
        return false;
    }
    if let Some(source) = source_house {
        let target_owner = target.owner();
        if !super::can_target_house(
            crit.affects_house(),
            source,
            target_owner,
            &world.houses,
            &world.house_alliances,
            &world.interner,
        ) {
            return false;
        }
    }
    let land = world.resolved_terrain.as_ref().and_then(|terrain| {
        terrain
            .cell(target.position.rx(), target.position.ry())
            .and_then(|cell| LandType::from_index(cell.land_type))
    });
    // `explicitEmptyCells = false`, no bridge consideration.
    if !cell_eligible(land, crit.affects_target(), None, false) {
        return false;
    }
    if !techno_eligible(
        target.category(),
        crit.affects_target(),
        world
            .interner
            .object_type(target.type_ref(), rules)
            .is_some_and(|object| object.is_vehicle()),
    ) {
        return false;
    }

    run.crit_warheads.push(warhead_ref);
    let applied = crit_effects(
        world,
        run,
        rules,
        overlay_registry,
        warhead,
        warhead_ref,
        source_id,
        source_house,
        target_id,
        effects,
    );
    run.crit_warheads.pop();
    applied
}

#[cfg(feature = "phobos")]
fn apply_spawns_crate(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    house: Option<InternedId>,
    impact: ProjectileCoord,
    effects: crate::sim::world::FrameEffects<'_>,
) {
    // `Detonate.cpp:49` encloses the SpawnsCrate block in `if (pHouse)`: a
    // house-less detonation draws nothing and places nothing.
    let Some(_house) = house else {
        return;
    };
    let settings = &warhead.phobos_spawns_crate;
    if settings.types.is_empty() {
        return;
    }
    let dice = world.scenario_rng.random_double();
    let Some(index) = super::phobos_extra_warheads::choose_one_weighted(dice, &settings.weights)
    else {
        return;
    };
    let Some(&powerup) = settings.types.get(index) else {
        return;
    };
    let Some(registry) = overlay_registry else {
        return;
    };
    let cell = (
        crate::util::lepton::lepton_to_cell(impact.x)
            .clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
        crate::util::lepton::lepton_to_cell(impact.y)
            .clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
    );
    if let Err(error) =
        crate::sim::crates::place_crate(world, rules, registry, cell, Some(powerup as u8), effects)
    {
        log::debug!("SpawnsCrate placement failed: {error}");
    }
}

#[cfg(feature = "phobos")]
#[allow(clippy::too_many_arguments)]
fn apply_change_owner(
    world: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    house: Option<InternedId>,
    target_id: u64,
    effects: crate::sim::world::FrameEffects<'_>,
) {
    let Some(house) = house else {
        return;
    };
    let is_mind_control = warhead.phobos_change_owner_set_as_mind_control;
    let immune = {
        let Some(target) = world.substrate.entities.get(target_id) else {
            return;
        };
        let psionic = is_mind_control
            && world
                .object_type(target.type_ref(), rules)
                .is_some_and(|object| object.immune_to_psionics);
        psionic || target.mind_control.is_mind_controlled()
    };
    if immune {
        return;
    }
    world.change_owner_with_rules(target_id, house, rules, overlay_registry, effects);
    if is_mind_control {
        // `MindControlledByAUnit` is Techno+0x2C4, VERA's
        // `MindControlLink::permanent`; `IsMindControlled` and the capture
        // admission read it, so a second `SetAsMindControl` hit refuses.
        if let Some(target) = world.substrate.entities.get_mut(target_id) {
            target.mind_control.mark_permanent_control();
        }
        if let Some(anim) = warhead.phobos_change_owner_mind_control_anim.as_deref() {
            world.attach_control_ring(target_id, anim, rules);
        }
    }
}

#[cfg(feature = "phobos")]
#[allow(clippy::too_many_arguments)]
fn crit_effects(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    warhead_ref: InternedId,
    source_id: u64,
    source_house: Option<InternedId>,
    target_id: u64,
    effects: &mut DeathEffects,
) -> bool {
    let crit = &warhead.crit;
    let globals = rules.crit_globals();
    let Some(target) = world.substrate.entities.get(target_id) else {
        return false;
    };
    let coord = crate::sim::movement::ground_pose::object_get_coords(
        target,
        world.resolved_terrain.as_ref(),
    );
    let anim_coord = crate::sim::anim_class::AnimWorldCoord {
        x: coord.x,
        y: coord.y,
        z: coord.z,
    };
    let owner_house = source_house.or_else(|| {
        world
            .substrate
            .entities
            .get(source_id)
            .map(|source| source.owner())
    });

    // `Crit.AnimList` at the affected target; the detonation's damage
    // animation only takes the list when `Crit.AnimOnAffectedTargets` is off.
    if crit.anim_on_affected_targets(globals) && !crit.anim_list().is_empty() {
        if crit.anim_list_create_all() {
            for name in crit.anim_list() {
                spawn_anim_at_coord(world, rules, name, anim_coord, owner_house);
            }
        } else {
            // `EMEffect || Crit_AnimList_PickRandom` selects a random entry;
            // otherwise index 0.
            let index = if warhead.em_effect || crit.anim_list_pick_random() {
                world.scenario_rng.next_range_i32_inclusive(
                    0,
                    i32::try_from(crit.anim_list().len() - 1).expect("native anim count fits i32"),
                )
            } else {
                0
            };
            let name = crit.anim_list()[index as usize].clone();
            spawn_anim_at_coord(world, rules, &name, anim_coord, owner_house);
        }
    }

    let mut damage = crit.extra_damage();
    if crit.extra_damage_apply_firepower_mult(globals)
        && let Some(owner) = world.substrate.entities.get(source_id)
    {
        let firepower = current_firepower_multiplier(world, rules, owner);
        damage = crate::sim::attach_effect::scale_i32(damage, firepower);
    }

    match crit
        .warhead()
        .map(|name| (name, crit.warhead_full_detonation()))
    {
        Some((name, true)) => {
            let Some(crit_warhead) = rules.warhead(name) else {
                return true;
            };
            let warhead_ref = world.interner.intern(name);
            let impact = ProjectileCoord::new(coord.x, coord.y, coord.z);
            let detonation = ProjectileDetonation {
                projectile_id: target_id,
                source_id,
                target: ProjectileTarget::Entity(target_id),
                impact,
                payload: ProjectilePayload::new(damage, warhead_ref, None, crit_warhead.bright),
                reason: ProjectileDetonationReason::CritWarhead,
            };
            let commit = commit_projectiles(world, run, &[detonation], rules, overlay_registry);
            effects.append(commit.effects);
            effects
                .under_attack_events
                .extend(commit.under_attack_events);
        }
        Some((name, false)) => {
            let Some(crit_warhead) = rules.warhead(name) else {
                return true;
            };
            // `DamageAreaWithTarget(coords, damage, owner, Crit.Warhead, true,
            // house, target)`: `MapClass::DamageArea` with the crit warhead and
            // affectsTiberium set; the target is the nested pass's
            // `DamageAreaTarget` when the crit warhead does not spread.
            let crit_ref = world.interner.intern(name);
            let impact = ProjectileCoord::new(coord.x, coord.y, coord.z);
            let (nested, mut pings, _crit_active, _area_result) =
                apply_area_damage_in_run_with_target(
                    world,
                    run,
                    rules,
                    overlay_registry,
                    impact,
                    damage,
                    crit_warhead,
                    true,
                    (source_id, source_house, crit_ref),
                    Some(target_id),
                );
            effects.append(nested);
            effects.under_attack_events.append(&mut pings);
        }
        None => {
            // `pTarget->ReceiveDamage(&damage, 0, warhead, owner, false,
            // false, house)` with the ORIGINAL warhead's id.
            let event =
                EntityDamageEvent::area(target_id, damage, 0, source_id, source_house, warhead_ref);
            let (nested, mut pings) =
                commit_entities(world, run, &[event], None, rules, overlay_registry);
            effects.append(nested);
            effects.under_attack_events.append(&mut pings);
        }
    }
    true
}

/// `TechnoExt::GetCurrentFirepowerMultiplier`: `Techno+0x158`-adjacent
/// `FirepowerMultiplier`, the house bias, the FIREPOWER rank stage and an
/// occupiable building's `OccupyDamageMultiplier=`.
#[cfg(feature = "phobos")]
pub(crate) fn current_firepower_multiplier(
    world: &Simulation,
    rules: &RuleSet,
    entity: &crate::sim::game_entity::GameEntity,
) -> f64 {
    use crate::rules::object_type::Ability;
    use crate::util::native_x87::NativeF64Bits;
    let f64_bits = |bits: NativeF64Bits| f64::from_bits(bits.bits());
    let mut multiplier = f64_bits(entity.crate_multipliers.firepower());
    multiplier *= world
        .houses
        .get(&entity.owner())
        .map_or(1.0, |house| f64_bits(house.firepower_bias()));
    if let Some(object) = world.interner.object_type(entity.type_ref(), rules) {
        if has_weapon_ability(entity.veterancy.rank_kind(), object, Ability::Firepower) {
            multiplier *= rules.general.veteran_combat;
        }
        if entity.category() == EntityCategory::Structure
            && object.can_be_occupied
            && object.can_occupy_fire
            && object.max_number_occupants > 0
        {
            multiplier *= f64::from(rules.garrison_rules.occupy_damage_multiplier);
        }
    }
    multiplier
}

/// `EnumFunctions::IsCellEligible`: the AllCells pair follows the target
/// cell's water flag; any other flag passes as long as it is not empty. A
/// fixture without terrain reads as land. `techno_content` is whether
/// `CellClass::GetContent` is a Techno when the caller passes
/// `explicitEmptyCells = true`, `None` when it does not; `bridge_land` is
/// `considerBridgesLand && ContainsBridge()`, which reads a bridged water cell
/// as land.
#[cfg(feature = "phobos")]
pub(crate) fn cell_eligible(
    land: Option<LandType>,
    allowed: AffectedTarget,
    techno_content: Option<bool>,
    bridge_land: bool,
) -> bool {
    if allowed == AffectedTarget::ALL {
        return true;
    }
    if techno_content == Some(false) && !allowed.contains(AffectedTarget::NO_CONTENT) {
        return false;
    }
    if allowed.contains(AffectedTarget::ALL_CELLS) {
        if land == Some(LandType::Water) && !bridge_land {
            allowed.contains(AffectedTarget::WATER)
        } else {
            allowed.contains(AffectedTarget::LAND)
        }
    } else {
        allowed != AffectedTarget::NONE
    }
}

/// `EnumFunctions::IsTechnoEligible` with `considerAircraftSeparately =
/// false`: the content flags split by class, aircraft under the Unit bit, and
/// a 1x1-with-undeploy building (`IsStrange`) under the Unit bit too.
#[cfg(feature = "phobos")]
pub(crate) fn techno_eligible(
    category: EntityCategory,
    allowed: AffectedTarget,
    strange_building: bool,
) -> bool {
    if allowed == AffectedTarget::ALL {
        return true;
    }
    if allowed.contains(AffectedTarget::ALL_CONTENTS) {
        return match category {
            EntityCategory::Infantry => allowed.contains(AffectedTarget::INFANTRY),
            EntityCategory::Unit | EntityCategory::Aircraft => {
                allowed.contains(AffectedTarget::UNIT)
            }
            EntityCategory::Structure if strange_building => allowed.contains(AffectedTarget::UNIT),
            EntityCategory::Structure => allowed.contains(AffectedTarget::BUILDING),
        };
    }
    allowed != AffectedTarget::NONE
}

/// `AnimExt::CreateRandomAnim` (`Ext/Anim/Body.cpp:407..432`): one random
/// entry constructed at the coordinates, owned by the firing house.
#[cfg(feature = "phobos")]
fn spawn_anim_at_world_coord(
    world: &mut Simulation,
    rules: &RuleSet,
    name: &str,
    coord: ProjectileCoord,
    owner_house: Option<InternedId>,
) {
    let world_coord = crate::sim::anim_class::AnimWorldCoord {
        x: coord.x,
        y: coord.y,
        z: coord.z,
    };
    spawn_anim_at_coord(world, rules, name, world_coord, owner_house);
}

#[cfg(feature = "phobos")]
fn spawn_anim_at_coord(
    world: &mut Simulation,
    rules: &RuleSet,
    name: &str,
    coord: crate::sim::anim_class::AnimWorldCoord,
    owner_house: Option<InternedId>,
) {
    let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
    let type_name = world.interner.intern(name);
    let descriptor =
        crate::sim::components::AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z);
    match world.spawn_anim_at_world(rules, descriptor, coord) {
        Ok(anim) => {
            if let Some(house) = owner_house {
                world.substrate.set_anim_owner_house(anim, house);
            }
        }
        Err(error) => log::debug!("crit animation [{name}] did not construct: {error}"),
    }
}

/// The targets `DetonateOnOneUnit` visits for this detonation: every item of
/// Phobos' `getCellSpreadItemsExt` walk for a cell-spread warhead, else the
/// intended target within 64 leptons of the impact (Ares' quarter-cell
/// tolerance).
#[cfg(feature = "phobos")]
fn detonation_targets(
    world: &Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    impact: ProjectileCoord,
    intended_target: Option<u64>,
) -> Vec<u64> {
    if warhead.cell_spread_f64 > 0.0 {
        // `getCellSpreadItemsExt(..., includeInAir = true,
        // CellSpread_Cylinder)` (`Ext/WarheadType/Detonate.cpp:174-175`).
        return super::phobos_airburst::cell_spread_technos(
            world,
            rules,
            impact,
            warhead.cell_spread_f64,
            warhead.phobos_cell_spread_cylinder,
        );
    }
    intended_target
        .filter(|&id| {
            world.substrate.entities.get(id).is_some_and(|target| {
                let coords = crate::sim::movement::ground_pose::object_get_coords(
                    target,
                    world.resolved_terrain.as_ref(),
                );
                let dx = i64::from(impact.x) - i64::from(coords.x);
                let dy = i64::from(impact.y) - i64::from(coords.y);
                let dz = i64::from(impact.z) - i64::from(coords.z);
                dx * dx + dy * dy + dz * dz <= 4096
            })
        })
        .into_iter()
        .collect()
}

#[cfg(test)]
#[path = "remove_mind_control_tests.rs"]
mod tests;
