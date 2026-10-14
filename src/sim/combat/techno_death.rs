//! `TechnoClass::ReceiveDamage`'s shared death block (`0x00701900`): the
//! death weapon, voxel debris and death sounds, the `Death_Announcement`
//! input, and the fatal lifecycle stages the world runs around them.

use super::*;

/// The `Death_Announcement` (`+0x3B8`) input for one death site.
///
/// Every native caller sits in a non-building `ReceiveDamage` override
/// (`BuildingClass` has none), and `0x004D98DD` skips `Spawned=` types.
/// Called by the damage kill loop only. The non-damage death sites that
/// natively route through `+0x16C` (`ReceiveDamage`) with `C4Warhead=`,
/// `InfantryClass::IronCurtain 0x00522632` and `CellClass::BlowUpBridge
/// 0x0047DDAE`, reach it the same way: VERA kills through that loop too.
/// Paths that natively skip `ReceiveDamage` (crush
/// `0x007416A0` → `RecordKill` only, `AircraftClass::Enter_Idle_Mode
/// 0x004179FD/0x00417B88` → `Crash` slot `+0x3DC`, the off-playfield
/// `UnInit` at `AircraftClass::AI 0x00414F93/0x00414FD1`) must not call it.
pub(crate) fn death_announcement_event(
    obj: &crate::rules::object_type::ObjectType,
    category: EntityCategory,
    rx: u16,
    ry: u16,
    owner: InternedId,
) -> Option<UnitLostEvent> {
    (category != EntityCategory::Structure && !obj.spawned).then_some(UnitLostEvent {
        rx,
        ry,
        owner,
    })
}

fn death_weapon_ftol_product_i32_f32(value: i32, multiplier: f32) -> Option<i32> {
    let multiplier = X87Chop53::load_f32(NativeF32Bits::from_bits(multiplier.to_bits())).ok()?;
    let product = X87Chop53::mul(X87Chop53::load_i32(value), multiplier);
    i32::try_from(X87Chop53::ftol_i64(product).ok()?).ok()
}

fn death_weapon_half_strength(strength: i32) -> Option<i32> {
    let half = X87Chop53::load_f64(NativeF64Bits::HALF).ok()?;
    let product = X87Chop53::mul(X87Chop53::load_i32(strength), half);
    i32::try_from(X87Chop53::ftol_i64(product).ok()?).ok()
}

/// The death arm's gate (`TechnoClass::ReceiveDamage 0x00702572..0x00702601`):
/// Explodes (`+0xD15`), the veteran or elite `EXPLODES` ability (IsVeteran
/// `0x0074FF90` with `+0x2A6`; IsElite `0x00750010` with `+0x2A6` or
/// `+0x2B8`), or `numbered_weapon` is `Suicide=` (`+0x144`): the weapon GetWeapon
/// (vtable `+0x3F8`, [`combat_weapon::get_weapon`]) answers for
/// `CurrentWeaponNumber` (`+0x138`, pushed at `0x0070257A`). `+0x138` is a
/// Gunner transport's passenger IFVMode (SetGunnerWeapon `0x0070DC70`), else
/// 0: it is not the last-fired slot. When it holds, KillPassengers
/// (`0x00702603..0x00702667`) and then Fire_Death_Weapon (`0x00702669`) run;
/// otherwise neither does.
pub(crate) fn death_arm_explodes(
    obj: &ObjectType,
    veterancy: u16,
    numbered_weapon: Option<&WeaponType>,
) -> bool {
    obj.explodes
        || super::veterancy::has_weapon_ability(
            super::veterancy::rank_from_u16(veterancy),
            obj,
            crate::rules::object_type::Ability::Explodes,
        )
        || numbered_weapon.is_some_and(|weapon| weapon.suicide)
}

