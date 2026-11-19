//! ObjectClass falling: parachutes and dropped bombs share one state owner.
//!
//! As in gamemd, a falling object's height is its Location Z
//! (`position.exact_z_leptons`). `ObjectClass::AI`'s falling block moves it
//! ([`Simulation::advance_fall`](crate::sim::world::Simulation::advance_fall)):
//! - each frame the Z moves by the FallRate: `Location.Z = GetZ() + FallRate`
//! - an attached Anim's retained Object+84 flag selects integer DEC, capped
//!   by `Rules.ParachuteMaxFallRate` (default `-3`); a new Object starts at 0
//! - the fall grounds when GetHeight is at most 0, and SetHeight(0) puts the
//!   object on the ground or deck
//! - the infantry keeps its base locomotor and body sequence during descent
//!
//! Free fall uses trunc(FallRate - 1.4), capped by NoParachuteMaxFallRate.
//! Landing retains the rate and attached-Anim/bomb bytes. The last normal
//! Anim detach clears +84; a later fall then uses free-fall acceleration.
//! A grounded bomb enters the synchronous C4 receiver with its current Health
//! and both force flags.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/game_entity, sim/entity_store, sim/locomotor.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::sim::entity_store::EntityStore;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;

/// What a landing cell answered: `CellClass::IsClearToMove`'s verdict for the
/// mover's `SpeedType`/`MovementZone` and the cell's own `ContainsBridge()`
/// (`+0x140 & 0x100`), which both Phobos arms copy into `OnBridge`.
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LandingCellVerdict {
    /// The `IsClearToMove` answer.
    pub clear: bool,
    /// `pCell->ContainsBridge()`.
    pub on_bridge: bool,
}

/// Begin parachute descent for an entity at world Z `drop_z` (leptons).
/// Returns `true` on success.
///
/// `ObjectClass::Paradrop @ 0x005F5940` raises the falling byte (`+0x8D`,
/// `0x005F5965`) and places the object at the drop coordinate: Unlimbo
/// (`vt+0xD8`), then SetLocation (`vt+0x1B4`, `0x005F5A50`). It preserves
/// the FallRate; only a new Object starts at 0. The caller's subsequent Anim
/// attachment owns the +84 acceleration flag.
///
/// The caller positions the entity's XY. Reveal keeps this Z as the Unlimbo
/// coordinate's.
pub fn begin_parachute_descent(entities: &mut EntityStore, entity_id: u64, drop_z: i32) -> bool {
    let Some(entity) = entities.get_mut(entity_id) else {
        return false;
    };

    entity.begin_parachute_fall(drop_z);
    true
}

/// Leptons the canopy is constructed above the falling object
/// (`ADD EDI, 0x4B` at `0x005F5AAA`).
const PARACHUTE_ANIM_Z_LIFT_LEPTONS: i32 = 0x4B;

/// `AnimClass` draw flags of the canopy (`PUSH 0x600`, `0x005F5ACF`).
const PARACHUTE_ANIM_DRAW_FLAGS: u32 = 0x600;

