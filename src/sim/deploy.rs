//! Infantry deployment actions and the shared DEPLOY event receiver.
//!
//! Infantry's Doing and shared Techno Stage own its sequence. The completion
//! receiver applies the independent crush byte and passive-scan timer effects,
//! including when its requested next action refuses. Unit simple deployment
//! uses its Mission leaf, the same Techno Stage and attached AnimClass.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::combat::{TargetKind, combat_weapon};
use crate::sim::intern::StringInterner;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::FrameChannel;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::{SimSoundEvent, Simulation};

/// Techno `+0x2A4`, a deployed infantryman's crush immunity, which the crush
/// test reads (`movement::bump_crush`). Construction clears it. The
/// deployment completion of an infantryman whose type is not
/// `DeployedCrushable=` grants it after the Deployed request ([`Self::grant`],
/// `0x00520B4E`) and revokes it after the Ready request ([`Self::revoke`],
/// `0x00520BAD`), even when the requested sequence refuses. It is retained
/// independently of Doing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub(crate) struct CrushImmunity(u8);

impl CrushImmunity {
    /// A fixture's raw byte.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn from_raw(raw: u8) -> Self {
        Self(raw)
    }

    /// The raw byte.
    pub(crate) const fn get(self) -> u8 {
        self.0
    }

    /// The Deployed completion's store of 1 (`0x00520B4E`).
    pub(crate) fn grant(&mut self) {
        self.0 = 1;
    }

    /// The Ready completion's store of 0 (`0x00520BAD`).
    pub(crate) fn revoke(&mut self) {
        self.0 = 0;
    }
}

/// Event9 DEPLOY4C76BC..4C7812. MCV conversion, simple deployment, the
/// deploy-fire arm, Unit transports and a tank bunker's occupant share this
/// event; Infantry uses its existing Mission_Unload deployment branch too.
/// Each concrete Mission_Unload branch owns its effects. A transport's
/// empty hold does not suppress this event: it still stops and queues Unload.
/// Native controls: tools/spatial_oracle/transport_unload.py --deploy-event.
///
/// The object must be alive (`+0x90`), out of limbo (`+0x81`), untethered
/// (`+0x418`) and not under EMP (`+0x504`, which nothing writes yet). Then it
/// sends OVER_OUT to its radio contact 0 (vt+0x274(3), `0x004C77E4`).
///
/// RESIDUAL: native refuses only an aircraft (`0x004C777E`) and the
/// Construction/Selling missions below; VERA admits Infantry and the Unit kinds
/// whose Unload arms it ports: an MCV, a simple deployer, a deploy-fire unit, a
/// Unit transport and a unit in a tank bunker. Other Units fail the deploy key's
/// CanDeploySlashUnload query; their externally supplied DEPLOY events remain
/// outside these represented Unload arms. Self-click still uses its separate
/// clicked-Mission owner and requires cargo for a transport.
pub(crate) fn issue_order(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    effects: FrameEffects<'_>,
) -> bool {
    if !sim.substrate.entities.get(id).is_some_and(|e| {
        e.lifecycle.object_alive()
            && !e.lifecycle.in_limbo()
            && e.tether.partner().is_none()
            && (e.category() == EntityCategory::Infantry
                || crate::sim::mcv_deploy::is_mcv(sim, e, rules)
                || crate::sim::unit_simple_deploy::is_simple_deployer(sim, e, rules)
                || unit_fires_on_deploy(sim, e, rules)
                || (e.category() == EntityCategory::Unit
                    && (e.bunker.link().installed_in().is_some()
                        || crate::sim::transport_unload::is_vehicle_transport_type(sim, e, rules))))
    }) {
        return false;
    }
    // Event9 4C771C..4C7756 checks the current cell's slope before the
    // Techno70C620 terrain projection query. It does not test health/dying.
    // EMP504 has no active state producer in this engine yet.
    if let Some(terrain) = sim.resolved_terrain.as_ref() {
        let actor = sim.substrate.entities.get(id).expect("admitted Unit");
        if !actor.on_bridge() {
            let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
            let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&actor.position);
            if cells.ground_fields(cells.lookup_world(x, y)).1 == 0
                && crate::sim::movement::ground_pose::terrain_projection_differs(&cells, [x, y])
            {
                return false;
            }
        }
    }
    //4C7768..4C7774 follows the projection query and its map lookup effects.
    if matches!(
        sim.substrate
            .entities
            .get(id)
            .expect("admitted Unit")
            .mission
            .current()
            .known(),
        Some(MissionType::Construction | MissionType::Selling)
    ) {
        return false;
    }
    // `0x004C778A..0x004C77DE`: the event is ignored while the Unit's cell
    // holds a WeaponsFactory= building (Cell_Building `0x0047C520`), so an
    // MCV still leaving its factory stays. Only the first ground Building's
    // type is tested; Mark owns membership independently of death state.
    let (rx, ry) = {
        let entity = sim.substrate.entities.get(id).unwrap();
        (entity.position.rx(), entity.position.ry())
    };
    if sim
        .substrate
        .occupancy
        .first_building_on_layer(rx, ry, MovementLayer::Ground)
        .and_then(|building| sim.substrate.entities.get(building))
        .and_then(|building| sim.object_type(building.type_ref(), rules))
        .is_some_and(|kind| kind.weapons_factory)
    {
        return false;
    }
    // Event DEPLOY: OVER_OUT to radio contact 0 (`0x004C77E4`), then the
    // Unit's class setter takes a null destination (`0x004C77F8`, Unit
    // `0x00741970`) and its class target setter a null target (`0x004C7804`)
    // before Queue_Mission(Unload) (`0x004C7812`).
    crate::sim::radio::transmit_to_contact(
        sim,
        id,
        crate::sim::radio::RadioMessage::Break,
        rules,
        effects,
    );
    // Queue's same-mission guard keeps the handler and timer on a repeated D.
    sim.assign_null_destination(id, rules, None, effects);
    let _ = sim.assign_target_represented(id, None, rules, None, effects);
    sim.mission_queue_exact(
        id,
        MissionId::from_known(MissionType::Unload),
        0,
        sim.session.binary_frame(),
        &crate::sim::mission::authority::LiveReadyInputProvider { rules },
    )
    .is_ok()
}

