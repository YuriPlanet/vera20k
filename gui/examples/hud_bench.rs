//! Frame costs of the HUD over synthetic, repeatable scenes.
//!
//! `cargo run --release -p vera20k-gui --example hud_bench`
//!
//! World case: 5000 objects that move every frame. Tree cases: 300 nodes,
//! where a panel moves before each layout. `paint` repeats the tree's last
//! picture, as its output borrows the HUD and so cannot be kept across a
//! change. Every paint case reuses one buffer, grown by an untimed first
//! frame.

use std::hint::black_box;
use std::time::{Duration, Instant};

use vera20k_gui::status::{Amount, Band, Element, ObjectView, SourceId, StatusTemplate, World};
use vera20k_gui::{Gui, Layer, NodeId, NodeKind, Origin, Prop, Rect, Refused, Rgba, Value};

const FRAMES: usize = 600;
const OBJECTS: usize = 5000;
const SOURCES: usize = 4;

// 10 panels, each a column of 7 rows of 3 leaves, all placed by rect.
const PANELS: usize = 10;
const ROWS: usize = 7;
const NODES: usize = PANELS * (2 + ROWS * 4);

const VIEWPORT: Rect = Rect::new(0, 0, 1920, 1080);

const SLATE: Rgba = Rgba(40, 44, 52, 255);
const SAND: Rgba = Rgba(190, 170, 130, 255);
const TEAL: Rgba = Rgba(60, 140, 150, 255);

struct Report {
    case: String,
    times: Vec<Duration>,
    commands: Option<usize>,
}

impl Report {
    fn new(case: impl Into<String>) -> Self {
        let times = Vec::with_capacity(FRAMES);
        Report {
            case: case.into(),
            times,
            commands: None,
        }
    }

    fn time(&mut self, work: impl FnOnce()) {
        let start = Instant::now();
        work();
        self.times.push(start.elapsed());
    }

    fn print(mut self) {
        self.times.sort_unstable();
        let last = self.times.len() - 1;
        // Nearest rank.
        let at = |percent: usize| {
            let rank = (self.times.len() * percent).div_ceil(100).saturating_sub(1);
            self.times[rank.min(last)].as_secs_f64() * 1e6
        };
        let commands = self.commands.map_or_else(|| "-".to_owned(), |n| n.to_string());
        println!(
            "{:<28} {:>9.2} {:>9.2} {:>9.2} {:>8}",
            self.case,
            at(50),
            at(95),
            at(100),
            commands,
        );
    }
}

/// A fixed scramble of `n`, so that scenes repeat from run to run.
fn scramble(n: usize) -> u32 {
    let mut x = (n as u32).wrapping_add(1).wrapping_mul(0x9E37_79B9);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85EB_CA6B);
    x ^ (x >> 13)
}

fn amounts() -> Vec<[Option<Amount>; SOURCES]> {
    (0..OBJECTS)
        .map(|object| {
            std::array::from_fn(|source| {
                let value = (scramble(object * SOURCES + source) % 100) as i32 + 1;
                Some(Amount { value, max: 100 })
            })
        })
        .collect()
}

fn objects(amounts: &[[Option<Amount>; SOURCES]]) -> Vec<ObjectView<'_>> {
    amounts
        .iter()
        .enumerate()
        .map(|(i, values)| ObjectView {
            id: i as u64,
            x: (scramble(i) % 1920) as i32,
            y: (scramble(i + OBJECTS) % 1080) as i32,
            type_id: (i % 16) as u32,
            category: 1,
            relation: 1,
            state: 1,
            values,
            slots: &[],
        })
        .collect()
}

