//! GPU-independent SHP frame counts used by authoritative match systems.
//!
//! The renderer may consume this catalog when deciding which effect frames to
//! make resident, but it is not the source of the counts. Binding happens from
//! merged rules/ART plus the active theater's asset search path, so graphical
//! and headless matches receive the same immutable inputs.

use std::collections::{BTreeMap, BTreeSet};

use crate::assets::asset_manager::AssetManager;
use crate::assets::shp_file::ShpFile;
use crate::rules::art_data;
use crate::rules::ruleset::RuleSet;

/// Raw and consumer-visible frame counts for one authoritative effect asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectAssetFrameCounts {
    raw: u16,
    available: u16,
}

impl EffectAssetFrameCounts {
    /// Literal unsigned frame count declared at SHP header offset `+6`.
    pub fn raw(self) -> u16 {
        self.raw
    }

    /// Existing effect-visible count after the AnimType body/shadow split.
    pub fn available(self) -> u16 {
        self.available
    }
}

/// Deterministic match input derived from particle image SHPs.
///
/// Keys are trimmed, uppercase asset IDs. A `BTreeMap` is deliberate: binding
/// and hashing must not inherit `HashMap`'s process-random iteration order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct EffectAssetCatalog {
    entries: BTreeMap<String, EffectAssetFrameCounts>,
}

impl EffectAssetCatalog {
    /// Bind every particle image root.
    ///
    /// Missing or malformed assets are optional at this layer: the entry is
    /// omitted after a warning and each simulation consumer retains its
    /// established local fallback. Scheduler-owned terrain/damage-fire
    /// animations remain the responsibility of `bind_scheduler_anim_assets`,
    /// whose required-asset failure semantics are intentionally stricter.
    pub fn bind(
        rules: &RuleSet,
        asset_manager: &AssetManager,
        theater_ext: &str,
        theater_name: &str,
    ) -> Self {
        let mut catalog = Self::default();

        for name in authoritative_effect_roots(rules) {
            let image_id = rules.art().resolve_effective_image_id(&name, &name);
            let candidates = art_data::anim_shp_candidates(
                Some(rules.art()),
                &name,
                &image_id,
                theater_ext,
                theater_name,
            );
            let Some((file_name, data)) = candidates.iter().find_map(|candidate| {
                asset_manager
                    .get_ref(candidate)
                    .map(|data| (candidate, data))
            }) else {
                log::warn!(
                    "Authoritative effect asset [{name}] is missing; consumers will use their fallback"
                );
                continue;
            };

            let raw = match ShpFile::frame_count_from_bytes(data) {
                Ok(count) => count,
                Err(error) => {
                    log::warn!(
                        "Authoritative effect asset [{name}] ({file_name}) has an invalid SHP header: {error}; consumers will use their fallback"
                    );
                    continue;
                }
            };
            let scheduler_owned = rules.art().scheduler_anim_types().contains(&name);
            let shadow = rules
                .art()
                .anim_runtime_config(&name)
                .is_some_and(|config| config.shadow);
            let available = available_effect_anim_frame_count(raw, scheduler_owned, shadow);
            catalog.insert(name, raw, available);
        }

        catalog
    }

    /// Consumer-visible frame count (particle image timing).
    pub fn effect_frame_count(&self, name: &str) -> Option<u16> {
        self.entry(name).map(|counts| counts.available)
    }

    /// Literal unsigned SHP header frame count.
    ///
    /// Particle animation-state parity can consume this independently of the
    /// AnimType body/shadow split. Native behavior for a modded particle image
    /// carrying `Shadow=yes` remains UNCHECKED; retaining both values prevents
    /// the catalog from baking that policy decision into asset binding.
    #[cfg(test)]
    pub fn raw_frame_count(&self, name: &str) -> Option<u16> {
        self.entry(name).map(|counts| counts.raw)
    }

