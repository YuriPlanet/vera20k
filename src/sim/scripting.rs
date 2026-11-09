//! Sim-layer scripting extension point.
//!
//! Scripts run inside the deterministic simulation: their state lives in
//! `Simulation` (serde + state hash) and every side effect is queued as data
//! for the host to execute. Nothing here holds a host pointer, reads
//! wall-clock time, or calls a renderer directly.
//!
//! A TechnoType opts in with `Script=<Name>`. The name is resolved against the
//! script registry, which native scripts and the optional file-backed scripts
//! in `<ra2_dir>/vera20k/scripts` populate at startup. An unresolved name is a
//! diagnostic, never a load error.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::laser::LaserBirth;
use crate::sim::intern::InternedId;

/// Deterministic stable id of a Techno entity (matches `GameEntity::stable_id`).
pub type EntityId = u64;

/// A stable script identity. Written into saves, so a published id must never
/// be reused for a different script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ScriptId(pub u32);

/// Bit mask of the events a script wants. Dispatch skips scripts that did not
/// subscribe, so a large script set does not wake on every entity every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EventMask(pub u32);

impl EventMask {
    pub const UPDATE: u32 = 1 << 0;
    pub const BULLET_DETONATED: u32 = 1 << 1;

    pub const fn of(bits: u32) -> Self {
        Self(bits)
    }

    pub const fn contains(self, bit: u32) -> bool {
        self.0 & bit != 0
    }
}

/// The events the sim offers to scripts. Payloads are plain data so the same
/// event can cross a module boundary later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptEvent {
    Update {
        entity: EntityId,
        frame: i32,
    },
    /// One of `owner`'s projectiles detonated at `impact`.
    BulletDetonated {
        owner: EntityId,
        projectile: InternedId,
        impact: (i32, i32, i32),
    },
}

impl ScriptEvent {
    pub const fn entity(&self) -> EntityId {
        match *self {
            Self::Update { entity, .. } => entity,
            Self::BulletDetonated { owner, .. } => owner,
        }
    }

    pub const fn bit(&self) -> u32 {
        match self {
            Self::Update { .. } => EventMask::UPDATE,
            Self::BulletDetonated { .. } => EventMask::BULLET_DETONATED,
        }
    }
}

/// One projectile detonation reported to the script layer this tick.
///
/// `world_receiver::commit_projectile_detonations_inline` pushes one per
/// detonation; `dispatch_script_frame` drains the list into `BulletDetonated`.
/// A one-frame channel, so it is never saved and never part of the state hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulletHit {
    pub owner: EntityId,
    pub projectile: InternedId,
    pub impact: (i32, i32, i32),
}

/// A script's per-entity state. Opaque bytes: native scripts choose their own
/// encoding, file scripts use JSON. The host only promises byte-for-byte
/// persistence and deterministic iteration order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptState(pub Vec<u8>);

/// All script states in a simulation, keyed by (entity, script).
///
/// `BTreeMap` keeps iteration and hashing order independent of insertion
/// history, which is required for save/replay and multiplayer determinism.
/// Unknown script ids are preserved on load but are not dispatched once their
/// script is removed from the registry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptStateStore {
    slots: BTreeMap<(EntityId, u32), Vec<u8>>,
}

impl ScriptStateStore {
    pub fn get(&self, entity: EntityId, script: ScriptId) -> Option<&[u8]> {
        self.slots.get(&(entity, script.0)).map(Vec::as_slice)
    }

    pub fn set(&mut self, entity: EntityId, script: ScriptId, bytes: Vec<u8>) {
        self.slots.insert((entity, script.0), bytes);
    }

    pub fn take(&mut self, entity: EntityId, script: ScriptId) -> Option<Vec<u8>> {
        self.slots.remove(&(entity, script.0))
    }

    /// Called when an entity leaves the world. Script state must not outlive
    /// its owner, otherwise a reused stable id could inherit stale bytes.
    pub fn remove_entity(&mut self, entity: EntityId) {
        self.slots
            .retain(|(slot_entity, _), _| *slot_entity != entity);
    }

