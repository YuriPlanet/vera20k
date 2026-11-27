//! Client reads (`client_get`, `client_get_many`, `client_find`,
//! `client_var_get`, `client_lookup`, `client_name_of`,
//! `client_has_kind`): only what this player's own interface can show
//! (RFC 10, "Client reads"; appendix 7 "client" column, 8.2, RFC 6e).
//!
//! The values come from the simulation's own readers
//! ([`crate::sim::ext_modules::Reader`]); this module only decides what may
//! be read, and `find` asks it through [`ReadGate`]. It is the anti-maphack
//! boundary, so it errs on the side of hiding:
//!
//! | Reference | This player | Anyone else |
//! | --- | --- | --- |
//! | Own house | every `all` and `own` property | — |
//! | Other house (allies too) | — | `Name`, `Country`, `Side`, `Index`, `IsDefeated`, `IsHuman`, `IsCivilian`, `Relation`; the rest `Refused` |
//! | Own object, own superweapon | every `all`, `own`, `seen` property | — |
//! | Other object seen now | — | `Type`, `Owner`, `Category`, `Health`, `Strength`, `HealthPercent`, `X`, `Y`, `Z`, `Cell`, `UnderConstruction`, `DoorOpen`, `Animating`, tint, laser trail and `ImageOverride`; the rest `Refused` |
//! | Other object not seen now | — | `Gone` (and `find` skips it) |
//! | Other house's superweapon | — | `Refused` |
//! | Cell | `LandType`, `Height`, `Waypoint`, `Zone.*` always; `Overlay`, `Ore`, `Obstructed`, `Passable.*` in current sight; `Revealed`, `Placeable` for the own house | as for this player |
//! | Type, global rules | everything (viewpoint: the own house) | |
//!
//! `sim` properties (`Team`, `Tag`, `ControlDomains`, `AiTriggersActive`,
//! `CellTag`, projectile `Data0`/`Data1`) and the sim-only objects (modifiers,
//! triggers, tags, teams) answer an empty value and `Refused`, and so do the
//! sim-only `find` scopes. `Selected` and `ControlGroup` are the client's
//! own ([`LocalFacts`]), for any object this player sees.
//!
//! What this player sees is what its screen shows, read from the same owner
//! the renderer, the pickers and the simulation's `Visible` use:
//! `Simulation::object_seen_by` (fog and shroud through
//! `FogState::shows_objects_to`, current sight under FogOfWar; cloaking,
//! sensors and `Invisible=` through the screen GetVisualState). Own objects
//! always exist for their player, in a transport too. A disguised enemy
//! (Infantry or Unit that `GameEntity::is_disguised_to` this player, as the
//! screen and the tooltip ask) reads as its disguise: `Type` and `Category`
//! of the worn type, `Owner` the worn house (`GameEntity::disguise_house`),
//! `Strength` the worn type's and `Health` the real `HealthPercent` of that;
//! one worn as terrain (no worn TechnoType or no worn house) is no object
//! at all. A spectator has no house: with `Viewer::sees_whole_map` every
//! object and cell is in sight, as itself, else none is.
//!
//! Variables follow their declared visibility (RFC 6e): `Public` for
//! everyone; `Private` for the holder house's player (an object's: its
//! owner's); `Allies` also for players mutually allied with that house;
//! `InSight` also for anyone who sees the object now. A global variable
//! that is not `Public`, and every Red Alert 2 variable, is `Refused`.
//!
//! Residuals:
//! - The runtime the app lends a client is `SimHost::runtime`, which while
//!   the simulation ticks elsewhere is the presentation copy: it has no
//!   objects, so every object reads `Gone` there (see the report on the
//!   glue's side).
//! - A `find` page's resume key (`next`) after a scan limit may be the
//!   serial of an object this player cannot see; it tells nothing of its
//!   kind, owner or place.
//! - A spectator in a room that shows a house's details is not modelled:
//!   spectators read no house's `own` properties.

use vera20k_wasm_ext::client::{client_action, client_event, node_kind};
use vera20k_wasm_ext::{
    Clause, ClauseSubject, FindPage, Invalid, NAME_ACTION_KIND, NAME_KINDS, NAME_PROPERTY,
    NAME_TECHNO_TYPE, Op, Query, SCOPE_MODIFIERS, SCOPE_OBJECTS, Status, TABLE_NAME_KIND,
    TABLE_PROPERTY, Visibility, action_kind_id, action_kind_name, property_id, property_name,
};

