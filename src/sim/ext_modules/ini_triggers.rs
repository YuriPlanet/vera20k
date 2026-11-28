//! Module triggers declared in INI (RFC 0001 "Binding a trigger to a type",
//! sections 2, 6a and 7; appendix "INI keys"):
//!
//! ```ini
//! [E1]
//! Wasm.Tags=GIPanicTag
//! [GIPanicTag]
//! Trigger=GIPanic
//! [GIPanic]
//! Event=TakeDamage
//! Where1=Subject.HealthPercent<30
//! Where2=Var(Subject,Panicked)=0
//! Action1=SetVar(Subject,Panicked,1)
//! Action2=Wasm:giPanic
//! Repeat=Repeating
//! ```
//!
//! **Layer.** Every key is read from the selected mod's own `rulesmd.ini`
//! (the stock `RULESMD.INI` without a mod), without layering, as the
//! `[VeraWasm]` policy is: RFC section 2 reads `Wasm.Tags` only from the
//! mod's file and ignores it in maps, so a map cannot add or erase type
//! bindings. [`IniTriggers::read`] checks the syntax when the scenario's
//! modules load ([`super::PreparedExtModules::load`]);
//! [`Simulation::install_ini_triggers`] resolves the names once the
//! simulation and its modules exist and builds each trigger section into the
//! one module trigger owner through `trigger_create`'s own path
//! (`Host::create_trigger`), bound by type through `trigger_bind`'s
//! (`Host::bind_trigger`). A type binding is looked up when an event is
//! raised, so objects created later are bound too. Each trigger section is
//! one trigger, shared by every type whose tags reach it.
//!
//! **Syntax.** `Event`, `Action` and `Where` without a number are number 1.
//! `Event<n>` and `Action<n>` count from 1 without gaps. `Where<n>` belongs
//! to event 1, `Event<k>.Where<n>` to event `k`. An event is `name` or
//! `name(op,interval,id,value)` (op `Eq`..`Ge` or `=`, `!=`, `<`, `<=`, `>`,
//! `>=`). A clause is `Subject|Other|House.Property<op>value` or
//! `Var(Global|Subject|Other|House,name)<op>value`. An action is
//! `Wasm:name` (`Wasm:mod.name`, `Wasm:map.name`), `Wasm:name(target,text)`
//! (the text, commas and all, reaches the module as the record's `args`), or
//! `name(target,params...)`: an `ActionTarget` name (empty for `Subject`),
//! then the parameters of the appendix 5.2/5.3 row in table order, empty
//! ones 0. With target `Cell`, a waypoint number comes first. An `id`
//! parameter takes a name of its kind (TechnoType, weapon, warhead,
//! superweapon, TeamType, variable, property, animation, voxel animation,
//! particle system) or a number, an enum parameter (`Mission`, `Outcome`,
//! `Blocked`, `ModifierOp`) a member name or its number, `house` a country
//! name, `cell` a waypoint number; `other` comes from the event and is not
//! written. An event's `id` takes a number, or for `PropertyPoll` a
//! property name and for `VariablePoll` a variable name. Section, key and
//! name mistakes refuse the match start with a message naming the section
//! and key.
//!
//! **Bound sections.** `Wasm.Tags` on a TechnoType binds by type; on a
//! weapon or warhead section (a weapon or warhead named in the rules) it
//! binds the weapon or warhead, which needs every event of the trigger to
//! carry one (`Fire`, `Killed`); on a projectile section it binds the
//! projectile type where this build can (else the start is refused, naming
//! the section).
//!
//! **Saves.** The triggers live in the module trigger registry, which the
//! trigger runtime saves with their bindings and per-object state; a loaded
//! game does not read the INI again.
//!
//! **Stopped modules.** An INI trigger is not a module's: when the module its
//! `Wasm:` actions address stops, the trigger keeps its other actions
//! (`ModuleTriggers::remove_source`). A `Wasm:` action naming a module the
//! scenario lacks refuses the start.
//!
//! **Maps.** A map's own `Wasm.Tags` is never read (RFC section 2); the
//! match tells the players so when a map carries one
//! (`PreparedExtModules::load`).
//!
//! Residuals:
//! - Sound parameters take numbers only: the simulation keeps no sound list
//!   (soundmd.ini belongs to the client), and no sound action runs from a
//!   module trigger yet.
//! - A section named both as a TechnoType and as a weapon, warhead or
//!   projectile binds as the TechnoType.
//! - A `House=` or `house` parameter naming a country without a house in this
//!   scenario leaves that trigger out (logged).
//! - All of one trigger's `Wasm:` actions must address one module.
//! - The map warning is a match notice, not a lobby one: the lobby does not
//!   read map rules.

use vera20k_wasm_ext::triggers::{
    ACTION_MODULE, ActionDef, ActionTarget, EVENT_PROPERTY_POLL, EVENT_VARIABLE_POLL, EventDef,
    FilterClause, FilterSubject, MAX_ACTIONS, MAX_CLAUSES, MAX_EVENTS, Repeat, VarTarget,
};
use vera20k_wasm_ext::{
    ActionParams, NAME_ANIM, NAME_PARTICLE_SYSTEM, NAME_PROJECTILE, NAME_SUPER_WEAPON,
    NAME_TECHNO_TYPE, NAME_VOXEL_ANIM, NAME_WARHEAD, NAME_WEAPON, Status, TABLE_PROPERTY,
    TriggerDef,
};

