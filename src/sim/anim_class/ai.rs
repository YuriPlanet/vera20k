//! `AnimClass::AI`: the per-frame visit with its gates, frame timing and
//! rate, damage and Ares weapon detonation, MakeInfantry, Start, Middle,
//! loop boundaries and the switch to the `Next=` type.

use super::spawn::{anim_random_rate, effective_bounds};
use super::{
    AnimConstructorInput, AnimDrawRuntime, AnimId, AnimObject, AnimOccupationOperation, AnimRemap,
    AnimSpawnError, AnimWorldCoord, TRAILER_DRAW_FLAGS,
};
use crate::rules::art_data::AnimTypeRuntimeConfig;
use crate::rules::overlay_types::TiberiumImageError;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::intern::InternedId;
use crate::sim::overlay_mark_host::RuntimeOverlayError;
use crate::sim::rng::SimRng;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::timer::CdTimer;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectKind;
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::native_x87::{NativeF64Bits, X87Chop53};

enum VisitAction<'a> {
    None,
    Destroy,
    MakeInfantry,
    Next(&'a str),
}

/// The facing a `MakeInfantry=` anim's infantryman is Unlimbo'd with
/// (`PUSH 0x60`, `0x00424A23`).
const MAKE_INFANTRY_UNLIMBO_FACING: u8 = 0x60;

/// The fields `Simulation::visit_anim`'s prefix and gates read, copied in one
/// lookup of the anim.
#[derive(Clone, Copy)]
struct AnimGates {
    /// The stored Location (`+0x9C`), owner-relative on an attached anim.
    location: AnimWorldCoord,
    bouncing: bool,
    inactive: bool,
    first_ai_guard: bool,
    delay_remaining: i32,
    /// Paused (`+0x19E`) or Temporal-paused (`+0x11A`).
    paused: bool,
    rate_reload: u16,
    frame_timer: CdTimer,
}

impl AnimGates {
    fn of(anim: &AnimObject) -> Self {
        Self {
            location: anim.world_coord,
            bouncing: anim.bounce.is_some(),
            inactive: anim.runtime.inactive,
            first_ai_guard: anim.runtime.first_ai_guard,
            delay_remaining: anim.runtime.delay_remaining,
            paused: anim.runtime.paused || anim.runtime.temporal_paused,
            rate_reload: anim.runtime.rate_reload,
            frame_timer: anim.runtime.frame_timer,
        }
    }
}

impl Simulation {
    pub fn visit_anim(
        &mut self,
        id: AnimId,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> Result<bool, RuntimeOverlayError> {
        // One read serves the visit's prefix and gates: most visits end at a
        // gate, and each step that may write the anim re-reads it (VERA-only,
        // results unchanged).
        let Some(anim) = self.anim(id) else {
            return Ok(false);
        };
        let type_id = anim.type_id;
        let mut gates = AnimGates::of(anim);
        // Borrowed from the rules, not the world: the visit below mutates the
        // world while it reads the type. Found by the type id's handle, not
        // its name (VERA-only, same config).
        let Some(config_handle) = self.interner.anim_type_config_handle(type_id, rules) else {
            self.destroy_anim(id, rules);
            return Ok(false);
        };
        let config = rules.art().anim_config(config_handle);

        // Ares `AnimClass_Update_AlphaLight` (hook `0x00423B0B`,
        // `Ares.dll 100476C0`) moves a glow before the flaming-guy test; a
        // flaming guy's moves at the head of its ObjectClass::AI
        // (`anim_flaming_guy_ai`).
        if rules.ares() && !config.is_flaming_guy {
            self.update_alpha_shape(id, ObjectKind::Anim, rules);
        }
        //423B17/423B1E precede the ordinary first-AI, delay and timer gates.
        if config.is_flaming_guy {
            self.anim_flaming_guy_ai(id, &config, rules, overlay_registry);
            let Some(reread) = self.anim(id).map(AnimGates::of) else {
                return Ok(false);
            };
            gates = reread;
        }
        // HideIfNoOre and the MakeInfantry mark read the stored Location
        // (`+0x9C`, `0x00423BD6..0x00423BFD`), not GetCoords: an owner-attached
        // anim stores an owner-relative delta there.
        let location = gates.location;

        // `AnimClass::AI @ 0x00423AC0`, before the MakeInfantry `vtable+0xF0`
        // call, the bounce-landing block, and the trailer block: with
        // `AnimType+0x359 HideIfNoOre`, `AnimClass+0x19D` is rewritten every
        // tick from its Location's cell — hidden when
        // `CellClass::Get_Tiberium_Value @ 0x00485020` is zero, visible
        // otherwise. Only drawing is suppressed; the AI keeps running.
        // Registry-less callers (fixtures) keep the current flag.
        if config.hide_if_no_ore
            && let Some(overlay_registry) = overlay_registry
        {
            let value = self.anim_cell_tiberium_value(location, rules, overlay_registry)?;
            // Phobos' `AnimClass_AI_Early @ 0x423B95` replacement keeps the same
            // gate and compares instead: `GetContainedTiberiumValue() <=
            // abs(HideIfNoOre.Threshold)`. `wrapping_abs` keeps the native
            // two's-complement result for INT_MIN, where C++'s `abs` overflows.
            #[cfg(feature = "phobos")]
            let hidden = value <= config.hide_if_no_ore_threshold.wrapping_abs();
            #[cfg(not(feature = "phobos"))]
            let hidden = value == 0;
            self.substrate.set_anim_hidden(id, hidden);
        }

        // AnimClass::AI performs this before its first-AI, inactive, delay,
        // visibility, and frame-timer gates. Repeated visits OR the same raw
        // bit; there is deliberately no contributor count.
        if config.make_infantry != -1 {
            Self::apply_make_infantry_raw_occupation(
                &mut self.substrate,
                &self.resolved_terrain,
                location,
                AnimOccupationOperation::Mark,
            );
        }
        // `AnimClass::AI 0x00423C24`: a bouncing chunk flies its body before
        // the trailer and the first-AI guard; touching down ends it.
        if gates.bouncing {
            if let Some(bridge_state_changed) =
                self.anim_bounce_step(id, &config, rules, overlay_registry, frame_effects)?
            {
                return Ok(bridge_state_changed);
            }
            let Some(reread) = self.anim(id).map(AnimGates::of) else {
                return Ok(false);
            };
            gates = reread;
        }

        // AnimAI4242A6/4242B0 gates only this producer on ObjectAlive and
        // inactive. Bounce above still runs before the retirement gate.
        if let Some(trailer_name) = config.trailer_anim.as_deref()
            && !gates.inactive
            && !self.substrate.pending_delete.contains_object(&id)
            && trailer_cadence_matches(
                u64::from(self.session.binary_frame()),
                config.trailer_seperation,
                rules.ares(),
            )
            && rules.art().anim_runtime_config(trailer_name).is_some()
        {
            // The trailer spawns at GetCoords after the body moved the anim.
            let Some(world_coord) = self.anim_absolute_coord(id) else {
                return Ok(false);
            };
            let trailer_type = self.interner.intern(trailer_name);
            let descriptor = AnimClassSpawnDescriptor {
                type_name: trailer_type,
                rx: 0,
                ry: 0,
                sub_x: crate::util::fixed_math::SIM_ZERO,
                sub_y: crate::util::fixed_math::SIM_ZERO,
                z: 0,
                delay: 1,
                loop_count: 1,
                draw_flags: TRAILER_DRAW_FLAGS,
                z_adjust: 0,
                reverse: false,
                use_cell_drawer: false,
                terrain_attached: false,
                draw_runtime: AnimDrawRuntime::default(),
                owner: None,
            };
            // The strict binder binds the parent's whole `TrailerAnim=`
            // closure; the tolerant one stops on a malformed image header, so
            // a mod's trailer can be unbindable here. It is then not spawned.
            let constructor = self.effect_anim_constructor(rules, trailer_type);
            match self.spawn_anim_at_world_with_constructor(
                rules,
                descriptor,
                world_coord,
                constructor,
                &mut FrameEffects::default(),
            ) {
                Ok(_) => {}
                Err(AnimSpawnError::Tiberium(error)) => {
                    log::warn!(
                        "trailer animation refused an invalid Tiberium Image start: {error}"
                    );
                }
                Err(error) => log::warn!("trailer animation not spawned: {error}"),
            }
            let Some(reread) = self.anim(id).map(AnimGates::of) else {
                return Ok(false);
            };
            gates = reread;
        }

        //AnimAI424322..42435F: a DropZone marker ends over the first ground
        //Building at its current GetCoords, before first-AI/delay/timer gates.
        //NoEffects does not gate this test; a deck-only Building does not end it.
        //The type compare is by config handle: the same answer as the names'
        //case-insensitive compare for this type, which has a config.
        if self.interner.is_drop_zone_anim(config_handle, rules)
            && self.anim_absolute_coord(id).is_some_and(|world| {
                Self::map_coord_cell(&self.resolved_terrain, world.x, world.y)
                    .and_then(|(x, y)| {
                        self.substrate.occupancy.first_building_on_layer(
                            x,
                            y,
                            crate::sim::movement::locomotor::MovementLayer::Ground,
                        )
                    })
                    .is_some()
            })
        {
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.inactive = true; //424358 writes+19B before UnInit.
                gates.inactive = true;
            }
        }

        // AnimAI42435F rereads after prefix callbacks. All inactive writers
        // share this retirement; a Bouncer may already have landed/retired.
        if gates.inactive {
            self.destroy_anim(id, rules);
            return Ok(false);
        }
        if gates.first_ai_guard {
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.first_ai_guard = false;
            }
            return Ok(false);
        }