use vera20k_wasm_ext::Source;
use crate::map::entities::EntityCategory;
use super::vars::FIRST_DECLARED_VAR;
use super::{
    ModuleSource, ReadGate, Reader, Ref, View, decode_ref, house_ref, lookup_name, name_of_id,
    techno_type, type_ref,
};
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::projectile::Projectile;
use crate::sim::runtime::SimRuntime;
use crate::sim::world::Simulation;

/// Whose view a client module reads with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewer {
    /// This player's house handle (reference kind 0); 0 for a spectator.
    pub house: i64,
    pub spectator: bool,
    /// A spectator in a room that shows the whole map.
    pub sees_whole_map: bool,
}

/// Name kinds the glue routes elsewhere (appendix 8.1): `Notify` to the
/// simulation's declared notify table, `Hotkey` to the module's own.
use vera20k_wasm_ext::{NAME_HOTKEY, NAME_NOTIFY};
/// `find` scopes that are the simulation's only (appendix 8.2): `Teams`,
/// `Triggers`, `Tags` and `Modifiers`.
const SIM_SCOPES: [i32; 4] = [4, 5, 6, SCOPE_MODIFIERS];
/// `has_kind` tables the client adds (appendix 8.3 `Table`).
const TABLE_NODE_KIND: i32 = 5;
const TABLE_CLIENT_EVENT: i32 = 6;
const TABLE_CLIENT_ACTION: i32 = 7;

/// Object properties another house's seen object answers (appendix 7.1, 7.2
/// "seen"); every other object property is the owner's (`own`) unless
/// listed in [`SIM_OBJECT`].
const SEEN_OBJECT: &[&str] = &[
    "Type",
    "Owner",
    "Category",
    "Health",
    "Strength",
    "HealthPercent",
    "X",
    "Y",
    "Z",
    "Cell",
    "UnderConstruction",
    "DoorOpen",
    "Animating",
    "TintColor",
    "TintIntensity",
    "LaserTrailType",
    "ImageOverride",
];
const SIM_OBJECT: &[&str] = &["Team", "Tag"];
/// Projectile properties (appendix 7.4): `seen` and `sim`; the rest `own`.
const SEEN_PROJECTILE: &[&str] = &["Type", "Category", "X", "Y", "Z", "Cell"];
const SIM_PROJECTILE: &[&str] = &["Data0", "Data1"];
/// House properties (appendix 7.5): public and `sim`; the rest `own`.
const PUBLIC_HOUSE: &[&str] = &[
    "Name",
    "Country",
    "Side",
    "Index",
    "IsDefeated",
    "IsHuman",
    "IsCivilian",
    "Relation",
];
const SIM_HOUSE: &[&str] = &["AiTriggersActive", "ControlDomains"];
/// Cell properties (appendix 7.6): public (with every `Zone.*`), the own
/// house's viewpoint ones and `sim`; the rest need the cell in sight.
const PUBLIC_CELL: &[&str] = &["LandType", "Height", "Waypoint"];
const OWN_CELL: &[&str] = &["Revealed", "Placeable"];
const SIM_CELL: &[&str] = &["CellTag"];
/// Properties only the client has: object ones answered here from
/// [`LocalFacts`], node and client-global ones (appendix 7.10, v1) by the
/// tree and local state.
const CLIENT_PROPERTIES: &[&str] = &[
    "Selected",
    "ControlGroup",
    "Name",
    "Label",
    "Art",
    "X",
    "Y",
    "Width",
    "Height",
    "Hold",
    "Gap",
    "Visible",
    "Enabled",
    "Dismissable",
    "Clickable",
    "Hoverable",
    "Items",
    "ItemTooltips",
    "Fill",
    "Backdrop",
    "Cell",
    "Viewer",
    "IsSpectator",
    "CaptureMapClick",
];

/// What a hidden property reads: an empty value and `Refused`.
const HIDDEN: (i64, Status) = (0, Status::Refused);

/// Local facts a read needs beside the world.
#[derive(Default, Clone, Copy)]
pub struct LocalFacts<'a> {
    /// This player's selection (`Selected`).
    pub selected: &'a [u64],
    /// (object, control group) pairs (`ControlGroup`).
    pub control_groups: &'a [(u64, i32)],
}

