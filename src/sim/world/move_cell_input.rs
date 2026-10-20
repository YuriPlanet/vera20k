//! Input-time Foot Move click queries: What_Action on a Cell for a Move
//! click, the Cell a Move click on a Cell resolves, and the destination a Move
//! click over an object resolves. Synchronized commands already carry the
//! chosen destination; their executor never repeats these input queries.

use super::Simulation;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::{CellCoordinate, cell_center};
use crate::sim::cell_rect::{PlayfieldBounds, cell_is_in_playfield_height_aware_in_query};
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::find_nearby_cell::{
    NearbyFootprint, NearbyQuery, PassabilityArgs, find_nearby_passable_cell, map_owned_radius_cap,
};
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose;
use crate::sim::movement::ground_pose::query_ground_height as query_ground;
use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
use crate::sim::occupancy::RawCellOccupationGrid;
use crate::sim::pathfinding::zone_map::{ZoneGrid, ZoneQueryCell};
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;

/// What_Action's answer, on a Cell (vt+0x70) or over an object (vt+0x74), in
/// the rungs a Move click reaches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveAction {
    /// Action 0: the arrow cursor, and a click orders nothing.
    None,
    /// Action 1.
    Move,
    /// Action 2: the barred cursor. A click still resolves a destination
    /// (`0x004D8065`, `0x004D77BD`).
    NoMove,
}

impl MoveAction {
    pub(crate) const fn native(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Move => 1,
            Self::NoMove => 2,
        }
    }
}

/// How What_Action on a Cell is asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellActionQuery {
    /// DetermineAction's (`0x00692610`) third argument, which Techno
    /// `0x00700600` reads as its second: 1 from the cursor update
    /// (`0x0069300B`); 0 from the button release (`0x00693267`), the
    /// selection's click loop (`0x004AE974`), ClickedAction (`0x004D7DA1`) and
    /// the Cell resolver (`0x004DE1E4`). With it set, the Move rungs answer
    /// Move before asking Can_Enter_Cell (`0x00700B51..0x00700B57`).
    pub mouse_over: bool,
    /// The Shift key (`0x00A8EC08`/`0x00A8EC0C`, `0x00700696..0x007006C5`),
    /// which Ctrl clears (`0x00700706..0x0070071B`).
    pub shift: bool,
}