impl crate::sim::world::Simulation {
    /// ObjectAI5F3F11..5F405B inside MissionAI, before mission dispatch and
    /// the locomotor. The landing callback precedes canopy wind-down and the
    /// forced C4 receiver. No fall-specific RNG, timer or detach calls occur;
    /// the synchronous production receiver owns all ordinary death effects.
    pub(crate) fn object_ai_fall(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Result<(bool, bool), crate::sim::world::FrameAdvanceError> {
        if self
            .substrate
            .entities
            .get(id)
            .is_none_or(|e| !e.is_falling_down())
        {
            return Ok((false, false));
        }
        let old_layer = self.entity_display_layer(id, rules);
        let landed = self.advance_fall(
            id,
            rules.general.parachute_max_fall_rate,
            rules,
            registry,
            effects,
        );
        let mut changed = false;
        // `Ext/Techno/Hooks.cpp:1030-1031`: the landing arm reads and clears
        // `OnParachuted` for every falling-down Techno, before the `IsABomb`
        // gate, so an unflagged later landing cannot keep it.
        #[cfg(feature = "phobos")]
        let mut on_parachuted = false;
        if landed {
            #[cfg(feature = "phobos")]
            {
                on_parachuted = self.take_on_parachuted(id);
            }
            changed =
                self.per_cell_process(id, super::PerCellReason::Arrival, rules, registry, effects)?;
            Self::wind_down_parachute_anim(&mut self.substrate, id);
            //5F3FA4..5F3FFA: PerCell can Limbo the object. The landing
            // frame updates and retains FallRate only after that callback.
            if let Some(entity) = self.substrate.entities.get_mut(id)
                && !entity.lifecycle.in_limbo()
            {
                entity.advance_fall_rate(
                    rules.general.parachute_max_fall_rate,
                    rules.general.no_parachute_max_fall_rate,
                    self.interner
                        .try_resolve(entity.type_ref())
                        .and_then(|_| self.interner.object_type(entity.type_ref(), rules)),
                    rules.ares(),
                    rules.ares_form(),
                );
            }
        }
        if self
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| e.lifecycle.in_limbo())
        {
            self.substrate.display.remove(id);
            return Ok((landed, changed));
        }
        let new_layer = self.entity_display_layer(id, rules);
        if old_layer != new_layer
            && let Some(layer) = new_layer
        {
            //5F4001..5F400E: resubmit only when the live layer changed.
            Self::submit_object_display(&mut self.substrate, id, layer);
        }
        if let Some(health) = self
            .substrate
            .entities
            .get(id)
            .filter(|e| {
                !e.lifecycle.in_limbo()
                    && !e.is_falling_down()
                    && e.was_dropped_as_bomb()
                    && e.health.current() > 0
            })
            .map(|e| e.health.current())
        {
            //5F402B copies current Health+6C into a local packet (not Strength).
            //
            // Phobos `ObjectClass_Update_FallingDown_ToDead` (hook `0x5F4021`,
            // `Ext/Techno/Hooks.cpp:1014-1090`) reads and clears
            // `TechnoExt::HoverShutdown` inside this arm and lets the
            // `HoverDrownable` answer pick the damage. The drowning arm takes
            // the whole current health (`:1033-1036`); the surviving arm runs
            // the `FallingDownDamage` model below.
            #[cfg(feature = "phobos")]
            let damage = {
                let took_shutdown = self.take_hover_shutdown(id);
                if took_shutdown && self.hover_drowns(id, rules) {
                    // The drowning arm keeps the whole current-Health packet
                    // (`Ext/Techno/Hooks.cpp:1033-1036`).
                    health
                } else if !self.landing_cell_clear(id, rules) {
                    // `IsClearToMove` fails before the ratio block: the whole
                    // current Health goes (`:1043-1055`).
                    health
                } else if on_parachuted {
                    // `if (!onParachuted)` gates the EMP arm and the whole
                    // ratio block (`:1067-1084`): the packet stays 0, so the
                    // landing deals no `FallingDownDamage` at all.
                    0
                } else {
                    // A shutdown over a non-drowning cell falls through into the
                    // same ratio block the canopy arm uses (`:1062-1084`).
                    self.phobos_falling_damage(id, health, rules)
                }
            };
            #[cfg(not(feature = "phobos"))]
            let damage = health;
            let c4 = self.interner.intern(&rules.bridge_warheads.c4_name);
            let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
                id,
                damage,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                c4,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            );
            self.commit_direct_damage_receiver(rules, registry, hit, effects);
        }
        Ok((landed, changed))
    }

    /// Phobos `TechnoExt::HoverShutdown`: the landing arm owns the single read
    /// and the clear (`Ext/Techno/Hooks.cpp:1026-1027`).
    #[cfg(feature = "phobos")]
    fn take_hover_shutdown(&mut self, id: u64) -> bool {
        self.substrate
            .entities
            .get_mut(id)
            .is_some_and(|entity| entity.take_hover_shutdown())
    }

    /// `Ext/Techno/Hooks.cpp:1030-1031`: the landing arm reads and clears
    /// `TechnoExt::OnParachuted` on every falling-down Techno.
    #[cfg(feature = "phobos")]
    fn take_on_parachuted(&mut self, id: u64) -> bool {
        self.substrate
            .entities
            .get_mut(id)
            .is_some_and(|entity| entity.take_on_parachuted())
    }

    /// `HoverDrownable.Get(RulesExt::Global()->HoverDrownable)`
    /// (`Ext/Techno/Hooks.cpp:1031`): the type's own value, else
    /// `[General] HoverDrownable`.
    #[cfg(feature = "phobos")]
    fn hover_drowns(&self, id: u64, rules: &crate::rules::ruleset::RuleSet) -> bool {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| {
                self.interner.try_resolve(entity.type_ref())?;
                self.interner.object_type(entity.type_ref(), rules)
            })
            .is_none_or(|object| {
                object
                    .phobos_hover_drown
                    .drowns(rules.phobos_globals.hover_drownable)
            })
    }

    /// The landing arm's ratio packet (`Ext/Techno/Hooks.cpp:1062-1084`): an
    /// EMP-locked unit whose `FallingDownDamage.AllowEMP` (type else
    /// `[CombatDamage]`, default `true`) refuses keeps the vanilla
    /// full-current-Health packet; otherwise the ratio is the type's
    /// `FallingDownDamage.Water` on a bridge-less water or beach cell, else
    /// the type's `FallingDownDamage`, else the `[CombatDamage]` global. A
    /// negative ratio takes that fraction of the current Health, one of at
    /// most 1.0 that fraction of `Strength`, and a larger one is absolute;
    /// each tier truncates into the `int` packet.
    #[cfg(feature = "phobos")]
    fn phobos_falling_damage(
        &self,
        id: u64,
        health: i32,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> i32 {
        let Some(entity) = self.substrate.entities.get(id) else {
            return health;
        };
        // A fixture entity may carry an id this interner never produced; it then
        // has no type keys, which is the pre-existing whole-health packet.
        if self.interner.try_resolve(entity.type_ref()).is_none() {
            return health;
        }
        let type_rules = self.interner.object_type(entity.type_ref(), rules);
        let allow_emp = type_rules
            .as_ref()
            .and_then(|object| object.phobos_falling_down_damage_allow_emp)
            .unwrap_or(rules.combat_damage.falling_down_damage_allow_emp);
        if !allow_emp && entity.emp_remaining() > 0 {
            return health;
        }
        let strength = self.object_strength(id, rules).unwrap_or(health);
        let ratio = if self.landing_in_water(id) {
            type_rules
                .as_ref()
                .and_then(|object| {
                    object
                        .phobos_falling_down_damage_water
                        .or(object.phobos_falling_down_damage)
                })
                .unwrap_or(rules.combat_damage.falling_down_damage)
        } else {
            type_rules
                .as_ref()
                .and_then(|object| object.phobos_falling_down_damage)
                .unwrap_or(rules.combat_damage.falling_down_damage)
        };
        if ratio < 0.0 {
            (f64::from(health) * -ratio) as i32
        } else if ratio <= 1.0 {
            (f64::from(strength) * ratio) as i32
        } else {
            ratio as i32
        }
    }

    /// The landing cell is water or beach and carries no bridge deck
    /// (`Ext/Techno/Hooks.cpp:1059-1061`: `!onBridge && (Water || Beach)`).
    #[cfg(feature = "phobos")]
    fn landing_in_water(&self, id: u64) -> bool {
        use crate::map::resolved_terrain::NativeCellQuery;
        use crate::rules::terrain_rules::LandType;

        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup_world(coord.x, coord.y);
        let land = cells.land_type(cell);
        (land == i32::from(LandType::Water.as_index())
            || land == i32::from(LandType::Beach.as_index()))
            && cells.flags(cell) & 0x100 == 0
    }

    /// `CellClass::IsClearToMove(pType->SpeedType, true, true, -1,
    /// pType->MovementZone, -1, pCell->ContainsBridge())`
    /// (`Ext/Techno/Hooks.cpp:1043-1055`): the cell a falling unit lands on
    /// must admit it. VERA reads the two arms that decide it here — the cell's
    /// reduced `ZoneType` against the mover's `MovementZone`
    /// (`MOVEMENT_ZONE_PASSABILITY`, the matrix the zone flood-fill uses) and
    /// the `LandType` row's speed for the mover's `SpeedType` (a zero row is
    /// impassable) — plus the deck plane the hook's `onBridge` argument
    /// selects.
    ///
    /// `onBridge` is the cell's `ContainsBridge()` (`+0x140 & 0x100`), and
    /// with level `-1` it moves the whole call onto the deck
    /// (`0x0048353C..0x0048354E`): the deck occupation byte
    /// (`AltOccupationFlags+0x128`) replaces the ground one and the land half
    /// is skipped outright (`0x004835DE..0x004835F4`). So a mover may land on
    /// a bridge deck over water its own `SpeedType` could never cross, which
    /// the two ground arms would refuse. Both ignore flags are set, so
    /// `0x00483558..0x00483572` masks the byte with `0xE0` then `0x5F` and
    /// leaves only `0x40`.
    ///
    /// A cell outside the playfield is not clear. An unknown cell, type or
    /// `SpeedType` answers clear so the pre-existing ratio packet stands.
    #[cfg(feature = "phobos")]
    fn landing_cell_clear(&self, id: u64, rules: &crate::rules::ruleset::RuleSet) -> bool {
        self.landing_cell_verdict(id, rules)
            .is_some_and(|verdict| verdict.clear)
    }

    /// [`Self::landing_cell_clear`] plus the two facts Phobos's type-change arm
    /// (`Ext/Foot/Body.cpp:772-785`) needs: `None` when `TryGetCellAt` finds no
    /// `CellClass` at the object's coordinate (the arm then takes its else
    /// branch), otherwise the `IsClearToMove` answer and the cell's
    /// `ContainsBridge()` it copies into `OnBridge`.
    #[cfg(feature = "phobos")]
    pub(crate) fn landing_cell_verdict(
        &self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<LandingCellVerdict> {
        use crate::map::resolved_terrain::NativeCellQuery;
        use crate::rules::locomotor_type::SpeedType;
        use crate::sim::occupancy::RawCellKey;

        let unknown = Some(LandingCellVerdict {
            clear: true,
            on_bridge: false,
        });
        let Some(entity) = self.substrate.entities.get(id) else {
            return unknown;
        };
        if self.interner.try_resolve(entity.type_ref()).is_none() {
            return unknown;
        }
        let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
            return unknown;
        };
        if object.speed_type == SpeedType::Invalid {
            return unknown;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return unknown;
        };
        let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup_world(coord.x, coord.y);
        let (rx, ry) = cells.coord(cell);
        let (Ok(rx), Ok(ry)) = (u16::try_from(rx), u16::try_from(ry)) else {
            return unknown;
        };
        let Some(resolved) = terrain.cell(rx, ry) else {
            return unknown;
        };
        if resolved.outside_playfield {
            return None;
        }
        let contains_bridge = crate::sim::cell_rect::canonical_cell_ref(terrain, cell)
            .bridge_flags_0x1180()
            & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
            != 0;
        if contains_bridge {
            let deck = self.substrate.raw_cell_occupation.bits_at(
                RawCellKey::Real(rx, ry),
                crate::sim::movement::locomotor::MovementLayer::Bridge,
            );
            return Some(LandingCellVerdict {
                clear: (deck & 0xE0) & 0x5F == 0,
                on_bridge: true,
            });
        }
        if !crate::sim::pathfinding::passability::is_passable_for_zone(
            resolved.zone_type,
            object.movement_zone,
        ) {
            return Some(LandingCellVerdict {
                clear: false,
                on_bridge: false,
            });
        }
        let clear = terrain
            .land_speed_costs(i32::from(resolved.land_type))
            .cost_for_speed_type(object.speed_type)
            // An unbound row (no rules bound to the grid) is unknown, not a
            // zero row: answer clear so the ratio packet stands.
            .is_none_or(|bits| f32::from_bits(bits.bits()) > 0.0);
        Some(LandingCellVerdict {
            clear,
            on_bridge: false,
        })
    }

    /// The type's `Strength=`, the ratio model's base for a ratio up to 1.0
    /// (`Ext/Techno/Hooks.cpp:1082`).
    #[cfg(feature = "phobos")]
    fn object_strength(&self, id: u64, rules: &crate::rules::ruleset::RuleSet) -> Option<i32> {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| {
                self.interner.try_resolve(entity.type_ref())?;
                self.interner.object_type(entity.type_ref(), rules)
            })
            .map(|object| object.strength)
    }

    /// Construct the falling object's canopy.
    ///
    /// gamemd-derived, the virtual `ObjectClass::Paradrop @ 0x005F5940`. It
    /// places the object itself (`Unlimbo` through `vtable+0xD8` at
    /// `0x005F5A3D`, then the coordinate through `vtable+0x1B4`) and then, at
    /// `0x005F5A9D..0x005F5B03`, for anything but a bullet it copies the
    /// object's coordinate, adds 75 leptons of Z, constructs
    /// `AnimClass(Rules+0xBBC, &coord, delay 0, loopCount 1, drawFlags 0x600,
    /// zAdjust 0, reverse 0)`, stores it at `Object+0x88` and makes the object
    /// its owner (`AnimClass::SetOwnerObject @ 0x00424B50`), so the canopy
    /// rides the descent. `Rules+0xBBC` is `[General] Parachute=`.
    ///
    /// The NPSE hook at the `Parachute=` read (`0x5F5ADD` -> cave
    /// `0xC2B0E4`..`0xC2FA8A`) branches the canopy: a type whose `IsBomb=`
    /// flag is set takes `Rules+0xBB8` (`BombParachute=`), infantry keeps
    /// `Parachute=`, and anything else takes the new `[General]
    /// VehicleParachute=` (Rules+0x400) when it is set and `Parachute=`
    /// otherwise. VERA has no parachuted bullets and no `IsBomb=` type flag,
    /// so the canopy here is the infantry/non-infantry branch; the bomb arms
    /// stay native-only (see the module residual).
    ///
    /// VERA keeps no `Object+0x88`: the canopy is found again as the owner's
    /// attached anim of the parachute type.
    ///
    /// RESIDUAL: a bullet takes `Rules+0xBB8` (`BombParachute=`) at the
    /// uncopied coordinate instead; VERA has no parachuted bullets.
    ///
    /// Native copies the owner's drawer (`vtable+0x1E4`) to Anim+0xD4; the
    /// House remap below retains that selection. It also copies the owner's
    /// Cell+0x10A light intensity to Anim+0xFC, separately from the instance
    /// ZAdjust at+0x100. Publish the read at this producer boundary; the
    /// existing lighting owner samples it before later relights.
    pub(crate) fn attach_parachute_anim(
        &mut self,
        rules: &crate::rules::ruleset::RuleSet,
        owner_id: u64,
    ) -> Option<crate::sim::anim_class::AnimId> {
        let owner = self.substrate.entities.get(owner_id)?;
        let object = self.interner.try_resolve(owner.type_ref()).and_then(|_| {
            self.interner
                .object_type_in_category(owner.type_ref(), owner.category().into(), rules)
        });
        // Ares 3.0 10047C60 (hook 0x5F5ADD), before the canopy type: an
        // `IsBomb` Foot sets IsABomb (+8F), so it lands as a bomb.
        if rules.ares_form()
            && owner.category().is_foot()
            && object.is_some_and(|object| object.techno_combat().roles().is_bomb())
        {
            self.substrate
                .entities
                .get_mut(owner_id)?
                .mark_parachuted_bomb();
        }
        let owner = self.substrate.entities.get(owner_id)?;
        let owner_house = owner.owner();
        let side = self
            .houses
            .get(&owner_house)
            .map(|house| house.side_index())
            .unwrap_or(-1);
        let country = self
            .houses
            .get(&owner_house)
            .and_then(|house| house.country())
            .map(|country| self.interner.resolve(country));
        let Some(canopy) = parachute_anim_name(rules, owner, object, side, country) else {
            self.substrate
                .entities
                .get_mut(owner_id)?
                .clear_parachute_animation();
            return None;
        };
        let type_name = self.interner.intern(canopy);
        let mut coord = self.anim_owner_coords(owner_id)?;
        coord.z = coord.z.wrapping_add(PARACHUTE_ANIM_Z_LIFT_LEPTONS);
        let (rx, ry, sub_x, sub_y, level) = coord.to_cell_sub_z();
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
            delay: 0,
            loop_count: 1,
            draw_flags: PARACHUTE_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            owner: None,
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_name, rx, ry, sub_x, sub_y, level,
            )
        };
        match self.spawn_anim_at_world(rules, descriptor, coord) {
            Ok(anim_id) => {
                // Object::Paradrop5F5AF1 publishes +88 before5F5AFE binds
                // the Anim owner. Keep a single held identity, not a type scan.
                self.substrate
                    .entities
                    .hold_parachute_animation(owner_id, anim_id)?;
                self.set_anim_owner_object(anim_id, Some(owner_id), rules);
                // Native Paradrop calls SetOwnerObject, then GetRemapColour
                // through the owner's vtable and stores that ConvertClass at
                // Anim+0xD4 (`ObjectClass::Paradrop @ 0x005F5940`,
                // `0x005F5AFE..0x005F5B19`). Freeze the same house selection
                // on the attached canopy after construction.
                self.substrate
                    .anims
                    .set_anim_house_remap(anim_id, owner_house);
                let owner = self.substrate.entities.get(owner_id)?;
                self.retain_anim_cell_light(
                    anim_id,
                    Some((owner.position.rx(), owner.position.ry())),
                );
                Some(anim_id)
            }
            Err(error) => {
                self.substrate
                    .entities
                    .get_mut(owner_id)?
                    .clear_parachute_animation();
                // An art type that never bound draws nothing natively either.
                log::debug!("parachute anim did not construct: {error}");
                None
            }
        }
    }

    /// The landing edge of the canopy.
    ///
    /// gamemd-derived, `ObjectClass::AI @ 0x005F3E70`: when the falling
    /// object's height reaches zero it clears the in-air byte and, if
    /// `Object+0x88` is set, zeroes the anim's remaining-loops byte
    /// (`MOV byte ptr [EAX+0x195], 0` at `0x005F3F9D`). The anim is not
    /// removed: with a count of zero `AnimClass::AI` tests the frame against
    /// the type's end frame (`0x004246E4`) and completes there (`0x0042475A`),
    /// so the canopy plays on from its loop into the rest of its frames and
    /// leaves at the last one. Retail `PARACH.SHP` has 140 frames and loops
    /// 20..39, so that is about a hundred frames of the canopy collapsing on
    /// the landed object.
    pub(crate) fn wind_down_parachute_anim(substrate: &mut ObjectSubstrate, owner_id: u64) {
        // ObjectClass::AI5F3F93..5F3FA4 reads held +88, even after
        // SetOwnerObject detached the canopy or the type's INI changed.
        if let Some(anim_id) = substrate
            .entities
            .get(owner_id)
            .and_then(|owner| owner.parachute_animation())
            && let Some(anim) = substrate.anims.get_mut(anim_id)
        {
            anim.end_loops();
        }
    }
}

