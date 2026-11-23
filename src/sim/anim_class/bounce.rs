//! The `Bouncer=` chunk: its constructor launch, its `BounceClass` flight,
//! its contact and landing anims and the Tiberium a landing leaves.

use super::{AnimId, AnimWorldCoord, bounce_spawn_count};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::art_data::AnimTypeRuntimeConfig;
use crate::rules::ruleset::RuleSet;
use crate::sim::bounce::{BounceOutcome, BounceState};
use crate::sim::movement::ground_pose::deck_surface_z;
use crate::sim::overlay_mark_host::{OverlayRecalc, RuntimeOverlayError};
use crate::sim::projectile::{ProjectileCoord, projectile_ground_z};
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
use crate::util::native_x87::{NativeF64Bits, NativeX87Error, X87Chop53};

/// `AnimTypeClass+0x320` MaxZVel: no INI key reads it; the AnimType
/// constructor writes 3.5 (`0x00427627`).
const BOUNCER_MAX_Z_VEL: NativeF64Bits = NativeF64Bits::from_bits(0x400c_0000_0000_0000);
/// The gravity the Bouncer arm hands `BounceClass::Init` as two literal pushes
/// (`0x3FF66666`/`0x60000000`, the double 1.4).
pub(super) const BOUNCER_GRAVITY: NativeF64Bits = NativeF64Bits::from_bits(0x3ff6_6666_6000_0000);
/// The `+ 1.0` bias of the Z divisor (`0x007E1718`).
const BOUNCER_Z_RANGE_BIAS: NativeF64Bits = NativeF64Bits::from_bits(0x3ff0_0000_0000_0000);
/// The body starts this far above the anim's coordinate.
const BOUNCER_LAUNCH_LIFT_LEPTONS: i32 = 10;
/// `AnimClass` constructor `drawFlags` for the anims a landing chunk makes:
/// `BounceAnim=`, `Wake=` and the splash (`0x600`), `ExpireAnim=` (`0x2600`,
/// zAdjust -30).
const BOUNCE_CONTACT_DRAW_FLAGS: u32 = 0x600;
const BOUNCE_EXPIRE_DRAW_FLAGS: u32 = 0x2600;
const BOUNCE_EXPIRE_Z_ADJUST: i32 = -30;
/// A water splash rises 3 leptons above the chunk (`ADD EDI, 3`, `0x00423DBE`).
const BOUNCE_SPLASH_LIFT_LEPTONS: i32 = 3;

/// The `Bouncer=` arm of `AnimClass::AnimClass` (`0x004224D9..0x00422648`):
/// three `Random__Next()` draws, Z then Y then X, each taken as `|draw| %
/// divisor` (`CDQ/XOR/SUB`, `IDIV`):
/// - `Velocity.Z = |a| % ftol(MaxZVel - MinZVel + 1.0) + MinZVel`
/// - `Velocity.Y = |b| % ftol(MaxXYVel + MaxXYVel) - MaxXYVel`, X likewise;
///
/// then `BounceClass::Init` from 10 leptons above the coordinate with the
/// type's `Elasticity=`, gravity 1.4 and no spin, which draws three more.
/// With the stock chunks' `MinZVel` above the fixed `MaxZVel` 3.5, the Z
/// divisor is negative (-20 large, -15 small).
///
/// Native execution: `tools/spatial_oracle/anim_bouncer_launch.py`.
pub(super) fn bouncer_launch(
    config: &AnimTypeRuntimeConfig,
    coord: AnimWorldCoord,
    rng: &mut crate::sim::rng::SimRng,
) -> Result<BounceState, NativeX87Error> {
    use crate::sim::rng::abs_before_modulo;

    let z_draw = rng.next_u32();
    let y_draw = rng.next_u32();
    let x_draw = rng.next_u32();
    let max_xy = X87Chop53::load_f64(config.max_xy_vel)?;
    let min_z = X87Chop53::load_f64(config.min_z_vel)?;
    let xy_divisor = X87Chop53::ftol_i64(X87Chop53::add(max_xy, max_xy))? as i32;
    let z_divisor = X87Chop53::ftol_i64(X87Chop53::add(
        X87Chop53::sub(X87Chop53::load_f64(BOUNCER_MAX_Z_VEL)?, min_z),
        X87Chop53::load_f64(BOUNCER_Z_RANGE_BIAS)?,
    ))? as i32;
    let velocity_z = X87Chop53::store_f32(X87Chop53::add(
        X87Chop53::load_i32(abs_before_modulo(z_draw, z_divisor)),
        min_z,
    ))?;
    let velocity_y = X87Chop53::store_f32(X87Chop53::sub(
        X87Chop53::load_i32(abs_before_modulo(y_draw, xy_divisor)),
        max_xy,
    ))?;
    let velocity_x = X87Chop53::store_f32(X87Chop53::sub(
        X87Chop53::load_i32(abs_before_modulo(x_draw, xy_divisor)),
        max_xy,
    ))?;
    BounceState::init(
        glam::IVec3::new(
            coord.x,
            coord.y,
            coord.z.wrapping_add(BOUNCER_LAUNCH_LIFT_LEPTONS),
        ),
        config.elasticity,
        BOUNCER_GRAVITY,
        NativeF64Bits::POSITIVE_ZERO,
        [velocity_x, velocity_y, velocity_z],
        NativeF64Bits::POSITIVE_ZERO,
        rng,
    )
}