impl Simulation {
    /// What_Action on a Cell (vt+0x70), in the rungs a Move click reaches: for
    /// a Foot, the class wrapper (Unit `0x007404B0`, Infantry `0x0051F800`,
    /// Aircraft `0x00417F80`) around Foot `0x004DDDE0` around Techno
    /// `0x00700600`; for a Building, [`Self::building_cell_move_action`]. The
    /// cursor and the click both ask it.
    ///
    /// For a Foot, in order:
    /// - each class wrapper answers 0 for a House that is not the player's
    ///   (`0x007404BF`, `0x0051F80E`, `0x00417F89`);
    /// - Techno answers 0 for a berserk object (`0x00700609`), a warped one
    ///   (`+0x270`/`+0x271`, `0x00700731`, `0x00700743`) and one the player
    ///   cannot order (vt+0xA0, `0x00700ACE`). vt+0x284 is the constant false
    ///   `0x0041BEE0` for all three classes, so Alt adds nothing;
    /// - outside the playfield it answers NoMove (`0x00700AF6`), then Move for
    ///   Shift (`0x00700B03`) and for the cursor's query (`0x00700B51`);
    /// - otherwise the mover's own Can_Enter_Cell with no facing, level or
    ///   source Cell ([`Self::techno_cell_move_rungs`]) answers Move up to 1,
    ///   else NoMove;
    /// - Foot turns any answer but 0 over a shrouded Cell centre into Move for
    ///   a MoveToShroud type inside the playfield, else NoMove
    ///   (`0x004DDE64..0x004DDEBE`);
    /// - a Unit's Move becomes 0 while it is deployed, deploying or
    ///   undeploying (6E0/6E1/6E2, `0x00740709..0x00740727`), so its deployed
    ///   IsSimpleDeployer arm (`0x00740752..0x00740767`) finds no Move;
    /// - an Infantry Deployer in Doing27..30 turns Move into NoMove
    ///   (`0x0051F8BB..0x0051F94E`), and an Infantry 0 becomes NoMove unless
    ///   it is a JumpJet or cannot be ordered (`0x0051FAAB..0x0051FADD`): a
    ///   berserk soldier.
    ///
    /// Not modelled, each answering another action where native reaches it:
    /// the planning rung (Ctrl+Alt or +0x43A, `0x00700815..0x007008A3`); the
    /// attack rungs (`0x007008A6..0x00700AB4`), which callers ask first
    /// (Ctrl's forced shot, and [`Self::cell_attack_rung_admits`]); a Unit
    /// harvester over ore or weeds
    /// (`0x007405F5..0x007406C3`, app-side in the cursor and the click); a
    /// tube entrance (Unit `0x007406CF`, Infantry `0x0051F9AC`); a JumpJet
    /// soldier over a tube's neighbourhood, which answers 0
    /// (`0x0051F860..0x0051F8A6`); the fogged-object scans; and DirectRocker
    /// (`+0x2A8`, `0x00700617`), which nothing in retail sets.
    pub fn cell_move_action(
        &self,
        viewer: InternedId,
        id: u64,
        cell: (u16, u16),
        query: CellActionQuery,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<MoveAction, String> {
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("What_Action receiver disappeared")?;
        if entity.category() == EntityCategory::Structure {
            return self.building_cell_move_action(viewer, id, cell, query, rules, registry);
        }
        if !self.house_is_human_player(entity.owner()) {
            return Ok(MoveAction::None);
        }
        let object = self
            .interner
            .object_type(entity.type_ref(), rules)
            .ok_or("What_Action receiver lacks its type")?;
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("What_Action needs map cells")?;
        let cells = NativeCellQuery::isolated(terrain);
        let clicked = (cell.0 as i16, cell.1 as i16);
        let in_playfield = cell_is_in_playfield_height_aware_in_query(
            (i32::from(clicked.0), i32::from(clicked.1)),
            self.playfield_bounds,
            Some(terrain),
            Some(&cells),
        );
        let controllable = self.techno_player_controllable(id, rules);
        let mut action = if entity.berserk.is_active()
            || entity.is_warped_out()
            || entity.is_warping_in()
            || !controllable
        {
            MoveAction::None
        } else {
            Self::techno_cell_move_rungs(object, in_playfield, query, rules, || {
                self.mover_can_enter(
                    id,
                    clicked,
                    InfantryEntryArgs::REPAIR,
                    EntryQueryMode::CheckLocomotor,
                    rules,
                    registry,
                )
            })?
        };
        if action != MoveAction::None && clicked_cell_shrouded(&self.fog, &cells, viewer, clicked)?
        {
            action = if object.move_to_shroud && in_playfield {
                MoveAction::Move
            } else {
                MoveAction::NoMove
            };
        }
        match entity.category() {
            EntityCategory::Unit if action == MoveAction::Move && entity.is_deployed() => {
                action = MoveAction::None;
            }
            EntityCategory::Infantry => {
                if action == MoveAction::Move && object.deployer && entity.infantry_deploy_doing() {
                    action = MoveAction::NoMove;
                }
                if action == MoveAction::None && !object.jumpjet && controllable {
                    action = MoveAction::NoMove;
                }
            }
            _ => {}
        }
        Ok(action)
    }

    /// Techno What_Action on a Cell's Move rungs (`0x00700AE1..0x00700C06`)
    /// for a receiver the player may order: NoMove outside the playfield
    /// (`0x00700AF6`), then Move for Shift (`0x00700B03`) and for the cursor's
    /// query (`0x00700B51`), otherwise the receiver's own Can_Enter_Cell with
    /// no facing, level or source Cell (`0x00700B5F..0x00700B7E`): Move up to
    /// 1. Above it a Building whose UndeploysInto unit is a
    /// `ResourceGatherer=` (TechnoType+0x5EC, `0x00700B84..0x00700BB4`)
    /// answers Move; any other receiver NoMove. The Subterranean (Type+D2C)
    /// second probe without the locomotor (`0x00700BD1..0x00700C06`) repeats
    /// that answer, since no class reads the argument it changes
    /// ([`EntryQueryMode`]).
    fn techno_cell_move_rungs(
        object: &crate::rules::object_type::ObjectType,
        in_playfield: bool,
        query: CellActionQuery,
        rules: &RuleSet,
        can_enter: impl FnOnce() -> Result<u8, String>,
    ) -> Result<MoveAction, String> {
        if !in_playfield {
            return Ok(MoveAction::NoMove);
        }
        if query.shift || query.mouse_over || can_enter()? <= 1 {
            return Ok(MoveAction::Move);
        }
        let gatherer = object.category == crate::rules::object_type::ObjectCategory::Building
            && object
                .undeploys_into
                .as_deref()
                .and_then(|unit| rules.object(unit))
                .is_some_and(|unit| unit.resource_gatherer);
        Ok(if gatherer {
            MoveAction::Move
        } else {
            MoveAction::NoMove
        })
    }

    /// What_Action on a Cell for a Building:
    /// `BuildingClass::What_Action_OnCell @ 0x00447540` around Techno
    /// `0x00700600`, in the rungs a Move click reaches. In order:
    /// - a building [`Self::takes_rally_point`] refuses for `viewer` answers
    ///   0: another House's, or one the player cannot order (Techno
    ///   `0x00700ACE`, Building `0x00447617..0x0044762A`);
    /// - a rally-line type (vt+0x284, `0x00455DA0`) answers Move, whatever
    ///   rung Techno reached (`0x0044774B..0x00447768`);
    /// - an UndeploysInto type asks Techno without the mouse-over argument
    ///   (`0x004475A0`), so its own Can_Enter_Cell ([`Self::building_can_enter`])
    ///   decides the cursor as it does the click
    ///   ([`Self::techno_cell_move_rungs`]): a Construction Yard over water
    ///   answers NoMove, and the Slave Miner's yard always moves.
    ///
    /// Callers ask the attack rungs first ([`Self::cell_attack_rung_admits`]),
    /// and the Ctrl+Alt guard rung (`0x00700815..0x007008A3`), which only a
    /// building without a rally line keeps. The cursor answers the Harvest 6
    /// over mapped Tiberium (`0x00447563..0x004475EE`), which the click treats
    /// as Move (`0x004436F0`). InvisibleInGame's 0 (`0x0044754B`) is not
    /// modelled: no retail type with a rally line or UndeploysInto sets it.
    fn building_cell_move_action(
        &self,
        viewer: InternedId,
        id: u64,
        cell: (u16, u16),
        query: CellActionQuery,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<MoveAction, String> {
        if !self.takes_rally_point(id, self.interner.resolve(viewer), rules) {
            return Ok(MoveAction::None);
        }
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("What_Action receiver disappeared")?;
        let object = self
            .interner
            .object_type(entity.type_ref(), rules)
            .ok_or("What_Action receiver lacks its type")?;
        if object.has_rally_line() {
            return Ok(MoveAction::Move);
        }
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("What_Action needs map cells")?;
        let cells = NativeCellQuery::isolated(terrain);
        let clicked = (cell.0 as i16, cell.1 as i16);
        let in_playfield = cell_is_in_playfield_height_aware_in_query(
            (i32::from(clicked.0), i32::from(clicked.1)),
            self.playfield_bounds,
            Some(terrain),
            Some(&cells),
        );
        let query = CellActionQuery {
            mouse_over: false,
            ..query
        };
        Self::techno_cell_move_rungs(object, in_playfield, query, rules, || {
            self.building_can_enter(id, clicked, rules, registry)
        })
    }

    /// The Cell a left click's receivers are asked about: the selection's
    /// click loop clamps the clicked Cell to the playfield
    /// (`Clamp_To_Playfield @ 0x00586E50`, `0x004AE77E`) before any receiver's
    /// What_Action on it. Without bounds or map cells it keeps the Cell:
    /// VERA-internal, as a live gamemd session always has both.
    pub fn clamped_click_cell(&self, cell: (u16, u16)) -> (u16, u16) {
        let (Some(bounds), Some(terrain)) = (self.playfield_bounds, self.resolved_terrain.as_ref())
        else {
            return cell;
        };
        let cells = NativeCellQuery::isolated(terrain);
        let (x, y) = crate::sim::cell_rect::clamp_cell_to_playfield(
            (i32::from(cell.0 as i16), i32::from(cell.1 as i16)),
            bounds,
            Some(terrain),
            Some(&cells),
        );
        (x as u16, y as u16)
    }

    /// Whether Techno What_Action on a Cell (`0x00700600`) reaches its
    /// unforced Attack rung for `id` (`0x007008A6..0x007009F1`). The caller
    /// then resolves that rung's tail (vt+0xA0 or InRange, answering 5) the
    /// way it resolves a forced shot at the Cell. In order:
    /// - 0 for a berserk or warped object (`0x00700609`, `0x00700731`,
    ///   `0x00700743`), and the rung needs a human player's object
    ///   (`0x007008AC`) holding a current weapon (vt+0x3F4, `0x007008BD`);
    /// - the Cell must hold a LegalTarget overlay (`0x0070092E..0x0070093E`)
    ///   or be a destroyable cliff (`0x00486900`) for a warhead without
    ///   `Fire=` (`0x00700908..0x00700924`);
    /// - then it attacks with no overlay (the cliff), a live veinhole monster,
    ///   or a Wall overlay its owner (`CellClass+0x50`) is not allied with
    ///   (`0x0070097E`), when the warhead has `Wall=`, or `Wood=` against an
    ///   `Armor=wood` overlay (`0x007009C8..0x007009F1`).
    ///
    /// Residuals: the planning rung before it answers for an object whose
    /// +0x43A is set (`0x00700840`; FootClass::ActivatePlanningWaypoint sets
    /// it at `0x004DC8D3`), which VERA does not model; the fogged-object scan
    /// behind the scenario's FogOfWar flag
    /// (`0x00700761..0x0070080F`: a fogged overlay replaces the live one and
    /// a fogged enemy building admits the rung) and the veinhole monster
    /// (`0x00700990`, no VERA owner) are not modelled; the later TerrainFire
    /// rung (vt+0x398, `0x00700A46`) is not either.
    pub fn cell_attack_rung_admits(
        &self,
        id: u64,
        cell: (u16, u16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.berserk.is_active()
            || entity.is_warped_out()
            || entity.is_warping_in()
            || !self.house_is_human_player(entity.owner())
        {
            return false;
        }
        let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
            return false;
        };
        let Some(weapon) = crate::sim::combat::combat_weapon::current_weapon(
            entity,
            object,
            &self.substrate.entities,
            rules,
            &self.interner,
        ) else {
            return false;
        };
        let warhead = weapon
            .warhead
            .as_deref()
            .and_then(|name| rules.warhead(name));
        let overlay_cell = self
            .overlay_grid
            .as_ref()
            .map(|grid| grid.cell(cell.0, cell.1));
        let overlay = overlay_cell
            .and_then(|cell| cell.overlay_id)
            .and_then(|overlay_id| registry?.flags(overlay_id));
        let cliff = self
            .resolved_terrain
            .as_ref()
            .is_some_and(|terrain| terrain.is_destroyable_cliff(cell.0, cell.1))
            && !warhead.is_some_and(|warhead| warhead.fire);
        if !(overlay.is_some_and(|overlay| overlay.legal_target) || cliff) {
            return false;
        }
        let Some(overlay) = overlay else {
            return true;
        };
        let enemy_owned = overlay_cell
            .and_then(|cell| cell.wall_owner)
            .is_some_and(|owner| {
                !crate::map::houses::is_allied_with(
                    &self.house_alliances,
                    self.interner.resolve(owner),
                    self.interner.resolve(entity.owner()),
                )
            });
        enemy_owned
            && overlay.wall
            && warhead
                .is_some_and(|warhead| warhead.wall || (warhead.wood && overlay.armor_is_wood))
    }

    /// `MapClass::IsShrouded @ 0x00586360` for `viewer` at a clicked Cell's
    /// coordinate, as Foot What_Action's shroud rung asks it and as the Unit
    /// harvest arms' gate does (`0x007405C1..0x007405E6`), which the app
    /// still answers for the cursor and the click.
    pub fn clicked_cell_is_shrouded(
        &self,
        viewer: InternedId,
        cell: (u16, u16),
    ) -> Result<bool, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("IsShrouded needs map cells")?;
        clicked_cell_shrouded(
            &self.fog,
            &NativeCellQuery::isolated(terrain),
            viewer,
            (cell.0 as i16, cell.1 as i16),
        )
    }

    /// The Cell a Foot's Move click resolves, before event encoding. The
    /// selection's click loop (`0x004AE94E..0x004AE990`) asks each receiver's
    /// own What_Action ([`Self::cell_move_action`], `0x004AE976`); its
    /// ClickedAction (vt+0x140: Unit `0x00738910`, Infantry `0x0051F250`,
    /// Aircraft `0x00417C80`, then Foot `0x004D7D50`) sends Move and NoMove
    /// alike to the Cell resolver `0x004DE1D0` (`0x004D7EB5`, `0x004D806F`),
    /// whatever the locomotor. Refused before it:
    /// - action 0, which ClickedAction has no arm for (`0x004D7DCC..0x004D7DD2`);
    /// - NoMove for a deployed (6E0) Unit (`0x0073891D..0x00738933`) or for an
    ///   Infantry in Doing27..30 (`0x0051F25D..0x0051F285`).
    ///
    /// ClickedAction's outside-playfield refusal (`0x004D7DB3..0x004D7DBF`)
    /// never fires: outside the playfield What_Action answers NoMove or 0.
    /// The Berserk refusals (`0x004D7D58`, `0x0073893D`, `0x0051F28F`) are
    /// [`Self::click_order_receivers`], which the producers apply first.
    /// `None` sends no order. Native comparisons: walk_first_path.json
    /// `gi_reissue` (GI and MTNK What_Action and click rows) and
    /// walk_move_admission.json (the resolver).
    pub fn cell_click_destination(
        &self,
        viewer: InternedId,
        id: u64,
        clicked: (u16, u16),
        shift: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Option<(u16, u16)>, String> {
        let action = self.cell_move_action(
            viewer,
            id,
            clicked,
            CellActionQuery {
                mouse_over: false,
                shift,
            },
            rules,
            registry,
        )?;
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("Cell click receiver disappeared")?;
        let refused = match action {
            MoveAction::None => true,
            MoveAction::NoMove => match entity.category() {
                EntityCategory::Unit => entity.is_fully_deployed(),
                EntityCategory::Infantry => entity.infantry_deploy_doing(),
                EntityCategory::Aircraft | EntityCategory::Structure => false,
            },
            MoveAction::Move => false,
        };
        if refused {
            return Ok(None);
        }
        let object = self
            .interner
            .object_type(entity.type_ref(), rules)
            .ok_or("Cell click receiver lacks its type")?;
        let map = self.click_map()?;
        let terrain = map.cells.terrain();
        resolve_foot_cell_click(
            &map,
            &FootCellClick {
                clicked: (clicked.0 as i16, clicked.1 as i16),
                action: action.native(),
                move_to_shroud: object.move_to_shroud,
                teleporter: object.teleporter,
                mover: self.click_mover(id, object)?,
            },
            |cell| {
                Ok(crate::sim::vision::cell_ground_mapped(
                    &self.fog, terrain, viewer, cell,
                ))
            },
        )
    }

    /// The class rungs of a Foot's What_Action over an object that turn the
    /// Techno action `action` into another Move action; each ClickedAction
    /// wrapper's re-ask (`0x007388A1`, `0x0051F1A1`) answers them too:
    /// - the deployer rungs turn Move into NoMove for a player's Deployer in
    ///   Doing27..30 (Infantry `0x0051EB81..0x0051EBD7`) and a player's
    ///   deployed (6E0) IsSimpleDeployer (Unit `0x0074037A..0x007403AD`);
    /// - the tails turn 0 into NoMove for a player's Unit or Infantry it
    ///   may control (vt+0xA0; Unit `0x007403EB..0x00740414`, Infantry
    ///   `0x0051F05E..0x0051F08B`).
    pub fn foot_object_class_action(
        &self,
        id: u64,
        action: MoveAction,
        rules: &RuleSet,
    ) -> MoveAction {
        let Some(entity) = self.substrate.entities.get(id) else {
            return action;
        };
        let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
            return action;
        };
        let human = self.house_is_human_player(entity.owner());
        match (entity.category(), action) {
            (EntityCategory::Infantry, MoveAction::Move)
                if human && object.deployer && entity.infantry_deploy_doing() =>
            {
                MoveAction::NoMove
            }
            (EntityCategory::Unit, MoveAction::Move)
                if human && object.is_simple_deployer && entity.is_fully_deployed() =>
            {
                MoveAction::NoMove
            }
            (EntityCategory::Unit | EntityCategory::Infantry, MoveAction::None)
                if human && self.techno_player_controllable(id, rules) =>
            {
                MoveAction::NoMove
            }
            _ => action,
        }
    }

    /// The destination a Foot's Move or NoMove click over an object orders:
    /// the object, or a Cell near it. The click loop
    /// (`0x004AE829..0x004AE844`) hands each receiver's own What_Action over
    /// the object to its ClickedAction (vt+0x144: Unit `0x00738890`, Infantry
    /// `0x0051F190`, Aircraft `0x00417BD0`) with 0 as the third argument. Move
    /// and NoMove reach Foot `0x004D74E0`, whose Move arm sends
    /// `ClickedMission(Move, NULL, destination)` (`0x004D7C76`); the object's
    /// Cell it adds as the fourth argument feeds only planning mode's event
    /// (`0x006FFC23..0x006FFCA9`).
    ///
    /// `action` is the receiver's What_Action over the object before the class
    /// rungs ([`Self::foot_object_class_action`]).
    ///
    /// No order (`None`) for:
    /// - action 0 that no tail turned (`0x004D7521..0x004D7527`);
    /// - an Aircraft's NoMove (`0x00417C12`) and a Unit's NoMove over itself
    ///   (`0x007388D5..0x007388E8`);
    /// - a berserk receiver (`0x007388EB`, `0x0051F1D8`, `0x004D74EC`);
    /// - NoMove where the object's coordinate (vt+0x48) is shrouded, for a
    ///   type without MoveToShroud (`0x004D77BD..0x004D77EA`);
    /// - a receiver the player cannot order (vt+0xA0, `0x004D77FB..0x004D7807`);
    /// - an Aircraft whose object coordinate, raised to its ground, is
    ///   shrouded (`0x004D7809..0x004D7896`).
    ///
    /// Otherwise [`resolve_foot_object_click`] picks the object or a Cell.
    ///
    /// Not modelled: DirectRocker (`+0x2A8`, `0x004D74FA..0x004D7514`), which
    /// nothing in retail sets; for a Subterranean (Type+D2C) mover, the
    /// shroud return gated on `AllowShroudedSubteranneanMoves` (Rules+0x17EF,
    /// `0x004D785F..0x004D7877`) and FNPC's height flag
    /// (`0x004D7A31..0x004D7A5A`), since no retail type is Subterranean.
    pub fn object_move_click(
        &self,
        viewer: InternedId,
        id: u64,
        target: u64,
        action: MoveAction,
        force_move_key: bool,
        rules: &RuleSet,
    ) -> Result<Option<TargetKind>, String> {
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("Move click receiver disappeared")?;
        if entity.category() == EntityCategory::Structure {
            return Err("a building answers ClickedAction through its own class".into());
        }
        let object = self
            .interner
            .object_type(entity.type_ref(), rules)
            .ok_or("Move click receiver lacks its type")?;
        let action = self.foot_object_class_action(id, action, rules);
        let refused = match action {
            MoveAction::None => true,
            MoveAction::NoMove => match entity.category() {
                EntityCategory::Aircraft => true,
                EntityCategory::Unit => target == id,
                EntityCategory::Infantry | EntityCategory::Structure => false,
            },
            MoveAction::Move => false,
        };
        if refused || entity.berserk.is_active() {
            return Ok(None);
        }
        let target_entity = self
            .substrate
            .entities
            .get(target)
            .ok_or("Move click target disappeared")?;
        let map = self.click_map()?;
        let terrain = map.cells.terrain();
        let open = |cell| {
            Ok(crate::sim::vision::cell_ground_mapped(
                &self.fog, terrain, viewer, cell,
            ))
        };
        let coords = ground_pose::object_get_coords(target_entity, Some(terrain));
        if action == MoveAction::NoMove
            && crate::sim::vision::coordinate_is_shrouded(&map.cells, coords, &open)?
            && !object.move_to_shroud
        {
            return Ok(None);
        }
        if !self.techno_player_controllable(id, rules) {
            return Ok(None);
        }
        let raised = DriveCoord {
            z: coords.z.max(query_ground(&map.cells, coords)?),
            ..coords
        };
        if entity.category() == EntityCategory::Aircraft
            && crate::sim::vision::coordinate_is_shrouded(&map.cells, raised, &open)?
        {
            return Ok(None);
        }
        let navigation = crate::sim::movement::nav_target_coordinate(
            NavTargetRef::object(target),
            None,
            self,
            Some(terrain),
            Some((rules, &self.interner)),
        )?;
        let click = FootObjectClick {
            target: ((navigation.x / 256) as i16, (navigation.y / 256) as i16),
            no_move: action == MoveAction::NoMove,
            infantry: entity.category() == EntityCategory::Infantry,
            force_move_key,
            mover: self.click_mover(id, object)?,
        };
        Ok(Some(match resolve_foot_object_click(&map, &click, open)? {
            Some((x, y)) => TargetKind::Cell(x, y),
            None => TargetKind::Entity(target),
        }))
    }

    /// The map both Foot click receivers read, through one fresh query
    /// identity that every nested call shares.
    fn click_map(&self) -> Result<ClickMap<'_>, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Move click needs map cells")?;
        let zones = self.zone_grid.as_ref().ok_or("Move click needs zones")?;
        let bounds = self
            .playfield_bounds
            .ok_or("Move click needs the playfield")?;
        let size = self
            .map_size_diamond()
            .ok_or("Move click needs the map Size")?;
        //One fresh query identity starts from the canonical retained bytes.
        let cells = NativeCellQuery::isolated(terrain);
        if cells.dummy().raw_flags() & 3 != 0 {
            return Err("input Dummy visibility outside the constructor-derived domain".into());
        }
        Ok(ClickMap {
            cells,
            zones,
            raw: &self.substrate.raw_cell_occupation,
            overlays: self.overlay_grid.as_ref(),
            bounds,
            size,
            frame: self.session.binary_frame(),
        })
    }