/// `TechnoClass::Fire_Death_Weapon @ 0x0070D690`'s weapon and damage, before
/// its caller's extra damage: `DeathWeapon=` (`+0xD18`), else the current
/// weapon (`GetCurrentWeapon`, vtable `+0x3F4`, the caller's `current_weapon`),
/// each at `ftol(Damage * DeathWeaponDamageModifier)`
/// (`0x0070D6EB..0x0070D6F7`); else `[CombatDamage] DeathWeapon=`
/// (`Rules+0xFDC`) at `ftol(Strength * 0.5)` (`0x0070D6FE..0x0070D71F`). No
/// weapon fires nothing. The function has no `Explodes=` gate: the death arm
/// gates it ([`death_arm_explodes`]); a crash impact calls it bare.
pub(super) fn fire_death_weapon_payload(
    rules: &RuleSet,
    obj: &ObjectType,
    current_weapon: Option<&WeaponType>,
    interner: &mut StringInterner,
) -> Option<(i32, InternedId, InternedId)> {
    let chosen = obj
        .death_weapon
        .as_deref()
        .and_then(|weapon_id| rules.weapon(weapon_id))
        .or(current_weapon);
    if let Some(weapon) = chosen {
        let damage =
            death_weapon_ftol_product_i32_f32(weapon.damage, obj.death_weapon_damage_modifier)?;
        let warhead_ref = interner.intern(weapon.warhead.as_ref()?);
        let weapon_ref = interner.intern(&weapon.id);
        return Some((damage, warhead_ref, weapon_ref));
    }
    let fallback = rules
        .combat_damage
        .death_weapon
        .as_deref()
        .and_then(|weapon_id| rules.weapon(weapon_id))?;
    let warhead_ref = interner.intern(fallback.warhead.as_ref()?);
    let weapon_ref = interner.intern(&fallback.id);
    Some((
        death_weapon_half_strength(obj.strength)?,
        warhead_ref,
        weapon_ref,
    ))
}

/// `TechnoClass::Fire_Death_Weapon @ 0x0070D690` with no extra damage: a real
/// bullet of the [`fire_death_weapon_payload`] weapon (`CreateBullet @
/// 0x0046B050`, owner and target the object itself) detonated at the object's
/// GetCoords (vt+0x48, `0x0070D77C`; a building's foundation centre) through
/// `DetonateAtCoord @ 0x004690B0`. `None` when no weapon fires.
pub(crate) fn death_weapon_detonation(
    world: &mut Simulation,
    id: u64,
    rules: &RuleSet,
) -> Option<ProjectileDetonation> {
    let entity = world.substrate.entities.get(id)?;
    let object = rules.object(world.interner.resolve(entity.type_ref()))?;
    let current_weapon = combat_weapon::current_weapon(
        entity,
        object,
        &world.substrate.entities,
        rules,
        &world.interner,
    );
    let impact = crate::sim::movement::ground_pose::object_get_coords(
        entity,
        world.resolved_terrain.as_ref(),
    );
    let (damage, warhead, weapon) =
        fire_death_weapon_payload(rules, object, current_weapon, &mut world.interner)?;
    Some(ProjectileDetonation {
        projectile_id: id,
        source_id: id,
        target: ProjectileTarget::Entity(id),
        impact: ProjectileCoord::new(impact.x, impact.y, impact.z),
        payload: ProjectilePayload::new(damage, warhead, weapon),
        reason: crate::sim::projectile::ProjectileDetonationReason::DeathWeapon,
    })
}

/// World-owned lifecycle work that brackets the native death helper for a
/// concrete fatal receiver. Passenger teardown precedes the nested death
/// weapon; represented UnInit follows it before the next outer receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FatalLifecycleStage {
    /// Surviving Techno ReceiveDamage postlude, before Infantry scatter and
    /// synchronous retaliation. The world owns ParticleSystem storage and the
    /// shared LogicVector, so maintenance crosses the existing inline hook.
    MaintainDamageSmoke {
        state: damage::DamageState,
    },
    /// ObjectClass's exact-zero callback transaction for an eligible delayed
    /// death. It runs while the target is still represented and Health is
    /// exactly zero, before TechnoClass arms/shortens the shared C4 timer and
    /// restores Alive/Health=1.
    PostMortemExactZero {
        killer_owner: Option<InternedId>,
    },
    BeforeDeathEffects,
    AfterDeathEffects,
}