/// The canopy a dropped object gets. Without NPatch, or for infantry, use
/// `[General] Parachute=`; other objects with NPatch take the NPSE
/// `[General] VehicleParachute=` when it is set and `Parachute=` otherwise
/// (`ObjectClass::Paradrop`'s hook `0x5F5ADD` -> cave `0xC2B0E4`:
/// `0xC2FA75..0xC2FA8A`). The cave's first arm sends an object whose type
/// has NPSE's `IsBomb=` flag to `BombParachute=` (`0xC2B0FC..0xC2B103`);
/// The NPSE-specific `BombParachute=` selection remains a residual.
fn parachute_anim_name<'r>(
    rules: &'r crate::rules::ruleset::RuleSet,
    entity: &crate::sim::game_entity::GameEntity,
    object: Option<&'r crate::rules::object_type::ObjectType>,
    side: i32,
    country: Option<&str>,
) -> Option<&'r str> {
    // ObjectClass_SpawnParachuted_Animation (installed Ares 3.0 DLL10047C60),
    // hook5F5ADD replaces `mov edx,[ecx+0xBBC]`
    // and resumes5F5AE3 with the chosen type in EDX. Foot types take
    // TechnoTypeExt+F0 before Country's GetParachuteAnim1001BBC0.
    if rules.ares()
        && entity.category().is_foot()
        && let Some(canopy) =
            object.and_then(|object| object.techno_combat().presentation().parachute_anim())
    {
        return Some(canopy);
    }
    // `GetParachuteAnim @ 0x1001BBC0` answers the country's own
    // `[Country] Parachute.Anim=` first; a never-set or `none` field is the
    // null pointer there and defers to the side.
    if let Some(country) = country
        && let Some(canopy) = rules.country_parachute_anim(country)
    {
        return Some(canopy);
    }
    // Ares `[Side] Parachute.Anim=` wins over both the stock canopy and the
    // NPSE non-infantry branch; an unset key leaves that existing owner, and
    // `none` falls through to `Find("PARACH")`
    // (`HouseTypeExt::GetParachuteAnim @ 0x1001BBC0`).
    if let Some(override_) = rules.side_parachute_override(side) {
        return Some(override_.unwrap_or("PARACH"));
    }
    if !crate::rules::NPATCH_ENABLED
        || entity.category() == crate::map::entities::EntityCategory::Infantry
    {
        rules.general.parachute_shp.as_deref()
    } else {
        rules
            .general
            .vehicle_parachute
            .as_deref()
            .or(rules.general.parachute_shp.as_deref())
    }
}

