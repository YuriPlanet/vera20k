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
//! The ALLY event (`EventClass::Execute 0x004C6DEE`) is the only announced
//! caller; its human line and the local EVA voice leave as
//! `SimSoundEvent::AllianceChanged` (`announce_alliance`).
//!
//! Evidence: instruction reading of `HouseClass::Can_Ally @ 0x00501540`,
//! `HouseClass::Make_Ally @ 0x004F9B70`, `HouseClass::Make_Enemy @
//! 0x004F9F90`, `HouseClass::Computer_Paranoid @ 0x00501640` and its skirmish
//! callers: a computer MCV's deploy (`0x007398D9` -> `0x00505180`, Paranoid at
//! `0x005051AF`) and a computer house's defeat (`0x004FC10D`). Its other
//! callers are a network player's exit (`0x0064CA0D`, `0x0050A7B4`) and
//! base initialization (`0x0050C2F0`, reached by TAction30).
//!
//! MakeAlly and MakeEnemy bracket their work with Map4ADEE0/4ADCD0.
//! The shared sight visitor walks the Techno registry before and after the
//! graph change, including a no-edge MakeEnemy: discovered Human objects,
//! directed allied Buildings (also unchanged edges), RevealToAll admission
//! and the separate gap callbacks. Source counters, caches and lifecycle
//! remain with the existing sight/gap owners. Synchronous discovery carries
//! the caller's rules, overlay registry and frame effects.
//!
//! RESIDUALS
//! - A newly allied Building still opens one bracket earlier than native's
//!   shared250 latch permits (#2415); see shroud_refresh::walk_map_sight.
//! - Only the session's current House has the native Human41B history.
//!   Additional multiplayer clients' independent discovery is unrepresented.
//! - Native's ReReveal on an ex-ally's mobile leaves this map's counters
//!   alone (`MapClass::RevealArea`'s house gate rejects it), so the last
//!   footprint it revealed stays counted there; VERA's per-map caches take it
//!   off at that mobile's next release. Trigger: MakeEnemy between Houses
//!   with standing allied units. Effect: native's ex-ally keeps seeing those
//!   cells. Frequency: rare (Paranoid, triggers).
//! - Whole-map transitions retain their separately bounded callback
//!   projection. Full FogOfWar object hiding/snapshots remain unimplemented
//!   (#2454); a corrected source counter is not visual-parity evidence.
//! - The radar redraw (`0x004F42F0`) is presentation; the app re-reads the
//!   graph and fog each frame.
//! - IsParanoid (`+0x24A`, `0x005016D5`) has no reader and is not stored.

use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use crate::map::houses::{
    HouseAllianceMap, HouseRoster, is_allied_with, normalize_house_name, with_house_key,
};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::is_ally_by_object;
use crate::sim::house_strategy::update_anger_nodes;
use crate::sim::intern::InternedId;
use crate::sim::intern::StringInterner;
use crate::sim::touch_log::live_read_check_enabled;
use crate::sim::vision::RevealViewers;
use crate::sim::world::{FrameEffects, Simulation};

/// The native ScenarioInit counter is nonzero during ReadScenarioINI's House
/// pass (ordinary campaign prior2). CanAlly501540 then bypasses live-world
/// defeated/count gates; MakeAlly4F9B70 writes only the sender's ally mask.
pub(crate) enum HouseAllianceAdmission<'a> {
    ScenarioInitialization { roster: &'a HouseRoster },
    Admitted(HouseAllianceMap),
}

/// The House ally graph (`HouseClass+0x5788` masks, see the module docs).
/// Only this module writes it: the install, MakeAlly and MakeEnemy, each of
/// which also forgets the reveal viewers. Serialized and hashed exactly as
/// the bare map it wraps.
///
/// Simulation readers ask it by house id ([`Self::is_allied`],
/// [`Self::allied_to`]) and get the by-name answer from a derived
/// [`AllyMatrix`], built at the first question after a change. The graph is
/// its source of truth: a replaced graph or the test accessor drops it, a
/// load or a new graph starts without it, and a bit write updates it in
/// place, or drops it when a new ally has no row. Debug builds check it
/// against the graph when it is built and at each answer it gives. Never
/// saved, hashed or compared.
///
/// A clone shares the graph and the matrix until a write copies the one it
/// changes, so the wave damage pass's snapshot copies neither.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct HouseAlliances {
    #[serde(serialize_with = "save_graph", deserialize_with = "load_graph")]
    graph: Arc<HouseAllianceMap>,
    #[serde(skip)]
    matrix: OnceLock<Option<Arc<AllyMatrix>>>,
}

