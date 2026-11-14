//! The house's own ledger: one pass over the entity store and the factories,
//! read by every decision that would otherwise ask "how many of mine" again.
//!
//! Before this module each decision re-walked `Observation::own_entities` for
//! its own question — the fighter count, the harvesters, the refineries, the
//! `[AI]` list counts, the price bands, the role coverage — six passes over the
//! whole store per frame, on a design that scales to twenty thousand objects.
//! One owner answers all of them once, and the answers are the same ones: the
//! counts are of the entities the projection yields, the lines are the
//! factories' own fields (`Observation::factory_line`), and the role coverage
//! is the same [`RoleWeights`] dot product the old `shortfall` computed.
//!
//! Rebuilt every frame, never cached across one: a ledger that lags the entity
//! store by a frame is a ledger a decision can be wrong with. The storage is
//! reused (the vectors keep their capacity), so a steady army costs no
//! allocation.
//!
//! Determinism: entity-storage order and `BTreeMap` order only. The role
//! arithmetic is `f64` and comparisons only, exactly as the weights it reads.

use std::collections::BTreeMap;

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::rules::unit_role::{ROLES, Role, RoleTable, role_against_armour};
use crate::sim::control::observation::{EnemyEntity, Observation};
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionId;
use crate::sim::production::{ProductionCategory, category_for_object};

/// The five pools a house's assets fall in, in the order [`Census::counts`]
/// holds them. Naval units are `Vehicle` types carrying `Naval=`, so they are
/// their own pool here rather than a second reading of the same category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Slot {
    Building,
    Infantry,
    Vehicle,
    Aircraft,
    Ship,
}

pub(super) const SLOTS: [Slot; 5] = [
    Slot::Building,
    Slot::Infantry,
    Slot::Vehicle,
    Slot::Aircraft,
    Slot::Ship,
];

impl Slot {
    pub(super) fn index(self) -> usize {
        match self {
            Self::Building => 0,
            Self::Infantry => 1,
            Self::Vehicle => 2,
            Self::Aircraft => 3,
            Self::Ship => 4,
        }
    }

    /// The pool a production category's finished product lands in. A defense
    /// is a building: it stands on base ground like any other.
    pub(super) fn of(category: ProductionCategory) -> Self {
        match category {
            ProductionCategory::Building | ProductionCategory::Defense => Self::Building,
            ProductionCategory::Infantry => Self::Infantry,
            ProductionCategory::Vehicle => Self::Vehicle,
            ProductionCategory::Aircraft => Self::Aircraft,
            ProductionCategory::Ship => Self::Ship,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Infantry => "infantry",
            Self::Vehicle => "vehicle",
            Self::Aircraft => "aircraft",
            Self::Ship => "ship",
        }
    }
}

/// The production lines, in the order [`Census::lines`] holds them.
pub(super) const LINE_BUILDING: usize = 0;
pub(super) const LINE_DEFENSE: usize = 1;
pub(super) const LINE_INFANTRY: usize = 2;
pub(super) const LINE_VEHICLE: usize = 3;
pub(super) const LINE_AIRCRAFT: usize = 4;
pub(super) const LINE_SHIP: usize = 5;

fn line_index(category: ProductionCategory) -> usize {
    match category {
        ProductionCategory::Building => LINE_BUILDING,
        ProductionCategory::Defense => LINE_DEFENSE,
        ProductionCategory::Infantry => LINE_INFANTRY,
        ProductionCategory::Vehicle => LINE_VEHICLE,
        ProductionCategory::Aircraft => LINE_AIRCRAFT,
        ProductionCategory::Ship => LINE_SHIP,
    }
}

/// One line's occupancy this frame.
///
/// A house that has never started a build has no factory of that category in
/// its own slot yet — `HouseClass`'s slot is filled when production starts,
/// not when the barracks is placed — so "no line" and "an idle line" look the
/// same from here, and both mean the same thing to a caller: nothing is being
/// made, so a request is what creates the line. Whether the house can make a
/// type at all is `CanBuild`'s answer, not this struct's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Line {
    pub(super) product: Option<InternedId>,
    pub(super) suspended: bool,
    pub(super) queued: usize,
    /// The finished product the factory still holds: a building waiting for a
    /// cell, or a unit waiting to be taken out.
    pub(super) ready: Option<InternedId>,
}

/// Whether the house is defending what it holds or looking for the enemy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Posture {
    /// Hold the held ground and the ground behind it; do not go looking.
    #[default]
    Turtle,
    /// Every opponent can erase the base and this house cannot answer in kind,
    /// so waiting only feeds the charge: go before the superweapon finishes.
    Race,
}

impl Line {
    /// The factory is working: something is on the line, in the queue, or
    /// finished and not yet taken.
    pub(super) fn busy(&self) -> bool {
        self.product.is_some() || self.ready.is_some() || self.queued > 0
    }
}

/// The nearest cell beside the base an object can stand on: outside the
/// anchor's own foundation, on ground a unit can walk, and not a cell one of
/// the house's buildings sits on.
///
/// The rings are walked outwards and the first admissible cell in the fixed
/// ring order wins, so the answer is the same every frame and does not depend
/// on what the entity store happens to yield.
pub(super) fn rally_near(
    obs: &Observation<'_>,
    anchor: (u16, u16),
    home: &[(u16, u16)],
) -> Option<(u16, u16)> {
    for ring in 2i32..=4 {
        for dy in -ring..=ring {
            for dx in -ring..=ring {
                if dx.abs().max(dy.abs()) != ring {
                    continue;
                }
                let (Some(rx), Some(ry)) = (
                    u16::try_from(i32::from(anchor.0) + dx).ok(),
                    u16::try_from(i32::from(anchor.1) + dy).ok(),
                ) else {
                    continue;
                };
                if !obs.is_ground_walkable(rx, ry) || home.contains(&(rx, ry)) {
                    continue;
                }
                return Some((rx, ry));
            }
        }
    }
    None
}

