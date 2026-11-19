//! Ares DisplayCredits10044490 and Tactical_Draw_FlyingStrings10064430.
//! The client owns copied text and draw lifetimes; it never changes the wallet
//! or retains live simulation pointers. Glyphs use the existing GAME.FNT atlas.

use crate::{
    render::{batch::SpriteInstance, bit_font::BitFont},
    rules::ruleset::RuleSet,
    sim::{
        cloak_disguise::VisualCharacterQuery, components::DriveCoord, flying_text::BountyText,
        projectile::ProjectileCoord, world::Simulation,
    },
};

struct CreditText {
    coord: ProjectileCoord,
    /// `FlyingStrings::Item::PixelOffset`, applied after the centred anchor.
    pixel_offset: [i32; 2],
    text: String,
    rgb: [u8; 3],
    life: i32,
    /// A composite drew it, so the next one ages it first.
    drawn: bool,
}

#[derive(Default)]
pub(crate) struct FlyingCredits {
    live: Vec<CreditText>,
}

impl FlyingCredits {
    ///10044497..1004459D has already consumed the pending sum/deadline.
    /// Test visibility at GetCoords BEFORE adding the text's height lift.
    pub(crate) fn admit(&mut self, sim: &Simulation, rules: &RuleSet, event: BountyText) {
        let Some(viewer) = sim.session().current_house() else {
            return;
        };
        let Some(terrain) = sim.resolved_terrain() else {
            return;
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
        if crate::sim::vision::point_is_shrouded(
            sim.fog(),
            &cells,
            viewer,
            DriveCoord {
                x: event.coord.x,
                y: event.coord.y,
                z: event.coord.z,
            },
        ) {
            return;
        }
        let actor = match event.entity_id {
            Some(id) => match sim.entities().get(id) {
                Some(actor) => Some(actor),
                None => return,
            },
            // Phobos `FlyingStrings::AddMoneyString` with `pSource = nullptr`
            // (warhead `TransactMoney`): no anchor object and no
            // visual-character test, only the coordinates and the owner.
            None => None,
        };
        // Phobos `FlyingStrings::AddMoneyString`: a non-All
        // `DisplaySoylentToHouses` compares the paying House with the local
        // player before the source's visual-character test.
        #[cfg(feature = "phobos")]
        if let Some(flags) = event.display_to {
            let Some(owner) = event
                .display_owner
                .or_else(|| actor.map(|actor| actor.owner()))
            else {
                return;
            };
            if !crate::sim::combat::can_target_house(
                flags,
                owner,
                viewer,
                sim.houses(),
                sim.house_alliances(),
                sim.interner(),
            ) {
                return;
            }
        }
        if let Some(actor) = actor {
            let observer = super::instances::observer_draw_context(
                sim,
                actor,
                Some(sim.interner().resolve(viewer)),
                Some(viewer),
                Some(rules),
            );
            if actor.visual_character(
                observer.cloaking_stages,
                observer.invisible,
                actor.discovery.owned_by_current_house(),
                VisualCharacterQuery::screen(
                    true,
                    observer.observer_present,
                    observer.detects_cloak,
                    observer.owner_is_mutually_allied,
                    observer.is_campaign,
                    false,
                ),
            ) == 5
            {
                return;
            }
        }
        //100444ED..10044527: native format is "%c$%d" with wrapping ABS.
        // Keep the i32::MIN representation rather than panicking in debug.
        let positive = event.amount > 0;
        let text = format!(
            "{}${}",
            if positive { '+' } else { '-' },
            event.amount.wrapping_abs()
        );
        let mut coord = event.coord;
        coord.z = coord.z.wrapping_add(event.z_offset);
        self.live.push(CreditText {
            coord,
            pixel_offset: event.pixel_offset,
            text,
            // Native RGB literals1009E814/1009E810, packed by10015830.
            rgb: if positive { [0, 255, 0] } else { [255, 0, 0] },
            // AddFlyingString100643D9 stores70+5; draw decrements after use.
            life: 75,
            drawn: false,
        });
    }

    pub(crate) fn clear_on_load(&mut self) {
        self.live.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.live.is_empty()
    }

    /// One composite's aging. 10064514 ages each record after drawing it;
    /// here a composite first ages the records an earlier one drew, so a draw
    /// between composites shows the same labels. Offscreen records age too.
    pub(crate) fn step(&mut self) {
        for record in &mut self.live {
            if record.drawn {
                record.life -= 1;
            }
            record.drawn = true;
        }
        self.live.retain(|record| record.life >= 0);
    }

    ///10064430..10064514: append order, no fade; only the final70 draws
    /// rise one pixel each.
    pub(crate) fn build_instances(
        &self,
        font: &BitFont,
        camera: [f32; 2],
        zoom: f32,
        // The battlefield in frame pixels: left, top, width, height.
        viewport: [u32; 4],
    ) -> Vec<SpriteInstance> {
        let mut out = Vec::new();
        for record in &self.live {
            let (x, y) = crate::util::lepton::absolute_leptons_to_screen(
                record.coord.x,
                record.coord.y,
                record.coord.z,
            );
            // The world attachment is already composited through wheel zoom
            // before Tactical's text pass. Keep glyph size/rise in frame pixels
            // while placing the anchor on the same zoomed map point.
            let x = (x - camera[0]) * zoom;
            let y = (y - camera[1]) * zoom;
            let [left, top, width, height] = viewport.map(|side| side as f32);
            if x >= left && y >= top && x < left + width && y < top + height {
                let rise = (record.life - 70).min(0);
                let left =
                    x + record.pixel_offset[0] as f32 - (font.text_width(&record.text) / 2) as f32;
                let rgb = crate::render::native_surface_format::RGB565;
                let tint = rgb
                    .unpack_rgb8(rgb.pack_rgb8(record.rgb))
                    .map(|v| f32::from(v) / 255.0);
                out.extend(font.build_text(
                    &record.text,
                    left,
                    y + record.pixel_offset[1] as f32 + rise as f32,
                    1.0,
                    0.0,
                    tint,
                    camera,
                ));
            }
        }
        out
    }
}

#[cfg(all(test, tests_app_presentation))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn bounty_text_obeys_shroud_and_ages_once_per_composite() {
        let _ares = crate::rules::ares_switch::read_ares_way(true);
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=100\nSight=0\n",
        ))
        .unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        let actor = sim
            .spawn_object("TANK", "Americans", 16, 16, 64, &rules)
            .unwrap();
        let viewer = sim.entities().get(actor).unwrap().owner();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(viewer));
        let event = BountyText {
            entity_id: Some(actor),
            amount: 30,
            coord: ProjectileCoord {
                x: 12 * 256,
                y: 12 * 256,
                z: 0,
            },
            z_offset: 256,
            #[cfg(feature = "phobos")]
            display_to: None,
            #[cfg(feature = "phobos")]
            display_owner: None,
            pixel_offset: [0, 0],
        };
        let mut credits = FlyingCredits::default();
        credits.admit(&sim, &rules, event);
        assert!(credits.live.is_empty(), "unmapped origin refuses a label");
        sim.fog_mut_for_test().reveal_all_for_owner(viewer);
        credits.admit(&sim, &rules, event);
        assert_eq!(credits.live[0].text, "+$30");
        assert_eq!(credits.live[0].rgb, [0, 255, 0]);
        credits.admit(
            &sim,
            &rules,
            BountyText {
                amount: -7,
                ..event
            },
        );
        assert_eq!(credits.live[1].text, "-$7");
        assert_eq!(credits.live[1].rgb, [255, 0, 0]);
        let font = crate::render::bit_font::tests::make_test_font(&[], 4);
        let camera = [-200.0, 0.0];
        let composite = |credits: &mut FlyingCredits, camera, view| {
            credits.step();
            credits.build_instances(&font, camera, 1.0, view)
        };
        let first = composite(&mut credits, camera, [0, 0, 2048, 2048]);
        assert!(!first.is_empty());
        for _ in 0..5 {
            assert_eq!(
                composite(&mut credits, camera, [0, 0, 2048, 2048])[0].position,
                first[0].position
            );
        }
        let rising = composite(&mut credits, camera, [0, 0, 2048, 2048]);
        assert_eq!(rising[0].position[1], first[0].position[1] - 1.0);
        assert!(
            rising.iter().all(|glyph| glyph.alpha == 1.0),
            "native labels do not fade"
        );
        assert_eq!(
            credits.build_instances(&font, camera, 1.0, [0, 0, 2048, 2048])[0].position,
            rising[0].position,
            "a draw between composites does not rise"
        );
        for _ in 0..70 {
            assert!(composite(&mut credits, [1e6, 1e6], [0, 0, 1, 1]).is_empty());
        }
        assert!(credits.live.is_empty(), "offscreen labels still expire");
        credits.admit(&sim, &rules, event);
        credits.clear_on_load();
        assert!(credits.live.is_empty());
    }

    /// Phobos `FlyingStrings::AddMoneyString`'s `DisplaySoylentToHouses=`
    /// filter: the paying House must be targetable by the local player, and
    /// the label keeps the event's pixel offset.
    #[cfg(feature = "phobos")]
    #[test]
    fn display_soylent_filters_by_the_paying_house() {
        use crate::rules::phobos_shields::AffectedHouse;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]
