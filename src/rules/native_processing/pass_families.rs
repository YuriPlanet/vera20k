//! `RulesPassProcessor` non-Techno families: Anim, Weapon, Bullet, Warhead,
//! plain (Terrain/Overlay/Smudge), Particle, ParticleSystem, VoxelAnim, Tiberium.

use super::RulesPassProcessor;
use super::{ProcessedType, RulesTypeFamily, read_ares_type_ref, read_member};
use crate::rules::error::RulesError;
use crate::rules::ini_parser::IniFile;
use crate::rules::ini_parser::is_native_none_type_name;
use crate::rules::object_image::ObjectImageKind;
use crate::rules::object_image::ObjectImageState;
use crate::rules::projectile_type::ProjectileArtState;
use crate::rules::sound_ini::SoundRegistry;
use crate::util::native_x87::NativeF64Bits;

impl RulesPassProcessor {
    pub(super) fn process_anim_family(
        &mut self,
        fixed_art: &IniFile,
        fixed_sounds: &SoundRegistry,
    ) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Animation) {
            let native_stored_id = self
                .families
                .get(&RulesTypeFamily::Animation)
                .and_then(|members| members.get(index))
                .map(|member| member.native_stored_id.clone());
            if let Some(section) = native_stored_id
                .as_deref()
                .and_then(|identity| fixed_art.section(identity))
            {
                // ReadTypeData 0x00679A5D..0x00679A82 visits the live Anim
                // registry before Techno readers can allocate later roots.
                // Anim ReadINI 0x00427D22 refuses a missing ART section via
                // AbstractType's lookup at 0x00410A7F. Record this actual visit,
                // not membership or eventual ART existence. See
                // .local/anim-type-art-read-acceptance.md.
                let theater = self.image_theater;
                let member = &mut self.family_mut(RulesTypeFamily::Animation)[index];
                member.read_object_sound(section, fixed_sounds);
                member.anim_art_read = true;
                // 0x00428344..0x004283BC: StartSound= is a ReadString128 and
                // VocClass::FindIndex (0x007514D0) that keeps the current ID
                // on an empty or unknown name. While the slot is still -1,
                // Report= is read the same way, but its miss stores -1.
                member.anim_references.start_sound = fixed_sounds
                    .read_rules_reference(
                        section,
                        "StartSound",
                        member.anim_references.start_sound.take(),
                    )
                    .or_else(|| fixed_sounds.read_rules_reference(section, "Report", None));
                // 0x004283C2..0x00428415: StopSound= the same way, no fallback.
                member.anim_references.stop_sound = fixed_sounds.read_rules_reference(
                    section,
                    "StopSound",
                    member.anim_references.stop_sound.take(),
                );
                // AnimTypeRead427D1B admits the inherited Object read, then
                //427D9D..427DDD supplies its own theater flags and image call.
                // This retains the existing fresh/unowned read projection;
                // shape-dependent flag rereads on resident owned images are
                // a separate cached-image lifecycle boundary.
                member.object_image.read_ini(
                    section,
                    fixed_art,
                    ObjectImageKind::Animation,
                    theater,
                );
                if !member.object_image.image().is_empty() {
                    member.object_image.read_anim_theater_flags(section);
                    member.object_image.load_anim_shp(
                        &member.native_stored_id,
                        theater,
                        crate::rules::object_image::AnimImagePhase::ReadIni,
                    );
                }
                let mut references = std::mem::take(&mut member.anim_references);
                for (key, family, field) in [
                    ("Next", RulesTypeFamily::Animation, &mut references.next),
                    ("Spawns", RulesTypeFamily::Animation, &mut references.spawns),
                    (
                        "TiberiumSpawnType",
                        RulesTypeFamily::Overlay,
                        &mut references.tiberium_spawn_type,
                    ),
                    (
                        "BounceAnim",
                        RulesTypeFamily::Animation,
                        &mut references.bounce_anim,
                    ),
                    (
                        "ExpireAnim",
                        RulesTypeFamily::Animation,
                        &mut references.expire_anim,
                    ),
                    (
                        "TrailerAnim",
                        RulesTypeFamily::Animation,
                        &mut references.trailer_anim,
                    ),
                    ("Warhead", RulesTypeFamily::Warhead, &mut references.warhead),
                ] {
                    if let Some(resolved) = self.resolve_scalar_from(section, key, family, 0x80) {
                        *field = resolved;
                    }
                }
                // ReadString with no default (0x00428716) skips an empty
                // read; any other name goes to ParticleType's factory
                // (0x00428730), whose answer is stored, null only for `<none>`.
                let spawns_particle = section.read_string("SpawnsParticle", "", 0x20);
                if !spawns_particle.is_empty() {
                    references.spawns_particle = self
                        .find_or_allocate_particle(&spawns_particle)
                        .map(|particle| {
                            self.families[&RulesTypeFamily::Particle][particle]
                                .native_stored_id
                                .clone()
                        });
                }
                // Ares AnimTypeClass_LoadFromINI (1000A4B0, hooks 0x4287DC /
                // 0x4287E9) runs after the original body.
                if crate::rules::ares_switch::ares_way()
                    && let Some(weapon) =
                        self.resolve_existing_type_from(section, "Weapon", RulesTypeFamily::Weapon)
                {
                    references.weapon = weapon;
                }
                self.family_mut(RulesTypeFamily::Animation)[index].anim_references = references;
            }
            index += 1;
        }
    }
}

