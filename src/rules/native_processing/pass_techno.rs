//! `RulesPassProcessor` TechnoType family reads: Ares techno extensions, prism
//! forwarding, factory owners and the TechnoType base body.

use super::RulesPassProcessor;
use super::{RulesTypeFamily, ares_country_index_of_name, read_member, read_nullable_type_vector};
use crate::rules::ini_parser::IniFile;
use crate::rules::ini_parser::IniSection;
use crate::rules::ini_parser::is_native_none_type_name;
use crate::rules::object_image::ObjectImageKind;
use crate::rules::object_type::WEAPON_SLOT_COUNT;
use crate::rules::prerequisite::parse_prerequisites;
use crate::rules::sound_ini::SoundRegistry;

impl RulesPassProcessor {
    pub(super) fn process_techno_family(
        &mut self,
        family: RulesTypeFamily,
        pass: &IniFile,
        fixed_art: &IniFile,
        fixed_sounds: &SoundRegistry,
    ) {
        let ares_way = crate::rules::ares_switch::ares_way();
        let mut index = 0;
        while index < self.family_len(family) {
            // `0x005240BD` clears +C8F on every pass, before the section
            // check. Ares hooks that instruction and jumps past it
            // (`InfantryTypeClass_LoadFromINI_DamageSparks`, Ares.dll
            // `0x100499E0`), so under Ares a set flag stays set.
            if family == RulesTypeFamily::Infantry && !ares_way {
                self.family_mut(family)[index].infantry_damage_sparks = false;
            }
            if let Some(raw) = self.begin_rules_member_read(family, index, pass) {
                // Owned copies of the member's id and body as this pass left
                // them: the reads below take the processor mutably, and a
                // Building's InvisibleInGame derivation writes the body before
                // its later reads, which keep the body without it.
                let member = &self.families[&family][index];
                let native_stored_id = member.native_stored_id.clone();
                let effective = member.body.clone();
                self.family_mut(family)[index].read_object_sound(&raw, fixed_sounds);
                self.process_techno_base(family, index, &raw);
                // Ares TechnoTypeClass_LoadFromINI_EarlyReader10041DC0
                // replaces lea edi,[ebp+CB8] at715320, then resumes715326.
                // Reading its isolated pointer in either mode also lets a
                // key-only game select the runtime Ares switch.
                if let Some(image) =
                    self.resolve_existing_type_from(&raw, "WaterImage", RulesTypeFamily::Vehicle)
                {
                    self.family_mut(family)[index].techno_references.water_image = image;
                }
                // TechnoTypeClass::ReadINI parses `Prerequisite=` (`0x007141C5`)
                // and `PrerequisiteOverride=` (`0x0071424B`) after `Dock=` and
                // `DeploysInto=` (`0x00713180`, `0x00713279`), the reads above
                // that can allocate a BuildingType, each with its current list
                // as the default.
                let member = &self.families[&family][index];
                let prerequisite_capacity = if ares_way { 0x800 } else { 0x80 };
                let prerequisite = parse_prerequisites(
                    raw.read_list("Prerequisite", prerequisite_capacity),
                    &member.prerequisite,
                    |name| self.building_index(name),
                    self.ares_prerequisites.as_ref(),
                );
                let prerequisite_override = parse_prerequisites(
                    raw.read_list("PrerequisiteOverride", prerequisite_capacity),
                    &member.prerequisite_override,
                    |name| self.building_index(name),
                    self.ares_prerequisites.as_ref(),
                );
                let member = read_member(&mut self.families, family, index);
                member.prerequisite = prerequisite;
                member.prerequisite_override = prerequisite_override;
                read_member(&mut self.families, family, index)
                    .recoil
                    .apply_pass(&raw);
                let weapon_count = self.families[&family][index].weapon_slots.weapon_count;
                read_member(&mut self.families, family, index)
                    .gunner_turrets
                    .apply_pass(
                        &native_stored_id,
                        &raw,
                        family.object_category().expect("TechnoType family"),
                        weapon_count,
                    );
                // Phobos `TechnoTypeExt::LoadFromINIFile`'s insignia block
                // runs later in the same type read: `InsigniaType=` resolves
                // against the `[InsigniaTypes]` state this pass allocated, and
                // its weapon loop uses this pass's native `WeaponCount` and
                // `Gunner` values.
                #[cfg(feature = "phobos")]
                if self.phobos_insignia_types.is_some() {
                    let catalog = self.phobos_insignia_types.take();
                    let member = &self.families[&family][index];
                    let weapon_count = member.weapon_slots.weapon_count;
                    let gunner = member.gunner_turrets.is_gunner();
                    read_member(&mut self.families, family, index)
                        .phobos_insignia
                        .apply_pass(&raw, catalog.as_ref(), weapon_count, gunner);
                    self.phobos_insignia_types = catalog;
                }
                // Ares TechnoTypeExt reads its insignia keys on the same pass
                // (Ares.dll 0x1003E355..0x1003E380, 0x1003E6D5).
                if ares_way {
                    let _ares = crate::rules::ares_switch::ares_reads();
                    read_member(&mut self.families, family, index)
                        .ares_insignia
                        .apply_pass(&raw);
                }
                if family == RulesTypeFamily::Infantry {
                    let member = read_member(&mut self.families, family, index);
                    member.infantry_pips.apply_pass(&raw);
                    member.infantry_civilian = raw.read_bool("Civilian", member.infantry_civilian);
                    // `effective` holds every pass so far, so its `Cyborg` is
                    // the retained +EAC; `0x005243E5..0x005243E7` only sets.
                    member.infantry_damage_sparks |= effective.read_bool("Cyborg", false);
                    member.infantry_infiltration.apply_pass(&raw);
                }
                // ObjectType ReadINI (`0x005F92F8..0x005F933B`) reads Image
                // into its 25-byte +1F8 over a copy of the retained text, so a
                // pass without the key keeps it. It names each fixed ART
                // section below; the later ART Image redirect is local to
                // visual asset loading.
                let theater = self.image_theater;
                let art_image_swap = self.rules_art_image_swap;
                let member = &mut self.family_mut(family)[index];
                if ares_way {
                    member.object_image.read_ares_alternate_theater_art(&raw);
                }
                member
                    .object_image
                    .read_ini(&raw, fixed_art, ObjectImageKind::Ordinary, theater);
                // Phobos ArtImageSwap names a TechnoType's visuals from its
                // own ART `Image=` while its flags stay on that section.
                if art_image_swap && family != RulesTypeFamily::Building {
                    member.object_image.apply_phobos_art_image_swap(fixed_art);
                }
                // TechnoRead7157B5..715825 loads current Image.SHP once
                // for every RTTI except Building7, independently of Voxel.
                // Its unconditional store clears a prior parent shape even
                // on a miss, before Palette71584B and LoadVoxel71609C.
                if family != RulesTypeFamily::Building {
                    member.object_image.load_plain_shp();
                }
                let image = member.object_image.image().to_string();
                let references = &mut member.techno_references;
                // ReadINI712170 reaches715852 on every admitted Rules layer,
                // after ObjectType's Image read. Empty/missing Palette keeps
                // the previous binding even when this pass changes Image.
                // A missing Rules [Type] never reaches this block (712194).
                if let Some(name) = fixed_art
                    .section_or_empty(&image)
                    .read_name("Palette", 0x20)
                {
                    references.palette_binding = Some(name.to_ascii_lowercase());
                }
                let member = read_member(&mut self.families, family, index);
                member.voxel_shadow_index = fixed_art
                    .section_or_empty(&image)
                    .read_int("ShadowIndex", member.voxel_shadow_index);
                // TechnoRead715B10..716005 reads the WeaponStruct geometry
                // after the retained Image selects this pass's fixed ART.
                let slots = &mut member.weapon_slots;
                slots.offsets.read_pass(
                    &fixed_art.section_or_empty(&image),
                    slots.turret_count,
                    slots.weapon_count,
                );
                // TechnoRead716090..7160A4 reaches LoadVoxel5F8110 only
                // with the retained Voxel byte. Later Image changes cannot
                // rename a resource whose load was skipped.
                if member.object_image.voxel() {
                    member.object_image.load_voxel();
                    // TechnoType71609C: capture this read's fields, not final render metadata.
                    let input = super::voxel_model_plan::GenericLoadInput {
                        image: member.object_image.visual_image().to_owned(),
                        unit: family == RulesTypeFamily::Vehicle,
                        apc: native_stored_id == "APC",
                        turret: effective.read_bool("Turret", false),
                        turret_count: member.weapon_slots.turret_count,
                        gattling: member.is_gattling,
                        no_spawn_alt: effective.read_bool("NoSpawnAlt", false),
                    };
                    self.model_plan
                        .load_generic(&mut member.object_image.model_slots, &input);
                }
                // Installed suffix hooks716123/716132 ->10040DF0 ->1003DEC0
                // reach the common TechnoType extension before subclass reads.
                if ares_way {
                    let _ares = crate::rules::ares_switch::ares_reads();
                    //1003E01B sizes to the live Side array;1003E040..1003E070
                    // reads each Survivor.Side%u through Infantry Find only.
                    let side_count = self
                        .families
                        .get(&RulesTypeFamily::Side)
                        .map_or(0, Vec::len);
                    let pilots = (0..side_count)
                        .map(|side| {
                            self.resolve_existing_type_from(
                                &raw,
                                &format!("Survivor.Side{side}"),
                                RulesTypeFamily::Infantry,
                            )
                        })
                        .collect::<Vec<_>>();
                    self.family_mut(family)[index].crew.read_pass(&raw, &pilots);
                    // 1003E093..1003E0F1 and on: the prerequisite lists after
                    // the Survivor reads. List 0 is the type's own `Prerequisite`.
                    let member = &mut self.family_mut(family)[index];
                    let mut list0 = std::mem::take(&mut member.prerequisite);
                    let mut prerequisites =
                        std::mem::take(member.techno_combat.prerequisites_mut());
                    prerequisites.read_pass(&raw, &mut list0, |key, prior| {
                        parse_prerequisites(
                            raw.read_list(key, 0x800),
                            prior,
                            |name| self.building_index(name),
                            self.ares_prerequisites.as_ref(),
                        )
                    });
                    let member = &mut self.family_mut(family)[index];
                    member.prerequisite = list0;
                    *member.techno_combat.prerequisites_mut() = prerequisites;
                    // TypeExt ReadINI1003EADC..1003EAED, NullableBool1000E9B0.
                    let member = &mut self.family_mut(family)[index];
                    // Ares1003EC3C..1003EC7C reads 2048 then strncpy_s with
                    // capacity32/_TRUNCATE. Missing/empty retains; either
                    // native none spelling clears the override.
                    member.selection_group = raw.read_string_with(
                        "GroupAs",
                        2048,
                        std::mem::take(&mut member.selection_group),
                        |_, value| {
                            if is_native_none_type_name(value) {
                                String::new()
                            } else {
                                crate::rules::ini_value::truncate_native_bytes(value, 31).to_owned()
                            }
                        },
                    );
                    member.aircraft_crashable =
                        raw.read_ares_nullable_bool("Crashable", member.aircraft_crashable);
                    // TechnoTypeExt Initialize (1003DEC0) and the chain's
                    // LoadFromINIFile reads (1003DFB0): Find-only pointers.
                    let mut techno_combat = std::mem::take(&mut member.techno_combat);
                    techno_combat.read_pass(
                        &raw,
                        &native_stored_id,
                        family == RulesTypeFamily::Aircraft,
                        |key, kind| {
                            use crate::rules::ares_techno_type::TypeRef;
                            let family = match kind {
                                TypeRef::Animation => RulesTypeFamily::Animation,
                                TypeRef::Warhead => RulesTypeFamily::Warhead,
                                TypeRef::Weapon => RulesTypeFamily::Weapon,
                            };
                            self.resolve_existing_type_from(&raw, key, family)
                        },
                        |key| self.resolve_existing_techno_type_from(&raw, key),
                        |key| self.resolve_existing_techno_type_list_from(&raw, key),
                        |name| {
                            self.find_existing(RulesTypeFamily::Animation, name)
                                .map(|index| {
                                    self.resolved_type_key(RulesTypeFamily::Animation, index)
                                })
                        },
                    );
                    self.family_mut(family)[index].techno_combat = techno_combat;
                    // 1003DFB0 names [MouseCursors] entries this pass's
                    // 1002AEF0 already allocated.
                    read_member(&mut self.families, family, index)
                        .ares_cursors
                        .read_techno_pass(&raw, &self.ares_mouse_cursors);
                } else {
                    // The isolated Find-only canopy read also admits a game
                    // whose rules/map names this Ares key without ares.mix.
                    let mut retained =
                        std::mem::take(&mut self.family_mut(family)[index].techno_combat);
                    retained.read_parachute_anim(|key| {
                        self.resolve_existing_type_from(&raw, key, RulesTypeFamily::Animation)
                    });
                    self.family_mut(family)[index].techno_combat = retained;
                }
                if ares_way {
                    self.process_factory_owners(family, index, &raw);
                }
                if ares_way {
                    use crate::rules::ares_driver::OperatorRequirement;
                    let _ares = crate::rules::ares_switch::ares_reads();
                    let operator = raw.read_name("Operator", 2048).map(|name| {
                        if name.eq_ignore_ascii_case("_ANY_") {
                            OperatorRequirement::Any
                        } else {
                            let types = self
                                .resolve_existing_techno_type_list_from(&raw, "Operator")
                                .expect("same nonempty Operator read");
                            if types.is_empty() {
                                OperatorRequirement::None
                            } else {
                                OperatorRequirement::Types(types)
                            }
                        }
                    });
                    self.family_mut(family)[index]
                        .driver
                        .read_pass(&raw, operator);
                    self.family_mut(family)[index].bounty.read_pass(&raw);
                }
                if ares_way {
                    // Find only: this nullable Anim reader never allocates.
                    self.family_mut(family)[index]
                        .promotion_abilities
                        .as_mut()
                        .expect("TechnoType promotion abilities")
                        .read_pass(&raw);
                    let sparkle = self.resolve_existing_type_from(
                        &raw,
                        "EMP.Sparkles",
                        RulesTypeFamily::Animation,
                    );
                    self.family_mut(family)[index]
                        .emp_type
                        .as_mut()
                        .expect("TechnoType EMP extension")
                        .read_pass(&raw, sparkle);
                    // TechnoTypeExt::LoadFromINI1003DEC0 calls the shared
                    // descriptor reader10058A50 at1003E8D4, before subclasses.
                    let animation = self.resolve_existing_type_from(
                        &raw,
                        "AttachEffect.Animation",
                        RulesTypeFamily::Animation,
                    );
                    let member = &mut self.family_mut(family)[index];
                    member
                        .self_attach_effect
                        .as_mut()
                        .expect("TechnoType attachment descriptor")
                        .read_pass(&raw, animation);
                    member.cloak_allowed =
                        raw.read_ares_bool("Cloakable.Allowed", member.cloak_allowed);
                    // TypeExt LoadFromINI1003DFB0 reads RadarJamRadius and the
                    // Find-only PoweredBy vector10010850 before the cloak keys.
                    let powered_by = self.resolve_existing_list_from(
                        &raw,
                        "PoweredBy",
                        RulesTypeFamily::Building,
                        2048,
                    );
                    self.family_mut(family)[index]
                        .cloak_power
                        .read_pass(&raw, powered_by);
                    let member = &mut self.family_mut(family)[index];
                    member.turret_rot_override =
                        raw.read_ares_nullable_int("TurretROT", member.turret_rot_override);
                    // TypeExt LoadFromINI1003DFB0: two independent
                    // Valueable<double> reads (binary64, percent scales).
                    let [iron_curtain, force_shield] = member.curtain_modifiers;
                    member.curtain_modifiers = [
                        raw.read_ares_double_bits("IronCurtain.Modifier", iron_curtain),
                        raw.read_ares_double_bits("ForceShield.Modifier", force_shield),
                    ];
                    //1003E232: the common extension applies to all Technos.
                    member.building_has_spotlight =
                        crate::rules::ares_switch::not_ares_read(|| {
                            raw.read_ares_bool("HasSpotlight", member.building_has_spotlight)
                        });
                    member.spotlight.read_pass(&raw);
                    member.super_weapon_techno.read_pass(&raw);
                    // TypeExt LoadFromINI1003DFB0: the Experience.* block,
                    // then the Promote.* block whose two type pointers use
                    // the Find-only reader1003FA40 (as Convert.* below).
                    let promote_types = ["Promote.VeteranType", "Promote.EliteType"]
                        .map(|key| self.resolve_existing_techno_type_from(&raw, key));
                    self.family_mut(family)[index]
                        .veterancy
                        .read_pass(&raw, promote_types);
                    // 1003DFB0: Cloneable, the Find-only BuildingType vector
                    // ClonedAt10010850 and pointer ClonedAs1003FA40, later
                    // FactoryPlant.Multiplier and the BuildTime.* block.
                    let cloned_at = self.resolve_existing_list_from(
                        &raw,
                        "ClonedAt",
                        RulesTypeFamily::Building,
                        2048,
                    );
                    let cloned_as = self.resolve_existing_techno_type_from(&raw, "ClonedAs");
                    self.family_mut(family)[index]
                        .production
                        .read_pass(&raw, cloned_at, cloned_as);
                }
                if ares_way
                    && let Some(target) =
                        self.resolve_existing_techno_type_from(&raw, "Convert.Water")
                {
                    self.family_mut(family)[index]
                        .techno_references
                        .convert_water = target;
                }
                if ares_way
                    && let Some(target) =
                        self.resolve_existing_techno_type_from(&raw, "Convert.Land")
                {
                    self.family_mut(family)[index]
                        .techno_references
                        .convert_land = target;
                }
                if ares_way
                    && let Some(target) =
                        self.resolve_existing_techno_type_from(&raw, "Convert.Script")
                {
                    self.family_mut(family)[index]
                        .techno_references
                        .convert_script = target;
                }
                match family {
                    RulesTypeFamily::Building => {
                        if ares_way
                            && let Some(supers) = self.resolve_existing_list_from(
                                &raw,
                                "SuperWeapons",
                                RulesTypeFamily::SuperWeapon,
                                2048,
                            )
                        {
                            // BuildingTypeExt1000E71B ->1000EC70: ordered
                            // lookup-only ID vector, missing/empty retains,
                            // unknown tokens are skipped, registered sentinel IDs
                            // still match before none rejection, no allocation.
                            self.family_mut(family)[index].additional_super_weapons = supers;
                        }
                        // BuildingTypeExt 1000DD40+0x201..0x271: the
                        // SpyEffect.SuperWeapon pointer is Find-only
                        // (1000CA80); blank clears, unknown retains.
                        if ares_way {
                            let super_weapon = self.resolve_existing_type_from(
                                &raw,
                                "SpyEffect.SuperWeapon",
                                RulesTypeFamily::SuperWeapon,
                            );
                            self.family_mut(family)[index]
                                .infiltration
                                .read_pass(&raw, super_weapon);
                            // 1000DD40: Rubble.Destroyed/Intact through the
                            // Find-only BuildingType reader 1000CA20 and their
                            // anims through the AnimType reader 1000C9C0.
                            let rubble_types = ["Rubble.Destroyed", "Rubble.Intact"].map(|key| {
                                self.resolve_existing_type_from(
                                    &raw,
                                    key,
                                    RulesTypeFamily::Building,
                                )
                            });
                            let rubble_anims =
                                ["Rubble.Destroyed.Anim", "Rubble.Intact.Anim"].map(|key| {
                                    self.resolve_existing_type_from(
                                        &raw,
                                        key,
                                        RulesTypeFamily::Animation,
                                    )
                                });
                            // 1000DD40 CanBeOccupiedBy: the Find-only
                            // InfantryType vector 1000ED90.
                            let allowed_occupiers = self.resolve_existing_list_from(
                                &raw,
                                "CanBeOccupiedBy",
                                RulesTypeFamily::Infantry,
                                2048,
                            );
                            // 1000DD40 Tunnel (INI_EX::ReadString, 2048):
                            // its index in this pass's [TunnelTypes] list.
                            let tunnel = raw
                                .read_name("Tunnel", 0x800)
                                .map(|name| self.ares_global_controls.building.tunnel_index(name));
                            // 1000DD40 SecretLab.PossibleBoons: the
                            // NullableVector<TechnoTypeClass*> reader 1000EA30
                            // over Find 10007EA0, unknown names skipped.
                            let possible_boons =
                                raw.read_name("SecretLab.PossibleBoons", 0x800).map(|_| {
                                    read_nullable_type_vector(
                                        &raw,
                                        "SecretLab.PossibleBoons",
                                        None,
                                        |name| self.find_existing_techno_type(name),
                                    )
                                });
                            // DockUnloadCell/DockUnloadFacing come from the
                            // type's Image= art section.
                            self.family_mut(family)[index].building_type_ext.read_pass(
                                &raw,
                                fixed_art.section_or_empty(&image),
                                crate::rules::ares_building_type::BuildingTypeExtReferences {
                                    rubble_types,
                                    rubble_anims,
                                    allowed_occupiers,
                                    tunnel,
                                    possible_boons,
                                },
                            );
                            // 1000DD40: Academy.Types/Academy.Ignore through
                            // the Find-only TechnoType vector1000EDD0, then
                            // the four bonuses and the computed +138 flag.
                            let types =
                                self.resolve_existing_techno_type_list_from(&raw, "Academy.Types");
                            let ignore =
                                self.resolve_existing_techno_type_list_from(&raw, "Academy.Ignore");
                            self.family_mut(family)[index]
                                .academy
                                .read_pass(&raw, types, ignore);
                            // 1000DD40+0x125, before the academy block.
                            self.family_mut(family)[index]
                                .production
                                .read_building_pass(&raw);
                            // 1000DD40 Cursor.Spy, after SlamSound.
                            read_member(&mut self.families, family, index)
                                .ares_cursors
                                .read_building_pass(&raw, &self.ares_mouse_cursors);
                        }
                        let member = read_member(&mut self.families, family, index);
                        // ReadINI460F50..460F9C: exact Rules key, ReadCoord529CA0,
                        // current value as default, no clamp or conversion. Read
                        // the Ares way, the common TechnoType extension read
                        // HasSpotlight above instead.
                        if !ares_way {
                            member.building_has_spotlight =
                                raw.read_bool("HasSpotlight", member.building_has_spotlight);
                        }
                        member.building_light.read_pass(&raw);
                        member.building_extra_power.read_pass(&raw);
                        member
                            .building_upgrade
                            .read_pass(&raw, fixed_art.section_or_empty(&image));
                        member.building_target_coord_offset = raw
                            .read_coord3("TargetCoordOffset", member.building_target_coord_offset);
                        // BuildingTypeClass::ReadINI @0x0045FE50 reads these in
                        // order with the current triples as Read3Int defaults
                        // (0x00529CA0 at0x0045FF13/0x0045FF3C).
                        member.building_half_damage_smoke_location1 = raw.read_coord3(
                            "HalfDamageSmokeLocation1",
                            member.building_half_damage_smoke_location1,
                        );
                        member.building_half_damage_smoke_location2 = raw.read_coord3(
                            "HalfDamageSmokeLocation2",
                            member.building_half_damage_smoke_location2,
                        );
                        member.building_foundation.read_stock_pass(
                            &image,
                            &native_stored_id,
                            |section, current| {
                                crate::rules::foundation::read_foundation(
                                    fixed_art.section_or_empty(section),
                                    current,
                                )
                            },
                        );
                        if ares_way {
                            member
                                .building_foundation
                                .read_custom_pass(fixed_art.section_or_empty(&image));
                        }
                        if let Some(unit) = self.resolve_scalar_from(
                            &raw,
                            "FreeUnit",
                            RulesTypeFamily::Vehicle,
                            0x80,
                        ) {
                            self.family_mut(family)[index].techno_references.free_unit = unit;
                        }
                        // Building4605BB..460683: three ReadString128 and
                        // FindOrAllocate calls retain empty reads and clear
                        // authored none. Their families and order are native.
                        for (slot, key, target_family) in [
                            (0, "SecretInfantry", RulesTypeFamily::Infantry),
                            (1, "SecretUnit", RulesTypeFamily::Vehicle),
                            (2, "SecretBuilding", RulesTypeFamily::Building),
                        ] {
                            if let Some(value) =
                                self.resolve_scalar_from(&raw, key, target_family, 0x80)
                            {
                                read_member(&mut self.families, family, index)
                                    .secret_lab
                                    .set_override(slot, value);
                            }
                        }
                        read_member(&mut self.families, family, index)
                            .secret_lab
                            .read_flag(&raw);
                        // BuildingTypeClass::ReadINI @0x00460DEB..0x00460E10:
                        // current-default InvisibleInGame forces Invisible+0xC9A
                        // and clears RadarVisible+0xC9B after the Techno reads.
                        // Keep these writes in the retained body so absent or
                        // invalid keys in later passes use the derived defaults.
                        // Source: tools.native_inspect disasm 0x460DEB --bytes 0x2C.
                        let body = &mut self.family_mut(family)[index].body;
                        if body.read_bool("InvisibleInGame", false) {
                            let mut derived = IniSection::new(native_stored_id.clone());
                            derived.set("Invisible", "yes");
                            derived.set("RadarVisible", "no");
                            body.overlay_rules_pass(&derived);
                        }
                        // ToOverlay's ART section is +1F8 (0x004610DE).
                        if let Some(overlay) = self.resolve_fixed_art_reference(
                            fixed_art,
                            &image,
                            "ToOverlay",
                            RulesTypeFamily::Overlay,
                        ) {
                            self.family_mut(family)[index].techno_references.to_overlay = overlay;
                        }
                    }
                    RulesTypeFamily::Aircraft => {
                        // AircraftTypeRead41CCEF..41CD4E uses current Rules
                        // Image+1F8, not ART Image's visual redirect. The Anim
                        // sweep already ran; allocation here must not read ART.
                        let animation = self.resolve_fixed_art_reference(
                            fixed_art,
                            &image,
                            "Trailer",
                            RulesTypeFamily::Animation,
                        );
                        let trailer = &mut self.family_mut(family)[index].aircraft_trailer;
                        let spawn_delay = fixed_art
                            .section_or_empty(&image)
                            .read_int("SpawnDelay", trailer.spawn_delay());
                        trailer.apply_read(animation, spawn_delay);
                    }
                    RulesTypeFamily::Vehicle => {
                        let member = read_member(&mut self.families, family, index);
                        member.unit_speed_type = member
                            .unit_speed_type
                            .read_vehicle_pass(&raw, effective.read_bool("Crusher", false));
                        // Unit7477C1 uses the +1F8 section without a type-name
                        // fallback. The existing reached-body projection
                        // retains the Turret default across only the admitted
                        // Rules passes.
                        let has_turret = effective.read_bool("Turret", false);
                        read_member(&mut self.families, family, index)
                            .unit_shp
                            .read_pass(fixed_art.section_or_empty(&image), has_turret);
                        #[cfg(feature = "phobos")]
                        {
                            let _phobos = crate::rules::ares_switch::not_ares_reads();
                            // `UnitTypeExt::LoadFromINI` reads the three
                            // `OreGathering.*` vectors in this order at the end
                            // of the UnitType body. An absent key retains the
                            // whole list; a present one replaces it, dropping
                            // any `Anims=` token the AnimType array lacks.
                            let mut gathering =
                                self.family_mut(family)[index].ore_gathering.clone();
                            if let Some(anims) = self.resolve_existing_list_from(
                                &raw,
                                "OreGathering.Anims",
                                RulesTypeFamily::Animation,
                                0x800,
                            ) {
                                gathering.anims = anims;
                            }
                            if let Some(tiberiums) =
                                raw.read_ares_int_list("OreGathering.Tiberiums")
                            {
                                gathering.tiberiums = tiberiums;
                            }
                            if let Some(frames) =
                                raw.read_ares_int_list("OreGathering.FramesPerDir")
                            {
                                gathering.frames_per_dir = frames;
                            }
                            self.family_mut(family)[index].ore_gathering = gathering;
                        }
                    }
                    RulesTypeFamily::Infantry => {
                        if let Some(weapon) = self.resolve_scalar_from(
                            &raw,
                            "OccupyWeapon",
                            RulesTypeFamily::Weapon,
                            0x80,
                        ) {
                            self.family_mut(family)[index]
                                .techno_references
                                .occupy_weapon = weapon;
                        }
                        if let Some(weapon) = self.resolve_scalar_from(
                            &raw,
                            "EliteOccupyWeapon",
                            RulesTypeFamily::Weapon,
                            0x80,
                        ) {
                            self.family_mut(family)[index]
                                .techno_references
                                .elite_occupy_weapon = weapon;
                        }
                        if let Some(anims) = self.resolve_list_from(
                            &raw,
                            "DeadBodies",
                            RulesTypeFamily::Animation,
                            0x80,
                        ) {
                            self.family_mut(family)[index].techno_references.dead_bodies = anims;
                        }
                        self.allocate_list_from(
                            &raw,
                            "DeathAnims",
                            RulesTypeFamily::Animation,
                            0x80,
                        );
                        // InfantryRead5246A1..52472D: Crawls and the four
                        // discharge frames from this pass's Image section.
                        self.family_mut(family)[index]
                            .infantry_art
                            .read_pass(fixed_art.section_or_empty(&image));
                        self.family_mut(family)[index]
                            .infantry_sequence_bank
                            .as_mut()
                            .expect("InfantryType constructor owns a sequence bank")
                            .read_pass(fixed_art, &image, fixed_sounds);
                    }
                    _ => unreachable!("only Techno families enter the Techno reader"),
                }
                if family == RulesTypeFamily::Building {
                    if let Some((turret, barrel)) =
                        super::voxel_frame_catalog::building_voxel_read_names(&effective)
                    {
                        let member = read_member(&mut self.families, family, index);
                        self.model_plan.load_building(
                            &mut member.object_image.model_slots,
                            turret.as_deref(),
                            Some(&barrel),
                        );
                    }
                }
                if ares_way && family == RulesTypeFamily::Building {
                    // 1000DD40 opens with the Prism forwarding reader
                    // 10010680, after every original Building allocation.
                    self.read_ares_prism_forwarding(index, &native_stored_id, &raw);
                    // Building suffix464A49/464A56 ->1000FDB0 runs after
                    // original Building fields. Use the current reached-body
                    // projection's stock readers, including prior defaults.
                    let aircraft_repair = effective.read_bool("UnitRepair", false)
                        && effective.read_factory("Factory", 0) == 3;
                    self.family_mut(family)[index]
                        .explicit_factory
                        .as_mut()
                        .expect("BuildingType owns its Ares extension")
                        .read_pass(&raw, aircraft_repair);
                }
            }
            index += 1;
        }
    }

