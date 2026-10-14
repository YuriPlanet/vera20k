//! Action4/7 Team creation, with the existing VM, Techno constructor, class
//! Unlimbo, admission and destructor owners. Original YR1.001 SHA256
//! 1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c:
//! CreateTeam6F09C0, SpawnReinforcements65D8E0, CreateMembers65DD30 and
//! PlaceMembers65E010. Executable controls: tools/spatial_oracle/team_creation.
//!
//! Normal TaskForce and Full hover-transport chains share construction,
//! membership, cargo, placement and scalar cleanup owners. Droppod uses the
//! existing paradrop carrier/flight/departure owners; Full storage fill remains
//! a separate mechanism and refuses explicitly. Team
//! retention of an off-map Dummy Cell identity remains a TeamTarget residual.

use super::{TeamScriptAction, TeamTarget, TeamTypeHouse};
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
use crate::sim::world::{FrameEffects, PlacementEvidence, Simulation, UninitContext};

impl Simulation {
    /// GetHouse6F2070: fixed Country attachment, otherwise the native
    /// Scenario1180 start-slot mapping used by Player@A..H510ED0. Country
    /// resolution uses House array order502D30, never sorted owner names.
    fn team_type_house(&self, team_type: InternedId, rules: &RuleSet) -> Option<InternedId> {
        match self.team_script_vm.team_type_ini.get(&team_type)?.house? {
            TeamTypeHouse::Country(country) => self.house_by_country_index(country, rules),
            TeamTypeHouse::PlayerAt(slot) => {
                self.house_by_trigger_selector(4507 + i32::from(slot), rules)
            }
        }
    }

    fn team_initial_zone(&self, team_type: InternedId) -> Option<TeamTarget> {
        let waypoint = self.team_script_vm.team_type_ini.get(&team_type)?.waypoint;
        self.session
            .waypoint_cell(waypoint)
            .filter(|&cell| cell != (0, 0))
            .map(|cell| self.canonical_team_cell(cell))
    }

    /// Original CreateTeam6F09C0 owner. The VM performs its shared Max gate
    /// and sole6E8A90 constructor; the Simulation resolves House and the
    /// Scenario-owned initial waypoint. Trigger4 calls with no explicit House,
    /// while House AI supplies itself, as4F8AAD does.
    pub(crate) fn create_team(
        &mut self,
        team_type: InternedId,
        explicit_owner: Option<InternedId>,
        rules: &RuleSet,
    ) -> Option<u64> {
        let owner = explicit_owner.or_else(|| self.team_type_house(team_type, rules))?;
        let zone = self.team_initial_zone(team_type);
        let placement_scope_active = self.object_placement_scope_active();
        let id = self.team_script_vm.construct_team(
            team_type,
            owner,
            self.session.game_mode_nonzero,
            self.session.binary_frame as i32,
            placement_scope_active,
            zone,
        )?;
        // The trigger definition owner retains the actual6F1341-resolved
        // TagType ordinal; never re-find it through a copied raw name.
        self.attach_team_trigger_tag(id, team_type);
        Some(id)
    }

    /// Trigger4 original6DEB6B brackets CreateTeam with A8E7AC. Recruiting
    /// remains in TeamAI's normal same-frame visit, using real class arrays.
    pub(crate) fn trigger_create_team(
        &mut self,
        team_type: InternedId,
        rules: &RuleSet,
        _registry: Option<&OverlayTypeRegistry>,
        _effects: FrameEffects<'_>,
    ) -> Option<u64> {
        self.with_object_placement_scope(|sim| sim.create_team(team_type, None, rules))
    }

