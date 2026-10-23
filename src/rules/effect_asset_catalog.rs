//! GPU-independent effect assets used by authoritative match systems.
//!
//! Besides SHP frame counts, the catalog admits the loaded custom palette
//! capability used by original Building DestroyAnim remapping. A rules request
//! alone cannot substitute for the native loader's non-null group.
//!
//! The renderer may consume this catalog when deciding which effect frames to
//! make resident, but it is not the source of the counts. Binding happens from
//! merged rules/ART plus the active theater's asset search path, so graphical
//! and headless matches receive the same immutable inputs.

use std::collections::{BTreeMap, BTreeSet};

use crate::assets::asset_manager::AssetManager;
use crate::assets::shp_file::ShpFile;
use crate::rules::art_data;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;

/// Raw and consumer-visible frame counts for one authoritative effect asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectAssetFrameCounts {
    raw: u16,
    available: u16,
}

/// Deterministic match input derived from effect SHPs and palette availability.
///
/// Keys are trimmed, uppercase asset IDs. A `BTreeMap` is deliberate: binding
/// and hashing must not inherit `HashMap`'s process-random iteration order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct EffectAssetCatalog {
    entries: BTreeMap<String, EffectAssetFrameCounts>,
    /// Successfully loaded groups admitted by the shared resource resolver:
    /// all names with Ares, only the selected singleton otherwise. Rebuilt
    /// with this catalog so failed requests never borrow another group.
    extra_building_palette_names: BTreeSet<String>,
}

impl EffectAssetCatalog {
    /// Bind every particle image root and the admitted custom palette groups.
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
        let requests: BTreeSet<_> = rules
            .all_objects()
            .filter_map(|object| object.custom_palette_name())
            .collect();
        let mut catalog = Self::default();
        for name in requests {
            if rules
                .load_extra_building_palette(asset_manager, theater_ext, Some(name))
                .is_some()
            {
                catalog.extra_building_palette_names.insert(name.to_owned());
            }
        }