    fn click_mover(
        &self,
        id: u64,
        object: &crate::rules::object_type::ObjectType,
    ) -> Result<ClickMover, String> {
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("Move click receiver disappeared")?;
        Ok(ClickMover {
            current: self.foot_current_coordinate(id)?,
            coordinate: self.foot_navigation_coordinate(id)?,
            marked: entity.lifecycle.cell_marked(),
            on_bridge: entity.on_bridge(),
            in_tube: entity.low_bridge_tube_state.is_some(),
            special_type: object.movement_zone == MovementZone::Subterranean
                || entity.category() == EntityCategory::Aircraft
                || (entity.category() == EntityCategory::Infantry && object.jumpjet),
            movement_zone: object.movement_zone,
            speed_type: object.speed_type,
        })
    }
}

/// The mover facts both Foot click receivers read: the Cell resolver
/// `0x004DE1D0` and the object receiver `0x004D74E0`.
struct ClickMover {
    /// The mover's own coordinate (Object+9C, altitude included), which
    /// IsInAir (`0x005F6B90`) measures against the ground.
    current: DriveCoord,
    /// vt+0x4C with no requester, whose Cell is the zone source.
    coordinate: DriveCoord,
    marked: bool,
    on_bridge: bool,
    in_tube: bool,
    /// The type terms of the special test after IsInAir (`0x004DE2EA..
    /// 0x004DE335`, `0x004D793E..0x004D797F`): Type+D2C (Subterranean), an
    /// Infantry's Type+D94 (JumpJet; 4DE1D0's House53A130 test is constant
    /// false), or an Aircraft.
    special_type: bool,
    movement_zone: MovementZone,
    speed_type: SpeedType,
}