    /// Entries in canonical asset-name order.
    #[cfg(test)]
    fn iter(&self) -> impl Iterator<Item = (&str, EffectAssetFrameCounts)> {
        self.entries
            .iter()
            .map(|(name, counts)| (name.as_str(), *counts))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn entry(&self, name: &str) -> Option<&EffectAssetFrameCounts> {
        canonical_asset_id(name).and_then(|name| self.entries.get(&name))
    }

    fn insert(&mut self, name: String, raw: u16, available: u16) {
        let Some(name) = canonical_asset_id(&name) else {
            return;
        };
        self.entries
            .insert(name, EffectAssetFrameCounts { raw, available });
    }

    #[cfg(test)]
    pub(crate) fn set_for_test(&mut self, name: &str, raw: u16, available: u16) {
        self.insert(name.to_string(), raw, available);
    }
}

/// Number of SHP frames visible to the frame-count consumer.
///
/// gamemd's `AnimTypeClass` INI load at `0x00427D00` calls the SHP loader at
/// `0x00427B50`, which seeds `End` from the signed SHP header count and halves
/// it for `Shadow=yes`. Scheduler-owned types retain the raw range here because
/// their already-bound runtime metadata owns the exact body/shadow bounds. This
/// is the behavior previously embedded in `SpriteAtlas`.
pub fn available_effect_anim_frame_count(
    raw_count: u16,
    scheduler_owned: bool,
    shadow: bool,
) -> u16 {
    if scheduler_owned || !shadow {
        return raw_count;
    }

    let body_count = raw_count / 2;
    if body_count > 0 {
        body_count
    } else {
        raw_count
    }
}

/// Animation types `Simulation`'s cliff-collapse producer names literally.
pub(crate) const CLIFF_COLLAPSE_ANIMS: [&str; 3] = ["XGRYMED1", "XGRYMED2", "XGRYSML1"];

/// The smoke a burning or crashing aircraft trails: `AircraftClass::AI` finds
/// it by this literal (`0x004150EF..0x00415102`), no rules key names it.
pub(crate) const AIRCRAFT_SMOKE_ANIM: &str = "SGRYSMK1";

/// The launch puffs and trail a missile's Rocket locomotor constructs:
/// `RocketLocomotionClass::Process @ 0x006622C0` finds them by
/// `AnimTypeClass::FindIndex @ 0x00427CB0` on the literals `[0x008399AC]`
/// (`0x008399BC`) and `[0x008399B0]` (`0x008399B4`); no rules key names them.
pub(crate) const ROCKET_TAKEOFF_ANIM: &str = "V3TAKOFF";
pub(crate) const ROCKET_TRAIL_ANIM: &str = "V3TRAIL";

/// The fireball a `NUKE` warhead's bullet waits on: `BulletClass::AI` finds
/// it by `AnimTypeClass::FindIndex @ 0x00427CB0` on the literal
/// `0x0081AF8C` (`0x00467EB1`); no rules key names it.
pub(crate) const NUKE_BALL_ANIM: &str = "NUKEBALL";

/// Every animation name the simulation can turn into an `AnimClass` instance,
/// which the loader must bind before the match starts.
///
/// Combat fills `CombatResult::explosion_effects` from three producers:
/// - the killing warhead's `AnimList=` pick
///   (`WarheadTypeClass::Detonate` -> `Warhead::SelectExplosionAnim @ 0x0048A4F0`),
/// - the infantry death animation for the warhead's `InfDeath=`,
/// - the dying object's own `Explosion=` / `DestroyAnim=` picks
///   (`UnitClass::Death_Explosion @ 0x00738680`,
///   `BuildingClass::DestructionEffects @ 0x004415F0`, and the Aircraft death
///   arm at `0x0041661F`, which picks `Explosion=` only).
///
/// The other roots (muzzle flashes, warps, superweapon and bridge animations,
/// ore twinkle, cliff collapse) are named at their inserts below.
///
/// The list is derived from loaded rules, never hand-written, and retail
/// authors several names with no art section or sprite, which is why the
/// binder that consumes it is the tolerant one.
pub fn anim_class_roots(rules: &RuleSet) -> Vec<String> {
    let mut roots = BTreeSet::new();
    let mut insert = |name: &str| {
        // Type factory references are literal IDs: comma tokens may contain
        // spaces. Particle asset filenames use their own canonicalizer below.
        if !name.is_empty() {
            roots.insert(name.to_ascii_uppercase());
        }
    };
    for warhead in rules.warheads_iter() {
        for name in &warhead.anim_list {
            insert(name);
        }
    }
    // Muzzle animations `TechnoClass::Fire_At` constructs
    // (`sim::world::damage_consequences`, `combat::fire_coord`).
    for weapon in rules.weapons_iter() {
        for name in weapon.anim.iter().chain(&weapon.occupant_anim) {
            insert(name);
        }
    }
    // Bullet AI466826..4668B8 constructs the retained ART Trailer before
    // moving the projectile. Its ordinary AnimClass needs the same binding
    // as every other producer, including in headless matches.
    for projectile in rules.projectiles_iter() {
        if let Some(name) = projectile.trailer.as_deref() {
            insert(name);
        }
    }
    for name in rules.general.infantry_death_anims.iter().flatten() {
        insert(name);
    }
    // Death debris chunks (`TechnoClass::ReceiveDamage 0x00702281`): a
    // type's own `DebrisAnims=` or `[General] MetallicDebris=`.
    for object in rules.all_objects() {
        for name in object
            .explosion_anims
            .iter()
            .chain(&object.destroy_anims)
            .chain(&object.debris_anims)
            .chain(&object.deploying_anim)
        {
            insert(name);
        }
    }
    for name in &rules.general.metallic_debris {
        insert(name);
    }
    // A chunk landing in water below the deck: `Wake=` and the first
    // `SplashList=` entry (`AnimClass::AI 0x00423D46..0x00423DDD`).
    insert(&rules.general.wake.name);
    for name in &rules.combat_damage.splash_list {
        insert(name);
    }
    // SelectAnim48A594 returns this retained AnimType for LightningWarhead.
    insert(&rules.general.weather_con_bolt_explosion);
    // Bullet46A2A1 substitutes this after an Iron Curtain area receipt2.
    insert(&rules.general.weapon_nullify_anim);
    // `[General] Parachute=`: the canopy `ObjectClass::Paradrop` attaches to a
    // dropped object (`sim::movement::parachute_descent`).
    if let Some(name) = rules.general.parachute_shp.as_deref() {
        insert(name);
    }
    // `[General] WarpOut=`: the teleport locomotor constructs it at both ends
    // of a relocation (`sim::movement::teleport_movement`).
    insert(&rules.general.warp_out.name);
    // `SuperClass::Launch` invoke animations (`sim::superweapon`).
    insert(&rules.general.iron_curtain_invoke_anim);
    insert(&rules.general.force_shield_invoke_anim);
    insert(&rules.general.ion_blast_anim);
    // The Chronosphere's source loop and both Chrono Warp blasts
    // (`sim::superweapon::chronosphere`).
    insert(&rules.general.chrono_placement_anim);
    insert(&rules.general.chrono_blast_anim);
    insert(&rules.general.chrono_blast_dest_anim);
    // The Psychic Dominator's two anims (`PsyDom::Start @ 0x0053AE50`,
    // `PsyDom::MindControlArea @ 0x0053B080`, `sim::superweapon::
    // psychic_dominator`): without them the strike follows no anim and lands
    // at once.
    insert(&rules.general.dominator_first_anim);
    insert(&rules.general.dominator_second_anim);
    // `[CombatDamage] ControlledAnimationType=` and
    // `PermaControlledAnimationType=`: the rings a captured or Dominated
    // object wears (`sim::capture_manager`).
    for name in [
        &rules.mind_control.controlled_anim,
        &rules.mind_control.perma_controlled_anim,
    ]
    .into_iter()
    .flatten()
    {
        insert(name);
    }
    // `[General] WeatherConClouds=` and `WeatherConBolts=`: the Lightning
    // Storm's clouds and bolts (`sim::superweapon::lightning_storm`).
    for name in rules
        .general
        .weather_con_clouds
        .iter()
        .chain(&rules.general.weather_con_bolts)
    {
        insert(name);
    }
    // `[General] BridgeExplosions=`: `CellClass::BlowUpBridge` and the
    // `CollapseBridge_*` walkers construct them (`world::bridge_orchestrator`).
    // Stock lists the same four types in `[TankOGas] AnimList=`, but nothing
    // ties the two lists together.
    for name in &rules.bridge_rules.explosions {
        insert(name);
    }
    // `[General] OreTwinkle=`: the post-`Full_Init` twinkle tail
    // (`sim::ore_twinkle`). No other list names it, so without this root every
    // twinkle failed to construct.
    if let Some(name) = rules.general.ore_twinkle.as_deref() {
        insert(name);
    }
    // `[Powerups]` token 2: the pickup anim `CellClass::PickupCrate @
    // 0x00481A00` constructs 200 leptons above the crate (`0x00483384`,
    // `sim::crates::pickup`). No warhead list names MONEY, HEALALL, ARMOR,
    // FIREPOWR, SPEED, VETERAN or REVEAL. A row's absent default (`NONE`)
    // and a mod's unregistered name resolve to native's `-1`: no root.
    for name in rules.powerups.anims.iter().flatten() {
        if rules.anim_type_names.contains(name) {
            insert(name);
        }
    }
    // The cliff-collapse debris types are literals in the producer
    // (`sim::world`); stock binds them only through warhead `AnimList=`.
    for name in CLIFF_COLLAPSE_ANIMS {
        insert(name);
    }
    // The aircraft smoke (`sim::world::crash`): without this root SGRYSMK1
    // never bound and every puff failed to construct.
    insert(AIRCRAFT_SMOKE_ANIM);
    // The rocket's puffs and trail (`sim::movement::rocket_movement`): without
    // these roots neither ever constructed.
    insert(ROCKET_TAKEOFF_ANIM);
    insert(ROCKET_TRAIL_ANIM);
    // The nuke's buildup (`sim::superweapon::nuke`): without this root the
    // falling missile waited on no anim and struck one frame after landing.
    insert(NUKE_BALL_ANIM);
    roots.into_iter().collect()
}

/// Report how many `anim_class_roots` the tolerant binder could not bind.
///
/// The binder already emits a per-name `warn!`, but a per-name line is invisible
/// in aggregate unless the count is stated once. Retail's standing count on a
/// TEMPERATE map is 35, all building animation names: 28 belong to art sections
/// no `[BuildingTypes]` entry uses, 7 to real buildings whose files no archive
/// holds. A data change shows up as a different number.
pub fn log_unbound_combat_explosion_roots(unbound: usize) {
    if unbound == 0 {
        return;
    }
    log::info!("{unbound} AnimClass animation root(s) did not bind and will draw nothing");
}

fn authoritative_effect_roots(rules: &RuleSet) -> BTreeSet<String> {
    let mut roots = BTreeSet::new();
    let mut insert = |name: &str| {
        if let Some(name) = canonical_asset_id(name) {
            roots.insert(name);
        }
    };

    // Particle images are the only frame counts a consumer asks for
    // (`sim::particles::system_ai`). Every animation producer constructs an
    // `AnimClass`, whose frame bounds come from `bind_anim_class_assets`.
    for particle in rules.particle_types_iter() {
        if let Some(name) = particle.image.as_deref() {
            insert(name);
        }
    }

    roots
}

fn canonical_asset_id(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use std::hash::{Hash, Hasher};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn available_count_preserves_existing_shadow_and_scheduler_semantics() {
        assert_eq!(available_effect_anim_frame_count(21, false, false), 21);
        assert_eq!(available_effect_anim_frame_count(20, false, true), 10);
        assert_eq!(available_effect_anim_frame_count(21, false, true), 10);
        assert_eq!(available_effect_anim_frame_count(20, true, true), 20);
        assert_eq!(available_effect_anim_frame_count(1, false, true), 1);
        assert_eq!(available_effect_anim_frame_count(0, false, true), 0);
    }

    #[test]
    fn catalog_keys_are_canonical_and_hash_independent_of_insertion_order() {
        let mut first = EffectAssetCatalog::default();
        first.set_for_test(" wake1 ", 21, 10);
        first.set_for_test("warPout", 16, 16);

        let mut second = EffectAssetCatalog::default();
        second.set_for_test("WARPOUT", 16, 16);
        second.set_for_test("WAKE1", 21, 10);

        assert_eq!(first, second);
        assert_eq!(first.raw_frame_count("Wake1"), Some(21));
        assert_eq!(first.effect_frame_count(" wake1 "), Some(10));
        assert_eq!(catalog_hash(&first), catalog_hash(&second));
        assert_eq!(
            first.iter().map(|(name, _)| name).collect::<Vec<_>>(),
            vec!["WAKE1", "WARPOUT"]
        );
    }

    #[test]
    fn loose_binding_covers_unspawned_particle_and_omits_malformed_optional_assets() {
        let root = TestRoot::new();
        std::fs::write(root.path().join("FX.SHP"), shp_with_undecodable_pixels(6))
            .expect("write header-valid effect SHP");
        std::fs::write(root.path().join("BROKEN.SHP"), shp_with_truncated_table(4))
            .expect("write malformed effect SHP");
        let assets = AssetManager::from_loose_root_for_test(root.path());

        let ini = IniFile::from_str(
            "[Particles]\n\
             0=Cloud\n\
             1=Torn\n\
             2=Absent\n\
             [Cloud]\n\
             Image=FX\n\
             [Torn]\n\
             Image=BROKEN\n\
             [Absent]\n\
             Image=MISSING\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("effect catalog rules");
        let art = ArtRegistry::from_ini(&IniFile::from_str("[FX]\nShadow=yes\n"));
        rules.install_art_data(art);

        let catalog = EffectAssetCatalog::bind(&rules, &assets, "TEM", "TEMPERATE");

        assert_eq!(catalog.raw_frame_count("FX"), Some(6));
        assert_eq!(catalog.effect_frame_count("FX"), Some(3));
        assert_eq!(catalog.raw_frame_count("BROKEN"), None);
    }

    /// A root whose SHP is missing is skipped, not fatal, and a bound root's
    /// `Next=` chain is bound with it. Retail art names building animations
    /// (`[CAARAY] ActiveAnim=CAARAY_A`) whose sprites never shipped.
    #[test]
    fn tolerant_binder_skips_missing_roots_and_follows_next_chains() {
        let root = TestRoot::new();
        std::fs::write(
            root.path().join("ALPHA.SHP"),
            shp_with_undecodable_pixels(6),
        )
        .expect("write root SHP");
        std::fs::write(root.path().join("BETA.SHP"), shp_with_undecodable_pixels(4))
            .expect("write chained SHP");
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let mut art = ArtRegistry::from_ini(&IniFile::from_str(
            "[ALPHA]\nNext=BETA\n[BETA]\nRate=450\n[ABSENT]\nRate=450\n",
        ));

        let skipped = art.bind_anim_class_assets(
            &["ABSENT".to_string(), "ALPHA".to_string()],
            &assets,
            "TEM",
            "TEMPERATE",
        );

        assert_eq!(skipped, 1, "the missing sprite is counted, not fatal");
        let bound = art.scheduler_anim_types();
        assert!(bound.contains("ALPHA"));
        assert!(bound.contains("BETA"), "Next= chain is bound with its root");
        assert!(!bound.contains("ABSENT"));
        assert_eq!(art.anim_runtime_config("BETA").unwrap().end, 4);
    }

    fn catalog_hash(catalog: &EffectAssetCatalog) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        catalog.hash(&mut hasher);
        hasher.finish()
    }

    fn shp_with_undecodable_pixels(frame_count: u16) -> Vec<u8> {
        let mut data = vec![0_u8; 8 + usize::from(frame_count) * 24];
        data[6..8].copy_from_slice(&frame_count.to_le_bytes());
        // A nonempty first frame with data_offset=0 makes full pixel decoding
        // fail, proving this binder intentionally requires only header metadata.
        data[12..14].copy_from_slice(&1_u16.to_le_bytes());
        data[14..16].copy_from_slice(&1_u16.to_le_bytes());
        data
    }

    fn shp_with_truncated_table(frame_count: u16) -> Vec<u8> {
        let mut data = vec![0_u8; 8];
        data[6..8].copy_from_slice(&frame_count.to_le_bytes());
        data
    }

    static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(0);

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let serial = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "vera20k-effect-catalog-{}-{serial}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create unique effect catalog test root");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod anim_class_root_tests {
    use super::anim_class_roots;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    #[test]
    fn bridge_roots_preserve_literal_factory_tokens_and_unread_types() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nMetallicDebris=ONE, FX, none, ,D,ONE\n\
             BridgeExplosions=FX, FX,none,<none>\n",
        ))
        .unwrap();
        let roots = anim_class_roots(&rules);
        for name in ["ONE", " FX", " NONE", " ", "D", "FX"] {
            assert!(roots.iter().any(|root| root == name), "{name:?}: {roots:?}");
        }
        assert!(!roots.iter().any(|root| root == "NONE"));
    }

    /// Every producer that constructs an `AnimClass` needs its type bound by
    /// the loader, or the animation silently draws nothing. The roots come
    /// from rules, so a renamed `[General]` key must show up here.
    #[test]
    fn roots_cover_teleport_superweapon_and_lightning_producers() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nWarpOut=MYWARP\nIronCurtainInvokeAnim=MYIRON\n\
             ForceShieldInvokeAnim=MYSHIELD\nIonBlast=MYRING\n\
             DominatorFirstAnim=MYHEAD\nDominatorSecondAnim=MYLOC\n\
             WeatherConClouds=MYCLOUD1,MYCLOUD2\nWeatherConBolts=MYBOLT\n\
             [CombatDamage]\nControlledAnimationType=MYMIND\n\
             PermaControlledAnimationType=MYPERMA\n",
        ))
        .expect("rules");
        let roots = anim_class_roots(&rules);
        for name in [
            "MYWARP", "MYIRON", "MYSHIELD", "MYRING", "MYCLOUD1", "MYCLOUD2", "MYBOLT", "MYHEAD",
            "MYLOC", "MYMIND", "MYPERMA",
        ] {
            assert!(
                roots.iter().any(|root| root == name),
                "{name} missing: {roots:?}"
            );
        }
    }

    /// Bridge collapse explosions are AnimClass objects; their types must be
    /// roots in their own right, not through a warhead that happens to list
    /// the same names.
    #[test]
    fn roots_cover_bridge_explosions_without_a_matching_warhead() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nBridgeExplosions=MYBRIDGE1,MYBRIDGE2\n",
        ))
        .expect("rules");
        let roots = anim_class_roots(&rules);
        for name in ["MYBRIDGE1", "MYBRIDGE2"] {
            assert!(
                roots.iter().any(|root| root == name),
                "{name} missing: {roots:?}"
            );
        }
    }

    /// Producers that name their type outside any warhead list: the ore
    /// twinkle, the cliff-collapse literals and the rocket's puffs and trail.
    #[test]
    fn roots_cover_ore_twinkle_and_producer_literals() {
        let rules = RuleSet::from_ini(&IniFile::from_str("[General]\nOreTwinkle=MYTWINKLE\n"))
            .expect("rules");
        let roots = anim_class_roots(&rules);
        for name in [
            "MYTWINKLE",
            "XGRYMED1",
            "XGRYMED2",
            "XGRYSML1",
            "V3TAKOFF",
            "V3TRAIL",
        ] {
            assert!(
                roots.iter().any(|root| root == name),
                "{name} missing: {roots:?}"
            );
        }
    }

    /// `[Powerups]` anims reach the binder; a `<none>` token and an absent
    /// row (the default `0,NONE`) contribute no root.
    #[test]
    fn roots_cover_powerup_pickup_anims() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[Animations]\n0=MONEY\n1=SPEED\n\
             [Powerups]\nMoney=20,MONEY,yes,2000\nUnit=20,<none>,no\nSpeed=10,SPEED,yes,1.2\n\
             Armor=10,UNREGISTERED,yes,1.5\n",
        ))
        .expect("rules");
        let roots = anim_class_roots(&rules);
        assert!(roots.iter().any(|root| root == "MONEY"), "{roots:?}");
        assert!(roots.iter().any(|root| root == "SPEED"), "{roots:?}");
        assert!(
            roots
                .iter()
                .all(|root| root != "NONE" && root != "<NONE>" && root != "UNREGISTERED")
        );
    }

    /// `RulesClass+0x298` defaults to a null type: without `IonBlast=` the
    /// Genetic Mutator constructs no animation, and no empty root is bound.
    #[test]
    fn absent_ion_blast_adds_no_root() {
        let rules = RuleSet::from_ini(&IniFile::from_str("[General]\n")).expect("rules");
        assert_eq!(rules.general.ion_blast_anim, "");
        assert!(anim_class_roots(&rules).iter().all(|root| !root.is_empty()));
    }
}
