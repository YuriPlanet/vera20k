//! The House ally graph and its live transitions.
//!
//! `Simulation::house_alliances` holds each house's ally mask
//! (`HouseClass+0x5788`) as a directed graph, `A -> {B}` when A's mask has
//! B's bit; `HouseClass::Is_Ally` reads only the asking house's mask. This
//! module writes it: the scenario's install, then MakeAlly and MakeEnemy,
//! which Computer_Paranoid drives for the computer houses. It is the only
//! graph: fog, fire and scan readers borrow it, so a later reader in the
//! same frame sees each write as native's single mask does.
//!
//! Evidence: instruction reading of `HouseClass::Can_Ally @ 0x00501540`,
//! `HouseClass::Make_Ally @ 0x004F9B70`, `HouseClass::Make_Enemy @
//! 0x004F9F90`, `HouseClass::Computer_Paranoid @ 0x00501640` and its skirmish
//! callers: a computer MCV's deploy (`0x007398D9` -> `0x00505180`, Paranoid at
//! `0x005051AF`) and a computer house's defeat (`0x004FC10D`). Its other
//! callers are a network player's exit (`0x0064CA0D`, `0x0050A7B4`) and
//! base initialization (`0x0050C2F0`, reached by TAction30).
//!
//! MakeAlly and MakeEnemy bracket their work with the map's release and
//! re-admission callbacks (`0x004ADEE0` at `0x004F9B9A`/`0x004F9FD8`,
//! `0x004ADCD0` at `0x004F9F4F`/`0x004FA1E3`), which walk every Techno for the
//! player whose map they write. VERA keeps a map per House, and ports the arm
//! whose outcome the ally bit decides: an ally's Buildings, whose sight
//! leaves the map of the House an edge stops counting as an ally and opens on
//! the map of one it starts counting (`Simulation::release_allied_building_sight`,
//! `Simulation::admit_allied_building_sight`). An ally's other objects take no
//! sight callback there: their sight reaches a new ally's map at their next
//! reveal event.
//!
//! RESIDUALS
//! - Natively, a Building whose latch (`+0x250`) is still set from a reveal
//!   made before the alliance opens on the new ally's map only at the next
//!   bracket; VERA opens it at this one
//!   (`Simulation::admit_allied_building_sight`).
//! - The rest of each bracket is unported. Its Human arm re-reveals each of
//!   the player's own discovered objects (`+0x41B`) where it stands, and the
//!   allied-Building arm re-reveals the Buildings of allies whose bit did not
//!   change. Trigger: every MakeAlly and MakeEnemy. Effect: native snaps a
//!   mover's footprint to its current cell a few frames before its next
//!   arrival would, resets a Spy Plane's `Damage=` footprint to its `Sight=`,
//!   and opens the sight of an own object that has none yet (one promoted
//!   into the playfield since its last arrival). Frequency: launch alliances
//!   and Computer_Paranoid. Risk: one-cell footprint differences until the
//!   next reveal event.
//! - Native's ReReveal on an ex-ally's mobile leaves this map's counters
//!   alone (`MapClass::RevealArea`'s house gate rejects it), so the last
//!   footprint it revealed stays counted there; VERA's per-map caches take it
//!   off at that mobile's next release. Trigger: MakeEnemy between Houses
//!   with standing allied units. Effect: native's ex-ally keeps seeing those
//!   cells. Frequency: rare (Paranoid, triggers).
//! - The bracket's `RevealToAll=` branch re-admission has no release half, so
//!   on its map each bracket counts a standing `RevealToAll=` building's
//!   sight once more. Computer_Paranoid never changes an alliance of the
//!   current (human) house. Trigger: a computer MCV deploy, scripted base
//!   initialization, or a computer house's defeat under `[AI] Paranoid`,
//!   while a `RevealToAll=` building stands. Effect: after the building
//!   dies, native keeps its cells counted, so a hostile Gap Generator cannot
//!   shroud them; VERA's can. Frequency: late game, rare.
//! - The brackets' gap callbacks (`+0x418`, `+0x414`) stay with the Gap
//!   Generator owner's per-frame materialization, which reads the ally graph.
//! - The human-only alliance messages and EVA lines (`0x004F9D4E..
//!   0x004F9F35`, `0x004FA0E4..0x004FA1D5`) and the radar redraw
//!   (`0x004F42F0`) are not represented. No VERA producer makes a human
//!   house ally or break an alliance: the ALLY event (`0x004C6E08`) and the
//!   diplomacy dialog (#1274) are not ported.
//! - IsParanoid (`+0x24A`, `0x005016D5`) has no reader and is not stored.