impl ClickMover {
    /// IsInAir (vt+0x54, `0x005F6B90`): GetHeight (`0x005F5F40`) two levels
    /// above the ground, sampled only when Object+74 is true. GetHeight
    /// subtracts OnBridge's deck independently of the ShouldBeOnBridge query.
    fn in_air(&self, cells: &NativeCellQuery<'_>) -> bool {
        use crate::sim::movement::air_movement::{object_flight_predicate, object_height_in_query};
        object_flight_predicate(
            self.marked,
            || object_height_in_query(self.current, self.on_bridge, Some(cells)),
            true,
        )
    }
}

/// The map queries both Foot click receivers make.
struct ClickMap<'a> {
    cells: NativeCellQuery<'a>,
    zones: &'a ZoneGrid,
    raw: &'a RawCellOccupationGrid,
    /// The live overlays whose `Wall=` FNPC's passability test reads.
    overlays: Option<&'a crate::sim::overlay_grid::OverlayGrid>,
    bounds: PlayfieldBounds,
    size: (i32, i32),
    frame: u32,
}

impl ClickMap<'_> {
    /// `0x00578460(cell, 1)`.
    fn in_playfield(&self, cell: (i16, i16)) -> bool {
        cell_is_in_playfield_height_aware_in_query(
            (i32::from(cell.0), i32::from(cell.1)),
            Some(self.bounds),
            Some(self.cells.terrain()),
            Some(&self.cells),
        )
    }

    /// The Cell of the mover's vt+0x4C coordinate, retained as the zone source.
    fn source(&self, mover: &ClickMover) -> NativeCellIdentity {
        self.cells
            .lookup_world(mover.coordinate.x, mover.coordinate.y)
    }

    /// ShouldBeOnBridge (vt+0xBC) with 0.
    fn source_bridge(&self, mover: &ClickMover) -> Result<bool, String> {
        ground_pose::navigation_should_be_on_bridge(
            &self.cells,
            mover.coordinate,
            mover.current,
            mover.on_bridge,
            mover.in_tube,
        )
    }

    /// The bridge flag of `cell`'s Map lookup (`+0x140 & 0x100`).
    fn bridge(&self, cell: (i16, i16)) -> bool {
        self.cells.flags(self.cells.lookup(cell)) & 0x100 != 0
    }

    /// Can_Reach_Zone `0x0056D100` from the retained source to a copy of
    /// `target`, with `target`'s bridge flag read first.
    fn reaches(
        &self,
        source: NativeCellIdentity,
        source_bridge: bool,
        target: (i16, i16),
        movement_zone: MovementZone,
    ) -> Result<bool, String> {
        let bridge = self.bridge(target);
        self.zones
            .can_reach_native(
                &self.cells,
                ZoneQueryCell::Retained(source),
                ZoneQueryCell::Copied(target),
                movement_zone,
                source_bridge,
                bridge,
                false,
                self.bounds,
                self.size,
            )
            .ok_or_else(|| "input lacks native zone topology".into())
    }

    /// FNPC `0x0056DC20` around `target` with both receivers' arguments: one
    /// Cell, `target`'s bridge flag, and the zone `0x0056D230` gives the
    /// retained source (`zone_source`, with its bridge flag), or none for -1.
    /// The bridge flag is read before the zone, as both push them. FNPC's
    /// (0,0) answer is `None`.
    fn nearby(
        &self,
        target: (i16, i16),
        speed_type: SpeedType,
        movement_zone: MovementZone,
        zone_source: Option<(NativeCellIdentity, bool)>,
    ) -> Result<Option<(u16, u16)>, String> {
        let bridge = self.bridge(target);
        let required_zone = zone_source
            .map(|(source, source_bridge)| {
                let p = self.cells.coord(source);
                // FNPC disables the comparison for DWORD -1 and raw 0xFFFF alike.
                self.zones
                    .get_zone_id_native_in_query(
                        self.cells.terrain(),
                        (p.0 as u16, p.1 as u16),
                        movement_zone,
                        source_bridge,
                        Some(&self.cells),
                    )
                    .ok_or("input FNPC source lacks native zone topology")
            })
            .transpose()?;
        Ok(find_nearby_passable_cell(
            (i32::from(target.0), i32::from(target.1)),
            &NearbyQuery {
                native_cells: Some(&self.cells),
                raw_occupation: Some(self.raw),
                passability: PassabilityArgs {
                    speed_type,
                    required_zone_id: required_zone,
                    movement_zone,
                    bridge_aware_zone: bridge,
                },
                footprint: NearbyFootprint::SINGLE,
                allow_bridge_cells: true,
                check_height: true,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(self.size.0, self.size.1),
                target_cell: None,
                path_grid: None,
                resolved_terrain: Some(self.cells.terrain()),
                overlay_grid: self.overlays,
                occupancy: None,
                entities: None,
                zone_grid: Some(self.zones),
                playfield_bounds: Some(self.bounds),
            },
            self.frame,
        )
        .filter(|cell| *cell != (0, 0)))
    }
}