impl Simulation {
    /// Anim423C4A and VoxelAnim749FA2 use the same Z cutoff for water
    /// expiry. Preserve the native ground probe (including a Dummy miss)
    /// and its +4 cutoff when the cell has no live fixed deck.
    pub(crate) fn bounce_landing_deck_z(&self, position: glam::IVec3) -> i32 {
        let ground = projectile_ground_z(
            self.resolved_terrain.as_ref(),
            &self.effective_shared_cell_dummy(),
            ProjectileCoord::new(position.x, position.y, position.z),
        );
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return ground.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS);
        };
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup_world(position.x, position.y);
        deck_surface_z(&cells, cell, ground)
    }

    /// `AnimClass::ProcessBounceResult @ 0x00423930` and the landing arm of
    /// `AnimClass::AI` (`0x00423C3C..0x00424298`). Returns true when the chunk
    /// touched down, which always destroys it: Bounced and Stopped both take
    /// the landing arm.
    ///
    /// Each tick the body integrates (`BounceClass::Update`) and the anim's
    /// Location follows it, truncated (`vtable+0x1B4`, `0x00423AA8`). Bounced
    /// first builds `BounceAnim=` and hits every object in the landing cell
    /// within `DamageRadius=` (Manhattan leptons) directly; Stopped destroys
    /// the anim there (`0x00423976`), before the landing arm. Then:
    /// - in water (`CellClass+0xEC == 2`) below the resolved deck plane:
    ///   `Wake=` at the Location and the first `SplashList=` 3 leptons
    ///   up, no damage;
    /// - otherwise, only when the type names an `ExpireAnim=`: that anim
    ///   (flags `0x2600`, zAdjust -30), then `Apply_area_damage @ 0x00489280`
    ///   with `ftol(Damage=)` and the type's `Warhead=`, sourceless and
    ///   houseless, then the combat light (`0x0048A620`, not forced).
    ///
    pub(super) fn anim_bounce_step(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut FrameEffects<'_>,
    ) -> Result<Option<bool>, RuntimeOverlayError> {
        let Some(mut body) = self.anim(id).and_then(|anim| anim.bounce) else {
            return Ok(None);
        };
        let outcome = self.anim_bounce_update(&mut body, rules);
        if config.is_meteor
            && let Err(error) = body.compensate_meteor_gravity()
        {
            log::warn!("meteor anim left the verified x87 domain: {error}");
            self.destroy_anim(id, rules);
            return Ok(Some(false));
        }
        let position = body.position_leptons();
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.bounce = Some(body);
        }
        let mut bridge_state_changed = false;
        match outcome {
            BounceOutcome::Falling => {}
            BounceOutcome::Bounced => {
                bridge_state_changed |= self.anim_bounce_contact(
                    id,
                    config,
                    rules,
                    overlay_registry,
                    position,
                    frame_effects,
                );
            }
            BounceOutcome::Stopped => self.destroy_anim(id, rules),
        }
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.world_coord = AnimWorldCoord {
                x: position.x,
                y: position.y,
                z: position.z,
            };
        }
        if outcome == BounceOutcome::Falling {
            return Ok(None);
        }
        bridge_state_changed |=
            self.anim_bounce_landing(config, rules, overlay_registry, position, frame_effects)?;
        self.destroy_anim(id, rules);
        Ok(Some(bridge_state_changed))
    }

    /// The Bounced arm of `AnimClass::ProcessBounceResult` (`0x00423981..
    /// 0x00423A88`): `BounceAnim=` at GetCoords, then a direct
    /// `ReceiveDamage(ftol(Damage=), AdjustForZ(distance), Warhead=)` on each
    /// object of the landing cell's FirstObject list within `DamageRadius=`,
    /// measured `|dx| + |dy|` from the body to the object's GetCoords.
    pub(super) fn anim_bounce_contact(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        _overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        position: glam::IVec3,
        frame_effects: &mut FrameEffects<'_>,
    ) -> bool {
        if let (Some(bounce_anim), Some(coord)) =
            (config.bounce_anim.as_deref(), self.anim_absolute_coord(id))
        {
            self.spawn_named_anim(rules, bounce_anim, coord, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
        }
        let Some(warhead_name) = config.warhead.as_deref() else {
            return false;
        };
        let damage = match X87Chop53::load_f64(config.damage).and_then(X87Chop53::ftol_i64) {
            Ok(damage) => damage as i32,
            Err(_) => return false,
        };
        // Ares 3.0 AnimClass_UpdateBounce_Damage (1005C410, hook 0x4239F0):
        // a negative DamageRadius or a Damage of zero damages nothing.
        if rules.ares() && (config.damage_radius < 0 || damage == 0) {
            return false;
        }
        let warhead_ref = self.interner.intern(warhead_name);
        self.apply_bounce_contact_damage(
            position,
            config.damage_radius,
            crate::sim::world::BounceContactDamage::Anim(damage),
            warhead_ref,
            rules,
            frame_effects,
        )
    }

    /// The landing arm proper (`0x00423C4A..0x00423EF8`); see
    /// [`Self::anim_bounce_step`].
    pub(super) fn anim_bounce_landing(
        &mut self,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        position: glam::IVec3,
        frame_effects: &mut FrameEffects<'_>,
    ) -> Result<bool, RuntimeOverlayError> {
        let location = AnimWorldCoord {
            x: position.x,
            y: position.y,
            z: position.z,
        };
        let above_deck = position.z >= self.bounce_landing_deck_z(position);
        if self.bounce_cell_is_water(position, rules) && !above_deck {
            //423CDB..423DE2: a meteor uses the LAST splash, without a Wake.
            if !config.is_meteor {
                let wake = &rules.general.wake.name;
                self.spawn_named_anim(rules, wake, location, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
            }
            let splash = if config.is_meteor {
                rules.combat_damage.splash_list.last()
            } else {
                rules.combat_damage.splash_list.first()
            };
            if let Some(splash) = splash {
                let splash_coord = AnimWorldCoord {
                    z: location.z.wrapping_add(BOUNCE_SPLASH_LIFT_LEPTONS),
                    ..location
                };
                self.spawn_named_anim(rules, splash, splash_coord, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
            }
            return Ok(false);
        }
        //423C4A..423EFD: only the explosion prefix requires ExpireAnim.
        //The Spawns/IsTiberium continuation still runs without it or a Warhead.
        let mut bridge_state_changed = false;
        if let Some(expire) = config.expire_anim.as_deref() {
            self.spawn_named_anim(
                rules,
                expire,
                location,
                0,
                BOUNCE_EXPIRE_DRAW_FLAGS,
                BOUNCE_EXPIRE_Z_ADJUST,
            );
            if let (Some(warhead_name), Ok(damage)) = (
                config.warhead.as_deref(),
                X87Chop53::load_f64(config.damage).and_then(X87Chop53::ftol_i64),
            ) && let Some(warhead) = rules.warhead(warhead_name)
            {
                let damage = damage as i32;
                let warhead_ref = self.interner.intern(warhead_name);
                let impact = crate::sim::projectile::ProjectileCoord::new(
                    position.x, position.y, position.z,
                );
                bridge_state_changed = crate::sim::combat::world_receiver::apply_area_damage(
                    self,
                    rules,
                    overlay_registry,
                    impact,
                    damage,
                    warhead,
                    true,
                    (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
                    frame_effects,
                )
                .bridge_state_changed;
                self.combat_light_requests
                    .emit(crate::sim::combat::CombatLightRequest::Impact {
                        target_id: None,
                        damage,
                        warhead_ref,
                        coord: impact,
                        force_create: false,
                        flags: 0,
                    });
            }
        }
        //423F37..423FC4: two inclusive draws precede the live constructor loop.
        //Each child completes constructor/Start before constructing the next.
        if let Some(child) = config.spawns.as_deref().filter(|_| config.spawn_count > 0) {
            let first = self
                .scenario_rng
                .next_range_i32_inclusive(0, config.spawn_count);
            let second = self
                .scenario_rng
                .next_range_i32_inclusive(0, config.spawn_count);
            let count = bounce_spawn_count(true, config.spawn_count, first, second);
            for _ in 0..count {
                self.spawn_named_anim(rules, child, location, 0, BOUNCE_CONTACT_DRAW_FLAGS, 0);
            }
        }
        if config.is_tiberium && !above_deck {
            self.anim_landing_tiberium(config, rules, overlay_registry, position, frame_effects)?;
        }
        Ok(bridge_state_changed)
    }

    /// AnimAI423FD0..424269. The radius gates a squared-distance loop, but
    /// the placement direction is ONLY outer_x&7 (89F688), not (x,y).
    /// Repeated visits and CanGerminate-before-null-SpawnType are intentional.
    fn anim_landing_tiberium(
        &mut self,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        position: glam::IVec3,
        frame_effects: &mut FrameEffects<'_>,
    ) -> Result<(), RuntimeOverlayError> {
        use crate::sim::tiberium::{NewTiberiumAdmission, TiberiumPlacementObjectContext};
        let Some(registry) = registry else {
            return Ok(());
        };
        let radius = config.tiberium_spread_radius;
        let first = radius.wrapping_neg();
        if first > radius {
            return Ok(());
        }
        for x in first..=radius {
            for y in first..=radius {
                if crate::util::native_x87::sqrt_approx_length([x, y]) > radius {
                    continue;
                }
                let (dx, dy) = crate::sim::tiberium::ADJACENT_OFFSETS[(x & 7) as usize];
                let cell = (
                    (position.x / 256).wrapping_add(dx) as u16,
                    (position.y / 256).wrapping_add(dy) as u16,
                );
                let admitted = {
                    let (Some(terrain), Some(grid)) =
                        (self.resolved_terrain.as_ref(), self.overlay_grid.as_ref())
                    else {
                        return Ok(());
                    };
                    let objects = TiberiumPlacementObjectContext::new(
                        &self.substrate.entities,
                        &self.substrate.occupancy,
                        &self.substrate.raw_cell_occupation,
                        rules,
                        &self.interner,
                        self.terrain_objects.cell_index(),
                    );
                    crate::sim::tiberium::admit_new_tiberium_target(
                        grid,
                        self.terrain_objects.spawning_cells(),
                        NewTiberiumAdmission::runtime(terrain, objects),
                        cell,
                    )
                };
                let Some(cell) = admitted else {
                    continue;
                };
                let Some(base) = config
                    .tiberium_spawn_type
                    .as_deref()
                    .and_then(|name| registry.id_for_name(name))
                else {
                    continue;
                };
                let variant = self.scenario_rng.next_range_i32_inclusive(0, 3);
                if let Ok(selected) = u8::try_from(i32::from(base).wrapping_add(variant)) {
                    crate::sim::crates::construct_runtime_overlay(
                        self,
                        &rules.crate_rules,
                        rules,
                        registry,
                        cell,
                        selected,
                        OverlayRecalc::Immediate,
                        frame_effects,
                    )?;
                }
                //424140 executes even when Overlay Unlimbo refused the cell.
                let density = self.scenario_rng.next_range_i32_inclusive(0, 2) as u8;
                if let Some(grid) = self.overlay_grid.as_mut() {
                    //424155: like Place487190's post-constructor store, a
                    //literal Cell+11E write even when no overlay was installed.
                    grid.write_cell_data(cell.0, cell.1, density);
                }
                self.dirty_cells.mark_tactical_once(cell);
                //Anim's ore arm does not call AddToGrowthQueue or radar6551C0.
            }
        }
        Ok(())
    }
}