use crate::map::houses::{HouseAllianceMap, HouseRoster, normalize_house_name};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::is_ally_by_object;
use crate::sim::house_strategy::update_anger_nodes;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

/// The native ScenarioInit counter is nonzero during ReadScenarioINI's House
/// pass (ordinary campaign prior2). CanAlly501540 then bypasses live-world
/// defeated/count gates; MakeAlly4F9B70 writes only the sender's ally mask.
pub(crate) enum HouseAllianceAdmission<'a> {
    ScenarioInitialization { roster: &'a HouseRoster },
    Admitted(HouseAllianceMap),
}

impl Simulation {
    /// Existing launch diplomacy graph owner: commit its admitted graph, then
    /// rebuild only recipients whose ally bits changed, in House array order.
    /// This retains the threat dependency of4F9BB9/4FA062/4FA0D5. The fresh
    /// ScenarioInit arm precedes every Techno, so live target/anger/paranoid
    /// sight effects cannot run. Live CanAlly/EVA remain outside that bound.
    pub(crate) fn install_house_alliances(
        &mut self,
        admission: HouseAllianceAdmission<'_>,
        rules: &RuleSet,
    ) {
        let alliances = match admission {
            HouseAllianceAdmission::Admitted(alliances) => alliances,
            HouseAllianceAdmission::ScenarioInitialization { roster } => {
                assert!(
                    self.entities().is_empty() && self.terrain_objects.objects().is_empty(),
                    "ScenarioInit diplomacy precedes map objects"
                );
                assert!(
                    !self.session.game_mode_nonzero(),
                    "this ScenarioInit admission is the campaign map-read family"
                );
                assert_eq!(
                    roster.houses.len(),
                    self.houses.order().len(),
                    "House ReadScenarioINI follows complete House construction"
                );
                roster.alliance_map()
            }
        };
        let changed: Vec<_> = self
            .houses
            .order()
            .iter()
            .copied()
            .filter(|owner| {
                let key = normalize_house_name(self.interner.resolve(*owner));
                self.house_alliances
                    .get(&key)
                    .into_iter()
                    .flatten()
                    .ne(alliances.get(&key).into_iter().flatten())
            })
            .collect();
        // Each MakeAlly or MakeEnemy that native's install makes brackets its
        // bit write: the allied Buildings of each edge it clears leave the
        // ally's map first, and those of each edge it sets open there after.
        let (before, after) = (
            self.ally_edges(&self.house_alliances),
            self.ally_edges(&alliances),
        );
        let context = crate::sim::world::UninitContext::with_rules(rules);
        for &(owner, viewer) in before.difference(&after) {
            self.release_allied_building_sight(owner, viewer, context);
        }
        self.house_alliances = alliances;
        self.reveal_viewers.forget();
        for owner in changed {
            self.rebuild_house_spatial_threat(owner, rules);
        }
        for &(owner, viewer) in after.difference(&before) {
            self.admit_allied_building_sight(owner, viewer, context);
        }
    }

    /// Every ally edge of `alliances` between two distinct houses of the
    /// House array: `(owner, ally)` where `owner`'s IsAlliedWith names `ally`.
    fn ally_edges(
        &self,
        alliances: &HouseAllianceMap,
    ) -> std::collections::BTreeSet<(InternedId, InternedId)> {
        let houses = &self.houses.order();
        houses
            .iter()
            .flat_map(|&owner| houses.iter().map(move |&ally| (owner, ally)))
            .filter(|&(owner, ally)| {
                owner != ally
                    && crate::map::houses::is_allied_with(
                        alliances,
                        self.interner.resolve(owner),
                        self.interner.resolve(ally),
                    )
            })
            .collect()
    }