    /// Normal SpawnReinforcements65D8E0. The action's native override is -1;
    /// a valid explicit override inside the playfield asks shared FNPC first.
    /// All members are constructed before any placement, preserving concrete
    /// constructor RNG→Create_ID order. Team/Script themselves spend no ID.
    pub(crate) fn spawn_team_reinforcements(
        &mut self,
        team_type: InternedId,
        waypoint_override: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> (bool, Option<(i16, i16)>) {
        let Some(definition) = self.team_script_vm.team_types.get(&team_type).copied() else {
            return (false, None);
        };
        let Some(task_force) = self
            .team_script_vm
            .task_forces
            .get(&definition.task_force_id)
        else {
            return (false, None);
        };
        if task_force.entries.is_empty() {
            return (false, None);
        }
        let metadata = self
            .team_script_vm
            .team_type_ini
            .get(&team_type)
            .cloned()
            .unwrap_or_default();
        //65D90F..65D970: an empty referenced program gains Guard(0), in
        //that same ScriptType owner. No duplicate fallback Script instance.
        let Some(script) = self.team_script_vm.scripts.get_mut(&definition.script_id) else {
            return (false, None);
        };
        if script.actions.is_empty() {
            script.actions.push(TeamScriptAction {
                action_id: 5,
                argument: 0,
            });
        }
        let Some(members) = self.create_reinforcement_members(team_type, rules, registry, effects)
        else {
            return (false, None);
        };
        let Some(owner) = self.team_type_house(team_type, rules) else {
            return (false, None);
        };
        let waypoint = if waypoint_override == -1 {
            metadata.waypoint
        } else {
            waypoint_override
        };
        let origin = self.session.waypoint_cell(waypoint).unwrap_or((0, 0));
        let nearby = waypoint_override != -1
            && crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (i32::from(origin.0), i32::from(origin.1)),
                self.playfield_bounds,
                self.resolved_terrain.as_ref(),
            );
        let placed = if metadata.droppod {
            self.place_paradrop_reinforcement_members(
                team_type, &members, origin, rules, registry, effects,
            )
        } else {
            self.place_reinforcement_members(
                &members, owner, origin, nearby, rules, registry, effects,
            )
        };
        //65DB6B..65DB97: only successful placement for an allied current
        //House records the REQUESTED origin in the existing review-cell ring.
        //The trigger dispatcher publishes this transient result in action order.
        let review_cell = self
            .session
            .current_house()
            .filter(|&current| {
                placed
                    && crate::map::houses::is_allied_with(
                        &self.house_alliances,
                        self.interner.resolve(owner),
                        self.interner.resolve(current),
                    )
            })
            .map(|_| origin);
        //65DB9C returns true regardless of PlaceMembers' result. Ordinary
        //refusals use scalar cleanup; a failed Droppod carrier retains the
        //already constructed Team/member chain, as65DBF0/65DD08 do.
        (true, review_cell)
    }