impl RulesPassProcessor {
    pub(super) fn process_weapon_family(&mut self, pass: &IniFile) {
        let ares_way = crate::rules::ares_switch::ares_way();
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Weapon) {
            if let Some(raw) = self.begin_rules_member_read(RulesTypeFamily::Weapon, index, pass) {
                // 7722D4..772350: these scalar reads precede the later
                // constructor-capable animation and type-reference reads.
                let state = &mut self.family_mut(RulesTypeFamily::Weapon)[index].weapon;
                state.damage = raw.read_int("Damage", state.damage);
                state.speed = raw.read_speed("Speed", state.speed);
                state.range_leptons = raw.read_range("Range", state.range_leptons);
                // Ares 3.0 WeaponTypeClass_LoadFromINI_ListLength (1005C0F0,
                // hook 0x772462): ParseList, shared 0x800 buffer, Find-only.
                let anims = if ares_way {
                    self.resolve_existing_list_from(&raw, "Anim", RulesTypeFamily::Animation, 0x800)
                } else {
                    self.resolve_list_from(&raw, "Anim", RulesTypeFamily::Animation, 0x80)
                };
                if let Some(anims) = anims {
                    self.family_mut(RulesTypeFamily::Weapon)[index].weapon.anim = anims;
                }
                // WeaponTypeClass::ReadINI @ `0x00772080` reads AssaultAnim
                // (`ReadString @ 0x00528A10`, `0x0077257C`), resolves nonempty
                // names with FindOrAllocate @ `0x00428B80` (`0x00772589`), and
                // stores the pointer at +0x114 (`0x007725A6`).
                if let Some(anim) =
                    self.resolve_scalar_from(&raw, "AssaultAnim", RulesTypeFamily::Animation, 0x80)
                {
                    self.family_mut(RulesTypeFamily::Weapon)[index]
                        .weapon
                        .assault_anim = anim;
                }
                if let Some(anim) =
                    self.resolve_scalar_from(&raw, "OccupantAnim", RulesTypeFamily::Animation, 0x80)
                {
                    self.family_mut(RulesTypeFamily::Weapon)[index]
                        .weapon
                        .occupant_anim = anim;
                }
                if let Some(anim) = self.resolve_scalar_from(
                    &raw,
                    "OpenToppedAnim",
                    RulesTypeFamily::Animation,
                    0x80,
                ) {
                    self.family_mut(RulesTypeFamily::Weapon)[index]
                        .weapon
                        .open_topped_anim = anim;
                }
                let incoming = raw.read_string("AttachedParticleSystem", "", 0x14);
                if let Some(system) = self.find_or_create_particle_system(&incoming) {
                    let id = self.families[&RulesTypeFamily::ParticleSystem][system]
                        .native_stored_id
                        .clone();
                    self.family_mut(RulesTypeFamily::Weapon)[index]
                        .weapon
                        .attached_particle_system = Some(id);
                }
                let incoming_warhead = raw.read_string("Warhead", "", 0x80);
                let warhead = (!incoming_warhead.is_empty())
                    .then(|| self.find_or_allocate(RulesTypeFamily::Warhead, &incoming_warhead));
                let incoming = raw.read_string("Projectile", "", 0x80);
                let projectile = (!incoming.is_empty())
                    .then(|| self.find_or_allocate(RulesTypeFamily::Projectile, &incoming));
                let state = &mut self.family_mut(RulesTypeFamily::Weapon)[index].weapon;
                if let Some(projectile) = projectile {
                    state.projectile = projectile;
                }
                if let Some(warhead) = warhead {
                    state.warhead = warhead;
                }
                // Ares WeaponTypeExt::LoadFromINIFile 10055520 follows the
                // whole ReadINI (hooks 0x7729B0/0x7729C7/0x7729D6). Its
                // Nullable<ParticleSystemTypeClass*> read finds only a type
                // that exists by now.
                if ares_way {
                    let current = &self.families[&RulesTypeFamily::Weapon][index]
                        .weapon
                        .ares_bolt_particle_system;
                    let system = read_ares_type_ref(&raw, "Bolt.ParticleSystem", current, |name| {
                        self.find_existing(RulesTypeFamily::ParticleSystem, name)
                            .map(|system| {
                                self.families[&RulesTypeFamily::ParticleSystem][system]
                                    .native_stored_id
                                    .clone()
                            })
                    });
                    self.family_mut(RulesTypeFamily::Weapon)[index]
                        .weapon
                        .ares_bolt_particle_system = system;
                    let current = &self.families[&RulesTypeFamily::Weapon][index]
                        .weapon
                        .ares_ivan_bomb_warhead;
                    let warhead = read_ares_type_ref(&raw, "IvanBomb.Warhead", current, |name| {
                        self.find_existing(RulesTypeFamily::Warhead, name)
                            .map(|warhead| {
                                self.families[&RulesTypeFamily::Warhead][warhead]
                                    .native_stored_id
                                    .clone()
                            })
                    });
                    self.family_mut(RulesTypeFamily::Weapon)[index]
                        .weapon
                        .ares_ivan_bomb_warhead = warhead;
                    let current = Some(
                        self.families[&RulesTypeFamily::Weapon][index]
                            .weapon
                            .ares_abductor_anim
                            .clone(),
                    );
                    let anim = read_ares_type_ref(&raw, "Abductor.Anim", &current, |name| {
                        self.find_existing(RulesTypeFamily::Animation, name)
                            .map(|anim| {
                                self.families[&RulesTypeFamily::Animation][anim]
                                    .native_stored_id
                                    .clone()
                            })
                    })
                    .flatten();
                    self.family_mut(RulesTypeFamily::Weapon)[index]
                        .weapon
                        .ares_abductor_anim = anim;
                    read_member(&mut self.families, RulesTypeFamily::Weapon, index)
                        .weapon
                        .ares_cursors
                        .read_pass(&raw, &self.ares_mouse_cursors);
                }
            }
            index += 1;
        }
    }

    /// WeaponType7729F0 writes +A8 only for a nonnull projectile with ROT=0.
    /// Original chronology and arithmetic: rules_oracle/weapon_speed_order.
    pub(super) fn postprocess_weapon_speeds(&mut self) {
        for index in 0..self.family_len(RulesTypeFamily::Weapon) {
            let state = &self.families[&RulesTypeFamily::Weapon][index].weapon;
            let Some(projectile) = state.projectile else {
                continue;
            };
            let bullet = &self.families[&RulesTypeFamily::Projectile][projectile].body;
            if bullet.read_int("ROT", 0) == 0 {
                // Phobos `WeaponTypeClass_SetSpeed_ApplyGravity` 0x772A0A
                // selects the type's own `Gravity` over the current Rules one.
                #[cfg(feature = "phobos")]
                let gravity = crate::rules::projectile_type::read_phobos_gravity(bullet).map_or(
                    crate::util::native_ballistics::BallisticGravity::Rules(self.rules_gravity),
                    crate::util::native_ballistics::BallisticGravity::Value,
                );
                #[cfg(not(feature = "phobos"))]
                let gravity =
                    crate::util::native_ballistics::BallisticGravity::Rules(self.rules_gravity);
                let speed = crate::util::native_ballistics::ballistic_launch_speed(
                    state.range_leptons,
                    gravity,
                    bullet.read_bool("Floater", false),
                );
                self.family_mut(RulesTypeFamily::Weapon)[index].weapon.speed = speed;
            }
        }
    }

    pub(super) fn process_bullet_family(
        &mut self,
        pass: &IniFile,
        fixed_art: &IniFile,
        fixed_sounds: &SoundRegistry,
    ) {
        // Colors66D3A0 precedes this sweep. Own the small name list while
        // reference readers below may allocate more native type members.
        let color_names: Vec<_> = self.colors.iter().map(|(name, _)| name.clone()).collect();
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Projectile) {
            if let Some(raw) =
                self.begin_rules_member_read(RulesTypeFamily::Projectile, index, pass)
            {
                // Full native body46BEE0: base Object ART reads precede the
                // clearing Image25 read and byte-narrowed Bullet ART fields.
                // Keep type-reference allocation here in native read order.
                let member = &mut self.family_mut(RulesTypeFamily::Projectile)[index];
                member.read_object_sound(&raw, fixed_sounds);
                let mut art =
                    std::mem::replace(&mut member.projectile_art, ProjectileArtState::new());
                let mut image =
                    std::mem::replace(&mut member.object_image, ObjectImageState::new(""));
                art.read_pass(
                    &mut image,
                    self.image_theater,
                    &raw,
                    fixed_art,
                    &color_names,
                    |incoming| {
                        let animation =
                            self.find_or_allocate(RulesTypeFamily::Animation, incoming)?;
                        Some(self.resolved_type_key(RulesTypeFamily::Animation, animation))
                    },
                );
                let member = &mut self.family_mut(RulesTypeFamily::Projectile)[index];
                member.projectile_art = art;
                member.object_image = image;
                if let Some(weapon) =
                    self.resolve_scalar_from(&raw, "AirburstWeapon", RulesTypeFamily::Weapon, 0x80)
                {
                    self.family_mut(RulesTypeFamily::Projectile)[index].airburst_weapon = weapon;
                }
                if let Some(weapon) =
                    self.resolve_scalar_from(&raw, "ShrapnelWeapon", RulesTypeFamily::Weapon, 0x80)
                {
                    self.family_mut(RulesTypeFamily::Projectile)[index].shrapnel_weapon = weapon;
                }
            }
            index += 1;
        }
    }

    pub(super) fn process_warhead_family(&mut self, pass: &IniFile) {
        let ares_way = crate::rules::ares_switch::ares_way();
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Warhead) {
            if let Some(raw) = self.begin_rules_member_read(RulesTypeFamily::Warhead, index, pass) {
                let particle = self.resolve_scalar_from(
                    &raw,
                    "Particle",
                    RulesTypeFamily::ParticleSystem,
                    0x80,
                );
                if let Some(particle) = particle {
                    self.family_mut(RulesTypeFamily::Warhead)[index].warhead_particle = particle;
                }
                // Warhead ctor75CF89/75CFB3 initializes both false. The
                // original reader uses the live field as its ReadBool default.
                let current = &mut self.family_mut(RulesTypeFamily::Warhead)[index].warhead_anim;
                current.conventional = raw.read_bool("Conventional", current.conventional);
                // WarheadTypeClass::ReadINI75D576..75D58A: the current
                // +14A byte is the default, including malformed values.
                current.sparky = raw.read_bool("Sparky", current.sparky);
                // Ares 3.0 WarheadTypeClass_LoadFromINI_ListLength (1005C080,
                // hook 0x75D660) reads AnimList, DebrisMaximums (WarheadType's
                // int list) and DebrisTypes here through ParseList (shared
                // 0x800 buffer, Find-only) and skips to 0x75D75D; SkipLists
                // (1005C0E0, hook 0x75DAE6) skips the later DebrisTypes read.
                let (anim_list, ares_debris_types) = if ares_way {
                    (
                        self.resolve_existing_list_from(
                            &raw,
                            "AnimList",
                            RulesTypeFamily::Animation,
                            0x800,
                        ),
                        Some(self.resolve_existing_list_from(
                            &raw,
                            "DebrisTypes",
                            RulesTypeFamily::VoxelAnimation,
                            0x800,
                        )),
                    )
                } else {
                    (
                        self.resolve_list_from(&raw, "AnimList", RulesTypeFamily::Animation, 0x80),
                        None,
                    )
                };
                if let Some(list) = anim_list {
                    self.family_mut(RulesTypeFamily::Warhead)[index]
                        .warhead_anim
                        .anim_list = list;
                }
                let current = &mut self.family_mut(RulesTypeFamily::Warhead)[index].warhead_anim;
                current.em_effect = raw.read_bool("EMEffect", current.em_effect);
                // The stock way reads DebrisTypes after EMEffect, allocating.
                let debris_types = match ares_debris_types {
                    Some(types) => types,
                    None => self.resolve_list_from(
                        &raw,
                        "DebrisTypes",
                        RulesTypeFamily::VoxelAnimation,
                        0x80,
                    ),
                };
                if let Some(types) = debris_types {
                    self.family_mut(RulesTypeFamily::Warhead)[index]
                        .warhead_anim
                        .debris_types = types;
                }
                // Installed Ares75DDCC->10054410 skips the entire original
                // raw11 read and its+149 derivation. Its independent vector
                // is read below even when the original section was absent.
                if !ares_way {
                    let current =
                        &mut self.family_mut(RulesTypeFamily::Warhead)[index].warhead_verses;
                    *current = crate::rules::warhead_type::read_verses(
                        &raw,
                        current.map(|value| f64::from_bits(value.bits())),
                    )
                    .map(|value| NativeF64Bits::from_bits(value.to_bits()));
                }
                #[cfg(feature = "phobos")]
                {
                    // Phobos `Crit.Warhead` is read with `Read<true>`, so the
                    // warhead it names is allocated here even when no other
                    // key lists it; the type body then reads like any other.
                    let _ = self.resolve_scalar_from(
                        &raw,
                        "Crit.Warhead",
                        RulesTypeFamily::Warhead,
                        0x80,
                    );
                }
            }
            if ares_way {
                let sparkle = pass
                    .section(&self.families[&RulesTypeFamily::Warhead][index].native_stored_id)
                    .and_then(|raw| {
                        self.resolve_existing_type_from(
                            raw,
                            "EMP.Sparkles",
                            RulesTypeFamily::Animation,
                        )
                    });
                let animation = pass
                    .section(&self.families[&RulesTypeFamily::Warhead][index].native_stored_id)
                    .and_then(|raw| {
                        self.resolve_existing_type_from(
                            raw,
                            "AttachEffect.Animation",
                            RulesTypeFamily::Animation,
                        )
                    });
                // WarheadTypeExt ReadINI10051D80: Temporal.WarpAway is a
                // Nullable<Anim>; DeathWeapon.Suppress a
                // ValueableVector<TechnoTypeClass*> (Find10007EA0 per token).
                let temporal_warp_away = pass
                    .section(&self.families[&RulesTypeFamily::Warhead][index].native_stored_id)
                    .and_then(|raw| {
                        self.resolve_existing_type_from(
                            raw,
                            "Temporal.WarpAway",
                            RulesTypeFamily::Animation,
                        )
                    });
                // Valueable<AnimTypeClass*> InfDeathAnim: the same Find
                // read, where `none` stores null and an unknown name retains.
                let inf_death_anim = pass
                    .section(&self.families[&RulesTypeFamily::Warhead][index].native_stored_id)
                    .and_then(|raw| {
                        self.resolve_existing_type_from(
                            raw,
                            "InfDeathAnim",
                            RulesTypeFamily::Animation,
                        )
                    });
                let death_weapon_suppress = pass
                    .section(&self.families[&RulesTypeFamily::Warhead][index].native_stored_id)
                    .and_then(|raw| {
                        self.resolve_existing_techno_type_list_from(raw, "DeathWeapon.Suppress")
                    });
                // IonCannon.Blast/.Beam (Nullable<Anim> 1002B4D0) and
                // IonCannon.Warhead (Nullable<Warhead> 10008D00): Find reads.
                let [ion_blast, ion_beam, ion_warhead] = [
                    ("IonCannon.Blast", RulesTypeFamily::Animation),
                    ("IonCannon.Beam", RulesTypeFamily::Animation),
                    ("IonCannon.Warhead", RulesTypeFamily::Warhead),
                ]
                .map(|(key, family)| {
                    pass.section(&self.families[&RulesTypeFamily::Warhead][index].native_stored_id)
                        .and_then(|raw| self.resolve_existing_type_from(raw, key, family))
                });
                // 10051D10's first call finds NUKEBALL for a NUKE warhead;
                // PreImpactAnim is a ValueableIdx<Anim> FindIndex read.
                let nuke_ball = self
                    .find_existing(
                        RulesTypeFamily::Animation,
                        crate::rules::effect_asset_catalog::NUKE_BALL_ANIM,
                    )
                    .map(|index| self.resolved_type_key(RulesTypeFamily::Animation, index));
                let pre_impact_anim = pass
                    .section(&self.families[&RulesTypeFamily::Warhead][index].native_stored_id)
                    .and_then(|raw| {
                        self.resolve_existing_type_from(
                            raw,
                            "PreImpactAnim",
                            RulesTypeFamily::Animation,
                        )
                    });
                let member = &mut self
                    .families
                    .get_mut(&RulesTypeFamily::Warhead)
                    .expect("Warhead family exists during its reader")[index];
                if !member.ares_warhead_loaded {
                    member.ares_warhead_loaded = true;
                    if member.native_stored_id.eq_ignore_ascii_case("NUKE") {
                        member
                            .ares_warhead
                            .as_mut()
                            .expect("WH Ares extension")
                            .combat_mut()
                            .initialize_nuke(nuke_ball);
                    }
                }
                let section = pass.section(&member.native_stored_id);
                if let Some(section) = section {
                    let ares = member.ares_warhead.as_mut().expect("WH Ares extension");
                    ares.combat_mut().read_nuke_pass(section, pre_impact_anim);
                    ares.read_pass(section, sparkle, animation);
                    ares.combat_mut().read_pass(
                        section,
                        temporal_warp_away,
                        inf_death_anim,
                        death_weapon_suppress,
                    );
                    ares.combat_mut().ion_cannon_mut().read_pass(
                        section,
                        ion_blast,
                        ion_beam,
                        ion_warhead,
                    );
                }
                member
                    .ares_verses
                    .as_mut()
                    .expect("Ares Warhead constructor installs its extension")
                    .read_pass(
                        section,
                        self.ares_armors
                            .as_ref()
                            .expect("Ares reader requires its armor catalog"),
                    );
            }
            index += 1;
        }
    }

    pub(super) fn process_plain_family(
        &mut self,
        family: RulesTypeFamily,
        pass: &IniFile,
        fixed_art: &IniFile,
        fixed_sounds: &SoundRegistry,
    ) {
        let mut index = 0;
        while index < self.family_len(family) {
            if let Some(raw) = self.begin_rules_member_read(family, index, pass) {
                self.family_mut(family)[index].read_object_sound(&raw, fixed_sounds);
                let theater = self.image_theater;
                let tree_strength = self.rules_tree_strength;
                match family {
                    RulesTypeFamily::Terrain => {
                        let member = &mut self.family_mut(family)[index];
                        let state = member.terrain_type_read_state.get_or_insert_with(
                            crate::rules::terrain_object_type::TerrainTypeReadState::new,
                        );
                        state.read_pass(
                            &raw,
                            fixed_art,
                            &mut member.object_image,
                            theater,
                            tree_strength,
                        );
                        #[cfg(feature = "phobos")]
                        {
                            // `TerrainTypeExt::LoadFromINI` is hooked at
                            // 0x0071E0A6, the end of this same ReadINI body, so
                            // its `ValueableIdx<ParticleTypeClass>` resolves
                            // against the registry as it stands at this point.
                            // Move the state out to read it through the
                            // registry's immutable lookups.
                            let mut state = self.family_mut(family)[index]
                                .terrain_type_read_state
                                .take()
                                .expect("the Terrain read pass above retained its state");
                            state.read_phobos_pass(&raw, |name| {
                                self.find_existing(RulesTypeFamily::Particle, name).map(
                                    |particle| {
                                        self.families[&RulesTypeFamily::Particle][particle]
                                            .native_stored_id
                                            .clone()
                                    },
                                )
                            });
                            self.family_mut(family)[index].terrain_type_read_state = Some(state);
                        }
                    }
                    RulesTypeFamily::Overlay => {
                        // OverlayRead5FE78B first calls ObjectRead. Its fixed
                        // ART cache retains a successful pre-Arctic section;
                        // only a miss permits lookup under the mutated Image.
                        let cached_art = self.family_mut(family)[index].object_image.read_ini(
                            &raw,
                            fixed_art,
                            ObjectImageKind::Ordinary,
                            theater,
                        );
                        let member = &mut self.family_mut(family)[index];
                        member.overlay_explodes =
                            raw.read_bool("Explodes", member.overlay_explodes);
                        // OverlayTypeClass::ReadINI5FE88A/5FE897 resolves
                        // this Rules reference after ObjectRead. Empty keeps
                        // the pointer; nonempty uses the sole Anim factory.
                        let cell_anim = self.resolve_scalar_from(
                            &raw,
                            "CellAnim",
                            RulesTypeFamily::Animation,
                            0x80,
                        );
                        let member = &mut self.family_mut(family)[index];
                        if let Some(cell_anim) = cell_anim {
                            member.overlay_cell_anim = cell_anim;
                        }
                        let art = cached_art.unwrap_or_else(|| {
                            fixed_art.section_or_empty(member.object_image.image())
                        });
                        member.overlay_damage_levels =
                            art.read_int("DamageLevels", member.overlay_damage_levels);
                        //5FE8F1..5FE92D: plain Image.SHP, independently of
                        // Voxel; no Arctic prefix or generic retry here.
                        if !member.object_image.theater() {
                            member.object_image.load_plain_shp();
                        }
                    }
                    _ => {}
                }
            }
            index += 1;
        }
    }

    pub(super) fn init_overlay_theater(&mut self) {
        for member in self.family_mut(RulesTypeFamily::Overlay) {
            member.object_image.init_overlay_theater();
        }
    }

    pub(super) fn init_anim_theater(&mut self) {
        let theater = self.image_theater;
        for member in self.family_mut(RulesTypeFamily::Animation) {
            member
                .object_image
                .init_anim_theater(&member.native_stored_id, theater);
        }
    }

    pub(super) fn process_particle_family(&mut self, pass: &IniFile, fixed_sounds: &SoundRegistry) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::Particle) {
            if let Some(raw) = self.begin_rules_member_read(RulesTypeFamily::Particle, index, pass)
            {
                self.family_mut(RulesTypeFamily::Particle)[index]
                    .read_object_sound(&raw, fixed_sounds);
                if let Some(warhead) =
                    self.resolve_scalar_from(&raw, "Warhead", RulesTypeFamily::Warhead, 0x80)
                {
                    self.family_mut(RulesTypeFamily::Particle)[index].particle_warhead = warhead;
                }
                // ParticleTypeReadINI644F50,645315..64535B: empty retains;
                // otherwise FindOrAllocate645430 recognizes only <none> as
                // null. The live-count loop also visits newly allocated types.
                if let Some(name) = raw.read_name("NextParticle", 0x20) {
                    let next = self.find_or_allocate_particle(name);
                    self.family_mut(RulesTypeFamily::Particle)[index].particle_next = next;
                }
                self.family_mut(RulesTypeFamily::Particle)[index]
                    .particle_read
                    .apply_pass(&raw);
            }
            index += 1;
        }
    }

    pub(super) fn process_particle_system_family(
        &mut self,
        pass: &IniFile,
        fixed_sounds: &SoundRegistry,
    ) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::ParticleSystem) {
            if let Some(raw) =
                self.begin_rules_member_read(RulesTypeFamily::ParticleSystem, index, pass)
            {
                self.family_mut(RulesTypeFamily::ParticleSystem)[index]
                    .read_object_sound(&raw, fixed_sounds);
                // ParticleSystemType ReadINI 0x0064430F..0x00644323: ReadString
                // into 0x40 bytes over the shared empty default 0x00889F64,
                // then an unconditional ParticleType FindOrCreate (0x00645430,
                // called at 0x00644318) whose index
                // replaces +294. A reached pass without HoldsWhat therefore
                // binds the empty-named ParticleType.
                let holds_what = raw.read_string("HoldsWhat", "", 0x40);
                let particle = self.find_or_allocate_particle(&holds_what);
                let member = &mut self.family_mut(RulesTypeFamily::ParticleSystem)[index];
                member.holds_what = particle;
                member.particle_system_behavior =
                    crate::rules::particle_system_type::ParticleSystemType::read_behavior(&raw);
            }
            index += 1;
        }
    }

    pub(super) fn process_voxel_anim_family(
        &mut self,
        pass: &IniFile,
        fixed_art: &IniFile,
        fixed_sounds: &SoundRegistry,
    ) {
        let mut index = 0;
        while index < self.family_len(RulesTypeFamily::VoxelAnimation) {
            if let Some(raw) =
                self.begin_rules_member_read(RulesTypeFamily::VoxelAnimation, index, pass)
            {
                self.family_mut(RulesTypeFamily::VoxelAnimation)[index]
                    .read_object_sound(&raw, fixed_sounds);
                // ObjectRead5F92D0 runs before the subtype's model selection.
                read_member(&mut self.families, RulesTypeFamily::VoxelAnimation, index)
                    .object_image
                    .read_ini(
                        &raw,
                        fixed_art,
                        ObjectImageKind::Ordinary,
                        self.image_theater,
                    );
                let member = &mut self.family_mut(RulesTypeFamily::VoxelAnimation)[index];
                member.voxel_anim_angular_velocities =
                    crate::rules::voxel_anim_type::read_angular_velocities(
                        &raw,
                        member.voxel_anim_angular_velocities,
                    );
                let mut references = std::mem::take(&mut member.voxel_anim_references);
                for (key, family, field) in [
                    (
                        "Spawns",
                        RulesTypeFamily::VoxelAnimation,
                        &mut references.spawns,
                    ),
                    (
                        "BounceAnim",
                        RulesTypeFamily::Animation,
                        &mut references.bounce_anim,
                    ),
                    (
                        "ExpireAnim",
                        RulesTypeFamily::Animation,
                        &mut references.expire_anim,
                    ),
                    (
                        "TrailerAnim",
                        RulesTypeFamily::Animation,
                        &mut references.trailer_anim,
                    ),
                    ("Warhead", RulesTypeFamily::Warhead, &mut references.warhead),
                    (
                        "AttachedSystem",
                        RulesTypeFamily::ParticleSystem,
                        &mut references.attached_system,
                    ),
                ] {
                    if key == "Warhead" {
                        //74B3D8..74B3F2 reads Damage after TrailerAnim and
                        //before DamageRadius/Warhead, with its current value.
                        let member = &mut self.family_mut(RulesTypeFamily::VoxelAnimation)[index];
                        member.voxel_anim_damage = crate::rules::voxel_anim_type::read_damage(
                            &raw,
                            member.voxel_anim_damage,
                        );
                    }
                    if let Some(resolved) = self.resolve_scalar_from(&raw, key, family, 0x80) {
                        *field = resolved;
                    }
                }
                self.family_mut(RulesTypeFamily::VoxelAnimation)[index].voxel_anim_references =
                    references;
                //74B23E..274: these are fresh false-default locals on each reached body.
                let flags = [
                    raw.read_bool("ShareBodyData", false),
                    raw.read_bool("ShareTurretData", false),
                    raw.read_bool("ShareBarrelData", false),
                ];
                let source = flags
                    .iter()
                    .any(|flag| *flag)
                    .then(|| raw.read_string("ShareSource", "<none>", 128));
                let source = source.as_deref().and_then(|name| {
                    [
                        RulesTypeFamily::Infantry,
                        RulesTypeFamily::Vehicle,
                        RulesTypeFamily::Aircraft,
                        RulesTypeFamily::Building,
                    ]
                    .into_iter()
                    .find_map(|family| {
                        // ReadTechno476EB0: only InfantryFind523C90 rejects these sentinels.
                        if family == RulesTypeFamily::Infantry && is_native_none_type_name(name) {
                            return None;
                        }
                        self.families
                            .get(&family)?
                            .iter()
                            .find(|member| member.native_stored_id.eq_ignore_ascii_case(name))
                            .map(|member| {
                                let slots = &member.object_image.model_slots;
                                if flags[0] {
                                    slots.main
                                } else if flags[1] {
                                    slots.auxiliary
                                } else {
                                    slots.barrel
                                }
                            })
                    })
                });
                let member =
                    read_member(&mut self.families, RulesTypeFamily::VoxelAnimation, index);
                member.voxel_anim_read = true;
                member.voxel_anim_share_flags = flags;
                if flags.iter().any(|flag| *flag) {
                    self.model_plan
                        .copy_debris(&mut member.object_image.model_slots, source);
                } else {
                    let image = member.object_image.image().to_owned();
                    self.model_plan
                        .load_debris(&mut member.object_image.model_slots, &image);
                }
            }
            index += 1;
        }
    }
}