    /// `HouseClass::Computer_Paranoid @ 0x00501640`: each live computer house
    /// turns on every live human house and allies with every other live one.
    pub(crate) fn computer_paranoid(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        // 0x00501640..0x00501662: the mode's vt+4, else Rules AlliesAllowed.
        // Battle and FreeForAll, the skirmish-listed modes, share vt+4
        // 0x005C0E30, which answers false.
        if !rules.general.allies_allowed {
            return;
        }
        // 0x00501668..0x00501675.
        if self.session.computer_slot_has_team() {
            return;
        }
        let game_mode_nonzero = self.session.game_mode_nonzero();
        // Both walks reload the House count after each call (0x005016DB,
        // 0x00501736).
        let mut index = 0;
        while index < self.houses.order().len() {
            let owner = self.houses.order()[index];
            index += 1;
            if !self.houses.get(&owner).is_some_and(|house| {
                !house.is_defeated() && !house.is_controlled_by_human(game_mode_nonzero)
            }) {
                continue;
            }
            let mut peer_index = 0;
            while peer_index < self.houses.order().len() {
                let other = self.houses.order()[peer_index];
                peer_index += 1;
                let Some(peer) = self.houses.get(&other).filter(|peer| !peer.is_defeated()) else {
                    continue;
                };
                // Itself too: CanAlly refuses it.
                if peer.is_controlled_by_human(game_mode_nonzero) {
                    self.make_enemy(owner, other, rules);
                } else {
                    self.make_ally(owner, other, rules, registry, effects);
                }
            }
        }
    }

