//! `TechnoClass::ReceiveDamage`'s shared death block (`0x00701900`): the
//! death weapon, voxel debris and death sounds, the `Death_Announcement`
//! input, and the fatal lifecycle stages the world runs around them.

use super::*;
use crate::sim::world::ObjectSubstrate;

/// The `Death_Announcement` (`+0x3B8`) input for one death site.
///
/// Native non-building receivers and Squid culling629C76 call this slot;
/// BuildingClass has none. `0x004D98DD` skips `Spawned=` types. Culling
/// announces the living hull before Stun without recording a kill. The other
/// non-damage death sites that
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
    interner: &mut StringInterner,
    substrate: &ObjectSubstrate,
    resolved_terrain: &Option<ResolvedTerrainGrid>,
    id: u64,
    rules: &RuleSet,
) -> Option<ProjectileDetonation> {
    let entity = substrate.entities.get(id)?;
    let object = interner.object_type(entity.type_ref(), rules)?;
    let current_weapon =
        combat_weapon::current_weapon(entity, object, &substrate.entities, rules, interner);
    let impact =
        crate::sim::movement::ground_pose::object_get_coords(entity, resolved_terrain.as_ref());
    let (damage, warhead, weapon) =
        fire_death_weapon_payload(rules, object, current_weapon, interner)?;
    let kind = rules.weapon(interner.resolve(weapon))?;
    Some(ProjectileDetonation {
        projectile_id: id,
        source_id: id,
        target: ProjectileTarget::Entity(id),
        impact: ProjectileCoord::new(impact.x, impact.y, impact.z),
        payload: ProjectilePayload::new(
            damage,
            warhead,
            kind.projectile.as_deref().map(|id| interner.intern(id)),
            kind.bright,
        )
        .with_installed_weapon(Some(weapon)),
        reason: crate::sim::projectile::ProjectileDetonationReason::DeathWeapon,
    })
}

/// World-owned lifecycle work at the concrete receiver's native boundaries.
/// Building garrison ejection follows the shared Techno death arm; represented
/// UnInit follows the concrete death effects before the next outer receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FatalLifecycleStage {
    /// Surviving Techno ReceiveDamage postlude, before Infantry scatter and
    /// synchronous retaliation. The world owns ParticleSystem storage and the
    /// shared LogicVector, so maintenance crosses the existing inline hook.
    MaintainDamageSmoke {
        state: damage::DamageState,
    },
    /// Building442625..44264C: eject its garrison and switch off its light
    /// after Techno death effects and saved-contact callbacks, immediately
    /// before DestructionEffects442665. Cargo cleanup follows SpawnSurvivors.
    BeforeBuildingDestruction,
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
        voxel_origin: origin,
        voxel_debris,
        explosion_effects,
    };
    if let Err(error) = throw_death_debris(&data, Some(owner), &mut host) {
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
    voxel_origin: glam::IVec3,
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

impl crate::sim::voxel_anim::ShpDebrisHost for DeathDebrisWorldHost<'_, '_> {
    fn rng(&mut self) -> &mut SimRng {
        &mut self.world.scenario_rng
    }

    fn construct_anim(
        &mut self,
        source: crate::sim::voxel_anim::ShpDebrisSource,
        index: usize,
    ) -> Result<(), crate::util::native_x87::NativeX87Error> {
        let name = self.debris_name(source, index);
        self.world
            .emit_debris_anim(self.rules, name, self.anim_coord, self.explosion_effects)
    }
}

impl crate::sim::voxel_anim::VoxelDebrisHost for DeathDebrisWorldHost<'_, '_> {
    fn voxel_origin(&self) -> glam::IVec3 {
        self.voxel_origin
    }

    fn construct_voxel(
        &mut self,
        type_id: crate::rules::voxel_anim_type::VoxelAnimTypeId,
        voxel_type: &crate::rules::voxel_anim_type::VoxelAnimType,
        owner_house: Option<InternedId>,
        origin: glam::IVec3,
    ) -> Result<(), crate::util::native_x87::NativeX87Error> {
        let admit_inline = world_receiver::callbacks_enabled(self.world);
        self.world.emit_debris_voxel(
            type_id,
            voxel_type,
            owner_house,
            origin,
            admit_inline,
            self.voxel_debris,
        )
    }
}

/// Shared list selection in Techno's death block and Infantry's Eaten arm
/// (5197E5/51984D): one Main draw even for a singleton, none for an empty list.
pub(crate) fn select_death_sound(
    choices: &[String],
    main_rng: &crate::sim::rng::MainRng,
) -> Option<String> {
    if choices.is_empty() {
        None
    } else {
        Some(choices[(main_rng.next_u32() % choices.len() as u32) as usize].clone())
    }
}

/// Concrete-class death work that native runs only after the shared Techno
/// death-weapon transaction has returned. Keeping the plan data-only avoids
/// consuming smudge RNG (or interning the InfDeath AnimType) too early.
pub(super) enum ConcreteDeathSmudgePlan {
    Infantry(crate::sim::world::InfantryDeathPostlude),
    Building,
}
