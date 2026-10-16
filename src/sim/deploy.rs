//! Infantry deployment actions and the shared Unit DEPLOY event receiver.
//!
//! Infantry's Doing and shared Techno Stage own its sequence. The completion
//! receiver applies the independent crush byte and passive-scan timer effects,
//! including when its requested next action refuses. Unit simple deployment
//! uses its Mission leaf, the same Techno Stage and attached AnimClass.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{TargetKind, combat_weapon};
use crate::sim::mission::{MissionId, MissionType};
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

/// Event9 DEPLOY4C778A..4C7812. MCV conversion and simple deployment
/// share this event; each concrete Mission_Unload branch owns its effects.
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
                || crate::sim::unit_simple_deploy::is_simple_deployer(sim, e, rules))
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
    // holds a WeaponsFactory= building (Cell_Building `0x0047C520`; VERA's
    // foundation scan stands in), so an MCV still leaving its factory stays.
    let (rx, ry) = {
        let entity = sim.substrate.entities.get(id).unwrap();
        (entity.position.rx(), entity.position.ry())
    };
    if crate::sim::credit_income::building_at_cell(sim, rx, ry)
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