/// Phobos parabombs' descent rate (`Ext/Bullet/Hooks.cpp:128-137`).
///
/// ```text
/// fallRate = ParabombFallRate - Parachuted.FallRate
/// if (fallRate < Parachuted.MaxFallRate.Get([General] ParachuteMaxFallRate))
///     fallRate = that cap
/// ParabombFallRate = fallRate      // persisted on BulletExt
/// ```
///
/// `Parachuted.FallRate=` is therefore the per-frame **acceleration** (a
/// positive step subtracted from a rate that is negative while descending)
/// and `Parachuted.MaxFallRate=` / `[General] ParachuteMaxFallRate` the
/// **terminal** rate — a lower bound in signed terms, i.e. the fastest the
/// canopy is allowed to fall. This is the same step/cap shape
/// [`crate::sim::object_fall::ObjectFallState::advance_rate`] uses for a
/// parachuting object; only the owners of `step` and `cap` differ.
///
/// The bullet's accumulated rate starts at `0` (`BulletExt::ParabombFallRate`
/// `Ext/Bullet/Body.h:61`), so the first frame computes `0 - step` and the
/// cap brings it up to the terminal rate whenever the step is positive.
#[cfg(feature = "phobos")]
pub fn parachuted_bullet_rate(current: i32, step: i32, cap: i32) -> i32 {
    current.wrapping_sub(step).max(cap)
}

