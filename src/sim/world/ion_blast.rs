//! IonBlastClass53CB10/53CBE0/53D310: one ordered vector for Dominator's
//! forceless ripples and TAction94's damaging blast. Native vector AA0118,
//! itemsAA011C/countAA0128, records coord/frame/skip-forces (14h bytes).
//!
//! Execute94's table entry6DD913 and IonBlastAtWP6E3380 use the tail
//! waypoint, signed cell centre and Ground578080, WITHOUT adding the deck.
//! The constructor appends at frame0. UpdateAll visits the captured indices
//! last to first, after the object loop55B64B and before crate timers55B65A.
//! Frame0 constructs splash-last/IonBlast then IonBeam at Z+5, applies the
//! optional bridge-deck area then the ground area, then Flash48A620. Every
//! unflagged frame traverses x-major ground lists in the surrounding7x7 cells.
//!
//! The actions and force kernel draw no RNG/write no timer/detach nothing.
//! Anim.Start and AreaDamage retain their existing RNG/timer/lifecycle owners.
//! Numeric evidence: tools/ion_blast_forces.json; frame retirement and ripple
//! tables: tools/superweapon_oracle.json. Other flow rests on instructions.
//!
//! Native uses a process-global vector; this port does not establish its
//! complete save/load and cross-scenario clearing behavior. Rust keeps it
//! match-owned: pending damaging blasts are saved and hashed so match restore
//! preserves future gameplay; visual-only blasts are omitted. Native global
//! retention across world replacement remains an explicit coverage boundary.

use super::{FrameEffects, Simulation};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::{overlay_types::OverlayTypeRegistry, ruleset::RuleSet};
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::combat::{CombatLightRequest, RAD_NO_ATTACKER, detonation_anim, world_receiver};
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::ion_blast_ripple::{RippleFrames, spiral_point};
use crate::sim::movement::{ground_pose, locomotor::MovementLayer};
use crate::sim::projectile::ProjectileCoord;
use crate::sim::rocking::ion_blast;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

const LAST_FRAME: i32 = 79;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct IonBlast {
    coord: [i32; 3],
    frame: i32,
    skip_forces: bool,
    /// Ares `WarheadTypeExt::IonExt` (saved with the Ares globals,
    /// 100523D0): the warhead whose applyRipples (10051AE0) raised the blast.
    #[cfg(feature = "ares")]
    #[serde(default)]
    ares_warhead: Option<crate::sim::intern::InternedId>,
}

pub(super) fn save<S: serde::Serializer>(
    blasts: &[IonBlast],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    blasts
        .iter()
        .filter(|blast| !blast.skip_forces)
        .collect::<Vec<_>>()
        .serialize(serializer)
}
pub(super) fn load<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<IonBlast>, D::Error> {
    Vec::deserialize(deserializer)
}

impl Simulation {
    /// MindControlArea53B087..53B0D4 sets skip-forces after construction.
    pub(crate) fn add_dominator_ion_blast(&mut self, coord: [i32; 3]) {
        self.ion_blasts.push(IonBlast {
            coord,
            frame: 0,
            skip_forces: true,
            #[cfg(feature = "ares")]
            ares_warhead: None,
        });
    }

    /// Execute94 ignores the scalar and all House/actor context. Missing Rust
    /// waypoint/terrain input safely refuses the original unchecked access.
    pub(crate) fn trigger_ion_blast(&mut self, waypoint: i32) -> bool {
        let Some(coord) = self.trigger_waypoint_ground_coord(waypoint) else {
            return false;
        };
        self.ion_blasts.push(IonBlast {
            coord,
            frame: 0,
            skip_forces: false,
            #[cfg(feature = "ares")]
            ares_warhead: None,
        });
        true
    }

    /// Ares WarheadTypeExt::applyRipples (installed 10051AE0), first in
    /// BulletClass_Detonate (100536E0) at its effect coordinate: an
    /// `IonCannon` or positive `Ripple.Radius` warhead constructs a blast,
    /// with DisableIonBeam (+0x10, the flag that skips the beam and the
    /// ripple forces) set unless `IonCannon=yes`, and maps it to the warhead.
    #[cfg(feature = "ares")]
    pub(crate) fn ares_apply_ripples(
        &mut self,
        coord: [i32; 3],
        warhead: &crate::rules::warhead_type::WarheadType,
    ) {
        let Some(ion) = warhead
            .ares_combat()
            .map(|combat| combat.ion_cannon())
            .filter(|ion| ion.raises_blast())
        else {
            return;
        };
        let skip_forces = !ion.enabled();
        let ares_warhead = Some(self.interner.intern(&warhead.id));
        self.ion_blasts.push(IonBlast {
            coord,
            frame: 0,
            skip_forces,
            ares_warhead,
        });
    }