    /// Drop every slot whose entity is no longer alive.
    pub fn retain_entities(&mut self, mut alive: impl FnMut(EntityId) -> bool) {
        self.slots.retain(|(entity, _), _| alive(*entity));
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// FNV-1a over the ordered slots. Self-contained and stable across Rust
    /// versions, unlike `std::collections::hash_map::DefaultHasher`.
    pub fn checksum(&self) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut fold = |byte: u8| {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(0x100_0000_01b3);
        };
        for ((entity, script), bytes) in &self.slots {
            for byte in entity.to_le_bytes() {
                fold(byte);
            }
            for byte in script.to_le_bytes() {
                fold(byte);
            }
            for byte in (bytes.len() as u64).to_le_bytes() {
                fold(byte);
            }
            for byte in bytes {
                fold(*byte);
            }
        }
        hash
    }
}

/// A script requests work through the context; the host executes commands
/// after dispatch in a fixed order. Scripts never mutate the world directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SimCommand {
    /// Pure area damage: `apply_area_damage` alone, no animation, no sound, no
    /// light and no screen shake.
    ApplyWarhead {
        /// Rule name; the host interns it when executing the command.
        warhead: String,
        damage: i32,
        coord: (i32, i32, i32),
    },
    /// One invisible bullet's `Detonate` at `coord`: the warhead's screen
    /// shake, the area damage, the `AnimList`/`SplashList` animation (which
    /// carries its own `Report=` sound and light) and the impact flash.
    Detonate {
        warhead: String,
        damage: i32,
        coord: (i32, i32, i32),
    },
    FireSuperWeapon {
        super_weapon: String,
        cell: (u16, u16),
    },
    /// `ScreenNukeFlash` for `frames` frames with a superweapon type's
    /// `Light.*` tint: the map-wide lighting a native nuke launch triggers.
    NukeFlash {
        super_weapon: String,
        frames: i32,
    },
}

/// What a script is allowed to observe and request during one event.
pub struct ScriptContext<'a> {
    pub rules: &'a RuleSet,
    pub frame: i32,
    /// The scripted entity's owner house. House-coloured lasers need it.
    pub owner: InternedId,
    /// World-space anchor of the subject's *target*, in leptons. Only
    /// meaningful while [`Self::has_target`]; never the subject itself, so a
    /// script that damages at `anchor` cannot hit its own owner.
    pub anchor: (i32, i32, i32),
    /// `GetHeight()` of the anchor: the ground under an air target sits
    /// `anchor.2 - target_height`.
    pub target_height: i32,
    pub is_air_target: bool,
    /// `TechnoClass+0x2B4` is non-null and names a live target.
    pub has_target: bool,
    /// Stable id of the target object, when the target is an object.
    pub target_entity: Option<EntityId>,
    /// Rule name of the projectile for `BulletDetonated`; `None` otherwise.
    pub projectile: Option<&'a str>,
    /// Deterministic side effects, executed by the host after dispatch.
    pub commands: Vec<SimCommand>,
    /// Presentation-only side effects, emitted into the frame channel.
    pub lasers: Vec<LaserBirth>,
}

impl<'a> ScriptContext<'a> {
    pub fn new(rules: &'a RuleSet, frame: i32) -> Self {
        Self {
            rules,
            frame,
            owner: InternedId::default(),
            anchor: (0, 0, 0),
            target_height: 0,
            is_air_target: false,
            has_target: false,
            target_entity: None,
            projectile: None,
            commands: Vec::new(),
            lasers: Vec::new(),
        }
    }

    /// Queue a beam. The host turns it into `LifecycleOutput::LaserCreated`,
    /// which `app/presentation/lasers.rs` consumes.
    pub fn emit_laser(&mut self, birth: LaserBirth) {
        self.lasers.push(birth);
    }

    /// Queue area damage without the warhead's presentation. Use
    /// [`Self::detonate`] when the impact should animate, sound and shake.
    pub fn apply_warhead(&mut self, warhead: &str, damage: i32, coord: (i32, i32, i32)) {
        self.commands.push(SimCommand::ApplyWarhead {
            warhead: warhead.to_owned(),
            damage,
            coord,
        });
    }

