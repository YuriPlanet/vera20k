//! Ares 3.0 firestorm walls (`docs/ares-firestorm.md`): the House firestorm
//! a `Type=Firestorm` superweapon (`superweapon::firewall`) or a trigger
//! raises, and the `Firestorm.Wall=` building sections it powers. Sources:
//! `src/Ext/Building/Hooks.Firewall.cpp`, `src/Ext/House/Hooks.Firewall.cpp`,
//! `BuildingExt` (`src/Ext/Building/Body.cpp`) and `HouseExt::SetFirestormState`
//! (`src/Ext/House/Body.cpp`) of the restored Ares source; the installed
//! Ares.dll bodies are cited beside each port.
//!
//! A powered section shows the powered frames, a looping anim and a random
//! flicker, burns every Foot in its cell and every bullet that enters it,
//! blocks infantry and vehicles, takes no damage (a hit shortens the
//! firestorm's drain instead) and cannot be sold. An unpowered section is
//! passable and sellable.
//!
//! Ledger: one Scenario `Random()` per powered section per Update on frames
//! not divisible by 8 while it has no flicker and is not a straight piece
//! (`10013280`); the drain timer of the owner's first Firestorm Super is
//! restarted by a hit (`10014A90`); no detach calls.

use std::collections::BTreeSet;

use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::projectile::{ProjectileCoord, ProjectileTarget};
use crate::sim::world::{FrameEffects, Simulation};

/// Building+0x584 (Special) holds the powered anim, Building+0x588
/// (SpecialTwo) the flicker.
const ACTIVE_ANIM_SLOT: u8 = 10;
const IDLE_ANIM_SLOT: u8 = 11;
/// The powered frames follow the 32 unpowered ones.
const ACTIVE_FRAME_OFFSET: u32 = 32;
/// A straight piece (connected north and south, or east and west) shows
/// neither anim.
const STRAIGHT_PIECES: [u32; 2] = [0b0101, 0b1010];
/// The four straight `0x0089F688` directions GetNeighbourCell visits:
/// north, east, south and west, bit `direction / 2` of the connections.
const STRAIGHT_DIRECTIONS: [usize; 4] = [0, 2, 4, 6];
/// AnimClass constructor flags and ZAdjust of every firestorm anim.
const ANIM_FLAGS: u32 = 0x600;
const ANIM_Z_ADJUST: i32 = -10;

/// The Houses whose firestorm is up: HouseClass::FirestormActive (`+0x1FA`),
/// which 3.0 uses instead of the 0.A HouseExt flag, and which only
/// [`Simulation::ares_set_firestorm_state`] writes. Their being any is
/// `HouseExt::IsAnyFirestormActive` (`0x100C19F4`, saved with the House
/// globals by `10021AC0`/`10022400`), the gate of every firestorm hook.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct FirestormHouses {
    active: BTreeSet<InternedId>,
}

impl FirestormHouses {
    /// `HouseExt::IsAnyFirestormActive`.
    pub(crate) fn any_active(&self) -> bool {
        !self.active.is_empty()
    }

    /// HouseClass::FirestormActive of `house`.
    pub(crate) fn is_active(&self, house: InternedId) -> bool {
        self.active.contains(&house)
    }

    /// SetFirestormState's head (`10022590`): false when `house` already is
    /// `active`, else it becomes so (the inlined UpdateAnyFirestormActive
    /// follows from the set).
    fn switch(&mut self, house: InternedId, active: bool) -> bool {
        if active {
            self.active.insert(house)
        } else {
            self.active.remove(&house)
        }
    }

    /// Folded only while a firestorm is up, so other worlds hash as before.
    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        if self.any_active() {
            std::hash::Hash::hash(b"ares-firestorm-houses-v1", hasher);
            std::hash::Hash::hash(&self.active, hasher);
        }
    }
}

/// BuildingClass::FirestormWallFrame (`+0x61C`, constructor 0) of an Ares
/// firestorm wall section: its four connections (bit `direction / 2` for a
/// same-House section north, east, south or west) plus 32 while powered.
/// Only [`Simulation::ares_update_firewall`] writes it; the body draw reads
/// it ([`firewall_body_frame`]).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub(crate) struct FirestormWallFrame(u32);

impl FirestormWallFrame {
    pub(crate) fn frame(self) -> u32 {
        self.0
    }

    pub(crate) fn is_zero(self) -> bool {
        self.0 == 0
    }

    fn connections(self) -> u32 {
        self.0 & 0xF
    }

    fn is_straight(self) -> bool {
        STRAIGHT_PIECES.contains(&self.connections())
    }

    /// UpdateFirewall's frame store (`10013280`): true when it changed.
    fn store(&mut self, frame: u32) -> bool {
        let changed = self.0 != frame;
        self.0 = frame;
        changed
    }
}

/// The section's type: BuildingTypeExt `Firestorm.Wall`.
fn is_firewall_type(object: &ObjectType) -> bool {
    object.building_type_ext().firestorm_wall
}

/// Ares BuildingClass_GetStaticImageFrame (`100149E0`, hook `0x43EFB3`),
/// replacing the stock `FirestormWall=` test of
/// `BuildingClass::GetCurrentFrame @ 0x0043EF90` (after its LaserFence one):
/// a `Firestorm.Wall` section not on Construction (Get_Mission, vt+0x184)
/// draws its [`FirestormWallFrame`]; anything else takes the ordinary
/// frames (`0x0043EFC6`).
pub(crate) fn firewall_body_frame(entity: &GameEntity, object: &ObjectType) -> Option<i32> {
    (is_firewall_type(object)
        && entity.mission.effective().known()
            != Some(crate::sim::mission::MissionType::Construction))
    .then_some(entity.ares_firestorm_wall.frame() as i32)
}

