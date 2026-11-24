//! What a bot may look at: what its House's player would see on screen, plus
//! the rules, read-only. Everything here is a plain read of authoritative
//! state, so every peer computes the same answer.
//!
//! Fair play is enforced here, not by the bots: the simulation stays private,
//! and every read of other Houses' objects or the map's contents goes through
//! [`BotView::sees`] — the House's shroud (explored cells; with fog of war,
//! units only in current sight and buildings in sight or last seen), minus
//! cloaked objects its sensors miss and disguises it cannot see through.
//! Own objects, the map's size and terrain stay fully visible, as for a
//! player. More only through a declared [`Cheat`] (`BotInfo::cheats`).

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::production::{BuildOption, FactoryView, ProductionCategory};
use crate::sim::world::Simulation;

/// A bot's declared advantage over a player. Granted only when the bot's
/// `BotInfo::cheats` lists it, and announced in the match's chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cheat {
    /// Every object and cell of the map, shroud or not.
    FullMapVision,
    /// [`EXTRA_MONEY_PER_MINUTE`] credits a game minute.
    ExtraMoney,
}

/// Credits [`Cheat::ExtraMoney`] adds every [`EXTRA_MONEY_EVERY`] frames.
pub const EXTRA_MONEY_PER_MINUTE: i32 = 1000;
pub const EXTRA_MONEY_EVERY: u32 = 900;

impl Cheat {
    pub fn describe(self) -> &'static str {
        match self {
            Self::FullMapVision => "full map vision",
            Self::ExtraMoney => "extra money",
        }
    }
}

pub struct BotView<'a> {
    sim: &'a Simulation,
    pub rules: &'a RuleSet,
    pub overlays: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    pub house: InternedId,
    owner_name: &'a str,
    full_vision: bool,
    fog_of_war: bool,
}

impl<'a> BotView<'a> {
    pub(crate) fn new(
        sim: &'a Simulation,
        rules: &'a RuleSet,
        overlays: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
        house: InternedId,
        cheats: &[Cheat],
    ) -> Self {
        Self {
            sim,
            rules,
            overlays,
            house,
            owner_name: sim.interner().resolve(house),
            full_vision: cheats.contains(&Cheat::FullMapVision),
            fog_of_war: sim.session().game_options().fog_of_war,
        }
    }

    /// Whether this House's player has explored cell (rx, ry): it shows on
    /// their map (not shroud, not under an enemy gap generator).
    pub fn explored(&self, rx: u16, ry: u16) -> bool {
        self.full_vision || {
            let fog = self.sim.fog();
            fog.is_cell_revealed(self.house, rx, ry) && !fog.is_cell_gap_covered(self.house, rx, ry)
        }
    }

    /// Whether this House's player sees `entity` on screen: own objects
    /// always; others where the map shows them (fog of war: units in
    /// current sight, buildings in sight or as last seen), not fully cloaked
    /// unless a sensor covers them, not disguised to this House.
    pub fn sees(&self, entity: &GameEntity) -> bool {
        if entity.owner() == self.house || self.full_vision {
            return true;
        }
        let (rx, ry) = cell(entity);
        let fog = self.sim.fog();
        if !self.explored(rx, ry) {
            return false;
        }
        if self.fog_of_war
            && !fog.is_cell_visible(self.house, rx, ry)
            && !(is_structure(entity) && fog.is_object_fogged(self.house, entity.stable_id()))
        {
            return false;
        }
        if entity.cloak.as_ref().is_some_and(|c| c.is_fully_cloaked())
            && !fog.senses_object_for_house(self.house, entity)
        {
            return false;
        }
        !entity.is_disguised_to(
            self.house,
            Some(fog),
            Some(self.sim.house_alliances()),
            self.sim.interner(),
        )
    }

    /// Whether cell (rx, ry) is in this House's sight now: explored and,
    /// with fog of war, currently visible (without fog every explored cell
    /// shows what stands there).
    pub fn in_sight(&self, rx: u16, ry: u16) -> bool {
        self.explored(rx, ry)
            && (!self.fog_of_war || self.sim.fog().is_cell_visible(self.house, rx, ry))
    }