/// The debris block of `TechnoClass::ReceiveDamage @ 0x00701900`, wired to the
/// dying object. Every draw is on the Scenario stream (`[0x00A8B230]+0x218`:
/// the count at `0x007022BA..0x007022C8`, the per-piece pick at `0x0070232B`,
/// and the VoxelAnim constructor's seven), not the death sounds' stream.
///
/// The entry gate `0x00702232..0x0070227B` skips debris only when GetHeight
/// is <=10, DropAsBomb's persistent `ObjectClass+0x8F` is set and the raw
/// Location cell is water. `ObjectClass::DropAsBomb @ 0x005F4160` sets the
/// byte; it remains set after landing. An ordinary water death, or a bomb
/// killed above ten leptons, reaches MaxDebris and consumes the block's draws.
/// Original instructions: `tools.native_inspect` on retail gamemd.exe.
///
/// The SHP half constructs a bouncing `AnimClass` per piece (`0x00421EA0`):
/// every stock debris AnimType is `Bouncer=yes` — all 26 named by `[General]
/// MetallicDebris=` or by any `DebrisAnims=` line, authored in `artmd.ini`
/// (`AnimTypeClass+0x35A`, read at `0x004286A7`). Each piece's constructor
/// draws run right after its pick and the piece enters Logic before the next
/// pick. Stock DBRIS actively reads RandomRate=220,600; native428784/42879F
/// divides by900 and4287D6 clamps to[1,1], so that coincident ranged request
/// advances no raw word. Bouncer's velocity/BounceInit draws are independent.
/// Original execution:
/// `tools/spatial_oracle/anim_bouncer_launch.py` (`debris_loop` rows).
/// Counts on gamemd's case-exact key read, which `ObjectType::from_ini_section`
/// matches (`CCINIClass::ReadInt @ 0x005276D0` CRCs the raw key bytes): 324 of
/// the 356 stock sections that throw reach an SHP arm (of 439 authoring
/// `MaxDebris=`, 83 author 0); the 17 `[VehicleTypes]` spelling `Maxdebris=`
/// (the Rhino among them) take the constructor default 0 and throw nothing.
pub(super) fn throw_debris_for_death(
    world: &mut Simulation,
    object_type: &ObjectType,
    rules: &RuleSet,
    owner: InternedId,
    origin: glam::IVec3,
    voxel_debris: &mut Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
    explosion_effects: &mut Vec<ExplosionEffect>,
) {
    use crate::sim::voxel_anim::{DebrisTypeData, throw_death_debris};

    if object_type.max_debris <= 0 {
        return;
    }
    let debris_types: Vec<_> = object_type
        .debris_types
        .iter()
        .map(|name| {
            rules
                .voxel_anim_type_id_by_name(name)
                .map(|id| (id, rules.voxel_anim_type(id)))
        })
        .collect();
    let data = DebrisTypeData {
        max_debris: object_type.max_debris,
        min_debris: object_type.min_debris,
        debris_types: &debris_types,
        debris_maximums: &object_type.debris_maximums,
        debris_anim_count: object_type.debris_anims.len(),
        metallic_debris_count: rules.general.metallic_debris.len(),
    };
    // Native702443/70254B lifts SHP debris by20 leptons. Voxel74950C
    // applies its own10-lepton lift through the existing numeric owner.
    let anim_coord = crate::sim::anim_class::AnimWorldCoord {
        x: origin.x,
        y: origin.y,
        z: origin.z.wrapping_add(0x14),
    };
    let mut host = DeathDebrisWorldHost {
        world,
        object_type,
        rules,
        anim_coord,
        voxel_debris,
        explosion_effects,
    };
    if let Err(error) = throw_death_debris(&data, Some(owner), origin, &mut host) {
        // Out-of-domain modded velocities retain the pieces already emitted;
        // the original ordered block has no rollback of earlier constructors.
        log::debug!("death debris stopped outside verified numeric domain: {error}");
    }
}

/// Live constructor adapter for the single debris-loop port. Numeric fixtures
/// collect the same port's results; production admits each piece at its call.
struct DeathDebrisWorldHost<'w, 'r> {
    world: &'w mut Simulation,
    object_type: &'r ObjectType,
    rules: &'r RuleSet,
    anim_coord: crate::sim::anim_class::AnimWorldCoord,
    voxel_debris: &'w mut Vec<crate::sim::voxel_anim::VoxelDebrisSpawn>,
    explosion_effects: &'w mut Vec<ExplosionEffect>,
}

impl<'r> DeathDebrisWorldHost<'_, 'r> {
    fn debris_name(
        &self,
        source: crate::sim::voxel_anim::ShpDebrisSource,
        index: usize,
    ) -> Option<&'r str> {
        use crate::sim::voxel_anim::ShpDebrisSource;
        match source {
            ShpDebrisSource::TypeDebrisAnims => self.object_type.debris_anims.get(index),
            ShpDebrisSource::RulesMetallicDebris => self.rules.general.metallic_debris.get(index),
        }
        .map(String::as_str)
    }
}

impl crate::sim::voxel_anim::DeathDebrisHost for DeathDebrisWorldHost<'_, '_> {
    fn rng(&mut self) -> &mut SimRng {
        &mut self.world.scenario_rng
    }