/// Values the Foot Cell resolver 4DE1D0 reads. Its own What_Action
/// (`0x004DE1E9`) asks what the click already asked, so the action arrives
/// selected; it is not a mission.
struct FootCellClick {
    clicked: (i16, i16),
    action: u32,
    move_to_shroud: bool,
    teleporter: bool,
    mover: ClickMover,
}

fn clicked_cell_shrouded(
    fog: &crate::sim::vision::FogState,
    cells: &NativeCellQuery<'_>,
    viewer: InternedId,
    clicked: (i16, i16),
) -> Result<bool, String> {
    Ok(crate::sim::vision::point_is_shrouded(
        fog,
        cells,
        viewer,
        clicked_coordinate(cells, clicked)?,
    ))
}

/// A clicked Cell's centre on its ground, raised to a bridge deck: the
/// coordinate What_Action and 4DE1D0 build before IsShrouded (`0x0074056E..
/// 0x007405CD`, `0x004DE1FD..0x004DE25D`).
fn clicked_coordinate(
    cells: &NativeCellQuery<'_>,
    clicked: (i16, i16),
) -> Result<DriveCoord, String> {
    let centre = cell_center(
        CellCoordinate {
            x: i32::from(clicked.0),
            y: i32::from(clicked.1),
        },
        0,
    );
    let mut point = DriveCoord {
        x: centre.x,
        y: centre.y,
        z: 0,
    };
    point.z = query_ground(cells, point)?;
    //4DE242/4DE38C are separate Map calls after the height query.
    if cells.flags(cells.lookup_world(point.x, point.y)) & 0x100 != 0 {
        point.z = point.z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS);
    }
    Ok(point)
}