/// `BuildingExt::IsActiveFirestormWall @ 10012770`: while any firestorm is
/// up, a live, placed `Firestorm.Wall` building of a House other than
/// `ignore` whose firestorm is up.
pub(crate) fn is_active_firestorm_wall(
    houses: &FirestormHouses,
    entities: &EntityStore,
    interner: &StringInterner,
    rules: &RuleSet,
    building: u64,
    ignore: Option<InternedId>,
) -> bool {
    if !houses.any_active() {
        return false;
    }
    let Some(entity) = entities.get(building) else {
        return false;
    };
    Some(entity.owner()) != ignore
        && !entity.in_limbo()
        && entity.is_object_alive()
        && houses.is_active(entity.owner())
        && interner
            .object_type(entity.type_ref(), rules)
            .is_some_and(is_firewall_type)
}

impl Simulation {
    /// The building of the cell at `(x, y)` (MapClass::GetCellAt, then
    /// CellClass::GetBuilding over the ground list); none for the dummy
    /// cell a miss returns.
    fn ares_cell_building(&self, x: i32, y: i32) -> Option<u64> {
        let terrain = self.resolved_terrain.as_ref()?;
        let NativeCellIdentity::Real(index) = terrain.native_cell_identity((x as i16, y as i16))
        else {
            return None;
        };
        let cell = &terrain.cells()[index];
        self.substrate
            .occupancy
            .first_building_on_layer(cell.rx, cell.ry, MovementLayer::Ground)
    }

    fn ares_is_firewall(&self, id: u64, rules: &RuleSet) -> bool {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .is_some_and(is_firewall_type)
    }