use super::{
    GLOBAL_VARS, LOCAL_VARS, ModuleSource, REF_KIND_SHIFT, REF_WAYPOINT, house_ref, type_ref,
};
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ini_value::scan_decimal_i32;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// ReadString buffers for these VERA keys.
const LIST_CAPACITY: usize = 0x800;
const VALUE_CAPACITY: usize = 0x200;

/// The INI triggers of a mod, syntax checked, names not yet resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct IniTriggers {
    /// Each section with `Wasm.Tags`, in file order, with the indices of the
    /// triggers its tags reach.
    bindings: Vec<(String, Vec<usize>)>,
    /// Each trigger section, in order of first use.
    triggers: Vec<TriggerSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TriggerSpec {
    section: String,
    events: Vec<EventSpec>,
    actions: Vec<(String, ActionSpec)>,
    house: Option<String>,
    difficulty: u8,
    repeat: Repeat,
    /// `Decide=Wasm:name`: the handler's module (`mod.`/`map.`) and name.
    decide: Option<(Option<ModuleSource>, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EventSpec {
    key: String,
    name: String,
    op: u8,
    interval: i32,
    /// A number or a name, resolved by event kind at install; `None` is −1.
    id: Option<String>,
    value: i64,
    clauses: Vec<(String, ClauseSpec)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClauseSpec {
    reads: Reads,
    op: u8,
    value: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Reads {
    /// `Subject.`, `Other.` or `House.` and a property name.
    Property(Whose, String),
    Var(VarTarget, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Whose {
    Subject,
    Other,
    House,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActionSpec {
    /// `Wasm:name` or `Wasm:name(target,text)`; a `None` source addresses
    /// the mod module (RFC section 2). The text goes to the module as the
    /// record's `args` (RFC 0001 section 6h).
    Module {
        source: Option<ModuleSource>,
        name: String,
        target: ActionTarget,
        /// `None` without parentheses.
        text: Option<String>,
    },
    Kind {
        name: String,
        target: ActionTarget,
        args: Vec<String>,
    },
}

/// One `ActionParams` field an INI action writes (appendix 5 tables).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Param {
    Id(IdKind),
    /// `arg` holding a member of an appendix 8.3 enum.
    Enum(EnumKind),
    Arg2,
    Value,
    Value2,
    Cell,
    House,
    Text,
}

/// What an `id` parameter names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdKind {
    /// A number only: a sound's `[SoundList]` index (module residuals).
    Sound,
    TechnoType,
    Weapon,
    Warhead,
    SuperWeapon,
    TeamType,
    Variable,
    Property,
    /// The AnimType array position.
    Anim,
    VoxelAnim,
    ParticleSystem,
    /// A registered module event's name (RFC 0001 section 6h).
    ModuleEvent,
}

/// The appendix 8.3 enum an `arg` parameter holds; it takes a member name
/// (ASCII case-insensitive) or a number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EnumKind {
    Mission,
    Outcome,
    Blocked,
    ModifierOp,
}

impl EnumKind {
    /// Members by number (appendix 8.3); `Mission` adds `None` = 255 and the
    /// INI spelling `Wait` of `Deliberate`.
    fn value(self, name: &str) -> Option<i32> {
        const MISSIONS: [&str; 32] = [
            "Sleep",
            "Attack",
            "Move",
            "QMove",
            "Retreat",
            "Guard",
            "Sticky",
            "Enter",
            "Capture",
            "Eaten",
            "Harvest",
            "AreaGuard",
            "Return",
            "Stop",
            "Ambush",
            "Hunt",
            "Unload",
            "Sabotage",
            "Construction",
            "Selling",
            "Repair",
            "Rescue",
            "Missile",
            "Harmless",
            "Open",
            "Patrol",
            "ParadropApproach",
            "ParadropOverfly",
            "Deliberate",
            "AttackMove",
            "SpyplaneApproach",
            "SpyplaneOverfly",
        ];
        let members: &[&str] = match self {
            Self::Mission => {
                if name.eq_ignore_ascii_case("None") {
                    return Some(255);
                }
                if name.eq_ignore_ascii_case("Wait") {
                    return Some(28);
                }
                &MISSIONS
            }
            Self::Outcome => &["Win", "Lose"],
            Self::Blocked => &["Nearby", "Fail"],
            Self::ModifierOp => &["Add", "Multiply", "Override"],
        };
        members
            .iter()
            .position(|member| member.eq_ignore_ascii_case(name))
            .and_then(|value| i32::try_from(value).ok())
    }
}

/// A key's place in its trigger section.
enum TriggerKey {
    Event(u32),
    Where { event: u32, number: u32 },
    Action(u32),
    Decide,
    House,
    Difficulty,
    Repeat,
}

impl IniTriggers {
    /// Reads every `Wasm.Tags` of `mod_rules` and the tag and trigger
    /// sections they reach. An error names the section and key.
    pub(crate) fn read(mod_rules: &IniFile) -> Result<Self, String> {
        let mut read = Self::default();
        for name in mod_rules.section_names() {
            let Some(tags) = mod_rules
                .section(name)
                .and_then(|section| section.read_trimmed_list("Wasm.Tags", LIST_CAPACITY))
            else {
                continue;
            };
            let mut reached = Vec::new();
            for tag in tags.into_iter().filter(|tag| !tag.is_empty()) {
                let section = mod_rules
                    .section(tag)
                    .ok_or_else(|| format!("[{name}] Wasm.Tags: no tag section [{tag}]"))?;
                let triggers = section
                    .read_trimmed_list("Trigger", LIST_CAPACITY)
                    .unwrap_or_default();
                if triggers.iter().all(|t| t.is_empty()) {
                    return Err(format!("[{tag}] Trigger: names no trigger section"));
                }
                for trigger in triggers.into_iter().filter(|t| !t.is_empty()) {
                    let index = match read.triggers.iter().position(|t| t.section == trigger) {
                        Some(index) => index,
                        None => {
                            let section = mod_rules.section(trigger).ok_or_else(|| {
                                format!("[{tag}] Trigger: no trigger section [{trigger}]")
                            })?;
                            read.triggers.push(TriggerSpec::read(trigger, section)?);
                            read.triggers.len() - 1
                        }
                    };
                    if !reached.contains(&index) {
                        reached.push(index);
                    }
                }
            }
            read.bindings.push((name.to_owned(), reached));
        }
        Ok(read)
    }
}

impl TriggerSpec {
    fn read(name: &str, section: &IniSection) -> Result<Self, String> {
        let mut events = std::collections::BTreeMap::<u32, (String, String)>::new();
        let mut clauses = Vec::<(u32, u32, String, String)>::new();
        let mut actions = std::collections::BTreeMap::<u32, (String, String)>::new();
        let mut spec = Self {
            section: name.to_owned(),
            events: Vec::new(),
            actions: Vec::new(),
            house: None,
            difficulty: 0b111,
            repeat: Repeat::OneTimeOr,
            decide: None,
        };
        for key in section.keys() {
            let fail = |why: String| format!("[{name}] {key}: {why}");
            let Some(value) = section.read_name(key, VALUE_CAPACITY) else {
                continue;
            };
            let fail_value = |why: &str| format!("[{name}] {key}={value}: {why}");
            let place = trigger_key(key).ok_or_else(|| fail("not a trigger section key".into()))?;
            let twice = || fail("given twice (a key without a number is number 1)".into());
            match place {
                TriggerKey::Event(n) => {
                    if events
                        .insert(n, (key.to_owned(), value.to_owned()))
                        .is_some()
                    {
                        return Err(twice());
                    }
                }
                TriggerKey::Where { event, number } => {
                    if clauses.iter().any(|c| (c.0, c.1) == (event, number)) {
                        return Err(twice());
                    }
                    clauses.push((event, number, key.to_owned(), value.to_owned()));
                }
                TriggerKey::Action(n) => {
                    if actions
                        .insert(n, (key.to_owned(), value.to_owned()))
                        .is_some()
                    {
                        return Err(twice());
                    }
                }
                TriggerKey::Decide => {
                    spec.decide = match parse_action(value) {
                        Ok(ActionSpec::Module {
                            source,
                            name,
                            text: None,
                            ..
                        }) => Some((source, name)),
                        _ => return Err(fail_value("a decision handler is Wasm:name")),
                    };
                }
                TriggerKey::House => spec.house = Some(value.to_owned()),
                TriggerKey::Difficulty => {
                    let mut mask = 0u8;
                    for level in value.split(',').map(str::trim) {
                        let bit = ["Easy", "Normal", "Hard"]
                            .iter()
                            .position(|d| d.eq_ignore_ascii_case(level))
                            .ok_or_else(|| fail_value("difficulties are Easy, Normal, Hard"))?;
                        mask |= 1 << bit;
                    }
                    spec.difficulty = mask;
                }
                TriggerKey::Repeat => {
                    spec.repeat = match value {
                        v if v.eq_ignore_ascii_case("OneTimeOr") => Repeat::OneTimeOr,
                        v if v.eq_ignore_ascii_case("OneTimeAnd") => Repeat::OneTimeAnd,
                        v if v.eq_ignore_ascii_case("Repeating") => Repeat::Repeating,
                        _ => {
                            return Err(fail_value("Repeat is OneTimeOr, OneTimeAnd or Repeating"));
                        }
                    };
                }
            }
        }
        let numbered = |count: usize, last: Option<u32>, what: &str, max: usize| {
            if last.is_some_and(|last| last as usize != count) {
                return Err(format!(
                    "[{name}] {what} keys must count from 1 without gaps"
                ));
            }
            if count > max {
                return Err(format!("[{name}] more than {max} {what} keys"));
            }
            Ok(())
        };
        numbered(
            events.len(),
            events.keys().last().copied(),
            "Event",
            MAX_EVENTS,
        )?;
        numbered(
            actions.len(),
            actions.keys().last().copied(),
            "Action",
            MAX_ACTIONS,
        )?;
        if events.is_empty() {
            return Err(format!("[{name}] needs an Event"));
        }
        if actions.is_empty() && spec.decide.is_none() {
            return Err(format!("[{name}] needs an Action or Decide"));
        }
        clauses.sort_by_key(|c| (c.0, c.1));
        for (n, (key, value)) in events {
            let mut event = parse_event(&value)
                .map_err(|why| format!("[{name}] {key}={value}: {why}"))?
                .with_key(key);
            for (_, _, key, value) in clauses.iter().filter(|c| c.0 == n) {
                let clause =
                    parse_clause(value).map_err(|why| format!("[{name}] {key}={value}: {why}"))?;
                event.clauses.push((key.clone(), clause));
            }
            if event.clauses.len() > MAX_CLAUSES {
                return Err(format!(
                    "[{name}] {}: more than {MAX_CLAUSES} Where keys",
                    event.key
                ));
            }
            spec.events.push(event);
        }
        if let Some((_, _, key, _)) = clauses.iter().find(|c| c.0 as usize > spec.events.len()) {
            return Err(format!("[{name}] {key}: its event does not exist"));
        }
        for (key, value) in actions.into_values() {
            let action =
                parse_action(&value).map_err(|why| format!("[{name}] {key}={value}: {why}"))?;
            spec.actions.push((format!("{key}={value}"), action));
        }
        Ok(spec)
    }
}

impl EventSpec {
    fn with_key(mut self, key: String) -> Self {
        self.key = key;
        self
    }
}

/// `Event`, `Event<n>`, `Where<n>`, `Event<k>.Where<n>`, `Action<n>` and the
/// other trigger section keys. A key without a number is number 1.
fn trigger_key(key: &str) -> Option<TriggerKey> {
    let number = |rest: &str| match rest {
        "" => Some(1),
        digits if digits.bytes().all(|b| b.is_ascii_digit()) => {
            let mut bytes = digits.as_bytes();
            scan_decimal_i32(&mut bytes)
                .and_then(|n| u32::try_from(n).ok())
                .filter(|n| *n > 0)
        }
        _ => None,
    };
    Some(match key {
        "Decide" => TriggerKey::Decide,
        "House" => TriggerKey::House,
        "Difficulty" => TriggerKey::Difficulty,
        "Repeat" => TriggerKey::Repeat,
        _ => {
            if let Some((event, clause)) = key.split_once(".Where") {
                TriggerKey::Where {
                    event: number(event.strip_prefix("Event")?)?,
                    number: number(clause)?,
                }
            } else if let Some(rest) = key.strip_prefix("Where") {
                TriggerKey::Where {
                    event: 1,
                    number: number(rest)?,
                }
            } else if let Some(rest) = key.strip_prefix("Event") {
                TriggerKey::Event(number(rest)?)
            } else {
                TriggerKey::Action(number(key.strip_prefix("Action")?)?)
            }
        }
    })
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .and_then(|_| text.get(prefix.len()..))
}

/// A whole decimal number, as one `sscanf` `%d` reads it.
fn number(text: &str) -> Result<i32, String> {
    let mut bytes = text.trim().as_bytes();
    match scan_decimal_i32(&mut bytes) {
        Some(value) if bytes.is_empty() => Ok(value),
        _ => Err(format!("{:?} is not a number", text.trim())),
    }
}

/// An empty argument is `default`.
fn number_or(text: Option<&String>, default: i32) -> Result<i32, String> {
    match text.map(|t| t.trim()) {
        None | Some("") => Ok(default),
        Some(text) => number(text),
    }
}

/// `name` or `name(a,b,...)`.
fn call(text: &str) -> Result<(&str, Vec<String>), String> {
    let text = text.trim();
    let (name, args) = match text.split_once('(') {
        None => (text, Vec::new()),
        Some((name, rest)) => {
            let inner = rest
                .strip_suffix(')')
                .ok_or_else(|| "a ( needs a closing )".to_owned())?;
            (
                name.trim(),
                inner.split(',').map(|a| a.trim().to_owned()).collect(),
            )
        }
    };
    if name.is_empty() || name.contains([' ', ')', ',']) {
        return Err(format!("{name:?} is not a name"));
    }
    Ok((name, args))
}

/// `Op` (appendix 8.3) by name or sign.
fn parse_op(text: &str) -> Result<u8, String> {
    const OPS: [(&str, &str); 6] = [
        ("Eq", "="),
        ("Ne", "!="),
        ("Lt", "<"),
        ("Le", "<="),
        ("Gt", ">"),
        ("Ge", ">="),
    ];
    OPS.iter()
        .position(|(name, sign)| name.eq_ignore_ascii_case(text) || *sign == text)
        .map(|op| op as u8)
        .ok_or_else(|| format!("{text:?} is not an Op (Eq, Ne, Lt, Le, Gt, Ge)"))
}

fn parse_event(text: &str) -> Result<EventSpec, String> {
    let (name, args) = call(text)?;
    if args.len() > 4 {
        return Err("an event takes op, interval, id and value".into());
    }
    let op = match args.first().map(|a| a.trim()) {
        None | Some("") => 0,
        Some(op) => parse_op(op)?,
    };
    Ok(EventSpec {
        key: String::new(),
        name: name.to_owned(),
        op,
        interval: number_or(args.get(1), 0)?,
        id: args
            .get(2)
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(str::to_owned),
        value: i64::from(number_or(args.get(3), 0)?),
        clauses: Vec::new(),
    })
}

fn parse_clause(text: &str) -> Result<ClauseSpec, String> {
    let at = text
        .find(['<', '>', '=', '!'])
        .ok_or_else(|| "a clause compares with =, !=, <, <=, > or >=".to_owned())?;
    let (left, right) = text.split_at(at);
    let left = left.trim();
    const SIGNS: [(&str, u8); 7] = [
        ("<=", 3),
        (">=", 5),
        ("!=", 1),
        ("==", 0),
        ("<", 2),
        (">", 4),
        ("=", 0),
    ];
    let (op, value) = SIGNS
        .iter()
        .find_map(|(sign, op)| right.strip_prefix(sign).map(|value| (*op, value)))
        .ok_or_else(|| format!("{right:?} does not start with a comparison"))?;
    let value = i64::from(number(value)?);
    let reads = if let Some(inner) = strip_prefix_ci(left, "Var(") {
        let (target, var) = inner
            .strip_suffix(')')
            .and_then(|inner| inner.split_once(','))
            .ok_or_else(|| "a variable clause is Var(target,name)".to_owned())?;
        let target = match target.trim() {
            t if t.eq_ignore_ascii_case("Global") => VarTarget::Global,
            t if t.eq_ignore_ascii_case("Subject") => VarTarget::Subject,
            t if t.eq_ignore_ascii_case("Other") => VarTarget::Other,
            t if t.eq_ignore_ascii_case("House") => VarTarget::House,
            t => return Err(format!("{t:?} is not Global, Subject, Other or House")),
        };
        Reads::Var(target, var.trim().to_owned())
    } else {
        let (whose, property) = left
            .split_once('.')
            .ok_or_else(|| "a property clause is Subject|Other|House.Property".to_owned())?;
        let whose = match whose.trim() {
            w if w.eq_ignore_ascii_case("Subject") => Whose::Subject,
            w if w.eq_ignore_ascii_case("Other") => Whose::Other,
            w if w.eq_ignore_ascii_case("House") => Whose::House,
            w => return Err(format!("{w:?} is not Subject, Other or House")),
        };
        Reads::Property(whose, property.trim().to_owned())
    };
    Ok(ClauseSpec { reads, op, value })
}

/// An `ActionTarget` by name; empty is `Subject`.
fn parse_target(text: &str) -> Result<ActionTarget, String> {
    if text.is_empty() {
        return Ok(ActionTarget::Subject);
    }
    [
        ("Subject", ActionTarget::Subject),
        ("Other", ActionTarget::Other),
        ("TagHolder", ActionTarget::TagHolder),
        ("Projectile", ActionTarget::Projectile),
        ("House", ActionTarget::House),
        ("Cell", ActionTarget::Cell),
    ]
    .into_iter()
    .find(|(n, _)| n.eq_ignore_ascii_case(text))
    .map(|(_, target)| target)
    .ok_or_else(|| format!("{text:?} is not an ActionTarget"))
}

fn parse_action(text: &str) -> Result<ActionSpec, String> {
    if let Some(call) = strip_prefix_ci(text.trim(), "Wasm:") {
        // `name(target,text)`: the text runs to the last `)` and keeps its
        // commas.
        let (name, target, text) = match call.split_once('(') {
            None => (call.trim(), ActionTarget::Subject, None),
            Some((name, rest)) => {
                let inner = rest
                    .trim_end()
                    .strip_suffix(')')
                    .ok_or_else(|| "a ( needs a closing )".to_owned())?;
                let (target, text) = inner.split_once(',').unwrap_or((inner, ""));
                (
                    name.trim(),
                    parse_target(target.trim())?,
                    Some(text.trim().to_owned()),
                )
            }
        };
        let (source, name) = match (strip_prefix_ci(name, "mod."), strip_prefix_ci(name, "map.")) {
            (Some(rest), _) => (Some(ModuleSource::Mod), rest),
            (_, Some(rest)) => (Some(ModuleSource::Map), rest),
            _ => (None, name),
        };
        if name.is_empty() || name.contains([' ', ',', ')']) {
            return Err("Wasm: needs a module action name".into());
        }
        return Ok(ActionSpec::Module {
            source,
            name: name.to_owned(),
            target,
            text,
        });
    }
    let (name, mut args) = call(text)?;
    let target = parse_target(args.first().map_or("", String::as_str))?;
    if !args.is_empty() {
        args.remove(0);
    }
    Ok(ActionSpec::Kind {
        name: name.to_owned(),
        target,
        args,
    })
}

/// The parameters of action `kind` in appendix 5.2/5.3 table order, without
/// `other` (a trigger's comes from its event). `None` for kinds INI cannot
/// write (`CreateProjectile` is code only; `Module` is `Wasm:name`).
fn action_params(kind: i32) -> Option<&'static [Param]> {
    use IdKind::*;
    use Param::*;
    if kind >= vera20k_wasm_ext::FIRST_ATOMIC_ACTION {
        let name = vera20k_wasm_ext::action_kind_name(kind).flatten()?;
        return Some(match name {
            "FireWeapon" => &[Id(Weapon), Value, Cell],
            "LaunchSuperWeapon" => &[Id(SuperWeapon), Cell],
            "SpawnAnim" | "DetachAnim" => &[Id(Anim)],
            "AttachAnim" => &[Id(Anim), Value2],
            "Damage" | "DetonateWarhead" => &[Id(Warhead), Value],
            "Heal" | "AddExperience" | "AddMoney" | "AddAmmo" | "SetScore" | "AddScore" => &[Value],
            "CreateObject" => &[Id(TechnoType), House, Enum(EnumKind::Mission)],
            "SetMission" => &[Enum(EnumKind::Mission), Cell],
            "Teleport" => &[Cell, Enum(EnumKind::Blocked)],
            "AllowType" => &[Id(TechnoType), Value],
            "SetOwner" => &[House],
            "SetProperty" => &[Id(Property), Value],
            "Modify" => &[
                Id(Property),
                Enum(EnumKind::ModifierOp),
                Arg2,
                Value,
                Value2,
            ],
            "RemoveModifiers" => &[Arg2, Id(Property)],
            "SetVar" => &[Id(Variable), Value],
            "DetonateWeapon" => &[Id(Weapon)],
            "EndFor" => &[Enum(EnumKind::Outcome)],
            "Log" => &[Text],
            "Convert" => &[Id(TechnoType), Value2],
            "Push" => &[Cell, Value, Value2],
            "SetFacing" => &[Value],
            "CreateTeamUnlimited" => &[Id(TeamType)],
            "RaiseEvent" => &[Id(ModuleEvent), Value, Value2],
            "CreateProjectile" | "Module" => return None,
            _ => &[],
        });
    }
    Some(match kind {
        1 | 2 | 11 | 12 | 20 | 21 | 22 | 53 | 54 | 70 | 103 => &[Text],
        4 | 5 | 7 | 80 | 107 => &[Id(TeamType)],
        10
        | 25..=30
        | 35
        | 44
        | 48
        | 52
        | 55..=58
        | 71..=73
        | 76..=79
        | 84..=87
        | 100
        | 105
        | 108
        | 110
        | 114
        | 117
        | 127
        | 139
        | 142..=144
        | 146
        | 148
        | 149 => &[Value],
        14 | 36..=38 | 123 | 124 => &[House],
        19 | 98 | 99 => &[Id(Sound)],
        41 => &[Id(Anim)],
        43 => &[Id(VoxelAnim)],
        88 => &[Id(ParticleSystem)],
        42 => &[Id(Weapon)],
        33 | 34 | 133 | 134 => &[Id(SuperWeapon)],
        40 => &[Value, Value2],
        104 => &[Id(TeamType), Value],
        106 | 115 | 131 => &[Id(TechnoType), Value],
        125 => &[Id(TechnoType)],
        128 | 135 | 137 | 140 => &[Cell],
        129 | 132 => &[Id(SuperWeapon), Value],
        0..=149 => &[],
        _ => return None,
    })
}

/// A waypoint reference (kind 3).
fn waypoint_ref(text: &str) -> Result<i64, String> {
    let number = number(text)?;
    if number < 0 {
        return Err(format!("waypoint {number}"));
    }
    Ok((REF_WAYPOINT << REF_KIND_SHIFT) | i64::from(number))
}

impl Simulation {
    /// Builds the INI triggers into the module trigger registry and binds
    /// them to their TechnoTypes (module docs). Runs once the scenario's
    /// modules are installed; an error names the section and key, and the
    /// match must not start.
    pub(crate) fn install_ini_triggers(
        &mut self,
        rules: &RuleSet,
        ini: &IniTriggers,
    ) -> Result<(), String> {
        let mut types = Vec::with_capacity(ini.bindings.len());
        for (section, _) in &ini.bindings {
            types.push(bound_type(rules, section).ok_or_else(|| {
                format!(
                    "[{section}] Wasm.Tags: [{section}] is not a TechnoType, weapon, warhead \
                     or projectile"
                )
            })?);
        }
        for (index, spec) in ini.triggers.iter().enumerate() {
            let Some((source, def)) = self.resolve_ini_trigger(rules, spec)? else {
                continue;
            };
            let section = &spec.section;
            let created = self.with_module_host(rules, source, |host| host.create_trigger(&def));
            let handle = match created {
                Ok((handle, Status::Ok)) => handle,
                Ok((_, status)) => {
                    return Err(format!("[{section}] the trigger answers {status:?}"));
                }
                Err(invalid) => return Err(format!("[{section}] {}", invalid.0)),
            };
            self.trigger_runtime
                .module_triggers_mut()
                .mark_ini(handle as u64);
            let bound = ini
                .bindings
                .iter()
                .zip(&types)
                .filter(|((_, reached), _)| reached.contains(&index));
            for ((type_section, _), &reference) in bound {
                match self.with_module_host(rules, source, |host| {
                    host.bind_trigger(handle, reference, true)
                }) {
                    Ok(Status::Ok) => {}
                    Ok(Status::Unsupported) if reference != bound_techno(rules, type_section) => {
                        return Err(format!(
                            "[{type_section}] Wasm.Tags: this game cannot bind a trigger to \
                             this kind of section yet; bind [{section}] to TechnoTypes, weapons \
                             or warheads"
                        ));
                    }
                    Ok(status) => {
                        return Err(format!(
                            "[{type_section}] Wasm.Tags: binding [{section}] answers {status:?}"
                        ));
                    }
                    Err(invalid) => {
                        return Err(format!("[{type_section}] Wasm.Tags: {}", invalid.0));
                    }
                }
            }
        }
        Ok(())
    }

    /// One trigger section as a `TriggerDef` and the module its `Wasm:`
    /// actions address; `None` when a house it names has no house here.
    fn resolve_ini_trigger(
        &self,
        rules: &RuleSet,
        spec: &TriggerSpec,
    ) -> Result<Option<(ModuleSource, TriggerDef)>, String> {
        let section = &spec.section;
        let decide = spec
            .decide
            .as_ref()
            .map(|(source, name)| (source.unwrap_or(ModuleSource::Mod), name.as_str()));
        let modules: Vec<ModuleSource> = spec
            .actions
            .iter()
            .filter_map(|(_, action)| match action {
                ActionSpec::Module { source, .. } => Some(source.unwrap_or(ModuleSource::Mod)),
                ActionSpec::Kind { .. } => None,
            })
            .chain(decide.map(|(source, _)| source))
            .collect();
        let source = modules.first().copied().unwrap_or(ModuleSource::Mod);
        if modules.iter().any(|s| *s != source) {
            return Err(format!(
                "[{section}] its Wasm: actions must all address one module"
            ));
        }
        let house = match &spec.house {
            None => 0,
            Some(country) => match self.ini_house(rules, country) {
                Ok(Some(house)) => house,
                Ok(None) => {
                    log::info!(
                        target: "ext_module",
                        "INI trigger [{section}] left out: no {country} house in this scenario"
                    );
                    return Ok(None);
                }
                Err(why) => return Err(format!("[{section}] House={country}: {why}")),
            },
        };
        let mut events = Vec::with_capacity(spec.events.len());
        for event in &spec.events {
            let fail = |why: String| format!("[{section}] {}: {why}", event.key);
            // `Event=Wasm:name`: a registered module event (INI
            // RFC 0001 section 6h).
            if let Some(name) = strip_prefix_ci(&event.name, "Wasm:") {
                let id = self
                    .ext_modules
                    .ini
                    .event_id(name.trim())
                    .ok_or_else(|| fail(format!("no module registered the event {name}")))?;
                let mut clauses = Vec::with_capacity(event.clauses.len());
                for (key, clause) in &event.clauses {
                    clauses.push(
                        self.resolve_clause(rules, clause)
                            .map_err(|why| format!("[{section}] {key}: {why}"))?,
                    );
                }
                events.push(EventDef {
                    kind: vera20k_wasm_ext::triggers::EVENT_MODULE,
                    op: 0,
                    interval: 0,
                    id,
                    value: 0,
                    clauses,
                });
                continue;
            }
            let kind = vera20k_wasm_ext::event_kind_id(&event.name)
                .ok_or_else(|| fail(format!("no event kind {}", event.name)))?;
            if !super::events::raises(rules, kind)
                && !vera20k_wasm_ext::decide::DECISION_ONLY_EVENTS.contains(&kind)
            {
                return Err(fail(format!("this game does not raise {} yet", event.name)));
            }
            let id = match event.id.as_deref() {
                None => -1,
                Some(text) => self.ini_event_id(kind, text).map_err(fail)?,
            };
            let mut clauses = Vec::with_capacity(event.clauses.len());
            for (key, clause) in &event.clauses {
                clauses.push(
                    self.resolve_clause(rules, clause)
                        .map_err(|why| format!("[{section}] {key}: {why}"))?,
                );
            }
            events.push(EventDef {
                kind,
                op: event.op,
                interval: event.interval,
                id,
                value: event.value,
                clauses,
            });
        }
        let mut actions = Vec::with_capacity(spec.actions.len());
        for (key, action) in &spec.actions {
            match self.resolve_action(rules, source, action) {
                Ok(Some(action)) => actions.push(action),
                Ok(None) => {
                    log::info!(
                        target: "ext_module",
                        "INI trigger [{section}] left out: {key} names a country without a house"
                    );
                    return Ok(None);
                }
                Err(why) => return Err(format!("[{section}] {key}: {why}")),
            }
        }
        let decide_key = match decide {
            Some((source, name)) => self
                .module_action_key(source, name)
                .map_err(|why| format!("[{section}] Decide: {why}"))?,
            None => -1,
        };
        Ok(Some((
            source,
            TriggerDef {
                house,
                repeat: spec.repeat,
                difficulty: spec.difficulty,
                flags: 0,
                decide_key,
                events,
                actions,
            },
        )))
    }

    /// A country's house reference; `None` when the scenario has no house of
    /// it.
    fn ini_house(&self, rules: &RuleSet, country: &str) -> Result<Option<i64>, String> {
        let index = rules
            .country_index(country.trim())
            .ok_or_else(|| format!("no country {}", country.trim()))?;
        Ok(self
            .house_by_country_index(index, rules)
            .and_then(|house| self.houses.index_of(house))
            .map(house_ref))
    }

    /// An event's `id`: a number, or by event kind a property name
    /// (`PropertyPoll`) or a variable name (`VariablePoll`).
    fn ini_event_id(&self, kind: i32, text: &str) -> Result<i32, String> {
        match kind {
            EVENT_VARIABLE_POLL => self.ini_variable(text),
            _ => match number(text) {
                Ok(number) => Ok(number),
                Err(why) if kind != EVENT_PROPERTY_POLL => Err(why),
                Err(_) => {
                    vera20k_wasm_ext::property_id(text).ok_or_else(|| format!("no property {text}"))
                }
            },
        }
    }

    /// A variable name (the mod's `vera.vars`, `map.` for the map's) or a
    /// Red Alert 2 variable number.
    fn ini_variable(&self, name: &str) -> Result<i32, String> {
        if let Ok(number) = number(name) {
            return match LOCAL_VARS.contains(&number) || GLOBAL_VARS.contains(&number) {
                true => Ok(number),
                false => Err(format!("no variable {number}")),
            };
        }
        self.ext_modules
            .vars
            .lookup(ModuleSource::Mod, name)
            .ok_or_else(|| format!("no declared variable {name}"))
    }

    fn resolve_clause(&self, rules: &RuleSet, clause: &ClauseSpec) -> Result<FilterClause, String> {
        let subject = match &clause.reads {
            Reads::Var(target, name) => FilterSubject::Var {
                var: self.ini_variable(name)?,
                target: *target,
            },
            Reads::Property(whose, name) => {
                let prop = vera20k_wasm_ext::property_id(name)
                    .ok_or_else(|| format!("no property {name}"))?;
                if !super::has_kind(rules, TABLE_PROPERTY, prop) {
                    return Err(format!("this game does not read {name} yet"));
                }
                match whose {
                    Whose::Subject => FilterSubject::Subject(prop),
                    Whose::Other => FilterSubject::Other(prop),
                    Whose::House => FilterSubject::House(prop),
                }
            }
        };
        Ok(FilterClause {
            subject,
            op: clause.op,
            value: clause.value,
        })
    }

    /// The dispatch key of `name` in module `source`'s `vera.actions`.
    fn module_action_key(&self, source: ModuleSource, name: &str) -> Result<i32, String> {
        let slot = self
            .ext_modules
            .slots
            .iter()
            .find(|slot| slot.source == source)
            .ok_or_else(|| match source {
                ModuleSource::Mod => "the mod has no rules.wasm".to_owned(),
                ModuleSource::Map => "the map has no rules module".to_owned(),
            })?;
        slot.module
            .action_key(name)
            .ok_or_else(|| format!("the {source:?} module's vera.actions has no {name}"))
    }

    /// One action as an `ActionDef`; `None` when a `house` parameter names a
    /// country without a house here.
    fn resolve_action(
        &self,
        rules: &RuleSet,
        source: ModuleSource,
        action: &ActionSpec,
    ) -> Result<Option<ActionDef>, String> {
        let (name, target, args) = match action {
            ActionSpec::Module {
                name, target, text, ..
            } => {
                let key = self.module_action_key(source, name)?;
                return Ok(Some(ActionDef {
                    kind: ACTION_MODULE,
                    target: *target,
                    params: ActionParams {
                        id: key,
                        ..ActionParams::default()
                    },
                    text: text.clone().unwrap_or_default(),
                }));
            }
            ActionSpec::Kind { name, target, args } => (name, *target, args),
        };
        let kind = vera20k_wasm_ext::action_kind_id(name)
            .ok_or_else(|| format!("no action kind {name}"))?;
        if kind == ACTION_MODULE {
            return Err("write a module action as Wasm:name".into());
        }
        if !super::actions::has_action(kind, rules) {
            return Err(format!("this game does not run {name} yet"));
        }
        let schema = action_params(kind).ok_or_else(|| format!("{name} is not written in INI"))?;
        let mut params = ActionParams::default();
        let mut text = String::new();
        let mut args = args.iter().map(String::as_str);
        if target == ActionTarget::Cell {
            params.cell = waypoint_ref(args.next().unwrap_or_default())
                .map_err(|why| format!("target Cell takes a waypoint first: {why}"))?;
        }
        let args: Vec<&str> = args.collect();
        if args.len() > schema.len() {
            return Err(format!("{name} takes {} parameters", schema.len()));
        }
        for (param, arg) in schema.iter().zip(args) {
            if arg.is_empty() {
                continue;
            }
            match param {
                Param::Id(kind) => params.id = self.ini_id(rules, *kind, arg)?,
                Param::Enum(kind) => {
                    params.arg = match number(arg) {
                        Ok(number) => number,
                        Err(_) => kind
                            .value(arg)
                            .ok_or_else(|| format!("{arg:?} is not a {kind:?}"))?,
                    };
                }
                Param::Arg2 => params.arg2 = number(arg)?,
                Param::Value => params.value = i64::from(number(arg)?),
                Param::Value2 => params.value2 = i64::from(number(arg)?),
                Param::Cell => params.cell = waypoint_ref(arg)?,
                Param::House => match self.ini_house(rules, arg)? {
                    Some(house) => params.house = house,
                    None => return Ok(None),
                },
                Param::Text => text = arg.to_owned(),
            }
        }
        Ok(Some(ActionDef {
            kind,
            target,
            params,
            text,
        }))
    }

    /// An `id` parameter: a number, or a name of `kind`.
    fn ini_id(&self, rules: &RuleSet, kind: IdKind, text: &str) -> Result<i32, String> {
        if kind == IdKind::Variable {
            return self.ini_variable(text);
        }
        if kind == IdKind::ModuleEvent {
            return self
                .ext_modules
                .ini
                .event_id(text)
                .ok_or_else(|| format!("no module registered the event {text}"));
        }
        if let Ok(number) = number(text) {
            return Ok(number);
        }
        let rules_name = |kind| super::rules_name_id(rules, kind, text).flatten();
        let found = match kind {
            IdKind::Sound | IdKind::Variable | IdKind::ModuleEvent => None,
            IdKind::TechnoType => rules_name(NAME_TECHNO_TYPE),
            IdKind::Weapon => rules_name(NAME_WEAPON),
            IdKind::Warhead => rules_name(NAME_WARHEAD),
            IdKind::SuperWeapon => rules_name(NAME_SUPER_WEAPON),
            IdKind::Anim => rules_name(NAME_ANIM),
            IdKind::VoxelAnim => rules_name(NAME_VOXEL_ANIM),
            IdKind::ParticleSystem => rules_name(NAME_PARTICLE_SYSTEM),
            IdKind::TeamType => self
                .stock_definition(crate::sim::trigger_runtime::StockReferenceKind::Team, text)
                .and_then(|index| i32::try_from(index).ok()),
            IdKind::Property => vera20k_wasm_ext::property_id(text),
        };
        found.ok_or_else(|| match kind {
            IdKind::Sound => format!(
                "{text:?} is not a number; give a sound as its soundmd.ini [SoundList] index"
            ),
            _ => format!("no {kind:?} {text}"),
        })
    }
}

/// What `Wasm.Tags` on `section` binds: the TechnoType of that name, else
/// the weapon, warhead or projectile (module docs); `None` for none of them.
fn bound_type(rules: &RuleSet, section: &str) -> Option<i64> {
    if let Some(handle) = rules.type_handle(section) {
        return Some(type_ref(NAME_TECHNO_TYPE, handle.0));
    }
    [
        (NAME_WEAPON, rules.weapon_index(section)),
        (NAME_WARHEAD, rules.warhead_index(section)),
        (NAME_PROJECTILE, rules.projectile_index(section)),
    ]
    .into_iter()
    .find_map(|(kind, id)| Some(type_ref(kind, id?)))
}

/// `section`'s TechnoType reference, or 0.
fn bound_techno(rules: &RuleSet, section: &str) -> i64 {
    rules
        .type_handle(section)
        .map_or(0, |handle| type_ref(NAME_TECHNO_TYPE, handle.0))
}