impl crate::sim::world::Simulation {
    /// `ObjectClass::AI`'s falling block (`0x005F3F11..0x005F3FFA`) for one
    /// object. Answers whether the fall grounded this frame.
    ///
    /// - `Location.Z = GetZ() + FallRate` (`0x005F3F2C..0x005F3F60`). On the
    ///   first frame the FallRate is 0, so the object hangs for a frame.
    /// - A marked object leaves its cell through its own Mark (`vt+0x124` at
    ///   `0x005F3F46`) before that write and marks again after it
    ///   (`0x005F3F58`). A paratrooper's layer is Ground at any height (Foot
    ///   `vt+0x78` = `0x004DB7E0` asks the locomotor; Walk's `0x0075C7E0`
    ///   returns Ground), so every fall frame prepends it to its cell's list
    ///   again and recalculates the cell.
    /// - GetHeight at most 0 (`0x005F3F6A`) grounds it: SetHeight(0)
    ///   (`0x005F3F7A`) puts it on the ground or deck, and the falling byte
    ///   clears (`0x005F3F86`).
    /// - An airborne object updates its retained rate here. On landing,
    ///   `object_ai_fall` first runs PerCell and canopy wind-down, then updates
    ///   the rate unless that callback put the object in Limbo (5F3FA4).
    ///
    /// Native rate controls are executed in drop_as_bomb.json.
    pub(crate) fn advance_fall(
        &mut self,
        stable_id: u64,
        max_fall_rate: i32,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(stable_id) else {
            return false;
        };
        let Some(rate) = entity.is_falling_down().then(|| entity.fall_rate()) else {
            return false;
        };
        let z = crate::sim::movement::ground_pose::object_world_z_leptons(
            entity,
            self.resolved_terrain.as_ref(),
        )
        .wrapping_add(rate);
        let marked = entity.lifecycle.cell_marked();
        let context = crate::sim::world::UninitContext::new(rules, registry).with_effects(effects);
        if marked {
            self.unmark_entity_remove(stable_id, context);
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.position.put_z(z);
        }
        if marked {
            self.mark_entity_put(stable_id, context);
        }
        let terrain = self.resolved_terrain.as_ref();
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return false;
        };
        if crate::sim::movement::air_movement::current_fly_height(entity, terrain) > 0 {
            if !entity.lifecycle.in_limbo() {
                entity.advance_fall_rate(
                    max_fall_rate,
                    rules.general.no_parachute_max_fall_rate,
                    self.interner
                        .try_resolve(entity.type_ref())
                        .and_then(|_| self.interner.object_type(entity.type_ref(), rules)),
                    rules.ares(),
                    rules.ares_form(),
                );
            }
            return false;
        }
        // SetHeight(0) (`0x005F3F7A`) runs before the falling byte clears
        // (`0x005F3F86`). Descent does not displace the locomotor, so there
        // is no piggyback to unwind here.
        self.set_object_height(stable_id, 0, rules, registry, effects);
        let Some(entity) = self.substrate.entities.get_mut(stable_id) else {
            return true;
        };
        entity.finish_fall();
        true
    }
}

#[cfg(all(test, tests_sim_movement))]
mod canopy_tests {
    use super::*;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::anim_class::AnimWorldCoord;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::Simulation;

