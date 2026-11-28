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
use std::hash::{Hash, Hasher};

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectAssetCatalog {
    entries: BTreeMap<String, EffectAssetFrameCounts>,
    /// Successfully loaded groups admitted by the shared resource resolver:
    /// all names with Ares, only the selected singleton otherwise. Rebuilt
    /// with this catalog so failed requests never borrow another group.
    extra_building_palette_names: BTreeSet<String>,
    /// Header admission for each declared overlay picture, by overlay ID.
    overlay_pictures: BTreeMap<u8, OverlayPicture>,
    /// VERA: the overlay types without a CellAnim that have an image at
    /// bind, from a picture or an SHP ([`overlay_admitted`]). A CellAnim
    /// admits the overlay on its own (ReadMapOverlayPacks 0x005FD36D), so
    /// these are the admissions authored placement reads from files: what
    /// `RuleSet::file_inputs_hash` folds ([`Self::fold_overlay_admission`]),
    /// whatever file gave them. Not in [`Hash`].
    overlay_admission: BTreeSet<u8>,
}

/// VERA-only, experimental (docs/picture-model-art.md): a declared overlay
/// picture's admission.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OverlayPicture {
    /// The picture was found and readable: the overlay has an image.
    loaded: bool,
    /// The overlay has no CellAnim, so this admission decides placement.
    places: bool,
    /// The game-folder file that supplied a picture whose admission is not
    /// the shipped SHP's, or that stands in for an SHP a retail install
    /// lacks (`FolderFiles::note_stand_in`), for a refused start to name.
    /// Naming only: nothing hashes it.
    file: Option<String>,
}