/// What a type is, to the decisions that count it. Resolved once per unit
/// while the ledger is built, so no decision has to re-open the rules.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Traits {
    /// A unit that fights: infantry, vehicle or aircraft with a primary weapon,
    /// and neither a harvester, an engineer nor a yard maker
    /// ([`super::orders::is_combat_unit`]).
    pub(super) combat: bool,
    /// `ResourceGatherer=`: the house's income.
    pub(super) harvester: bool,
    /// `ResourceDestination=`: an unloading bay.
    pub(super) refinery: bool,
    /// `BuildConst=` eligible: a Construction Yard.
    pub(super) yard: bool,
    /// `DeploysInto=` a yard: an MCV.
    pub(super) yard_maker: bool,
    pub(super) engineer: bool,
    /// It walks into a building: `Infiltrate=`, which retail's ReadINI postpass
    /// also grants to `C4=`, `Engineer=` and `Agent=`. So an engineer is an
    /// infiltrator, and a want for spies is not a want for this field —
    /// counting spies with it counts the engineers as spies and never builds
    /// one. [`Self::spy`] is the spy's own tag.
    pub(super) infiltrator: bool,
    /// `Agent=`: the infiltrator that enters `Spyable=` buildings, where every
    /// other infiltrator enters `Capturable=` ones.
    pub(super) spy: bool,
    pub(super) occupier: bool,
    /// `DeployFire=`: a soldier the D key turns into a position
    /// (`[E1]` and `[GGI]` carry `Deployer=yes` with it).
    pub(super) deployer: bool,
    /// `NotHuman=yes` with `Natural=yes`, the retail attack dog's own pair: a
    /// scanner with teeth, and the only type the rules mark as neither human
    /// nor mechanical.
    pub(super) scout: bool,
    /// It has a weapon in some slot, primary or not.
    ///
    /// `Primary=` alone was the test, which is a fact about retail's key
    /// layout and not about the type: a unit a mod gives only a `Secondary=` is
    /// armed, and the brain refused to treat it as army. What slot the weapon
    /// sits in is [`Self::deploy_slot`]'s business.
    pub(super) armed: bool,
    /// It takes a building by walking into it: `Engineer=` or `Infiltrate=`.
    pub(super) captures: bool,
    /// It carries others: `Passengers=` at or above a transport's seats.
    pub(super) carries: bool,
    /// The weapon slot it fires while deployed, for a `DeployFire=` infantry:
    /// `DeployFireWeapon=`, slot 1 (the secondary) by default. `None` when the
    /// type has no such pose or the slot is empty.
    pub(super) deploy_slot: Option<usize>,
    /// `C4=yes`: it takes a building apart by walking into it
    /// ([`ObjectType::c4`]). The engineer's cousin, and not the same job.
    pub(super) c4: bool,
    /// `Explodes=yes`: it takes itself apart, and `DeathWeapon=` goes off with
    /// it (`[TERROR]`, `[DTRUCK]`). The tag that says "spend this one and it
    /// spends itself", as opposed to `c4`'s "walk it in and keep it".
    pub(super) explodes: bool,
    /// `OpenTopped=yes`: its passengers fire out of it.
    ///
    /// With `Passengers=` this is the IFV, the battle fortress and every mod's
    /// take on them, which is why the tag is the key and not the name. What an
    /// open-topped carrier *becomes* is its passenger's `IFVMode=`, and that is
    /// runtime state — the tag says the body can do it, not what it is doing.
    pub(super) open_topped: bool,
    /// `Spawned=yes` (`TechnoTypeClass+0xD54`): another object made it, so it is
    /// not the player's to command — a paradrop's `PDPLANE`, a carrier's
    /// Hornet, a V3's missile.
    ///
    /// The key says so in retail: `[PDPLANE]` carries `Spawned=yes ; Created by
    /// another object and therefore not player controllable` and
    /// `Selectable=no`. It also carries a dummy `Primary=`, so without this tag
    /// it reads as army: the brain handed a cargo plane a combat order, the
    /// plane's mission was rewritten mid-drop, and the paradrop never landed —
    /// which is exactly what the log shows (`fire ParaDropSpecial` repeating
    /// for the rest of the match).
    pub(super) spawned: bool,
    /// `BuildLimit=1`: the type the rules allow one of, which is what a hero
    /// is (`[TANY] BuildLimit=1`, `[BORIS] BuildLimit=1`).
    ///
    /// The wishlist's "保护英雄单位（限制建造的都是英雄单位）": the rules' own
    /// statement of "there is one of these and it cannot be replaced" is the
    /// cap, so no name list is needed and a mod's hero is covered.
    pub(super) irreplaceable: bool,
    /// `UnitRepair=yes` (`[GADEPT]`, `ini/rulesmd.ini:11895`): a shed that puts a
    /// vehicle back together. What a hurt vehicle walks to instead of walking
    /// home.
    pub(super) repairs_units: bool,
    /// `Aircraft`: the one category with no ground cell of its own.
    ///
    /// A guard order is a cell to stand on, and an aircraft has none: the
    /// `AirportBound=` plane's `Guard` replaces the airfield's rearm-and-return
    /// mission, so it flies out on the racks it had. Layers that stand a unit
    /// somewhere ask this instead of the category, so the rule is written once.
    pub(super) aircraft: bool,
    /// Its weapon spends the unit: `Suicide=yes` on the primary
    /// (`[TERROR] TerrorBomb`, `[DTRUCK] Demobomb`).
    ///
    /// The wishlist's "恐怖分子，自爆卡车…有特殊攻击能力且存在误伤". A suicide
    /// weapon is not a rifle with a shorter range: firing it removes the unit,
    /// so the ordinary rules — advance with the fist, trade at a third of a
    /// health bar, kite out of a shell — are all wrong for it. It is a
    /// one-shot, and the only decision that fits is "spend it on something
    /// worth more than it costs".
    pub(super) suicide: bool,
    /// Firing it hurts the ground around the target too: a warhead with
    /// `CellSpread=` above 1 (`DemobombWH 8`, `RadEruptionWarhead 10`,
    /// `TerrorBombWH 2`), or a `AreaFire=` weapon.
    ///
    /// This is what "误伤" is made of. The layer that picks a target for one of
    /// these has to know that a hostile standing among the house's own units is
    /// not a target at all: the blast does not read the owner list.
    pub(super) splash: bool,
    /// Firing it does not damage but transforms: `Temporal=yes`
    /// (`ChronoBeam`, the chrono legionnaire's rifle), or a warhead that
    /// mind-controls.
    ///
    /// The wishlist's "超时空军团兵…存在误伤或者中断后攻击失效": a temporal
    /// attack is a channel, not a shot, and a `Move` that interrupts it leaves
    /// the victim whole and the unit's gun on cooldown. A layer that would walk
    /// this unit must not.
    pub(super) channels: bool,
}