    /// Whether own unit `unit`, standing in the middle of cell `at`, would
    /// have the seen object `target` within reach of the weapon it picks
    /// against it: the engine's own SelectWeapon and InRange (foundation,
    /// elevation, minimum range, line of fire), measured from that cell's
    /// ground instead of where the unit stands.
    pub fn in_range_from(&self, unit: &GameEntity, target: &GameEntity, at: (u16, u16)) -> bool {
        use crate::sim::combat::{TargetKind, combat_weapon, in_range, line_of_fire};
        let sim = self.sim;
        if unit.owner() != self.house || !self.sees(target) {
            return false;
        }
        let (Some(object), Some(terrain)) = (
            self.object_type(unit.type_ref()),
            sim.resolved_terrain.as_ref(),
        ) else {
            return false;
        };
        let goal = TargetKind::Entity(target.stable_id());
        let Some((_, Some(selected))) = combat_weapon::select_weapon_against(
            self.rules,
            unit,
            object,
            &combat_weapon::attacker_facts(unit, object),
            Some(&goal),
            sim,
            sim.interner(),
            Some(terrain),
            Some(sim.house_alliances()),
        ) else {
            return false;
        };
        let Some(ground) = terrain.cell(at.0, at.1) else {
            return false;
        };
        let x = i32::from(at.0) * 256 + 128;
        let y = i32::from(at.1) * 256 + 128;
        let z = crate::util::lepton::ground_height_leptons(ground.level, ground.slope_type, x, y)
            .unwrap_or(0);
        let los = line_of_fire::LineOfFireInputs {
            overlay_grid: sim.overlay_grid.as_ref(),
            overlay_registry: self.overlays,
            alliances: Some(sim.house_alliances()),
            buildings: Some(line_of_fire::CellBuildings::of(sim)),
        };
        in_range::compute_in_range(
            unit,
            (i64::from(x), i64::from(y), i64::from(z)),
            &goal,
            selected.weapon,
            self.rules,
            sim.interner(),
            sim,
            terrain,
            &los,
        )
    }