    fn fixture() -> (Simulation, RuleSet, u64) {
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[General]\nParachute=PARACH\n[E1]\nStrength=125\n",
        ))
        .expect("rules");
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[PARACH]\nRate=900\nLoopStart=2\nLoopEnd=5\nLoopCount=-1\n\
             [VEHCHUTE]\nRate=900\nLoopStart=2\nLoopEnd=5\nLoopCount=-1\n",
        ));
        art.bind_anim_frame_count_for_test("PARACH", 6);
        art.bind_anim_frame_count_for_test("VEHCHUTE", 8);
        // Synthetic fixture supplies ART directly, without native read-admission replay.
        rules.install_art_fixture(art);

        let mut sim = Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        let id = sim.substrate.allocate_stable_id();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(id, "E1", "Americans", 10, 11));
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(false);
        assert!(begin_parachute_descent(
            &mut sim.substrate.entities,
            id,
            600
        ));
        (sim, rules, id)
    }

    /// `0x005F5A9D..0x005F5B03`: the object's coordinate plus 75 leptons of Z,
    /// the constructor row, then `SetOwnerObject(object)`.
    #[test]
    fn landing_targets_the_held_canopy_and_old_expiry_keeps_its_replacement() {
        let (mut sim, rules, id) = fixture();
        let first = sim.attach_parachute_anim(&rules, id).unwrap();
        let current = sim.attach_parachute_anim(&rules, id).unwrap();
        assert_eq!(
            sim.entities().get(id).unwrap().parachute_animation(),
            Some(current)
        );
        Simulation::wind_down_parachute_anim(&mut sim.substrate, id);
        assert_eq!(sim.anim(current).unwrap().runtime().loop_remaining, 0);
        assert_eq!(
            sim.anim(first).unwrap().runtime().loop_remaining,
            u8::MAX,
            "same owner and type do not make the old Anim the held canopy"
        );
        sim.notify_anim_pointer_expired(first);
        assert_eq!(
            sim.entities().get(id).unwrap().parachute_animation(),
            Some(current)
        );
        sim.notify_anim_pointer_expired(current);
        assert_eq!(sim.entities().get(id).unwrap().parachute_animation(), None);
        assert!(
            sim.entities().get(id).unwrap().is_parachuted(),
            "the physical +84 flag is independent of the +88 reference"
        );
    }

    #[test]
    fn canopy_identity_roundtrips_and_reenters_the_expiry_index() {
        let (mut sim, rules, id) = fixture();
        let canopy = sim.attach_parachute_anim(&rules, id).unwrap();
        // Entity serde is the production snapshot payload. Keep this regression
        // on the new retained field rather than synthetic envelope metadata.
        let bytes = serde_json::to_vec(sim.entities().get(id).unwrap()).unwrap();
        let expected = sim.state_hash();
        sim.notify_anim_pointer_expired(canopy);
        assert_ne!(sim.state_hash(), expected);
        let restored: GameEntity = serde_json::from_slice(&bytes).unwrap();
        sim.substrate.entities.insert(restored);
        assert_eq!(sim.state_hash(), expected);
        assert_eq!(
            sim.entities().get(id).unwrap().parachute_animation(),
            Some(canopy)
        );
        sim.notify_anim_pointer_expired(canopy);
        assert_eq!(
            sim.entities().get(id).unwrap().parachute_animation(),
            None,
            "the anim-holder index notes a restored entity's held canopy"
        );
    }

    /// `0x005F5A9D..0x005F5B03`: the object's coordinate plus 75 leptons of Z,
    /// the constructor row, then `SetOwnerObject(object)`.
    #[test]
    fn the_canopy_is_built_75_leptons_above_the_falling_object_and_rides_it() {
        let (mut sim, rules, id) = fixture();
        let anim_id = sim.attach_parachute_anim(&rules, id).expect("canopy");
        let anim = sim.anim(anim_id).unwrap();
        assert_eq!(sim.interner.resolve(anim.type_id()), "PARACH");
        assert_eq!((anim.draw_flags(), anim.z_adjust()), (0x600, 0));
        assert_eq!(anim.owner_entity(), Some(id));
        assert_eq!(
            anim.remap(),
            Some(crate::sim::anim_class::AnimRemap::House(
                sim.entities().get(id).unwrap().owner()
            )),
            "Paradrop copies the owner's ConvertClass to Anim+0xD4"
        );
        assert_eq!(
            anim.world_coord(),
            AnimWorldCoord { x: 0, y: 0, z: 75 },
            "stored owner-relative: only the lift"
        );
        let body = sim.anim_owner_coords(id).unwrap();
        assert_eq!(body.z, 600, "the object hangs at its drop Z");
        assert_eq!(
            sim.anim_absolute_coord(anim_id),
            Some(AnimWorldCoord {
                z: body.z + 75,
                ..body
            })
        );

        // A few frames of descent: the canopy comes down with the object.
        for _ in 0..6 {
            sim.advance_fall(
                id,
                -3,
                crate::rules::ruleset::RuleSet::empty_for_test(),
                None,
                crate::sim::world::FrameEffects::default(),
            );
        }
        let lower = sim.anim_owner_coords(id).unwrap().z;
        assert!(lower < 600);
        assert_eq!(sim.anim_absolute_coord(anim_id).unwrap().z, lower + 75);
        let cell = sim.entities().get(id).unwrap().position;
        assert!(
            matches!(sim.lighting_sources.pending().as_ref(),
            [crate::sim::light_sources::LightingEvent::RetainAnimIntensity {
                anim, cell: Some(sampled),
            }] if *anim == anim_id && *sampled == (cell.rx(), cell.ry())),
            "Paradrop samples once at attachment; descent never resamples"
        );
    }

    /// `0x005F3F9D`: landing zeroes the canopy's remaining loops; it is not
    /// removed, it plays to its loop end and leaves on its own.
    #[test]
    fn landing_winds_the_canopy_down_instead_of_removing_it() {
        let (mut sim, rules, id) = fixture();
        let anim_id = sim.attach_parachute_anim(&rules, id).expect("canopy");
        assert_eq!(sim.anim(anim_id).unwrap().runtime().loop_remaining, u8::MAX);

        crate::sim::world::Simulation::wind_down_parachute_anim(&mut sim.substrate, id);

        let anim = sim.anim(anim_id).expect("still in the store");
        assert_eq!(anim.runtime().loop_remaining, 0);
        assert!(!anim.runtime().inactive);
        assert_eq!(anim.owner_entity(), Some(id));
    }

    /// Ares `HouseTypeExt::GetParachuteAnim @ 0x1001BBC0`: a Side's explicit
    /// `Parachute.Anim=none` falls back to `PARACH`, not to no canopy.
    #[test]
    fn ares_side_parachute_none_falls_back_to_parach() {
        let _ares = crate::rules::ares_switch::read_ares_way(true);
        let processed = crate::rules::native_processing::RulesLayerStack::new(IniFile::from_str(
            "[General]\nParachute=OTHER\n[Sides]\nGDI=x\nNod=x\nThirdSide=x\nFourth=x\n\
             [Fourth]\nParachute.Anim=none\n",
        ))
        .process()
        .expect("side rules");
        let rules = RuleSet::from_processed_rules(&processed).expect("side projection");
        let entity = GameEntity::test_default(1, "E1", "Americans", 10, 11);
        assert_eq!(
            parachute_anim_name(&rules, &entity, None, 3, None),
            Some("PARACH")
        );
    }

    #[test]
    fn no_parachute_type_no_canopy() {
        let (mut sim, mut rules, id) = fixture();
        rules.general.parachute_shp = None;
        assert_eq!(sim.attach_parachute_anim(&rules, id), None);
        crate::sim::world::Simulation::wind_down_parachute_anim(&mut sim.substrate, id);
        assert_eq!(sim.substrate.anims.iter().count(), 0);
    }

    /// The production attachment and wind-down honor the compiled capability,
    /// even when a caller supplies an extension name directly on GeneralRules.
    #[test]
    fn vehicle_parachute_selection_and_landing_respect_the_capability() {
        let (mut sim, mut rules, id) = fixture();
        rules.general.vehicle_parachute = Some("VEHCHUTE".to_string());
        let anim_id = sim.attach_parachute_anim(&rules, id).expect("canopy");
        let expected = if crate::rules::NPATCH_ENABLED {
            "VEHCHUTE"
        } else {
            "PARACH"
        };
        assert_eq!(
            sim.interner.resolve(sim.anim(anim_id).unwrap().type_id()),
            expected
        );
        crate::sim::world::Simulation::wind_down_parachute_anim(&mut sim.substrate, id);
        assert_eq!(sim.anim(anim_id).unwrap().runtime().loop_remaining, 0);

        // Infantry keeps Parachute next to the vehicle's custom canopy;
        // an unset vehicle pointer falls back to it (NPSE C2FA75/C2F939).
        let infantry = sim.substrate.allocate_stable_id();
        sim.substrate.entities.insert(
            crate::sim::game_entity::GameEntity::test_default_of_category(
                infantry,
                "E1",
                "Americans",
                12,
                10,
                crate::map::entities::EntityCategory::Infantry,
            ),
        );
        sim.substrate
            .entities
            .get_mut(infantry)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(false);
        let anim_id = sim.attach_parachute_anim(&rules, infantry).expect("canopy");
        assert_eq!(
            sim.interner.resolve(sim.anim(anim_id).unwrap().type_id()),
            "PARACH"
        );

        // Without the key the vehicle falls back to `Parachute=`.
        rules.general.vehicle_parachute = None;
        let vehicle_id = sim.substrate.allocate_stable_id();
        sim.substrate
            .entities
            .insert(crate::sim::game_entity::GameEntity::test_default(
                vehicle_id,
                "E1",
                "Americans",
                14,
                10,
            ));
        sim.substrate
            .entities
            .get_mut(vehicle_id)
            .unwrap()
            .lifecycle
            .set_in_limbo_for_test(false);
        let fallback = sim
            .attach_parachute_anim(&rules, vehicle_id)
            .expect("canopy");
        assert_eq!(
            sim.interner.resolve(sim.anim(fallback).unwrap().type_id()),
            "PARACH"
        );
    }

    /// NPSE ReadGeneral C31005..C31017 allocates the named AnimType and
    /// retains its pointer on an empty read; FindOrAllocate clears `none`.
    #[test]
    fn vehicle_parachute_reader_admits_the_asset_and_retains_layered_values() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};

        let mut layers =
            RulesLayerStack::new(IniFile::from_str("[General]\nVehicleParachute=vehchute\n"));
        let art = IniFile::from_str("[vehchute]\nRate=900\nLoopCount=-1\n");
        let rules =
            RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap()).unwrap();
        assert_eq!(
            rules.general.vehicle_parachute.as_deref(),
            crate::rules::NPATCH_ENABLED.then_some("VEHCHUTE")
        );
        assert_eq!(
            rules
                .anim_type_names
                .iter()
                .any(|name| name.eq_ignore_ascii_case("vehchute")),
            crate::rules::NPATCH_ENABLED,
            "a canopy referenced only by this key must be allocated and read"
        );
        assert_eq!(
            crate::rules::effect_asset_catalog::anim_class_roots(&rules)
                .iter()
                .any(|name| name.eq_ignore_ascii_case("vehchute")),
            crate::rules::NPATCH_ENABLED,
            "the production loader must bind the custom canopy"
        );
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[General]\nVehicleParachute=\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            rules.general.vehicle_parachute.as_deref(),
            crate::rules::NPATCH_ENABLED.then_some("VEHCHUTE")
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[General]\nVehicleParachute=<none>\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(rules.general.vehicle_parachute, None);
    }

    /// Phobos `ObjectClass_Update_FallingDown_ToDead` (hook `0x5F4021`,
    /// `Ext/Techno/Hooks.cpp:1014-1043`): only the `IsABomb` arm reads and
    /// clears `TechnoExt::HoverShutdown`, and `HoverDrownable` then picks the
    /// damage packet.
    #[cfg(feature = "phobos")]
    #[test]
    fn only_the_bomb_landing_arm_takes_the_hover_shutdown() {
        fn land(bomb: bool) -> bool {
            let rules = RuleSet::from_ini(&IniFile::from_str(
                "[General]\nHoverDrownable=no\n[InfantryTypes]\n0=E1\n[E1]\nStrength=125\n",
            ))
            .expect("rules");
            let mut sim = Simulation::new();
            sim.interner = crate::sim::intern::test_interner();
            let id = sim.substrate.allocate_stable_id();
            let mut entity = GameEntity::test_default(id, "E1", "Americans", 10, 11);
            entity.lifecycle.set_in_limbo_for_test(false);
            entity.health.set_for_test(5000);
            // Grounded, so the first frame is the landing frame.
            entity.position.set_exact_z_leptons_for_test(Some(0));
            if bomb {
                entity.begin_bomb_fall();
            } else {
                entity.begin_parachute_fall(0);
            }
            entity.mark_hover_shutdown();
            sim.substrate.entities.insert(entity);
            sim.object_ai_fall(id, &rules, None, FrameEffects::default())
                .expect("landing arm");
            sim.substrate
                .entities
                .get(id)
                .expect("bomb landing retains the member")
                .hover_shutdown()
        }

        assert!(!land(true), "the bomb arm reads and clears the flag");
        assert!(
            land(false),
            "a canopy landing is not a bomb and never reads it"
        );
    }
}