    /// `HouseClass::Make_Ally @ 0x004F9B70` for its live and initialization
    /// callers, which pass no announcement.
    pub(crate) fn make_ally(
        &mut self,
        owner: InternedId,
        other: InternedId,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        // Keep the live and initialization arms in this same graph owner.
        if !self.can_ally(owner, other, rules) {
            return;
        }
        // 0x004F9B9F..0x004F9BB3, then the threat rebuild (0x004F9BB9).
        self.set_ally_bit(owner, other, true);
        self.rebuild_house_spatial_threat(owner, rules);
        // 0x004F9BBE..0x004F9BEA: the ally's anger is cancelled, which
        // rescans the enemy; 0x004F9BEF..0x004F9BFC: the ally is no enemy.
        let anger = self
            .houses
            .get(&owner)
            .and_then(|house| house.grudge_scores().get(&other).copied())
            .unwrap_or(0);
        update_anger_nodes(
            &mut self.houses,
            &self.house_alliances,
            &self.interner,
            owner,
            other,
            anger.wrapping_neg(),
        );
        if let Some(house) = self.houses.get_mut(&owner)
            && house.enemy_house() == Some(other)
        {
            house.forget_enemy();
        }
        // 0x004F9B9A's release bracket ran while `owner` did not count
        // `other` an ally (CanAlly refuses an existing ally), so it took no
        // allied Building's sight off `other`'s map.
        'live: {
            if self.object_placement_scope_active() {
                // 0x004F9C06..0x004F9C31: ordinary initialization skips the
                // live Paranoid, target and announcement tail. Native also
                // keeps an initial Allies mask at +0x1D8 for WriteScenarioINI
                // 0x0050131C; map-INI export and its GameMode 4 exception are
                // outside this boolean-mode model.
                break 'live;
            }
            // 0x004F9C37..0x004F9C7A: a human house allying with a house type
            // that is not MultiplayPassive turns the computers paranoid.
            let game_mode_nonzero = self.session.game_mode_nonzero();
            if game_mode_nonzero
                && rules.general.paranoid
                && self
                    .houses
                    .get(&owner)
                    .is_some_and(|house| house.is_controlled_by_human(game_mode_nonzero))
                && self
                    .houses
                    .get(&other)
                    .is_some_and(|house| !house.multiplay_passive())
            {
                self.computer_paranoid(rules, registry, effects);
            }
            // 0x004F9C7F..0x004F9D48: in Logic order, each of the house's
            // Technos (As_Techno 0x0040DD70) out of limbo (+0x81) whose
            // target is a Techno of a house it now counts as friendly takes
            // Assign_Target(NULL) (vt+0x3C8).
            for id in self.logic_order().to_vec() {
                let Some(entity) = self.substrate.entities.get(id) else {
                    continue;
                };
                if entity.owner() != owner || !entity.is_ai_alive() || entity.lifecycle.in_limbo() {
                    continue;
                }
                let Some(TargetKind::Entity(target)) = entity.attack_target.target() else {
                    continue;
                };
                if let Some(target_owner) = self.substrate.entities.get(target).map(|t| t.owner())
                    && is_ally_by_object(
                        Some(&self.house_alliances),
                        &self.interner,
                        owner,
                        target_owner,
                    )
                {
                    let _ =
                        self.assign_target_represented(id, None, Some(rules), registry, effects);
                }
            }
        }
        // 0x004F9F46..0x004F9F4F: both arms end in the re-admission bracket.
        self.admit_allied_building_sight(
            owner,
            other,
            crate::sim::world::UninitContext::new(Some(rules), registry).with_effects(effects),
        );
    }

    /// `HouseClass::Make_Enemy @ 0x004F9F90` for its live callers, which
    /// pass no announcement.
    pub(crate) fn make_enemy(&mut self, owner: InternedId, other: InternedId, rules: &RuleSet) {
        // The current graph/anger writes also run during initialization.
        // Initial saved-mask stores 0x004FA03A/0x004FA0B0 share the map-INI export
        // boundary documented by MakeAlly; they are not a second live graph.
        let (Some(house), Some(other_house)) = (self.houses.get(&owner), self.houses.get(&other))
        else {
            return;
        };
        // 0x004F9FA9..0x004F9FC9.
        if self.session.game_mode_nonzero()
            && (other_house.multiplay_passive() || house.multiplay_passive())
        {
            return;
        }
        // 0x004F9FD8: the release bracket precedes the anger and the bit
        // writes. Each alliance this call breaks (below) takes its Buildings'
        // sight off the ex-ally's map; the re-admission bracket at the end
        // (0x004FA1E3) has nothing to open on a map whose ally bit cleared.
        if is_ally_by_object(Some(&self.house_alliances), &self.interner, owner, other) {
            let context = crate::sim::world::UninitContext::with_rules(rules);
            self.release_allied_building_sight(owner, other, context);
            if is_ally_by_object(Some(&self.house_alliances), &self.interner, other, owner) {
                self.release_allied_building_sight(other, owner, context);
            }
        }
        // 0x004F9FDD..0x004F9FE2.
        update_anger_nodes(
            &mut self.houses,
            &self.house_alliances,
            &self.interner,
            owner,
            other,
            1,
        );
        // 0x004F9FE7..0x004FA01A: only an alliance is broken.
        if !is_ally_by_object(Some(&self.house_alliances), &self.interner, owner, other) {
            return;
        }
        self.set_ally_bit(owner, other, false);
        self.rebuild_house_spatial_threat(owner, rules);
        // 0x004FA067..0x004FA0DF: the other house breaks its side too, then
        // rescans its enemy: native passes it as both house and peer, and a
        // house holds no anger node of its own.
        if is_ally_by_object(Some(&self.house_alliances), &self.interner, other, owner) {
            self.set_ally_bit(other, owner, false);
            self.rebuild_house_spatial_threat(other, rules);
            update_anger_nodes(
                &mut self.houses,
                &self.house_alliances,
                &self.interner,
                other,
                other,
                1,
            );
        }
    }

    /// `HouseClass::Can_Ally @ 0x00501540`, including its initialization gate.
    fn can_ally(&self, owner: InternedId, other: InternedId, rules: &RuleSet) -> bool {
        // 0x00501548..0x00501579: itself, or an existing ally.
        if is_ally_by_object(Some(&self.house_alliances), &self.interner, owner, other) {
            return false;
        }
        let (Some(house), Some(_)) = (self.houses.get(&owner), self.houses.get(&other)) else {
            return false;
        };
        // 0x0050157C..0x0050159E: a house type of the `Civilian` side, in a
        // nonzero game mode.
        if self.session.game_mode_nonzero() && self.house_type_is_civilian(other, rules) {
            return false;
        }
        if self.object_placement_scope_active() {
            // 0x005015A1..0x005015AE precedes the defeated/live-house-count gates.
            return true;
        }
        // 0x005015B1..0x005015BF.
        if house.is_defeated() {
            return false;
        }
        // 0x005015C2..0x00501638: among the live houses whose type is not
        // MultiplayPassive, an alliance that leaves no enemy is refused. The
        // friends among them are the house itself and its allies; the new
        // ally counts whatever its type (0x0050162F).
        let mut live = 0;
        let mut friends = 1;
        for &peer in self.houses.order() {
            if !self
                .houses
                .get(&peer)
                .is_some_and(|peer| !peer.is_defeated() && !peer.multiplay_passive())
            {
                continue;
            }
            live += 1;
            if is_ally_by_object(Some(&self.house_alliances), &self.interner, owner, peer) {
                friends += 1;
            }
        }
        live != friends
    }

    /// Write `owner`'s ally bit for `other`.
    fn set_ally_bit(&mut self, owner: InternedId, other: InternedId, allied: bool) {
        let other_key = normalize_house_name(self.interner.resolve(other));
        let allies = self
            .house_alliances
            .entry(normalize_house_name(self.interner.resolve(owner)))
            .or_default();
        if allied {
            allies.insert(other_key);
        } else {
            allies.remove(&other_key);
        }
        self.reveal_viewers.forget();
    }
}

#[cfg(test)]
#[path = "house_diplomacy_tests.rs"]
mod tests;