fn move_objects(objects: &mut [ObjectView<'_>], frame: usize) {
    let step = if frame.is_multiple_of(2) { 1 } else { -1 };
    for object in objects {
        object.x += step;
    }
}

fn status_world(templates: usize) -> Result<World, Refused> {
    let mut world = World::default();
    for i in 0..templates {
        let template = StatusTemplate {
            name: format!("gauge-{i}"),
            elements: vec![Element::Bar {
                rect: Rect::new(-16, -24 - 6 * i as i32, 32, 4),
                source: SourceId(i as u16),
                bands: vec![
                    Band {
                        percent: 0,
                        color: SAND,
                    },
                    Band {
                        percent: 50,
                        color: TEAL,
                    },
                ],
                background: Some(SLATE),
            }],
            ..Default::default()
        };
        world.add_template(Origin::Host, template)?;
    }
    Ok(world)
}

fn bench_status(templates: usize) -> Result<Report, Refused> {
    let world = status_world(templates)?;
    let amounts = amounts();
    let mut objects = objects(&amounts);
    let plural = if templates == 1 { "" } else { "s" };
    let mut report = Report::new(format!("paint_status, {templates} template{plural}"));

    let mut out = Vec::new();
    world.paint_status(&objects, &mut out);
    for frame in 0..FRAMES {
        move_objects(&mut objects, frame);
        out.clear();
        report.time(|| {
            world.paint_status(black_box(&objects), &mut out);
            black_box(out.as_slice());
        });
    }
    report.commands = Some(out.len());
    Ok(report)
}

struct PanelTree {
    hud: Gui,
    panels: Vec<NodeId>,
}

fn place(hud: &mut Gui, id: NodeId, x: i32, y: i32, w: i32, h: i32) -> Result<(), Refused> {
    hud.set(id, Prop::Rect, Value::Rect(Rect::new(x, y, w, h)))
}

fn panel_x(p: usize) -> i32 {
    10 + (p % 5) as i32 * 380
}

fn panel_tree() -> Result<PanelTree, Refused> {
    let mut hud = Gui::new(NODES);
    let mut panels = Vec::new();
    for p in 0..PANELS {
        let panel = hud.create_root(Layer::Panels, Origin::Host, NodeKind::Panel)?;
        let (x, y) = (panel_x(p), 10 + (p / 5) as i32 * 440);
        place(&mut hud, panel, x, y, 360, 420)?;
        hud.set(panel, Prop::Background, Value::Rgba(SLATE))?;
        hud.set(panel, Prop::Clip, Value::Bool(true))?;

        let column = hud.create_by(NodeKind::Panel, Some(panel), Origin::Host)?;
        let (x, y) = (x + 10, y + 10);
        place(&mut hud, column, x, y, 340, 400)?;

        for r in 0..ROWS {
            let row = hud.create_by(NodeKind::Panel, Some(column), Origin::Host)?;
            let y = y + r as i32 * 46;
            place(&mut hud, row, x, y, 340, 40)?;

            let icon = hud.create_by(NodeKind::Image, Some(row), Origin::Host)?;
            place(&mut hud, icon, x, y, 24, 24)?;
            hud.set(
                icon,
                Prop::Art,
                Value::Text(format!("bench/icon-{}", r % 3)),
            )?;

            let button = hud.create_by(NodeKind::Button, Some(row), Origin::Host)?;
            place(&mut hud, button, x + 32, y, 180, 28)?;
            hud.set(button, Prop::Art, Value::Text("bench/plate".to_owned()))?;
            hud.set(button, Prop::Label, Value::Text(format!("Entry {p}.{r}")))?;

            let bar = hud.create_by(NodeKind::Bar, Some(row), Origin::Host)?;
            let percent = (scramble(p * ROWS + r) % 100) as i32 + 1;
            place(&mut hud, bar, x + 220, y, 100, 8)?;
            hud.set(bar, Prop::Fill, Value::Int(percent))?;
            hud.set(bar, Prop::Background, Value::Rgba(SAND))?;
            hud.set(bar, Prop::Color, Value::Rgba(TEAL))?;
        }
        panels.push(panel);
    }
    Ok(PanelTree { hud, panels })
}

fn bench_tree() -> Result<[Report; 2], Refused> {
    let PanelTree { mut hud, panels } = panel_tree()?;
    let mut layout = Report::new("layout");
    let mut paint = Report::new("paint");

    hud.layout(VIEWPORT);
    for frame in 0..FRAMES {
        let p = frame % PANELS;
        // Each panel alternates between two places, so every layout places
        // that panel again (and copies the others).
        let x = panel_x(p) + ((frame / PANELS + 1) % 2) as i32;
        place(&mut hud, panels[p], x, 10 + (p / 5) as i32 * 440, 360, 420)?;
        layout.time(|| hud.layout(black_box(VIEWPORT)));
    }
    hud.layout(VIEWPORT);

    let mut out = Vec::new();
    hud.paint(&mut out);
    for _ in 0..FRAMES {
        out.clear();
        paint.time(|| {
            black_box(&hud).paint(&mut out);
            black_box(out.as_slice());
        });
    }
    paint.commands = Some(out.len());
    Ok([layout, paint])
}

fn print_heading(scene: &str) {
    println!("\n{scene}, {FRAMES} frames");
    println!(
        "{:<28} {:>9} {:>9} {:>9} {:>8}",
        "case", "p50 us", "p95 us", "max us", "commands"
    );
}

fn main() -> Result<(), Refused> {
    if cfg!(debug_assertions) {
        println!("debug build: use --release for figures that mean anything");
    }

    print_heading(&format!("{OBJECTS} objects"));
    bench_status(1)?.print();
    bench_status(4)?.print();

    print_heading(&format!("{NODES}-node panel tree"));
    for report in bench_tree()? {
        report.print();
    }
    Ok(())
}
