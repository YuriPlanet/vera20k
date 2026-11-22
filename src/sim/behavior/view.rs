//! What one executing object may look at: its own state, and the threats
//! around it.
//!
//! [`ObjectView`] is a projection, not a handle: it copies the fields an
//! execution behaviour decides on and is cheap to build. [`ThreatView`] is a
//! borrowed, radius-limited enemy query; it allocates nothing and reads the
//! house alliance graph the game already keeps, so "enemy" means what it means
//! everywhere else (`sim::house_diplomacy`, `MultiplayPassive` houses
//! excluded).
//!
//! Neither type writes anything. An execution behaviour that wants to act
//! returns a [`super::goal::Goal`]; [`super::order`] is the only writer path.

use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionId;
use crate::sim::world::Simulation;

/// One executing object, as it sees itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectView {
    /// The stable id every goal names as its actor or target.
    pub entity_id: u64,
    pub type_id: InternedId,
    pub cell: (u16, u16),
    pub health: i32,
    /// Full strength from the frozen rules the object was built with.
    pub max_health: i32,
    /// `effective()`: the mission the object is on, or is queued onto, now.
    pub mission: MissionId,
    /// The object this one is currently aiming at, if any.
    pub attack_target: Option<u64>,
    /// Frames left before the weapon may fire again. Zero or less is ready.
    pub rearm_frames_left: i32,
    /// The primary weapon's range in cells, rounded up; zero when unarmed.
    pub weapon_range_cells: i32,
    /// The type's `Ammo=` limit; `None` when unlimited (`Ammo=-1`, the
    /// default). Remaining rounds are not projected: no per-shot ammunition
    /// counter is ported for units in this slice, so this is a limit, not a
    /// magazine reading.
    pub ammo_limit: Option<i32>,
    pub is_aircraft: bool,
    pub is_structure: bool,
    pub is_engineer: bool,
}

impl ObjectView {
    /// Project `entity_id` as the rules describe it. `None` when the object is
    /// gone or its type is not in the rules.
    pub fn new(sim: &Simulation, rules: &RuleSet, entity_id: u64) -> Option<Self> {
        let entity = sim.entities().get(entity_id)?;
        let ty = sim.object_type(entity.type_ref(), rules)?;
        let (range_leptons, _) = gun_range(rules, ty);
        Some(Self {
            entity_id,
            type_id: entity.type_ref(),
            cell: (entity.position.rx(), entity.position.ry()),
            health: entity.health.current(),
            max_health: ty.strength,
            mission: entity.mission.effective(),
            attack_target: match entity.attack_target.target() {
                Some(TargetKind::Entity(id)) => Some(id),
                _ => None,
            },
            rearm_frames_left: entity
                .seldom
                .rearm_timer
                .remaining(sim.session().binary_frame() as i32),
            weapon_range_cells: cells_of_leptons(range_leptons),
            ammo_limit: (ty.ammo >= 0).then_some(ty.ammo),
            is_aircraft: ty.category == ObjectCategory::Aircraft,
            is_structure: ty.category == ObjectCategory::Building,
            is_engineer: ty.engineer,
        })
    }

    /// Health as a fraction of full strength, in permille (0..=1000).
    pub fn health_permille(&self) -> u16 {
        if self.max_health <= 0 {
            return 0;
        }
        let scaled = i64::from(self.health.max(0)) * 1000 / i64::from(self.max_health);
        u16::try_from(scaled.clamp(0, 1000)).unwrap_or(1000)
    }

    /// Whether the weapon may fire this frame.
    pub fn weapon_ready(&self) -> bool {
        self.rearm_frames_left <= 0
    }

}

/// The primary weapon's effective range in leptons and the frames its reload
/// takes, as the rules carry them.
fn gun_range(rules: &RuleSet, ty: &crate::rules::object_type::ObjectType) -> (i32, i32) {
    let Some(name) = ty.primary() else {
        return (0, 0);
    };
    let Some(weapon) = rules.weapon(name) else {
        return (0, 0);
    };
    let minimum = weapon.minimum_range_leptons.max(0);
    ((weapon.range_leptons - minimum).max(0), weapon.rof)
}