    /// Queue a full detonation: damage plus the warhead's own presentation
    /// (AnimList animation, its `Report=` sound, screen shake, impact flash).
    pub fn detonate(&mut self, warhead: &str, damage: i32, coord: (i32, i32, i32)) {
        self.commands.push(SimCommand::Detonate {
            warhead: warhead.to_owned(),
            damage,
            coord,
        });
    }

    /// Queue a scenario flash with the named superweapon type's `Light.*` tint.
    pub fn nuke_flash(&mut self, super_weapon: &str, frames: i32) {
        self.commands.push(SimCommand::NukeFlash {
            super_weapon: super_weapon.to_owned(),
            frames,
        });
    }

    /// Queue a superweapon launch through `Simulation::fire_super_weapon`.
    pub fn fire_super_weapon(&mut self, super_weapon: &str, cell: (u16, u16)) {
        self.commands.push(SimCommand::FireSuperWeapon {
            super_weapon: super_weapon.to_owned(),
            cell,
        });
    }
}

/// Implemented by every script. File-backed scripts provide an adapter that
/// runs a `.rhai` source through the same trait.
pub trait Script: Send + Sync {
    fn id(&self) -> ScriptId;

    /// The value a TechnoType's `Script=` key names.
    fn binding_name(&self) -> &str {
        ""
    }

    fn interests(&self) -> EventMask;

    /// `state` is the calling entity's slot; it is written back to the store
    /// only if the script changed it.
    fn on_event(&self, event: &ScriptEvent, state: &mut ScriptState, ctx: &mut ScriptContext<'_>);
}

/// Compile-time registry. Dispatch order is registration order, which is part
/// of the deterministic contract; never sort by name or by hash here.
pub struct ScriptRegistry {
    scripts: Vec<Box<dyn Script>>,
}

impl Default for ScriptRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ScriptRegistry {
    pub const fn new() -> Self {
        Self {
            scripts: Vec::new(),
        }
    }

    pub fn register(&mut self, script: Box<dyn Script>) {
        let id = script.id();
        assert!(
            !self.scripts.iter().any(|existing| existing.id() == id),
            "duplicate ScriptId"
        );
        self.scripts.push(script);
    }

    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.scripts.len()
    }

    /// Resolve a TechnoType's `Script=` value to a registered script. An empty
    /// registry yields `None` for every binding; the host reports that as a
    /// diagnostic rather than failing the load.
    pub fn binding_for(&self, name: &str) -> Option<ScriptId> {
        self.scripts
            .iter()
            .find(|script| script.binding_name().eq_ignore_ascii_case(name))
            .map(|script| script.id())
    }