    /// The building's type is linkable (`IsLinkable 1000D770`): a
    /// `Firestorm.Wall` section or an `IsTrench` trench.
    fn ares_is_linkable(&self, id: u64, rules: &RuleSet) -> bool {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.building_type_ext().is_linkable())
    }

    /// [`is_active_firestorm_wall`] over this world.
    pub(crate) fn ares_is_active_firestorm_wall(
        &self,
        building: u64,
        ignore: Option<InternedId>,
        rules: &RuleSet,
    ) -> bool {
        is_active_firestorm_wall(
            &self.ares_firestorm,
            &self.substrate.entities,
            &self.interner,
            rules,
            building,
            ignore,
        )
    }

    /// `HouseExt::ExtData::SetFirestormState @ 10022590` (annotated
    /// RespondToTrenchRedirectClick): nothing when `house` already is
    /// `active`; otherwise the flag changes and each of the House's
    /// `Firestorm.Wall` buildings (House+0x68, in order) updates its frame,
    /// anim and victims (`UpdateFirewall(true)`), and its Cell (vt+0x1B8) is
    /// collected for the closing `MapClass::Update_Pathfinding_1/2`
    /// ([`Self::ares_firestorm_update_pathfinding`]).
    pub(crate) fn ares_set_firestorm_state(
        &mut self,
        house: InternedId,
        active: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if !self.ares_firestorm.switch(house, active) {
            return;
        }
        let Some(buildings) = self
            .houses
            .get(&house)
            .map(|state| state.base_projection().buildings().to_vec())
        else {
            return;
        };
        let mut cells = Vec::new();
        for id in buildings {
            if self.ares_is_firewall(id, rules) {
                self.ares_update_firewall(id, true, rules, registry, effects);
                cells.extend(self.object_coords_cell(id));
            }
        }
        self.ares_firestorm_update_pathfinding(&cells, rules);
    }

    /// SetFirestormState's tail (`10022590`): `Update_Pathfinding_1`
    /// (RebuildZoneConnectivity `0x0056C510`, from the retained classes,
    /// before any of these cells changed) and `Update_Pathfinding_2`
    /// (`0x00586990`) over the sections' `cells`, whose Recalc (`0x0047D2B0`)
    /// reaches RecalcZoneType's building arm: a `Firestorm.Wall` section of
    /// a House whose firestorm is up makes its cell Impassable (`0x00483D9E`,
    /// Ares 10014B70, hook 0x483D94), any other leaves the terrain class.
    /// The path grid keeps blocking every building cell either way.
    ///
    /// RESIDUAL: VERA's other Recalc writers (an overlay change, the
    /// playfield resize) take the terrain inputs alone, so a later Recalc
    /// of a powered section's cell there drops the Impassable class; a
    /// section placed or removed while the firestorm is up keeps its
    /// cell's class until the next toggle.
    pub(crate) fn ares_firestorm_update_pathfinding(
        &mut self,
        cells: &[(u16, u16)],
        rules: &RuleSet,
    ) {
        {
            let (Some(zones), Some(path), Some(terrain)) = (
                self.zone_grid.as_deref_mut(),
                self.path_grid.as_deref(),
                self.resolved_terrain.as_ref(),
            ) else {
                return;
            };
            let records = self
                .bridge_state
                .as_ref()
                .map_or(&[][..], |bridges| bridges.endpoint_records());
            zones.rebuild_base_connectivity_preserving_hierarchy(path, terrain, records);
        }
        let cells: Vec<(i16, i16)> = cells.iter().map(|&(x, y)| (x as i16, y as i16)).collect();
        let mut batch = FirestormZoneBatch { sim: self, rules };
        if let Err(error) =
            crate::sim::pathfinding::zone_incremental::recalculate_zone_batch(&mut batch, &cells)
        {
            log::warn!("firestorm zone batch fell back to a rebuild: {error}");
            let _ = self.rebuild_dynamic_navigation(rules);
        }
    }

    /// RecalcZoneType's building arm for the cell at `(rx, ry)` (its
    /// building, CellClass::GetBuilding): a `Firestorm.Wall` section whose
    /// House's firestorm (HouseClass+0x1FA) is up.
    fn ares_firestorm_cell_impassable(&self, rx: u16, ry: u16, rules: &RuleSet) -> bool {
        self.ares_cell_building(i32::from(rx), i32::from(ry))
            .and_then(|id| self.substrate.entities.get(id))
            .is_some_and(|section| {
                self.ares_firestorm.is_active(section.owner())
                    && self
                        .object_type(section.type_ref(), rules)
                        .is_some_and(is_firewall_type)
            })
    }

    /// A restored world rebuilds its navigation from the terrain template,
    /// whose classes know no buildings: give every powered section's cell
    /// back the Impassable class its last Recalc gave it, before that
    /// rebuild.
    pub(crate) fn ares_firestorm_restore_zone_classes(&mut self, rules: &RuleSet) {
        if !self.ares_firestorm.any_active() {
            return;
        }
        let cells: Vec<(u16, u16)> = self
            .substrate
            .entities
            .iter_sorted()
            .filter(|(_, entity)| entity.category() == EntityCategory::Structure)
            .filter_map(|(id, _)| self.object_coords_cell(id))
            .filter(|&(rx, ry)| self.ares_firestorm_cell_impassable(rx, ry, rules))
            .collect();
        if let Some(terrain) = self.resolved_terrain.as_mut() {
            for cell in cells {
                terrain.recalc_zone_type_with_buildings_at(cell, true);
            }
        }
    }

    /// `BuildingExt::ExtData::UpdateFirewall @ 10013280`, which 3.0 calls
    /// only for `Firestorm.Wall` sections. `changed_state` false is the
    /// per-Update flicker: while powered, on a frame not divisible by 8, a
    /// section with no flicker (Building+0x588) that is not a straight piece
    /// draws one Scenario `Random()`; 1 in 16 starts `FirestormIdleAnim` 740
    /// leptons north-west of its centre (GetCoords, vt+0x48; delay 0, one
    /// loop). `changed_state` true recomputes the frame from its
    /// same-House neighbours (`GetFirewallFlags`, inlined) plus 32 while
    /// powered, and toggles the powered anim (Building+0x584): a powered,
    /// unstraight section without one starts `FirestormActiveAnim` 128
    /// leptons north-west of its centre (delay 1, endless) with the
    /// building's fog; any other that has one deletes it. Both anims are
    /// marked as building anims (Anim+0x118) and keep ZAdjust -10. A
    /// powered section then burns its victims.
    ///
    /// The frame change's `CellClass::Setup(-1)` and `UpdatePlacement(2)`
    /// redraw: VERA derives the cell attributes from terrain (the zone
    /// class follows in [`Self::ares_firestorm_update_pathfinding`]) and
    /// redraws every frame.
    pub(crate) fn ares_update_firewall(
        &mut self,
        id: u64,
        changed_state: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let owner = entity.owner();
        let active = self.ares_firestorm.is_active(owner);
        let globals = rules.ares_firestorm_globals();
        let centre = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        );
        if !changed_state {
            if !active {
                return;
            }
            let frame = self.session.binary_frame();
            if frame & 7 != 0
                && entity.building_art.anim_slots()[usize::from(IDLE_ANIM_SLOT)].is_none()
                && !entity.ares_firestorm_wall.is_straight()
                && self.scenario_rng.next_u32() & 0xF == 0
                && let Some(name) = globals.idle_anim()
            {
                let coord = crate::sim::anim_class::AnimWorldCoord {
                    x: centre.x.wrapping_sub(740),
                    y: centre.y.wrapping_sub(740),
                    z: centre.z,
                };
                if let Some(anim) = self.ares_firestorm_anim(rules, name, coord, 0, 1) {
                    self.install_ares_firewall_anim(id, IDLE_ANIM_SLOT, anim);
                }
            }
        } else {
            let flags = self.ares_firewall_flags(id, owner, rules)
                + if active { ACTIVE_FRAME_OFFSET } else { 0 };
            let Some(entity) = self.substrate.entities.get_mut(id) else {
                return;
            };
            entity.ares_firestorm_wall.store(flags);
            let straight = entity.ares_firestorm_wall.is_straight();
            let has_anim =
                entity.building_art.anim_slots()[usize::from(ACTIVE_ANIM_SLOT)].is_some();
            if active && !straight && !has_anim {
                if let Some(name) = globals.active_anim() {
                    let coord = crate::sim::anim_class::AnimWorldCoord {
                        x: centre.x.wrapping_sub(128),
                        y: centre.y.wrapping_sub(128),
                        z: centre.z,
                    };
                    if let Some(anim) = self.ares_firestorm_anim(rules, name, coord, 1, 0) {
                        self.fog.inherit_object_fog(id, anim);
                        self.install_ares_firewall_anim(id, ACTIVE_ANIM_SLOT, anim);
                    }
                }
            } else if has_anim {
                self.clear_building_anim_slot(id, ACTIVE_ANIM_SLOT);
            }
        }
        if active {
            self.ares_immolate_victims(id, owner, rules, registry, effects);
        }
    }

    /// `BuildingExt::GetFirewallFlags` (inlined in `10013280`): bit
    /// `direction / 2` for each straight neighbour cell of the section's
    /// Location whose building is a placed, live `Firestorm.Wall` of `owner`.
    fn ares_firewall_flags(&self, id: u64, owner: InternedId, rules: &RuleSet) -> u32 {
        let Some(entity) = self.substrate.entities.get(id) else {
            return 0;
        };
        let (x, y) = (
            i32::from(entity.position.rx()),
            i32::from(entity.position.ry()),
        );
        let mut flags = 0;
        for direction in STRAIGHT_DIRECTIONS {
            let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction];
            let Some(neighbour) = self.ares_cell_building(x + dx, y + dy) else {
                continue;
            };
            let linked = self.substrate.entities.get(neighbour).is_some_and(|other| {
                other.owner() == owner && !other.in_limbo() && other.is_object_alive()
            }) && self.ares_is_firewall(neighbour, rules);
            if linked {
                flags |= 1 << (direction >> 1);
            }
        }
        flags
    }

    /// `BuildingExt::ExtData::UpdateFirewallLinks @ 10013630`, the Put
    /// (hook `0x440D01`) and Remove (hook `0x445DF4`) of a building: for a
    /// `Firestorm.Wall` one, `UpdateFirewall(true)` of itself while placed
    /// and alive, then of each `Firestorm.Wall` building in a straight
    /// neighbour cell, whatever its House.
    pub(crate) fn ares_update_firewall_links(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if !self.ares_is_firewall(id, rules) {
            return;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let placed = !entity.in_limbo() && entity.is_object_alive();
        let (x, y) = (
            i32::from(entity.position.rx()),
            i32::from(entity.position.ry()),
        );
        if placed {
            self.ares_update_firewall(id, true, rules, registry, effects);
        }
        for direction in STRAIGHT_DIRECTIONS {
            let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction];
            if let Some(neighbour) = self.ares_cell_building(x + dx, y + dy)
                && self.ares_is_firewall(neighbour, rules)
            {
                self.ares_update_firewall(neighbour, true, rules, registry, effects);
            }
        }
    }

    /// `BuildingExt::ExtData::ImmolateVictims`, inlined at the end of
    /// `10013280`: each Foot in the section's cell (its ground list, the
    /// next link read before the burn) that is not in an open-topped
    /// transport (`+0x82`) and whose type is not `IgnoresFirestorm=`
    /// (`+0x239`) burns.
    fn ares_immolate_victims(
        &mut self,
        id: u64,
        owner: InternedId,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let cell = (entity.position.rx(), entity.position.ry());
        let mut bridge_changed = false;
        let mut next = self.cell_objects(cell, MovementLayer::Ground).next();
        while let Some(member) = next {
            next = self.next_cell_object(member);
            let CellObjectMember::Entity(victim) = member else {
                continue;
            };
            if self.ares_firestorm_burns(victim, rules) {
                bridge_changed |=
                    self.ares_immolate_victim(owner, victim, rules, registry, effects);
            }
        }
        bridge_changed
    }

    /// The Foot test both burn paths share: a Foot not in an open-topped
    /// transport whose type is not `IgnoresFirestorm=`.
    fn ares_firestorm_burns(&self, id: u64, rules: &RuleSet) -> bool {
        self.substrate.entities.get(id).is_some_and(|entity| {
            matches!(
                entity.category(),
                EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
            ) && !entity.passenger_role.in_open_transport()
                && !self
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.ignores_firestorm)
        })
    }

    /// `BuildingExt::ExtData::ImmolateVictim` (destroy true, inlined in
    /// `10013280` and called by `10014BA0`): a victim with Health above 0
    /// takes its Health as `FirestormWarhead=` (else `C4Warhead=`) damage
    /// with no attacker, ignoring defenses and keeping its passengers in,
    /// credited to the wall's House (`ReceiveDamage(&Health, 0, warhead,
    /// NULL, 1, 1, Owner)`), then `FirestormGroundAnim` (below 100 leptons,
    /// GetHeight vt+0x1C8) or `FirestormAirAnim` burns at its GetCoords.
    /// Returns whether a bridge changed.
    ///
    /// RESIDUAL: an explicit `FirestormWarhead=none` passes a null warhead
    /// natively; VERA drops the hit (as for every null warhead, see
    /// `combat::receiver_call`) and still shows the anim.
    fn ares_immolate_victim(
        &mut self,
        owner: InternedId,
        victim: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(victim) else {
            return false;
        };
        let health = entity.health.current();
        if health <= 0 {
            return false;
        }
        let terrain = self.resolved_terrain.as_ref();
        let coords = crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
        let height = crate::sim::movement::air_movement::current_fly_height(entity, terrain);
        let globals = rules.ares_firestorm_globals();
        let mut bridge_changed = false;
        if let Some(warhead) = globals.warhead(&rules.bridge_warheads.c4_name) {
            let event = crate::sim::combat::EntityDamageEvent::direct_receiver(
                victim,
                health,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                Some(owner),
                self.interner.intern(warhead),
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            );
            bridge_changed = self
                .commit_direct_damage_receiver(rules, registry, event, effects)
                .bridge_state_changed;
        }
        if let Some(name) = rules.ares_firestorm_globals().victim_anim(height) {
            self.spawn_named_anim(rules, name, coords.into(), 0, ANIM_FLAGS, ANIM_Z_ADJUST);
        }
        bridge_changed
    }

    /// `ImmolateVictim` with destroy false, for a bullet: one with Health
    /// (its damage) above 0 shows the victim anim at `at`, from its height
    /// over the ground there.
    fn ares_immolate_bullet(&mut self, bullet: u64, at: ProjectileCoord, rules: &RuleSet) {
        let Some(projectile) = self.projectiles.get(bullet) else {
            return;
        };
        if projectile.payload().base_damage <= 0 {
            return;
        }
        let on_bridge = projectile.on_bridge();
        let coord = crate::sim::components::DriveCoord {
            x: at.x,
            y: at.y,
            z: at.z,
        };
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        let height = crate::sim::movement::air_movement::object_height_in_query(
            coord,
            on_bridge,
            cells.as_ref(),
        );
        if let Some(name) = rules.ares_firestorm_globals().victim_anim(height) {
            self.spawn_named_anim(rules, name, coord.into(), 0, ANIM_FLAGS, ANIM_Z_ADJUST);
        }
    }

    /// Ares FootClass_Update (`10014BA0`, hook `0x4DA53E`), the firestorm
    /// arm ahead of the tiberium heal: while any firestorm is up, a live
    /// Foot that burns ([`Self::ares_firestorm_burns`]) standing in the cell
    /// of an active firestorm wall (of any House) is immolated. Returns
    /// whether a bridge changed.
    pub(crate) fn ares_foot_firestorm(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        if !self.ares_firestorm.any_active() || !self.ares_firestorm_burns(id, rules) {
            return false;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if !entity.is_object_alive() {
            return false;
        }
        let Some(wall) = self.ares_cell_building(
            i32::from(entity.position.rx()),
            i32::from(entity.position.ry()),
        ) else {
            return false;
        };
        if !self.ares_is_active_firestorm_wall(wall, None, rules) {
            return false;
        }
        let Some(owner) = self.substrate.entities.get(wall).map(GameEntity::owner) else {
            return false;
        };
        self.ares_immolate_victim(owner, id, rules, registry, effects)
    }

    /// Ares BuildingClass_ReceiveDamage_FSW (`10014A90`, hook `0x4423E7`),
    /// reached after the stock LaserFence and Immune repair hut returns: a
    /// hit on an active firestorm wall does nothing to it (`0x4423B7`
    /// returns 0); `F2I(damage * DamageToFirestormDamageCoefficient)`, when
    /// positive, comes off the charge timer of its House's first
    /// `Type=Firestorm` Super (HouseClass::FindSuperWeapon). Returns whether
    /// the wall took the hit.
    pub(crate) fn ares_firestorm_absorbs_damage(
        &mut self,
        target: u64,
        damage: i32,
        rules: &RuleSet,
    ) -> bool {
        if !self.ares_is_active_firestorm_wall(target, None, rules) {
            return false;
        }
        use crate::util::native_x87::MaskedX87Chop53 as X87;
        let coefficient = rules.ares_firestorm_globals().damage_coefficient();
        let amount =
            X87::fistp_i32_masked(X87::mul(X87::load_i32(damage), X87::load_f64(coefficient)));
        if amount > 0
            && let Some(owner) = self.substrate.entities.get(target).map(GameEntity::owner)
        {
            crate::sim::superweapon::shorten_ares_firestorm_drain(self, rules, owner, amount);
        }
        true
    }

    /// The replaced `MapClass::FindFirstFirestorm @ 0x005880A0` (Ares
    /// `10014DF0`): with any firestorm up and `start` not `end`, the centre
    /// (Z 0) of the first cell from `start`'s to `end`'s whose building is
    /// an active firestorm wall not of `ignore`. The cells follow
    /// `CellSequenceEnumerator` (YRpp `Helpers/Enumerators.h`): a
    /// Bresenham walk that steps one axis at a time.
    pub(crate) fn ares_find_first_firestorm(
        &self,
        start: ProjectileCoord,
        end: ProjectileCoord,
        ignore: Option<InternedId>,
        rules: &RuleSet,
    ) -> Option<(i32, i32)> {
        if !self.ares_firestorm.any_active() || start == end {
            return None;
        }
        // Coord2Cell divides toward zero.
        let cell = |coord: ProjectileCoord| ((coord.x / 256) as i16, (coord.y / 256) as i16);
        let (mut x, mut y) = cell(start);
        let (end_x, end_y) = cell(end);
        let (dx, dy) = (end_x.wrapping_sub(x), end_y.wrapping_sub(y));
        let (step_x, step_y) = (dx.signum(), dy.signum());
        let distance_x = dx.wrapping_mul(step_x).wrapping_mul(2);
        let distance_y = dy.wrapping_mul(step_y).wrapping_mul(2);
        let mut value = if distance_x > distance_y {
            i32::from(distance_y) - i32::from(distance_x) / 2
        } else {
            i32::from(distance_x) - i32::from(distance_y) / 2
        };
        loop {
            if let Some(building) = self.ares_cell_building(i32::from(x), i32::from(y))
                && self.ares_is_active_firestorm_wall(building, ignore, rules)
            {
                return Some((i32::from(x), i32::from(y)));
            }
            if (x, y) == (end_x, end_y) {
                return None;
            }
            if step_x == 0 || step_y == 0 {
                x = x.wrapping_add(step_x);
                y = y.wrapping_add(step_y);
            } else if distance_x > distance_y {
                if value < 0 {
                    value += i32::from(distance_y);
                    x = x.wrapping_add(step_x);
                } else {
                    value -= i32::from(distance_x);
                    y = y.wrapping_add(step_y);
                }
            } else if value < 0 {
                value += i32::from(distance_x);
                y = y.wrapping_add(step_y);
            } else {
                value -= i32::from(distance_y);
                x = x.wrapping_add(step_x);
            }
        }
    }

    /// The firer's House (Bullet Owner's Owner); none once the firer is
    /// gone.
    fn ares_bullet_house(&self, bullet: u64) -> Option<InternedId> {
        let projectile = self.projectiles.get(bullet)?;
        self.substrate
            .entities
            .get(projectile.source_id())
            .map(GameEntity::owner)
    }

    /// Ares TechnoClass_Fire_FSW (`10024C00`, hooks `0x6FF008` and
    /// `0x6FF860`), between TechnoClass::Fire's bullet construction and its
    /// BulletClass::Fire (vt+0x1F0): while any firestorm is up, a bullet
    /// whose type is not `IgnoresFirestorm=` that would cross an active
    /// firestorm wall not of its firer's House between `origin` (the launch
    /// coordinate, `[ESP+0x44]`) and `target` (`[ESP+0x88]`) takes the first
    /// object of that cell (CellClass::GetContent) as its Target, and the
    /// firer's next passive scan may drop its own target (`+0x50C`).
    pub(crate) fn ares_fire_at_firestorm(
        &mut self,
        bullet: u64,
        origin: ProjectileCoord,
        target: ProjectileCoord,
        rules: &RuleSet,
    ) {
        if !self.ares_firestorm.any_active() {
            return;
        }
        let Some(projectile) = self.projectiles.get(bullet) else {
            return;
        };
        let firer = projectile.source_id();
        if projectile
            .payload()
            .projectile_type()
            .and_then(|id| rules.projectile(self.interner.resolve(id)))
            .is_some_and(|kind| kind.bullet_type_ext.ignores_firestorm)
        {
            return;
        }
        let house = self.ares_bullet_house(bullet);
        let Some((x, y)) = self.ares_find_first_firestorm(origin, target, house, rules) else {
            return;
        };
        let content = self
            .native_cell_content((x as u16, y as u16))
            .next()
            .and_then(|member| match member {
                CellObjectMember::Entity(id) => Some(ProjectileTarget::Entity(id)),
                CellObjectMember::Terrain(_) => None,
            })
            .unwrap_or(ProjectileTarget::None);
        self.projectiles.ares_firestorm_retarget(bullet, content);
        if let Some(entity) = self.substrate.entities.get_mut(firer) {
            entity.passive_scan.ares_lose_target_now();
        }
    }

    /// Ares BulletClass_SetMovement_Obstacle (`1001A940`, hook `0x4688BD`),
    /// the firestorm arm ahead of the obstacle walk of an `Inviso=` bullet's
    /// BulletClass::Fire: an active firestorm wall not of its firer's House
    /// between its Location and `placement` (whatever its type's
    /// `IgnoresFirestorm=`) burns it at that cell's centre, on the cell's
    /// floor, and UnInits it without a detonation (`0x468A3F`). Returns
    /// whether it burnt.
    pub(crate) fn ares_inviso_firestorm(
        &mut self,
        bullet: u64,
        placement: ProjectileCoord,
        rules: &RuleSet,
    ) -> bool {
        let Some(origin) = self.projectiles.get(bullet).map(|bullet| bullet.position()) else {
            return false;
        };
        let house = self.ares_bullet_house(bullet);
        let Some((x, y)) = self.ares_find_first_firestorm(origin, placement, house, rules) else {
            return false;
        };
        // SetLocation at the cell centre, Z MapClass::GetCellFloorHeight
        // (`0x00578080`); the UnInit follows, so only the burn reads it.
        let centre = ProjectileCoord::new(x * 256 + 128, y * 256 + 128, 0);
        let floor = crate::sim::projectile::projectile_ground_z(
            self.resolved_terrain.as_ref(),
            &self.effective_shared_cell_dummy(),
            centre,
        );
        let at = ProjectileCoord::new(centre.x, centre.y, floor);
        self.ares_immolate_bullet(bullet, at, rules);
        let retired = self
            .retire_non_entity_object(bullet, crate::sim::world::UninitContext::with_rules(rules));
        debug_assert!(retired);
        true
    }

    /// Ares BulletClass_Update_Ranged (`1001AC90`, hook `0x467B94`), the
    /// firestorm arm: the flight committed `bullet` into an active
    /// firestorm wall's cell (`ProjectileCollisionPhase::AresFirestorm`);
    /// it burns there before its UnInit (`0x467FBA`).
    pub(crate) fn ares_burn_flying_bullet(&mut self, bullet: u64, rules: &RuleSet) {
        if let Some(at) = self.projectiles.get(bullet).map(|bullet| bullet.position()) {
            self.ares_immolate_bullet(bullet, at, rules);
        }
    }

    /// `BuildingExt::buildLines @ 100113D0`, after a building's Unlimbo by
    /// HouseClass::Place_Production (hook `0x4FB257`, `10014D50`) and by
    /// the computer's BuildingClass::ExitObject (hook `0x445355`,
    /// `10014A50`), with the placement `cell`: a linkable type
    /// (`IsLinkable 1000D770`, `Firestorm.Wall`) looks up to
    /// `GuardRange / 256` cells north, east, south and west. The first
    /// building it meets ends the look: one of the same or an allied House
    /// that is also `Firestorm.Wall` sets the line to the cells before it,
    /// any other leaves none; a missing cell (TryGetCellAt) or one the type
    /// cannot stand on (CellClass::CanThisExistHere, `0x0047C620`) ends it
    /// with none. Each line cell then gets a new section of the type
    /// (CreateObject), Put at its centre (Z 0, facing 0) inside the A8E7AC
    /// placement scope, then QueueMission(Construction), DiscoveredBy(the
    /// House) and the ready byte (`+0x6DD`). 3.0 checks neither the Put nor
    /// the cell: a refused section stays constructed in limbo.
    ///
    /// An `IsTrench` type links the same way to a trench of its kind
    /// (`IsLinkable`'s other arm and `canLinkTo`'s `sameTrench`, BuildingExt
    /// `+0x84`, both inlined here).
    pub(crate) fn ares_build_lines(
        &mut self,
        id: u64,
        cell: (u16, u16),
        owner: InternedId,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some((object, building_owner)) =
            self.substrate.entities.get(id).and_then(|entity| {
                Some((self.object_type(entity.type_ref(), rules)?, entity.owner()))
            })
        else {
            return;
        };
        if !object.building_type_ext().is_linkable() {
            return;
        }
        let max_distance = object.guard_range.map_or(0, |range| range.to_num::<i32>());
        let owner_name = self.interner.resolve(owner).to_owned();
        for direction in STRAIGHT_DIRECTIONS {
            let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction];
            let length = self.ares_link_length(
                object,
                building_owner,
                owner,
                cell,
                (dx, dy),
                max_distance,
                rules,
                registry,
            );
            for distance in 1..=length {
                let rx = (i32::from(cell.0) + dx * distance) as u16;
                let ry = (i32::from(cell.1) + dy * distance) as u16;
                let Some(section) = self.construct_object_limbo_at_height(
                    &object.id,
                    &owner_name,
                    rx,
                    ry,
                    0,
                    0,
                    rules,
                    effects,
                ) else {
                    continue;
                };
                let _ = self.with_object_placement_scope(|sim| {
                    sim.reveal_constructed_object_at_coord_with_overlay_context(
                        section,
                        crate::sim::components::DriveCoord {
                            x: i32::from(rx).wrapping_mul(256).wrapping_add(128),
                            y: i32::from(ry).wrapping_mul(256).wrapping_add(128),
                            z: 0,
                        },
                        0,
                        crate::sim::world::PlacementEvidence::EvaluateMark,
                        rules,
                        registry,
                        effects,
                    )
                });
                if let Some(entity) = self.substrate.entities.get_mut(section) {
                    crate::sim::mission::authority::queue_entity_mission_deferred(
                        entity,
                        crate::sim::mission::MissionId::from_known(
                            crate::sim::mission::MissionType::Construction,
                        ),
                    );
                }
                self.discover_techno(
                    section,
                    owner,
                    crate::sim::world::UninitContext::new(rules, registry).with_effects(effects),
                );
                if let Some(entity) = self.substrate.entities.get_mut(section) {
                    entity.mission_leaf.set_building_ready_latch(1);
                }
            }
        }
    }

    /// buildLines' look in one direction: the line's length.
    fn ares_link_length(
        &self,
        object: &ObjectType,
        building_owner: InternedId,
        owner: InternedId,
        cell: (u16, u16),
        (dx, dy): (i32, i32),
        max_distance: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> i32 {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return 0;
        };
        let query = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        for distance in 1..=max_distance {
            let x = i32::from(cell.0) + dx * distance;
            let y = i32::from(cell.1) + dy * distance;
            let Some(index) = terrain.native_fixed_cell_index(x as i16, y as i16) else {
                return 0;
            };
            let found = &terrain.cells()[index];
            if let Some(other) = self.substrate.occupancy.first_building_on_layer(
                found.rx,
                found.ry,
                MovementLayer::Ground,
            ) {
                // canLinkTo (inlined): the same or an allied House, both
                // `Firestorm.Wall` or trenches of one kind.
                let linked = self.substrate.entities.get(other).is_some_and(|other| {
                    (other.owner() == building_owner
                        || self.house_alliances.is_allied(
                            &self.interner,
                            building_owner,
                            other.owner(),
                        ))
                        && self
                            .object_type(other.type_ref(), rules)
                            .is_some_and(|other| {
                                let (this, other) =
                                    (object.building_type_ext(), other.building_type_ext());
                                (this.firestorm_wall && other.firestorm_wall)
                                    || this.same_trench(other)
                            })
                });
                return if linked { distance - 1 } else { 0 };
            }
            if !crate::sim::build_site::is_clear_to_build(
                self,
                rules,
                registry,
                NativeCellIdentity::Real(index),
                crate::sim::build_site::building_speed_type(object),
                Some(object),
                Some(owner),
                Some(&query),
                None,
            ) {
                return 0;
            }
        }
        0
    }

    /// `FirestormWall_PlacementShadow @ 0x006D59D0` as Ares sends a linkable
    /// type there (sub_6D5030 `10015140`, hook `0x6D5455`, in place of the
    /// stock `FirestormWall=`) and tests the linkable endpoint (sub_6D59D0
    /// `10015170`, hook `0x6D5A5C`): the PLACE markers of the active
    /// placement. North, east, south and west, up to `GuardRange / 256`
    /// cells: a linkable building (a `Firestorm.Wall` section or a trench of
    /// any kind) of a human House ends the look and marks the cells before
    /// it; a cell that is not clear to build for the placing House, or
    /// reaching the range, marks none.
    pub(crate) fn ares_firestorm_placement_shadow(
        &self,
        object: &ObjectType,
        origin: (u16, u16),
        owner: InternedId,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        query: &crate::map::resolved_terrain::NativeCellQuery<'_>,
    ) -> Vec<(u16, u16)> {
        let range = object.guard_range.map_or(0, |range| range.to_num::<i32>());
        let mut cells = Vec::new();
        for direction in STRAIGHT_DIRECTIONS {
            let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction];
            let mut count = 0;
            if range <= 0 {
                continue;
            }
            let ended = loop {
                let x = i32::from(origin.0) + dx * (count + 1);
                let y = i32::from(origin.1) + dy * (count + 1);
                let identity = query.lookup((x as i16, y as i16));
                let building = match identity {
                    NativeCellIdentity::Real(index) => {
                        let found = &query.terrain().cells()[index];
                        self.substrate.occupancy.first_building_on_layer(
                            found.rx,
                            found.ry,
                            MovementLayer::Ground,
                        )
                    }
                    NativeCellIdentity::Dummy => None,
                };
                if let Some(building) = building
                    && self.ares_is_linkable(building, rules)
                    && self
                        .substrate
                        .entities
                        .get(building)
                        .and_then(|entity| self.houses.get(&entity.owner()))
                        .is_some_and(|house| house.is_human())
                {
                    break true;
                }
                if !crate::sim::build_site::is_clear_to_build(
                    self,
                    rules,
                    registry,
                    identity,
                    crate::sim::build_site::building_speed_type(object),
                    Some(object),
                    Some(owner),
                    Some(query),
                    None,
                ) {
                    break false;
                }
                count += 1;
                if range <= count {
                    break false;
                }
            };
            if ended {
                cells.extend((1..=count).map(|distance| {
                    (
                        (i32::from(origin.0) + dx * distance) as u16,
                        (i32::from(origin.1) + dy * distance) as u16,
                    )
                }));
            }
        }
        cells
    }

    /// One firestorm anim (flags 0x600, ZAdjust -10) at `coord`.
    fn ares_firestorm_anim(
        &mut self,
        rules: &RuleSet,
        name: &str,
        coord: crate::sim::anim_class::AnimWorldCoord,
        delay: u16,
        loop_count: i32,
    ) -> Option<crate::sim::anim_class::AnimId> {
        let type_name = self.interner.intern(name);
        let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
        let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
            delay,
            loop_count,
            draw_flags: ANIM_FLAGS,
            z_adjust: ANIM_Z_ADJUST,
            reverse: false,
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_name, rx, ry, sub_x, sub_y, z,
            )
        };
        match self.spawn_anim_at_world(rules, descriptor, coord) {
            Ok(anim) => Some(anim),
            Err(error) => {
                log::debug!("firestorm anim [{name}] did not construct: {error}");
                None
            }
        }
    }
}

