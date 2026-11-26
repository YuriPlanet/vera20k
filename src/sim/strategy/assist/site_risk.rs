//! The destination is part of the cost of a specialist task. A legal plot or
//! Capture click is not evidence that the resulting asset can survive.
//! Only currently visible hostiles, own available ground fighters and rules
//! are inputs; absence of a sighting is not a guarantee of safety.
use crate::rules::{object_type::{ObjectCategory,ObjectType}, ruleset::RuleSet};
use crate::sim::control::{Goal, brain::Outbox, intent::Order, observation::{EnemyEntity,Observation}};
use super::{AssistBrain, census::{Census,Slot}, safety::{self,Envelope}, siege_counter, tactics, advance::cell_distance};

type Cell = (u16,u16);

#[derive(Clone,Copy,Debug)]
struct Threat { cell: Cell, reach: i32, mobile: bool }

/// Evaluate the weapon against the *future* building's armour, even when
/// the capturable building currently belongs to the same house as the gun.
/// The current occupant's weapon is used for Gunners. No hidden type query.
fn reach_against(rules:&RuleSet, obs:&Observation<'_>, e:&EnemyEntity, victim:&ObjectType)->i32 {
    let Some(ty)=obs.object_type(rules,e.type_id) else {return 0;};
    if ty.category==ObjectCategory::Building && !obs.visible_enemy_building_operational(rules,e.entity_id) {return 0;}
    let reach = |w:&crate::rules::weapon_type::WeaponType| {
        if !w.never_use && (w.damage>0 || w.spawner)
            && rules.weapon_projectile(w).is_some_and(|p|p.ag)
            && siege_counter::effective_ground_warhead(rules,ty,w,victim) {
            siege_counter::max_range_cells(w)
        } else {0}
    };
    if ty.gunner {obs.active_gunner_weapon(rules,e.entity_id).map_or(0,reach)}
    else {(0..ty.weapon_record_count()).filter_map(|slot|rules.weapon(ty.weapon_at(slot)?)).map(reach).max().unwrap_or(0)}
}

fn threats(rules:&RuleSet,obs:&Observation<'_>,target:&ObjectType,ignored:Option<u64>,visible:&[EnemyEntity])->Vec<Threat> {
    let (w,h)=target.foundation.dimensions();
    // Conservative footprint margin (centre/origin conventions differ for
    // native building coordinates). Never treat a point just outside a gun
    // as proof the entire building will be outside its fire.
    let margin=i32::from(w.max(h)).clamp(1,8);
    visible.iter().filter(|e|Some(e.entity_id)!=ignored).filter_map(|e| {
        let ty=obs.object_type(rules,e.type_id)?;
        let reach=reach_against(rules,obs,e,target);
        let mobile=ty.category!=ObjectCategory::Building;
        // Enemy engineers can retake a newly captured site. Treat them as
        // nearby soft pressure, not an imaginary gun with a lethal range.
        (reach>0 || (mobile && ty.engineer)).then_some(Threat {cell:e.cell,
            reach:if reach>0 {reach.saturating_add(margin)} else {0},mobile})
    }).collect()
}

fn pressure(threats:&[Threat],cell:Cell)->(bool,usize) {
    let mut hard=false; let mut soft=0;
    for t in threats {
        let d=cell_distance(t.cell,cell);
        if t.reach>0 && d<=t.reach {hard=true;}
        else if t.mobile && d<=t.reach.saturating_add(5).max(6) {soft+=1;}
    }
    (hard,soft)
}

fn acceptable(hard:bool,soft:usize,support:usize)->bool {
    // Direct coverage is a veto, even with an escort. Nearby mobile pressure
    // is graded: allow a modest risk with real local ground support.
    !hard && (soft==0 || support>=soft.saturating_add(1))
}

fn supports(census:&Census,cell:Cell)->usize {
    census.units.iter().filter(|u|census.available(u) && u.health_permille>=600
        && matches!(u.slot,Slot::Infantry|Slot::Vehicle) && !u.traits.aircraft
        && cell_distance(u.cell,cell)<=10).count()
}