    /// Dispatch one event to every subscribed script. Returns the collected
    /// context so the host can execute commands and emit presentation outputs
    /// in order.
    pub fn dispatch<'r>(
        &self,
        event: &ScriptEvent,
        states: &mut ScriptStateStore,
        rules: &'r RuleSet,
        frame: i32,
    ) -> ScriptContext<'r> {
        self.dispatch_with(event, states, ScriptContext::new(rules, frame))
    }

    /// Dispatch with a host-prepared context (entity anchor, target facts).
    pub fn dispatch_with<'r>(
        &self,
        event: &ScriptEvent,
        states: &mut ScriptStateStore,
        mut ctx: ScriptContext<'r>,
    ) -> ScriptContext<'r> {
        for script in &self.scripts {
            if !script.interests().contains(event.bit()) {
                continue;
            }
            let mut state = ScriptState(
                states
                    .get(event.entity(), script.id())
                    .unwrap_or_default()
                    .to_vec(),
            );
            script.on_event(event, &mut state, &mut ctx);
            states.set(event.entity(), script.id(), state.0);
        }
        ctx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counter;

    impl Script for Counter {
        fn id(&self) -> ScriptId {
            ScriptId(7)
        }

        fn interests(&self) -> EventMask {
            EventMask::of(EventMask::UPDATE)
        }

        fn on_event(
            &self,
            event: &ScriptEvent,
            state: &mut ScriptState,
            _ctx: &mut ScriptContext<'_>,
        ) {
            if matches!(event, ScriptEvent::Update { .. }) {
                let ticks = state.0.first().copied().unwrap_or(0).wrapping_add(1);
                state.0 = vec![ticks];
            }
        }
    }

    #[test]
    fn dispatch_updates_only_subscribed_entities_and_persists_state() {
        let rules = RuleSet::empty_for_test();
        let mut registry = ScriptRegistry::new();
        registry.register(Box::new(Counter));
        let mut states = ScriptStateStore::default();

        let update = ScriptEvent::Update {
            entity: 42,
            frame: 10,
        };
        registry.dispatch(&update, &mut states, &rules, 10);
        registry.dispatch(&update, &mut states, &rules, 11);
        assert_eq!(states.get(42, ScriptId(7)), Some(&[2u8][..]));

        // A different entity has its own slot.
        registry.dispatch(
            &ScriptEvent::Update {
                entity: 43,
                frame: 12,
            },
            &mut states,
            &rules,
            12,
        );
        assert_eq!(states.get(43, ScriptId(7)), Some(&[1u8][..]));

        // Slots of entities that left the world are pruned.
        states.retain_entities(|entity| entity != 42);
        assert_eq!(states.len(), 1);
        assert_eq!(states.get(42, ScriptId(7)), None);
    }

    #[test]
    fn checksum_is_order_independent_and_sensitive_to_state() {
        let mut a = ScriptStateStore::default();
        a.set(2, ScriptId(1), vec![1, 2]);
        a.set(1, ScriptId(2), vec![3]);
        let mut b = ScriptStateStore::default();
        b.set(1, ScriptId(2), vec![3]);
        b.set(2, ScriptId(1), vec![1, 2]);
        assert_eq!(a.checksum(), b.checksum());

        b.set(2, ScriptId(1), vec![1, 2, 3]);
        assert_ne!(a.checksum(), b.checksum());
    }

    #[test]
    fn commands_and_lasers_are_collected_without_touching_the_world() {
        let rules = RuleSet::empty_for_test();
        let mut ctx = ScriptContext::new(&rules, 5);
        ctx.apply_warhead("TestWH", 100, (1, 2, 3));
        ctx.fire_super_weapon("TestSW", (4, 5));
        assert_eq!(ctx.commands.len(), 2);
        assert!(ctx.lasers.is_empty());
        assert_eq!(ctx.frame, 5);
    }
}

#[path = "scripting/source.rs"]
mod source;

pub use source::{
    MANIFEST_NAME, MODULE_EXT, SCRIPT_DIR, SOURCE_EXT, ScriptDirectory, ScriptEntry, ScriptFile,
    ScriptFileKind, ScriptManifest,
};

/// Process-wide registry. It is installed once before the first frame and is
/// immutable afterwards, so dispatch never takes a lock and every simulation
/// in the process sees the same set.
static SCRIPT_REGISTRY: OnceLock<ScriptRegistry> = OnceLock::new();

pub fn script_registry() -> &'static ScriptRegistry {
    SCRIPT_REGISTRY.get_or_init(ScriptRegistry::new)
}

/// Install once, before any frame runs. A second install returns the registry
/// back so the caller can report it instead of silently dropping scripts.
pub fn install_script_registry(registry: ScriptRegistry) -> Result<(), ScriptRegistry> {
    SCRIPT_REGISTRY.set(registry)
}

#[path = "scripting/runtime.rs"]
mod runtime;

#[path = "scripting/file.rs"]
mod file;

pub use file::{FileScript, discover as discover_file_scripts};

/// Install the script registry once, before the first frame.
///
/// File scripts are registered first so a `.rhai` file can provide any binding
/// it declares; native scripts fill the bindings no file provides.
pub fn install_scripts(directory: Option<&ScriptDirectory>) {
    let mut registry = ScriptRegistry::new();
    if let Some(directory) = directory {
        let (scripts, diagnostics) = file::discover(directory);
        for diagnostic in &diagnostics {
            log::warn!("script: {diagnostic}");
        }
        if !scripts.is_empty() {
            log::info!(
                "script: {} file script(s) from {}",
                scripts.len(),
                directory.root.display()
            );
        }
        for script in scripts {
            registry.register(Box::new(script));
        }
    }
    if let Err(returned) = install_script_registry(registry) {
        log::warn!(
            "script registry already installed; {} script(s) dropped",
            returned.len()
        );
    }
}