0=TANK
[TANK]
Strength=100
Sight=0
",
        ))
        .unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        let actor = sim
            .spawn_object("TANK", "Americans", 16, 16, 64, &rules)
            .unwrap();
        let viewer = sim.entities().get(actor).unwrap().owner();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(viewer));
        sim.fog_mut_for_test().reveal_all_for_owner(viewer);
        let event = BountyText {
            entity_id: Some(actor),
            amount: 40,
            coord: ProjectileCoord {
                x: 16 * 256,
                y: 16 * 256,
                z: 0,
            },
            z_offset: 0,
            display_to: Some(AffectedHouse::ENEMIES),
            display_owner: None,
            pixel_offset: [5, -3],
        };
        let mut credits = FlyingCredits::default();
        credits.admit(&sim, &rules, event);
        assert!(
            credits.live.is_empty(),
            "the paying House is the viewer, not an enemy"
        );
        credits.admit(
            &sim,
            &rules,
            BountyText {
                display_to: Some(AffectedHouse::OWNER),
                ..event
            },
        );
        assert_eq!(credits.live[0].pixel_offset, [5, -3]);
        assert_eq!(credits.live[0].text, "+$40");
    }

    /// `AddMoneyString`'s `pOwner` is not always the anchor object's owner:
    /// the drain target label anchors at the drained building while its House
    /// filter asks the drainer's House. `display_owner` carries that owner.
    #[cfg(feature = "phobos")]
    #[test]
    fn the_money_label_house_filter_uses_its_explicit_owner() {
        use crate::rules::phobos_shields::AffectedHouse;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]
