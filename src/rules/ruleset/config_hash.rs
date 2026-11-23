//! `RuleSet::simulation_config_hash`: the compatibility identity of the
//! loaded rules and the ART inputs bound to them.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use crate::rules::object_type::ObjectCategory;
use crate::util::native_x87::NativeF64Bits;

use super::{RuleSet, default_fog_rate, default_shroud_rate};

impl RuleSet {
    /// Compatibility identity for processed rules plus the resolved animation,
    /// effect-frame, terrain-spawner frame, smudge-selection, projectile
    /// launch and TechnoType `CanBeHidden=` and `Palette=` ART inputs bound to
    /// this ruleset.
    /// Other asset-derived simulation inputs are added by later ownership
    /// slices and are not claimed by this hash yet. Replays and saves compare
    /// it across platforms, so it folds with the portable
    /// [`StateHasher`](crate::util::state_hasher::StateHasher).
    pub fn simulation_config_hash(&self) -> u64 {
        let mut hasher = crate::util::state_hasher::StateHasher::new();
        // v34: Country starting-veterancy; optional extensions add domains below.
        b"rules-simulation-config-v34".hash(&mut hasher);
        self.source_ini_hash.hash(&mut hasher);
        // Eager and authored loads recover the same deck, including wooden rims.
        b"independent-high-bridge-deck-v6".hash(&mut hasher);
        // A previous scenario's list survives RulesClass's Type reset even
        // when the incoming source text omits AIHateDelays.
        if !self.general.ai_hate_delays.is_empty() {
            b"ai-hate-delays-v1".hash(&mut hasher);
            self.general.ai_hate_delays.hash(&mut hasher);
        }
        // Identical current text can inherit different CombatDamage values.
        b"drain-money-controls-v1".hash(&mut hasher);
        self.general.drain_money_frame_delay.hash(&mut hasher);
        self.general.drain_money_amount.hash(&mut hasher);
        // The separately saved global89EA40 table can differ for identical
        // current INI text after earlier Process passes or a load.
        self.terrain_rules.hash(&mut hasher);
        // ReadAudioVisual669A60/669A99/669FC3 can retain different fixed
        // sound references for identical current INI text after a reset/load.
        b"global-sound-references-v1".hash(&mut hasher);
        self.general.bomb_ticking_sound.hash(&mut hasher);
        self.general.bomb_attach_sound.hash(&mut hasher);
        self.general.psychic_reveal_activate_sound.hash(&mut hasher);
        [
            &self.general.gui_build_sound,
            &self.general.gui_tab_sound,
            &self.general.building_slam,
        ]
        .hash(&mut hasher);
        b"mind-control-sound-references-v1".hash(&mut hasher);
        (
            &self.mind_control.mind_control_sound,
            &self.mind_control.mind_cleared_sound,
            &self.mind_control.overload_sound,
        )
            .hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    &object.mind_cleared_sound,
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        b"infantry-sequence-bank-v1".hash(&mut hasher);
        Self::by_upper_id(self.object_list.iter().filter_map(|object| {
            object
                .infantry_sequence_bank()
                .map(|bank| (&object.id, bank))
        }))
        .hash(&mut hasher);

        b"powerups-v1".hash(&mut hasher);
        self.powerups.hash(&mut hasher);
        // ReadINI71551E..715720 can retain different weights for identical
        // current text. Include the projected bits and process-resident defaults.
        b"threat-coefficients-v1".hash(&mut hasher);
        self.general.threat_coefficient_defaults.hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    object.threat_coefficients(),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        b"attack-cursor-disguise-v1".hash(&mut hasher);
        self.general.attack_cursor_on_disguise.hash(&mut hasher);
        b"allow-shrouded-subterannean-moves-v1".hash(&mut hasher);
        self.general
            .allow_shrouded_subterannean_moves
            .hash(&mut hasher);
        // InfantryTypeRead52455F retains this byte across missing/invalid
        // reads; equal current source text may therefore evacuate differently.
        let mut civilian_types: Vec<_> = self
            .object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Infantry && object.civilian)
            .map(|object| object.id.to_ascii_uppercase())
            .collect();
        if !civilian_types.is_empty() {
            civilian_types.sort_unstable();
            b"infantry-civilian-v1".hash(&mut hasher);
            civilian_types.hash(&mut hasher);
        }
        // InfantryType ReadINI5245B8..5245F5 retains both theft bytes across
        // invalid later reads. Flattened text alone cannot identify that state.
        let mut theft_flags: Vec<_> = self
            .object_list
            .iter()
            .filter(|object| {
                object.category == ObjectCategory::Infantry
                    && (object.thief || object.vehicle_thief)
            })
            .map(|object| {
                (
                    object.id.to_ascii_uppercase(),
                    object.thief,
                    object.vehicle_thief,
                )
            })
            .collect();
        if !theft_flags.is_empty() {
            theft_flags.sort_unstable();
            b"infantry-theft-flags-v1".hash(&mut hasher);
            theft_flags.hash(&mut hasher);
        }
        // OverlayRead5FE8BE / InitTheater5FE620 retain typed history that a
        // flattened Image key cannot recover; saved resource plans matter too.
        self.overlay_registry.hash_configuration(&mut hasher);
        // ObjectTypeRead5F93D5 can retain a different Voc ID for identical
        // current text. Keep concrete type identities distinct in the hash.
        b"object-crush-sounds-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    &object.crush_sound,
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Fixed ART foundation state is not covered by the layered RULES
        // source hash. Include retained declared/list/outline geometry, also
        // when a later rules pass no longer authors the original custom key.
        b"building-foundation-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| (object.id.to_ascii_uppercase(), &object.foundation))
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // ReadOwners4750D0 retains an explicit zero RequiredHouses mask
        // through later absent/empty reads. Current-pass source text does
        // not encode this history; keep ordinary stock hashes unchanged.
        let mut empty_required_houses: Vec<_> = self
            .object_list
            .iter()
            .filter(|object| {
                object.required_houses.is_empty() && object.required_houses_mask(self) == 0
            })
            .map(|object| (object.category, object.id.to_ascii_uppercase()))
            .collect();
        if !empty_required_houses.is_empty() {
            empty_required_houses.sort_unstable();
            b"required-houses-explicit-empty-v1".hash(&mut hasher);
            empty_required_houses.hash(&mut hasher);
        }
        // Bullet46BEE0 retains this byte even when a later raw value is
        // invalid. Equal flattened INIs can therefore have different flight
        // damage. Preserve the previous hash domain for all-false stock rules.
        let mut degenerating_projectiles: Vec<_> = self
            .projectiles_iter()
            .filter(|projectile| projectile.degenerates)
            .map(|projectile| projectile.id.to_ascii_uppercase())
            .collect();
        if !degenerating_projectiles.is_empty() {
            degenerating_projectiles.sort();
            b"projectile-degenerates-v1".hash(&mut hasher);
            degenerating_projectiles.hash(&mut hasher);
        }
        // Per-pass gates and overlapping writes can retain different tables
        // for identical current sources.
        b"gattling-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (object.is_gattling, object.gattling_stages),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        b"country-parent-v1".hash(&mut hasher);
        // The native parent lookup scans registration order and checks Name
        // aliases as well as IDs. These retained inputs can differ after an
        // identical later pass, even when source_ini_hash is the same.
        self.country_ids
            .iter()
            .map(|id| {
                let country = &self.countries[id];
                (id, &country.name, &country.parent_country)
            })
            .collect::<Vec<_>>()
            .hash(&mut hasher);
        b"country-starting-veterancy-v1".hash(&mut hasher);
        Self::by_upper_id(
            self.countries
                .iter()
                .map(|(id, country)| (id, &country.veteran_types)),
        )
        .hash(&mut hasher);
        // Peers must list the same lobby colours and draw a priority alike.
        b"skirmish-colors-v1".hash(&mut hasher);
        self.skirmish_color_mode.hash(&mut hasher);
        // Which plant a computer house splices is simulation state.
        if self.ares {
            b"ares-country-powerplants-v1".hash(&mut hasher);
            Self::by_upper_id(
                self.countries
                    .iter()
                    .map(|(id, country)| (id, &country.power_plants)),
            )
            .hash(&mut hasher);
        }
        // Which units a country's paradrop sends is simulation state, so the
        // Ares country overrides enter the state hash alongside it.
        if self.ares {
            b"ares-country-paradrop-v1".hash(&mut hasher);
            Self::by_upper_id(
                self.countries
                    .iter()
                    .map(|(id, country)| (id, &country.paradrop)),
            )
            .hash(&mut hasher);
        }
        #[cfg(feature = "phobos")]
        {
            b"phobos-super-availability-v1".hash(&mut hasher);
            self.phobos_super_weapon_metadata.hash(&mut hasher);
        }
        if self.phobos {
            self.parallel_ai_queues.hash_into(&mut hasher);
        }
        if self.ares {
            b"ares-side-defaults-v1".hash(&mut hasher);
            b"ares-super-availability-v1".hash(&mut hasher);
            self.ares_super_weapon_metadata.hash(&mut hasher);
            b"ares-weapon-ammo-v1".hash(&mut hasher);
            Self::by_upper_id(
                self.weapon_entries()
                    .map(|(id, weapon)| (id, weapon.ammo_cost())),
            )
            .hash(&mut hasher);
            self.object_list
                .iter()
                .filter(|object| !object.additional_super_weapons().is_empty())
                .map(|object| (&object.id, object.additional_super_weapons()))
                .collect::<Vec<_>>()
                .hash(&mut hasher);
            self.side_ids.hash(&mut hasher);
            self.side_extensions.hash(&mut hasher);
        }
        #[cfg(feature = "phobos")]
        {
            b"phobos-sidebar-gdi-positions-v1".hash(&mut hasher);
            self.side_gdi_positions.hash(&mut hasher);
        }
        #[cfg(feature = "phobos")]
        {
            b"phobos-radiation-types-v1".hash(&mut hasher);
            self.radiation.radiation_types.hash(&mut hasher);
            // Bound weapon `RadType=` names, by weapon ID: the map's
            // iteration order differs between targets.
            let mut rad_types: Vec<_> = self
                .weapon_entries()
                .map(|(id, weapon)| (id, &weapon.rad_type))
                .collect();
            rad_types.sort_unstable_by_key(|&(id, _)| id);
            rad_types.hash(&mut hasher);
            b"phobos-shield-types-v1".hash(&mut hasher);
            self.shield_types.hash(&mut hasher);
            self.shield_globals.apply_armor_mult.hash(&mut hasher);
            self.shield_globals
                .condition_yellow
                .to_bits()
                .hash(&mut hasher);
            self.shield_globals
                .condition_red
                .to_bits()
                .hash(&mut hasher);
            self.shield_globals.pips.hash(&mut hasher);
            self.shield_globals.pips_building.hash(&mut hasher);
            self.shield_globals.pips_background.hash(&mut hasher);
            self.shield_globals.pips_building_empty.hash(&mut hasher);
            // Bound techno `ShieldType=` names, in registry order.
            self.object_list
                .iter()
                .map(|object| (&object.id, object.shield_type()))
                .collect::<Vec<_>>()
                .hash(&mut hasher);
            b"phobos-digital-display-v1".hash(&mut hasher);
            self.digital_display_types.hash(&mut hasher);
            self.digital_display_globals.hash(&mut hasher);
            b"phobos-placement-preview-v1".hash(&mut hasher);
            self.placement_preview_globals.hash(&mut hasher);
            self.object_list
                .iter()
                .map(|object| {
                    (
                        &object.id,
                        object.digital_display_types(),
                        object.digital_display_disable(),
                        object.digital_display_health_fake_at_disguise(),
                        object.placement_preview(),
                        object.placement_preview_shape(),
                        object.placement_preview_shape_frame(),
                        object.placement_preview_offset(),
                        object.placement_preview_remap(),
                        object.placement_preview_palette(),
                        object.placement_preview_translucency(),
                    )
                })
                .collect::<Vec<_>>()
                .hash(&mut hasher);
        }
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| (object.id.to_ascii_uppercase(), object.building_upgrade()))
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // The process registry is admitted only when its retained capability
        // matches this binary. Include that immutable build identity even if
        // the selected INIs contain no extension keys. The `false` held the
        // retired `ares` build feature and keeps a released game's hash.
        b"rules-build-compatibility-v1".hash(&mut hasher);
        false.hash(&mut hasher);
        // The game's Ares switch: two peers or a replay with the same layers
        // but another `ares.mix` answer play different games. Folded only
        // when Ares plays, so a stock game's hash stays as it was.
        if self.ares {
            b"ares-plays-v1".hash(&mut hasher);
        }
        if self.ares {
            b"ares-global-controls-v1".hash(&mut hasher);
            self.ares_global_controls.hash(&mut hasher);
            b"ares-factory-owners-v1".hash(&mut hasher);
            self.object_list
                .iter()
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.factory_owners_hash_state(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
                .hash(&mut hasher);
            // A previous Process can retain this manual-action policy even
            // when the current source stack contains only invalid tokens.
            b"ares-no-manual-fire-v1".hash(&mut hasher);
            self.object_list
                .iter()
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.no_manual_fire(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
                .hash(&mut hasher);
            // ObjectType IgnoresFirestorm=, which only the Ares firestorm
            // walls read; folded only once a type sets it.
            let ignores_firestorm = self
                .object_list
                .iter()
                .filter(|object| object.ignores_firestorm)
                .map(|object| (object.category, object.id.to_ascii_uppercase()))
                .collect::<std::collections::BTreeSet<_>>();
            if !ignores_firestorm.is_empty() {
                b"ares-ignores-firestorm-v1".hash(&mut hasher);
                ignores_firestorm.hash(&mut hasher);
            }
            b"ares-keep-alive-v1".hash(&mut hasher);
            let influence_types = self
                .object_list
                .iter()
                .filter(|object| {
                    object.designator_range().is_some()
                        || object.inhibitor_range().is_some()
                        || object.hunter_seeker_ignored()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        (
                            object.designator_range(),
                            object.inhibitor_range(),
                            object.hunter_seeker_ignored(),
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !influence_types.is_empty() {
                b"ares-sw-influence-types-v2".hash(&mut hasher);
                influence_types.hash(&mut hasher);
            }
            let chronoshift = self
                .object_list
                .iter()
                .filter(|object| object.ares_chronoshift() != Default::default())
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.ares_chronoshift(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !chronoshift.is_empty() {
                b"ares-chronoshift-types-v1".hash(&mut hasher);
                chronoshift.hash(&mut hasher);
            }
            self.object_list
                .iter()
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.keep_alive(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
                .hash(&mut hasher);
            let conversions = self
                .object_list
                .iter()
                .filter_map(|object| {
                    object
                        .script_conversion()
                        .map(|target| ((object.category, object.id.to_ascii_uppercase()), target))
                })
                .collect::<BTreeMap<_, _>>();
            if !conversions.is_empty() {
                b"ares-script-conversion-v1".hash(&mut hasher);
                conversions.hash(&mut hasher);
            }
            let surface_conversions = self
                .object_list
                .iter()
                .filter_map(|object| {
                    let targets = (
                        object.surface_conversion(true),
                        object.surface_conversion(false),
                    );
                    (targets.0.is_some() || targets.1.is_some())
                        .then(|| ((object.category, object.id.to_ascii_uppercase()), targets))
                })
                .collect::<BTreeMap<_, _>>();
            if !surface_conversions.is_empty() {
                b"ares-surface-conversion-v1".hash(&mut hasher);
                surface_conversions.hash(&mut hasher);
            }
            let turn_overrides = self
                .object_list
                .iter()
                .filter_map(|object| {
                    object
                        .turret_rot_override()
                        .map(|rate| ((object.category, object.id.to_ascii_uppercase()), rate))
                })
                .collect::<BTreeMap<_, _>>();
            if !turn_overrides.is_empty() {
                b"ares-secondary-turn-rate-v1".hash(&mut hasher);
                turn_overrides.hash(&mut hasher);
            }
        }
        // Equal flattened INIs may have different retained group lists.
        self.ares_prerequisites.hash(&mut hasher);
        b"retained-armor-bindings-and-ares-versus-v1".hash(&mut hasher);
        self.ares_armors.hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    object.armor_id(),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        Self::by_upper_id(
            self.terrain_object_types
                .iter()
                .map(|(name, terrain)| (name, terrain.armor_id())),
        )
        .hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            // TerrainTypeExt::LoadFromINI retains these across Process calls.
            // They change spawner placement, particle births and RNG draws.
            b"phobos-retained-ore-spawners-v1".hash(&mut hasher);
            Self::by_upper_id(
                self.terrain_object_types
                    .iter()
                    .map(|(name, terrain)| (name, &terrain.spawns_tiberium_ext)),
            )
            .hash(&mut hasher);
        }
        // TerrainTypeReadINI71DEA0 retains post-read writes and ReadInt
        // defaults across Process calls. Equal current text can admit
        // different Strength, Logic members, targets or occupation, so identify
        // the effective fields as well as the current source layers.
        b"terrain-strength-logic-targeting-occupation-and-foundation-v6".hash(&mut hasher);
        Self::by_upper_id(self.terrain_object_types.iter().map(|(name, terrain)| {
            (
                name,
                (
                    terrain.strength,
                    terrain.is_logic(),
                    terrain.legal_target,
                    terrain.raw_occupation_bits(false),
                    terrain.raw_occupation_bits(true),
                    terrain.foundation.as_str(),
                    terrain.has_occupy_list(),
                ),
            )
        }))
        .hash(&mut hasher);
        Self::by_upper_id(
            self.warhead_entries()
                .map(|(name, warhead)| (name, warhead.ares_verses_hash_state())),
        )
        .hash(&mut hasher);

        if self.ares {
            // TypeExt bounty values and the attacker's enable bit are gameplay
            // inputs; display-only overrides do not enter this map or its hash.
            let bounty = self
                .object_list
                .iter()
                .filter(|object| object.bounty_rules().has_gameplay_configuration())
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.bounty_rules(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !bounty.is_empty() {
                b"ares-bounty-rules-v1".hash(&mut hasher);
                bounty.hash(&mut hasher);
            }
            // Ares TypeExt driver fields retain earlier successful reads even
            // when the merged text contains later invalid/empty values.
            let drivers = self
                .object_list
                .iter()
                .filter(|object| {
                    object.driver_rules() != &crate::rules::ares_driver::DriverTypeRules::default()
                        || object.vehicle_thief_sound(false).is_some()
                        || object.vehicle_thief_sound(true).is_some()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        (
                            object.driver_rules(),
                            object.vehicle_thief_sound(false),
                            object.vehicle_thief_sound(true),
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !drivers.is_empty() {
                b"ares-driver-rules-v1".hash(&mut hasher);
                drivers.hash(&mut hasher);
            }
            let aircraft_crash_overrides = self
                .object_list
                .iter()
                .filter_map(|object| {
                    object
                        .aircraft_crashable_override()
                        .map(|value| ((object.category, object.id.to_ascii_uppercase()), value))
                })
                .collect::<BTreeMap<_, _>>();
            if !aircraft_crash_overrides.is_empty() {
                b"ares-aircraft-crashable-v1".hash(&mut hasher);
                aircraft_crash_overrides.hash(&mut hasher);
            }
            // TypeExt IronCurtain.Modifier/ForceShield.Modifier scale every
            // curtain TechnoClass::IronCurtain (hook 70E2B0) applies.
            let curtain_modifiers = self
                .object_list
                .iter()
                .filter(|object| object.ares_curtain_modifiers() != [NativeF64Bits::ONE; 2])
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.ares_curtain_modifiers(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !curtain_modifiers.is_empty() {
                b"ares-curtain-modifiers-v1".hash(&mut hasher);
                curtain_modifiers.hash(&mut hasher);
            }
            let cloak_power = self
                .object_list
                .iter()
                .filter(|object| {
                    object.cloak_power()
                        != &crate::rules::ares_cloak_power::CloakPowerTypeRules::default()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.cloak_power(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !cloak_power.is_empty() {
                b"ares-cloak-power-rules-v1".hash(&mut hasher);
                cloak_power.hash(&mut hasher);
            }
            // TypeExt BuildTime.*, FactoryPlant.Multiplier and cloning
            // decide build times, prices and clones.
            let production = self
                .object_list
                .iter()
                .filter(|object| {
                    object.ares_production()
                        != &crate::rules::ares_production::ProductionTypeRules::default()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.ares_production(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !production.is_empty() {
                b"ares-production-rules-v1".hash(&mut hasher);
                production.hash(&mut hasher);
            }
            // TypeExt Experience.*/Promote.* and BuildingTypeExt Academy.*
            // decide who gains experience and what a promotion converts to.
            let veterancy = self
                .object_list
                .iter()
                .filter(|object| {
                    object.ares_veterancy()
                        != &crate::rules::ares_veterancy::VeterancyTypeRules::default()
                        || object.academy()
                            != &crate::rules::ares_veterancy::AcademyTypeRules::default()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        (object.ares_veterancy(), object.academy()),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !veterancy.is_empty() || self.ares_veterancy().veteran_flash_timer() != 0 {
                b"ares-veterancy-rules-v1".hash(&mut hasher);
                veterancy.hash(&mut hasher);
                self.ares_veterancy().hash(&mut hasher);
            }
            // BuildingTypeExt rubble, repair and garrison keys.
            let building_type_ext = self
                .object_list
                .iter()
                .filter(|object| {
                    object.building_type_ext()
                        != &crate::rules::ares_building_type::BuildingTypeExtRules::default()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.building_type_ext(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            let building_globals = self.ares_building_globals();
            if !building_type_ext.is_empty()
                || building_globals
                    != &crate::rules::ares_building_type::BuildingGlobalRules::default()
            {
                b"ares-building-type-ext-rules-v1".hash(&mut hasher);
                building_type_ext.hash(&mut hasher);
                building_globals.hash(&mut hasher);
            }
            let crews = self
                .object_list
                .iter()
                .filter(|object| {
                    object.crew_rules() != &crate::rules::ares_crew::CrewTypeRules::default()
                })
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.crew_rules(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !crews.is_empty() {
                b"ares-survivor-rules-v1".hash(&mut hasher);
                crews.hash(&mut hasher);
            }
            b"ares-emp-rules-v1".hash(&mut hasher);
            self.emp_sparkles.hash(&mut hasher);
            self.object_list
                .iter()
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        (
                            object.emp_rules(),
                            object.promotion_abilities(),
                            object.self_attach_effect(),
                            object.cloak_allowed(),
                        ),
                    )
                })
                .collect::<BTreeMap<_, _>>()
                .hash(&mut hasher);
            Self::by_upper_id(
                self.warhead_entries()
                    .map(|(name, wh)| (name, wh.ares_rules())),
            )
            .hash(&mut hasher);
        }

        // UnitType747734 retains CarriesCrate across later absent/invalid
        // keys; equal current source text can still mean different cargo.
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Vehicle)
            .map(|object| (object.id.to_ascii_uppercase(), object.carries_crate()))
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Global construction order changes which category106 mutates when
        // IDs coincide. Current TechLevels can also differ after retained
        // default reads even when the compatibility INI is identical.
        b"native-techno-type-order-and-tech-level-v1".hash(&mut hasher);
        self.native_techno_type_order.hash(&mut hasher);
        for (category, id, handle) in &self.native_techno_type_order {
            (category, id).hash(&mut hasher);
            handle
                .map(|handle| self.object_by_handle(handle).tech_level)
                .hash(&mut hasher);
        }
        // A continued Rules process can retain a different BuildingType+EBC
        // for the same current source stack. Hash the effective projection.
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| (object.id.to_ascii_uppercase(), object.target_coord_offset()))
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Process-resident Gravity and Weapon postpass results can differ for
        // identical current source stacks because earlier passes retained them.
        self.general.gravity.hash(&mut hasher);
        self.general.tree_strength.hash(&mut hasher);
        self.general.infantry_repair_step.hash(&mut hasher);
        self.general.unit_repair_rate.to_bits().hash(&mut hasher);
        self.general.charge_to_drain_ratio.hash(&mut hasher);
        self.general
            .infantry_repair_rate
            .to_bits()
            .hash(&mut hasher);
        self.general.wind_direction.hash(&mut hasher);
        self.general.parachute_max_fall_rate.hash(&mut hasher);
        self.general.no_parachute_max_fall_rate.hash(&mut hasher);
        self.general.detail.hash(&mut hasher);
        [
            self.general.extra_unit_light,
            self.general.extra_infantry_light,
            self.general.extra_aircraft_light,
        ]
        .hash(&mut hasher);
        self.general.ambient_change_rate_nonzero.hash(&mut hasher);
        self.general
            .ambient_change_interval_frames
            .hash(&mut hasher);
        self.general.ambient_change_step.hash(&mut hasher);
        self.general.prism_support.hash(&mut hasher);
        // These current-default inputs survive a process registry handoff.
        self.general.ion_cannon_damage.hash(&mut hasher);
        self.general.spotlight.hash(&mut hasher);
        if self.ares {
            let extensions: Vec<_> = self
                .object_list
                .iter()
                .filter(|object| {
                    object.has_spotlight || object.spotlight_rules() != Default::default()
                })
                .map(|object| {
                    (
                        object.category,
                        &object.id,
                        object.has_spotlight,
                        object.spotlight_rules(),
                    )
                })
                .collect();
            if !extensions.is_empty() {
                b"ares-spotlight-type-v1".hash(&mut hasher);
                extensions.hash(&mut hasher);
            }
        }
        self.object_list
            .iter()
            .filter(|o| o.category == ObjectCategory::Building)
            .map(|o| (&o.id, o.has_spotlight))
            .collect::<Vec<_>>()
            .hash(&mut hasher);
        self.general.ion_blast_anim.hash(&mut hasher);
        self.general.ion_beam_anim.hash(&mut hasher);
        self.bridge_warheads.ion_cannon_name.hash(&mut hasher);
        // Earlier Rules passes retain these signed divisors even when the
        // current source stack omits them; sale crew and its RNG consumers
        // must not be presented as the same effective configuration.
        self.general.allied_survivor_divisor.hash(&mut hasher);
        self.general.soviet_survivor_divisor.hash(&mut hasher);
        self.general.third_survivor_divisor.hash(&mut hasher);
        self.general.maximum_cheer_rate.hash(&mut hasher);
        self.general.comp_easy_bonus.hash(&mut hasher);
        self.general.patrol_scan_minutes.hash(&mut hasher);
        if self.general.shroud_grow || self.general.shroud_rate != default_shroud_rate() {
            b"shroud-growth-controls-v1".hash(&mut hasher);
            self.general.shroud_grow.hash(&mut hasher);
            self.general.shroud_rate.hash(&mut hasher);
        }
        if self.general.fog_rate != default_fog_rate() {
            b"fog-regrowth-rate-v1".hash(&mut hasher);
            self.general.fog_rate.hash(&mut hasher);
        }
        // Earlier Process passes can retain these despite identical current text.
        if self.general.drop_zone_anim.is_some() || self.general.drop_zone_radius != 1024 {
            b"drop-zone-controls-v1".hash(&mut hasher);
            self.general.drop_zone_anim.hash(&mut hasher);
            self.general.drop_zone_radius.hash(&mut hasher);
        }
        // An earlier Process pass can retain Behind past an absent key.
        if self.general.behind.is_some() {
            b"behind-anim-v1".hash(&mut hasher);
            self.general.behind.hash(&mut hasher);
        }
        // CanBeHidden is read from ART (0x007121EB), outside the source INI
        // hash; it decides which objects construct the Behind anim.
        self.object_list
            .iter()
            .map(|o| (&o.id, o.can_be_hidden))
            .collect::<Vec<_>>()
            .hash(&mut hasher);
        // So is Palette= (0x0071582B); DestructionEffects gives a custom
        // palette type's DestroyAnim its scheme (0x00441D2F).
        self.object_list
            .iter()
            .filter(|o| o.custom_palette_name().is_some())
            .map(|o| (&o.id, o.custom_palette_name()))
            .collect::<Vec<_>>()
            .hash(&mut hasher);
        self.general.default_mirage_disguises.hash(&mut hasher);
        self.general.infantry_disguises.hash(&mut hasher);
        if self
            .general
            .secret_lab_choices
            .iter()
            .any(|list| !list.is_empty())
        {
            b"secret-production-choices-v1".hash(&mut hasher);
            self.general.secret_lab_choices.hash(&mut hasher);
        }
        for building in self
            .object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
        {
            if building.secret_lab() != &crate::rules::secret_lab::SecretLabRules::default() {
                b"secret-lab-type-v1".hash(&mut hasher);
                building.id.hash(&mut hasher);
                building.secret_lab().hash(&mut hasher);
            }
        }
        self.general.infantry_blink_disguise_time.hash(&mut hasher);
        let nominal_types = self
            .object_list
            .iter()
            .filter(|object| object.nominal())
            .map(|object| ((object.category, object.id.to_ascii_uppercase()), true))
            .collect::<BTreeMap<_, _>>();
        if !nominal_types.is_empty() {
            b"retained-type-nominal-v1".hash(&mut hasher);
            nominal_types.hash(&mut hasher);
        }
        // The native type reader retains these flags through missing keys
        // and later process passes. Equal current source text can therefore
        // yield different disguise admission, reveal and observer behavior.
        b"retained-type-disguise-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (
                        object.can_disguise,
                        object.perma_disguise,
                        object.detect_disguise,
                        object.disguise_when_still,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // The ordered Rules processor retains these references through
        // missing keys and later passes: each TechnoType's
        // (`TechnoTypeReferences`), ShrapnelWeapon +2B4, SuperWeapon
        // WeaponType +9C and AuxBuilding +C8, the WeaponType anims and
        // AttachedParticleSystem, ParticleType and VoxelAnimType references,
        // Warhead DebrisTypes, the AnimTypes' fixed-ART references and the
        // RulesClass Type pointers and lists. RulesClass also keeps the crate
        // and rocket blocks across Process calls, their scalars across
        // Full_Init's reset.
        // AircraftTypeSave41CE90/Load41CE20 retain this pair independently
        // of final source text. Native ComputeCRC41CDB0 omits it; this is
        // VERA's complete simulation configuration identity, not that CRC.
        b"aircraft-trailer-v1".hash(&mut hasher);
        Self::by_upper_id(
            self.object_list
                .iter()
                .filter(|object| object.category == ObjectCategory::Aircraft)
                .map(|object| (&object.id, object.aircraft_trailer())),
        )
        .hash(&mut hasher);
        b"retained-type-references-v3".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (
                        (
                            &object.image,
                            &object.death_weapon,
                            &object.deploys_into,
                            &object.occupy_weapon,
                            &object.elite_occupy_weapon,
                            &object.undeploys_into,
                            &object.powers_unit,
                            &object.free_unit,
                            &object.to_overlay,
                        ),
                        (
                            &object.dock,
                            &object.unloading_class,
                            &object.deploying_anim,
                            &object.enslaves,
                            &object.spawns,
                            &object.explosion_anims,
                            &object.destroy_anims,
                            &object.dead_bodies,
                        ),
                        (
                            &object.natural_particle_system,
                            &object.refinery_smoke_particle_system,
                            &object.damage_particle_systems,
                            &object.destroy_particle_systems,
                            &object.debris_types,
                            &object.debris_anims,
                        ),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        Self::by_upper_id(
            self.projectile_entries()
                .map(|(id, projectile)| (id, &projectile.shrapnel_weapon)),
        )
        .hash(&mut hasher);
        let airburst_weapons =
            Self::by_upper_id(self.projectile_entries().filter_map(|(id, projectile)| {
                projectile
                    .airburst_weapon
                    .as_ref()
                    .map(|weapon| (id, weapon))
            }));
        if !airburst_weapons.is_empty() {
            b"airburst-weapons-v1".hash(&mut hasher);
            airburst_weapons.hash(&mut hasher);
        }
        // A later invalid raw token retains BulletType+294. Equal flattened
        // INIs can therefore differ in the child-fan and parent-cluster gates.
        let airburst_flags = self
            .projectile_entries()
            .filter(|(_, projectile)| projectile.airburst)
            .map(|(id, _)| id.to_ascii_uppercase())
            .collect::<std::collections::BTreeSet<_>>();
        if !airburst_flags.is_empty() {
            b"airburst-retained-flag-v1".hash(&mut hasher);
            airburst_flags.hash(&mut hasher);
        }
        // Ares BulletTypeExt keys: solidity, trenches and launch scatter.
        if self.ares {
            let bullet_type_ext = self
                .projectile_entries()
                .filter(|(_, projectile)| {
                    projectile.bullet_type_ext
                        != crate::rules::ares_bullet_type::BulletTypeExtRules::default()
                })
                .map(|(id, projectile)| (id.to_ascii_uppercase(), &projectile.bullet_type_ext))
                .collect::<std::collections::BTreeMap<_, _>>();
            if !bullet_type_ext.is_empty() {
                b"ares-bullet-type-ext-v1".hash(&mut hasher);
                bullet_type_ext.hash(&mut hasher);
            }
        }
        // Phobos airburst/splits inputs. Defaults leave the stock hash
        // unchanged; any authored value must invalidate a save.
        #[cfg(feature = "phobos")]
        {
            let phobos_projectiles =
                Self::by_upper_id(self.projectile_entries().filter_map(|(id, projectile)| {
                    let value = &projectile.phobos_airburst;
                    (value != &crate::rules::phobos_airburst::PhobosAirburstType::default()).then(
                        || {
                            let fields = (
                                (
                                    value.splits,
                                    value.airburst_spread.to_bits(),
                                    value.retarget_accuracy.to_bits(),
                                    value.retarget_self,
                                    value.retarget_self_probability.to_bits(),
                                ),
                                (
                                    value.around_target,
                                    value.airburst_use_cluster,
                                    value.airburst_random_clusters,
                                    value.airburst_target_as_source,
                                    value.airburst_target_as_source_skip_height,
                                ),
                                (
                                    value.splits_targeting_distance,
                                    value.splits_targeting_distance_cylindrical,
                                    value.splits_allow_repeat_targets,
                                    value.splits_target_cell_range,
                                    value.splits_use_weapon_targeting,
                                    value.airburst_weapon_apply_firepower_mult,
                                    value.airburst_weapon_source_scatter_min,
                                    value.airburst_weapon_source_scatter_max,
                                    value.airburst_weapon_use_firing_effects,
                                    value.airburst_weapon_head_to_target,
                                    value.airburst_weapon_radial_fire_segments,
                                    value.aa_only,
                                ),
                            );
                            (id, fields)
                        },
                    )
                }));
            if !phobos_projectiles.is_empty() {
                b"phobos-airburst-splits-v1".hash(&mut hasher);
                phobos_projectiles.hash(&mut hasher);
            }
            if self.phobos_airburst != Default::default() {
                b"phobos-airburst-globals-v1".hash(&mut hasher);
                self.phobos_airburst.hash(&mut hasher);
            }
            let can_target_houses = Self::by_upper_id(
                self.weapon_entries()
                    .filter(|(_, weapon)| {
                        weapon.can_target_houses()
                            != crate::rules::phobos_shields::AffectedHouse::ALL
                    })
                    .map(|(id, weapon)| (id, weapon.can_target_houses().bits())),
            );
            if !can_target_houses.is_empty() {
                b"phobos-weapon-can-target-houses-v1".hash(&mut hasher);
                can_target_houses.hash(&mut hasher);
            }
            let weapon_targeting = Self::by_upper_id(
                self.weapon_entries()
                    .filter(|(_, weapon)| {
                        weapon.weapon_targeting()
                            != crate::rules::phobos_airburst::PhobosWeaponTargeting::default()
                    })
                    .map(|(id, weapon)| (id, weapon.weapon_targeting())),
            );
            if !weapon_targeting.is_empty() {
                b"phobos-weapon-targeting-v1".hash(&mut hasher);
                weapon_targeting.len().hash(&mut hasher);
                for (id, targeting) in &weapon_targeting {
                    id.hash(&mut hasher);
                    targeting.hash_state(&mut hasher);
                }
            }
        }
        Self::by_upper_id(self.super_weapons.iter().map(|(id, super_weapon)| {
            (id, (&super_weapon.weapon_type, &super_weapon.aux_building))
        }))
        .hash(&mut hasher);
        // Keep stock rules hashes unchanged. Extension payload references are
        // resolved state too: a layered clear/rebind must invalidate a save.
        let generic_warheads = Self::by_upper_id(
            self.super_weapons
                .iter()
                .filter_map(|(id, sw)| sw.generic_warhead.as_ref().map(|settings| (id, settings))),
        );
        if !generic_warheads.is_empty() {
            generic_warheads.hash(&mut hasher);
        }
        let hunters = Self::by_upper_id(
            self.super_weapons
                .iter()
                .filter_map(|(id, sw)| sw.hunter_seeker.as_ref().map(|settings| (id, settings))),
        );
        if !hunters.is_empty() {
            b"ares-hunter-seeker-v1".hash(&mut hasher);
            hunters.hash(&mut hasher);
            if self.ares {
                self.ares_global_controls
                    .hunter_seeker_buildings()
                    .hash(&mut hasher);
            }
        }
        // No extension tags means no extra hash input: preserve stock hashes.
        let building_requirements =
            Self::by_upper_id(self.super_weapons.iter().filter_map(|(id, sw)| {
                sw.building_requirements
                    .as_ref()
                    .map(|requirements| (id, requirements))
            }));
        if !building_requirements.is_empty() {
            "Ares.SuperWeaponBuildingRequirements".hash(&mut hasher);
            building_requirements.hash(&mut hasher);
        }
        let superweapon_money = Self::by_upper_id(
            self.super_weapons
                .iter()
                .filter(|(_, sw)| sw.money_amount != 0)
                .map(|(id, sw)| (id, (sw.money_amount, &sw.insufficient_funds_eva))),
        );
        if !superweapon_money.is_empty() {
            "Ares.SuperWeaponMoney".hash(&mut hasher);
            superweapon_money.hash(&mut hasher);
        }
        if self.ares {
            let default = crate::rules::superweapon_type::AresSuperWeaponPresentation::default();
            let presentation = Self::by_upper_id(
                self.super_weapons
                    .iter()
                    .filter(|(_, sw)| sw.presentation != default)
                    .map(|(id, sw)| (id, &sw.presentation)),
            );
            if !presentation.is_empty() {
                "Ares.SuperWeaponPresentation".hash(&mut hasher);
                presentation.hash(&mut hasher);
            }
        }
        let deliveries = Self::by_upper_id(
            self.super_weapons
                .iter()
                .filter_map(|(id, sw)| sw.unit_delivery.as_ref().map(|delivery| (id, delivery))),
        );
        if !deliveries.is_empty() {
            "Ares.UnitDelivery".hash(&mut hasher);
            deliveries.hash(&mut hasher);
        }
        Self::by_upper_id(self.weapon_entries().map(|(id, weapon)| {
            (
                id,
                (
                    &weapon.anim,
                    &weapon.occupant_anim,
                    &weapon.open_topped_anim,
                    &weapon.attached_particle_system,
                ),
            )
        }))
        .hash(&mut hasher);
        Self::by_upper_id(
            self.warhead_entries()
                .map(|(id, warhead)| (id, &warhead.debris_types)),
        )
        .hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            let jumpjet_overrides = Self::by_upper_id(
                self.warhead_entries()
                    .filter(|(_, warhead)| !warhead.jumpjet_override.is_empty())
                    .map(|(id, warhead)| (id, warhead.jumpjet_override)),
            );
            if !jumpjet_overrides.is_empty() {
                b"phobos-warhead-jumpjet-override-v1".hash(&mut hasher);
                jumpjet_overrides.len().hash(&mut hasher);
                for (id, overrides) in &jumpjet_overrides {
                    id.hash(&mut hasher);
                    overrides.hash_state(&mut hasher);
                }
            }
        }
        for particle in &self.particle_types {
            (&particle.name, &particle.warhead).hash(&mut hasher);
        }
        for voxel_anim in &self.voxel_anim_types {
            (
                &voxel_anim.name,
                &voxel_anim.spawns,
                &voxel_anim.bounce_anim,
                &voxel_anim.expire_anim,
                &voxel_anim.trailer_anim,
                &voxel_anim.warhead,
                &voxel_anim.attached_system,
            )
                .hash(&mut hasher);
        }
        self.anim_type_references.hash(&mut hasher);
        self.combat_damage.ammo_crate_damage.hash(&mut hasher);
        // Native retained defaults can differ with identical incoming INIs.
        self.combat_damage
            .tiberium_explosion_damage
            .hash(&mut hasher);
        for tiberium in self.tiberium_types.types() {
            (
                &tiberium.section,
                tiberium.color_scheme(),
                tiberium.debris(),
                tiberium.image(),
            )
                .hash(&mut hasher);
        }
        // Ares TiberiumExt and the TechnoTypeExt tiberium/harvester keys keep
        // retained reads. Defaults leave the stock hash unchanged.
        if self.ares {
            let tiberium_ext = self
                .tiberium_types
                .types()
                .iter()
                .filter(|tiberium| tiberium.ares() != &Default::default())
                .map(|tiberium| (&tiberium.section, tiberium.ares()))
                .collect::<Vec<_>>();
            let techno_tiberium = self
                .object_list
                .iter()
                .filter(|object| object.techno_combat().tiberium() != &Default::default())
                .map(|object| {
                    (
                        (object.category, object.id.to_ascii_uppercase()),
                        object.techno_combat().tiberium(),
                    )
                })
                .collect::<BTreeMap<_, _>>();
            if !tiberium_ext.is_empty() || !techno_tiberium.is_empty() {
                b"ares-tiberium-rules-v1".hash(&mut hasher);
                tiberium_ext.hash(&mut hasher);
                techno_tiberium.hash(&mut hasher);
            }
        }
        self.general.barrel_explode.hash(&mut hasher);
        self.general.barrel_debris.hash(&mut hasher);
        for kind in &self.voxel_anim_types {
            kind.damage.hash(&mut hasher);
        }
        self.combat_damage.flame_damage().hash(&mut hasher);
        (
            self.combat_damage.flame_damage2(),
            &self.bridge_warheads.c4_name,
            &self.bridge_warheads.crush_name,
            &self.bridge_warheads.ion_cannon_name,
            &self.combat_damage.ivan_warhead,
            &self.combat_damage.drain_animation_type,
            &self.mind_control.controlled_anim,
            &self.mind_control.perma_controlled_anim,
            &self.radiation.site_warhead,
            &self.general.mutate_warhead,
            &self.general.mutate_explosion_warhead,
            &self.general.empulse_warhead,
        )
            .hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            if !self.mind_control.mind_control_ignore_size
                || self.mind_control.mind_control_link_visible_to_house
                    != crate::rules::phobos_shields::AffectedHouse::ALL
                || self.mind_control.multi_mind_control_release_victim
            {
                b"phobos-mind-control-globals-v1".hash(&mut hasher);
                self.mind_control.mind_control_ignore_size.hash(&mut hasher);
                self.mind_control
                    .mind_control_link_visible_to_house
                    .bits()
                    .hash(&mut hasher);
                self.mind_control
                    .multi_mind_control_release_victim
                    .hash(&mut hasher);
            }
            let control_sizes = Self::by_upper_id(self.object_list.iter().filter_map(|object| {
                let value = (
                    object.mind_control_range_limit,
                    object.mind_control_ignore_size,
                    object.mind_control_size,
                    object
                        .mind_control_link_visible_to_house
                        .map(|house| house.bits()),
                    object.multi_mind_control_release_victim,
                );
                (value != (0, None, 1, None, None)).then_some((&object.id, value))
            }));
            if !control_sizes.is_empty() {
                b"phobos-mind-control-size-v1".hash(&mut hasher);
                control_sizes.hash(&mut hasher);
            }
        }
        let general = &self.general;
        if crate::rules::NPATCH_ENABLED {
            general.vehicle_parachute.hash(&mut hasher);
        }
        (
            (
                &general.ore_twinkle,
                &general.barrel_particle,
                &general.nuke_take_off,
                &general.wake.name,
                &general.warp_in.name,
                &general.warp_out.name,
                &general.warp_away.name,
                &general.chrono_sparkle1.name,
                &general.move_flash.name,
                &general.parachute_shp,
            ),
            (
                &general.dominator_warhead,
                &general.dominator_first_anim,
                &general.dominator_second_anim,
                &general.chrono_placement_anim,
                &general.chrono_blast_anim,
                &general.chrono_blast_dest_anim,
                &general.iron_curtain_invoke_anim,
                &general.force_shield_invoke_anim,
                &general.infantry_death_anims,
            ),
            (
                &general.engineer_infantry,
                &general.technician,
                &general.allied_crew,
                &general.soviet_crew,
                &general.third_crew,
            ),
        )
            .hash(&mut hasher);
        general
            .damage_fire_types
            .iter()
            .map(|anim| &anim.name)
            .collect::<Vec<_>>()
            .hash(&mut hasher);
        general.on_fire.hash(&mut hasher);
        (
            &general.dead_bodies,
            &general.base_unit_types,
            &general.pad_aircraft_types,
            &self.shipyard_types,
            &self.repair_bay_types,
            &self.harvester_unit_types,
        )
            .hash(&mut hasher);
        (
            (
                &self.build_const_types,
                &self.build_power_types,
                &self.build_refinery_types,
                &self.build_barracks_types,
                &self.build_tech_types,
                &self.build_weapons_types,
            ),
            (
                &self.build_radar_types,
                &self.allied_base_defense_types,
                &self.soviet_base_defense_types,
                &self.third_base_defense_types,
                &self.concrete_wall_types,
                &self.ns_gate_types,
                &self.ew_gate_types,
            ),
        )
            .hash(&mut hasher);
        self.crate_rules.hash(&mut hasher);
        self.missile_spawn.hash(&mut hasher);
        let damage = &self.combat_damage;
        (
            &damage.default_large_grey_smoke_system,
            &damage.default_small_grey_smoke_system,
            &damage.default_spark_system,
            &damage.default_large_red_smoke_system,
            &damage.default_small_red_smoke_system,
            &damage.default_debris_smoke_system,
            &damage.default_fire_stream_system,
            &damage.default_test_particle_system,
            &damage.default_repair_particle_system,
        )
            .hash(&mut hasher);
        self.general.missile_rot_var.to_bits().hash(&mut hasher);
        self.general.safety_altitude.hash(&mut hasher);
        self.general.line_trail_color_override.hash(&mut hasher);
        self.general.chrono_beam_color.hash(&mut hasher);
        self.general.lightning_warhead.hash(&mut hasher);
        self.general.weather_con_bolt_explosion.hash(&mut hasher);
        self.general.weapon_nullify_anim.hash(&mut hasher);
        // Only while on, so a game without the queue hashes as before it.
        if self.general.building_production_queue {
            b"building-production-queue".hash(&mut hasher);
        }
        if self.phobos {
            b"phobos-ai-dehardcode-v1".hash(&mut hasher);
            self.general.ai_fire_sale.hash(&mut hasher);
            self.general.ai_fire_sale_delay.hash(&mut hasher);
            self.general.ai_all_to_hunt.hash(&mut hasher);
            self.general.gather_when_mcv_deploy.hash(&mut hasher);
            b"phobos-power-surplus-v1".hash(&mut hasher);
            self.general.enable_power_surplus.hash(&mut hasher);
            self.general.power_surplus.hash(&mut hasher);
            self.general
                .power_surplus_scale_to_drain_amount
                .hash(&mut hasher);
        }
        self.combat_damage.splash_list.hash(&mut hasher);
        self.combat_damage.death_weapon.hash(&mut hasher);
        // The live family order is independent of HashMap iteration and may
        // survive earlier passes absent from the current source stack.
        self.weapon_array.hash(&mut hasher);
        Self::by_upper_id(self.warhead_entries().map(|(name, wh)| {
            (
                name,
                (
                    wh.conventional,
                    wh.em_effect,
                    wh.sparky,
                    &wh.anim_list,
                    wh.verses_f64.map(f64::to_bits),
                ),
            )
        }))
        .hash(&mut hasher);
        #[cfg(feature = "phobos")]
        {
            let warhead_anims =
                Self::by_upper_id(self.warhead_entries().filter_map(|(name, warhead)| {
                    (warhead.phobos_anim
                        != crate::rules::phobos_warhead_anim::PhobosWarheadAnimRules::default())
                    .then_some((name, &warhead.phobos_anim))
                }));
            if !warhead_anims.is_empty() {
                b"phobos-warhead-anim-v1".hash(&mut hasher);
                warhead_anims.hash(&mut hasher);
            }
            if self.phobos_warhead_anim != Default::default() {
                b"phobos-warhead-anim-globals-v1".hash(&mut hasher);
                self.phobos_warhead_anim.hash(&mut hasher);
            }
        }
        #[cfg(feature = "phobos")]
        {
            let control_anims =
                Self::by_upper_id(self.warhead_entries().filter_map(|(name, wh)| {
                    wh.mind_control_anim
                        .as_ref()
                        .map(|anim| (name, anim.to_ascii_uppercase()))
                }));
            if !control_anims.is_empty() {
                b"phobos-warhead-mind-control-anim-v1".hash(&mut hasher);
                control_anims.hash(&mut hasher);
            }
        }
        Self::by_upper_id(self.weapon_entries().map(|(id, weapon)| {
            (
                id,
                (
                    (
                        weapon.speed,
                        &weapon.projectile,
                        weapon.damage,
                        &weapon.warhead,
                        weapon.range_leptons,
                    ),
                    (
                        weapon.is_laser,
                        weapon.is_house_color,
                        weapon.is_big_laser,
                        weapon.laser_duration,
                        weapon.laser_inner_color,
                        weapon.laser_outer_color,
                        weapon.laser_outer_spread,
                    ),
                ),
            )
        }))
        .hash(&mut hasher);
        // Selection order and duplicate references affect the scenario RNG's
        // consumers, including the truncated retail pool's unread AnimType D.
        self.general.metallic_debris.hash(&mut hasher);
        self.general.weather_con_clouds.hash(&mut hasher);
        self.general.weather_con_bolts.hash(&mut hasher);
        self.bridge_rules.explosions.hash(&mut hasher);
        self.animation_sequences.hash(&mut hasher);
        self.unit_shp_read_states.hash(&mut hasher);
        self.effect_assets.hash(&mut hasher);
        self.voxel_assets.hash(&mut hasher);
        self.voxel_model_plan.hash(&mut hasher);
        self.buildup_assets.hash(&mut hasher);
        self.rubble_assets.hash(&mut hasher);
        self.terrain_assets.hash(&mut hasher);
        b"art-smudge-config-v2".hash(&mut hasher);
        self.art_registry.fold_smudge_anim_dims(&mut hasher);
        // These effective Bullet values feed FireAt and drawing after load. Hash
        // consumed values in canonical type order, not ART insertion order,
        // authored-key presence, or unrelated presentation metadata.
        b"art-projectile-config-v4".hash(&mut hasher);
        Self::by_upper_id(self.projectile_entries().map(|(id, p)| {
            (
                id,
                (
                    (
                        &p.image,
                        &p.image_loads,
                        p.voxel_image_load(),
                        p.voxel,
                        p.theater,
                        p.new_theater,
                        p.inviso,
                    ),
                    (p.rotates, p.flat, p.anim_palette, p.color_scheme()),
                    (
                        p.use_line_trail,
                        p.line_trail_color,
                        p.line_trail_color_decrement,
                    ),
                    (
                        p.anim_low,
                        p.anim_high,
                        p.anim_rate,
                        p.spawn_delay,
                        &p.trailer,
                    ),
                ),
            )
        }))
        .hash(&mut hasher);
        // The fixed SOUNDMD registry changes which names resolve even when
        // Rules text is identical. Empty versus nonempty vectors gates Main
        // draws; list order/duplicates select the sound. Hash effective lists
        // in canonical type order, independently of registry insertion order.
        b"resolved-type-sound-lists-v4".hash(&mut hasher);
        if self.ares {
            let promotion_sounds = self
                .object_list
                .iter()
                .filter_map(|object| {
                    let sounds = (
                        object.promotion_sound_override(false),
                        object.promotion_sound_override(true),
                    );
                    (sounds.0.is_some() || sounds.1.is_some())
                        .then(|| ((object.category, object.id.to_ascii_uppercase()), sounds))
                })
                .collect::<BTreeMap<_, _>>();
            if !promotion_sounds.is_empty() {
                b"ares-type-promotion-sounds-v1".hash(&mut hasher);
                promotion_sounds.hash(&mut hasher);
            }
        }
        self.general.voice_ifv_repair().hash(&mut hasher);
        self.general.upgrade_veteran_sound.hash(&mut hasher);
        self.general.upgrade_elite_sound.hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (
                        &object.voice_select,
                        &object.voice_select_enslaved,
                        object.voice_move(),
                        object.voice_enter(),
                        object.voice_capture(),
                        object.voice_harvest(),
                        &object.move_sound,
                        &object.voice_special_attack,
                        &object.voice_feedback,
                        object.voice_attack(),
                        std::array::from_fn::<_, 4, _>(|slot| {
                            object.voice_attack_override((slot / 2) as i32, slot % 2 != 0)
                        }),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| {
                (
                    object.id.to_ascii_uppercase(),
                    self.building_launch_height(object),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Elite Airstrike fallbacks depend on reached pass boundaries, not
        // merely merged text; managers copy these values into gameplay state.
        b"airstrike-settings-v1".hash(&mut hasher);
        self.general.airstrike_abort_sound.hash(&mut hasher);
        self.general.airstrike_attack_voice.hash(&mut hasher);
        self.general.laser_target_color.hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    &object.airstrike,
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Radial launch settings likewise survive earlier reached passes even
        // when the current source stack omits their key.
        b"radial-fire-segments-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    object.radial_fire_segments,
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // Reached-pass barrel resets are not derivable from final merged text.
        b"voxel-recoil-config-v1".hash(&mut hasher);
        for object in &self.object_list {
            object.recoil.hash(&mut hasher);
        }
        // The same current source stack can retain different FV mappings from
        // earlier Process calls. Gunner initialization and passenger entry use
        // these fields, so restore must compare the effective owner state.
        b"gunner-turret-config-v1".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                (
                    (object.category, object.id.to_ascii_uppercase()),
                    (
                        object.gunner,
                        object.ifv_mode,
                        object.turret_count,
                        &object.gunner_turrets,
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        // GetFLH consumes the retained type records and the existing pivot.
        b"art-weapon-flh-v2".hash(&mut hasher);
        self.object_list
            .iter()
            .map(|object| {
                let art = self
                    .art_registry
                    .get(&object.image)
                    .or_else(|| self.art_registry.get(&object.id));
                let slots = (0..object.weapon_record_count())
                    .map(|index| {
                        let elite = object
                            .elite_weapon_list
                            .get(index)
                            .is_some_and(Option::is_some);
                        (
                            object.weapon_flh(index as i32, false),
                            object.weapon_flh(index as i32, elite),
                        )
                    })
                    .collect::<Vec<_>>();
                let ports = object.open_topped.then(|| {
                    (0..5)
                        .map(|port| object.alternate_fire_flh(port))
                        .collect::<Vec<_>>()
                });
                let consumed = (art.map_or(0, |art| art.turret_offset), slots, ports);
                (object.id.to_ascii_uppercase(), consumed)
            })
            .collect::<BTreeMap<_, _>>()
            .hash(&mut hasher);
        self.hash_building_body_config(&mut hasher);
        self.hash_building_slot_config(&mut hasher);
        hasher.finish()
    }

    /// VERA: what the simulation takes from the contents of picture and
    /// model files, in one number that a network match's start certifies
    /// (`net::live_commands::StartupAgreement::new`). No room
    /// compares those files, so two games whose rules agree could still time
    /// a building's unfolding, an animation's end or a shot's origin
    /// differently and leave step in mid-match; with this they do not start.
    ///
    /// It folds only what a simulation reader takes from a file: the Buildup
    /// SHP's frame count behind the construction and sale controls, a
    /// resource spawner's frame count, which LeaveRubble images are long
    /// enough, which main voxel models loaded with their first limb's half
    /// sizes, the frame count, bounds and middle frame of each animation that
    /// affects play or that a simulation owner waits on, the first weather
    /// bolt's image height, whether a scorching or cratering animation's
    /// frame is large, and in an Ares game which particle AlphaImage SHPs
    /// loaded. A file that changes only how something looks (other pixels,
    /// another palette, a model's other parts, the file that supplied an
    /// image, the frames of an effect animation that changes nothing but its
    /// picture) leaves the number as it is, so players with such files play
    /// together. Values the INI text decides ride along where they share a
    /// fold; the rules digest compares them already.
    ///
    /// RESIDUAL: of the picture-only animations a simulation owner waits on,
    /// only a unit's `DeployingAnim=`, the Psychic Dominator's two and the
    /// Lightning Storm's clouds are folded. Another one (a building's
    /// delayed-fire animation, an Ares superweapon's own lists, with its
    /// `Bolts=` image height) with other frames in two games still takes
    /// their battles apart; a mod's files are compared with the mod.
    pub fn file_inputs_hash(&self) -> u64 {
        let mut hasher = crate::util::state_hasher::StateHasher::new();
        b"rules-file-inputs-v1".hash(&mut hasher);
        // Construction and sale timing (`world_spawn::construction`,
        // `production_sell`), TIBTRE timing (`terrain_spawn`) and the rubble
        // a destroyed building leaves (`building_after_destruction`).
        self.buildup_assets.hash(&mut hasher);
        self.terrain_assets.hash(&mut hasher);
        self.rubble_assets.fold_admissions(&mut hasher);
        // IsVoxel and the tilt matrix behind a shot's origin
        // (`combat::area_rocker`, `combat::fire_coord`).
        self.voxel_assets.fold_loaded(&mut hasher);
        // The length and middle frame of each anim that affects play
        // (`anim_class`), and of the ones that pace a deployment
        // (`unit_simple_deploy`), the Psychic Dominator and the Lightning
        // Storm (`Simulation::anim_stage_and_frames`).
        self.art_registry
            .fold_anim_frame_bounds(self.paced_anims(), &mut hasher);
        // `LightningStorm::CreateCloudBolt` (`0x0053A16B`) lifts its clouds
        // by half the first bolt image's height.
        self.general
            .weather_con_bolts
            .first()
            .map(|bolt| {
                self.art_registry
                    .anim_runtime_config(bolt)
                    .and_then(|config| config.raw_shp_height)
            })
            .hash(&mut hasher);
        // The smudge a scorching or cratering anim leaves (`anim_class::ai`).
        self.art_registry.fold_smudge_frame_classes(&mut hasher);
        // Ares picks its own particle simulation for a system whose held
        // type has no loaded AlphaImage (`particles::ares_system`). Without
        // Ares an alpha image only lights and draws.
        if self.ares {
            b"ares-particle-alpha-images-v1".hash(&mut hasher);
            self.particle_types_iter()
                .filter(|particle| self.alpha_images.loaded(&particle.alpha_image))
                .map(|particle| particle.alpha_image.as_str())
                .collect::<std::collections::BTreeSet<_>>()
                .hash(&mut hasher);
        }
        hasher.finish()
    }

    /// The picture-only AnimTypes a simulation owner waits on, which
    /// [`Self::file_inputs_hash`] folds with the ones that affect play: each
    /// unit's `DeployingAnim=`, the Psychic Dominator's two and the Lightning
    /// Storm's clouds.
    fn paced_anims(&self) -> impl Iterator<Item = &str> {
        self.object_list
            .iter()
            .filter_map(|object| object.deploying_anim.as_deref())
            .chain([
                self.general.dominator_first_anim.as_str(),
                self.general.dominator_second_anim.as_str(),
            ])
            .chain(self.general.weather_con_clouds.iter().map(String::as_str))
    }

    /// VERA: the game-folder files (a loose file's name, else an archive's,
    /// `retail_sources::game_folder_file`) that change what
    /// [`Self::file_inputs_hash`] folds, in name order: each supplies a
    /// picture or model whose folded value is not the one the copy a retail
    /// install reads gives, or one a retail install lacks. Empty for a game
    /// whose folder changes none, a graphics pack that only redraws
    /// included. Each binder notes them as it reads
    /// (`retail_sources::FolderFiles`), and a refused start names them
    /// (`agreement::start_changes_text`).
    pub fn file_input_folder_files(&self) -> Vec<String> {
        let weather_bolt = self.general.weather_con_bolts.first().map(String::as_str);
        let mut files: std::collections::BTreeSet<&str> = self
            .art_registry
            .anim_folder_files(self.paced_anims(), weather_bolt);
        files.extend(self.buildup_assets.folder_files());
        files.extend(self.terrain_assets.folder_files());
        files.extend(self.rubble_assets.folder_files());
        files.extend(self.voxel_assets.folder_files());
        if self.ares {
            files.extend(
                self.particle_types_iter()
                    .filter_map(|particle| self.alpha_images.folder_file(&particle.alpha_image)),
            );
        }
        files.into_iter().map(str::to_owned).collect()
    }

    /// Slot constructors consume selected names, offsets and the common runtime
    /// metadata. Canonical ordering makes ART section insertion order irrelevant.
    fn hash_building_slot_config(&self, hasher: &mut impl Hasher) {
        b"building-slot-config-v1".hash(hasher);
        self.anim_type_names.hash(hasher);
        self.anim_type_art_read_states.hash(hasher);
        self.anim_type_images.hash(hasher);
        self.art_registry.scheduler_anim_types().hash(hasher);
        let mut types = self.anim_type_names.clone();
        types.extend(self.art_registry.scheduler_anim_types().iter().cloned());
        for name in types {
            name.hash(hasher);
            self.art_registry.anim_runtime_config(&name).hash(hasher);
        }
        let buildings = self
            .object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| (object.id.to_ascii_uppercase(), object))
            .collect::<BTreeMap<_, _>>();
        for (name, object) in buildings {
            name.hash(hasher);
            object.powered.hash(hasher);
            object.powered_special.hash(hasher);
            let entry = self
                .art_registry
                .resolve_metadata_entry(&object.id, &object.image);
            entry
                .map(|e| e.building_anim_power)
                .unwrap_or([Default::default(); 21])
                .hash(hasher);
            entry.is_some_and(|e| e.is_anim_delayed_fire).hash(hasher);
            entry.is_some_and(|e| e.silo_damage).hash(hasher);
            let mut slots = self
                .art_registry
                .resolve_metadata_entry(&object.id, &object.image)
                .map(|entry| {
                    entry
                        .building_anims
                        .iter()
                        //0..2 are immutable asset declarations here. The
                        // consumed PowerUp records live on BuildingUpgradeRules
                        // and are hashed once with that retained Type state.
                        .filter(|config| config.native_slot >= 3)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            slots.sort_by_key(|config| config.native_slot);
            slots.hash(hasher);
        }
    }

    /// Effective body inputs used by the receiver's native43EF90 frame guard.
    /// Parser provenance: tools/spatial_oracle/building_body_rules.json.
    fn hash_building_body_config(&self, hasher: &mut impl Hasher) {
        b"building-body-config-v1".hash(hasher);
        self.object_list
            .iter()
            .filter(|object| object.category == ObjectCategory::Building)
            .map(|object| {
                let art = self
                    .art_registry
                    .resolve_metadata_entry(&object.id, &object.image);
                (
                    object.id.to_ascii_uppercase(),
                    (
                        object.firestorm_wall,
                        art.map_or(9, |entry| entry.building_gate_stages),
                        art.map_or([[0, 1, 0]; 4], |entry| entry.building_body_ranges),
                    ),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .hash(hasher);
    }
}
