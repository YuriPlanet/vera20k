//! Lua userdata for technos and houses, plus the command queue mods fill.
//!
//! Objects are cheap handles: an id and the shared per-frame snapshot. Methods
//! always answer from the newest snapshot, so a handle kept across frames never
//! keeps stale positions or health alive. Orders are not applied here; they are
//! queued as `Command` payloads and submitted by the app after the Lua tick,
//! through the same admission path as player input.

use std::cell::RefCell;
use std::rc::Rc;

use mlua::{AnyUserData, Lua, MetaMethod, UserData, UserDataMethods};

use crate::sim::command::Command;
use crate::sim::intern::InternedId;

use super::PendingCommand;
use super::snapshot::{EntitySnapshot, FrameSnapshot, HouseSnapshot};

/// Handles shared by every userdata of one host.
#[derive(Clone)]
pub(crate) struct ScriptContext {
    pub(crate) snapshot: Rc<RefCell<FrameSnapshot>>,
    pub(crate) commands: Rc<RefCell<Vec<PendingCommand>>>,
}

impl ScriptContext {
    fn entity(&self, id: u64) -> Option<EntitySnapshot> {
        self.snapshot.borrow().entity(id).cloned()
    }

    fn house(&self, name: InternedId) -> Option<HouseSnapshot> {
        self.snapshot.borrow().houses.get(&name).cloned()
    }

    fn queue(&self, owner: InternedId, command: Command) {
        self.commands
            .borrow_mut()
            .push(PendingCommand { owner, command });
    }
}

pub(super) struct TechnoUserdata {
    id: u64,
    context: ScriptContext,
}

impl TechnoUserdata {
    pub(super) fn new(id: u64, context: ScriptContext) -> Self {
        Self { id, context }
    }
}

impl UserData for TechnoUserdata {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("GetId", |_, this, ()| Ok(this.id));

        methods.add_method("IsAlive", |_, this, ()| {
            Ok(this.context.entity(this.id).is_some())
        });

        methods.add_method("GetTypeName", |_, this, ()| {
            Ok(this.context.entity(this.id).map(|entity| entity.type_name))
        });

        methods.add_method("GetHealth", |_, this, ()| {
            Ok(this.context.entity(this.id).map(|entity| entity.health))
        });

        methods.add_method("GetMaxHealth", |_, this, ()| {
            Ok(this.context.entity(this.id).map(|entity| entity.max_health))
        });

        methods.add_method("GetKind", |_, this, ()| {
            Ok(this.context.entity(this.id).map(|entity| entity.kind()))
        });

        methods.add_method("GetPosition", |lua, this, ()| {
            let Some(entity) = this.context.entity(this.id) else {
                return Ok(None);
            };
            let position = lua.create_table()?;
            position.set("x", entity.x)?;
            position.set("y", entity.y)?;
            position.set("z", entity.z)?;
            Ok(Some(position))
        });

        methods.add_method("GetOwner", |lua, this, ()| {
            let Some(entity) = this.context.entity(this.id) else {
                return Ok(None);
            };
            if this.context.house(entity.owner).is_none() {
                return Ok(None);
            }
            let house = HouseUserdata::new(entity.owner, this.context.clone());
            Ok(Some(lua.create_userdata(house)?))
        });

        methods.add_method("GetDistanceTo", |_, this, other: AnyUserData| {
            let other = other.borrow::<TechnoUserdata>()?;
            let Some(mine) = this.context.entity(this.id) else {
                return Ok(None);
            };
            let Some(theirs) = other.context.entity(other.id) else {
                return Ok(None);
            };
            Ok(Some(
                ((mine.x - theirs.x).powi(2) + (mine.y - theirs.y).powi(2)).sqrt(),
            ))
        });

        // --- Commands -----------------------------------------------------

        methods.add_method("MoveTo", |_, this, (x, y): (f64, f64)| {
            let Some(entity) = this.context.entity(this.id) else {
                return Ok(false);
            };
            if !entity.is_mobile() {
                return Ok(false);
            }
            let (Some(target_rx), Some(target_ry)) = (cell_coordinate(x), cell_coordinate(y))
            else {
                return Ok(false);
            };
            this.context.queue(
                entity.owner,
                Command::Move {
                    entity_id: this.id,
                    target_rx,
                    target_ry,
                },
            );
            Ok(true)
        });

