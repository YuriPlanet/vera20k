//! Ares DisplayCredits10044490 and Tactical_Draw_FlyingStrings10064430.
//! The client owns copied text and draw lifetimes; it never changes the wallet
//! or retains live simulation pointers. Glyphs use the existing GAME.FNT atlas.

use crate::{
    render::{batch::SpriteInstance, bit_font::BitFont},
    rules::ruleset::RuleSet,
    sim::{
        bounty::BountyText, cloak_disguise::VisualCharacterQuery, components::DriveCoord,
        projectile::ProjectileCoord, world::Simulation,
    },
};

struct CreditText {
    coord: ProjectileCoord,
    text: String,
    rgb: [u8; 3],
    life: i32,
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
        let Some(actor) = sim.entities().get(event.entity_id) else {
            return;
        };
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
            text,
            // Native RGB literals1009E814/1009E810, packed by10015830.
            rgb: if positive { [0, 255, 0] } else { [255, 0, 0] },
            // AddFlyingString100643D9 stores70+5; draw decrements after use.
            life: 75,
        });
    }

    pub(crate) fn clear_on_load(&mut self) {
        self.live.clear();
    }

    ///10064430..10064514: append order, no fade; only the final70 draws
    /// rise one pixel each. Offscreen records still age in the draw tail.
    pub(crate) fn build_instances(
        &mut self,
        font: &BitFont,
        camera: [f32; 2],
        zoom: f32,
        viewport: [u32; 2],
    ) -> Vec<SpriteInstance> {
        let mut out = Vec::new();
        for record in &mut self.live {
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
            if x >= 0.0 && y >= 0.0 && x < viewport[0] as f32 && y < viewport[1] as f32 {
                let rise = (record.life - 70).min(0);
                let left = x - (font.text_width(&record.text) / 2) as f32;
                let rgb = crate::render::native_surface_format::RGB565;
                let tint = rgb
                    .unpack_rgb8(rgb.pack_rgb8(record.rgb))
                    .map(|v| f32::from(v) / 255.0);
                out.extend(font.build_text(
                    &record.text,
                    left,
                    y + rise as f32,
                    1.0,
                    0.0,
                    tint,
                    camera,
                ));
            }
            record.life -= 1;
        }
        self.live.retain(|record| record.life >= 0);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn bounty_text_obeys_shroud_and_ages_only_at_the_draw_owner() {
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
            entity_id: actor,
            amount: 30,
            coord: ProjectileCoord {
                x: 12 * 256,
                y: 12 * 256,
                z: 0,
            },
            z_offset: 256,
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
        let first = credits.build_instances(&font, camera, 1.0, [2048, 2048]);
        assert!(!first.is_empty());
        for _ in 0..5 {
            assert_eq!(
                credits.build_instances(&font, camera, 1.0, [2048, 2048])[0].position,
                first[0].position
            );
        }
        let rising = credits.build_instances(&font, camera, 1.0, [2048, 2048]);
        assert_eq!(rising[0].position[1], first[0].position[1] - 1.0);
        assert!(
            rising.iter().all(|glyph| glyph.alpha == 1.0),
            "native labels do not fade"
        );
        for _ in 0..76 {
            assert!(
                credits
                    .build_instances(&font, [1e6, 1e6], 1.0, [1, 1])
                    .is_empty()
            );
        }
        assert!(credits.live.is_empty(), "offscreen labels still expire");
        credits.admit(&sim, &rules, event);
        credits.clear_on_load();
        assert!(credits.live.is_empty());
    }
}