        // `AnimClass::AI @ 0x00423AC0` delay countdown: the visit that takes it
        // to zero calls `AnimClass::Start @ 0x00424CE0` (call site `0x004243A1`)
        // and returns. The field also holds the `RandomLoopDelay=` pause, so a
        // looping anim restarts, and replays its start sound, after each pause.
        // `0x00424386..0x00424399` tests `!= 0` and decrements, so a negative
        // pause counts on down and holds the anim.
        if gates.delay_remaining != 0 {
            let Some(anim) = self.substrate.anims.get_mut(id) else {
                return Ok(false);
            };
            anim.runtime.delay_remaining = anim.runtime.delay_remaining.wrapping_sub(1);
            if anim.runtime.delay_remaining == 0 {
                self.anim_start(id, &config, rules, overlay_registry, frame_effects)?;
            }
            return Ok(false);
        }

        // AnimAI4243AE..4243B6 checks Object+90 after first-AI/delay.
        // FlamingGuyAI can retire this receiver before the ordinary tail.
        if self.substrate.pending_delete.contains_object(&id) {
            return Ok(false);
        }
        // AnimAI4243BC..424427: compare the exact CellAnim type at virtual
        // GetCoords minus384 in XY. A mismatch writes inactive but falls
        // through this visit's frame/damage work; retirement is next visit.
        if config.is_animated_tiberium {
            let Some(mut location) = self.anim_absolute_coord(id) else {
                return Ok(false);
            };
            location.x = location.x.wrapping_sub(384);
            location.y = location.y.wrapping_sub(384);
            let (overlay, _) = self.anim_cell_overlay_fields(location);
            let matches = if let Some(overlay) = overlay {
                let registry = overlay_registry.unwrap_or_else(|| rules.overlay_registry());
                registry
                    .flags(overlay)
                    .ok_or(TiberiumImageError::MissingOverlay(i32::from(overlay)))?
                    .cell_anim
                    .as_deref()
                    // The overlay type holds its CellAnim= as a name, not an
                    // AnimType handle; the compare allocates nothing.
                    .is_some_and(|name| name.eq_ignore_ascii_case(self.interner.resolve(type_id)))
            } else {
                false
            };
            if !matches && let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.inactive = true;
            }
        }
        let mut action = VisitAction::None;
        let mut random_loop_delay = None;
        let current_frame = self.session.binary_frame() as i32;
        //42449B: power pause follows first-AI/delay gates and precedes timer advance;
        //4244A9: the Temporal erase pause (+11A) right after it.
        if gates.paused || gates.rate_reload == 0 || !gates.frame_timer.expired(current_frame) {
            return Ok(false);
        }
        {
            let Some(anim) = self.substrate.anims.get_mut(id) else {
                return Ok(false);
            };
            anim.runtime
                .frame_timer
                .start(current_frame, i32::from(anim.runtime.rate_reload));
            anim.runtime.current_frame = anim
                .runtime
                .current_frame
                .wrapping_add(anim.runtime.frame_step);
        }
        let bridge_state_changed =
            self.anim_frame_damage(id, &config, rules, overlay_registry, frame_effects);
        // AnimAI42464C checks only ObjectAlive after receiver callbacks.
        // Ordinary Anim retirement queues retained storage; scalar deletion
        // removes it. Owner expiry's inactive byte is a later-visit gate.
        if self.anim(id).is_none() || self.substrate.pending_delete.contains_object(&id) {
            return Ok(bridge_state_changed);
        }
        let middle = self.anim(id).is_some_and(|anim| {
            // `0x0042465D..0x00424687`: the committed stage reaching the
            // type's middle frame (`+0x298`, zero meaning Start already ran
            // Middle) calls Middle unless the anim is bouncing (`+0x194`, the
            // constructor's byte, which a `Next=` switch keeps), before the boundary
            // tail.
            middle_frame(&config).is_some_and(|middle| {
                config.start.wrapping_add(anim.runtime.current_frame) == middle
            }) && anim.bounce.is_none()
        });
        if middle {
            self.anim_middle(id, &config, rules, overlay_registry);
        }
        let Some(anim) = self.substrate.anims.get_mut(id) else {
            return Ok(bridge_state_changed);
        };
        let boundary = advance_anim_boundary(anim, &config);
        match boundary {
            AnimBoundary::Continue | AnimBoundary::Bounce => return Ok(bridge_state_changed),
            AnimBoundary::Loop => {
                random_loop_delay = config.random_loop_delay;
            }
            AnimBoundary::Complete => {
                action = if let Some(next) = config.next.as_deref() {
                    VisitAction::Next(next)
                } else if config.make_infantry != -1 {
                    VisitAction::MakeInfantry
                } else {
                    VisitAction::Destroy
                };
            }
        }

        // `0x004247B1..0x004247E5`: unless both bounds are 0, Random_Ranged
        // (`0x0065C7E0`) on the Scenario stream draws the signed pause; equal
        // bounds draw nothing.
        if let Some((low, high)) = random_loop_delay {
            let delay = self.scenario_rng.next_range_i32_inclusive(low, high);
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.delay_remaining = delay;
            }
        }
        match action {
            VisitAction::None => {}
            VisitAction::Destroy => {
                if let Some(anim) = self.substrate.anims.get_mut(id) {
                    anim.completed = true;
                }
                self.destroy_anim(id, rules);
            }
            VisitAction::MakeInfantry => {
                if self.anim_make_infantry(
                    id,
                    config.make_infantry,
                    rules,
                    overlay_registry,
                    frame_effects,
                ) {
                    if let Some(anim) = self.substrate.anims.get_mut(id) {
                        anim.completed = true;
                    }
                    self.destroy_anim(id, rules);
                }
            }
            VisitAction::Next(next) => {
                self.switch_anim_type(id, next, rules, overlay_registry, frame_effects)?;
            }
        }
        Ok(bridge_state_changed)
    }

    /// AnimClass::AI424513..424647, after the stage/timer commit. Debit the
    /// retained fraction before GetCoords/AreaDamage can retire this emitter.
    /// The body adds no RNG request; receiver consequences keep their owners.
    fn anim_frame_damage(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let ares = rules.ares_form();
        let Some(anim) = self.anim(id) else {
            return false;
        };
        let damage = f64::from_bits(config.damage.bits());
        //424513..424532 rejects zero/negative/unordered, then instance+194.
        if !(damage > 0.0) || anim.bounce.is_some() {
            return false;
        }
        // 0x00424538..0x0042455E: an owner whose WhatAmI is Terrain (0x24)
        // scales Damage by 5.0 (0x007E3568). A tree owner is kept in
        // `owner_terrain`, never in `owner_entity`.
        let terrain_owner = anim.owner_terrain.is_some();
        // Ares AnimClass_Update_DamageDelay1000B1AF..1000B1E7 passes the
        // animation's House, falling back to an attached Techno's owner.
        // Neither owner is an attacking object; keep RAD_NO_ATTACKER below.
        let source_house = if ares {
            anim.owner_house.or_else(|| {
                anim.owner_entity
                    .and_then(|owner| self.substrate.entities.get(owner))
                    .map(|owner| owner.owner())
            })
        } else {
            None
        };
        let no_effects = anim.no_effects;
        let type_id = anim.type_id;
        let accumulator = f64::from_bits(anim.damage_accumulator.bits());
        // Rust does not specify arithmetic NaN payloads across platforms.
        let store = |sim: &mut Self, value: f64| {
            let bits = if value.is_nan() {
                0x7ff8_0000_0000_0000
            } else {
                value.to_bits()
            };
            sim.substrate.anims.get_mut(id).unwrap().damage_accumulator =
                NativeF64Bits::from_bits(bits);
        };
        // Ares 3.0 AnimClass_Update_DamageDelay (1000AFE0, hook 0x424538,
        // resuming at 0x42464C or 0x42465D): with `Damage.Delay=` above zero
        // and `Damage=` at least 1, +0x188 counts frames and the whole Damage
        // (truncated, times 5 on a terrain owner) lands once the count reaches
        // the delay, which then restarts from zero. Otherwise the original
        // fraction accumulates. Its conversion (100772B0) truncates and gives
        // the integer indefinite out of range; a result below 1 or a
        // no-effects anim delivers nothing and keeps the count.
        let whole = if ares {
            let multiplier = if terrain_owner { 5 } else { 1 };
            let delay = config.ares_damage_delay;
            let delayed = delay >= 1 && !(damage < 1.0);
            let counted = if delayed {
                accumulator + 1.0
            } else {
                f64::from(multiplier) * damage + accumulator
            };
            store(self, counted);
            let short = if delayed {
                counted < f64::from(delay)
            } else {
                counted < 1.0
            };
            if short {
                return false;
            }
            let whole = if delayed {
                ares_truncate(damage).wrapping_mul(multiplier)
            } else {
                ares_truncate(counted)
            };
            if whole < 1 || no_effects {
                return false;
            }
            store(
                self,
                if delayed {
                    0.0
                } else {
                    counted - f64::from(whole)
                },
            );
            // Weapon= detonates a bullet of that weapon instead (0x42464C).
            if let Some(weapon) = config
                .ares_weapon
                .as_deref()
                .and_then(|name| rules.weapon(name))
            {
                return self.ares_anim_weapon_detonation(
                    id,
                    weapon,
                    whole,
                    rules,
                    overlay_registry,
                    effects,
                );
            }
            whole
        } else {
            let increment = if terrain_owner { damage * 5.0 } else { damage };
            let accumulated = accumulator + increment;
            store(self, accumulated);
            if !(accumulated >= 1.0) || no_effects {
                return false;
            }
            //4245AC..4245D5 consumes Math7C5F00's low signed32 result, including
            // masked conversion/overflow, and subtracts that signed integer.
            let whole = X87Chop53::ftol_f64_low_masked(accumulated);
            store(self, accumulated - f64::from(whole));
            whole
        };
        //4245C3..424630 compares the own stored ID exactly, not Image.
        let warhead_name = if self.interner.resolve(type_id) == "INVISO" {
            Some(rules.bridge_warheads.c4_name.as_str())
        } else {
            rules.combat_damage.flame_damage2()
        };
        // Ares1000B179 first reads AnimType+330 (the existing art Warhead
        // reader); only a null pointer uses the stock INVISO/FlameDamage2
        // fallback.
        let warhead_name = if ares {
            config.warhead.as_deref().or(warhead_name)
        } else {
            warhead_name
        };
        let Some((name, warhead)) =
            warhead_name.and_then(|name| rules.warhead(name).map(|warhead| (name, warhead)))
        else {
            return false;
        };
        let Some(coord) = self.anim_absolute_coord(id) else {
            return false;
        };
        let warhead_ref = self.interner.intern(name);
        crate::sim::combat::world_receiver::apply_area_damage(
            self,
            rules,
            overlay_registry,
            crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z),
            whole,
            warhead,
            true,
            (
                crate::sim::combat::RAD_NO_ATTACKER,
                source_house,
                warhead_ref,
            ),
            effects,
        )
        .bridge_state_changed
    }

    /// Ares AnimClass_Update_DamageDelay's `Weapon=` arm (1000B116..
    /// 1000B169): BulletTypeExt::CreateBullet of the weapon's Projectile at
    /// the anim's cell, no owner, the computed damage, the weapon's Warhead
    /// and Bright, given the weapon, Removed, detonated at the anim's
    /// GetCoords and Released. That is the bare `DetonateAtCoord`
    /// Fire_Death_Weapon makes (`ProjectileDetonationReason::DeathWeapon`).
    /// Returns whether a bridge changed.
    fn ares_anim_weapon_detonation(
        &mut self,
        id: AnimId,
        weapon: &crate::rules::weapon_type::WeaponType,
        damage: i32,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let (Some(warhead), Some(coord)) =
            (weapon.warhead.as_deref(), self.anim_absolute_coord(id))
        else {
            return false;
        };
        let impact = crate::sim::projectile::ProjectileCoord::new(coord.x, coord.y, coord.z);
        let (rx, ry) = (coord.x.div_euclid(256), coord.y.div_euclid(256));
        let target = match (u16::try_from(rx), u16::try_from(ry)) {
            (Ok(rx), Ok(ry)) => crate::sim::projectile::ProjectileTarget::looked_up_cell(
                self.resolved_terrain.as_ref(),
                rx,
                ry,
            ),
            _ => crate::sim::projectile::ProjectileTarget::DummyCell,
        };
        let payload = crate::sim::projectile::ProjectilePayload::new(
            damage,
            self.interner.intern(warhead),
            weapon
                .projectile
                .as_deref()
                .map(|id| self.interner.intern(id)),
            weapon.bright,
        )
        .with_installed_weapon(Some(self.interner.intern(&weapon.id)));
        let detonation = crate::sim::projectile::ProjectileDetonation {
            projectile_id: id,
            source_id: crate::sim::combat::RAD_NO_ATTACKER,
            target,
            impact,
            payload,
            reason: crate::sim::projectile::ProjectileDetonationReason::DeathWeapon,
        };
        self.commit_logic_projectile_detonations(rules, overlay_registry, &[detonation], effects)
    }

    /// The end of a `MakeInfantry=` anim's last loop with no `Next=`
    /// (`AnimClass::AI @ 0x00423AC0`, `0x00424932..0x00424B30`), in order:
    /// 1. ClearCellOccupancy (vt+0xF4 = `0x00426300`) at the stored Location
    ///    (`0x00424963`).
    /// 2. A `MakeInfantry=` above `[General] AnimToInfantry=`'s count
    ///    (`Rules+0xCF4`, signed `JG` at `0x00424982`) creates nothing.
    /// 3. With no owner (`+0x180`) or a defeated one (`+0x1F5`), the first
    ///    house in HouseClass::Array order whose side is `Civilian` becomes
    ///    the owner ([`Simulation::civilian_side_house`],
    ///    `0x0042499C..0x004249D8`). Without one the owner stays, and with
    ///    no owner nothing is created (`0x004249E6`).
    /// 4. `AnimToInfantry=`'s entry's CreateObject (vt+0x8C) for the owner,
    ///    then Unlimbo (vt+0xD8) at the Location facing `0x60` (`0x00424A42`).
    /// 5. A refused Unlimbo steps the stage (`+0xAC`) back one and keeps the
    ///    anim (`0x00424B23`): its next completed frame tries again with a
    ///    new object. The refused one stays in limbo, as it does natively.
    ///    With Ares the refused one is UnInit and the anim ends (below).
    /// 6. Placed where the anim's cell (GetCoords, `0x005657A0`) carries the
    ///    bridge flag (`+0x140 & 0x100`) and the Location is above the cell's
    ///    GetCoords Z (`0x00486840`), it moves onto the deck: Mark(UP),
    ///    OnBridge (`+0x8C`), Mark(DOWN) (`0x00424AA0..0x00424AE6`).
    /// 7. Hunt (`0xF`, Queue_Mission vt+0x1E8) unless the owner's IsHuman
    ///    (`+0x1EC`, `0x00424AF2`).
    ///
    /// Returns whether the anim ends; it then completes (`+0x179`) and
    /// UnInits (`0x00424B0A..0x00424B15`).
    ///
    /// Native execution: `tools/superweapon_oracle.py` section
    /// `make_infantry`, replayed in `superweapon::genetic_converter_tests`.
    ///
    /// RESIDUAL: a `MakeInfantry=` equal to the count, or below -1, reads
    /// outside the vector (`0x00424A05`); VERA creates nothing. Retail's only
    /// `MakeInfantry=` anim is GENDEATH (0), and `AnimToInfantry=` holds
    /// BRUTE (production reader).
    pub(crate) fn anim_make_infantry(
        &mut self,
        id: AnimId,
        make_infantry: i32,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        let Some((location, owner)) = self
            .anim(id)
            .map(|anim| (anim.world_coord, anim.owner_house))
        else {
            return true;
        };
        Self::apply_make_infantry_raw_occupation(
            &mut self.substrate,
            &self.resolved_terrain,
            location,
            AnimOccupationOperation::Clear,
        );
        let kinds = &rules.general.anim_to_infantry;
        if i64::from(make_infantry) > kinds.len() as i64 {
            return true;
        }
        let Some(kind) = usize::try_from(make_infantry)
            .ok()
            .and_then(|index| kinds.get(index))
        else {
            return true;
        };
        let defeated = |sim: &Self, house: InternedId| {
            sim.houses
                .get(&house)
                .is_some_and(|state| state.is_defeated())
        };
        let owner = if owner.is_none_or(|house| defeated(self, house)) {
            match self.civilian_side_house(rules) {
                Some(civilian) => {
                    self.substrate.set_anim_owner_house(id, civilian);
                    Some(civilian)
                }
                None => owner,
            }
        } else {
            owner
        };
        let Some(owner) = owner else {
            return true;
        };
        let (rx, ry, _, _, _) = location.to_cell_sub_z();
        // Rules6707DC/67BB10 retains an InfantryType pointer; AnimAI4249F9
        // calls that type's creator, independent of other name registries.
        let Some(object) =
            rules.object_in_category(crate::rules::object_type::ObjectCategory::Infantry, kind)
        else {
            return true;
        };
        let Some(infantry) = self.construct_resolved_object_limbo_at_height(
            object,
            owner,
            rx,
            ry,
            0,
            0,
            rules,
            frame_effects,
        ) else {
            log::debug!("MakeInfantry anim {id}: [{kind}] did not construct");
            return true;
        };
        let placed = self.reveal_constructed_object_at_coord_with_overlay_context(
            infantry,
            crate::sim::components::DriveCoord {
                x: location.x,
                y: location.y,
                z: location.z,
            },
            MAKE_INFANTRY_UNLIMBO_FACING,
            crate::sim::world::PlacementEvidence::EvaluateMark,
            rules,
            overlay_registry,
            frame_effects,
        );
        if placed.is_none() {
            // Ares 3.0 AnimClass_Update (1005C3E0, hook 0x424B23): the
            // refused infantryman is UnInit and the anim ends (TimeToDie,
            // UnInit) instead of trying again.
            if rules.ares_form() {
                self.uninit_with_context(
                    infantry,
                    crate::sim::world::UninitContext::new(rules).with_effects(frame_effects),
                );
                return true;
            }
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.runtime.current_frame = anim.runtime.current_frame.wrapping_sub(1);
            }
            return false;
        }
        let at = self.anim_absolute_coord(id).unwrap_or(location);
        let lift = self.resolved_terrain.as_ref().is_some_and(|terrain| {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let cell = cells.lookup_world(at.x, at.y);
            let crate::map::cell_index::NativeCellIdentity::Real(_) = cell else {
                return false;
            };
            let (cell_x, cell_y) = cells.coord(cell);
            cells.flags(cell) & 0x100 != 0
                && location.z
                    > crate::sim::projectile::cell_ground_coord(
                        Some(terrain),
                        cell_x as u16,
                        cell_y as u16,
                    )
                    .z
        });
        if lift {
            self.foot_mark_remove(infantry, rules, overlay_registry, frame_effects);
            if let Some(entity) = self.substrate.entities.get_mut(infantry) {
                entity.on_bridge.store(true);
            }
            self.foot_mark_put(infantry, rules, overlay_registry, frame_effects);
        }
        let human = self
            .houses
            .get(&owner)
            .is_some_and(|house| house.is_human());
        if !human && let Some(entity) = self.substrate.entities.get_mut(infantry) {
            crate::sim::mission::authority::queue_entity_mission_deferred(
                entity,
                crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Hunt),
            );
        }
        true
    }

    fn choose_anim_rate(
        session: &ScenarioSession,
        scenario_rng: &mut SimRng,
        config: &AnimTypeRuntimeConfig,
    ) -> u16 {
        let drawn = anim_random_rate(config, scenario_rng);
        Self::anim_rate(session, config, drawn)
    }

    /// The frame delay from `Rate=` or a drawn `RandomRate=` pick, normalized
    /// to the game speed for `Normalized=` types.
    pub(super) fn anim_rate(
        session: &ScenarioSession,
        config: &AnimTypeRuntimeConfig,
        random_rate: Option<u16>,
    ) -> u16 {
        let delay = random_rate.unwrap_or(config.rate_logic_frames);
        if config.normalized {
            session.game_options().normalized_anim_delay(delay)
        } else {
            delay
        }
    }

    /// Native `AnimClass::Start @ 0x00424CE0` — the anim's sound emitter.
    ///
    /// `AnimClass::Start @ 0x00424CE0`: it Marks, then
    /// `if (Anim+0x198 /* silent */ == 0 && AnimType+0x2F8 != -1)` takes the
    /// coordinate through vtable `+0x48` and plays it with `VocClass::PlayAt`,
    /// then calls `AnimClass::Middle @ 0x00424F00` when `AnimType+0x298 == 0`.
    /// `Middle` is `SpawnsParticle=` looped `NumParticles=` times, `Scorch=`,
    /// `Crater=`, `ForceBigCraters=`, and plays nothing. Callers: the
    /// constructor for a zero delay (`0x00422702`), and `AnimClass::AI` when
    /// the delay countdown reaches zero (`0x004243A1`) or a `Next=` type takes
    /// over (`0x00424925`).
    ///
    /// `AnimType+0x2F8` is one slot: `AnimTypeClass::ReadINI @ 0x00427D00`
    /// reads `Report=` into it only when `StartSound=` resolved to `-1`
    /// (`0x00428344..0x004283BC`); `config.start_sound` holds that slot.
    ///
    /// Constructor-time starts read the match's overlay types too, so a
    /// one-frame marking anim (no retail anim has one) marks at construction.
    pub(super) fn anim_start(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> Result<(), AnimSpawnError> {
        let ares = rules.ares_form();
        let sound_name = self.anim(id).and_then(|anim| {
            (!anim.no_effects())
                .then(|| config.start_sound.as_deref())
                .flatten()
                .map(|name| rules.sound_name(name))
        });
        if let Some(sound_name) = sound_name
            && let Some(world) = self.anim_absolute_coord(id)
        {
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.start_sound_active = true;
            }
            self.sound_events.emit(SimSoundEvent::AnimationStarted {
                anim_id: id,
                sound_id: sound_name,
                world,
            });
        } else if self.anim(id).is_some_and(|anim| anim.start_sound_active) {
            // Start424D32 clears the prior start handle when NoEffects is
            // set or the new type has no StartSound. A late TAction41 flag
            // never cancels the constructor's sound before this later Start.
            if let Some(world) = self.anim_absolute_coord(id) {
                self.sound_events.emit(SimSoundEvent::AnimationStopped {
                    anim_id: id,
                    stop_sound_id: None,
                    world,
                });
            }
            self.substrate
                .anims
                .get_mut(id)
                .expect("restarting animation remains present")
                .start_sound_active = false;
        }
        if middle_frame(config).is_none() {
            self.anim_middle(id, config, rules, overlay_registry);
        }

        // AnimClass::Start424D5F..424EF3, original gamemd instructions:
        // the constructor reaches this before TAction41's late NoEffects.
        // Middle may run callbacks, so read the live instance/type again.
        let Some(type_id) = self
            .anim(id)
            .filter(|anim| !anim.no_effects())
            .map(|anim| anim.type_id())
        else {
            return Ok(());
        };
        if !self
            .interner
            .anim_type_config(type_id, rules)
            .is_some_and(|config| config.tiberium_chain_reaction)
        {
            return Ok(());
        }
        let Some(coord) = self.anim_absolute_coord(id) else {
            return Ok(());
        };
        let registry = rules.overlay_registry();
        let cell = {
            let Some(terrain) = self.resolved_terrain.as_ref() else {
                return Ok(());
            };
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            match crate::sim::movement::ground_pose::query_object_cell(
                &cells,
                crate::sim::components::DriveCoord {
                    x: coord.x,
                    y: coord.y,
                    z: coord.z,
                },
            ) {
                receiver @ crate::map::cell_index::NativeCellIdentity::Real(_) => {
                    let (x, y) = cells.coord(receiver);
                    (x as u16, y as u16)
                }
                crate::map::cell_index::NativeCellIdentity::Dummy => {
                    if cells.dummy().overlay_fields().0.is_some_and(|overlay| {
                        registry.flags(overlay).is_some_and(|flags| flags.tiberium)
                    }) {
                        log::warn!(
                            "Anim.Start mineral-bearing shared Dummy mutation is unsupported"
                        );
                    }
                    return Ok(());
                }
            }
        };
        let resource = self
            .overlay_grid
            .as_ref()
            .filter(|grid| cell.0 < grid.width() && cell.1 < grid.height())
            .map(|grid| {
                crate::sim::tiberium::tiberium_cell_view(
                    grid,
                    registry,
                    &rules.tiberium_types,
                    cell,
                )
            })
            .transpose()?
            .flatten();
        let Some(resource) = resource else {
            return Ok(());
        };
        // Retain the selected class across removal; the cleared overlay no
        // longer identifies its Debris or Color (424DAA..424DC0).
        let mineral = rules
            .tiberium_types
            .get(resource.tiberium_type)
            .expect("resolved mineral type");
        Self::reduce_tiberium_at_with_native_context(
            &self.terrain_objects,
            &self.ore_growth_config,
            &mut self.ore_growth_state,
            &self.session,
            &mut self.scenario_rng,
            &self.substrate,
            &mut self.resolved_terrain,
            &mut self.overlay_grid,
            &mut self.dirty_cells,
            cell,
            i32::from(resource.overlay_data) + 1,
            rules,
            Some(registry),
        )?;
        // Reduce480A80 publishes Recalc's caches before the child constructor.
        // No zone repair belongs to that first Recalc.
        self.publish_recalculated_cells(rules, &[cell]);
        let debris = mineral.debris();
        let throw_debris = if ares {
            // Ares 3.0 10050D50 (hook 0x424DD3), after the Debris count test:
            // RandomRanged(0, 99) < Debris.Chance replaces the one-in-three draw.
            !debris.is_empty()
                && self.scenario_rng.next_range_i32_inclusive(0, 99)
                    < mineral.ares().debris_chance()
        } else {
            // 424DDF..424DF3 uses one signed raw draw. Only zero is consumed:
            // abs(raw % 3)==0 also matches wrapped abs(raw)%3==0 at INT_MIN.
            !debris.is_empty() && self.scenario_rng.next_raw_abs_modulo(3) == 0
        };
        if throw_debris {
            if let Some(mut child_coord) = self.anim_absolute_coord(id) {
                child_coord.z = child_coord.z.wrapping_add(10);
                let selected = self
                    .scenario_rng
                    .next_range_u32_inclusive(0, (debris.len() - 1) as u32)
                    as usize;
                let (rx, ry, sub_x, sub_y, level) = child_coord.to_cell_sub_z();
                let type_name = self.interner.intern(&debris[selected]);
                let descriptor = AnimClassSpawnDescriptor {
                    draw_flags: 0x600,
                    ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, level)
                };
                match self.spawn_anim_at_world_with_constructor(
                    rules,
                    descriptor,
                    child_coord,
                    AnimConstructorInput::Runtime,
                    frame_effects,
                ) {
                    Ok(child) => {
                        // 424E72..424E95 assigns Convert and retained Cell
                        // light AFTER all child constructor/Start callbacks.
                        if let Ok(entry) = u8::try_from(mineral.color_scheme() / 2) {
                            self.substrate.anims.set_anim_remap(
                                child,
                                AnimRemap::ColorScheme(
                                    crate::rules::house_colors::HouseColorIndex(entry),
                                ),
                            );
                        }
                        self.retain_anim_cell_light(child, Some(cell));
                    }
                    Err(error @ AnimSpawnError::Tiberium(_)) => return Err(error),
                    Err(error) => {
                        log::warn!("Anim.Start mineral Debris failed to construct: {error}")
                    }
                }
            }
        }
        // The damage centre is raw Object.Location, whereas the retained Cell
        // and child above use GetCoords. Zero damage still leaves the suffix.
        // RESIDUAL: Start returns nothing, so the bridge-change flag is dropped
        // and a bridge hit here skips that frame's bridge art refresh; only a
        // TiberiumExplosionDamage above retail's 0 reaches it.
        // Ares 3.0 10050DA0 (hook 0x424EC5): the mineral's ExplosionWarhead
        // and ExplosionDamage, else C4Warhead and TiberiumExplosionDamage.
        let (warhead_name, damage) = if ares {
            (
                mineral
                    .ares()
                    .explosion_warhead(Some(rules.bridge_warheads.c4_name.as_str())),
                mineral
                    .ares()
                    .explosion_damage(rules.combat_damage.tiberium_explosion_damage),
            )
        } else {
            (
                Some(rules.bridge_warheads.c4_name.as_str()),
                rules.combat_damage.tiberium_explosion_damage,
            )
        };
        if let Some(raw) = self.anim(id).map(|anim| anim.world_coord)
            && let Some(warhead_name) = warhead_name
            && let Some(warhead) = rules.warhead(warhead_name)
        {
            let warhead_ref = self.interner.intern(warhead_name);
            crate::sim::combat::world_receiver::apply_area_damage(
                self,
                rules,
                Some(registry),
                crate::sim::projectile::ProjectileCoord::new(raw.x, raw.y, raw.z),
                damage,
                warhead,
                false,
                (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
                frame_effects,
            );
        }
        // Start424EDA/424EE8/424EF3 is a distinct, unconditional second
        // Recalc -> AssignOrphaned -> hierarchy update, never a second Reduce.
        if let (Some(grid), Some(terrain)) =
            (self.overlay_grid.as_mut(), self.resolved_terrain.as_mut())
        {
            grid.recalculate_runtime_cell(terrain, registry, cell);
        }
        self.publish_cell_and_assign_orphaned_zone(rules, cell, None);
        Ok(())
    }

    /// `AnimClass::Middle @ 0x00424F00`: marks the ground under the anim.
    ///
    /// The size is the type's middle-frame size (`ArtEntry::frame_width`,
    /// 30 x 30 without an image). Nothing lands when the anim stands 30
    /// leptons or more above the ground (`vtable+0x1C8`, `ObjectClass::
    /// GetHeight @ 0x005F5F40`, measured from the stored Location; anims
    /// never set OnBridge). The marks land at `GetCoords` (`vtable+0x48`).
    ///
    /// Before the mark-height gate,425008..42504B sends SpawnsParticle to
    /// the shared GasCloudSys NumParticles times at the raw Location.
    fn anim_middle(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        if let Some(kind) = config
            .spawns_particle
            .as_deref()
            .and_then(|name| rules.p_type_id_by_name(name))
            && let Some(coord) = self.anim(id).map(|anim| anim.world_coord)
        {
            let spread = rules.ares_form() && config.ares_spawns_particle_range != [0, 0];
            if spread {
                self.ares_spawn_particles_around(id, kind, config, rules);
            } else {
                for _ in 0..config.num_particles {
                    self.spawn_gas_particle(
                        kind,
                        glam::IVec3::new(coord.x, coord.y, coord.z),
                        rules,
                    );
                }
            }
        }
        if !(config.scorch || config.crater) {
            return;
        }
        let (Some(stored), Some(coord)) = (
            self.anim(id).map(|anim| anim.world_coord),
            self.anim_absolute_coord(id),
        ) else {
            return;
        };
        let stored = crate::sim::projectile::ProjectileCoord::new(stored.x, stored.y, stored.z);
        let ground = crate::sim::projectile::projectile_ground_z(
            self.resolved_terrain.as_ref(),
            &self.effective_shared_cell_dummy(),
            stored,
        );
        if stored.z.wrapping_sub(ground) >= ANIM_MIDDLE_MAX_HEIGHT {
            return;
        }
        // The middle frame's size is on the ART entry, which the lookup
        // finds by the name in ASCII uppercase, on the stack.
        let (width, height) = self
            .anim(id)
            .and_then(|anim| rules.art().get(self.interner.resolve(anim.type_id)))
            .map_or((30, 30), |entry| {
                (i32::from(entry.frame_width), i32::from(entry.frame_height))
            });
        let marks = crate::sim::combat::smudge_dispatch::AnimMiddleMarks {
            scorch: config.scorch,
            crater: config.crater,
            force_big_craters: config.force_big_craters,
            width,
            height,
        };
        let coord = crate::sim::smudge_grid::SimCoord {
            x: coord.x,
            y: coord.y,
            z: coord.z,
        };
        self.commit_smudge_request_inline(
            rules,
            overlay_registry,
            crate::sim::combat::SmudgeSpawnRequest::AnimMiddle { coord, marks },
        );
    }

    /// Ares 3.0 AnimClass_Expired_SpawnsParticle (1000AE40, hook 0x425002,
    /// resuming at 0x42504D): each of the NumParticles particles lands at
    /// `|RandomRanged(min, max)|` leptons from the anim's GetCoords, at an
    /// angle drawn within its share of the circle (`RandomRanged(1,
    /// 0x7FFFFFFF)` times 1/0x7FFFFFFF), keeping the anim's height above the
    /// floor (`0x00578080`). The arithmetic is the DLL's x87 code; its CRT
    /// cos/sin are libm's.
    fn ares_spawn_particles_around(
        &mut self,
        id: AnimId,
        kind: crate::rules::particle_type::ParticleTypeId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
    ) {
        use crate::util::native_x87::NativeF64Bits as Bits;
        let Some(coord) = self.anim_absolute_coord(id) else {
            return;
        };
        let floor = |sim: &Self, x: i32, y: i32, z: i32| {
            crate::sim::projectile::projectile_ground_z(
                sim.resolved_terrain.as_ref(),
                &sim.effective_shared_cell_dummy(),
                crate::sim::projectile::ProjectileCoord::new(x, y, z),
            )
        };
        let height = coord.z.wrapping_sub(floor(self, coord.x, coord.y, coord.z));
        let load = |value: f64| {
            X87Chop53::load_f64(Bits::from_bits(value.to_bits())).expect("finite constant")
        };
        let store = |value| f64::from_bits(X87Chop53::store_f64(value).map_or(0, Bits::bits));
        let count = config.num_particles;
        let step =
            X87Chop53::div(load(6.283185307179586), X87Chop53::load_i32(count)).map_or(0.0, store);
        let [minimum, maximum] = config.ares_spawns_particle_range;
        let mut angle = 0.0;
        for _ in 0..count {
            let distance = self
                .scenario_rng
                .next_range_i32_inclusive(minimum, maximum)
                .wrapping_abs();
            let value = self.scenario_rng.next_range_i32_inclusive(1, 0x7FFF_FFFF);
            let radians = store(X87Chop53::add(
                X87Chop53::mul(
                    X87Chop53::mul(X87Chop53::load_i32(value), load(4.656612873077393e-10)),
                    load(step),
                ),
                load(angle),
            ));
            let (cos, sin) = (libm::cos(radians), libm::sin(radians));
            let along = |trig: f64| {
                ares_truncate_x87(X87Chop53::mul(X87Chop53::load_i32(distance), load(trig)))
            };
            let x = along(cos).wrapping_add(coord.x);
            let y = coord.y.wrapping_sub(along(sin));
            let z = floor(self, x, y, coord.z).wrapping_add(height);
            self.spawn_gas_particle(kind, glam::IVec3::new(x, y, z), rules);
            angle = store(X87Chop53::add(load(angle), load(step)));
        }
    }

    pub(super) fn switch_anim_type(
        &mut self,
        id: AnimId,
        next: &str,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> Result<(), AnimSpawnError> {
        let Some(config) = rules.art().anim_runtime_config(next) else {
            self.destroy_anim(id, rules);
            return Ok(());
        };
        let Some((effective_end, effective_loop_end)) = effective_bounds(config) else {
            self.destroy_anim(id, rules);
            return Ok(());
        };
        let type_id = self.interner.intern(next);
        let rate_reload = Self::choose_anim_rate(&self.session, &mut self.scenario_rng, config);
        let frame_timer =
            CdTimer::started(self.session.binary_frame() as i32, i32::from(rate_reload));
        let stop_sound_id = config
            .stop_sound
            .as_deref()
            .map(|sound| self.interner.intern(sound));
        // A remapped anim's new type is a new atlas key.
        self.substrate.anims.note_look(id);
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.type_id = type_id;
            anim.damage_accumulator = NativeF64Bits::POSITIVE_ZERO;
            anim.effective_end = effective_end;
            anim.effective_loop_end = effective_loop_end;
            anim.stop_sound_id = stop_sound_id;
            // 0x00424913..0x0042491F: Value (+0xAC) takes Next's Start as
            // written, though it is otherwise relative to Start, so the first
            // drawn frame is twice Start; the step (+0xC4) keeps the old
            // type's direction. Retail Next targets all have Start=0 and no
            // Reverse=, where this equals a fresh start.
            anim.runtime.current_frame = config.start;
            anim.runtime.delay_remaining = 0;
            anim.runtime.rate_reload = rate_reload;
            anim.runtime.frame_timer = frame_timer;
            anim.runtime.loop_remaining = native_loop_remaining(config.loop_count, 1);
            anim.runtime.first_ai_guard = false;
            anim.runtime.inactive = false;
        }
        self.anim_start(id, config, rules, overlay_registry, frame_effects)
    }
}