/// Seats at which a type is a transport rather than a fighting unit, in the
/// retail rules.
///
/// The night hawk (`[SHAD]`, `Passengers=5`) and the hind (`[HIND]`,
/// `Passengers=10`) are the air transports, and both carry a cannon — so "has a
/// weapon" counts them as army. They are not army: they are how the army gets
/// somewhere. The bar is [`crate::rules::unit_role::TRANSPORT_SEATS`], the
/// owner's reading, not a second copy of it.
const TRANSPORT_SEATS: i32 = crate::rules::unit_role::TRANSPORT_SEATS;

/// What a type's weapons do beyond "damage the thing you aimed at", collected
/// in one pass over the slots.
///
/// Every field is read off the weapon or its warhead rather than off a name, so
/// a mod's bomb truck is covered by the same reading. The split from [`Traits`]
/// exists only because the keys live on two other objects.
#[derive(Debug, Default, Clone, Copy)]
struct WeaponTraits {
    suicide: bool,
    splash: bool,
    area_fire: bool,
    channels: bool,
}

impl Traits {
    /// What a type can do, read from the keys the type itself declares.
    ///
    /// One owner for the question every decision asks — "is this thing army, a
    /// tool, a carrier, a position" — and the answer is *the type's own keys*,
    /// not its name and not the shape of retail's `Primary=`. The brain used to
    /// answer it in five places with five hand-rolled tests over one or two
    /// keys, which is correct for `[E1]` and `[ADOG]` and wrong for anything a
    /// mod adds: an engineer with no `Primary=` was not army and not a tool, a
    /// unit with only a `Secondary=` was not army at all, and a type whose
    /// custom ability lives in a key nobody had looked at yet was invisible.
    ///
    /// Every field below is a key. A new key that describes what a type can do
    /// belongs here first, and the decisions read it from here.
    pub(super) fn of(rules: &RuleSet, ty: &ObjectType) -> Self {
        let armed = (0..8).any(|slot| ty.weapon_at(slot).is_some());
        // The weapons this type fires, in every slot: a suicide truck's bomb is
        // its `Primary=`, and a deployed desolator's eruption is its
        // `Secondary=`, so reading slot 0 alone would miss half of them.
        let mut weapon_traits = WeaponTraits::default();
        for slot in 0..8 {
            let Some(weapon) = ty.weapon_at(slot).and_then(|name| rules.weapon(&name)) else {
                continue;
            };
            weapon_traits.suicide |= weapon.suicide;
            weapon_traits.area_fire |= weapon.area_fire;
            if let Some(warhead) = weapon
                .warhead
                .as_deref()
                .and_then(|name| rules.warhead(name))
            {
                // Above 1 is a blast that reaches a neighbouring cell; exactly 1
                // is the target's own cell, which is every weapon.
                weapon_traits.splash |= warhead.cell_spread_f64 > 1.0;
                weapon_traits.channels |= warhead.temporal || warhead.mind_control;
            }
        }
        let captures = ty.engineer || ty.infiltrate;
        let carries = ty.passengers >= TRANSPORT_SEATS;
        // `DeployFireWeapon=` names the slot, defaulting to the secondary: the
        // G.I. stands up with `M60` (slot 0) and digs in with `Para` (slot 1).
        let deploy_slot = ty
            .deploy_fire
            .then(|| usize::try_from(ty.deploy_fire_weapon.max(0)).ok())
            .flatten()
            .filter(|slot| ty.weapon_at(*slot).is_some());
        Self {
            // `spawned` is the controllability tag, and it has to be here as
            // well as on the ledger: a cargo plane carries a dummy `Primary=`
            // and answers every other test as army, so without it the brain
            // hands the paradrop's own plane a combat order, the plane's
            // mission is rewritten mid-drop, and the drop never lands.
            // One owner for "is this army": the rules-level predicate the
            // autoplay policy asks too, so the two brains cannot disagree
            // about what a squad is made of.
            combat: crate::rules::unit_role::is_army(rules, ty),
            harvester: ty.resource_gatherer,
            refinery: ty.resource_destination,
            yard: ty.build_const_eligible,
            yard_maker: super::expand::makes_a_yard(rules, ty),
            engineer: ty.engineer,
            infiltrator: ty.infiltrate,
            spy: ty.agent,
            occupier: ty.occupier,
            deployer: ty.deploy_fire,
            scout: ty.not_human && ty.natural,
            armed,
            captures,
            carries,
            deploy_slot,
            c4: ty.c4,
            explodes: ty.explodes,
            open_topped: ty.open_topped,
            spawned: ty.spawned,
            irreplaceable: ty.build_limit == 1,
            repairs_units: ty.unit_repair,
            aircraft: ty.category == ObjectCategory::Aircraft,
            suicide: weapon_traits.suicide,
            splash: weapon_traits.splash || weapon_traits.area_fire,
            channels: weapon_traits.channels,
        }
    }
}

/// One object of the house, as the decisions see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Unit {
    pub(super) entity_id: u64,
    pub(super) type_id: InternedId,
    pub(super) cell: (u16, u16),
    /// Health as permille of the type's full strength.
    pub(super) health_permille: u16,
    /// What it is worth in a fight, in hit points: [`fight_points`] of its own
    /// type at the health it has left, and zero for anything that does not
    /// fight. Resolved while the ledger is built, because the ledger is the
    /// only pass that has the type open.
    pub(super) points: i32,
    pub(super) mission: MissionId,
    pub(super) orderable: bool,
    /// A team's script or one of the player's control groups owns it
    /// ([`Observation::owned_by_hand`]).
    pub(super) hand: bool,
    pub(super) slot: Slot,
    /// The price band of its type: cheap, the line unit, the heavy end.
    pub(super) tier: usize,
    pub(super) traits: Traits,
}

impl Unit {
    /// The mission owns this unit's destination: a garrison, a depot visit, a
    /// repair job or a sabotage is under way, current or queued.
    ///
    /// A movement or kite layer that orders a `Move` over one of these clears
    /// the nav list, and the mission does not put it back — `mission_capture`
    /// only re-aims when `NavCom` is empty, and `NavCom` is now a cell. That is
    /// the walk-in that "took the command" and then stood at the wall, and the
    /// depot visit that ends at the fence.
    /// Whether the brain may order this unit at all: the world takes an order
    /// for it, the player is not steering it, and no other object made it.
    ///
    /// The last clause is the controllability tag every layer needs: a
    /// paradrop's cargo plane, a carrier's Hornet and a V3's missile carry
    /// `Spawned=yes` and are driven by the object that made them, so an order
    /// from here does not steer them — it derails them. Read it instead of
    /// restating two of the three tests at each call site.
    pub(super) fn controllable(&self) -> bool {
        self.orderable && !self.hand && !self.traits.spawned
    }

