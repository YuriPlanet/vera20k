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

/// Current Rust producers whose animation names have not yet been lifted into
/// parsed rules fields. Keep the producer and this binding root list together
/// until that follow-up is complete.
const LIGHTNING_BOLT_ANIMS: [&str; 3] = ["WCLBOLT1", "WCLBOLT2", "WCLBOLT3"];

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
            let image_id = rules.art_registry.resolve_effective_image_id(&name, &name);
            let candidates = art_data::anim_shp_candidates(
                Some(&rules.art_registry),
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
            let scheduler_owned = rules.art_registry.scheduler_anim_types().contains(&name);
            let shadow = rules
                .art_registry
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

/// Every animation name the simulation can turn into an `AnimClass` instance,
/// which the loader must bind before the match starts.
///
/// The three producers that fill `CombatResult::explosion_effects`:
/// - the killing warhead's `AnimList=` pick
///   (`WarheadTypeClass::Detonate` -> `Warhead::SelectExplosionAnim @ 0x0048A4F0`),
/// - the infantry death animation for the warhead's `InfDeath=`,
/// - the dying object's own `Explosion=` / `DestroyAnim=` pick
///   (`UnitClass::Death_Explosion @ 0x00738680`).
///
/// The list is derived from loaded rules, never hand-written: over retail
/// `rulesmd.ini` it resolves to 58 distinct names (34 `AnimList=`, 14
/// `Explosion=`, 13 `DestroyAnim=`, plus the infantry-death family), which is
/// why the binder that consumes it must tolerate the handful retail authors
/// with no art section.
pub fn anim_class_roots(rules: &RuleSet) -> Vec<String> {
    let mut roots = BTreeSet::new();
    let mut insert = |name: &str| {
        if let Some(name) = canonical_asset_id(name) {
            roots.insert(name);
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
    for name in rules.general.infantry_death_anims.iter().flatten() {
        insert(name);
    }
    for object in rules.all_objects() {
        for name in object.explosion_anims.iter().chain(&object.destroy_anims) {
            insert(name);
        }
    }
    // `[General] WarpOut=`: the teleport locomotor constructs it at both ends
    // of a relocation (`sim::movement::teleport_movement`).
    insert(&rules.general.warp_out.name);
    // `SuperClass::Launch` invoke animations (`sim::superweapon`).
    insert(&rules.general.iron_curtain_invoke_anim);
    insert(&rules.general.force_shield_invoke_anim);
    insert(&rules.general.ion_blast_anim);
    // Lightning Storm bolts (`sim::superweapon::lightning_storm`).
    for name in LIGHTNING_BOLT_ANIMS {
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
    // The cliff-collapse debris types are literals in the producer
    // (`sim::world`); stock binds them only through warhead `AnimList=`.
    for name in CLIFF_COLLAPSE_ANIMS {
        insert(name);
    }
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
        rules.merge_art_data(&art);

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

    /// Every producer that constructs an `AnimClass` needs its type bound by
    /// the loader, or the animation silently draws nothing. The roots come
    /// from rules, so a renamed `[General]` key must show up here.
    #[test]
    fn roots_cover_teleport_superweapon_and_lightning_producers() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nWarpOut=MYWARP\nIronCurtainInvokeAnim=MYIRON\n\
             ForceShieldInvokeAnim=MYSHIELD\nIonBlast=MYRING\n",
        ))
        .expect("rules");
        let roots = anim_class_roots(&rules);
        for name in [
            "MYWARP", "MYIRON", "MYSHIELD", "MYRING", "WCLBOLT1", "WCLBOLT2", "WCLBOLT3",
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
    /// twinkle and the cliff-collapse literals.
    #[test]
    fn roots_cover_ore_twinkle_and_cliff_collapse_literals() {
        let rules = RuleSet::from_ini(&IniFile::from_str("[General]\nOreTwinkle=MYTWINKLE\n"))
            .expect("rules");
        let roots = anim_class_roots(&rules);
        for name in ["MYTWINKLE", "XGRYMED1", "XGRYMED2", "XGRYSML1"] {
            assert!(
                roots.iter().any(|root| root == name),
                "{name} missing: {roots:?}"
            );
        }
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
