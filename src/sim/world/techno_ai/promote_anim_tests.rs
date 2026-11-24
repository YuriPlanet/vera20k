//! Phobos `TechnoClass_AI_PromoteAnim @ 0x6FA07A` through the production
//! rank-crossing step: `TechnoExt::ShowPromoteAnim`
//! (`Ext/Techno/Body.Visuals.cpp:863-874`) with the type lists replacing the
//! `[AudioVisual]` tiers.
use super::*;
use crate::rules::art_data::ArtRegistry;
use crate::sim::anim_class::AnimId;

fn scene_rules() -> RuleSet {
    let mut rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[General]\nVeteranCombat=1.0\n\
         [Countries]\n0=Americans\n1=Russians\n\
         [AudioVisual]\nPromote.VeteranAnimation=GLOBVET\nPromote.EliteAnimation=GLOBELI\n\
         [InfantryTypes]\n0=HERO\n1=PLAIN\n\
         [HERO]\nStrength=100\nSpeed=4\nPromote.VeteranAnimation=HEROVET\n\
         [PLAIN]\nStrength=100\nSpeed=4\n\
         [Animations]\n0=GLOBVET\n1=GLOBELI\n2=HEROVET\n",
    ))
    .expect("promote rules");
    rules.install_art_data(ArtRegistry::empty());
    rules
}

fn anim_ids(sim: &Simulation) -> Vec<AnimId> {
    sim.substrate.anims.iter().map(|(id, _)| *id).collect()
}

fn played_since(sim: &Simulation, before: &[AnimId]) -> Vec<(String, Option<u64>)> {
    let mut played: Vec<(String, Option<u64>)> = sim
        .substrate
        .anims
        .iter()
        .filter(|(id, _)| !before.contains(id))
        .map(|(_, anim)| {
            (
                sim.interner.resolve(anim.type_id()).to_owned(),
                anim.owner_entity(),
            )
        })
        .collect();
    played.sort();
    played
}

#[test]
fn promote_animation_type_lists_replace_the_global_tiers() {
    let rules = scene_rules();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 31);
    let hero = sim
        .spawn_object("HERO", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let plain = sim
        .spawn_object("PLAIN", "Americans", 12, 10, 0, &rules)
        .unwrap();
    let american = sim.interner.intern("Americans");
    sim.object_ai_stage(Some(&rules)); // the first sample caches silently
    assert!(
        sim.substrate.anims.iter().next().is_none(),
        "a first rank sample plays no animation"
    );

    // Veteran: the type's list replaces the global one; a type without a list
    // keeps the `[AudioVisual]` veteran list.
    let before = anim_ids(&sim);
    for id in [hero, plain] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .veterancy
            .set_veteran();
    }
    sim.object_ai_stage(Some(&rules));
    let mut expected = vec![
        ("GLOBVET".to_owned(), Some(plain)),
        ("HEROVET".to_owned(), Some(hero)),
    ];
    expected.sort();
    assert_eq!(played_since(&sim, &before), expected);
    for (anim_id, anim) in sim.substrate.anims.iter() {
        if before.contains(anim_id) {
            continue;
        }
        let owner = anim
            .owner_entity()
            .expect("CreateRandomAnim's owner object");
        let entity = sim.substrate.entities.get(owner).unwrap();
        let expected = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        let absolute = sim.anim_absolute_coord(*anim_id).unwrap();
        assert_eq!(
            [absolute.x, absolute.y, absolute.z],
            [expected.x, expected.y, expected.z],
            "the anim sits at the promoted object's GetCoords"
        );
        assert_eq!(anim.owner_house(), Some(american));
        assert_eq!(
            anim.invoker(),
            Some(owner),
            "CreateRandomAnim's invoker is the promoted object"
        );
        assert_eq!(anim.invoker_house(), Some(american));
    }

    // Elite: `[HERO]` has no elite list, so both units take the global elite
    // list — the veteran list is never a fallback for the elite tier.
    let before = anim_ids(&sim);
    for id in [hero, plain] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .veterancy
            .set_elite();
    }
    sim.object_ai_stage(Some(&rules));
    let mut expected = vec![
        ("GLOBELI".to_owned(), Some(plain)),
        ("GLOBELI".to_owned(), Some(hero)),
    ];
    expected.sort();
    assert_eq!(played_since(&sim, &before), expected);

    // The native `else if` fires the elite list for any non-Veteran tier,
    // including a demotion to Rookie.
    let before = anim_ids(&sim);
    sim.substrate
        .entities
        .get_mut(hero)
        .unwrap()
        .veterancy
        .set_rookie();
    sim.object_ai_stage(Some(&rules));
    assert_eq!(
        played_since(&sim, &before),
        vec![("GLOBELI".to_owned(), Some(hero))]
    );
}