fn cells_of_leptons(leptons: i32) -> i32 {
    let per_cell = crate::util::lepton::LEPTONS_PER_CELL_I32;
    leptons.div_euclid(per_cell) + i32::from(leptons.rem_euclid(per_cell) != 0)
}

/// Chebyshev distance in cells: the metric the game's own adjacency uses.
pub fn cell_distance(a: (u16, u16), b: (u16, u16)) -> i32 {
    let dx = i32::from(a.0).abs_diff(i32::from(b.0));
    let dy = i32::from(a.1).abs_diff(i32::from(b.1));
    i32::try_from(dx.max(dy)).unwrap_or(i32::MAX)
}

/// One hostile object near the observer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Threat {
    pub entity_id: u64,
    pub type_id: InternedId,
    pub cell: (u16, u16),
    /// Chebyshev distance in cells from the query's origin.
    pub cells: i32,
}

/// The enemies around one house, borrowed: a radius-limited query, no storage.
pub struct ThreatView<'a> {
    sim: &'a Simulation,
    house: InternedId,
    /// The houses this one may not target: its alliance set, read once when the
    /// view is made, from the same graph `ally_test` answers.
    allies: Vec<InternedId>,
}

impl<'a> ThreatView<'a> {
    pub fn new(sim: &'a Simulation, house: InternedId) -> Self {
        let asker = sim.interner().resolve(house).to_owned();
        let allied = crate::map::houses::ally_test(sim.house_alliances.as_map(), &asker);
        let allies = sim
            .houses()
            .keys()
            .copied()
            .filter(|other| allied(sim.interner().resolve(*other)))
            .collect();
        Self { sim, house, allies }
    }

    /// Whether this view treats `owner` as an ally, and so never as a target.
    fn is_allied(&self, owner: InternedId) -> bool {
        self.allies.contains(&owner)
    }

    /// The house the query is scoped to.
    pub fn house(&self) -> InternedId {
        self.house
    }

    /// The nearest live hostile object within `radius` cells of `from`, ties
    /// going to the earlier id. Deterministic: ids are walked in storage order.
    pub fn nearest(&self, from: (u16, u16), radius: i32) -> Option<Threat> {
        self.within(from, radius)
            .min_by_key(|threat| (threat.cells, threat.entity_id))
    }

    /// Every live hostile object within `radius` cells of `from`.
    ///
    /// One pass over the entity store per call: the cost is `entities × stepped
    /// routines` per frame and the caller bounds the second factor with its
    /// per-frame rotation budget.
    pub fn within(&self, from: (u16, u16), radius: i32) -> impl Iterator<Item = Threat> + '_ {
        let house = self.house;
        let houses = self.sim.houses();
        self.sim.entities().values().filter_map(move |entity| {
            let owner = entity.owner();
            if owner == house || !is_live_target(entity) {
                return None;
            }
            let target = houses.get(&owner)?;
            if target.multiplay_passive() || self.is_allied(owner) {
                return None;
            }
            let cell = (entity.position.rx(), entity.position.ry());
            let cells = cell_distance(from, cell);
            if cells > radius {
                return None;
            }
            Some(Threat {
                entity_id: entity.stable_id(),
                type_id: entity.type_ref(),
                cell,
                cells,
            })
        })
    }

    /// How many live hostile objects stand within `radius` cells of `from`.
    pub fn count_within(&self, from: (u16, u16), radius: i32) -> usize {
        self.within(from, radius).count()
    }
}

/// Alive, out of limbo, above zero strength: the same predicate the order gate
/// uses for actors.
fn is_live_target(entity: &GameEntity) -> bool {
    entity.is_object_alive() && !entity.lifecycle.in_limbo() && entity.health.current() > 0
}