/// One client's view of the world for the duration of a call.
#[derive(Clone, Copy)]
pub struct ClientView<'a> {
    pub runtime: &'a SimRuntime,
    pub viewer: Viewer,
    pub local: LocalFacts<'a>,
    /// The calling module's source: variable names without a `mod.` or
    /// `map.` prefix are its own (RFC 2, "Names").
    pub source: Source,
}

/// This player's house: array index and id; `None` for a spectator.
#[derive(Clone, Copy)]
struct Me {
    index: Option<usize>,
    id: Option<InternedId>,
}

impl ClientView<'_> {
    fn sim(&self) -> &Simulation {
        &self.runtime.simulation
    }

    /// The simulation's reader with no gate: for the values, once this
    /// module decided they may be read.
    fn reader(&self) -> Reader<'_> {
        let rules = &self.runtime.resources.rules;
        Reader {
            sim: &self.runtime.simulation,
            rules,
            overlay_registry: Some(rules.overlay_registry()),
            gate: None,
        }
    }

    fn me(&self) -> Me {
        let none = Me {
            index: None,
            id: None,
        };
        if self.viewer.spectator {
            return none;
        }
        let Ok(Some(Ref::House(index))) = decode_ref(self.viewer.house) else {
            return none;
        };
        match self.sim().houses.order().get(index) {
            Some(&id) => Me {
                index: Some(index),
                id: Some(id),
            },
            None => none,
        }
    }

    /// The viewpoint this player's reads use: its own house (RFC 10).
    fn own_view(&self) -> View {
        View {
            house: self.me().index,
            object: None,
            placing: None,
        }
    }

    /// Whether this player has cell (`x`, `y`) in sight now: as its screen
    /// shows other houses' objects there (`FogState::shows_objects_to`,
    /// current sight under FogOfWar, not under a hostile gap).
    fn cell_in_sight(&self, x: u16, y: u16) -> bool {
        let sim = self.sim();
        match self.me().id {
            Some(viewer) => {
                sim.fog
                    .shows_objects_to(viewer, x, y, sim.session.game_options().fog_of_war)
            }
            None => self.viewer.spectator && self.viewer.sees_whole_map,
        }
    }

    /// Whether `find(Cells)` lists cell (`x`, `y`): one this player's screen
    /// draws, explored and not under a hostile gap.
    fn cell_listed(&self, x: u16, y: u16) -> bool {
        match self.me().id {
            Some(viewer) => self.sim().fog.shows_objects_to(viewer, x, y, false),
            None => self.viewer.spectator && self.viewer.sees_whole_map,
        }
    }

    /// Whether `a` and `b` count each other as allies.
    fn mutually_allied(&self, a: InternedId, b: InternedId) -> bool {
        let sim = self.sim();
        let alliances = &sim.house_alliances;
        alliances.is_allied(&sim.interner, a, b) && alliances.is_allied(&sim.interner, b, a)
    }
}

/// How this player sees an object of another house.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sight {
    /// Not at all: under shroud or fog, cloaked, in limbo, or worn as
    /// terrain. It does not exist for the reader.
    Unseen,
    AsItself,
    /// As its disguise: the worn TechnoType ID (`None`: the screen shows
    /// the real type) and the worn house's index.
    Disguised {
        worn: Option<u32>,
        house: usize,
    },
}

/// How this player sees `entity`, an object of another house (or any
/// object, for a spectator).
fn sight(view: &ClientView<'_>, entity: &GameEntity) -> Sight {
    let sim = view.sim();
    let rules = &view.runtime.resources.rules;
    let Some(viewer) = view.me().id else {
        let shown = view.viewer.spectator
            && view.viewer.sees_whole_map
            && entity.lifecycle.object_alive()
            && !entity.lifecycle.in_limbo();
        return if shown {
            Sight::AsItself
        } else {
            Sight::Unseen
        };
    };
    if !sim.object_seen_by(rules, entity, viewer) {
        return Sight::Unseen;
    }
    let disguised = matches!(
        entity.category(),
        EntityCategory::Infantry | EntityCategory::Unit
    ) && entity.is_disguised_to(
        viewer,
        Some(&sim.fog),
        Some(&sim.house_alliances),
        &sim.interner,
    );
    if !disguised {
        return Sight::AsItself;
    }
    let Some(house) = entity
        .disguise_house(true, Some(viewer), false)
        .and_then(|house| sim.houses.index_of(house))
    else {
        return Sight::Unseen;
    };
    match sim.forced_disguise_type(entity.stable_id(), rules) {
        None => Sight::Disguised { worn: None, house },
        Some(worn) => match sim.interner.type_handle(worn, rules) {
            Some(handle) => Sight::Disguised {
                worn: Some(handle.0),
                house,
            },
            // Worn as terrain or an overlay: not an object on screen.
            None => Sight::Unseen,
        },
    }
}