/// `Ext/Bullet/Hooks.cpp:128-137`: the parabomb rate is the step/cap shape,
/// with the step subtracted and the cap a signed lower bound.
#[cfg(all(test, tests_sim_movement, feature = "phobos"))]
mod parabomb_rate_tests {
    use super::parachuted_bullet_rate;

    /// Native: `fallRate = ParabombFallRate - Parachuted.FallRate;` then
    /// `if (fallRate < maxFallRate) fallRate = maxFallRate;` with
    /// `ParabombFallRate` starting at `0` (`Ext/Bullet/Body.h:61`). A step of
    /// 1 therefore walks the rate one lepton per frame until it reaches the
    /// cap, and the cap holds it there.
    #[test]
    fn the_accumulated_rate_reaches_the_cap_and_stays_there() {
        let mut rate = 0;
        let frames: Vec<i32> = (0..8)
            .map(|_| {
                rate = parachuted_bullet_rate(rate, 1, -7);
                rate
            })
            .collect();
        assert_eq!(frames, [-1, -2, -3, -4, -5, -6, -7, -7]);
    }

    /// A slower step walks the rate down to the cap one frame at a time.
    #[test]
    fn a_small_step_reaches_the_cap_late() {
        let mut rate = 0;
        let frames: Vec<i32> = (0..5)
            .map(|_| {
                rate = parachuted_bullet_rate(rate, 3, -7);
                rate
            })
            .collect();
        assert_eq!(frames, [-3, -6, -7, -7, -7]);
    }

    /// The native `if (fallRate < maxFallRate) fallRate = maxFallRate;` is a
    /// signed maximum: a rate already above (less negative than) the cap
    /// keeps its step, while only a rate below the cap is raised to it. The
    /// cap is not applied to the step before the subtraction.
    #[test]
    fn the_cap_is_the_only_brake() {
        assert_eq!(
            parachuted_bullet_rate(-4, 1, -7),
            -5,
            "a rate above the cap is not raised to it"
        );
        assert_eq!(
            parachuted_bullet_rate(-6, 1, -7),
            -7,
            "the step lands exactly on the cap"
        );
        assert_eq!(
            parachuted_bullet_rate(-8, 1, -7),
            -7,
            "a rate below the cap is raised to it"
        );
        assert_eq!(
            parachuted_bullet_rate(-8, 1, -100),
            -9,
            "a deeper cap leaves the step unchanged"
        );
    }

    /// The native expression is `ParabombFallRate - Parachuted.FallRate` in
    /// `int` arithmetic; `wrapping_sub` keeps that overflow behaviour. An
    /// overflow can wrap the rate above the cap, which the maximum then keeps.
    #[test]
    fn the_step_subtracts_with_native_int_overflow() {
        assert_eq!(
            parachuted_bullet_rate(i32::MIN, 1, i32::MIN),
            i32::MAX,
            "i32::MIN - 1 wraps to i32::MAX, above the i32::MIN cap"
        );
        assert_eq!(parachuted_bullet_rate(0, i32::MIN, i32::MIN), i32::MIN);
    }
}