/// The head of `UnitClass::Mission_Unload @ 0x0073D630`: a Unit in no tank
/// bunker (`+0x2E4`) whose type has `Passengers=` below 1 and `DeployFire=`
/// without `IsSimpleDeployer=` takes the deploy-fire arm
/// ([`Simulation::unit_deploy_fire_unload`]) before every other Unload arm.
/// Retail: the Chaos Drone (`[CAOS]`).
pub fn unit_fires_on_deploy(
    sim: &Simulation,
    entity: &crate::sim::game_entity::GameEntity,
    rules: &RuleSet,
) -> bool {
    entity.category() == EntityCategory::Unit
        && entity.bunker.link().installed_in().is_none()
        && sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| {
                object.passengers < 1 && object.deploy_fire && !object.is_simple_deployer
            })
}

impl Simulation {
    /// The object's own cell: GetMapCoords (vt+0x1B8) through
    /// MapClass::GetCellAt (`0x005657A0`), whose miss is the shared dummy cell
    /// stamped with the coordinate. None without terrain or a navigation
    /// coordinate.
    pub(crate) fn own_map_cell(&self, id: u64) -> Option<(u16, u16)> {
        let coord = self.foot_navigation_coordinate(id).ok()?;
        self.packed_map_cell(coord.x, coord.y)
    }

    /// `MapClass::Get_CellClass @ 0x005657A0` for world XY divided by 256
    /// toward zero and packed to 16 bits: the real cell, or the stamped shared
    /// Dummy. `None` only without terrain.
    pub(crate) fn packed_map_cell(&self, x: i32, y: i32) -> Option<(u16, u16)> {
        let terrain = self.resolved_terrain.as_ref()?;
        let cell = terrain.native_cell_identity(((x / 256) as i16, (y / 256) as i16));
        let at = terrain.native_cell_coord(cell);
        Some((at.0 as u16, at.1 as u16))
    }