/// Whether `entity` exists for this player: its own, or seen now.
fn object_exists(view: &ClientView<'_>, entity: &GameEntity) -> bool {
    view.me().id == Some(entity.owner()) || sight(view, entity) != Sight::Unseen
}

/// The house a projectile belongs to: its source object's owner.
fn projectile_owner(sim: &Simulation, projectile: &Projectile) -> Option<InternedId> {
    sim.entities()
        .get(projectile.source_id())
        .map(GameEntity::owner)
}

/// Whether `projectile` exists for this player: its own or an ally's, or
/// over a cell in sight.
fn projectile_exists(view: &ClientView<'_>, projectile: &Projectile) -> bool {
    let sim = view.sim();
    let owner = projectile_owner(sim, projectile);
    if let (Some(me), Some(owner)) = (view.me().id, owner)
        && (me == owner || sim.house_alliances.is_allied(&sim.interner, me, owner))
    {
        return true;
    }
    let position = projectile.position();
    match (
        u16::try_from(position.x / 256),
        u16::try_from(position.y / 256),
    ) {
        (Ok(x), Ok(y)) => view.cell_in_sight(x, y),
        _ => false,
    }
}

/// Whether this player can see object `handle` now (fog, shroud,
/// cloak/detection; spectators per room). Its own objects always count as
/// seen; a disguise worn as terrain does not.
pub fn sees_object(view: &ClientView<'_>, handle: i64) -> bool {
    let Ok(Some(Ref::Object(id))) = decode_ref(handle) else {
        return false;
    };
    let sim = view.sim();
    match sim.entities().get(id) {
        Some(entity) => object_exists(view, entity),
        None => sim
            .projectiles
            .get(id)
            .is_some_and(|projectile| projectile_exists(view, projectile)),
    }
}

/// `client_get` for simulation references (kinds 0..4).
pub fn get(
    view: &ClientView<'_>,
    reference: i64,
    prop: i32,
) -> Result<(i64, Status), Invalid> {
    read_gated(view, reference, prop, view.own_view())
}

/// A property as this player may read it, viewpoint properties for `at`
/// (always this player's house; a cells query adds its type).
fn read_gated(
    view: &ClientView<'_>,
    reference: i64,
    prop: i32,
    at: View,
) -> Result<(i64, Status), Invalid> {
    let reader = view.reader();
    let name = property_name(prop).unwrap_or_default();
    let Some(target) = decode_ref(reference)? else {
        return Ok((0, Status::Gone));
    };
    let sim = view.sim();
    let me = view.me();
    Ok(match target {
        Ref::Object(id) => {
            if let Some(entity) = sim.entities().get(id) {
                return object_read(view, entity, reference, prop, name, at);
            }
            if let Some(projectile) = sim.projectiles.get(id) {
                let own = me.id.is_some() && projectile_owner(sim, projectile) == me.id;
                if !own && !projectile_exists(view, projectile) {
                    return Ok((0, Status::Gone));
                }
                return Ok(
                    if SIM_PROJECTILE.contains(&name) || !own && !SEEN_PROJECTILE.contains(&name) {
                        HIDDEN
                    } else {
                        reader.read_in(reference, prop, at)?
                    },
                );
            }
            // Modifiers and module triggers are the simulation's only.
            let sim_only = sim.ext_modules.modifiers().get(id).is_some()
                || sim.trigger_runtime.module_triggers().get(id).is_some();
            if sim_only { HIDDEN } else { (0, Status::Gone) }
        }
        Ref::House(index) => {
            if sim.houses.order().get(index).is_none() {
                (0, Status::Gone)
            } else if PUBLIC_HOUSE.contains(&name) {
                reader.read_in(reference, prop, at)?
            } else if SIM_HOUSE.contains(&name) || me.index != Some(index) {
                HIDDEN
            } else {
                reader.read_in(reference, prop, at)?
            }
        }
        // Superweapons are `own` (appendix 7.9).
        Ref::SuperWeapon { house, .. } if me.index == Some(house) => {
            reader.read_in(reference, prop, at)?
        }
        Ref::SuperWeapon { .. } => HIDDEN,
        Ref::Type { .. } | Ref::Global => reader.read_in(reference, prop, at)?,
        Ref::Cell { x, y } => cell_read(view, (x, y), reference, prop, name, at)?,
        Ref::Waypoint(number) => match reader.waypoint_cell(number) {
            Some(cell) => cell_read(view, cell, reference, prop, name, at)?,
            None => (0, Status::NotFound),
        },
    })
}