pub(super) struct YardSurvey { guns:Vec<Envelope>, threats:Vec<Threat> }
impl YardSurvey {
    pub(super) fn read(rules:&RuleSet,obs:&Observation<'_>,yard:&ObjectType,maker:Option<u64>)->Self {
        let visible:Vec<_>=obs.visible_enemy_entities().collect();
        let guns=maker.map_or_else(Vec::new,|id|safety::envelopes(rules,obs,id,&visible,true));
        Self {guns,threats:threats(rules,obs,yard,None,&visible)}
    }
    pub(super) fn route(&self,from:Cell,to:Cell)->bool {
        safety::safe_leg(&self.guns,from,to,true)
    }
    pub(super) fn site(&self,census:&Census,from:Option<Cell>,cell:Cell)->bool {
        let (hard,soft)=pressure(&self.threats,cell);
        let direct=hard || self.guns.iter().any(|g|cell_distance(g.at,cell)<=g.reach);
        !direct && from.is_none_or(|p|self.route(p,cell))
            && (soft==0 || acceptable(false,soft,supports(census,cell)))
    }
    fn exposure(&self,cell:Cell)->usize {
        self.guns.iter().filter(|g|cell_distance(g.at,cell)<=g.reach).count()
    }
    #[cfg(all(test, tests_sim_p_s))]
    pub(super) fn terrain_only_for_test()->Self {Self {guns:Vec::new(),threats:Vec::new()}}
}

// Direct checks apply to all capture entrances, including bridge repairs and
// ferried engineers. A bridge repair/own repair is not a prospective prize.
fn prize_type<'r>(rules:&'r RuleSet,obs:&Observation<'_>,target:u64)->Option<&'r ObjectType>{
    if obs.owns_object(target) {return None;}
    obs.entity_type(rules,target).filter(|ty|ty.category==ObjectCategory::Building && ty.capturable && !ty.bridge_repair_hut)
}

pub(super) fn capture_direct_safe(rules:&RuleSet,obs:&Observation<'_>,target:u64,cell:Cell,visible:&[EnemyEntity])->bool {
    let Some(ty)=prize_type(rules,obs,target) else {return true;};
    !pressure(&threats(rules,obs,ty,Some(target),visible),cell).0
}

pub(super) fn capture_site_safe(census:&Census,rules:&RuleSet,obs:&Observation<'_>,target:u64,visible:&[EnemyEntity])->bool {
    let Some(cell)=obs.observed_object_cell(target) else {return false;};
    let Some(ty)=prize_type(rules,obs,target) else {return true;};
    let (hard,soft)=pressure(&threats(rules,obs,ty,Some(target),visible),cell);
    !hard && (soft==0 || acceptable(false,soft,supports(census,cell)))
}