/// Original 4DE1D0 for every Foot. No A* search and no
/// entity/mission/navigation mutation occurs here. Native comparisons live in
/// tools/spatial_oracle/walk_move_admission (Infantry and Unit/Drive
/// receivers, original input action supplied).
///
/// FNPC's tenth argument (`0x004DE3DB..0x004DE3F9`: 1 for a Type+D2C mover
/// at or above the ground, else 0) is not passed: no retail type is
/// Subterranean.
fn resolve_foot_cell_click(
    map: &ClickMap<'_>,
    click: &FootCellClick,
    open: impl Fn(NativeCellIdentity) -> Result<bool, String>,
) -> Result<Option<(u16, u16)>, String> {
    let cells = &map.cells;
    let in_playfield = map.in_playfield(click.clicked);
    let point = clicked_coordinate(cells, click.clicked)?;
    if !click.move_to_shroud && crate::sim::vision::coordinate_is_shrouded(cells, point, &open)? {
        return Ok(None);
    }
    let source = map.source(&click.mover);
    let in_air = click.mover.in_air(cells);
    //4DE2B7..4DE2DA: Subterranean searches as Normal. Fly becomes Infantry
    //only when House53A130 answers true, and it is constant false.
    let movement_zone = match click.mover.movement_zone {
        MovementZone::Subterranean => MovementZone::Normal,
        zone => zone,
    };
    let special = in_air || click.mover.special_type;
    let source_bridge = map.source_bridge(&click.mover)?;
    let point = clicked_coordinate(cells, click.clicked)?;
    let shrouded = crate::sim::vision::coordinate_is_shrouded(cells, point, &open)?;
    let unrestricted = special && click.action == 2;
    let clicked =
        (click.clicked != (0, 0)).then_some((click.clicked.0 as u16, click.clicked.1 as u16));
    if !unrestricted && in_playfield && !shrouded {
        if special || (click.teleporter && click.action != 2) {
            return Ok(clicked);
        }
        if map.reaches(source, source_bridge, click.clicked, movement_zone)? {
            return Ok(clicked);
        }
    }
    map.nearby(
        click.clicked,
        click.mover.speed_type,
        movement_zone,
        (!unrestricted).then_some((source, source_bridge)),
    )
}