/// An object's property for this player (module docs).
fn object_read(
    view: &ClientView<'_>,
    entity: &GameEntity,
    reference: i64,
    prop: i32,
    name: &str,
    at: View,
) -> Result<(i64, Status), Invalid> {
    let id = entity.stable_id();
    let own = view.me().id == Some(entity.owner());
    let seen = if own {
        Sight::AsItself
    } else {
        sight(view, entity)
    };
    if seen == Sight::Unseen {
        return Ok((0, Status::Gone));
    }
    match name {
        "Selected" => return Ok((i64::from(view.local.selected.contains(&id)), Status::Ok)),
        "ControlGroup" => {
            let group = view
                .local
                .control_groups
                .iter()
                .find(|(object, _)| *object == id)
                .map_or(-1, |&(_, group)| i64::from(group));
            return Ok((group, Status::Ok));
        }
        _ => {}
    }
    if SIM_OBJECT.contains(&name) || !own && !SEEN_OBJECT.contains(&name) {
        return Ok(HIDDEN);
    }
    let reader = view.reader();
    if name == "NearbyEnemyCount" {
        let count = reader.nearby_enemy_count(entity, |other| match sight(view, other) {
            Sight::Unseen => None,
            Sight::AsItself => Some(other.owner()),
            Sight::Disguised { house, .. } => view.sim().houses.order().get(house).copied(),
        });
        return Ok((count, Status::Ok));
    }
    let Sight::Disguised { worn, house } = seen else {
        return reader.read_in(reference, prop, at);
    };
    // A disguise reads as the worn type and house; the health bar's
    // fraction stays the real one.
    let worn = worn.map(|id| type_ref(NAME_TECHNO_TYPE, id));
    Ok(match (name, worn) {
        // The disguise supplies the image this viewer sees.
        ("ImageOverride", _) => (0, Status::Ok),
        ("Owner", _) => (house_ref(house), Status::Ok),
        ("Type", Some(worn)) => (worn, Status::Ok),
        ("Category" | "Strength", Some(worn)) => reader.read_in(worn, prop, at)?,
        ("Health", Some(worn)) => {
            let percent = property_id("HealthPercent")
                .map_or(Ok((0, Status::Unsupported)), |p| {
                    reader.read_in(reference, p, at)
                })?;
            let strength = property_id("Strength").map_or(Ok((0, Status::Unsupported)), |p| {
                reader.read_in(worn, p, at)
            })?;
            match (percent, strength) {
                ((percent, Status::Ok), (strength, Status::Ok)) => {
                    (percent * strength / 100, Status::Ok)
                }
                ((_, Status::Ok), (_, status)) | ((_, status), _) => (0, status),
            }
        }
        _ => reader.read_in(reference, prop, at)?,
    })
}

/// A cell's property for this player (module docs); `reference` is the
/// cell or the waypoint read.
fn cell_read(
    view: &ClientView<'_>,
    (x, y): (u16, u16),
    reference: i64,
    prop: i32,
    name: &str,
    at: View,
) -> Result<(i64, Status), Invalid> {
    let reader = view.reader();
    if PUBLIC_CELL.contains(&name) || name.starts_with("Zone.") {
        return reader.read_in(reference, prop, at);
    }
    let on_map = view
        .sim()
        .resolved_terrain()
        .is_none_or(|terrain| terrain.cell(x, y).is_some());
    if !on_map {
        return Ok((0, Status::NotFound));
    }
    let readable = if SIM_CELL.contains(&name) {
        false
    } else if OWN_CELL.contains(&name) {
        view.me().index.is_some()
    } else {
        view.cell_in_sight(x, y)
    };
    if readable {
        reader.read_in(reference, prop, at)
    } else {
        Ok(HIDDEN)
    }
}