fn save_graph<S: serde::Serializer>(
    graph: &Arc<HouseAllianceMap>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&**graph, serializer)
}

fn load_graph<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<HouseAllianceMap>, D::Error> {
    <HouseAllianceMap as serde::Deserialize>::deserialize(deserializer).map(Arc::new)
}

impl std::fmt::Debug for HouseAlliances {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("HouseAlliances").field(&self.graph).finish()
    }
}

impl PartialEq for HouseAlliances {
    fn eq(&self, other: &Self) -> bool {
        self.graph == other.graph
    }
}

impl Eq for HouseAlliances {}

impl HouseAlliances {
    /// A graph with no matrix built yet.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn new(graph: HouseAllianceMap) -> Self {
        Self {
            graph: Arc::new(graph),
            matrix: OnceLock::new(),
        }
    }

    /// The graph by normalized house name, as saved and hashed.
    pub fn as_map(&self) -> &HouseAllianceMap {
        &self.graph
    }

    /// `HouseClass::IsAlliedWith @ 0x004F9A50`: whether `asker`'s own ally
    /// mask holds `other`, one-way; a house is its own ally. It answers as
    /// [`is_allied_with`] does for the two names.
    pub fn is_allied(
        &self,
        interner: &StringInterner,
        asker: InternedId,
        other: InternedId,
    ) -> bool {
        self.allied_to(interner, other)(asker)
    }

    /// [`Self::is_allied`] of many askers about one `other`, which looks up
    /// `other`'s matrix bit once: whether each asker counts `other` an ally.
    pub(crate) fn allied_to<'a>(
        &'a self,
        interner: &'a StringInterner,
        other: InternedId,
    ) -> impl Fn(InternedId) -> bool + 'a {
        let other_bit = self
            .matrix(interner)
            .and_then(|matrix| Some((matrix, row_bit(matrix.row(other)?))));
        move |asker| {
            // The by-name test's case-insensitive self arm: one interner's
            // distinct ids never name strings equal under ASCII case folding.
            if asker == other {
                return true;
            }
            let Some(allied) = other_bit.and_then(|(matrix, bit)| matrix.holds_bit(asker, bit))
            else {
                return self.by_name(interner, asker, other);
            };
            debug_assert!(
                !live_read_check_enabled() || allied == self.by_name(interner, asker, other),
                "ally matrix differs from the graph: {} -> {}",
                interner.resolve(asker),
                interner.resolve(other),
            );
            allied
        }
    }

    /// The graph's answer by name ([`is_allied_with`]).
    fn by_name(&self, interner: &StringInterner, asker: InternedId, other: InternedId) -> bool {
        is_allied_with(
            &self.graph,
            interner.resolve(asker),
            interner.resolve(other),
        )
    }

    /// The matrix, built on first use, when it answers for `interner`'s ids.
    fn matrix(&self, interner: &StringInterner) -> Option<&AllyMatrix> {
        self.matrix
            .get_or_init(|| AllyMatrix::build(&self.graph, interner).map(Arc::new))
            .as_deref()
            .filter(|matrix| matrix.interner == interner.serial())
    }

    fn replace(&mut self, alliances: HouseAllianceMap) {
        self.graph = Arc::new(alliances);
        self.matrix = OnceLock::new();
    }

    /// Write `owner_key`'s ally bit for `other_key` (normalized house names).
    fn set_ally_bit(&mut self, owner_key: String, other_key: String, allied: bool) {
        let kept = self.matrix.get_mut().is_none_or(|built| {
            built
                .as_mut()
                .is_some_and(|matrix| Arc::make_mut(matrix).set(&owner_key, &other_key, allied))
        });
        if !kept {
            self.matrix = OnceLock::new();
        }
        let allies = Arc::make_mut(&mut self.graph).entry(owner_key).or_default();
        if allied {
            allies.insert(other_key);
        } else {
            allies.remove(&other_key);
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn as_map_mut_for_test(&mut self) -> &mut HouseAllianceMap {
        self.matrix = OnceLock::new();
        Arc::make_mut(&mut self.graph)
    }
}

/// [`HouseAlliances`]' graph as ally masks, read through one interner: a row
/// per house name with an ally or in an ally set (the launch graph also keys
/// every roster house and slot, most with no ally) and the row of each id the
/// interner held at the build, by the same key ([`with_house_key`]) the
/// by-name test uses. It cannot answer for an id interned later or from
/// another interner, and is not built for more names than a mask has bits;
/// those questions go to the graph by name.
#[derive(Debug, Clone)]
struct AllyMatrix {
    /// The [`StringInterner::serial`] the id rows were read through.
    interner: u64,
    /// Those house names in graph order; a name's index is its row.
    names: Vec<String>,
    /// By `InternedId::index`: the row of the id's name, or [`NO_ROW`].
    rows: Vec<u8>,
    /// Bit `c` of `allies[r]`: row `r`'s ally set holds row `c`'s name.
    allies: [u64; MATRIX_ROWS],
}

const MATRIX_ROWS: usize = u64::BITS as usize;
/// An id whose name has no ally and is in no ally set: it is allied with
/// nobody but itself, and nobody else is allied with it.
const NO_ROW: u8 = u8::MAX;

/// Row `row`'s bit in a mask; none for [`NO_ROW`].
fn row_bit(row: u8) -> u64 {
    1u64.checked_shl(u32::from(row)).unwrap_or(0)
}

impl AllyMatrix {
    fn build(graph: &HouseAllianceMap, interner: &StringInterner) -> Option<Self> {
        let names: BTreeSet<&String> = graph
            .iter()
            .filter(|(_, allies)| !allies.is_empty())
            .flat_map(|(house, allies)| std::iter::once(house).chain(allies))
            .collect();
        if names.len() > MATRIX_ROWS {
            return None;
        }
        let mut matrix = Self {
            interner: interner.serial(),
            names: names.into_iter().cloned().collect(),
            rows: Vec::new(),
            allies: [0; MATRIX_ROWS],
        };
        matrix.rows = (0..interner.len() as u32)
            .map(|index| {
                let name = interner.resolve(InternedId::from_index(index));
                with_house_key(name, |key| matrix.name_row(key)).unwrap_or(NO_ROW)
            })
            .collect();
        for (house, allies) in graph {
            for ally in allies {
                matrix.set(house, ally, true);
            }
        }
        if live_read_check_enabled() {
            let housed: Vec<_> = (0..matrix.rows.len() as u32)
                .map(InternedId::from_index)
                .filter(|&id| matrix.row(id) != Some(NO_ROW))
                .collect();
            for &asker in &housed {
                for &other in &housed {
                    assert_eq!(
                        asker == other || matrix.holds(asker, other) == Some(true),
                        is_allied_with(graph, interner.resolve(asker), interner.resolve(other)),
                        "ally matrix built unlike the graph: {} -> {}",
                        interner.resolve(asker),
                        interner.resolve(other),
                    );
                }
            }
        }
        Some(matrix)
    }

    fn name_row(&self, name: &str) -> Option<u8> {
        self.names
            .binary_search_by(|probe| probe.as_str().cmp(name))
            .ok()
            .map(|row| row as u8)
    }

    /// `id`'s row, when the matrix covers it.
    fn row(&self, id: InternedId) -> Option<u8> {
        self.rows.get(id.index() as usize).copied()
    }

    /// Whether `asker`'s ally set holds `other`, when both are covered.
    fn holds(&self, asker: InternedId, other: InternedId) -> Option<bool> {
        let bit = row_bit(self.row(other)?);
        self.holds_bit(asker, bit)
    }

    /// Whether `asker`'s ally set holds the name whose [`row_bit`] is `bit`,
    /// when `asker` is covered.
    fn holds_bit(&self, asker: InternedId, bit: u64) -> Option<bool> {
        let allies = self.allies.get(usize::from(self.row(asker)?)).copied();
        Some(allies.is_some_and(|allies| allies & bit != 0))
    }

    /// Write the bit; false for a new ally without a row. A name without a
    /// row has no ally and is in no set, so it has no bit to clear.
    fn set(&mut self, owner: &str, other: &str, allied: bool) -> bool {
        let (Some(owner), Some(other)) = (self.name_row(owner), self.name_row(other)) else {
            return !allied;
        };
        let (owner, other) = (usize::from(owner), usize::from(other));
        if allied {
            self.allies[owner] |= 1 << other;
        } else {
            self.allies[owner] &= !(1 << other);
        }
        true
    }
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
                    .as_map()
                    .get(&key)
                    .into_iter()
                    .flatten()
                    .ne(alliances.get(&key).into_iter().flatten())
            })
            .collect();
        // A live install uses the same map visitor as individual writes.
        // Campaign ScenarioInit reaches this before any Techno exists.
        let sight_changed = !changed.is_empty();
        let context = crate::sim::world::UninitContext::with_rules(rules);
        if sight_changed {
            self.release_alliance_sight(context);
        }
        self.house_alliances.replace(alliances);
        self.reveal_viewers.forget();
        for owner in changed {
            self.rebuild_house_spatial_threat(owner, rules);
        }
        if sight_changed {
            self.admit_alliance_sight(context);
        }
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
                    self.make_enemy(owner, other, rules, registry, effects, false);
                } else {
                    self.make_ally(owner, other, rules, registry, effects, false);
                }
            }
        }
    }

    /// `Apply_Squad_Alliances @ 0x0069B170`, which Read_Scenario calls after
    /// FillInData (`0x00684B45..0x00684B53`). In GameMode 4 it clears
    /// FixedAlliance (`0x0069B18A..0x0069B19E`); when both of Start Game
    /// Now's clan lists hold a name, each entry gathers every human House
    /// (`+0x1EC`) whose name (`+0x15FF4`) matches it under `_stricmp`, in
    /// House array order (`0x0069B29B..0x0069B33A`). Each list's Houses then
    /// ally every other one in it, unannounced (`0x0069B41E..0x0069B4BE`),
    /// and FixedAlliance is set (`0x0069B4DB..0x0069B4E5`).
    /// GameMode 4 skips Make_Ally's initialization bypass
    /// (`0x004F9C1F..0x004F9C28`), so the live tail runs here as it does in
    /// VERA outside the placement scope.
    /// RESIDUAL: Session+0x688, set beside the flag (`0x0069B4D4`), has no
    /// traced reader. Native still holds ScenarioInit `[0xA8E7AC]` here
    /// (released at `0x00684B63`), so Can_Ally returns early (`0x005015A1`);
    /// VERA runs its defeated and remaining-enemy gates, which no valid clan
    /// start trips. Whether the live tail's alliance lines reach the message
    /// list at start is not checked.
    pub(crate) fn apply_squad_alliances(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        if self.session.game_mode() != crate::sim::scenario_session::ScenarioGameMode::Wol {
            return;
        }
        self.session.set_fixed_alliance(false);
        let lists = self.session.wol_squad_lists().clone();
        if lists.iter().any(Vec::is_empty) {
            return;
        }
        for list in &lists {
            let mut group = Vec::new();
            for name in list {
                for owner in self.houses.order() {
                    let house = &self.houses[owner];
                    if house.is_human()
                        && house.native_scenario_name().is_some_and(|house_name| {
                            crate::util::native_string::crt_strcmpi(house_name, name).is_eq()
                        })
                    {
                        group.push(*owner);
                    }
                }
            }
            if group.len() > 1 {
                for &owner in &group {
                    for &other in &group {
                        if owner != other {
                            self.make_ally(
                                owner,
                                other,
                                rules,
                                registry,
                                FrameEffects::default(),
                                false,
                            );
                        }
                    }
                }
            }
        }
        self.session.set_fixed_alliance(true);
    }

    /// `HouseClass::Make_Ally @ 0x004F9B70`. Only the ALLY event passes
    /// `announce` (`0x004C6E1E`); Paranoid, the triggers and the launch
    /// alliances pass 0.
    pub(crate) fn make_ally(
        &mut self,
        owner: InternedId,
        other: InternedId,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
        announce: bool,
    ) {
        // Keep the live and initialization arms in this same graph owner.
        if !self.can_ally(owner, other, rules) {
            return;
        }
        let context = crate::sim::world::UninitContext::new(rules, registry).with_effects(effects);
        self.release_alliance_sight(context);
        // 0x004F9B9F..0x004F9BB3, then the threat rebuild (0x004F9BB9).
        Self::set_ally_bit(
            &self.interner,
            &mut self.house_alliances,
            &mut self.reveal_viewers,
            owner,
            other,
            true,
        );
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
                    let _ = self.assign_target_represented(id, None, rules, registry, effects);
                }
            }
            self.announce_alliance(owner, other, true, announce);
        }
        // 0x004F9F46..0x004F9F4F: both arms end in the re-admission bracket.
        self.admit_alliance_sight(context);
    }

    /// `HouseClass::Make_Enemy @ 0x004F9F90`. Only the ALLY event passes
    /// `announce` (`0x004C6E08`).
    pub(crate) fn make_enemy(
        &mut self,
        owner: InternedId,
        other: InternedId,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
        announce: bool,
    ) {
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
        let context = crate::sim::world::UninitContext::new(rules, registry).with_effects(effects);
        self.release_alliance_sight(context);
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
        if is_ally_by_object(Some(&self.house_alliances), &self.interner, owner, other) {
            Self::set_ally_bit(
                &self.interner,
                &mut self.house_alliances,
                &mut self.reveal_viewers,
                owner,
                other,
                false,
            );
            self.rebuild_house_spatial_threat(owner, rules);
            // 0x004FA067..0x004FA0DF: the other house breaks its side too, then
            // rescans its enemy: native passes it as both house and peer, and a
            // house holds no anger node of its own.
            let both = is_ally_by_object(Some(&self.house_alliances), &self.interner, other, owner);
            if both {
                Self::set_ally_bit(
                    &self.interner,
                    &mut self.house_alliances,
                    &mut self.reveal_viewers,
                    other,
                    owner,
                    false,
                );
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
            if !self.object_placement_scope_active() {
                self.announce_alliance(owner, other, false, announce && both);
            }
        }
        //4FA1DA is shared by changed-edge and no-edge paths.
        self.admit_alliance_sight(context);
    }

    /// The human tail of MakeAlly (`0x004F9D4E..0x004F9F35`) and MakeEnemy
    /// (`0x004FA0E4..0x004FA1D5`), outside initialization. Every client
    /// prints `TXT_HAS_ALLIED` / `TXT_AT_WAR` with both UI names when the
    /// sender is human in a nonzero mode (MakeAlly also needs a target that
    /// is not MultiplayPassive). The EVA line depends on the local House, so
    /// it is chosen here, at the call, against the graph as it now stands.
    /// `voice` is `announce`, and for a break also that both sides broke.
    fn announce_alliance(
        &mut self,
        owner: InternedId,
        other: InternedId,
        formed: bool,
        voice: bool,
    ) {
        let game_mode_nonzero = self.session.game_mode_nonzero();
        let human = self
            .houses
            .get(&owner)
            .is_some_and(|house| house.is_controlled_by_human(game_mode_nonzero));
        let passive_target = self
            .houses
            .get(&other)
            .is_some_and(|house| house.multiplay_passive());
        let line = game_mode_nonzero && human && (!formed || !passive_target);
        let local = self.session.current_house();
        let allied = |asker, house| {
            is_ally_by_object(Some(&self.house_alliances), &self.interner, asker, house)
        };
        // Every voice sits inside the line's gate (0x004F9D72..0x004F9D8D and
        // 0x004FA0EB..0x004FA106 jump past both).
        let eva = match (voice && line, formed, local) {
            (false, _, _) | (_, _, None) => None,
            // 0x004F9E01..0x004F9E85: the alliance is now mutual.
            (true, true, Some(local)) if allied(other, owner) => {
                if self.local_player_defeated() {
                    None
                } else if allied(local, other) && allied(local, owner) {
                    Some(crate::sim::world::AllianceEva::Formed)
                } else {
                    Some(crate::sim::world::AllianceEva::EnemyFormed)
                }
            }
            // 0x004F9E8A..0x004F9F35: one way, only the two Houses hear it.
            (true, true, Some(local)) if game_mode_nonzero && local == owner => {
                Some(crate::sim::world::AllianceEva::Requesting)
            }
            (true, true, Some(local)) if game_mode_nonzero && local == other => {
                Some(crate::sim::world::AllianceEva::Requested)
            }
            (true, true, _) => None,
            // 0x004FA186..0x004FA1D5.
            (true, false, Some(local))
                if game_mode_nonzero && (local == owner || local == other) =>
            {
                Some(crate::sim::world::AllianceEva::Broken)
            }
            (true, false, _) => None,
        };
        if line || eva.is_some() {
            self.sound_events
                .emit(crate::sim::world::SimSoundEvent::AllianceChanged {
                    owner,
                    other,
                    formed,
                    line,
                    eva,
                });
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
    fn set_ally_bit(
        interner: &StringInterner,
        house_alliances: &mut HouseAlliances,
        reveal_viewers: &mut RevealViewers,
        owner: InternedId,
        other: InternedId,
        allied: bool,
    ) {
        let owner_key = normalize_house_name(interner.resolve(owner));
        let other_key = normalize_house_name(interner.resolve(other));
        house_alliances.set_ally_bit(owner_key, other_key, allied);
        reveal_viewers.forget();
    }
}

#[cfg(test)]
#[path = "house_diplomacy_tests.rs"]
mod tests;