0=TANK
[TANK]
Strength=100
Sight=0
",
        ))
        .unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        let tank = sim
            .spawn_object("TANK", "Americans", 16, 16, 64, &rules)
            .unwrap();
        let viewer = sim.entities().get(tank).unwrap().owner();
        let drainer = sim.interner().get("Russians").expect("arena enemy house");
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(viewer));
        sim.fog_mut_for_test().reveal_all_for_owner(viewer);
        let event = BountyText {
            entity_id: Some(tank),
            amount: -40,
            coord: ProjectileCoord {
                x: 16 * 256,
                y: 16 * 256,
                z: 0,
            },
            z_offset: 0,
            display_to: Some(AffectedHouse::OWNER),
            display_owner: Some(drainer),
            pixel_offset: [0, 0],
        };
        let mut credits = FlyingCredits::default();
        credits.admit(&sim, &rules, event);
        assert!(
            credits.live.is_empty(),
            "the drainer's House is not the viewer even though the anchor is"
        );
        credits.admit(
            &sim,
            &rules,
            BountyText {
                display_to: Some(AffectedHouse::ENEMIES),
                ..event
            },
        );
        assert_eq!(credits.live[0].text, "-$40");
    }

    /// Phobos `FlyingStrings::AddMoneyString`'s warhead `TransactMoney` call
    /// passes `pSource = nullptr`: the label has no anchor object, so the
    /// source's visual-character test is skipped and the House filter reads
    /// the event's explicit owner.
    #[cfg(feature = "phobos")]
    #[test]
    fn a_source_less_money_label_uses_its_explicit_owner() {
        use crate::rules::phobos_shields::AffectedHouse;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]
0=TANK
[TANK]
Strength=100
Sight=0
",
        ))
        .unwrap();
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 23);
        *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
        let tank = sim
            .spawn_object("TANK", "Americans", 16, 16, 64, &rules)
            .unwrap();
        let viewer = sim.entities().get(tank).unwrap().owner();
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(viewer));
        sim.fog_mut_for_test().reveal_all_for_owner(viewer);
        let event = BountyText {
            entity_id: None,
            amount: 9,
            coord: ProjectileCoord {
                x: 16 * 256,
                y: 16 * 256,
                z: 0,
            },
            z_offset: 0,
            display_to: Some(AffectedHouse::OWNER),
            display_owner: Some(viewer),
            pixel_offset: [0, 0],
        };
        let mut credits = FlyingCredits::default();
        credits.admit(&sim, &rules, event);
        assert_eq!(credits.live[0].text, "+$9");
        credits.admit(
            &sim,
            &rules,
            BountyText {
                display_to: Some(AffectedHouse::ENEMIES),
                ..event
            },
        );
        assert_eq!(
            credits.live.len(),
            1,
            "the explicit owner fails the enemies filter"
        );
    }
}