/// `AnimClass::Middle @ 0x00424F00`'s height gate (`0x00425057`).
pub(crate) const ANIM_MIDDLE_MAX_HEIGHT: i32 = 30;

/// The type's middle frame, `AnimTypeClass+0x298`: the raw SHP frame count
/// halved (`AnimTypeClass::Load_Image @ 0x00427B50`). None when it is zero,
/// where Start runs Middle instead.
pub(super) fn middle_frame(config: &AnimTypeRuntimeConfig) -> Option<i32> {
    // AnimTypeLoad428855 calls generic LoadSHP, not bounds resolver427B50;
    // an unread type's newly resident header must not replace ctor Middle0.
    if !config.art_body_read {
        return None;
    }
    config
        .middle_frame
        .or_else(|| config.raw_shp_frame_count.map(|raw| raw / 2))
        .filter(|&middle| middle != 0)
}

pub(super) fn native_loop_remaining(loop_count: i32, constructor_loop: i32) -> u8 {
    // gamemd-derived: `AnimClass::Constructor @ 0x00421EA0`, branch at
    // 0x004226BF. The constructor argument is compared as signed before its
    // low byte participates in the wrapping LoopCount multiplication.
    let constructor_factor = if constructor_loop > 1 {
        constructor_loop as u8
    } else {
        1
    };
    (loop_count as u8).wrapping_mul(constructor_factor).max(1)
}