    /// The Lifetime below which a blast lives (`0x0053CBF5`: 79). Ares
    /// IonBlastClass_Update_Duration (hook 0x53CBF5, installed 100541B0)
    /// takes a mapped warhead's `Ripple.Radius + 1` when lower.
    fn ion_blast_lifetime(&self, blast: &IonBlast, rules: &RuleSet) -> i32 {
        #[cfg(feature = "ares")]
        if let Some(ion) = self.ares_ion_cannon(blast, rules) {
            return ion.blast_lifetime();
        }
        let _ = (blast, rules);
        LAST_FRAME
    }

    #[cfg(feature = "ares")]
    fn ares_ion_cannon<'r>(
        &self,
        blast: &IonBlast,
        rules: &'r RuleSet,
    ) -> Option<&'r crate::rules::ares_warhead::IonCannonRules> {
        let warhead = self.interner.warhead_type(blast.ares_warhead?, rules)?;
        Some(warhead.ares_combat()?.ion_cannon())
    }

    pub(super) fn hash_ion_blasts(&self, hasher: &mut impl Hasher) {
        let blasts: Vec<_> = self
            .ion_blasts
            .iter()
            .filter(|blast| !blast.skip_forces)
            .collect();
        if !blasts.is_empty() {
            b"ion-blast-pending-forces-v1".hash(hasher);
            blasts.hash(hasher);
        }
    }

    ///53D310 captures the initial last index; callbacks can append another
    ///blast, which is visible immediately but waits until the next update.
    pub(super) fn update_ion_blasts(
        &mut self,
        rules: &RuleSet,
        overlays: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let mut bridge_changed = false;
        for index in (0..self.ion_blasts.len()).rev() {
            let blast = self.ion_blasts[index];
            if blast.frame >= self.ion_blast_lifetime(&blast, rules) {
                self.ion_blasts.remove(index);
                continue;
            }
            if !blast.skip_forces {
                let mut rock = true;
                if blast.frame == 0 {
                    let (changed, rocks) = self.start_ion_blast(&blast, rules, overlays, effects);
                    bridge_changed |= changed;
                    rock = rocks;
                }
                if rock {
                    self.apply_ion_blast_forces(blast, rules);
                }
            }
            self.ion_blasts[index].frame += 1;
        }
        bridge_changed
    }

    /// Frame 0 of a blast (`0x0053CC63..0x0053CE0A`): the splash or blast
    /// anim and the beam, the area damage and the flash. Returns whether a
    /// bridge changed and whether this frame's ripple forces run. Ares
    /// IonBlastClass_Update_Beam (hook 0x53CC63, installed 10053F10) gives a
    /// mapped warhead's blast `IonCannon.Blast`/`.Beam`/`.Warhead`/`.Damage`
    /// over the Rules values (a null anim or warhead builds or damages
    /// nothing) and skips the frame's forces for `IonCannon.Rock=no`.
    fn start_ion_blast(
        &mut self,
        blast: &IonBlast,
        rules: &RuleSet,
        overlays: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> (bool, bool) {
        let coord = blast.coord;
        let general = &rules.general;
        let ion_cannon_warhead = rules.bridge_warheads.ion_cannon_name.as_str();
        #[cfg(feature = "ares")]
        let ares = self.ares_ion_cannon(blast, rules);
        #[cfg(feature = "ares")]
        let (blast_anim, beam_anim, warhead_name, damage, rock) = match ares {
            Some(ion) => (
                ion.blast(&general.ion_blast_anim),
                ion.beam(&general.ion_beam_anim),
                ion.warhead(ion_cannon_warhead),
                ion.damage(general.ion_cannon_damage),
                ion.rock(),
            ),
            None => (
                Some(general.ion_blast_anim.as_str()),
                Some(general.ion_beam_anim.as_str()),
                Some(ion_cannon_warhead),
                general.ion_cannon_damage,
                true,
            ),
        };
        #[cfg(not(feature = "ares"))]
        let (blast_anim, beam_anim, warhead_name, damage, rock) = (
            Some(general.ion_blast_anim.as_str()),
            Some(general.ion_beam_anim.as_str()),
            Some(ion_cannon_warhead),
            general.ion_cannon_damage,
            true,
        );
        let impact = ProjectileCoord::new(coord[0], coord[1], coord[2]);
        //53CC63..53CD14 selects raw cell land, not overlay/nav passability.
        let anim = if detonation_anim::land_at(self, impact) == 2 {
            Some(
                rules
                    .combat_damage
                    .splash_list
                    .last()
                    .map(String::as_str)
                    .unwrap_or(""),
            )
        } else {
            blast_anim
        };
        for name in [anim, beam_anim].into_iter().flatten() {
            let type_id = self.interner.intern(name);
            let point = AnimWorldCoord {
                x: coord[0],
                y: coord[1],
                z: coord[2].wrapping_add(5),
            };
            let (rx, ry, sx, sy, z) = point.to_cell_sub_z();
            let descriptor = AnimClassSpawnDescriptor {
                draw_flags: 0x600,
                ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sx, sy, z)
            };
            // Missing asset/type behavior belongs to the existing constructor.
            let _ = self.spawn_anim_at_world(rules, descriptor, point);
        }
        //53CD67 re-queries after both Anim.Start callbacks. Only raw100
        //adds the separate AA013C deck (416), not the broader flags500 gate.
        let flags = if let Some(terrain) = self.resolved_terrain.as_ref() {
            let query = NativeCellQuery::canonical(terrain);
            query.flags(query.lookup_world(coord[0], coord[1]))
        } else {
            let dummy = self.effective_shared_cell_dummy();
            dummy.stamp_coord(
                (coord[0] / 256) as i16 as i32,
                (coord[1] / 256) as i16 as i32,
            );
            dummy.raw_flags()
        };
        let Some(warhead) = warhead_name.and_then(|name| rules.warhead(name)) else {
            return (false, rock);
        };
        let wh = self.interner.intern(&warhead.id);
        let mut bridge_changed = false;
        if flags & 0x100 != 0 {
            bridge_changed |= world_receiver::apply_area_damage(
                self,
                rules,
                overlays,
                ProjectileCoord::new(
                    coord[0],
                    coord[1],
                    coord[2].wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS),
                ),
                damage,
                warhead,
                true,
                (RAD_NO_ATTACKER, None, wh),
                effects,
            )
            .bridge_state_changed;
        }
        bridge_changed |= world_receiver::apply_area_damage(
            self,
            rules,
            overlays,
            impact,
            damage,
            warhead,
            true,
            (RAD_NO_ATTACKER, None, wh),
            effects,
        )
        .bridge_state_changed;
        self.combat_light_requests.emit(CombatLightRequest::Impact {
            target_id: None,
            damage,
            warhead_ref: wh,
            coord: impact,
            force_create: false,
            flags: 0,
        });
        (bridge_changed, rock)
    }

    fn apply_ion_blast_forces(&mut self, blast: IonBlast, rules: &RuleSet) {
        let center = (
            crate::util::lepton::lepton_to_cell_packed(blast.coord[0]),
            crate::util::lepton::lepton_to_cell_packed(blast.coord[1]),
        );
        for dx in -3..=3i16 {
            for dy in -3..=3i16 {
                let requested = (center.0.wrapping_add(dx), center.1.wrapping_add(dy));
                let cell = if let Some(terrain) = self.resolved_terrain.as_ref() {
                    let query = NativeCellQuery::canonical(terrain);
                    let identity = query.lookup(requested);
                    if identity == NativeCellIdentity::Dummy {
                        continue;
                    }
                    query.coord(identity)
                } else {
                    self.effective_shared_cell_dummy()
                        .stamp_coord(i32::from(requested.0), i32::from(requested.1));
                    continue;
                };
                let cell = (cell.0 as u16, cell.1 as u16);
                let mut current = self
                    .substrate
                    .occupancy
                    .get(cell.0, cell.1)
                    .and_then(|list| list.first_on_layer(MovementLayer::Ground));
                while let Some(id) = current {
                    //53CE85/53CE91 tests WhatAmI: Infantry523340=15,
                    //Unit746E20=1; Aircraft41C180=2 is excluded.
                    if let Some(entity) = self.substrate.entities.get(id)
                        && matches!(
                            entity.category(),
                            crate::map::entities::EntityCategory::Unit
                                | crate::map::entities::EntityCategory::Infantry
                        )
                    {
                        let p =
                            ground_pose::object_location(entity, self.resolved_terrain.as_ref());
                        let position = [p.x, p.y, p.z];
                        let radius = ion_blast::projected_radius(blast.coord, position);
                        if radius < 256 {
                            let sample = RippleFrames::shared().force_sample(blast.frame, radius);
                            let facing = entity.body_facing_current(self.session.binary_frame());
                            let loaded = self
                                .interner
                                .type_handle(entity.type_ref(), rules)
                                .is_some_and(|handle| rules.object_has_loaded_voxel(handle));
                            let entity = self.substrate.entities.get_mut(id).unwrap();
                            //53CF56..53CF7F occurs BEFORE the loaded-VXL gate.
                            if sample > 0 {
                                entity
                                    .foot_speed
                                    .set_speed_fraction(crate::util::fixed_math::SIM_ZERO);
                                entity.lift_by_ion_blast(
                                    (spiral_point(i32::from(sample)).1 as i8).wrapping_mul(2),
                                );
                            }
                            if sample >= 0 && loaded {
                                ion_blast::apply(
                                    &mut entity.rocking,
                                    blast.coord,
                                    position,
                                    facing,
                                    blast.frame,
                                );
                            }
                        }
                    }
                    current = self
                        .substrate
                        .occupancy
                        .get(cell.0, cell.1)
                        .and_then(|list| list.next_on_layer(MovementLayer::Ground, id));
                }
            }
        }
        // RESIDUAL: the shared Dummy has no native E4 object-list storage;
        // like other area walkers, a nonempty Dummy list is outside coverage.
    }

    /// Vector order; DrawAll visits last to first.
    pub fn ion_blasts(&self) -> impl DoubleEndedIterator<Item = ([i32; 3], i32)> + '_ {
        self.ion_blasts
            .iter()
            .map(|blast| (blast.coord, blast.frame))
    }
}

#[cfg(test)]
#[path = "ion_blast_tests.rs"]
mod tests;