/// The ordered callbacks of `0x00586990` for SetFirestormState's batch
/// ([`Simulation::ares_firestorm_update_pathfinding`]): the shared batch
/// order, the fine-zone clear and the hierarchy patch around each cell
/// (`0x00584550`), with Recalc's class from the building arm.
struct FirestormZoneBatch<'a> {
    sim: &'a mut Simulation,
    rules: &'a RuleSet,
}

impl crate::sim::pathfinding::zone_incremental::ZoneBatchHost for FirestormZoneBatch<'_> {
    type Error = String;

    fn admitted(&mut self, coord: (i16, i16)) -> Result<bool, String> {
        let terrain = self
            .sim
            .resolved_terrain
            .as_ref()
            .ok_or("firestorm zone batch has no terrain")?;
        Ok(crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(coord.0), i32::from(coord.1)),
            self.sim.playfield_bounds(),
            Some(terrain),
        ))
    }

    fn clear_fine_zone(&mut self, coord: (i16, i16)) -> Result<(), String> {
        let zones = self
            .sim
            .zone_grid
            .as_deref_mut()
            .ok_or("firestorm zone batch has no zones")?;
        let (base, hierarchy) = zones.base_and_hierarchy_mut();
        hierarchy.levels_mut()[0]
            .set_native_zone_at(coord, base.native_bridge_source_size(), 0)
            .ok_or_else(|| "firestorm zone batch coordinate has no native zone storage".into())
    }

    fn recalc_at(&mut self, (x, y): (i16, i16)) -> Result<(), String> {
        let (Ok(rx), Ok(ry)) = (u16::try_from(x), u16::try_from(y)) else {
            return Ok(());
        };
        let impassable = self.sim.ares_firestorm_cell_impassable(rx, ry, self.rules);
        if let Some(terrain) = self.sim.resolved_terrain.as_mut() {
            terrain.recalc_zone_type_with_buildings_at((rx, ry), impassable);
        }
        self.sim.publish_recalculated_cells(self.rules, &[(rx, ry)]);
        Ok(())
    }

    fn fine_zone(
        &self,
        coord: (i16, i16),
    ) -> Result<crate::sim::pathfinding::zone_map::ZoneId, String> {
        self.sim
            .zone_grid
            .as_deref()
            .and_then(|zones| zones.hierarchy_zone_at_native(0, (coord.0 as u16, coord.1 as u16)))
            .ok_or_else(|| "firestorm zone batch has no native fine-zone storage".into())
    }

    fn patch_hierarchy(&mut self, coord: (i16, i16)) -> Result<(), String> {
        let bounds = self.sim.playfield_bounds();
        let sim = &mut *self.sim;
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .ok_or("firestorm zone batch has no terrain")?;
        let records = sim
            .bridge_state
            .as_ref()
            .map_or(&[][..], |bridges| bridges.endpoint_records());
        let zones = sim
            .zone_grid
            .as_deref_mut()
            .ok_or("firestorm zone batch has no zones")?;
        crate::sim::pathfinding::zone_incremental::repair_zone_hierarchy_around_cell(
            zones, coord, bounds, terrain, records,
        );
        Ok(())
    }
}

#[cfg(test)]
#[path = "ares_firestorm_tests.rs"]
mod tests;