    /// `UnitClass::Mission_Unload`'s deploy-fire arm
    /// (`0x0073DCD3..0x0073DD5A`, admitted by [`unit_fires_on_deploy`]): the
    /// unit targets its own cell (vt+0x3C8), fires weapon 0 at the Target it
    /// then holds when GetFireError (vt+0x3C0, range checked) answers OK
    /// (FireAt vt+0x3CC), keeps that Target and returns one Scenario draw
    /// `RandomRanged(0, 2)` plus 14. Returns the delay and whether the shot
    /// changed bridge state.
    pub(crate) fn unit_deploy_fire_unload(
        &mut self,
        id: u64,
        rules: &RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> (i32, bool) {
        let mut bridge_changed = false;
        // Native always has a cell; without one VERA fires at nothing rather
        // than at a Target the unit held before.
        let targeted = self.own_map_cell(id).is_some_and(|(rx, ry)| {
            self.assign_target_represented(
                id,
                Some(TargetKind::Cell(rx, ry)),
                rules,
                overlay_registry,
                effects,
            )
            .is_ok()
        });
        let shot = targeted
            .then(|| self.substrate.entities.get(id))
            .flatten()
            .and_then(|actor| {
                let object = self.object_type(actor.type_ref(), rules)?;
                let target = actor.attack_target.target()?;
                let subject = crate::sim::combat::fire_error_world::FireSubject {
                    world: self,
                    rules,
                    overlay_registry,
                    fog: None,
                    firer: actor,
                    obj: object,
                    target: Some(target),
                    weapon_index: 0,
                };
                (subject.fire_error(true) == crate::sim::combat::fire_error::FireError::Ok)
                    .then_some(target)
            });
        if let Some(target) = shot {
            bridge_changed = self
                .commit_fire_visit(
                    crate::sim::combat::world_fire::FireVisit::Direct {
                        id,
                        target,
                        weapon_index: 0,
                    },
                    rules,
                    overlay_registry,
                    effects,
                )
                .bridge_state_changed;
        }
        (
            self.scenario_rng.next_range_u32_inclusive(0, 2) as i32 + 14,
            bridge_changed,
        )
    }
}

impl Simulation {
    /// `TechnoClass::CanDeploySlashUnload @ 0x00700D50` (vt+0x314, no class
    /// overrides it): may this object deploy or unload now? Its callers are
    /// the deploy key (`0x00730C74`), Unit self What_Action (`0x0073FFEA`,
    /// refusal answers NoDeploy) and `UnitClass::Deploy` (`0x007393CC`).
    ///
    /// Every class first refuses at a tunnel mouth (Cell `0x00484AE0`).
    /// - Unit: refused in a tube (Foot `+0x684 >= 0`); needs DeploysInto,
    ///   Passengers, `IsSimpleDeployer=`, a tank-bunker link (`+0x2E4`) or
    ///   `DeployFire=` (`0x00746D00`, Type `+0x6AC`). With DeploysInto it is
    ///   refused while a parasite eats it (`+0x694`) or, when DeploysInto is a
    ///   `ConstructionYard=`, while mind-controlled (`+0x2C0`). With
    ///   `Passengers=` above 0 a transport is also refused on a bridge
    ///   (`+0x8C`), beside a bridge deck (`CellClass+0x140` bit `0x100` on
    ///   its cell or a N/S/E/W neighbour, `0x00700F28..0x00701064`), on Water
    ///   when its owner's IsHuman byte is set (House `+0x1EC`, `0x0070106A`),
    ///   and in a building's cell unless the building is a `Bunker=`
    ///   (`0x00701087..0x007010BB`).
    /// - Building: `0x00465D30` always refuses.
    /// - Infantry: a type with `UndeployDelay=` above -1 (`0x005224F0`) is
    ///   refused while deployed (Doing 27..30, `0x00522510`), in Doing 31, or
    ///   while its rearm timer runs (`+0x2EC`); then `Deployer=` decides
    ///   (`0x005224E0`).
    /// - Aircraft: `Passengers=` other than 0.
    ///
    /// `vt+0x310` (`0x00700D10`), the deploy key's other test, refuses only a
    /// Unit in a tube or under EMP, which this already refuses.
    ///
    /// The feature-gated Ares producer reaches the existing Unit EMP refusal.
    /// Aircraft EMP and Unit DeathFrame's separate virtual override remain
    /// outside the ordinary live ground-victim chain.
    pub fn can_deploy_slash_unload(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return false;
        };
        let (rx, ry) = (entity.position.rx(), entity.position.ry());
        let terrain = self.resolved_terrain.as_ref();
        if terrain
            .is_some_and(|terrain| terrain.is_tube_deploy_neighborhood((rx as i16, ry as i16)))
        {
            return false;
        }
        match entity.category() {
            EntityCategory::Unit => {
                //700E47..700E53 invokes Unit746C90 -> Techno70EFD0 before
                // admission. For the live Drive victim, positive+504 vetoes
                // both AI Hunt deploy and Guard→Unload through this owner.
                // Active-retail EMP producer reachability is not established;
                // default builds retain their current constructor-zero path.
                #[cfg(feature = "ares")]
                if entity.emp_remaining() > 0 {
                    return false;
                }
                if entity.low_bridge_tube_state.is_some() {
                    return false;
                }
                let bunkered = entity.bunker.link().installed_in().is_some();
                if object.deploys_into.is_none()
                    && object.passengers == 0
                    && !object.is_simple_deployer
                    && !bunkered
                    && !object.deploy_fire
                {
                    return false;
                }
                if object.deploys_into.is_some()
                    && (entity.parasite.eater().is_some()
                        || (crate::sim::mcv_deploy::deploys_into(self, entity, rules)
                            .is_some_and(|yard| yard.construction_yard)
                            && entity.mind_control.controller().is_some()))
                {
                    return false;
                }
                if object.passengers <= 0 {
                    return true;
                }
                if entity.on_bridge() {
                    return false;
                }
                if let Some(terrain) = terrain
                    && let Some(own) = terrain.native_fixed_cell_index(rx as i16, ry as i16)
                {
                    let cell = |index: usize| &terrain.cells()[index];
                    let (x, y) = (rx as i16, ry as i16);
                    let near_deck = std::iter::once(Some(own))
                        .chain([4, 0, 2, 6].map(|direction| {
                            let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction];
                            terrain.native_fixed_cell_index(x + dx as i16, y + dy as i16)
                        }))
                        .flatten()
                        .any(|index| cell(index).bridge_facts.has_structural_bridge());
                    if near_deck
                        || (self
                            .houses
                            .get(&entity.owner())
                            .is_some_and(|house| house.is_human())
                            && cell(own).land_type == LandType::Water.as_index())
                    {
                        return false;
                    }
                }
                !self
                    .substrate
                    .occupancy
                    .first_building_on_layer(rx, ry, MovementLayer::Ground)
                    .and_then(|building| self.substrate.entities.get(building))
                    .and_then(|building| self.object_type(building.type_ref(), rules))
                    .is_some_and(|building| !building.bunker)
            }
            EntityCategory::Structure => false,
            EntityCategory::Infantry => {
                if object.undeploy_delay > -1
                    && (entity.infantry_deploy_doing()
                        || entity
                            .mission_leaf
                            .as_infantry()
                            .is_some_and(|leaf| leaf.doing() == 31)
                        || entity
                            .rearm_timer
                            .remaining(self.session.binary_frame() as i32)
                            != 0)
                {
                    return false;
                }
                object.deployer
            }
            EntityCategory::Aircraft => object.passengers != 0,
        }
    }
}