        methods.add_method("Attack", |_, this, target: AnyUserData| {
            let target = target.borrow::<TechnoUserdata>()?;
            let Some(entity) = this.context.entity(this.id) else {
                return Ok(false);
            };
            if !entity.is_mobile() || this.context.entity(target.id).is_none() {
                return Ok(false);
            }
            this.context.queue(
                entity.owner,
                Command::Attack {
                    attacker_id: this.id,
                    target_id: target.id,
                },
            );
            Ok(true)
        });

        methods.add_method("Scatter", |_, this, ()| {
            let Some(entity) = this.context.entity(this.id) else {
                return Ok(());
            };
            if !entity.is_mobile() {
                return Ok(());
            }
            this.context
                .queue(entity.owner, Command::Scatter { entity_id: this.id });
            Ok(())
        });

        methods.add_method("Stop", |_, this, ()| {
            let Some(entity) = this.context.entity(this.id) else {
                return Ok(false);
            };
            if !entity.is_mobile() {
                return Ok(false);
            }
            this.context
                .queue(entity.owner, Command::Stop { entity_id: this.id });
            Ok(true)
        });

        methods.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| {
            let other = other.borrow::<TechnoUserdata>()?;
            Ok(this.id == other.id)
        });
    }
}

pub(super) struct HouseUserdata {
    name: InternedId,
    context: ScriptContext,
}

impl HouseUserdata {
    pub(super) fn new(name: InternedId, context: ScriptContext) -> Self {
        Self { name, context }
    }
}

impl UserData for HouseUserdata {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("GetName", |_, this, ()| {
            Ok(this
                .context
                .house(this.name)
                .map(|house| house.display_name))
        });

        methods.add_method("GetCredits", |_, this, ()| {
            Ok(this.context.house(this.name).map(|house| house.credits))
        });

        methods.add_method("IsHuman", |_, this, ()| {
            Ok(this.context.house(this.name).map(|house| house.is_human))
        });

        methods.add_method("GetAIDifficulty", |_, this, ()| {
            Ok(this.context.house(this.name).map(|house| house.difficulty))
        });

        methods.add_method("IsAlliedWith", |_, this, other: AnyUserData| {
            let other = other.borrow::<HouseUserdata>()?;
            Ok(this
                .context
                .snapshot
                .borrow()
                .alliances
                .contains(&(this.name, other.name)))
        });

        methods.add_meta_method(MetaMethod::Eq, |_, this, other: AnyUserData| {
            let other = other.borrow::<HouseUserdata>()?;
            Ok(this.name == other.name)
        });
    }
}

/// `MoveTo` takes map-cell coordinates as a number; the native setter takes a
/// cell. Out-of-range input is refused rather than clamped to a corner.
fn cell_coordinate(value: f64) -> Option<u16> {
    if !value.is_finite() || !(0.0..=f64::from(u16::MAX)).contains(&value) {
        return None;
    }
    Some(value.floor() as u16)
}

/// Build the Lua handle for one techno id.
pub(super) fn techno_handle(
    lua: &Lua,
    context: &ScriptContext,
    id: u64,
) -> mlua::Result<AnyUserData> {
    lua.create_userdata(TechnoUserdata::new(id, context.clone()))
}