/// `client_find`: the simulation's `find`, through this player's gate.
/// The viewpoint is always this player's house. Objects are filtered by
/// owner and type as this player sees them (a disguise's), so those filters
/// become clauses; the sim-only scopes answer an empty page and `Refused`.
pub fn find(
    view: &ClientView<'_>,
    query: &Query,
    cap: usize,
) -> Result<(FindPage, Status), Invalid> {
    if SIM_SCOPES.contains(&query.scope) {
        return Ok((FindPage::default(), Status::Refused));
    }
    // Still a reference, though the client's viewpoint is its own house.
    decode_ref(query.viewpoint)?;
    let mut query = query.clone();
    query.viewpoint = view.me().index.map_or(0, house_ref);
    if query.scope == SCOPE_OBJECTS
        && let Some(answer) = seen_filters(view, &mut query)?
    {
        return Ok(answer);
    }
    let reader = Reader {
        gate: Some(view),
        ..view.reader()
    };
    reader.find(&query, cap)
}

/// Turns an objects query's `owner` and `type` into leading `Owner =` and
/// `Type =` clauses, which read as this player sees them. Checks them as
/// the simulation's `find` does; `Some` answers a query whose owner is gone.
fn seen_filters(
    view: &ClientView<'_>,
    query: &mut Query,
) -> Result<Option<(FindPage, Status)>, Invalid> {
    let rules = &view.runtime.resources.rules;
    if query.type_ref != 0 {
        let techno = match decode_ref(query.type_ref)? {
            Some(Ref::Type {
                kind: NAME_TECHNO_TYPE,
                id,
            }) => i32::try_from(id).is_ok_and(|id| techno_type(rules, id).is_some()),
            _ => false,
        };
        if !techno {
            return Err(Invalid(format!(
                "{:#x} is not a TechnoType for an Objects query",
                query.type_ref
            )));
        }
    }
    match decode_ref(query.owner)? {
        None => {}
        Some(Ref::House(index)) if index < view.sim().houses.order().len() => {}
        Some(Ref::House(_)) => return Ok(Some((FindPage::default(), Status::Gone))),
        Some(_) => {
            return Err(Invalid(format!(
                "{:#x} is not a house reference",
                query.owner
            )));
        }
    }
    let clause = |name: &str, value: i64| {
        property_id(name).map(|prop| Clause {
            subject: ClauseSubject::Property(prop),
            op: Op::Eq,
            value,
        })
    };
    let mut clauses: Vec<Clause> = [("Owner", query.owner), ("Type", query.type_ref)]
        .into_iter()
        .filter(|&(_, value)| value != 0)
        .filter_map(|(name, value)| clause(name, value))
        .collect();
    clauses.append(&mut query.clauses);
    query.clauses = clauses;
    query.owner = 0;
    query.type_ref = 0;
    Ok(None)
}

impl ReadGate for ClientView<'_> {
    fn lists(&self, reference: i64) -> bool {
        match decode_ref(reference) {
            Ok(Some(Ref::Object(_))) => sees_object(self, reference),
            Ok(Some(Ref::Cell { x, y })) => self.cell_listed(x, y),
            Ok(Some(Ref::SuperWeapon { house, .. })) => self.me().index == Some(house),
            _ => false,
        }
    }

    fn clause_read(&self, reference: i64, prop: i32, view: View) -> Result<(i64, Status), Invalid> {
        read_gated(self, reference, prop, view)
    }

    fn clause_var(&self, scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
        var_get(self, scope, var)
    }
}

/// `client_var_get`: a variable under its declared visibility (RFC 6e;
/// module docs). A wrong scope or unknown variable is invalid, as in the
/// simulation.
pub fn var_get(
    view: &ClientView<'_>,
    scope: i64,
    var: i32,
) -> Result<(i64, Status), Invalid> {
    let answer = view.reader().read_var(scope, var)?;
    let sim = view.sim();
    let visibility = if var < FIRST_DECLARED_VAR {
        // The Red Alert 2 variables: global and private.
        Visibility::Private
    } else {
        sim.ext_modules
            .vars()
            .visibility(var)
            .unwrap_or(Visibility::Private)
    };
    if visibility == Visibility::Public {
        return Ok(answer);
    }
    let me = view.me();
    let shared = |holder: InternedId| {
        me.id.is_some_and(|me| {
            me == holder || visibility != Visibility::Private && view.mutually_allied(me, holder)
        })
    };
    let readable = match decode_ref(scope)? {
        Some(Ref::House(index)) => sim
            .houses
            .order()
            .get(index)
            .is_some_and(|&house| shared(house)),
        Some(Ref::Object(id)) => {
            let Some(entity) = sim.entities().get(id) else {
                return Ok((0, Status::Gone));
            };
            if !object_exists(view, entity) {
                return Ok((0, Status::Gone));
            }
            shared(entity.owner()) || visibility == Visibility::InSight
        }
        // Global, and anything the simulation's read already refused.
        _ => false,
    };
    Ok(if readable { answer } else { HIDDEN })
}