impl Simulation {
    /// Shared DeploySound/UndeploySound receiver. Infantry Do_Action51D939
    /// requests it for an admitted changed action; Unit739AC0/739CD0 request
    /// it on each admitted updater visit. Every site plays at the actor's own
    /// Location: Infantry `0x0051D94F..0x0051D9CA`, Unit deploy
    /// `0x00739C86..0x00739CB9`, Unit undeploy `0x00739E81..0x00739EB3`.
    pub(crate) fn emit_deploy_action_sound(
        interner: &mut StringInterner,
        substrate: &ObjectSubstrate,
        terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
        sound_events: &mut FrameChannel<SimSoundEvent>,
        id: u64,
        requested: i32,
        rules: &RuleSet,
    ) -> Result<(), String> {
        if !matches!(requested, 27 | 31) {
            return Ok(());
        }
        let actor = substrate
            .entities
            .get(id)
            .ok_or("deployment sound receiver retired")?;
        let object = interner
            .object_type(actor.type_ref(), rules)
            .ok_or("deployment sound requires the object type")?;
        let sound = if requested == 27 {
            object.deploy_sound.as_deref()
        } else {
            object.undeploy_sound.as_deref()
        };
        let Some(sound) = sound else {
            return Ok(());
        };
        let world = crate::sim::movement::ground_pose::object_location(actor, terrain).into();
        let sound_id = interner.intern(sound);
        sound_events.emit(if requested == 27 {
            SimSoundEvent::EntityDeployed {
                deploy_sound_id: sound_id,
                world,
            }
        } else {
            SimSoundEvent::EntityUndeployed {
                undeploy_sound_id: sound_id,
                world,
            }
        });
        Ok(())
    }

    /// Infantry vtable+54C = Stop callback521B40. Clear6E4 BEFORE calling
    /// unforced Do_Action27: a zero-health action may re-enter Stop_Driver.
    pub(crate) fn infantry_pending_deploy_stop_callback(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("pending deployment Stop receiver retired")?;
        if !actor
            .mission_leaf
            .as_infantry()
            .is_some_and(|leaf| leaf.pending_deploy() != 0)
        {
            return Ok(());
        }
        self.substrate
            .entities
            .get_mut(id)
            .expect("same pending deployment receiver")
            .mission_leaf
            .take_infantry_pending_deploy();
        self.infantry_do_action(id, 27, false, rules, effects)?;
        Ok(())
    }