    /// `BuildingTypeExt::cPrismForwarding::LoadFromINIFile @ 10010680` for
    /// the Building at `index` (`rules::ares_prism_forwarding`), then, for an
    /// enabled tower, its two support weapon reads (`100103F0`, normal then
    /// elite): ReadString(2048) and `WeaponTypeClass::FindOrAllocate`
    /// (`10010570`), which answers null for a blank or `none` name and then
    /// changes nothing. The weapon goes to the slot an earlier pass chose, or
    /// the first empty one of 2..17, and gets `[CombatDamage] C4Warhead` when
    /// it has no Warhead yet and `NeverUse=yes` (the weapon's own later reads
    /// still override both).
    fn read_ares_prism_forwarding(&mut self, index: usize, type_id: &str, raw: &IniSection) {
        let _ares = crate::rules::ares_switch::ares_reads();
        let targets = self.resolve_existing_list_from(
            raw,
            "PrismForwarding.Targets",
            RulesTypeFamily::Building,
            2048,
        );
        let prism_type = self.rules_prism_type.clone();
        let enabled = self.family_mut(RulesTypeFamily::Building)[index]
            .building_type_ext
            .prism_forwarding_mut()
            .read_pass(raw, type_id, prism_type.as_deref(), targets);
        if !enabled {
            return;
        }
        for (key, elite) in [
            ("PrismForwarding.SupportWeapon", false),
            ("PrismForwarding.EliteSupportWeapon", true),
        ] {
            let Some(Some(weapon)) =
                self.resolve_scalar_from(raw, key, RulesTypeFamily::Weapon, 2048)
            else {
                continue;
            };
            let member = &mut self.family_mut(RulesTypeFamily::Building)[index];
            let mut slot = member
                .building_type_ext
                .prism_forwarding()
                .support_weapon_index(elite);
            if slot < 0 {
                let Some(unused) = member.weapon_slots.unused_prism_slot(elite) else {
                    // Ares stops the game here; VERA reports it and leaves
                    // the tower without that support weapon.
                    log::error!(
                        "BuildingType [{type_id}] is a Prism Tower however there are no free \
                         weapon slots to assign the {}support weapon to",
                        if elite { "elite " } else { "" }
                    );
                    continue;
                };
                slot = unused as i32;
                member
                    .building_type_ext
                    .prism_forwarding_mut()
                    .set_support_weapon_index(elite, slot);
            }
            member
                .weapon_slots
                .install_prism_support_weapon(slot as usize, elite, weapon.clone());
            let Some(weapon_index) = self.find_existing(RulesTypeFamily::Weapon, &weapon) else {
                continue;
            };
            let c4 = self
                .rules_type_references
                .pointer("C4Warhead")
                .and_then(|id| self.find_existing(RulesTypeFamily::Warhead, id));
            let member = &mut self.family_mut(RulesTypeFamily::Weapon)[weapon_index];
            if member.weapon.warhead.is_none() {
                member.weapon.warhead = c4;
            }
            let mut never_use = IniSection::new(member.native_stored_id.clone());
            never_use.set("NeverUse", "yes");
            member.body.overlay_rules_pass(&never_use);
        }
    }