/// Values Foot `0x004D74E0`'s Move arm reads once it has an order to send.
struct FootObjectClick {
    /// The object's Cell: its vt+0x4C coordinate with no requester, each axis
    /// divided by 256 toward zero (`0x004D78AC..0x004D78E8`).
    target: (i16, i16),
    /// NoMove, which enters the Move arm at `0x004D77FB` once its own shroud
    /// test passes.
    no_move: bool,
    infantry: bool,
    /// The force-move key (`0x00A8EBF8`/`0x00A8EBFC`, `0x004D79E1..0x004D7A1A`),
    /// which the arm reads because the click loop passes 0.
    force_move_key: bool,
    mover: ClickMover,
}

/// Foot `0x004D74E0`'s Move arm from the object's Cell on
/// (`0x004D78AC..0x004D7C08`): the Cell FNPC finds near the object's Cell, or
/// `None` to keep the object. Special movers (IsInAir or the type terms of
/// [`ClickMover::special_type`]) keep the object inside the playfield, and
/// their NoMove searches with any zone. Any other mover keeps the object only
/// over a mapped Cell its zone reaches, unless it clicked NoMove or is an
/// Infantry under the force-move key; otherwise, as outside the playfield, it
/// searches its own zone. FNPC's (0,0) answer keeps the object
/// (`0x004D7AC5..0x004D7ADE`).
fn resolve_foot_object_click(
    map: &ClickMap<'_>,
    click: &FootObjectClick,
    open: impl Fn(NativeCellIdentity) -> Result<bool, String>,
) -> Result<Option<(u16, u16)>, String> {
    let cells = &map.cells;
    let in_playfield = map.in_playfield(click.target);
    let source = map.source(&click.mover);
    //4D791C..4D7931: Subterranean searches as Normal and Fly as Infantry, with
    //no House test.
    let movement_zone = match click.mover.movement_zone {
        MovementZone::Subterranean => MovementZone::Normal,
        MovementZone::Fly => MovementZone::Infantry,
        zone => zone,
    };
    let special = click.mover.in_air(cells) || click.mover.special_type;
    let source_bridge = map.source_bridge(&click.mover)?;
    //4D7990..4D79DF: the Cell's centre on its ground, with no bridge raise.
    let point = cell_center(
        CellCoordinate {
            x: i32::from(click.target.0),
            y: i32::from(click.target.1),
        },
        0,
    );
    let mut centre = DriveCoord {
        x: point.x,
        y: point.y,
        z: 0,
    };
    centre.z = query_ground(cells, centre)?;
    let shrouded = crate::sim::vision::coordinate_is_shrouded(cells, centre, &open)?;
    let own_zone = Some((source, source_bridge));
    let nearby = |zone_source| {
        map.nearby(
            click.target,
            click.mover.speed_type,
            movement_zone,
            zone_source,
        )
    };
    if special && click.no_move {
        return nearby(None);
    }
    if !in_playfield {
        return nearby(own_zone);
    }
    if special {
        return Ok(None);
    }
    if shrouded
        || click.no_move
        || (click.force_move_key && click.infantry)
        || !map.reaches(source, source_bridge, click.target, movement_zone)?
    {
        return nearby(own_zone);
    }
    Ok(None)
}

#[cfg(any(test, feature = "test-support"))]
#[path = "move_cell_input_tests.rs"]
mod tests;