    /// SpawnReinforcements65DBA6..65DD08: Droppod is an ordinary PDPLANE
    /// carrying the already constructed Team's entire Foot chain. It does
    /// not replace their locomotors or manufacture another set of infantry.
    /// Constructor then edge pick then mission/target then scoped Unlimbo
    /// then whole cargo attach and direct Commence preserve the native order.
    /// Steam3e81a617..08600, original literals/slots/reader packet in the
    /// ALL01 handoff; aircraft/paradrop owners carry the subsequent lifecycle.
    fn place_paradrop_reinforcement_members(
        &mut self,
        team_type: InternedId,
        members: &[u64],
        target: (i16, i16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        use crate::rules::object_type::ObjectCategory;
        use crate::sim::components::DriveCoord;
        use crate::sim::world::edge_cell::Edge;

        let Some(&head) = members.first() else {
            return false;
        };
        let plane_type = crate::sim::superweapon::paradrop::PDPLANE;
        if rules
            .object_in_category(ObjectCategory::Aircraft, plane_type)
            .is_none()
        {
            // Native FindIndex(-1) would dereference before the Aircraft
            // array. A missing literal type is outside a valid retail rules
            // closure; retain explicit diagnostics instead of a fake plane.
            log::warn!("Droppod reinforcement requires native aircraft type {plane_type}");
            return false;
        }
        //65DBD0 reads the returned head's live owner, not a copied TeamType
        // Country or current player. The carrier itself never joins the Team.
        let Some(owner) = self
            .substrate
            .entities
            .get(head)
            .map(|entity| entity.owner())
        else {
            return false;
        };
        let owner_name = self.interner.resolve(owner).to_string();
        let plane = self.with_object_placement_scope(|sim| {
            sim.construct_object_limbo_at_height(
                plane_type,
                &owner_name,
                0,
                0,
                0,
                0,
                rules,
                effects,
            )
        });
        let Some(plane) = plane else {
            return false;
        };
        self.substrate
            .entities
            .get_mut(plane)
            .unwrap()
            .mark_mission_only();
        let metadata = self
            .team_script_vm
            .team_type_ini
            .get(&team_type)
            .cloned()
            .unwrap_or_default();
        let spawn_cell = if metadata.use_transport_origin {
            // GetTransportWaypoint6F18E0: -1 produces the shared zero cell;
            // other inputs read the Scenario waypoint owner68BCC0.
            self.session
                .waypoint_cell(metadata.transport_waypoint)
                .unwrap_or((0, 0))
        } else {
            //65DC11 rereads head+21C after the carrier constructor. The
            // existing House edge/GetEdge and Map4AA440 owners spend the
            // original stream's draws; no random synthetic starting offset.
            let Some(owner) = self
                .substrate
                .entities
                .get(head)
                .map(|entity| entity.owner())
            else {
                self.discard_constructed_limbo(plane, Some(rules), effects);
                return false;
            };
            let (authored, waypoint) = self.houses.get(&owner).map_or((-1, 0), |house| {
                (house.authored_edge(), house.waypoint_edge)
            });
            let Some(cell) =
                self.aircraft_edge_cell(Edge::authored_or_waypoint(authored, waypoint))
            else {
                self.discard_constructed_limbo(plane, Some(rules), effects);
                return false;
            };
            (cell.0 as i16, cell.1 as i16)
        };
        self.team_member_queue_mission(plane, MissionType::ParadropApproach, rules);
        self.team_member_set_destination(plane, None, rules, registry, effects);
        let canonical_target = self.canonical_team_cell(target);
        self.team_member_assign_target(plane, canonical_target, rules, registry, effects);
        //65DC9B..65DCC6 builds CoordStruct from the literal signed start
        //cell center with Z0 (not Cell's ground GetCoords), inside A8E7AC.
        let coord = DriveCoord {
            x: i32::from(spawn_cell.0).wrapping_shl(8).wrapping_add(128),
            y: i32::from(spawn_cell.1).wrapping_shl(8).wrapping_add(128),
            z: 0,
        };
        let placed = self.with_object_placement_scope(|sim| {
            sim.reveal_constructed_object_at_coord_with_overlay_context(
                plane,
                coord,
                0,
                PlacementEvidence::MarkSucceeded,
                rules,
                registry,
                effects,
            )
            .is_some()
        });
        if !placed {
            //65DD08 deletes only the carrier. The already constructed Team
            //members remain held; no additional UnInit/kill is invented.
            self.discard_constructed_limbo(plane, Some(rules), effects);
            return false;
        }
        let cargo_members: Vec<_> = members
            .iter()
            .map(|&member| {
                let entity = self
                    .substrate
                    .entities
                    .get(member)
                    .expect("constructed Team member");
                let size = self.object_type(entity.type_ref(), rules).unwrap().size;
                (member, size)
            })
            .collect();
        // Every Foot carries CargoClass even when Type.Passengers is0. The
        // shared Cargo owner supplies its empty capability when first used.
        // Native +6C9 payload latch is the existing paradrop residual: the
        // unselectable retail PDPLANE's missions26/27 never use its fire gate.
        let loaded = self.add_cargo_passenger_chain(
            plane,
            &cargo_members,
            UninitContext::new(Some(rules), registry).with_effects(effects),
        );
        assert!(
            loaded,
            "constructed paradrop carrier retains its CargoClass"
        );
        for &member in members {
            self.substrate
                .entities
                .get_mut(member)
                .unwrap()
                .passenger_role
                .set_inside_transport(plane, false);
        }
        //65DCF9 invokes Aircraft Commence41B870 directly, with no Ready
        //query, after the entire payload attaches. Use the sole mission owner.
        let _ = self.mission_commence_exact(plane, self.session.binary_frame);
        true
    }

    /// Original CreateMembers65DD30: concrete constructor/AddMember order,
    /// separate reverse-construction chains and the one Full cargo attach.
    /// The normal, Full and Droppod placement callers share this owner.
    pub(super) fn create_reinforcement_members(
        &mut self,
        team_type: InternedId,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> Option<Vec<u64>> {
        let definition = *self.team_script_vm.team_types.get(&team_type)?;
        let metadata = self
            .team_script_vm
            .team_type_ini
            .get(&team_type)
            .cloned()
            .unwrap_or_default();
        let entries = self
            .team_script_vm
            .task_forces
            .get(&definition.task_force_id)?
            .entries
            .clone();
        // Full65DE53..65DE80 fills StorageClass before veterancy and
        // AddMember. The selected ALL01 types must pass the production
        // scenario reader's Storage0 boundary; the mineral/bale migration
        // does not represent
        // this arbitrary four-slot float fill. Preserve an explicit boundary
        // instead of inventing a second storage owner for other Full teams.
        if metadata.full
            && entries.iter().any(|entry| {
                entry.count > 0
                    && rules
                        .object_in_category(
                            entry.member_type.category,
                            self.interner.resolve(entry.member_type.id),
                        )
                        .is_some_and(|object| object.storage > 0)
            })
        {
            log::warn!(
                "TeamType {} Full StorageClass fill is unported",
                self.interner.resolve(team_type)
            );
            return None;
        }
        let owner = self.team_type_house(team_type, rules)?;
        let zone = self.team_initial_zone(team_type);
        let team = self.team_script_vm.construct_team_instance(
            team_type,
            owner,
            self.session.binary_frame as i32,
            zone,
        )?;
        self.attach_team_trigger_tag(team, team_type);
        let state = self
            .team_script_vm
            .teams
            .get_mut(&team)
            .expect("constructed Team");
        state.forced_active = true;
        state.under_strength = false;
        //65DDAB/65DDD1: scan all TaskForce entries, regardless of amount,
        // using the existing aircraft-transport decision6EF470. When any
        // type is Naval, only naval transports enter the carrier chain.
        let air_transports = self.team_air_transports(team, rules);
        let any_naval = entries.iter().any(|entry| {
            rules
                .object_in_category(
                    entry.member_type.category,
                    self.interner.resolve(entry.member_type.id),
                )
                .is_some_and(|object| object.naval)
        });
        let has_unload_script = self
            .team_script_vm
            .scripts
            .get(&definition.script_id)
            .is_some_and(|script| script.actions.iter().any(|action| action.action_id == 8));
        let owner_name = self.interner.resolve(owner).to_string();
        let mut members = Vec::new();
        let mut transports = Vec::new();
        for entry in entries {
            let type_name = self.interner.resolve(entry.member_type.id).to_string();
            for _ in 0..entry.count {
                let member = self.with_object_placement_scope(|sim| {
                    sim.construct_object_limbo_at_height(
                        &type_name,
                        &owner_name,
                        0,
                        0,
                        0,
                        0,
                        rules,
                        effects,
                    )
                });
                let Some(member) = member else {
                    continue;
                };
                let entity = self
                    .substrate
                    .entities
                    .get_mut(member)
                    .expect("constructed member");
                match metadata.veteran_level {
                    0 => entity.veterancy.set_rookie(),
                    2 => entity.veterancy.set_veteran(),
                    3 => entity.veterancy.set_elite(),
                    _ => {}
                }
                self.with_object_placement_scope(|sim| {
                    sim.team_add_member(team, member, false, rules, registry, effects)
                });
                //65DEE4 explicitly initiates each new member, even when it
                //was not the first one AddMember prepended.
                if let Some(link) = self
                    .team_script_vm
                    .teams
                    .get_mut(&team)
                    .and_then(|team| team.members.iter_mut().find(|link| link.id == member))
                {
                    link.initiated = true;
                }
                let is_transport =
                    metadata.full
                        && self.substrate.entities.get(member).is_some_and(|entity| {
                            entity.category()
                                == if air_transports {
                                    EntityCategory::Aircraft
                                } else {
                                    EntityCategory::Unit
                                }
                                && self.object_type(entity.type_ref(), rules).is_some_and(
                                    |object| object.passengers > 0 && (object.naval || !any_naval),
                                )
                        });
                if is_transport {
                    transports.push(member);
                } else {
                    members.push(member);
                }
            }
        }
        if members.is_empty() && transports.is_empty() {
            self.destroy_team(team, rules, effects);
            return None;
        }
        //65DF63..65E004: both +30 temporary chains prepend on construction.
        // Only the transport-chain HEAD receives the entire member chain,
        // in one4733A0 call, without capacity/size or per-member radio gates.
        members.reverse();
        if let Some(&transport) = transports.last() {
            if !members.is_empty() {
                let cargo_members: Vec<_> = members
                    .iter()
                    .map(|&member| {
                        let entity = self.substrate.entities.get(member).unwrap();
                        let size = self.object_type(entity.type_ref(), rules).unwrap().size;
                        (member, size)
                    })
                    .collect();
                let loaded = self.add_cargo_passenger_chain(
                    transport,
                    &cargo_members,
                    UninitContext::new(Some(rules), registry).with_effects(effects),
                );
                assert!(
                    loaded,
                    "Full reinforcement cargo must remain constructor-complete"
                );
                // This inverse relation is the existing Cargo-owned Rust
                // membership view. Native4733A0 itself writes only its +30
                // chain/+114 count/head, not a rider's +11C or OpenTopped.
                for &member in &members {
                    self.substrate
                        .entities
                        .get_mut(member)
                        .expect("loaded Full member")
                        .passenger_role
                        .set_inside_transport(transport, false);
                }
                if air_transports && has_unload_script {
                    self.substrate
                        .entities
                        .get_mut(transport)
                        .unwrap()
                        .mark_mission_only();
                }
            }
            transports.reverse();
            members = transports;
        }
        Some(members)
    }

    /// Normal65E010 without Tunnel/Droppod. A successful member leaves the
    /// candidate unchanged for the next member; never invent formation
    /// offsets. Failure tests native eight directions and the inverted Size
    /// gate65E554: only OUTSIDE Size candidates reach class admission. Original
    /// execution on an already placed Unit pins all8 XY candidates and scalar
    /// cleanup; that control is not a fresh-reinforcement refusal claim.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn place_reinforcement_members(
        &mut self,
        members: &[u64],
        owner: InternedId,
        mut cell: (i16, i16),
        nearby: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let edge = self
            .houses
            .get(&owner)
            .map_or(0, |house| house.authored_edge());
        let edge = if edge == -1 { 0 } else { edge };
        if nearby {
            let speed = self
                .substrate
                .entities
                .get(members[0])
                .and_then(|member| self.object_type(member.type_ref(), rules))
                .expect("reinforcement has its resolved TechnoType")
                .speed_type;
            cell = self
                .find_plain_passable_cell(
                    (i32::from(cell.0), i32::from(cell.1)),
                    speed,
                    None,
                    MovementZone::Normal,
                    (1, 1),
                )
                .map_or((0, 0), |(x, y)| (x as i16, y as i16));
        }
        let mut placed_any = false;
        for &member in members {
            if cell == (0, 0) {
                self.discard_constructed_limbo(member, Some(rules), effects);
                continue;
            }
            let aircraft = self
                .substrate
                .entities
                .get(member)
                .is_some_and(|member| member.category() == EntityCategory::Aircraft);
            let heading = if nearby {
                0
            } else {
                edge.wrapping_mul(2).wrapping_shl(13) as u16
            };
            let heading = if aircraft {
                heading.wrapping_sub(0x8000)
            } else {
                heading
            };
            let facing = (((u32::from(heading) >> 7) + 1) >> 1) as u8;
            loop {
                let target = self.canonical_team_cell(cell);
                let coord = self
                    .team_target_coord(target)
                    .expect("native Cell center coordinate");
                //65E1C2..65E1CF increments A8E7AC for the complete attempt. Guard,
                //CanEnter and the final refusal's scalarF8 still observe it;
                //a retry decrements before restarting. Unit's destructor
                //uses this same byte to retain cargo on that refusal path.
                let (placed, next) = self.with_object_placement_scope(|sim| {
                    let placed = sim
                        .reveal_constructed_object_at_coord_with_overlay_context(
                            member,
                            coord,
                            facing,
                            PlacementEvidence::UnitEntryAdmitted,
                            rules,
                            registry,
                            effects,
                        )
                        .is_some();
                    if placed {
                        if !aircraft {
                            sim.team_member_queue_mission(member, MissionType::Guard, rules);
                            sim.team_member_commence(member, rules);
                        }
                        return (true, None);
                    }
                    let next = crate::util::direction_tables::CELL_DELTAS
                        .iter()
                        .enumerate()
                        .find_map(|(direction, &(dx, dy))| {
                            let candidate = (
                                cell.0.wrapping_add(dx as i16),
                                cell.1.wrapping_add(dy as i16),
                            );
                            if sim.map_cell_in_bounds(candidate) {
                                return None;
                            }
                            (sim.mover_can_enter(
                                member,
                                candidate,
                                InfantryEntryArgs {
                                    direction: direction as i32,
                                    height: -1,
                                    previous_cell: None,
                                },
                                EntryQueryMode::CheckLocomotor,
                                rules,
                                registry,
                            )
                            .expect("reinforcement uses its shared class admission receiver")
                                == 0)
                                .then_some(candidate)
                        });
                    if next.is_none() {
                        sim.discard_constructed_limbo(member, Some(rules), effects);
                    }
                    (false, next)
                });
                if placed {
                    placed_any = true;
                    break;
                }
                cell = next.unwrap_or((0, 0));
                if cell == (0, 0) {
                    break;
                }
            }
        }
        placed_any
    }
}