/// The config hash's overlay fold (`RuleSet::simulation_config_hash`, saves
/// and replays) is the declared pictures' admission, as before the start's
/// own fold existed: nothing for retail art.
impl Hash for EffectAssetCatalog {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.entries.hash(state);
        self.extra_building_palette_names.hash(state);
        let mut facts = self
            .overlay_pictures
            .iter()
            .filter(|(_, picture)| picture.places)
            .peekable();
        if facts.peek().is_some() {
            b"overlay-picture-admission-v1".hash(state);
            for (id, picture) in facts {
                id.hash(state);
                picture.loaded.hash(state);
            }
        }
    }
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
            // VERA-only, experimental (docs/picture-model-art.md): an image
            // whose art section declares `Picture=` counts the picture's
            // frames; its SHP is never opened.
            let raw = if let Some((section, decl)) = rules.art().resolve_declared_picture(&name, "")
            {
                let Some(raw) = rules
                    .art()
                    .picture_header(section, decl, asset_manager, theater_name)
                    .and_then(|facts| u16::try_from(facts.frames).ok())
                else {
                    continue;
                };
                raw
            } else {
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
                match ShpFile::frame_count_from_bytes(data) {
                    Ok(count) => count,
                    Err(error) => {
                        log::warn!(
                            "Authoritative effect asset [{name}] ({file_name}) has an invalid SHP header: {error}; consumers will use their fallback"
                        );
                        continue;
                    }
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

        let overlays = rules.overlay_registry();
        if rules.art().declares_any_picture() {
            let folder = crate::rules::retail_sources::FolderFiles::new(asset_manager);
            for id in 0..overlays.len().min(256) {
                let id = id as u8;
                let Some(name) = overlays.name(id) else {
                    continue;
                };
                let Some((section, decl)) = overlays.declared_picture(name, rules.art()) else {
                    continue;
                };
                let facts = rules
                    .art()
                    .picture_header(section, decl, asset_manager, theater_name);
                let loaded = facts.is_some();
                let places = overlays
                    .flags(id)
                    .is_some_and(|flags| flags.cell_anim.is_none());
                let mut file = BTreeSet::new();
                // Named when it admits otherwise than the shipped SHP
                // (`load_overlay_shape`), or stands in for one a retail
                // install lacks.
                if places {
                    folder.note_stand_in(
                        facts.as_ref().map_or(&decl.file, |facts| &facts.file),
                        loaded.then_some(()),
                        overlays.shp_candidates(name, theater_ext, theater_name),
                        |bytes| ShpFile::from_bytes(bytes).ok().map(drop),
                        &mut file,
                    );
                }
                catalog.overlay_pictures.insert(
                    id,
                    OverlayPicture {
                        loaded,
                        places,
                        file: file.pop_first(),
                    },
                );
            }
        }
        // VERA: every placing overlay's admission, from its picture or its
        // SHP, as authored placement takes it (`resolved_overlay_shp_ids`).
        let admission = (0..overlays.len().min(256))
            .map(|id| id as u8)
            .filter(|&id| {
                overlays
                    .flags(id)
                    .is_some_and(|flags| flags.cell_anim.is_none())
                    && overlay_admitted(
                        overlays,
                        &catalog,
                        asset_manager,
                        id,
                        theater_ext,
                        theater_name,
                    )
            })
            .collect();
        catalog.overlay_admission = admission;
        catalog
    }

    /// VERA: which overlay types without a CellAnim have an image, from a
    /// picture or an SHP alike: what `RuleSet::file_inputs_hash` folds of
    /// overlays. Equal admissions give equal folds whether the SHP is there
    /// or a picture stands in for it.
    pub(crate) fn fold_overlay_admission<H: Hasher>(&self, state: &mut H) {
        b"overlay-admission-v1".hash(state);
        self.overlay_admission.hash(state);
    }

    pub(crate) fn overlay_picture_files(&self) -> impl Iterator<Item = &str> {
        self.overlay_pictures
            .values()
            .filter_map(|picture| picture.file.as_deref())
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
    // The Ares roots below join only for Rules read the Ares way.
    let ares_way = crate::rules::ares_switch::ares_way();
    // Ares common Launch10033068..100330AA constructs SW.Animation through
    // the same AnimClass owner as other effects, even when it is hidden.
    if ares_way {
        for sw in rules.super_weapons.values() {
            if let Some(Some(animation)) = &sw.presentation.animation {
                insert(animation);
            }
        }
    }
    for warhead in rules.warheads_iter() {
        for name in &warhead.anim_list {
            insert(name);
        }
        // Installed Ares EMP10062610/10062FD0 constructs the retained
        // Warhead sparkle as AnimClass421EA0. Existing-pointer lookup alone
        // does not bind its physical bounds for that constructor.
        if ares_way && let Some(name) = warhead.ares_rules().and_then(|emp| emp.sparkles()) {
            insert(name);
        }
        // AttachedEffect 100586E0 constructs the registered AnimClass with
        // flags 0x600 and then attaches it to the victim. It needs physical
        // bounds through this same loader closure before construction.
        if ares_way
            && let Some(name) = warhead
                .ares_rules()
                .and_then(|wh| wh.attach_effect().animation())
        {
            insert(name);
        }
        // Ares `InfDeathAnim=` replaces the death-table arm with a literal
        // AnimClass built at the victim's cell, so it is an ordinary
        // construction root and needs the same physical bounds. A warhead may
        // set only `InfDeathAnim`, with no `InfDeath=`, so nothing else names
        // the animation, and without this root the constructor refuses with
        // `animation type [X] has no bound SHP frame count` and the kill plays
        // no death animation at all.
        if ares_way
            && let Some(name) = warhead
                .ares_combat()
                .and_then(|combat| combat.inf_death_anim())
        {
            insert(name);
        }
    }
    // First activation10062625 may fall back through Type/General, while
    // refresh uses only the explicit Warhead pointer. Bind every configured
    // producer through this same closure, for graphical and headless loads.
    if ares_way {
        for object in rules.all_objects() {
            if let Some(Some(name)) = object.emp_rules().and_then(|emp| emp.sparkles()) {
                insert(name);
            }
            // Self effects reach the same Anim constructor as Warhead effects
            // (Ares Update10058E00, self-attach10058FC5 -> Attach10058520
            // -> CreateAnim100586E0). Their image
            // bounds are gameplay inputs too: an attached anim may kill its
            // parent through ordinary AnimAI damage and its death weapon.
            if let Some(name) = object
                .self_attach_effect()
                .and_then(|effect| effect.animation())
            {
                insert(name);
            }
        }
        if let Some(name) = rules.emp_sparkles() {
            insert(name);
        }
    }
    // Muzzle animations `TechnoClass::Fire_At` constructs
    // (`sim::world::damage_consequences`, `combat::fire_coord`), and the
    // assault anim at each occupant KillOccupants (`0x004586B7`) kills.
    for weapon in rules.weapons_iter() {
        for name in weapon
            .anim
            .iter()
            .chain(&weapon.occupant_anim)
            .chain(&weapon.assault_anim)
        {
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
    // AircraftAI414ED0..414F42 constructs this retained fixed-ART AnimType
    // through ordinary Anim421EA0, including dynamically allocated aliases.
    for object in rules.all_objects() {
        if let Some(name) = object.aircraft_trailer().animation() {
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
    // BuildingReceiveDamage442832..4428F4 constructs an OnFire Anim before
    // attaching it. Keep each resolved slot key, including unread late AV
    // allocations, in the ordinary AnimClass asset-binding closure.
    for name in &rules.general.on_fire {
        insert(name);
    }
    for name in &rules.general.metallic_debris {
        insert(name);
    }
    // AnimStart424DCC..424E5F constructs the retained mineral Debris type.
    // Bind these roots even when no weapon or death effect names them.
    for tiberium in rules.tiberium_types.types() {
        for name in tiberium.debris() {
            insert(name);
        }
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
    // NPSE ObjectClass::Paradrop C2F939..C2F94D substitutes Rules+400
    // for non-infantry. It needs the same physical bounds as Parachute.
    if rules.npatch()
        && let Some(name) = rules.general.vehicle_parachute.as_deref()
    {
        insert(name);
    }
    // Ares Type/Country/Side `Parachute.Anim=`: an attached canopy needs the same
    // physical animation bounds before the simulator creates it.
    if rules.ares() {
        for name in rules.parachute_animations() {
            insert(name);
        }
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
    // TechnoClass::Install_Drain_Link70FD70 constructs Rules+31C's
    // DrainAnimationType at70FDF5..70FE14 and attaches it to the drainer.
    // Bind its physical frames before Fire_At6FDF8C can create the AnimClass;
    // the same scheduler closure supplies the renderer's animation atlas.
    if let Some(name) = rules.combat_damage.drain_animation_type.as_deref() {
        insert(name);
    }
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
    for &index in rules.powerups.animation_indices() {
        if let Some(name) = rules.anim_type_runtime_key_by_index(index) {
            insert(&name);
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
    // Slot 9's names are on the rules section (`0x00464450..0x004644FD`), so
    // no art row above reaches them. Unbound, the constructor is refused and
    // Grand_Opening (`0x00446662`) leaves the turret undrawn.
    roots.extend(
        rules
            .all_objects()
            .filter_map(crate::rules::art_data::shp_turret_anim_config)
            .flat_map(|row| {
                let damaged = row.damaged_variant.map(|variant| variant.anim_type);
                std::iter::once(row.anim_type).chain(damaged)
            })
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
///
/// VERA-only, experimental (`docs/picture-model-art.md`): a type whose art
/// declares `Picture=` has an image when its picture is found and readable;
/// its SHP is never opened.
pub(super) fn resolved_overlay_shp_ids(
    registry: &OverlayTypeRegistry,
    catalog: &EffectAssetCatalog,
    asset_manager: &AssetManager,
    theater_ext: &str,
    theater_name: &str,
) -> BTreeSet<u8> {
    (0..registry.len().min(usize::from(u8::MAX) + 1))
        .map(|raw_id| raw_id as u8)
        .filter(|&overlay_id| {
            overlay_admitted(
                registry,
                catalog,
                asset_manager,
                overlay_id,
                theater_ext,
                theater_name,
            )
        })
        .collect()
}

/// Whether overlay type `overlay_id` has an image: its declared picture
/// loaded ([`EffectAssetCatalog::bind`]), else its SHP loads
/// ([`load_overlay_shape`]). The one admission authored placement and the
/// start's file inputs read.
fn overlay_admitted(
    registry: &OverlayTypeRegistry,
    catalog: &EffectAssetCatalog,
    asset_manager: &AssetManager,
    overlay_id: u8,
    theater_ext: &str,
    theater_name: &str,
) -> bool {
    let Some(name) = registry.name(overlay_id) else {
        return false;
    };
    match catalog.overlay_pictures.get(&overlay_id) {
        Some(picture) => picture.loaded,
        None => {
            load_overlay_shape(asset_manager, registry, name, theater_ext, theater_name).is_some()
        }
    }
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

#[cfg(all(test, tests_rules_a_n))]
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

    /// VERA-only (docs/picture-model-art.md): a particle image whose art
    /// declares `Picture=` counts the picture's frames, not its SHP's, and an
    /// overlay type whose art declares one has an image without any SHP. A
    /// retail install has no SHP for it, so the start's file inputs change
    /// and name the picture; once it is gone, the overlay is admitted as on
    /// a retail install and nothing is named.
    #[test]
    fn declared_pictures_give_particle_counts_and_overlay_presence() {
        let root = TestRoot::new();
        std::fs::write(root.path().join("FX.SHP"), shp_with_undecodable_pixels(6))
            .expect("write effect SHP");
        let picture = crate::assets::picture_file::test_picture(4, 4, Some(4));
        std::fs::write(root.path().join("fx.png"), &picture).expect("write effect picture");
        std::fs::write(root.path().join("wall.png"), &picture).expect("write wall picture");
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let ini = IniFile::from_str(
            "[Particles]\n0=Cloud\n[Cloud]\nImage=FX\n\
             [OverlayTypes]\n0=PICWALL\n1=BAREWALL\n[PICWALL]\nWall=yes\n[BAREWALL]\nWall=yes\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("effect catalog rules");
        rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(
            "[FX]\nShadow=yes\nPicture=fx.png\n[PICWALL]\nPicture=wall.png\n",
        )));

        let catalog = EffectAssetCatalog::bind(&rules, &assets, "TEM", "TEMPERATE");
        assert_eq!(catalog.raw_frame_count("FX"), Some(4), "the SHP holds 6");
        assert_eq!(catalog.effect_frame_count("FX"), Some(2));

        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        let available = rules.resolved_overlay_shp_ids(&assets, "TEM", "TEMPERATE");
        assert!(available.contains(&rules.overlay_registry().id_for_name("PICWALL").unwrap()));
        assert!(!available.contains(&rules.overlay_registry().id_for_name("BAREWALL").unwrap()));
        let loaded = rules.file_inputs_hash();
        assert!(
            rules
                .file_input_folder_files()
                .iter()
                .any(|name| name.eq_ignore_ascii_case("wall.png"))
        );

        std::fs::remove_file(root.path().join("wall.png")).unwrap();
        let assets = AssetManager::from_loose_root_for_test(root.path());
        rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
        assert_ne!(rules.file_inputs_hash(), loaded);
        assert!(rules.file_input_folder_files().is_empty());
    }

    /// VERA-only (docs/picture-model-art.md): an overlay picture that admits
    /// the overlay as the shipped SHP does leaves the start's file inputs the
    /// original game's and names no file, also once a repacked archive drops
    /// that SHP (the picture is then named, for a refused start); one for an
    /// overlay the shipped archives hold no SHP for changes them and is
    /// named, as does a picture that cannot be read where the SHP is gone.
    #[test]
    fn an_overlay_picture_that_admits_what_its_shp_admits_keeps_the_retail_file_inputs() {
        use crate::assets::asset_manager::MediaArchiveMode;
        let dir = crate::assets::asset_manager::test_support::audio_test_install(
            "overlay-picture-file-inputs",
        );
        let ini = IniFile::from_str(
            "[OverlayTypes]
0=SHPWALL
1=NEWWALL
[SHPWALL]
Wall=yes
[NEWWALL]
Wall=yes
",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("overlay rules");
        let shp_name = rules
            .overlay_registry()
            .shp_candidates("SHPWALL", "TEM", "TEMPERATE")
            .remove(0);
        // A 2x2 one-frame raw SHP.
        let mut shp = vec![0_u8; 8 + 24];
        shp[2..4].copy_from_slice(&2u16.to_le_bytes());
        shp[4..6].copy_from_slice(&2u16.to_le_bytes());
        shp[6..8].copy_from_slice(&1u16.to_le_bytes());
        shp[12..14].copy_from_slice(&2u16.to_le_bytes());
        shp[14..16].copy_from_slice(&2u16.to_le_bytes());
        shp[28..32].copy_from_slice(&32u32.to_le_bytes());
        shp.extend_from_slice(&[1, 2, 3, 4]);
        std::fs::write(
            dir.path().join("ra2md.mix"),
            crate::map::source::test_support::make_new_format_mix_bytes(&[(
                shp_name.as_str(),
                shp.as_slice(),
            )]),
        )
        .unwrap();
        let picture = crate::assets::picture_file::test_picture(4, 4, Some(2));
        std::fs::write(dir.path().join("shpwall.png"), &picture).unwrap();
        std::fs::write(dir.path().join("newwall.png"), &picture).unwrap();
        let mut bound = |art: &str| {
            rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(art)));
            let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
            rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
            (rules.file_inputs_hash(), rules.file_input_folder_files())
        };
        let (retail, retail_files) = bound("");
        assert!(retail_files.is_empty(), "{retail_files:?}");
        assert_eq!(
            bound("[SHPWALL]\nPicture=shpwall.png\n"),
            (retail, Vec::new())
        );
        let (added, files) = bound("[NEWWALL]\nPicture=newwall.png\n");
        assert_ne!(added, retail);
        assert_eq!(files, ["newwall.png"]);
        // A repacked archive without the SHP the picture replaces.
        std::fs::write(
            dir.path().join("ra2md.mix"),
            crate::map::source::test_support::make_new_format_mix_bytes(&[(
                "unrelated.bin",
                b"fixture".as_slice(),
            )]),
        )
        .unwrap();
        assert_eq!(bound("[SHPWALL]\nPicture=shpwall.png\n").0, retail);
        std::fs::write(dir.path().join("shpwall.png"), b"not a picture").unwrap();
        assert_ne!(bound("[SHPWALL]\nPicture=shpwall.png\n").0, retail);
    }

    /// The NPSE `[General] VehicleParachute=` canopy is bound through the same
    /// anim asset root the stock `Parachute=` uses, so a mod's own canopy
    /// carries physical frame bounds before `AnimClass::effective_bounds`
    /// reads them.
    #[test]
    fn vehicle_parachute_canopy_binds_through_the_production_load() {
        let root = TestRoot::new();
        std::fs::write(
            root.path().join("VEHCHUTE.SHP"),
            shp_with_undecodable_pixels(6),
        )
        .expect("write canopy SHP");
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let art = IniFile::from_str("[PARACH]\nLoopCount=-1\n[VEHCHUTE]\nLoopCount=-1\n");
        // `[Animations]` slot 1 also feeds the fifth infantry death anim, so
        // the canopy takes slot 0 and the stock canopy slot 1 to keep this
        // fixture from rooting the wrong name there.
        let processed = crate::rules::native_processing::RulesLayerStack::new(IniFile::from_str(
            "[Animations]\n0=VEHCHUTE\n1=PARACH\n\
             [General]\nParachute=PARACH\nVehicleParachute=vehchute\n",
        ))
        .process_with_fixed_art(&art)
        .expect("production retained Rules read");
        let mut rules =
            RuleSet::from_processed_rules(&processed).expect("project the processed rules");
        rules.install_art_data(ArtRegistry::from_ini(&art));

        assert!(
            rules.npatch(),
            "the fixture's VehicleParachute key turns NPatch on"
        );
        assert_eq!(
            rules.general.vehicle_parachute.as_deref(),
            Some("VEHCHUTE"),
            "the extension reader publishes the pointer"
        );
        let roots = anim_class_roots(&rules);
        assert!(
            roots.iter().any(|root| root == "VEHCHUTE"),
            "the canopy's art root follows the extension pointer"
        );
        rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
        assert_eq!(
            rules
                .art()
                .anim_runtime_config("VEHCHUTE")
                .and_then(|config| config.raw_shp_frame_count),
            Some(6),
            "the bound canopy carries the SHP's physical frame bounds"
        );
        assert!(rules.art().scheduler_anim_types().contains("VEHCHUTE"));
    }

    #[test]
    fn ares_canopy_key_alone_activates_its_reader_and_asset_root() {
        let _stock = crate::rules::ares_switch::read_ares_way(false);
        let art = IniFile::from_str("[TYPECHUTE]\nLoopCount=-1\n");
        let processed = crate::rules::native_processing::RulesLayerStack::new(IniFile::from_str(
            "[Animations]\n0=TYPECHUTE\n[InfantryTypes]\n0=E1\n\
             [E1]\nParachute.Anim=TYPECHUTE\n",
        ))
        .process_with_fixed_art(&art)
        .unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        assert!(rules.ares());
        assert_eq!(
            rules
                .object("E1")
                .unwrap()
                .techno_combat()
                .presentation()
                .parachute_anim(),
            Some("TYPECHUTE")
        );
        assert!(
            anim_class_roots(&rules)
                .iter()
                .any(|name| name == "TYPECHUTE")
        );
        rules.install_art_fixture(ArtRegistry::from_ini(&art));
        rules.bind_anim_frame_count_for_test("TYPECHUTE", 6);
        let mut sim = crate::sim::world::Simulation::new();
        sim.entities_mut_for_test().insert(
            crate::sim::game_entity::GameEntity::test_default_of_category(
                1,
                "E1",
                "Americans",
                10,
                11,
                crate::map::entities::EntityCategory::Infantry,
            ),
        );
        *sim.interner = crate::sim::intern::test_interner();
        let anim = sim.attach_parachute_anim(&rules, 1).unwrap();
        assert_eq!(
            sim.interner.resolve(sim.anim(anim).unwrap().type_id()),
            "TYPECHUTE"
        );
    }

    #[test]
    fn ares_canopy_type_and_country_bind_through_the_production_load() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::sim::game_entity::GameEntity;
        use crate::sim::world::Simulation;

        let _ares = crate::rules::ares_switch::read_ares_way(true);
        let root = TestRoot::new();
        std::fs::write(
            root.path().join("TYPECHUTE.SHP"),
            shp_with_undecodable_pixels(6),
        )
        .unwrap();
        std::fs::write(
            root.path().join("COUNTRYCHUTE.SHP"),
            shp_with_undecodable_pixels(8),
        )
        .unwrap();
        let assets = AssetManager::from_loose_root_for_test(root.path());
        let art = IniFile::from_str("[TYPECHUTE]\nLoopCount=-1\n[COUNTRYCHUTE]\nLoopCount=-1\n");
        let mut stack = RulesLayerStack::new(IniFile::from_str(
            "[Animations]\n0=TYPECHUTE\n1=PARACH\n2=COUNTRYCHUTE\n\
             [InfantryTypes]\n0=E1\n[E1]\nParachute.Anim=TYPECHUTE\n\
             [Countries]\n0=Americans\n[Americans]\nSide=GDI\nParachute.Anim=COUNTRYCHUTE\n",
        ));
        let processed = stack.process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        let roots = anim_class_roots(&rules);
        assert!(roots.iter().any(|name| name == "TYPECHUTE"));
        assert!(roots.iter().any(|name| name == "COUNTRYCHUTE"));
        rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
        assert_eq!(
            rules
                .art()
                .anim_runtime_config("TYPECHUTE")
                .unwrap()
                .raw_shp_frame_count,
            Some(6)
        );
        assert_eq!(
            rules
                .art()
                .anim_runtime_config("COUNTRYCHUTE")
                .unwrap()
                .raw_shp_frame_count,
            Some(8)
        );

        let mut sim = Simulation::new();
        sim.entities_mut_for_test()
            .insert(GameEntity::test_default_of_category(
                1,
                "E1",
                "Americans",
                10,
                11,
                crate::map::entities::EntityCategory::Infantry,
            ));
        *sim.interner = crate::sim::intern::test_interner();
        let owner = sim.entities().get(1).unwrap().owner();
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner,
                0,
                Some(owner),
                true,
                0,
                10,
            ));
        let attach = |sim: &mut Simulation, rules: &RuleSet| {
            let anim = sim.attach_parachute_anim(rules, 1).unwrap();
            Simulation::wind_down_parachute_anim(sim.substrate_mut_for_test(), 1);
            assert_eq!(sim.anim(anim).unwrap().owner_entity(), Some(1));
            assert_eq!(sim.anim(anim).unwrap().runtime().loop_remaining, 0);
            sim.interner
                .resolve(sim.anim(anim).unwrap().type_id())
                .to_owned()
        };
        assert_eq!(attach(&mut sim, &rules), "TYPECHUTE");
        stack.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[E1]\nParachute.Anim=UNKNOWN\n"),
        );
        let processed = stack.process_with_fixed_art(&art).unwrap();
        let mut retained = RuleSet::from_processed_rules(&processed).unwrap();
        retained.install_art_fixture(rules.art().clone());
        assert_eq!(attach(&mut sim, &retained), "TYPECHUTE");
        stack.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[E1]\nParachute.Anim=<none>\n"),
        );
        let processed = stack.process_with_fixed_art(&art).unwrap();
        let mut cleared = RuleSet::from_processed_rules(&processed).unwrap();
        cleared.install_art_fixture(rules.art().clone());
        assert_eq!(attach(&mut sim, &cleared), "COUNTRYCHUTE");
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
        for ares_way in [false, true] {
            let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
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
                let expected = if ares_way {
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
                assert_eq!(roots.iter().any(|root| root == name), ares_way, "{name}");
            }
            // Same tolerant binder used by the app loader; no test-only frame
            // setter. Missing unrelated default roots remain tolerant.
            rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
            let mut sim = Simulation::with_seed(17);
            for name in names {
                let expected = ares_way.then_some(6);
                assert_eq!(
                    rules
                        .art()
                        .anim_runtime_config(name)
                        .unwrap()
                        .raw_shp_frame_count,
                    expected,
                    "{name}"
                );
                assert_eq!(rules.art().scheduler_anim_types().contains(name), ares_way);
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
                assert_eq!(spawned.is_ok(), ares_way, "{name}: {spawned:?}");
            }
        }
    }

    #[test]
    fn self_attached_damage_animation_binds_and_reaches_the_helpers_death_weapon() {
        for ares_way in [false, true] {
            let _ares = crate::rules::ares_switch::read_ares_way(ares_way);
            let root = TestRoot::new();
            std::fs::write(
                root.path().join("PULSE.SHP"),
                shp_with_undecodable_pixels(6),
            )
            .unwrap();
            let assets = AssetManager::from_loose_root_for_test(root.path());
            // Keep PULSE out of the stock InfDeath registry slots, so its only
            // producer is the unit's own AttachEffect rather than a Warhead.
            let ini = crate::sim::arena_fixture::clear_ground_ini(
                "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=HELPER\n1=VICTIM\n\
             [HELPER]\nStrength=1\nArmor=target\nExplodes=yes\nDeathWeapon=BURST\n\
             AttachEffect.Duration=-1\nAttachEffect.Animation=PULSE\n\
             [VICTIM]\nStrength=1000\nArmor=heavy\n[ArmorTypes]\ntarget=0%\n\
             [Animations]\n0=DUMMY\n1=ELECTRO\n2=PULSE\n\
             [CombatDamage]\nFlameDamage2=FIRE\n[Warheads]\n0=FIRE\n1=BLAST\n\
             [FIRE]\nCellSpread=.3\nVerses=0%,0%,0%,0%,0%,0%,0%,0%,0%,0%,0%\nVersus.target=100%\n\
             [BLAST]\nCellSpread=1\nPercentAtMax=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
             [WeaponTypes]\n0=BURST\n[BURST]\nDamage=100\nRange=1\nProjectile=INVISO\nWarhead=BLAST\n\
             [Projectiles]\n0=INVISO\n[INVISO]\nInviso=yes\n",
            );
            let art = IniFile::from_str("[PULSE]\nRate=900\nLoopCount=-1\nDamage=10\n");
            let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
            rules.install_art_data(ArtRegistry::from_ini(&art));
            let roots = anim_class_roots(&rules);
            rules.bind_anim_class_assets(&roots, &assets, "TEM", "TEMPERATE");
            assert_eq!(
                rules
                    .art()
                    .anim_runtime_config("PULSE")
                    .unwrap()
                    .raw_shp_frame_count,
                ares_way.then_some(6),
            );
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let helper = sim
                .spawn_object("HELPER", "Americans", 20, 20, 0, &rules)
                .unwrap();
            let victim = sim
                .spawn_object("VICTIM", "Russians", 21, 20, 0, &rules)
                .unwrap();
            for _ in 0..8 {
                sim.advance_tick(&[], Some(&rules), None, Some(rules.overlay_registry()), 22);
            }
            let alive = sim
                .entities()
                .get(helper)
                .is_some_and(|entity| entity.is_object_alive());
            assert_eq!(alive, !ares_way);
            assert_eq!(
                sim.entities().get(victim).unwrap().health.current() < 1000,
                ares_way,
                "the self animation must reach the real death-weapon receiver",
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

#[cfg(all(test, tests_rules_a_n))]
mod anim_class_root_tests {
    use super::anim_class_roots;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    #[test]
    fn bridge_roots_preserve_literal_factory_tokens_and_unread_types() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            // FX is registered: Ares reads BridgeExplosions Find-only
            // (Buf_General 1005BB00, hook 0x66D55E).
            "[Animations]\n0=FX\n[General]\nMetallicDebris=ONE, FX, none, ,D,ONE\n\
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
            // The weather anims are registered for Ares's Find-only lists.
            "[Animations]\n0=MYCLOUD1\n1=MYCLOUD2\n2=MYBOLT\n\
             [General]\nBehind=MYBEHIND\nWarpOut=MYWARP\nIronCurtainInvokeAnim=MYIRON\n\
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