/// Ares.dll's float-to-int conversion (100772B0): toward zero, and the
/// integer indefinite (`i32::MIN`) for NaN or a value out of range.
fn ares_truncate(value: f64) -> i32 {
    X87Chop53::load_f64(NativeF64Bits::from_bits(value.to_bits()))
        .map_or(i32::MIN, ares_truncate_x87)
}

fn ares_truncate_x87(value: crate::util::native_x87::X87Value) -> i32 {
    X87Chop53::ftol_i64(value)
        .ok()
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(i32::MIN)
}

/// `0x004242CA..0x004242DF`: a separation of 1, else the signed frame count
/// modulo the separation (IDIV), so a negative separation also spawns.
/// Ares hooks 0x004242CA (`AnimClass_Update_FixIE_TrailerSeperation`,
/// Ares.dll `1000B210`): a separation below 1 jumps to 0x00424322 and spawns
/// none; 1 and above take the IDIV.
/// RESIDUAL: gamemd faults dividing by a zero `TrailerSeperation=`; VERA
/// spawns no trailer there.
pub(super) fn trailer_cadence_matches(binary_frame: u64, separation: i32, ares: bool) -> bool {
    if ares && separation < 1 {
        return false;
    }
    separation == 1 || (separation != 0 && (binary_frame as i32).wrapping_rem(separation) == 0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AnimBoundary {
    Continue,
    Bounce,
    Loop,
    Complete,
}

/// Native AnimClass::AI committed-frame tail, 0x0042468C..0x004247B1.
/// Native boundary/reset goldens: tools/anim_oracle/boundary.json. The direction
/// flags select the reverse endpoint and reset; the signed frame step does not.
pub(super) fn advance_anim_boundary(
    anim: &mut AnimObject,
    config: &AnimTypeRuntimeConfig,
) -> AnimBoundary {
    let stage = anim.runtime.current_frame;
    let last_loop = anim.runtime.loop_remaining <= 1;
    let loop_end = anim.effective_loop_end.wrapping_sub(config.start);

    // 0x0042468C..0x004246DC: ping-pong has its own lower-end equality test,
    // returns immediately after NEG, and does not consume a loop.
    if config.ping_pong
        && if last_loop {
            stage >= anim.effective_end || stage == 0
        } else {
            stage >= loop_end || stage == config.start
        }
    {
        anim.runtime.frame_step = anim.runtime.frame_step.wrapping_neg();
        return AnimBoundary::Bounce;
    }

    let reverse = config.reverse || anim.runtime.constructor_reverse;
    let upper_end = if last_loop {
        anim.effective_end
    } else {
        loop_end
    };
    // 0x0042470C..0x00424738: Shadow adds the forward LoopEnd-Start boundary
    // even on the final loop. It does not replace the ordinary End test.
    if !(stage >= upper_end
        || (config.shadow && !reverse && stage >= loop_end)
        || (reverse && stage <= 0))
    {
        return AnimBoundary::Continue;
    }
    if anim.runtime.loop_remaining != 0 && anim.runtime.loop_remaining != u8::MAX {
        anim.runtime.loop_remaining -= 1;
    }
    if anim.runtime.loop_remaining == 0 {
        return AnimBoundary::Complete;
    }
    // 0x0042477B..0x004247AB: reset from the two reverse flags, independently of
    // the current (possibly ping-pong-negated) frame step.
    anim.runtime.current_frame = if reverse {
        anim.effective_loop_end
    } else {
        config.loop_start.wrapping_sub(config.start)
    };
    AnimBoundary::Loop
}