#[cfg(all(test, tests_sim_movement, feature = "phobos"))]
mod falling_damage_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::combat::Health;
    use crate::sim::world::Simulation;

    fn scene(type_keys: &str, global: &str, water: bool) -> (Simulation, RuleSet, u64) {
        let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
            "[VehicleTypes]
0=FALLER
             [FALLER]
Strength=1000
Armor=none
Speed=4
{type_keys}             [CombatDamage]
{global}"
        ));
        let rules = RuleSet::from_ini(&ini).expect("falling rules");
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 3);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let id = sim
            .spawn_object("FALLER", "Americans", 10, 10, 0, &rules)
            .expect("faller");
        sim.entities_mut_for_test().get_mut(id).unwrap().health = Health::new(400);
        sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(32, 32, |x, y| {
            let mut cell = crate::map::resolved_terrain::ResolvedTerrainCell::clear_for_test(x, y);
            if water && (x, y) == (10, 10) {
                cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
            }
            cell
        }));
        (sim, rules, id)
    }

    /// `Ext/Techno/Hooks.cpp:1014-1090`: a negative ratio takes that fraction
    /// of the current health, a ratio up to 1.0 that fraction of Strength and
    /// a value above 1.0 is absolute; the type key falls back to the global.
    #[test]
    fn the_three_falling_damage_tiers_match_the_hook() {
        let (sim, rules, id) = scene(
            "FallingDownDamage=0.5
",
            "",
            false,
        );
        assert_eq!(
            sim.phobos_falling_damage(id, 400, &rules),
            500,
            "positive ratio uses Strength"
        );
        let (sim, rules, id) = scene(
            "FallingDownDamage=-0.5
",
            "",
            false,
        );
        assert_eq!(
            sim.phobos_falling_damage(id, 400, &rules),
            200,
            "negative takes that fraction of the current health"
        );
        let (sim, rules, id) = scene(
            "",
            "FallingDownDamage=0.25
",
            false,
        );
        assert_eq!(
            sim.phobos_falling_damage(id, 400, &rules),
            250,
            "the global fallback"
        );
        let (sim, rules, id) = scene(
            "FallingDownDamage.Water=2
",
            "FallingDownDamage=0.25
",
            true,
        );
        assert_eq!(
            sim.phobos_falling_damage(id, 400, &rules),
            2,
            "water selects the type Water value, absolute above 1.0"
        );
    }

    /// `FallingDownDamage.AllowEMP=no`: an EMP-locked unit keeps the vanilla
    /// full-current-Health packet.
    #[test]
    fn emp_allow_emp_no_keeps_the_full_health_packet() {
        let (mut sim, rules, id) = scene(
            "FallingDownDamage.AllowEMP=no
FallingDownDamage=0.5
",
            "",
            false,
        );
        sim.entities_mut_for_test()
            .get_mut(id)
            .unwrap()
            .set_emp_remaining_for_test(30);
        assert_eq!(sim.phobos_falling_damage(id, 400, &rules), 400);
    }

    /// A hover unit that shut down over a **non-drowning** cell falls through
    /// into the same ratio block as the canopy arm
    /// (`Ext/Techno/Hooks.cpp:1038-1084`): the shutdown arm is only the
    /// `HoverDrownable` answer, not a whole-`Strength` packet of its own.
    #[test]
    fn a_non_drowning_hover_shutdown_takes_the_falling_down_ratio() {
        fn landing(keys: &str, global: &str) -> (Option<i32>, bool) {
            let (mut sim, rules, id) = scene(keys, global, false);
            let entity = sim.entities_mut_for_test().get_mut(id).unwrap();
            entity.position.set_exact_z_leptons_for_test(Some(0));
            entity.begin_bomb_fall();
            entity.mark_hover_shutdown();
            sim.object_ai_fall(id, &rules, None, FrameEffects::default())
                .expect("landing arm");
            let health = sim
                .substrate
                .entities
                .get(id)
                .map(|entity| entity.health.current());
            let cleared = sim
                .substrate
                .entities
                .get(id)
                .is_none_or(|entity| !entity.hover_shutdown());
            (health, cleared)
        }

        // Strength=1000, current Health=400, so 0.25 is a 250 packet while the
        // default ratio (1.0) packets the whole Strength and kills the unit.
        // The arena carries no `[C4]`, so name and declare one: an unresolved
        // warhead zeroes the packet and would hide the arm.
        let c4 = "C4Warhead=C4
[Warheads]
0=C4
[C4]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";
        let (health, cleared) = landing(
            "HoverDrownable=no
FallingDownDamage=0.25
",
            c4,
        );
        assert!(cleared, "the landing arm reads and clears the flag");
        assert_eq!(health, Some(150), "the ratio scales the packet");
        let (health, _) = landing(
            "HoverDrownable=no
",
            c4,
        );
        assert!(
            health.is_none_or(|current| current <= 0),
            "the default ratio is 1.0, which still kills: {health:?}"
        );
    }

    /// `if (!onParachuted)` (`Ext/Techno/Hooks.cpp:1067-1084`): a Techno issued
    /// as a parachute fall (`FootClass_Paradrop`, `:1907`) lands with a 0
    /// packet, so neither the EMP arm nor the ratio runs. The flag is read and
    /// cleared on that landing (`:1030-1031`), before the `IsABomb` gate.
    #[test]
    fn a_parachuted_landing_skips_the_falling_down_ratio() {
        const C4: &str = "C4Warhead=C4
[Warheads]
0=C4
[C4]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";
        fn landing(parachuted: bool) -> (Option<i32>, bool) {
            let (mut sim, rules, id) = scene("FallingDownDamage=0.25\n", C4, false);
            let entity = sim.entities_mut_for_test().get_mut(id).unwrap();
            entity.position.set_exact_z_leptons_for_test(Some(0));
            entity.begin_bomb_fall();
            if parachuted {
                entity.mark_on_parachuted();
            }
            sim.object_ai_fall(id, &rules, None, FrameEffects::default())
                .expect("landing arm");
            let health = sim
                .substrate
                .entities
                .get(id)
                .map(|entity| entity.health.current());
            let cleared = sim
                .substrate
                .entities
                .get_mut(id)
                .is_none_or(|entity| !entity.take_on_parachuted());
            (health, cleared)
        }

        let (health, cleared) = landing(true);
        assert!(cleared, "the landing arm reads and clears the flag");
        assert_eq!(
            health,
            Some(400),
            "the 0.25 ratio (a 100 packet) is skipped, so the 400 stands"
        );
        let (health, _) = landing(false);
        assert_eq!(health, Some(150), "without the flag the ratio still runs");
    }

    /// `CellClass::IsClearToMove(SpeedType, true, true, -1, MovementZone, -1,
    /// onBridge)` (`Ext/Techno/Hooks.cpp:1043-1055`): a landing cell the mover's
    /// `MovementZone` cannot pass takes the whole current Health before the
    /// ratio block runs, so a low `FallingDownDamage` cannot save it.
    #[test]
    fn a_blocked_landing_cell_takes_the_whole_health_packet() {
        const C4: &str = "C4Warhead=C4
[Warheads]
0=C4
[C4]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";
        fn land(zone: Option<u8>) -> (bool, Option<i32>) {
            // Water cell with the type's Water ratio set low: the ratio packet
            // would leave 300 of 400, the blocked arm takes all of it.
            let (mut sim, rules, id) = scene(
                "FallingDownDamage.Water=0.1
",
                C4,
                true,
            );
            if let Some(zone) = zone {
                sim.resolved_terrain
                    .as_mut()
                    .expect("scene grid")
                    .cell_mut_for_test(10, 10)
                    .expect("water cell")
                    .zone_type = zone;
            }
            let clear = sim.landing_cell_clear(id, &rules);
            let entity = sim.entities_mut_for_test().get_mut(id).unwrap();
            entity.position.set_exact_z_leptons_for_test(Some(0));
            entity.begin_bomb_fall();
            sim.object_ai_fall(id, &rules, None, FrameEffects::default())
                .expect("landing arm");
            let health = sim
                .substrate
                .entities
                .get(id)
                .map(|entity| entity.health.current());
            (clear, health)
        }

        // Reduced ZoneType 4 is the Water column, which only the water rows of
        // `MOVEMENT_ZONE_PASSABILITY` pass; a Normal mover never does.
        let (clear, health) = land(Some(4));
        assert!(
            !clear,
            "the water column is not clear for MovementZone Normal"
        );
        assert!(
            health.is_none_or(|current| current <= 0),
            "the whole current Health goes: {health:?}"
        );
        // The same cell with its ground class left as authored: clear, so the
        // type's Water ratio applies.
        let (clear, health) = land(None);
        assert!(clear, "a ground-classed cell is clear");
        assert_eq!(health, Some(300), "0.1 of Strength 1000 is a 100 packet");
    }

    /// `Ext/Techno/Hooks.cpp:1043-1055` passes `pCell->ContainsBridge()`, and
    /// with level `-1` that moves the whole check onto the deck
    /// (`0x0048353C..0x0048354E`, land half skipped at `0x004835DE`): the water
    /// ground under the deck stops refusing the landing, and only the deck
    /// occupation byte survives the two ignore masks (`0xE0`, then `0x5F`,
    /// leaving `0x40`).
    #[test]
    fn a_bridge_deck_landing_reads_the_deck_plane_not_the_water_ground() {
        let (mut sim, rules, id) = scene("", "", true);
        // Reduced ZoneType 4 is the Water column, which the ground arms refuse
        // for a Normal mover; the same cell with the Clear column is admitted.
        {
            let cell = sim
                .resolved_terrain
                .as_mut()
                .expect("scene grid")
                .cell_mut_for_test(10, 10)
                .expect("water cell");
            cell.zone_type = 4;
        }
        assert!(
            !sim.landing_cell_clear(id, &rules),
            "the water ground refuses the landing off a bridge"
        );
        sim.resolved_terrain
            .as_mut()
            .expect("scene grid")
            .cell_mut_for_test(10, 10)
            .expect("water cell")
            .bridge_facts
            .raw_flags |= crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
        assert!(
            sim.landing_cell_clear(id, &rules),
            "the deck plane admits the same landing"
        );
        // A structure on the deck is the one bit both ignore masks leave.
        sim.substrate.raw_cell_occupation.mark_deck(10, 10, 0x40);
        assert!(
            !sim.landing_cell_clear(id, &rules),
            "an occupied deck refuses the landing"
        );
    }
}