    fn process_factory_owners(&mut self, family: RulesTypeFamily, index: usize, raw: &IniSection) {
        let _ares = crate::rules::ares_switch::ares_reads();
        let mut state = self.family_mut(family)[index]
            .factory_owners
            .take()
            .expect("TechnoType owns its Ares extension");
        state.read_pass(
            raw,
            |token| {
                ares_country_index_of_name(
                    token,
                    self.families
                        .get(&RulesTypeFamily::Country)
                        .into_iter()
                        .flatten()
                        .map(|member| member.native_stored_id.as_str()),
                )
            },
            || {
                self.resolve_existing_list_indices_from(
                    raw,
                    "BuiltAt",
                    RulesTypeFamily::Building,
                    2048,
                )
                .map(|indices| {
                    indices
                        .into_iter()
                        .map(|index| {
                            i32::try_from(index)
                                .expect("BuildingTypes exceeds native i32 index space")
                        })
                        .collect()
                })
            },
        );
        self.family_mut(family)[index].factory_owners = Some(state);
    }

    fn process_techno_base(&mut self, family: RulesTypeFamily, index: usize, raw: &IniSection) {
        let ares_way = crate::rules::ares_switch::ares_way();
        if let Some(weapon) =
            self.resolve_scalar_from(raw, "DeathWeapon", RulesTypeFamily::Weapon, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .death_weapon = weapon;
        }
        // Ares 3.0 TechnoTypeClass_LoadFromINI_ListLength (1005BF90, hook
        // 0x7125DF) reads these six lists here through ParseList (shared 0x800
        // buffer, Find-only), DebrisMaximums being ObjectType's int list, and
        // skips the original reads to 0x712830; SkipLists1/2 (1005C020 hook
        // 0x713171, 1005C040 hook 0x713C10) skip the later Dock and
        // Damage/DestroyParticleSystems reads.
        if ares_way {
            for (key, list_family) in [
                ("DamageParticleSystems", RulesTypeFamily::ParticleSystem),
                ("DestroyParticleSystems", RulesTypeFamily::ParticleSystem),
                ("Dock", RulesTypeFamily::Building),
                ("DebrisTypes", RulesTypeFamily::VoxelAnimation),
                ("DebrisAnims", RulesTypeFamily::Animation),
            ] {
                let Some(list) = self.resolve_existing_list_from(raw, key, list_family, 0x800)
                else {
                    continue;
                };
                let references = &mut self.family_mut(family)[index].techno_references;
                *match key {
                    "DamageParticleSystems" => &mut references.damage_particle_systems,
                    "DestroyParticleSystems" => &mut references.destroy_particle_systems,
                    "Dock" => &mut references.dock,
                    "DebrisTypes" => &mut references.debris_types,
                    _ => &mut references.debris_anims,
                } = list;
            }
        } else {
            if let Some(types) =
                self.resolve_list_from(raw, "DebrisTypes", RulesTypeFamily::VoxelAnimation, 0x80)
            {
                self.family_mut(family)[index]
                    .techno_references
                    .debris_types = types;
            }
            if let Some(anims) =
                self.resolve_list_from(raw, "DebrisAnims", RulesTypeFamily::Animation, 0x80)
            {
                self.family_mut(family)[index]
                    .techno_references
                    .debris_anims = anims;
            }
        }

        // TechnoType ReadINI 0x0071284A..0x00712AAD. IsChargeTurret, read at
        // 0x0071287E between WeaponCount and ClearAllWeapons, is GunnerTurrets'.
        let mut slots = std::mem::take(&mut self.family_mut(family)[index].weapon_slots);
        slots.turret_count = raw.read_int("TurretCount", slots.turret_count);
        // Clamped to VERA's bound (`WEAPON_COUNT_LIMIT`): the slot loop below
        // and the Ares way's vectors grow with it.
        slots.weapon_count = raw
            .read_int("WeaponCount", slots.weapon_count)
            .min(crate::rules::object_type::WEAPON_COUNT_LIMIT);
        slots.clear_all_weapons = raw.read_bool("ClearAllWeapons", slots.clear_all_weapons);
        if slots.turret_count > 0 {
            if ares_way {
                self.family_mut(family)[index]
                    .gunner_turrets
                    .prepare_numbered_read(slots.weapon_count);
                //10040020 resizes before reading: shrinking destroys the
                // extra tail; regrowing supplies null WeaponStruct pointers.
                let extra = usize::try_from(slots.weapon_count)
                    .unwrap_or(0)
                    .saturating_sub(WEAPON_SLOT_COUNT);
                slots.extra_base.resize(extra, None);
                slots.extra_elite.resize(extra, None);
            }
            let capacity = if ares_way { 2048 } else { 0x80 };
            for slot in 1..=slots.weapon_count {
                // Ares1003C6B0 ->10010570 retains Find-or-Allocate semantics
                // (constructor trampoline100103C0 -> WeaponType771C70).
                let base = self.resolve_scalar_from(
                    raw,
                    &format!("Weapon{slot}"),
                    RulesTypeFamily::Weapon,
                    capacity,
                );
                let elite = self.resolve_scalar_from(
                    raw,
                    &format!("EliteWeapon{slot}"),
                    RulesTypeFamily::Weapon,
                    capacity,
                );
                let index = (slot - 1) as usize;
                if index < WEAPON_SLOT_COUNT {
                    if let Some(weapon) = base {
                        slots.base[index] = weapon;
                    }
                    if let Some(weapon) = elite {
                        slots.elite[index] = weapon;
                    }
                } else if ares_way {
                    if let Some(weapon) = base {
                        slots.extra_base[index - WEAPON_SLOT_COUNT] = weapon;
                    }
                    if let Some(weapon) = elite {
                        slots.extra_elite[index - WEAPON_SLOT_COUNT] = weapon;
                    }
                }
                // Original gamemd writes out of bounds beyond eighteen. The
                // stock way keeps its existing safety boundary.
            }
        } else if !slots.clear_all_weapons {
            // 0x007129AB..0x00712A89: Primary/Secondary are base slots 0/1,
            // ElitePrimary/EliteSecondary elite slots 0/1.
            for (key, elite, slot) in [
                ("Primary", false, 0),
                ("Secondary", false, 1),
                ("ElitePrimary", true, 0),
                ("EliteSecondary", true, 1),
            ] {
                if let Some(weapon) =
                    self.resolve_scalar_from(raw, key, RulesTypeFamily::Weapon, 0x80)
                {
                    let array = if elite {
                        &mut slots.elite
                    } else {
                        &mut slots.base
                    };
                    array[slot] = weapon;
                }
            }
        }
        if slots.clear_all_weapons {
            // 0x00712A8F..0x00712AAD clears slots 0 and 1 after either branch.
            for array in [&mut slots.base, &mut slots.elite] {
                array[0] = None;
                array[1] = None;
            }
        }
        self.family_mut(family)[index].weapon_slots = slots;

        // Ares reads Dock at 0x7125DF instead (above).
        if !ares_way
            && let Some(buildings) =
                self.resolve_list_from(raw, "Dock", RulesTypeFamily::Building, 0x80)
        {
            self.family_mut(family)[index].techno_references.dock = buildings;
        }
        if let Some(building) =
            self.resolve_scalar_from(raw, "DeploysInto", RulesTypeFamily::Building, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .deploys_into = building;
        }
        if let Some(unit) =
            self.resolve_scalar_from(raw, "UndeploysInto", RulesTypeFamily::Vehicle, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .undeploys_into = unit;
        }
        if let Some(unit) =
            self.resolve_scalar_from(raw, "PowersUnit", RulesTypeFamily::Vehicle, 0x80)
        {
            self.family_mut(family)[index].techno_references.powers_unit = unit;
        }
        if let Some(anims) =
            self.resolve_list_from(raw, "Explosion", RulesTypeFamily::Animation, 0x80)
        {
            self.family_mut(family)[index].techno_references.explosion = anims;
        }
        if let Some(anims) =
            self.resolve_list_from(raw, "DestroyAnim", RulesTypeFamily::Animation, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .destroy_anim = anims;
        }
        if let Some(system) = self.resolve_scalar_from(
            raw,
            "NaturalParticleSystem",
            RulesTypeFamily::ParticleSystem,
            0x80,
        ) {
            self.family_mut(family)[index]
                .techno_references
                .natural_particle_system = system;
        }
        if let Some(system) = self.resolve_scalar_from(
            raw,
            "RefinerySmokeParticleSystem",
            RulesTypeFamily::ParticleSystem,
            0x80,
        ) {
            self.family_mut(family)[index]
                .techno_references
                .refinery_smoke_particle_system = system;
        }
        // Ares reads both at 0x7125DF instead (above).
        if !ares_way {
            if let Some(systems) = self.resolve_list_from(
                raw,
                "DamageParticleSystems",
                RulesTypeFamily::ParticleSystem,
                0x80,
            ) {
                self.family_mut(family)[index]
                    .techno_references
                    .damage_particle_systems = systems;
            }
            if let Some(systems) = self.resolve_list_from(
                raw,
                "DestroyParticleSystems",
                RulesTypeFamily::ParticleSystem,
                0x80,
            ) {
                self.family_mut(family)[index]
                    .techno_references
                    .destroy_particle_systems = systems;
            }
        }
        // TechnoTypeClass::ReadINI712170, original714016..71410F:
        // read the current-default flag before the count/rates and stage gate.
        let member = &mut self.family_mut(family)[index];
        member.is_gattling = raw.read_bool("IsGattling", member.is_gattling);
        member.gattling_stages.apply_pass(raw, member.is_gattling);

        // ReadINI714570..71458A passes the current+634 to ReadInt5276D0.
        // An absent exact-case key (also physically blank values omitted by
        // the INI loader) retains it. Present integers stay signed/unclamped.
        // This precedes the AirstrikeTeam constructor reads.
        let member = &mut self.family_mut(family)[index];
        member.lepton_mind_control_offset =
            raw.read_int("LeptonMindControlOffset", member.lepton_mind_control_offset);
        member.tech_level = raw.read_int("TechLevel", member.tech_level);

        // Preserve both the six-field fallback order and Aircraft constructor
        // chronology at TechnoType71458A..714654. Later text merging cannot
        // recover an elite value reset by an omitted key in a reached section.
        let mut airstrike = std::mem::take(&mut self.family_mut(family)[index].airstrike);
        airstrike.apply_pass(raw, |name| {
            self.find_or_allocate(RulesTypeFamily::Aircraft, name)
                .map(|aircraft| {
                    self.families[&RulesTypeFamily::Aircraft][aircraft]
                        .native_stored_id
                        .clone()
                })
        });
        self.family_mut(family)[index].airstrike = airstrike;
        if let Some(unit) =
            self.resolve_scalar_from(raw, "UnloadingClass", RulesTypeFamily::Vehicle, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .unloading_class = unit;
        }
        if let Some(anim) =
            self.resolve_scalar_from(raw, "DeployingAnim", RulesTypeFamily::Animation, 0x80)
        {
            self.family_mut(family)[index]
                .techno_references
                .deploying_anim = anim;
        }
        if let Some(infantry) =
            self.resolve_scalar_from(raw, "Enslaves", RulesTypeFamily::Infantry, 0x80)
        {
            self.family_mut(family)[index].techno_references.enslaves = infantry;
        }
        if let Some(aircraft) =
            self.resolve_scalar_from(raw, "Spawns", RulesTypeFamily::Aircraft, 0x80)
        {
            self.family_mut(family)[index].techno_references.spawns = aircraft;
        }
        // Original ReadINI7147B4..7147C8 stores ReadInt5276D0 directly.
        // Missing sections never enter this reader; omitted keys retain the
        // prior value, including across process-resident registry handoffs.
        let member = &mut self.family_mut(family)[index];
        member.radial_fire_segments =
            raw.read_int("RadialFireSegments", member.radial_fire_segments);
        // ReadINI71551E..715720. begin_rules_member_read has admitted this
        // actual section, so zero/unordered values now take this pass's General.
        let defaults = self.rules_threat_coefficient_defaults;
        self.family_mut(family)[index]
            .threat_coefficients
            .read_type(raw, defaults);
    }

    /// A reference read from the fixed ART section the type's Image names; a
    /// missing section reads nothing.
    fn resolve_fixed_art_reference(
        &mut self,
        fixed_art: &IniFile,
        image: &str,
        key: &str,
        family: RulesTypeFamily,
    ) -> Option<Option<String>> {
        if image.is_empty() {
            return None;
        }
        self.resolve_scalar_from(fixed_art.section(image)?, key, family, 0x80)
    }
}