    /// A live, placed object this House sees, by id.
    pub fn entity(&self, id: u64) -> Option<&'a GameEntity> {
        self.sim
            .entities()
            .get(id)
            .filter(|e| e.is_alive() && !e.in_limbo() && self.sees(e))
    }

    /// Every live, placed object of other Houses this House sees.
    pub fn visible_objects(&self) -> impl Iterator<Item = &'a GameEntity> + 'a {
        let view = self.fair_copy();
        self.sim.entities().values().filter(move |e| {
            e.owner() != view.house && e.is_alive() && !e.in_limbo() && view.sees(e)
        })
    }

    /// Seen buildings of neutral Houses (tech buildings, civilians).
    pub fn neutral_structures(&self) -> impl Iterator<Item = &'a GameEntity> + 'a {
        let sim = self.sim;
        self.visible_objects().filter(move |e| {
            is_structure(e)
                && sim
                    .houses()
                    .get(&e.owner())
                    .is_some_and(|h| h.multiplay_passive())
        })
    }

    /// This House's superweapons, as its sidebar shows them.
    pub fn super_weapons(
        &self,
    ) -> Option<
        &'a std::collections::BTreeMap<InternedId, crate::sim::superweapon::SuperWeaponInstance>,
    > {
        self.sim.super_weapons().get(&self.house)
    }

    /// The parts of `self` [`Self::sees`] reads, for iterators that outlive
    /// the borrow of `self`.
    fn fair_copy(&self) -> BotView<'a> {
        BotView {
            sim: self.sim,
            rules: self.rules,
            overlays: self.overlays,
            house: self.house,
            owner_name: self.owner_name,
            full_vision: self.full_vision,
            fog_of_war: self.fog_of_war,
        }
    }

    /// Whether cell (rx, ry) holds a wall of this House (walls are
    /// overlays once placed, not objects).
    pub fn own_wall_at(&self, rx: u16, ry: u16) -> bool {
        let (Some(grid), Some(overlays)) = (self.sim.overlay_grid(), self.overlays) else {
            return false;
        };
        let cell = grid.cell(rx, ry);
        cell.wall_owner == Some(self.house)
            && cell
                .overlay_id
                .and_then(|id| overlays.flags(id))
                .is_some_and(|flags| flags.wall)
    }

    /// The simulation frame (the native frame counter).
    pub fn frame(&self) -> u32 {
        self.sim.session().binary_frame()
    }

    pub fn house_state(&self) -> &'a HouseState {
        &self.sim.houses()[&self.house]
    }

    pub fn credits(&self) -> i32 {
        self.house_state().economy().available_money()
    }

    /// (output, drain) of this House's power.
    pub fn power(&self) -> (i32, i32) {
        crate::sim::production::power_balance_for_owner(self.sim, self.rules, self.owner_name)
    }

    /// Whether `owner`'s power is down (drain above output): what a player
    /// reads off its dark buildings and silent Tesla Coils.
    pub fn power_low(&self, owner: InternedId) -> bool {
        let (output, drain) = crate::sim::production::power_balance_for_owner(
            self.sim,
            self.rules,
            self.sim.interner().resolve(owner),
        );
        drain > output
    }

    pub fn name(&self, id: InternedId) -> &'a str {
        self.sim.interner().resolve(id)
    }

    pub fn id_of(&self, name: &str) -> Option<InternedId> {
        self.sim.interner().get(name)
    }

    pub fn object_type(&self, type_id: InternedId) -> Option<&'a ObjectType> {
        self.sim.interner().object_type(type_id, self.rules)
    }

    /// Every live, placed object of House `owner`, seen or not.
    fn objects_of(&self, owner: InternedId) -> impl Iterator<Item = &'a GameEntity> + 'a {
        self.sim
            .entities()
            .values()
            .filter(move |e| e.owner() == owner && e.is_alive() && !e.in_limbo())
    }

    pub fn own(&self) -> impl Iterator<Item = &'a GameEntity> + 'a {
        self.objects_of(self.house)
    }

    pub fn is_enemy(&self, other: InternedId) -> bool {
        if other == self.house {
            return false;
        }
        let Some(house) = self.sim.houses().get(&other) else {
            return false;
        };
        !house.multiplay_passive()
            && !house.is_defeated()
            && !self
                .sim
                .house_alliances()
                .is_allied(self.sim.interner(), self.house, other)
    }

    /// The Houses this one fights, in registration order.
    pub fn enemies(&self) -> Vec<InternedId> {
        self.sim
            .houses()
            .order()
            .iter()
            .copied()
            .filter(|&id| self.is_enemy(id))
            .collect()
    }

    /// Every live, placed object of every enemy House that this House sees.
    pub fn enemy_objects(&self) -> impl Iterator<Item = &'a GameEntity> + 'a {
        let enemies = self.enemies();
        self.visible_objects()
            .filter(move |e| enemies.contains(&e.owner()))
    }

    /// Every live, placed object of Houses allied with this one that this
    /// House sees (not its own, not neutral Houses').
    pub fn allied_objects(&self) -> impl Iterator<Item = &'a GameEntity> + 'a {
        let view = self.fair_copy();
        let sim = self.sim;
        self.visible_objects().filter(move |e| {
            sim.houses()
                .get(&e.owner())
                .is_some_and(|h| !h.multiplay_passive() && !h.is_defeated())
                && !view.is_enemy(e.owner())
        })
    }

    /// The sidebar's options, with whether each can start now.
    pub fn build_options(&self) -> Vec<BuildOption> {
        crate::sim::production::build_options_for_owner(self.sim, self.rules, self.owner_name)
    }

    pub fn factory(&self, category: ProductionCategory) -> Option<FactoryView<'a>> {
        self.sim.production().factories().view(self.house, category)
    }

    /// Whether a finished building of `type_name` may be placed with its
    /// top-left cell at (rx, ry) now, as the placement cursor shows it.
    /// Bots also leave two cells free around every ore spawner (a
    /// `SpawnsTiberium=yes` Terrain), so it has room to grow ore (playtest,
    /// 2026-11-14).
    pub fn can_place(&self, type_name: &str, rx: u16, ry: u16) -> bool {
        if self.near_ore_spawner(type_name, rx, ry) {
            return false;
        }
        crate::sim::production::placement_preview_for_owner_with_overlays(
            self.sim,
            self.rules,
            self.owner_name,
            type_name,
            rx,
            ry,
            self.overlays,
        )
        .is_some_and(|preview| preview.valid)
    }

    fn near_ore_spawner(&self, type_name: &str, rx: u16, ry: u16) -> bool {
        const FREE: i32 = 2;
        let (w, h) = self
            .rules
            .object(type_name)
            .map(|ty| ty.foundation.dimensions())
            .unwrap_or((1, 1));
        let (x0, y0) = (i32::from(rx) - FREE, i32::from(ry) - FREE);
        let (x1, y1) = (
            i32::from(rx) + i32::from(w) - 1 + FREE,
            i32::from(ry) + i32::from(h) - 1 + FREE,
        );
        self.sim
            .terrain_objects
            .spawning_cells()
            .iter()
            .any(|&(x, y)| {
                let (x, y) = (i32::from(x), i32::from(y));
                (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
            })
    }

    /// Whether this House's MCV `mcv` could deploy standing on (rx, ry), as
    /// its deploy cursor shows: `UnitClass::TryToDeploy @ 0x00738D30` tests
    /// the DeploysInto type at the cell minus (1, 1) with CanPlaceAt for no
    /// House. Explored cells only. The MCV is lifted for the test, as the
    /// native lifts it (Mark(UP) `0x00738D58`) and the deploy cursor reads
    /// the map (`LiftedUnit`), so its own cell answers too.
    pub fn can_deploy_at(&self, mcv: &GameEntity, rx: u16, ry: u16) -> bool {
        if mcv.owner() != self.house || !self.explored(rx, ry) || rx == 0 || ry == 0 {
            return false;
        }
        let Some(yard) = crate::sim::mcv_deploy::deploys_into(self.sim, mcv, self.rules) else {
            return false;
        };
        crate::sim::build_site::can_place_building_at(
            self.sim,
            self.rules,
            self.overlays,
            yard,
            (rx as i16 - 1, ry as i16 - 1),
            None,
            None,
            Some(&crate::sim::build_site::LiftedUnit::read(
                self.sim,
                mcv.stable_id(),
            )),
        )
    }

    /// Whether this House's building `id` takes a repair order now (its
    /// repair cursor: `can_repair_building`), and whether it is repairing
    /// (the wrench over it). `None` for anything not this House's.
    pub fn repair_state(&self, id: u64) -> Option<(bool, bool)> {
        let entity = self.sim.entities().get(id)?;
        (entity.owner() == self.house).then(|| {
            (
                crate::sim::production::can_repair_building(self.sim, self.rules, id),
                entity.building.repairing.is_on(),
            )
        })
    }

    /// Whether cell (rx, ry) holds ore or gems, as far as this House has
    /// explored it (shroud: no).
    pub fn has_ore(&self, rx: u16, ry: u16) -> bool {
        if !self.explored(rx, ry) {
            return false;
        }
        let (Some(grid), Some(overlays)) = (self.sim.overlay_grid(), self.overlays) else {
            return false;
        };
        grid.cell(rx, ry)
            .overlay_id
            .and_then(|id| overlays.flags(id))
            .is_some_and(|flags| flags.tiberium)
    }

    /// The crates this House's player sees: explored cells (with fog of
    /// war, in sight) holding a crate overlay.
    pub fn crates(&self) -> Vec<(u16, u16)> {
        let (Some(grid), Some(overlays)) = (self.sim.overlay_grid(), self.overlays) else {
            return Vec::new();
        };
        let (w, h) = self.map_size();
        self.sim
            .crate_authority()
            .slots()
            .iter()
            .filter(|slot| !slot.is_empty() && slot.cell_x > 0 && slot.cell_y > 0)
            .map(|slot| (slot.cell_x as u16, slot.cell_y as u16))
            .filter(|&(x, y)| x < w && y < h && self.explored(x, y))
            .filter(|&(x, y)| {
                self.full_vision
                    || !self.fog_of_war
                    || self.sim.fog().is_cell_visible(self.house, x, y)
            })
            .filter(|&(x, y)| {
                grid.cell(x, y)
                    .overlay_id
                    .and_then(|id| overlays.flags(id))
                    .is_some_and(|flags| flags.crate_type)
            })
            .collect()
    }

    /// Crates are on in this match (the setup's option, which every player
    /// sees in the lobby).
    pub fn crates_enabled(&self) -> bool {
        self.sim.session().game_options().crates
    }

    /// Whether a unit moving like `mz` can drive from `from` to `to` on the
    /// ground layer (the path finder's own zone labels; no zones: yes).
    pub fn can_reach(
        &self,
        mz: crate::rules::locomotor_type::MovementZone,
        from: (u16, u16),
        to: (u16, u16),
    ) -> bool {
        use crate::sim::movement::locomotor::MovementLayer;
        self.sim.zone_grid().is_none_or(|zones| {
            zones.can_reach(mz, from, MovementLayer::Ground, to, MovementLayer::Ground)
        })
    }

    /// The map's multiplayer start positions (waypoints 0-7), as the map
    /// preview shows them to every player; which one an opponent took is
    /// not told.
    pub fn start_locations(&self) -> Vec<(u16, u16)> {
        let session = self.sim.session();
        (0..8)
            .filter(|&i| session.waypoint_is_valid(i))
            .filter_map(|i| session.waypoint_cell(i))
            .filter(|&(x, y)| x > 0 && y > 0)
            .map(|(x, y)| (x as u16, y as u16))
            .collect()
    }

    /// Whether a cell is inside the map's playfield (`LocalSize=`, the part
    /// the player can see and scroll to), `MapClass::IsCellInPlayfield
    /// @ 0x00578460` without height. The cell array is larger: its corners
    /// outside stay black for good.
    pub fn in_playfield(&self, rx: u16, ry: u16) -> bool {
        self.sim
            .playfield_bounds()
            .is_none_or(|b| b.contains_geometry_packed(rx.into(), ry.into()))
    }

    /// Map size in cells (width, height of the cell array).
    pub fn map_size(&self) -> (u16, u16) {
        let session = self.sim.session();
        (session.map_width() as u16, session.map_height() as u16)
    }
}

/// A cell of an object.
pub fn cell(entity: &GameEntity) -> (u16, u16) {
    (entity.position.rx(), entity.position.ry())
}

pub fn is_structure(entity: &GameEntity) -> bool {
    entity.category() == EntityCategory::Structure
}

/// Squared cell distance.
pub fn dist2(a: (u16, u16), b: (u16, u16)) -> i32 {
    let dx = i32::from(a.0) - i32::from(b.0);
    let dy = i32::from(a.1) - i32::from(b.1);
    dx * dx + dy * dy
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