        for name in authoritative_effect_roots(rules) {
            let image_id = rules.art().resolve_effective_image_id(&name, &name);
            let candidates = art_data::effect_shp_candidates(
                crate::rules::art_data::EffectImageKind::Particle,
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

    pub(crate) fn loaded_extra_building_palette_name(&self, name: &str) -> Option<&str> {
        self.extra_building_palette_names
            .get(name)
            .map(String::as_str)
    }

    /// Consumer-visible frame count (particle image timing).
    #[cfg(test)]
    fn effect_frame_count(&self, name: &str) -> Option<u16> {
        self.entry(name).map(|counts| counts.available)
    }

    /// Literal unsigned SHP header frame count.
    ///
    /// Particle animation-state parity can consume this independently of the
    /// AnimType body/shadow split. Native behavior for a modded particle image
    /// carrying `Shadow=yes` remains UNCHECKED; retaining both values prevents
    /// the catalog from baking that policy decision into asset binding.
    #[cfg(any(test, feature = "test-support"))]
    pub fn raw_frame_count(&self, name: &str) -> Option<u16> {
        self.entry(name).map(|counts| counts.raw)
    }

    /// Entries in canonical asset-name order.
    #[cfg(any(test, feature = "test-support"))]
    fn iter(&self) -> impl Iterator<Item = (&str, EffectAssetFrameCounts)> {
        self.entries
            .iter()
            .map(|(name, counts)| (name.as_str(), *counts))
    }

    #[cfg(any(test, feature = "test-support"))]
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

    #[cfg(any(test, feature = "test-support"))]
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

/// Nuclear warning literal81907C: one-time Launch6CDB86 ->43B5E0 and
/// Building Mission_Missile44C9DC. Both construct an ordinary AnimClass.
pub(crate) const PSI_WARNING_ANIM: &str = "PSIWARN";

/// ParasiteClass::UpdateSquidGrapple6298CC looks up this executable literal
/// (83665C), independently of weapon AnimList and other authored roots.
pub(crate) const SQUID_GRAPPLE_ANIM: &str = "SQDG";

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
        // Installed Ares EMP10062610/10062FD0 constructs the retained
        // Warhead sparkle as AnimClass421EA0. Existing-pointer lookup alone
        // does not bind its physical bounds for that constructor.
        #[cfg(feature = "ares")]
        if let Some(name) = warhead.ares_rules().and_then(|emp| emp.sparkles()) {
            insert(name);
        }
        // AttachedEffect 100586E0 constructs the registered AnimClass with
        // flags 0x600 and then attaches it to the victim. It needs physical
        // bounds through this same loader closure before construction.
        #[cfg(feature = "ares")]
        if let Some(name) = warhead
            .ares_rules()
            .and_then(|wh| wh.attach_effect().animation())
        {
            insert(name);
        }
    }
    // First activation10062625 may fall back through Type/General, while
    // refresh uses only the explicit Warhead pointer. Bind every configured
    // producer through this same closure, for graphical and headless loads.
    #[cfg(feature = "ares")]
    {
        for object in rules.all_objects() {
            if let Some(Some(name)) = object.emp_rules().and_then(|emp| emp.sparkles()) {
                insert(name);
            }
        }
        if let Some(name) = rules.emp_sparkles() {
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
            .chain(&object.dead_bodies)
        {
            insert(name);
        }
    }
    // Infantry520BC6..520CA4 constructs its type's DeadBodies, else the
    // General list. The completion owner must find bound physical shapes
    // even when no warhead or other producer names these corpse animations.
    for name in &rules.general.dead_bodies {
        insert(name);
    }
    for name in &rules.general.metallic_debris {
        insert(name);
    }
    // AreaDamage48A34D constructs the retained BarrelExplode; Voxel749F30
    // constructs its ExpireAnim74A1AF and BounceAnim74A87F. Bind these
    // producers explicitly rather than depending on another warhead's list.
    if let Some(name) = rules.general.barrel_explode.as_deref() {
        insert(name);
    }
    for kind in rules.voxel_anim_types_iter() {
        for name in kind.expire_anim.iter().chain(&kind.bounce_anim) {
            insert(name);
        }
    }
    // BuildingDestructionEffects441AF3 uses find-only on literal FIRE3.
    // An orphan ART section must not create a registered Type.
    if rules.anim_type_names.contains("FIRE3") {
        insert("FIRE3");
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
    // Techno70F1D0 attaches [General] Behind= to an obscured object. Its
    // AnimClass must have physical frame bounds before the AI can create it.
    if let Some(name) = rules.general.behind.as_deref() {
        insert(name);
    }
    // `[General] WarpOut=`: the teleport locomotor constructs it at both ends
    // of a relocation (`sim::movement::teleport_movement`).
    insert(&rules.general.warp_out.name);
    // TemporalClass::Update71A8FA..71A909 constructs WarpAway at the erased
    // target's Location. The frozen leaf AI constructs ChronoSparkle1 every
    // 24 frames (Unit73622F, Infantry51BAF6..51BB69, Building4403D4).
    // Both need physical frame bounds before sim::temporal can spawn them;
    // the bound scheduler set also supplies the atlas's anim.pal effect roots.
    insert(&rules.general.warp_away.name);
    insert(&rules.general.chrono_sparkle1.name);
    // `[General] MoveFlash=`: the marker a Move click on a Cell constructs
    // there (`FootClass::ClickedAction_Cell`, `0x004D7F8A`;
    // `Simulation::spawn_move_flash`).
    insert(&rules.general.move_flash.name);
    // `SuperClass::Launch` invoke animations (`sim::superweapon`).
    insert(&rules.general.iron_curtain_invoke_anim);
    insert(&rules.general.force_shield_invoke_anim);
    insert(&rules.general.ion_blast_anim);
    insert(&rules.general.ion_beam_anim);
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
    insert(PSI_WARNING_ANIM);
    insert(SQUID_GRAPPLE_ANIM);
    roots.into_iter().collect()
}

/// AnimClass asset roots bound tolerantly: a type whose SHP is missing draws
/// nothing, as natively, instead of failing the load.
///
/// Building animations belong here, not in the strict scheduler closure. Art
/// defines building sections no rules list names and whose sprites retail never
/// shipped (`[CAARAY]` with `ActiveAnim=CAARAY_A`), and a few real buildings
/// name animations no archive holds (`GAGAP_A`, `GAREFNL4`), so requiring every
/// art building animation made every retail map fail to load. (Most of the
/// names that first failed were a separate resolver fault, since fixed in
/// `art_data::effect_shp_candidates`.)
pub fn tolerant_anim_class_roots(rules: &RuleSet) -> Vec<String> {
    let mut roots = anim_class_roots(rules);
    roots.extend(
        rules
            .art()
            .building_anim_roots()
            .into_iter()
            .filter(|name| rules.anim_type_names.contains(name)),
    );
    roots
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

/// Overlay type IDs whose native SHP candidates load as an SHP from the active
/// theater's archives. Authored-overlay finalization (`map::authored_overlay`)
/// reads it on graphical and headless loads alike.
pub fn resolved_overlay_shp_ids(
    registry: &OverlayTypeRegistry,
    asset_manager: &AssetManager,
    theater_ext: &str,
    theater_name: &str,
) -> BTreeSet<u8> {
    let mut available = BTreeSet::new();
    for raw_id in 0..registry.len().min(usize::from(u8::MAX) + 1) {
        let overlay_id = raw_id as u8;
        let Some(name) = registry.name(overlay_id) else {
            continue;
        };
        if load_overlay_shape(asset_manager, registry, name, theater_ext, theater_name).is_some() {
            available.insert(overlay_id);
        }
    }
    available
}

/// Resolve the reached OverlayRead5FE928 / InitTheater5FE620 / saved
/// Load5FEAF0 plan once, using native cached LoadFileFromMIX5B40B0 selection.
/// The first physical SHP is the resident image even if its frames are empty.
/// Authored admission, drawing and radar all consume this same resource owner.
pub fn load_overlay_shape(
    asset_manager: &AssetManager,
    registry: &OverlayTypeRegistry,
    name: &str,
    theater_ext: &str,
    theater_name: &str,
) -> Option<(String, ShpFile)> {
    registry
        .shp_candidates(name, theater_ext, theater_name)
        .into_iter()
        .find_map(|candidate| {
            let loaded = asset_manager.load_file_from_mix(&candidate)?;
            let shp = ShpFile::from_bytes(&loaded.bytes).ok()?;
            Some((candidate, shp))
        })
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
        insert(&particle.image);
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
    fn saved_overlay_suffix_runs_only_after_generic_shape_misses() {
        let Some((_, retail)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let bytes = retail.get_ref("SEAL.SHP").unwrap();
        let root = TestRoot::new();
        for name in ["GTFALL.SHP", "GAPRIM.SHP", "GTPRIM.SHP"] {
            std::fs::write(root.path().join(name), bytes).unwrap();
        }
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let mut processed =
            crate::rules::native_processing::RulesLayerStack::new(IniFile::from_str(
                "[OverlayTypes]\n0=FALL\n1=PRIM\n[FALL]\nImage=GAFALL\n[PRIM]\nImage=GAPRIM\n",
            ))
            .process()
            .unwrap();
        let registry = OverlayTypeRegistry::from_processed_rules(&processed);
        assert!(load_overlay_shape(&assets, &registry, "FALL", "TEM", "TEMPERATE").is_none());
        // OverlayLoad5FEB45 tries generic GAFALL/GGFALL first;5FEBEC
        // appends one theater-letter attempt only while Shape is null.
        processed.restore_object_image_loads("TEMPERATE").unwrap();
        let registry = OverlayTypeRegistry::from_processed_rules(&processed);
        assert_eq!(
            load_overlay_shape(&assets, &registry, "FALL", "TEM", "TEMPERATE")
                .unwrap()
                .0,
            "GTFALL.SHP"
        );
        assert_eq!(
            load_overlay_shape(&assets, &registry, "PRIM", "TEM", "TEMPERATE")
                .unwrap()
                .0,
            "GAPRIM.SHP",
            "a resident generic primary wins over the available suffix"
        );
    }

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

    /// A root whose SHP is missing stays a real null-image Anim, and a bound root's
    /// `Next=` chain is bound with it. Retail art names building animations
    /// (`[CAARAY] ActiveAnim=CAARAY_A`) whose sprites never shipped.
    #[test]
    fn tolerant_binder_keeps_imageless_roots_and_follows_next_chains() {
        let root = TestRoot::new();
        std::fs::write(
            root.path().join("ALPHA.SHP"),
            shp_with_undecodable_pixels(6),
        )
        .expect("write root SHP");
        std::fs::write(root.path().join("BETA.SHP"), shp_with_undecodable_pixels(4))
            .expect("write chained SHP");
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let art = IniFile::from_str("[ALPHA]\nNext=BETA\n[BETA]\nRate=450\n[ABSENT]\nRate=450\n");
        // The ordered Rules processor reads ALPHA's `Next=` with its ART body.
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str("[Animations]\n0=ALPHA\n1=BETA\n2=ABSENT\n"),
            &art,
        )
        .expect("anim rules");
        rules.install_art_data(ArtRegistry::from_ini(&art));

        let skipped = rules.bind_anim_class_assets(
            &["ABSENT".to_string(), "ALPHA".to_string()],
            &assets,
            "TEM",
            "TEMPERATE",
        );

        assert_eq!(skipped, 0, "AnimRead427D00 succeeds with a null image");
        let bound = rules.art().scheduler_anim_types();
        assert!(bound.contains("ALPHA"));
        assert!(bound.contains("BETA"), "Next= chain is bound with its root");
        assert!(bound.contains("ABSENT"));
        let absent = rules.art().anim_runtime_config("ABSENT").unwrap();
        assert_eq!(
            (absent.raw_shp_frame_count, absent.end, absent.loop_end),
            (Some(0), 0, 0)
        );
        assert_eq!(rules.art().anim_runtime_config("BETA").unwrap().end, 4);
    }

    #[test]
    fn ares_emp_sparkle_roots_bind_physical_bounds_before_anim_construction() {
        use crate::sim::{
            anim_class::AnimWorldCoord, components::AnimClassSpawnDescriptor, world::Simulation,
        };
        let native = crate::test_fixture::json("tools/rules_oracle/ares_emp.json");
        for row in native["registered_animation_readers"].as_array().unwrap() {
            let mut source = String::from("[Warheads]\n0=EMP\n[Animations]\n");
            for (index, name) in row["fixture"]["registered"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
            {
                source.push_str(&format!("{index}={}\n", name.as_str().unwrap()));
            }
            source.push_str(&format!(
                "[EMP]\nEMP.Sparkles={}\n",
                row["fixture"]["raw"].as_str().unwrap()
            ));
            let rules = RuleSet::from_ini(&IniFile::from_str(&source)).unwrap();
            let expected = if crate::rules::ARES_ENABLED {
                row["result"]["reference"].as_str()
            } else {
                None
            };
            assert_eq!(
                rules
                    .warhead("EMP")
                    .unwrap()
                    .ares_rules()
                    .and_then(|emp| emp.sparkles()),
                expected,
                "{row}"
            );
        }
        let root = TestRoot::new();
        let names = ["WH_SPARK", "TYPE_SPARK", "GLOBAL_SPARK", "AE_VISUAL"];
        for name in names {
            std::fs::write(
                root.path().join(format!("{name}.SHP")),
                shp_with_undecodable_pixels(6),
            )
            .expect("write header-valid synthetic sparkle SHP");
        }
        let assets = AssetManager::from_loose_root_for_test(root.path());
        // GeneralRules retains the original InfDeath5 animation at registry
        // index1 even without Ares. Reserve that slot so this fixture isolates
        // EMP roots rather than also binding TYPE_SPARK through electrocution.
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=CONTROL\n[Warheads]\n0=EMP\n[Animations]\n0=WH_SPARK\n1=ELECTRO\n2=TYPE_SPARK\n3=GLOBAL_SPARK\n4=AE_VISUAL\n[General]\nEMPulseSparkles=GLOBAL_SPARK\n[CONTROL]\nStrength=100\nEMP.Sparkles=TYPE_SPARK\n[EMP]\nEMP.Duration=2\nEMP.Sparkles=WH_SPARK\nAttachEffect.Duration=5\nAttachEffect.Animation=ae_visual\n",
        );
        let art = IniFile::from_str(
            "[WH_SPARK]\nLoopCount=-1\n[TYPE_SPARK]\nLoopCount=-1\n[GLOBAL_SPARK]\nLoopCount=-1\n[AE_VISUAL]\nLoopCount=-1\n",
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art)
            .expect("production retained Rules/ART reads");
        rules.install_art_data(ArtRegistry::from_ini(&art));
        // The diagnostic's ART-name check alone cannot admit construction:
        // all configs exist here but still lack physical SHP frame bounds.
        for name in names {
            assert_eq!(
                rules
                    .art()
                    .anim_runtime_config(name)
                    .unwrap()
                    .raw_shp_frame_count,
                None
            );
        }
        let roots = anim_class_roots(&rules);
        for name in names {
            assert_eq!(
                roots.iter().any(|root| root == name),
                crate::rules::ARES_ENABLED,
                "{name}"
            );
        }
        // Same tolerant binder used by the app loader; no test-only frame
        // setter. Missing unrelated default roots remain tolerant.
        rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
        let mut sim = Simulation::with_seed(17);
        for name in names {
            let expected = crate::rules::ARES_ENABLED.then_some(6);
            assert_eq!(
                rules
                    .art()
                    .anim_runtime_config(name)
                    .unwrap()
                    .raw_shp_frame_count,
                expected,
                "{name}"
            );
            assert_eq!(
                rules.art().scheduler_anim_types().contains(name),
                crate::rules::ARES_ENABLED
            );
            let type_name = sim.intern(name);
            let world = AnimWorldCoord {
                x: 128,
                y: 128,
                z: 0,
            };
            let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
            let spawned = sim.spawn_anim_at_world(
                &rules,
                AnimClassSpawnDescriptor {
                    draw_flags: 0x600,
                    ..AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z)
                },
                world,
            );
            assert_eq!(
                spawned.is_ok(),
                crate::rules::ARES_ENABLED,
                "{name}: {spawned:?}"
            );
        }
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
            "[General]\nBehind=MYBEHIND\nWarpOut=MYWARP\nIronCurtainInvokeAnim=MYIRON\n\
             ForceShieldInvokeAnim=MYSHIELD\nIonBlast=MYRING\n\
             DominatorFirstAnim=MYHEAD\nDominatorSecondAnim=MYLOC\n\
             WeatherConClouds=MYCLOUD1,MYCLOUD2\nWeatherConBolts=MYBOLT\n\
             [CombatDamage]\nControlledAnimationType=MYMIND\n\
             PermaControlledAnimationType=MYPERMA\n",
        ))
        .expect("rules");
        let roots = anim_class_roots(&rules);
        for name in [
            "MYBEHIND", "MYWARP", "MYIRON", "MYSHIELD", "MYRING", "MYCLOUD1", "MYCLOUD2", "MYBOLT",
            "MYHEAD", "MYLOC", "MYMIND", "MYPERMA",
        ] {
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
    fn roots_cover_overlay_and_voxel_death_producers() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nBarrelExplode=MYBLAST\n[VoxelAnims]\n0=MYCHUNK\n\
             [MYCHUNK]\nExpireAnim=MYEXPIRE\nBounceAnim=MYBOUNCE\n\
             [Animations]\n999=FIRE3\n",
        ))
        .unwrap();
        let roots = anim_class_roots(&rules);
        for name in ["MYBLAST", "MYEXPIRE", "MYBOUNCE", "FIRE3"] {
            assert!(
                roots.iter().any(|root| root == name),
                "{name} absent: {roots:?}"
            );
        }
    }

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
}