    pub(super) fn in_service(&self) -> bool {
        use crate::sim::mission::MissionType;
        // `Sleep` is not one of them: it is the mission a freshly created
        // object carries before its first AI slot, so including it would take
        // every idle unit out of the kite and dodge passes. A docked unit is
        // held by the retreat's own `tracked.hold` instead.
        matches!(
            self.mission.known(),
            Some(
                MissionType::Capture
                    | MissionType::Enter
                    | MissionType::Repair
                    | MissionType::Sabotage
            )
        )
    }
}

/// The house's assets for one frame.
#[derive(Debug, Default)]
pub(super) struct Census {
    /// Building, infantry, vehicle, aircraft, ship.
    pub(super) counts: [usize; 5],
    /// Every own type's count, for the questions a name-parsed rules list asks.
    pub(super) by_type: BTreeMap<InternedId, usize>,
    pub(super) lines: [Line; 6],
    pub(super) units: Vec<Unit>,
    pub(super) enemies: Vec<EnemyEntity>,
    /// Visible enemy buildings that carry an offensive superweapon, and how many
    /// enemy houses that is spread across. Read as a pair: a count alone says
    /// nothing, because a mod decides how many houses there are.
    pub(super) enemy_strike_sites: usize,
    /// The house's own buildings that carry an offensive superweapon. What makes
    /// a defensive posture a *choice* rather than a surrender.
    pub(super) own_strike_sites: usize,
    /// Houses this one is at war with.
    pub(super) enemy_houses: usize,
    /// Neutral and civilian buildings: nobody's, so they are in no enemy list,
    /// but they are ground the army walks into. What may be shot out of the way
    /// is decided where the walking happens.
    pub(super) neutrals: Vec<EnemyEntity>,
    /// Cells of the house's own buildings: the yard, the plants, the
    /// factories, the defenses.
    pub(super) home_cells: Vec<(u16, u16)>,
    /// The base's anchor: the yard when one stands, otherwise whatever building
    /// the house still has.
    pub(super) base_cell: Option<(u16, u16)>,
    /// Where the army gathers, and what a hurt unit walks home to when the
    /// house has no `UnitRepair=` bay: a cell **beside** the base.
    ///
    /// The anchor is the yard's own cell, and the yard is a 3x3 foundation with
    /// no lane into it, so a retreat told to walk there spends the whole
    /// retreat running around the outside of its own base and arrives where it
    /// started — the unit that "returned to base" and then paced inside it.
    pub(super) rally: Option<(u16, u16)>,
    /// The cell of the house's `UnitRepair=` building, when one stands: where a
    /// hurt vehicle goes back to. The post a retreat walks to, instead of the
    /// yard — a unit that walks home from a fight and stands there is a unit the
    /// house paid for and cannot use.
    pub(super) repair_cell: Option<(u16, u16)>,
    /// The first `UnitRepair=` building itself, so the retreat can click it
    /// rather than walk at its cell: entering the bay is an Enter order on the
    /// building (`FootClass::ClickedAction`'s depot arm), and a cell cannot be
    /// entered.
    pub(super) repair_building: Option<u64>,
    pub(super) harvesters: usize,
    pub(super) refineries: usize,
    pub(super) yards: usize,
    pub(super) yard_makers: usize,
    /// Units that fight, harvesters and engineers aside.
    pub(super) fighters: usize,
    /// Combat units with no order, no destination, no target and no live
    /// intent: the wishlist's "单位在发呆".
    pub(super) idle: usize,
    /// Fighting units per price band, per pool.
    pub(super) tiers: [[usize; 3]; 5],
    /// Finished buildings waiting for a cell, in factory order.
    pub(super) ready: Vec<InternedId>,
    /// The jobs the enemy's own types ask for, counted.
    pub(super) threat: [u32; ROLES.len()],
    /// How many of this house's fighting units can actually do each job: their
    /// weight in it clears [`ROLE_FLOOR`].
    pub(super) able: [u32; ROLES.len()],
}

impl Census {
    /// Whether every bay has the harvesters it wants
    /// ([`super::build::HARVESTERS_PER_REFINERY`]).
    ///
    /// One owner for the measure the reserve, the tree's gate and the vehicle
    /// line all read: three copies of the comparison is three chances for the
    /// house to call itself short in one place and served in another.
    pub(super) fn harvesters_served(&self) -> bool {
        self.harvesters >= super::build::HARVESTERS_PER_REFINERY * self.refineries.max(1)
    }

    /// Whether the house should be defending what it holds rather than looking
    /// for the enemy.
    ///
    /// The reading is two ratios and no constants, because both the number of
    /// houses and the number of superweapons a mod hands them are the mod's
    /// decision:
    ///
    /// - **their coverage** — strike sites per enemy house. Twenty-nine sites
    ///   against twenty-nine houses is one each, which is exactly the same war
    ///   as one site against one house, and a fixed threshold cannot tell those
    ///   apart.
    /// - **ours** — strike sites per yard, which is the only denominator the
    ///   house owns.
    ///
    /// A house that cannot be out-waited does not wait. When every opponent can
    /// erase the base and this house cannot answer in kind, a defensive posture
    /// only feeds the charge: the strike lands on a base that never moved. So
    /// that one case is `Race` — push before the superweapon finishes — and
    /// everything else is `Turtle`: hold what is held, do not go looking.
    ///
    /// Read through the fog. A site that has not been seen is not counted, so
    /// an unseen enemy cannot be proved to have one; the unknown therefore
    /// resolves toward the defensive side, because a push is the step that
    /// cannot be taken back.
    pub(super) fn posture(&self) -> Posture {
        let theirs =
            f64::from(self.enemy_strike_sites as u32) / f64::from(self.enemy_houses.max(1) as u32);
        let ours = f64::from(self.own_strike_sites as u32) / f64::from(self.yards.max(1) as u32);
        if theirs >= 1.0 && ours < 1.0 {
            Posture::Race
        } else {
            Posture::Turtle
        }
    }
}

/// The weight at which a type counts as able to do a job rather than merely
/// carrying some of it. Owned by the role model, because the autoplay policy
/// has to count the same way for its own shortfall.
const ROLE_FLOOR: f64 = crate::rules::unit_role::ROLE_FLOOR;