/// A last check on a durable expansion appointment as well as fresh orders.
/// This also runs when only production is enabled. Keep recall/foreign-tech
/// ownership, but invalidate the unsafe plot so next frame can find another.
pub(super) fn review_makers(brain:&mut AssistBrain,rules:&RuleSet,obs:&Observation<'_>,out:&mut Outbox){
    if !obs.production_enabled() {return;}
    let makers:Vec<_>=brain.census.units.iter().filter(|u|u.traits.yard_maker && u.controllable()).copied().collect();
    brain.maker_risk_retry.retain(|id,_|makers.iter().any(|u|u.entity_id==*id));
    if makers.is_empty(){return;}
    for unit in makers {
        let id=unit.entity_id;
        let Some(ty)=obs.entity_type(rules,id) else {continue;};
        let Some(yard)=ty.deploys_into.as_deref().and_then(|name|rules.object(name)) else {continue;};
        let survey=YardSurvey::read(rules,obs,yard,Some(id));
        let journey=brain.maker_journeys.get(&id).map(|j|j.0);
        let proposed=out.orders.iter().find(|o|o.actor==id).map(|o|o.goal);
        let native=obs.destination(id);
        let exposed=survey.exposure(unit.cell)>0;
        let plot_bad=journey.is_some_and(|cell|!survey.site(&brain.census,Some(unit.cell),cell));
        let move_bad=|cell| !survey.route(unit.cell,cell) || (plot_bad && journey==Some(cell))
            || (brain.blocked_expansion_sites.get(&cell).is_some_and(|end|*end>obs.tick())
                && !survey.site(&brain.census,Some(unit.cell),cell))
            || (exposed && cell==unit.cell);
        let native_bad=native.is_some_and(move_bad);
        let bad=proposed.is_some_and(|g|match g {
            Goal::Deploy=>!survey.site(&brain.census,Some(unit.cell),unit.cell),
            Goal::Move {cell}=>move_bad(cell),
            Goal::Stop=>false,
            _=>true,
        });
        let safe_next=!bad && matches!(proposed,Some(Goal::Move {cell}) if cell!=unit.cell);
        let safe_native=!native_bad && native.is_some_and(|cell|cell!=unit.cell);
        if !plot_bad && !native_bad && !bad && (!exposed || safe_next || safe_native) {continue;}
        if let Some(site)=journey.filter(|_|plot_bad){
            brain.blocked_expansion_sites.insert(site,obs.tick().saturating_add(600));
        }
        if plot_bad || bad || native_bad {
            brain.expansion_probe=None;
            brain.maker_journeys.remove(&id);
            brain.maker_obstructed.remove(&id);
            brain.yard_commit.remove(&id);
        }
        // A safe new escape is preferable to the old dangerous appointment.
        if safe_next {continue;}
        brain.tracked.cancel_for_safety(id);
        out.orders.retain(|o|o.actor!=id);
        out.attachments.retain(|a|a.actor!=id);
        let due=brain.maker_risk_retry.get(&id).is_none_or(|at|obs.tick()>=*at);
        if due {
            brain.maker_risk_retry.insert(id,obs.tick().saturating_add(60));
            log::info!("auto play: expansion_risk_veto tick={} actor={id} plot={journey:?} native={native:?} exposed={exposed}",obs.tick());
        }
        if safe_native {continue;}
        // A safe idle maker waits for a new plot, instead of wandering.
        // An exposed maker may take a genuinely outward step, even if it
        // takes several steps to leave a long-range weapon's envelope.
        let rear=brain.census.rally.or(brain.census.base_cell).unwrap_or(unit.cell);
        let mut best=None;
        if exposed && due {
            for ring in [2i32,4,6,8] {
                for (dx,dy) in [(ring,0),(-ring,0),(0,ring),(0,-ring),(ring,ring),(ring,-ring),(-ring,ring),(-ring,-ring)] {
                    let (Ok(x),Ok(y))=(u16::try_from(i32::from(unit.cell.0)+dx),u16::try_from(i32::from(unit.cell.1)+dy)) else {continue;};
                    let cell=(x,y);
                    if !obs.is_cell_in_playfield(x,y) || (obs.has_ground_grid() && !obs.is_ground_walkable(x,y))
                        || !obs.can_reach(ty.movement_zone,unit.cell,cell) || !survey.route(unit.cell,cell) {continue;}
                    let score=(survey.exposure(cell),cell_distance(cell,rear),-ring,cell);
                    if best.is_none_or(|v|score<v){best=Some(score);}
                }
            }
        }
        // Cancel an actively dangerous native destination without waiting for
        // the diagnostic cooldown. No forced deploy, teleport or queue edit.
        let replacement=best.map(|(_,_,_,cell)|Goal::Move {cell}).or_else(||native_bad.then_some(Goal::Stop));
        if let Some(goal)=replacement {out.orders.insert(0,Order {actor:id,goal});}
    }
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    #[test]
    fn footprint_fire_is_hard_but_nearby_mobile_pressure_is_graded(){
        let t=[Threat {cell:(20,20),reach:8,mobile:true}];
        assert_eq!(pressure(&t,(28,20)),(true,0));
        assert_eq!(pressure(&t,(30,20)),(false,1));
        assert!(!acceptable(true,0,99));
        assert!(!acceptable(false,1,0));
        assert!(acceptable(false,1,2));
        assert!(acceptable(false,0,0));
    }

    fn scene()->(crate::sim::world::Simulation,RuleSet) {
        let rules=RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[VehicleTypes]\n0=MCV\n[InfantryTypes]\n0=ENG\n[BuildingTypes]\n0=YARD\n1=TOWER\n2=PRIZE\n\
             [AI]\nBuildConst=YARD\n[MCV]\nStrength=500\nSpeed=5\nArmor=heavy\nDeploysInto=YARD\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
             [ENG]\nStrength=100\nSpeed=5\nEngineer=yes\nArmor=none\n\
             Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
             [YARD]\nStrength=1000\nFoundation=3x3\nFactory=BuildingType\nArmor=concrete\n\
             [PRIZE]\nStrength=500\nFoundation=1x1\nCapturable=yes\nArmor=concrete\n\
             [TOWER]\nStrength=1000\nFoundation=1x1\nPrimary=Shell\nPowered=no\nIsBaseDefense=yes\n\
             [Shell]\nDamage=100\nROF=30\nRange=6\nWarhead=StructureOnly\nProjectile=Bullet\n\
             [Bullet]\nAG=yes\nAA=no\nInviso=yes\n\
             [StructureOnly]\nVerses=0%,0%,0%,100%,100%,100%,100%,100%,100%,100%,100%\n"
        )).expect("site risk fixture rules");
        let mut sim=crate::sim::arena_fixture::rival_arena_known(&rules,0xA480);
        sim.intern_rule_type_ids(&rules);sim.resolve_type_handles(&rules);
        (sim,rules)
    }

    #[test]
    fn a_safe_engineer_route_does_not_prove_the_captured_building_can_survive() {
        let (mut sim,rules)=scene();
        let owner=sim.interner.intern("Americans");
        let engineer=sim.spawn_object("ENG","Americans",5,20,255,&rules).unwrap();
        let prize=sim.spawn_object("PRIZE","Russians",20,20,255,&rules).unwrap();
        sim.spawn_object("TOWER","Russians",21,18,255,&rules).unwrap();
        let obs=Observation::new(&sim,owner);
        let visible:Vec<_>=obs.visible_enemy_entities().collect();
        let guns=safety::envelopes(&rules,&obs,engineer,&visible,true);
        assert!(safety::safe_leg(&guns,(5,20),(20,20),false));
        assert!(!capture_direct_safe(&rules,&obs,prize,(20,20),&visible));
        assert!(!safety::specialist_approach_safe(&rules,&obs,engineer,prize));
    }

    #[test]
    fn a_new_tower_invalidates_an_active_expansion_appointment() {
        let (mut sim,rules)=scene();
        let owner=sim.interner.intern("Americans");
        let maker=sim.spawn_object("MCV","Americans",5,20,255,&rules).unwrap();
        let yard=rules.object("YARD").unwrap();
        let mut brain=AssistBrain::new(0xA481);
        let site=(20,20);
        {
            let obs=Observation::new(&sim,owner);
            brain.rebuild_census(&rules,&obs);
            assert!(brain.census.units.iter().any(|u|u.entity_id==maker && u.traits.yard_maker));
            assert!(YardSurvey::read(&rules,&obs,yard,Some(maker)).site(&brain.census,Some((5,20)),site));
        }
        assert!(sim.apply_command("Americans",&Goal::Move {cell:site}.command(maker),Some(&rules)));
        brain.maker_journeys.insert(maker,(site,(5,20),0));
        sim.spawn_object("TOWER","Russians",21,18,255,&rules).unwrap();
        let obs=Observation::new(&sim,owner);
        brain.rebuild_census(&rules,&obs);
        let mut out=Outbox::default();
        review_makers(&mut brain,&rules,&obs,&mut out);
        assert!(!brain.maker_journeys.contains_key(&maker));
        assert!(brain.blocked_expansion_sites.contains_key(&site));
        assert!(out.orders.iter().any(|o|o.actor==maker && o.goal==Goal::Stop));
    }
}