/// Build the Lua handle for one house.
pub(super) fn house_handle(
    lua: &Lua,
    context: &ScriptContext,
    name: InternedId,
) -> mlua::Result<AnyUserData> {
    lua.create_userdata(HouseUserdata::new(name, context.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::lua::snapshot::HouseSnapshot;
    use crate::map::entities::EntityCategory;

    /// One player-owned tank at cell (40, 20); the shared command buffer.
    fn tank_fixture() -> (
        Lua,
        ScriptContext,
        Rc<RefCell<Vec<PendingCommand>>>,
        InternedId,
    ) {
        let house = InternedId::from_index(1);
        let mut snapshot = FrameSnapshot::default();
        snapshot.house_order.push(house);
        snapshot.houses.insert(
            house,
            HouseSnapshot {
                display_name: "P1".into(),
                credits: 10_000,
                is_human: true,
                difficulty: "normal",
            },
        );
        snapshot.entity_order.push(7);
        snapshot.entities.insert(
            7,
            EntitySnapshot {
                id: 7,
                owner: house,
                type_id: InternedId::from_index(2),
                type_name: "MTNK".into(),
                category: EntityCategory::Unit,
                health: 300,
                max_health: 300,
                x: 40.0,
                y: 20.0,
                z: 0.0,
            },
        );
        let commands = Rc::new(RefCell::new(Vec::new()));
        let context = ScriptContext {
            snapshot: Rc::new(RefCell::new(snapshot)),
            commands: Rc::clone(&commands),
        };
        (Lua::new(), context, commands, house)
    }

    #[test]
    fn move_to_queues_a_move_command_for_the_unit_owner() {
        let (lua, context, commands, house) = tank_fixture();
        let handle = techno_handle(&lua, &context, 7).expect("tank handle");
        lua.globals().set("u", handle).expect("global");

        let admitted: bool = lua.load("return u:MoveTo(42, 24)").eval().expect("MoveTo");
        assert!(admitted, "MoveTo on a live mobile unit must queue");

        let queued = commands.borrow();
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].owner, house);
        let Command::Move {
            entity_id,
            target_rx,
            target_ry,
        } = queued[0].command
        else {
            panic!("expected Command::Move, got {:?}", queued[0].command);
        };
        assert_eq!(entity_id, 7);
        assert_eq!(target_rx, 42);
        assert_eq!(target_ry, 24);
    }

    #[test]
    fn stop_and_scatter_queue_their_commands() {
        let (lua, context, commands, house) = tank_fixture();
        let handle = techno_handle(&lua, &context, 7).expect("tank handle");
        lua.globals().set("u", handle).expect("global");

        let stopped: bool = lua.load("return u:Stop()").eval().expect("Stop");
        assert!(stopped, "Stop on a live mobile unit must queue");
        // Scatter reports acceptance by returning nothing (nil), unlike
        // Stop/MoveTo's boolean; only the queue is the authority here.
        lua.load("u:Scatter()").exec().expect("Scatter");

        let queued = commands.borrow();
        assert_eq!(queued.len(), 2);
        assert_eq!(queued[0].owner, house);
        assert!(matches!(queued[0].command, Command::Stop { entity_id: 7 }));
        assert!(matches!(
            queued[1].command,
            Command::Scatter { entity_id: 7 }
        ));
    }

    #[test]
    fn commands_refuse_stale_or_immobile_handles() {
        let (lua, context, commands, _house) = tank_fixture();
        let live = techno_handle(&lua, &context, 7).expect("tank handle");
        // A handle whose id is not in the snapshot: the unit died this frame.
        let stale = techno_handle(&lua, &context, 999).expect("stale handle");
        lua.globals().set("live", live).expect("global");
        lua.globals().set("stale", stale).expect("global");

        let stale_move: bool = lua.load("return stale:MoveTo(1, 1)").eval().expect("stale");
        assert!(!stale_move, "a dead unit must refuse orders");

        // Buildings are not mobile: the same id cannot move once recategorized.
        context
            .snapshot
            .borrow_mut()
            .entities
            .get_mut(&7)
            .expect("tank")
            .category = EntityCategory::Structure;
        let building_move: bool = lua
            .load("return live:MoveTo(1, 1)")
            .eval()
            .expect("building");
        assert!(!building_move, "a structure must refuse move orders");

        // The refusals queued nothing.
        assert!(commands.borrow().is_empty());
    }

    #[test]
    fn out_of_range_and_nan_targets_are_refused() {
        let (lua, context, commands, _house) = tank_fixture();
        let handle = techno_handle(&lua, &context, 7).expect("tank handle");
        lua.globals().set("u", handle).expect("global");

        for script in [
            "return u:MoveTo(1/0, 0)",
            "return u:MoveTo(-5, 0)",
            "return u:MoveTo(0, 70000)",
        ] {
            let admitted: bool = lua.load(script).eval().expect(script);
            assert!(!admitted, "{script} must refuse");
        }
        assert!(commands.borrow().is_empty());
    }
}