/// What one object is worth in a fight, in hit points.
///
/// `Strength=` is the rules' own measure of what a type survives, and it is the
/// only number both sides can be scored in: a Grizzly's 300 against a
/// conscript's 100 is close to how many of the latter the former is worth.
/// Health scales it, because a damaged tank is not a tank.
///
/// A type that cannot shoot back is worth nothing here whatever it survives —
/// a refinery is ground the army stands on, not a threat it has to outfight —
/// which is also what keeps the enemy's own economy out of the comparison.
///
/// "Cannot shoot back" is a weapon in no slot, not an empty `Primary=`: a type
/// a mod arms through `Secondary=` alone still fights, and scoring it at zero
/// made the brain refuse to count it against the enemy or to spend it.
pub(super) fn fight_points(ty: &ObjectType, health_permille: u16) -> i32 {
    if !(0..8).any(|slot| ty.weapon_at(slot).is_some()) {
        return 0;
    }
    let full = ty.strength.max(0);
    let scaled = i64::from(full) * i64::from(health_permille.min(1_000)) / 1_000;
    i32::try_from(scaled).unwrap_or(i32::MAX)
}

/// The price band a unit falls in: cheap, the line unit, or the heavy end. The
/// cuts are retail's own steps — a soldier around 200, a line vehicle around
/// 900, the heavy and long-range types above that — read as the type's own
/// `Cost=` through the house's cost factors.
pub(super) fn price_tier(cost: i32) -> usize {
    if cost <= CHEAP_UNIT_CREDITS {
        0
    } else if cost <= MID_UNIT_CREDITS {
        1
    } else {
        2
    }
}

/// The price cuts between the bands a unit can fall in.
pub(super) const CHEAP_UNIT_CREDITS: i32 = 500;
pub(super) const MID_UNIT_CREDITS: i32 = 1_000;

/// The job that answers `ty`: shooting at an aircraft is its own job, and for
/// everything else it is the armour the type wears.
pub(super) fn counter(ty: &ObjectType) -> Option<Role> {
    if ty.category == ObjectCategory::Aircraft {
        return Some(Role::AntiAir);
    }
    role_against_armour(ty.armor_id())
}

impl Census {
    /// What the lines have been promised and have not paid out yet: the
    /// product on each line, plus the copies queued behind it.
    ///
    /// A build starts owing its `Cost_Of` and pays it down as it steps, so a
    /// queue costs the wallet nothing today and a budget that reads the
    /// balance alone spends the same credits twice. The queue holds
    /// `QueueEntry` types the projection does not name, so a queued copy is
    /// priced at the product it is queued behind — the answer is a floor, and
    /// a floor is what a budget wants.
    pub(super) fn committed(&self, rules: &RuleSet, obs: &Observation<'_>) -> i32 {
        let mut total = 0i32;
        for line in &self.lines {
            let Some(type_id) = line.product else {
                continue;
            };
            let Some(ty) = obs.object_type(rules, type_id) else {
                continue;
            };
            let copies = i32::try_from(line.queued)
                .unwrap_or(i32::MAX)
                .saturating_add(1);
            total = total.saturating_add(obs.cost_of(rules, ty).saturating_mul(copies));
        }
        total
    }

    /// Rebuild the ledger from this frame's projection. Called once, at the
    /// top of the brain's `think`.
    pub(super) fn refresh(&mut self, roles: &RoleTable, rules: &RuleSet, obs: &Observation<'_>) {
        self.counts = [0; 5];
        self.by_type.clear();
        self.lines = [Line::default(); 6];
        self.units.clear();
        self.enemies.clear();
        self.neutrals.clear();
        self.home_cells.clear();
        self.repair_cell = None;
        self.repair_building = None;
        self.ready.clear();
        self.harvesters = 0;
        self.refineries = 0;
        self.yards = 0;
        self.yard_makers = 0;
        self.fighters = 0;
        self.idle = 0;
        self.tiers = [[0; 3]; 5];
        self.threat = [0; ROLES.len()];
        self.able = [0; ROLES.len()];
        self.enemy_strike_sites = 0;
        self.own_strike_sites = 0;
        self.enemy_houses = obs.enemy_house_count();

        for entity in obs.own_entities() {
            let Some(ty) = obs.object_type(rules, entity.type_id) else {
                continue;
            };
            let slot = Slot::of(category_for_object(ty));
            let max_health = ty.strength.max(0);
            let health_permille = if max_health > 0 {
                let scaled = i64::from(entity.health.max(0)) * 1000 / i64::from(max_health);
                u16::try_from(scaled.clamp(0, 1000)).unwrap_or(1000)
            } else {
                0
            };
            let traits = Traits::of(rules, ty);
            let combat = traits.combat;
            let tier = if combat {
                price_tier(obs.cost_of(rules, ty))
            } else {
                0
            };

            self.counts[slot.index()] += 1;
            *self.by_type.entry(entity.type_id).or_insert(0) += 1;
            if traits.harvester {
                self.harvesters += 1;
            }
            if traits.refinery {
                self.refineries += 1;
            }
            if traits.yard {
                self.yards += 1;
                self.base_cell = Some(entity.cell);
            }
            if traits.yard_maker {
                self.yard_makers += 1;
            }
            if ty.category == ObjectCategory::Building {
                self.home_cells.push(entity.cell);
                if super::build::grants_a_strike(rules, ty) {
                    self.own_strike_sites += 1;
                }
                self.base_cell.get_or_insert(entity.cell);
                if traits.repairs_units {
                    self.repair_cell.get_or_insert(entity.cell);
                    self.repair_building.get_or_insert(entity.entity_id);
                }
            }
            let points = if combat {
                fight_points(ty, health_permille)
            } else {
                0
            };
            if combat {
                self.fighters += 1;
                // Idle: nothing to walk to and nothing to shoot at. The census
                // runs before the frame's orders, so a unit that was given something
                // last frame and is executing it is not idle — this is the unit that
                // the ladder has no entry for.
                let busy = obs.destination(entity.entity_id).is_some()
                    || obs.attack_target(entity.entity_id).is_some();
                if !busy {
                    self.idle += 1;
                }
                self.tiers[slot.index()][tier] += 1;
                let weights = roles.weights(&ty.id);
                for role in ROLES {
                    if weights.get(role) >= ROLE_FLOOR {
                        self.able[role.index()] += 1;
                    }
                }
            }

            self.units.push(Unit {
                entity_id: entity.entity_id,
                type_id: entity.type_id,
                cell: entity.cell,
                health_permille,
                points,
                mission: entity.mission,
                orderable: entity.orderable,
                hand: obs.owned_by_hand(entity.entity_id),
                slot,
                tier,
                traits,
            });
        }

        for category in [
            ProductionCategory::Building,
            ProductionCategory::Defense,
            ProductionCategory::Infantry,
            ProductionCategory::Vehicle,
            ProductionCategory::Aircraft,
            ProductionCategory::Ship,
        ] {
            let index = line_index(category);
            if let Some(line) = obs.factory_line(category) {
                self.lines[index] = Line {
                    product: line.product,
                    suspended: line.suspended,
                    queued: line.queued,
                    ready: obs.finished_product(category),
                };
            }
        }
        for building in obs.ready_buildings() {
            self.ready.push(building.type_id);
        }

        for enemy in obs.enemy_entities() {
            let Some(ty) = obs.object_type(rules, enemy.type_id) else {
                continue;
            };
            // A building is ground the army stands on, not a threat it has to
            // be composed against, and a spawn child is a round already in the
            // air (`[V3ROCKET] Spawned=yes`).
            if ty.category != ObjectCategory::Building
                && !ty.spawned
                && let Some(role) = counter(ty)
            {
                self.threat[role.index()] += 1;
            }
            // The strike sites are the one enemy building count that changes a
            // posture: a base that can be erased from across the map cannot be
            // out-waited, and a base that cannot is worth defending.
            if ty.category == ObjectCategory::Building && super::build::grants_a_strike(rules, ty) {
                self.enemy_strike_sites += 1;
            }
            self.enemies.push(enemy);
        }
        // Nobody's buildings are not threats and not targets of their own;
        // they are what the army finds in its way, and they are collected here
        // so no layer has to scan the map for them again.
        self.neutrals.extend(obs.neutral_structures());
    }

