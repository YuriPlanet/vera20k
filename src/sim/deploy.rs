//! Infantry deployment actions and the shared Unit DEPLOY event receiver.
//!
//! Infantry's Doing and shared Techno Stage own its sequence. The completion
//! receiver applies the independent crush byte and passive-scan timer effects,
//! including when its requested next action refuses. Unit simple deployment
//! uses its Mission leaf, the same Techno Stage and attached AnimClass.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::sim::combat::{TargetKind, combat_weapon};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::FrameEffects;
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

/// Event9 DEPLOY4C778A..4C7812. MCV conversion, simple deployment and the
/// deploy-fire arm share this event; each concrete Mission_Unload branch owns
/// its effects.
///
/// RESIDUAL:
/// - Native refuses only an aircraft (`0x004C777E`) and the
///   Construction/Selling missions below; VERA admits only those three Unit
///   kinds, whose Unload arms it ports. Trigger: a DEPLOY event for another
///   object, which no VERA sender issues.
/// - Native breaks radio contact (vt+0x274(3), `0x004C77E4`) and deploys;
///   VERA refuses a unit with a radio partner. Trigger: D on a unit in radio
///   contact, such as one docked at a service depot. Effect: it ignores D.
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
            && (crate::sim::mcv_deploy::is_mcv(sim, e, rules)
                || crate::sim::unit_simple_deploy::is_simple_deployer(sim, e, rules)
                || unit_fires_on_deploy(sim, e, rules))
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
    // Event DEPLOY: the Unit's class setter takes a null destination
    // (`0x004C77F8`, Unit `0x00741970`) and its class target setter a null
    // target (`0x004C7804`) before Queue_Mission(Unload) (`0x004C7812`).
    // Queue's same-mission guard keeps the handler and timer on a repeated D.
    sim.assign_null_destination(id, Some(rules), None, effects);
    let _ = sim.assign_target_represented(id, None, Some(rules), None, effects);
    sim.mission_queue_exact(
        id,
        MissionId::from_known(MissionType::Unload),
        0,
        sim.session.binary_frame,
        &crate::sim::mission::authority::EntityReadyInputProvider,
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
        let terrain = self.resolved_terrain.as_ref()?;
        let cell = terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
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
                Some(rules),
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
    /// RESIDUAL: the EMP refusal (vt+0x37C) of Units and Aircraft is not
    /// represented: EMP has no Rust producer.
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
                            .is_some_and(|house| house.is_human)
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
                            .remaining(self.session.binary_frame as i32)
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
    /// it on each admitted updater visit.
    pub(crate) fn emit_deploy_action_sound(
        &mut self,
        id: u64,
        requested: i32,
        rules: &RuleSet,
    ) -> Result<(), String> {
        if !matches!(requested, 27 | 31) {
            return Ok(());
        }
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("deployment sound receiver retired")?;
        let object = self
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
        let (rx, ry) = (actor.position.rx(), actor.position.ry());
        let sound_id = self.interner.intern(sound);
        self.sound_events.push(if requested == 27 {
            SimSoundEvent::EntityDeployed {
                deploy_sound_id: sound_id,
                rx,
                ry,
            }
        } else {
            SimSoundEvent::EntityUndeployed {
                undeploy_sound_id: sound_id,
                rx,
                ry,
            }
        });
        Ok(())
    }

    /// Infantry vtable+54C = Stop callback521B40. Clear6E4 BEFORE calling
    /// unforced Do_Action27: a zero-health action may re-enter Stop_Driver.
    pub(crate) fn infantry_pending_deploy_stop_callback(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
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
        let rules = rules.ok_or("pending deployment Stop callback requires rules")?;
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
            //70E120 uses the SprayAttack slot70DD70, not GetCurrentWeapon
            //70E1A0. The existing GetWeapon owner resolves the live tier.
            let actor = self.substrate.entities.get(id).expect("same Unload actor");
            let slot = if object.spray_attack { 0 } else { 1 };
            let area_fire = combat_weapon::weapon_for_index(object, actor.veterancy.rank(), slot)
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
                        Some(rules),
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
        self.mission_assign_exact(
            id,
            MissionId::from_known(MissionType::Guard),
            self.session.binary_frame,
        )
        .map_err(|cause| format!("Infantry Unload Guard: {cause}"))?;
        self.assign_destination_represented(id, None, Some(rules), None, effects)
            .map_err(|cause| format!("Infantry Unload destination: {cause}"))?;
        Ok(if delay > -1 { delay } else { 450 })
    }
}