impl RulesPassProcessor {
    pub(super) fn process_tiberiums(&mut self, pass: &IniFile) -> Result<(), RulesError> {
        let Some(registry) = pass.section("Tiberiums") else {
            return Ok(());
        };

        for key in registry.keys() {
            let slot = crate::rules::ini_value::crt_atoi(key);
            let identity = registry.read_string(key, "", 0x18);
            if identity.is_empty() {
                continue;
            }

            if slot < 0 {
                return Err(RulesError::InvalidValue {
                    section: "Tiberiums".to_string(),
                    key: key.to_string(),
                    expected: "a nonnegative native Tiberium slot".to_string(),
                    value: key.to_string(),
                });
            }

            let index = if slot < self.tiberiums.len() as i32 {
                slot as usize
            } else {
                self.tiberiums.push(ProcessedType::new(identity));
                self.tiberiums.len() - 1
            };
            let native_stored_id = self.tiberiums[index].native_stored_id.clone();
            if let Some(section) = pass.section(&native_stored_id).cloned() {
                self.tiberiums[index].body.overlay_rules_pass(&section);
                // TiberiumClass::ReadINI @ 0x00721A50, original instructions
                //721B20..721B3A, uses the current Color ordinal
                // and the catalog already present in this pass. ReadColor474A90
                // can normalize ctor0 to shade53 index1 even for an absent key.
                self.tiberiums[index].tiberium_color_scheme = section.read_color_scheme(
                    "Color",
                    self.tiberiums[index].tiberium_color_scheme,
                    self.colors.iter().map(|(name, _)| name.as_str()),
                );
                //721B40..721C1E: nonempty ReadString128 replaces the list
                // with ordered nonnull AnimType428B80 answers. Keep this late
                // factory site after this pass's Anim ART sweep668EF0.
                if let Some(debris) =
                    self.resolve_list_from(&section, "Debris", RulesTypeFamily::Animation, 0x80)
                {
                    self.tiberiums[index].tiberium_debris = debris;
                }
                // Image is the last read (721C3F..721CD6), after the Debris
                // factory calls. Capture this physical pass's Overlay slot;
                // projecting the merged body must not bind it a second time.
                let overlay_count = self
                    .families
                    .get(&RulesTypeFamily::Overlay)
                    .map_or(0, Vec::len);
                self.tiberiums[index]
                    .tiberium_image
                    .read_ini(&section, overlay_count)?;
                // Ares 3.0 100508D0 (hooks 0x721C7B/0x721CDC): the TiberiumExt
                // keys after Image; its two warheads are Find-only reads.
                if crate::rules::ares_switch::ares_way() {
                    let mut ext = std::mem::take(&mut self.tiberiums[index].tiberium_ext);
                    ext.read_pass(&section, |key| {
                        self.resolve_existing_type_from(&section, key, RulesTypeFamily::Warhead)
                    });
                    self.tiberiums[index].tiberium_ext = ext;
                }
            }
        }
        Ok(())
    }
}