    /// The neutral building standing on `cell`, if one does.
    pub(super) fn neutral_at(&self, cell: (u16, u16)) -> Option<(u64, (u16, u16))> {
        self.neutrals
            .iter()
            .find(|building| building.cell == cell)
            .map(|building| (building.entity_id, building.cell))
    }

    /// The nearest neutral building within `cells` of `from` that `admit`
    /// takes, ties by id.
    ///
    /// `admit` is the caller's question about the building itself: whether
    /// clearing it is work worth doing is a rules question, and the ledger does
    /// not open the rules. The order is settled here so the answer does not
    /// depend on what the entity store happens to yield.
    pub(super) fn neutral_within(
        &self,
        from: (u16, u16),
        cells: i32,
        admit: impl Fn(&EnemyEntity) -> bool,
    ) -> Option<(u64, (u16, u16))> {
        self.neutrals
            .iter()
            .filter(|building| admit(building))
            .filter(|building| super::squared_distance(building.cell, from) <= cells * cells)
            .min_by_key(|building| {
                (
                    super::squared_distance(building.cell, from),
                    building.entity_id,
                )
            })
            .map(|building| (building.entity_id, building.cell))
    }

    pub(super) fn type_count(&self, type_id: InternedId) -> usize {
        self.by_type.get(&type_id).copied().unwrap_or(0)
    }

    /// The count of the types `names` lists, from the rules' own `[AI]` lists.
    pub(super) fn count_named(&self, obs: &Observation<'_>, names: &[String]) -> usize {
        names
            .iter()
            .filter_map(|name| obs.type_id(name))
            .map(|type_id| self.type_count(type_id))
            .sum()
    }

    pub(super) fn slot_count(&self, slot: Slot) -> usize {
        self.counts[slot.index()]
    }

    pub(super) fn line(&self, category: ProductionCategory) -> &Line {
        &self.lines[line_index(category)]
    }

    pub(super) fn combat_units(&self) -> impl Iterator<Item = &Unit> {
        self.units.iter().filter(|unit| unit.traits.combat)
    }

    /// What the house has to fight with: one number, in hit points, so that a
    /// decision that asks "is this enough" has something to compare against
    /// rather than a count of things that are not alike.
    pub(super) fn army_points(&self) -> i32 {
        self.units
            .iter()
            .fold(0i32, |total, unit| total.saturating_add(unit.points))
    }

    /// What the enemy has within `radius` cells of `cell`, in hit points: the
    /// force that will meet an army walking there.
    ///
    /// Types are opened here because this is the one pass that holds them; the
    /// answer is what the house can see, and an enemy it has never seen is not
    /// one it can weigh — the honest reading, and the reason the comparison
    /// below is a floor and not a promise.
    pub(super) fn enemy_points(
        &self,
        rules: &RuleSet,
        obs: &Observation<'_>,
        cell: (u16, u16),
        radius: i32,
    ) -> i32 {
        let reach = radius.saturating_mul(radius);
        self.enemies
            .iter()
            .filter(|enemy| {
                let dx = i32::from(enemy.cell.0) - i32::from(cell.0);
                let dy = i32::from(enemy.cell.1) - i32::from(cell.1);
                dx * dx + dy * dy <= reach
            })
            .filter_map(|enemy| obs.object_type(rules, enemy.type_id))
            .fold(0i32, |total, ty| {
                total.saturating_add(fight_points(ty, 1_000))
            })
    }

    /// The job gaps: what the enemy's field asks for, less what this house can
    /// already answer, floored at zero. `None` when nothing is short.
    ///
    /// Counted in units rather than summed weights: a type either can do a job
    /// or it cannot, so one IFV no longer "covers" a tank the way a weight sum
    /// let it, which is what left the army one type deep.
    pub(super) fn shortfall(&self) -> Option<[i32; ROLES.len()]> {
        let mut need = [0i32; ROLES.len()];
        let mut short = false;
        for role in ROLES {
            let gap = i32::try_from(self.threat[role.index()])
                .unwrap_or(i32::MAX)
                .saturating_sub(i32::try_from(self.able[role.index()]).unwrap_or(i32::MAX));
            if gap > 0 {
                need[role.index()] = gap;
                short = true;
            }
        }
        short.then_some(need)
    }

