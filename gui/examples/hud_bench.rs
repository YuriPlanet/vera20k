//! Frame costs of the HUD over synthetic, repeatable scenes.
//!
//! `cargo run --release -p vera20k-gui --example hud_bench`
//!
//! World cases: 5000 objects that move every frame. Tree cases: 300 nodes,
//! where a panel moves before each layout and the pointer jumps before each
//! move. `paint` repeats the tree's last picture, as its output borrows the
//! HUD and so cannot be kept across a change. Every paint case reuses one
//! buffer, grown by an untimed first frame.

use std::hint::black_box;
use std::time::{Duration, Instant};

use vera20k_gui::hud::world::{
    Amount, Anchor, Band, Element, ObjectView, Shape, SourceId, StatusTemplate, World,
};
use vera20k_gui::hud::{Hud, InputPolicy, Layer, Origin, PaintOp, Pointer, batches};
use vera20k_gui::{NodeId, NodeKind, Prop, Rect, Refused, Rgba, Value};

const FRAMES: usize = 600;
const OBJECTS: usize = 5000;
const SOURCES: usize = 4;
const LISTS: usize = 500;

// 10 panels, each a column of 7 rows of 3 leaves.
const PANELS: usize = 10;
const ROWS: usize = 7;
const NODES: usize = PANELS * (2 + ROWS * 4);

const VIEWPORT: Rect = Rect::new(0, 0, 1920, 1080);
const OFF_THE_PANELS: (i32, i32) = (1910, 1070);

const SLATE: Rgba = Rgba(40, 44, 52, 255);
const SAND: Rgba = Rgba(190, 170, 130, 255);
const TEAL: Rgba = Rgba(60, 140, 150, 255);

struct Report {
    case: String,
    times: Vec<Duration>,
    commands: Option<usize>,
    batches: Option<usize>,
}