    /// Admitted completion arms of DoType_Sequencer520AE0. Its owner tests
    /// signed Stage/count before entering here. The suffix is independent of
    /// Do_Action's return:27→28 then2A4/timer70F770;31→0 then2A4 only.
    /// Original controls: tools/spatial_oracle/infantry_deploy_action.json.
    pub(crate) fn infantry_deploy_completion(
        &mut self,
        id: u64,
        completed_doing: i32,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let next = match completed_doing {
            27 => 28,
            31 => 0,
            other => return Err(format!("deployment completion received Doing{other}")),
        };
        self.infantry_do_action(id, next, true, rules, effects)?;
        //520B3B/520B9A re-read the type AFTER the concrete action receiver.
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("deployment completion receiver retired")?;
        let crushable = self
            .object_type(actor.type_ref(), rules)
            .ok_or("deployment completion requires the Infantry type")?
            .deployed_crushable;
        if !crushable {
            let actor = self
                .substrate
                .entities
                .get_mut(id)
                .expect("same deployment completion receiver");
            if completed_doing == 27 {
                actor.native_crush_immunity.grant();
            } else {
                actor.native_crush_immunity.revoke();
            }
        }
        if completed_doing == 27 {
            self.shorten_passive_scan_timer(id);
        }
        Ok(())
    }

    /// Original Infantry Mission_Unload51F6E0. Action admission does not gate
    /// the subsequent weapon, direct Mission::Assign(Guard), or NULL class
    /// destination effects. FootUnload4DA2B0→Mission5B2EF0 returns450.
    pub(crate) fn infantry_mission_unload(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) -> Result<i32, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("Infantry Unload receiver retired")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Infantry Unload requires the type")?;
        let doing = actor
            .mission_leaf
            .as_infantry()
            .ok_or("Infantry Unload requires Infantry Doing")?
            .doing();
        if !object.deployer {
            return Ok(450);
        }
        let mut delay = -1;
        if (27..=30).contains(&doing) {
            if object.undeploy_delay <= -1 {
                self.infantry_do_action(id, 31, true, rules, effects)?;
            }
        } else {
            self.infantry_do_action(id, 27, true, rules, effects)?;
            //70E120 uses the SprayAttack selector70DD70, not GetCurrentWeapon
            //70E1A0. The shared owner forwards it through GetWeapon70E140.
            let actor = self.substrate.entities.get(id).expect("same Unload actor");
            let (_, deployed_weapon) = combat_weapon::deploy_weapon(object, actor.veterancy.rank());
            let area_fire = deployed_weapon
                .and_then(|(weapon, _)| rules.weapon(weapon))
                .is_some_and(|weapon| weapon.area_fire);
            if area_fire {
                //5F3E50→410A40 compares native literal82557C "DESO".
                if object.id.eq_ignore_ascii_case("DESO") {
                    delay = rules
                        .animation_sequence(&object.id)
                        .and_then(|set| set.infantry_action(27))
                        .ok_or("DESO Unload requires its native Deploy sequence record")?
                        .frames_per_facing
                        .wrapping_add(1);
                } else {
                    let coord = self.foot_navigation_coordinate(id)?;
                    let requested = ((coord.x / 256) as i16, (coord.y / 256) as i16);
                    let terrain = self
                        .resolved_terrain
                        .as_ref()
                        .ok_or("AreaFire Unload requires map cells")?;
                    let cell = terrain.native_cell_identity(requested);
                    let at = terrain.native_cell_coord(cell);
                    self.assign_target_represented(
                        id,
                        Some(TargetKind::Cell(at.0 as u16, at.1 as u16)),
                        rules,
                        None,
                        effects,
                    )
                    .map_err(|cause| format!("AreaFire Unload target: {cause}"))?;
                }
            }
            if object.undeploy_delay > -1 {
                delay = object.undeploy_delay;
            }
        }
        self.substrate
            .mission_assign_exact(
                id,
                MissionId::from_known(MissionType::Guard),
                self.session.binary_frame(),
            )
            .map_err(|cause| format!("Infantry Unload Guard: {cause}"))?;
        self.assign_destination_represented(id, None, rules, None, effects)
            .map_err(|cause| format!("Infantry Unload destination: {cause}"))?;
        Ok(if delay > -1 { delay } else { 450 })
    }
}