    /// One line of the ledger, for the log the client already writes: a report
    /// of "the AI builds nothing" has to be answerable from the log alone.
    pub(super) fn describe(&self) -> String {
        let slots: Vec<String> = SLOTS
            .iter()
            .map(|slot| format!("{}={}", slot.label(), self.slot_count(*slot)))
            .collect();
        format!(
            "{} harv={} ref={} yard={} fighters={} idle={} threat=[{}] able=[{}]",
            slots.join(" "),
            self.harvesters,
            self.refineries,
            self.yards,
            self.fighters,
            self.idle,
            self.threat
                .iter()
                .map(|count| count.to_string())
                .collect::<Vec<_>>()
                .join(","),
            self.able
                .iter()
                .map(|count| count.to_string())
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::sim::world::Simulation;

    /// The retail rival arena, needed to field real object types. The rules
    /// come from the repository's own `ini/` snapshot, so the ledger's counts
    /// are regression tested without a retail install.
    fn arena(seed: u64) -> Option<(Simulation, RuleSet)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        Some((sim, rules))
    }

    fn ledger(rules: &RuleSet, sim: &Simulation, owner: InternedId) -> Census {
        let mut census = Census::default();
        census.refresh(
            &RoleTable::build(rules),
            rules,
            &Observation::new(sim, owner),
        );
        census
    }

    /// The ledger is what a decision asks instead of six walks over the entity
    /// store, so it has to answer all of them: the five pools, the key types,
    /// and each line's occupancy.
    #[test]
    fn the_ledger_counts_the_house_by_pool_type_and_line() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("GACNST", "Americans", 16, 16, 255, &rules)
            .expect("a yard");
        sim.spawn_object("GAPOWR", "Americans", 20, 16, 255, &rules)
            .expect("a power plant");
        for cell in [(10u16, 10u16), (12, 10), (14, 10)] {
            sim.spawn_object("E1", "Americans", cell.0, cell.1, 255, &rules)
                .expect("a G.I.");
        }
        sim.spawn_object("MTNK", "Americans", 10, 14, 255, &rules)
            .expect("a Grizzly");
        sim.spawn_object("CMIN", "Americans", 14, 14, 255, &rules)
            .expect("a Chrono Miner");

        let census = ledger(&rules, &sim, owner);
        assert_eq!(
            census.slot_count(Slot::Building),
            2,
            "the yard and the plant"
        );
        assert_eq!(census.slot_count(Slot::Infantry), 3, "three G.I.s");
        assert_eq!(
            census.slot_count(Slot::Vehicle),
            2,
            "a Grizzly and a Chrono Miner"
        );
        assert_eq!(census.yards, 1, "one yard");
        assert_eq!(census.harvesters, 1, "one harvester");
        assert_eq!(census.fighters, 4, "three G.I.s and the Grizzly");
        assert!(
            census.lines.iter().all(|line| !line.busy()),
            "no factory is making anything"
        );
    }