    fn construct_anim(
        &mut self,
        source: crate::sim::voxel_anim::ShpDebrisSource,
        index: usize,
    ) -> Result<(), crate::util::native_x87::NativeX87Error> {
        let native_unique_id = crate::sim::native_identity::NativeUniqueIdCursor::assign_runtime(
            &mut self.world.native_unique_ids,
        );
        let name = self.debris_name(source, index);
        let config = name.and_then(|name| {
            self.rules
                .art()
                .anim_runtime_config(&name.to_ascii_uppercase())
        });
        let mut draws = if let Some(config) = config {
            crate::sim::anim_class::anim_constructor_draws(
                config,
                self.anim_coord,
                &mut self.world.scenario_rng,
            )?
        } else {
            crate::sim::anim_class::AnimConstructorDraws {
                native_unique_id: None,
                random_rate: None,
                bounce: None,
            }
        };
        draws.native_unique_id = Some(native_unique_id);
        if let Some(name) = name {
            let shp_name = self.world.interner.intern(name);
            let (rx, ry, sub_x, sub_y, z) = self.anim_coord.to_cell_sub_z();
            let effect = ExplosionEffect {
                shp_name,
                rx,
                ry,
                sub_x,
                sub_y,
                z,
                world_z: self.anim_coord.z,
                death: Some(destruction_effects::DeathAnimSpawn {
                    draws: Some(draws),
                    ..destruction_effects::DeathAnimSpawn::at(self.anim_coord, 0)
                }),
            };
            if world_receiver::callbacks_enabled(self.world) {
                crate::sim::world::damage_consequences::admit_explosion_effect(
                    self.world, self.rules, effect,
                );
            } else {
                // Explicit callback-disabled tests retain their packet seam.
                self.explosion_effects.push(effect);
            }
        }
        Ok(())
    }

    fn admit_voxel(&mut self, spawn: &crate::sim::voxel_anim::VoxelDebrisSpawn) {
        if world_receiver::callbacks_enabled(self.world) {
            self.world
                .admit_death_debris(std::iter::once(spawn.clone()));
        } else {
            self.voxel_debris.push(spawn.clone());
        }
    }
}

/// Select the death cues one dying object contributes.
///
/// The building tail is `BuildingClass::DestructionEffects`: when the dying
/// object is a structure whose type carries **no** `DieSound=` entries,
/// gamemd plays the global `[AudioVisual] BuildingDieSound` at the building's
/// own coordinate — `0x0044173F MOV ECX,[type+0x520]` reads the `DieSound`
/// vector's count (`TechnoTypeClass+0x510..0x528`), `0x0044174A CMP ECX,EBX ;
/// JNZ` skips the global when it is non-zero (`EBX` is zeroed at
/// `0x00441606` and stays zero), and `0x00441773 MOV ECX,[Rules+0x6E8]` +
/// `0x00441779 CALL VocClass::PlayAtCoord @ 0x00750E20` is the play. It is
/// not owner-gated and draws no RNG — there is one id, not a list.
///
/// Where the global sits relative to the per-type `VoiceDie=`/`DieSound=`
/// draws is UNCHECKED: native emits it from the building-specific destruction
/// path, not from the shared Techno death transaction. It cannot matter on
/// retail data, because the branch that reaches it requires an empty
/// `DieSound=` list and no stock building pairs `VoiceDie=` with an empty
/// `DieSound=`.
pub(super) fn append_selected_death_sounds(
    object_type: &ObjectType,
    category: EntityCategory,
    building_die_sound: Option<&str>,
    owner_is_human: bool,
    main_rng: &crate::sim::rng::MainRng,
    rx: u16,
    ry: u16,
    death_sounds: &mut Vec<(String, u16, u16)>,
) {
    let mut append_choice = |choices: &[String]| {
        if choices.is_empty() {
            return;
        }
        let index = (main_rng.next_u32() % choices.len() as u32) as usize;
        death_sounds.push((choices[index].clone(), rx, ry));
    };

    if owner_is_human {
        append_choice(&object_type.voice_die);
    }
    append_choice(&object_type.die_sounds);

    if category == EntityCategory::Structure
        && object_type.die_sounds.is_empty()
        && let Some(sound_id) = building_die_sound.filter(|id| !id.is_empty())
    {
        death_sounds.push((sound_id.to_owned(), rx, ry));
    }
}

/// Concrete-class death work that native runs only after the shared Techno
/// death-weapon transaction has returned. Keeping the plan data-only avoids
/// consuming smudge RNG (or interning the InfDeath AnimType) too early.
pub(super) enum ConcreteDeathSmudgePlan {
    Infantry(crate::sim::world::InfantryDeathPostlude),
    Building,
}