impl Report {
    fn new(case: impl Into<String>) -> Self {
        let times = Vec::with_capacity(FRAMES);
        Report {
            case: case.into(),
            times,
            commands: None,
            batches: None,
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
        let count = |n: Option<usize>| n.map_or_else(|| "-".to_owned(), |n| n.to_string());
        println!(
            "{:<28} {:>9.2} {:>9.2} {:>9.2} {:>8} {:>8}",
            self.case,
            at(50),
            at(95),
            at(100),
            count(self.commands),
            count(self.batches),
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

fn bench_lists() -> Result<Report, Refused> {
    let mut world = World::default();
    for i in 0..LISTS {
        let shapes = vec![
            Shape::Rect {
                rect: Rect::new(-6, -6, 12, 12),
                color: SAND,
            },
            Shape::Line {
                from: (0, 0),
                to: (20, -20),
                width: 2,
                color: TEAL,
            },
        ];
        world.add_list(
            Origin::Host,
            Anchor::Object((i * (OBJECTS / LISTS)) as u64),
            shapes,
        )?;
    }
    let amounts = amounts();
    let mut objects = objects(&amounts);
    let mut report = Report::new(format!("paint_lists, {LISTS} lists"));

    let mut out = Vec::new();
    world.paint_lists(|_| Some((0, 0)), &mut out);
    for frame in 0..FRAMES {
        move_objects(&mut objects, frame);
        out.clear();
        let project = |anchor| match anchor {
            Anchor::Object(id) => objects.get(id as usize).map(|o| (o.x, o.y)),
            Anchor::Cell { .. } | Anchor::Screen { .. } => None,
        };
        report.time(|| {
            world.paint_lists(black_box(project), &mut out);
            black_box(out.as_slice());
        });
    }
    report.commands = Some(out.len());
    Ok(report)
}

struct PanelTree {
    hud: Hud,
    panels: Vec<NodeId>,
    buttons: Vec<NodeId>,
}

fn set_ints(hud: &mut Hud, id: NodeId, props: &[(Prop, i32)]) -> Result<(), Refused> {
    props
        .iter()
        .try_for_each(|(prop, value)| hud.set(id, *prop, Value::Int(*value)))
}

fn panel_tree() -> Result<PanelTree, Refused> {
    let mut hud = Hud::new(NODES, InputPolicy::default());
    let (mut panels, mut buttons) = (Vec::new(), Vec::new());
    for p in 0..PANELS {
        let panel = hud.create_root(Layer::Panels, Origin::Host, NodeKind::Panel)?;
        let (x, y) = (10 + (p % 5) as i32 * 380, 10 + (p / 5) as i32 * 440);
        set_ints(
            &mut hud,
            panel,
            &[
                (Prop::X, x),
                (Prop::Y, y),
                (Prop::Width, 360),
                (Prop::Height, 420),
            ],
        )?;
        hud.set(panel, Prop::Background, Value::Rgba(SLATE))?;
        hud.set(panel, Prop::Clip, Value::Bool(true))?;
        hud.set(panel, Prop::BlocksPointer, Value::Bool(true))?;

        let column = hud.create(NodeKind::Column, panel, Origin::Host)?;
        set_ints(
            &mut hud,
            column,
            &[
                (Prop::X, 10),
                (Prop::Y, 10),
                (Prop::Width, 340),
                (Prop::Height, 400),
                (Prop::Gap, 6),
            ],
        )?;

        for r in 0..ROWS {
            let row = hud.create(NodeKind::Row, column, Origin::Host)?;
            set_ints(
                &mut hud,
                row,
                &[(Prop::Width, 340), (Prop::Height, 40), (Prop::Gap, 8)],
            )?;

            let icon = hud.create(NodeKind::Image, row, Origin::Host)?;
            set_ints(&mut hud, icon, &[(Prop::Width, 24), (Prop::Height, 24)])?;
            hud.set(
                icon,
                Prop::Art,
                Value::Text(format!("bench/icon-{}", r % 3)),
            )?;

            let button = hud.create(NodeKind::Button, row, Origin::Host)?;
            set_ints(&mut hud, button, &[(Prop::Width, 180), (Prop::Height, 28)])?;
            hud.set(button, Prop::Hoverable, Value::Bool(true))?;
            hud.set(button, Prop::Art, Value::Text("bench/plate".to_owned()))?;
            hud.set(button, Prop::Label, Value::Text(format!("Entry {p}.{r}")))?;
            buttons.push(button);

            let bar = hud.create(NodeKind::Bar, row, Origin::Host)?;
            let percent = (scramble(p * ROWS + r) % 100) as i32 + 1;
            set_ints(
                &mut hud,
                bar,
                &[(Prop::Width, 100), (Prop::Height, 8), (Prop::Fill, percent)],
            )?;
            hud.set(bar, Prop::Background, Value::Rgba(SAND))?;
            hud.set(bar, Prop::Color, Value::Rgba(TEAL))?;
        }
        panels.push(panel);
    }
    Ok(PanelTree {
        hud,
        panels,
        buttons,
    })
}

#[derive(PartialEq)]
enum Material<'a> {
    Solid,
    Glyphs,
    Picture(&'a str),
    Surface(&'a str),
}

fn material<'a>(op: &PaintOp<'a>) -> Material<'a> {
    match *op {
        PaintOp::Fill { .. } => Material::Solid,
        PaintOp::Text { .. } => Material::Glyphs,
        PaintOp::Image { art, .. } => Material::Picture(art),
        PaintOp::External { key, .. } => Material::Surface(key),
    }
}

fn bench_tree() -> Result<[Report; 3], Refused> {
    let PanelTree {
        mut hud,
        panels,
        buttons,
    } = panel_tree()?;
    let mut layout = Report::new("layout");
    let mut pointer = Report::new("pointer move");
    let mut paint = Report::new("paint");

    hud.layout(VIEWPORT);
    for frame in 0..FRAMES {
        let p = frame % PANELS;
        // Each panel alternates between two places, so every layout has work.
        let x = 10 + (p % 5) as i32 * 380 + ((frame / PANELS + 1) % 2) as i32;
        hud.set(panels[p], Prop::X, Value::Int(x))?;
        layout.time(|| hud.layout(black_box(VIEWPORT)));

        let target = buttons[scramble(frame) as usize % buttons.len()];
        let (x, y) = match hud.rect(target) {
            Some(r) if !frame.is_multiple_of(5) => (r.x + r.w / 2, r.y + r.h / 2),
            _ => OFF_THE_PANELS,
        };
        pointer.time(|| {
            black_box(hud.input(black_box(Pointer::Move { x, y })));
        });
        black_box(hud.drain_events());
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
    paint.batches = Some(batches(&out, material).count());
    Ok([layout, pointer, paint])
}

fn print_heading(scene: &str) {
    println!("\n{scene}, {FRAMES} frames");
    println!(
        "{:<28} {:>9} {:>9} {:>9} {:>8} {:>8}",
        "case", "p50 us", "p95 us", "max us", "commands", "batches"
    );
}

fn main() -> Result<(), Refused> {
    if cfg!(debug_assertions) {
        println!("debug build: use --release for figures that mean anything");
    }

    print_heading(&format!("{OBJECTS} objects"));
    bench_status(1)?.print();
    bench_status(4)?.print();
    bench_lists()?.print();

    print_heading(&format!("{NODES}-node panel tree"));
    for report in bench_tree()? {
        report.print();
    }
    Ok(())
}