    /// What a type is comes from the keys the type declares, and every decision
    /// reads it from here. The point is the mod case: a unit that says
    /// `Engineer=yes` is a tool and not army, one that says `Passengers=` is a
    /// carrier and not army, and one whose weapon sits only in `Secondary=` is
    /// armed — none of which the hand-rolled tests that used to live in five
    /// decisions could tell.
    #[test]
    fn capabilities_are_read_from_the_types_own_keys() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, owner);
        let of = |name: &str| {
            let ty = obs
                .type_id(name)
                .and_then(|type_id| obs.object_type(&rules, type_id))
                .unwrap_or_else(|| panic!("{name} is in the rules"));
            (ty, Traits::of(&rules, ty))
        };

        let (_, engineer) = of("ENGINEER");
        assert!(engineer.captures && !engineer.combat, "a tool, not army");
        let (_, gi) = of("E1");
        assert!(gi.combat && gi.armed, "a rifleman is army");
        assert_eq!(gi.deploy_slot, Some(1), "and digs in with its second slot");
        let (_, dog) = of("ADOG");
        assert!(dog.scout && dog.armed, "the dog scans, and bites");
        let (_, transport) = of("SHAD");
        assert!(transport.carries && !transport.combat, "a carrier");
        let (_, tank) = of("MTNK");
        assert!(tank.combat && !tank.captures, "a tank");
    }

    /// A hero is the type the rules allow one of, which is `BuildLimit=1` and
    /// not a name: `[TANY]` and `[BORIS]` both carry it, and a mod's hero is
    /// covered by the same query.
    #[test]
    fn a_type_the_rules_allow_one_of_is_irreplaceable() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, owner);
        let of = |name: &str| {
            let ty = obs
                .type_id(name)
                .and_then(|type_id| obs.object_type(&rules, type_id))
                .unwrap_or_else(|| panic!("{name} is in the rules"));
            (ty, Traits::of(&rules, ty))
        };

        let (ty, tanya) = of("TANY");
        assert_eq!(ty.build_limit, 1, "the retail cap");
        assert!(tanya.irreplaceable, "and the trait follows it");
        let (ty, boris) = of("BORIS");
        assert_eq!(ty.build_limit, 1);
        assert!(boris.irreplaceable);

        // The ordinary line is not: a rifleman is bought again.
        let (_, gi) = of("E1");
        assert!(!gi.irreplaceable, "a G.I. is replaceable");
        let (_, tank) = of("MTNK");
        assert!(!tank.irreplaceable, "and so is a Grizzly");
    }

    /// A fight is weighed in what the other side can shoot back with, so a
    /// building with no gun is worth nothing in it — the enemy's refinery is
    /// ground the army walks over, not a defender it has to outfight.
    #[test]
    fn a_type_that_cannot_shoot_back_is_worth_nothing_in_a_fight() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, owner);
        let by_name = |name: &str| {
            obs.type_id(name)
                .and_then(|type_id| obs.object_type(&rules, type_id))
                .unwrap_or_else(|| panic!("{name} is in the rules"))
        };
        assert_eq!(
            fight_points(by_name("NAREFN"), 1_000),
            0,
            "a refinery has no gun"
        );
        let tank = fight_points(by_name("MTNK"), 1_000);
        assert!(tank > 0, "a Grizzly does: {tank}");
        assert_eq!(
            fight_points(by_name("MTNK"), 500),
            tank / 2,
            "half health is half the tank"
        );
    }

    /// What the army is short of is read off the jobs the enemy's own types
    /// ask for: a tank by something that bites armour, a conscript by
    /// something that bites soldiers, and neither by reach.
    #[test]
    fn the_jobs_the_army_is_short_of_are_the_enemys_own() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [("HTNK", (6u16, 6u16)), ("E2", (8, 8))] {
            sim.spawn_object(name, "Russians", cell.0, cell.1, 64, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        let census = ledger(&rules, &sim, owner);
        let need = census
            .shortfall()
            .expect("an army of nothing is short of it all");

        assert!(
            need[Role::AntiVehicle.index()] > 0,
            "a Rhino tank: {need:?}"
        );
        assert!(
            need[Role::AntiInfantry.index()] > 0,
            "a conscript: {need:?}"
        );
        assert_eq!(need[Role::AntiAir.index()], 0, "nothing in the air");
        assert_eq!(need[Role::AntiStructure.index()], 0, "no buildings");
        assert_eq!(need[Role::Artillery.index()], 0, "nothing asks for reach");
    }

    /// A round already in the air is not an aircraft. `[V3ROCKET]` sits in the
    /// aircraft table with `Spawned=yes`, and reading it as air asked the army
    /// for anti-air: the measured match answered two rockets with ten IFVs
    /// while the bunkers and Tesla coils that took the base went unanswered.
    #[test]
    fn a_fired_rocket_does_not_ask_the_army_for_anti_air() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        for (name, cell) in [("V3ROCKET", (6u16, 6u16)), ("E2", (8, 8))] {
            sim.spawn_object(name, "Russians", cell.0, cell.1, 64, &rules)
                .unwrap_or_else(|| panic!("{name}"));
        }
        let census = ledger(&rules, &sim, owner);
        let need = census
            .shortfall()
            .expect("an army of nothing is short of it all");
        assert_eq!(
            census.threat[Role::AntiAir.index()],
            0,
            "the rocket is not air"
        );
        assert!(
            need[Role::AntiInfantry.index()] > 0,
            "the conscript still counts"
        );
    }

    /// A building is ground the army stands on, not a threat to compose
    /// against, so a base with nothing but structures asks for nothing. A
    /// refinery is not a fair sample: it comes with a harvester of its own,
    /// and a harvester is a vehicle.
    #[test]
    fn a_base_of_buildings_asks_the_army_for_nothing() {
        let Some((mut sim, rules)) = arena(0xA17) else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        sim.spawn_object("NAPOWR", "Russians", 8, 8, 255, &rules)
            .expect("an enemy power plant");
        let census = ledger(&rules, &sim, owner);
        assert!(census.threat.iter().all(|count| *count == 0));
        assert!(census.shortfall().is_none());
    }

    /// An aircraft carries the tag no ground layer can use: it has no cell to
    /// stand on. The guard step stands units on a cell, so the tag has to be
    /// readable or that step hands a plane a `Guard`, which replaces the
    /// airfield's rearm-and-return and sends it out on the racks it had.
    #[test]
    fn an_aircraft_is_marked_as_having_no_ground_post() {
        let Some((_, rules)) = arena(0xA17) else {
            return;
        };
        for name in ["ORCA", "BEAG", "AMRADR"] {
            let ty = rules.object(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(
                Traits::of(&rules, ty).aircraft,
                ty.category == ObjectCategory::Aircraft,
                "{name}: the tag follows the category"
            );
        }
        // And something that walks is not tagged, so the layer that skips
        // aircraft does not skip half the army with it.
        let tank = rules.object("MTNK").expect("the Allied tank");
        assert!(!Traits::of(&rules, tank).aircraft);
    }

    /// The three weapon behaviours the wishlist names, read off the retail
    /// keys rather than off the types' names.
    #[test]
    fn a_weapon_that_spends_its_unit_or_its_neighbours_is_marked() {
        let Some((_, rules)) = arena(0xA17) else {
            return;
        };
        let of = |name: &str| {
            let ty = rules.object(name).unwrap_or_else(|| panic!("{name}"));
            Traits::of(&rules, ty)
        };

        // `Suicide=yes` on the primary: firing costs the unit.
        assert!(of("TERROR").suicide, "[TERROR] TerrorBomb is Suicide=yes");
        assert!(of("DTRUCK").suicide, "[DTRUCK] Demobomb is Suicide=yes");
        assert!(!of("E1").suicide, "a rifleman's M60 does not spend him");

        // `CellSpread=` above 1 reaches the neighbouring cells.
        assert!(of("DTRUCK").splash, "DemobombWH CellSpread=8");
        assert!(
            of("DESO").splash,
            "RadEruptionWarhead CellSpread=10 on the secondary"
        );
        assert!(!of("E1").splash, "a rifle hits one cell");

        // `Temporal=yes` is a channel, not a shot.
        assert!(
            of("CLEG").channels,
            "[CLEG] NeutronRifle carries ChronoBeam, which is Temporal=yes"
        );
        assert!(!of("E1").channels, "a rifle does not transform its target");
    }

    /// A spawned type is not the house's to command. `[PDPLANE]` is the
    /// paradrop's cargo plane, and retail says so itself:
    /// `Spawned=yes ; Created by another object and therefore not player
    /// controllable`. It also carries a dummy `Primary=`, so read as army it
    /// was handed a combat order in the middle of the drop and the drop never
    /// landed.
    #[test]
    fn the_cargo_plane_is_neither_army_nor_the_houses_to_order() {
        let Some((_, rules)) = arena(0xA17) else {
            return;
        };
        let ty = rules.object("PDPLANE").expect("the paradrop's plane");
        let traits = Traits::of(&rules, ty);
        assert!(traits.spawned, "`Spawned=yes` is the controllability tag");
        assert!(!traits.combat, "a spawned object is not army");
        let unit = Unit {
            entity_id: 1,
            type_id: InternedId::default(),
            cell: (10, 10),
            health_permille: 1_000,
            points: 0,
            mission: MissionId::from_known(crate::sim::mission::MissionType::Move),
            orderable: true,
            hand: false,
            slot: Slot::Aircraft,
            tier: 0,
            traits,
        };
        assert!(
            !unit.controllable(),
            "the world would take the order, and the brain still may not give it"
        );
    }

    /// The clearing sweep asks the ledger for a neutral building, and what may
    /// be cleared is the caller's question: a house the riflemen can garrison
    /// and a derrick the engineer can capture are worth more standing than the
    /// walk around them costs.
    #[test]
    fn a_neutral_building_a_click_can_use_is_not_the_one_cleared() {
        let mut census = Census::default();
        for (id, cell) in [(7u64, (10u16, 10u16)), (9, (12, 10))] {
            census.neutrals.push(EnemyEntity {
                entity_id: id,
                type_id: InternedId::default(),
                cell,
            });
        }
        let only = |wanted: u64| move |building: &EnemyEntity| building.entity_id == wanted;
        assert_eq!(
            census.neutral_within((10, 10), 3, only(9)),
            Some((9, (12, 10))),
            "the admitted building is the answer, not merely the nearest one"
        );
        assert_eq!(
            census.neutral_within((10, 10), 1, only(9)),
            None,
            "and the distance still applies to it"
        );
    }
}