/// `client_lookup` for every name kind but `Notify` and `Hotkey` (the glue
/// routes those; here they answer `Unsupported`). Variable names are the
/// match's declared ones, found for this module's source.
pub fn lookup(
    view: &ClientView<'_>,
    kind: i32,
    name: &str,
) -> Result<(i32, Status), Invalid> {
    if !(0..NAME_KINDS).contains(&kind) {
        return Err(Invalid(format!("unknown name kind {kind}")));
    }
    let found = |id: Option<i32>| id.map_or((-1, Status::NotFound), |id| (id, Status::Ok));
    Ok(match kind {
        NAME_NOTIFY | NAME_HOTKEY => (-1, Status::Unsupported),
        NAME_PROPERTY => found(property_id(name)),
        NAME_ACTION_KIND => found(action_kind_id(name)),
        _ => lookup_name(
            view.sim(),
            &view.runtime.resources.rules,
            module_source(view.source),
            kind,
            name,
        ),
    })
}

/// `client_name_of`, the counterpart of [`lookup`].
pub fn name_of(
    view: &ClientView<'_>,
    kind: i32,
    id: i32,
) -> Result<(String, Status), Invalid> {
    if !(0..NAME_KINDS).contains(&kind) {
        return Err(Invalid(format!("unknown name kind {kind}")));
    }
    let named = |name: Option<&str>| {
        name.map_or((String::new(), Status::NotFound), |name| {
            (name.to_owned(), Status::Ok)
        })
    };
    match kind {
        NAME_NOTIFY | NAME_HOTKEY => Ok((String::new(), Status::Unsupported)),
        _ if id == -1 => Ok((String::new(), Status::NotFound)),
        NAME_PROPERTY => property_name(id)
            .map(|name| (name.to_owned(), Status::Ok))
            .ok_or_else(|| Invalid(format!("unknown property {id}"))),
        NAME_ACTION_KIND => action_kind_name(id)
            .map(named)
            .ok_or_else(|| Invalid(format!("unknown action kind {id}"))),
        _ => name_of_id(view.sim(), &view.runtime.resources.rules, kind, id),
    }
}

/// `client_has_kind`: the simulation's tables (events, actions, properties,
/// name kinds, scopes) plus what the client adds: its own properties, the
/// `Notify` and `Hotkey` name kinds, node kinds, client events and client
/// actions (`HomeCamera` is `later`).
pub fn has_kind(view: &ClientView<'_>, table: i32, id: i32) -> bool {
    let rules = &view.runtime.resources.rules;
    match table {
        TABLE_PROPERTY => {
            crate::sim::ext_modules::has_kind(rules, table, id)
                || property_name(id).is_some_and(|name| CLIENT_PROPERTIES.contains(&name))
        }
        TABLE_NAME_KIND => {
            matches!(id, NAME_NOTIFY | NAME_HOTKEY)
                || crate::sim::ext_modules::has_kind(rules, table, id)
        }
        TABLE_NODE_KIND => (node_kind::TEXT..=node_kind::BAR).contains(&id),
        TABLE_CLIENT_EVENT => (client_event::CLICK..=client_event::SIM_EVENT).contains(&id),
        TABLE_CLIENT_ACTION => {
            (client_action::PLAY_SOUND..=client_action::REGISTER_HOTKEY).contains(&id)
                && id != client_action::HOME_CAMERA
        }
        _ => crate::sim::ext_modules::has_kind(rules, table, id),
    }
}

const fn module_source(source: Source) -> ModuleSource {
    match source {
        Source::Mod => ModuleSource::Mod,
        Source::Map => ModuleSource::Map,
    }
}

#[cfg(all(test, tests_sim_c_g))]
#[path = "client_reads_tests.rs"]
mod tests;
