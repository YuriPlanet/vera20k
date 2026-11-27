//! Status templates a mod or map adds over the battlefield (VERA, RFC 0001
//! appendix 2.1 `client_status` and "Status templates"): bars, pip rows,
//! icons and numbers drawn at every object the local player sees, laid out
//! by `vera20k-gui`'s status templates (`World::paint_status`).
//!
//! A template comes from INI or from a client module, in one form: the keys
//! of an INI section. The mod's `veraui.ini` and the map list theirs in
//! `[VeraStatus] Templates=` and give each a `[VeraStatus.<name>]` section;
//! a module hands the same keys to `client_status` with `Name=` first. A
//! template whose name its source already has replaces it; one with no
//! `ElementN=` removes it.
//!
//! ```ini
//! [VeraStatus]
//! Templates=Energy
//! HideNative=HealthBars          ; HealthBars, Pips, Marks, Groups
//!
//! [VeraStatus.Energy]
//! Category=Vehicle,Infantry      ; Vehicle, Infantry, Building, Aircraft
//! Relation=Own,Ally              ; Own, Ally, Enemy
//! State=Selected                 ; Selected, Hovered, Damaged
//! Types=MTNK,HTNK
//! When1=AtLeast,Var:Energy,1     ; Present, BelowMax, AtLeast, AtMost
//! Element1=Bar,-16,-30,32,3,Var:Energy/100,#000000,50:#00C0FF,0:#0040FF
//! Element2=Number,18,-34,Var:Energy,#FFFFFF
//! Element3=Segments,-16,-22,4,4,4,0,5,Health,pips.shp
//! Element4=Icon,-8,-44,0,0,myicon.shp
//! ```
//!
//! Every rect is from the object's anchor: a mobile's projected centre
//! lowered by its type's `PixelSelectionBracketDelta=`, a building's first
//! health pip, where the original's health bar starts. Pictures are found by
//! file name (`pips.shp`) as the modules' `Art` is; colours are `#RRGGBB` or `#RRGGBBAA`.
//!
//! The amounts (`Source`): `Health` (of `Strength=`), `Shield` (Phobos
//! shield), `Ammo` (of `Ammo=`), `Passengers` (of `Passengers=`), `Rank`
//! (0..2 of 2) and `Var:<name>` an object-scope module variable, with
//! `/<max>` a number or `Var:<name>` for a bar. A variable shows only where
//! the player may read it (RFC 6e); an amount the object lacks hides what
//! reads it.
//!
//! `HideNative=` (in `[VeraStatus]` or a module's template) hides the
//! original's families for the match: only the mod's templates and a map's
//! under `[VeraWasm] MapUI=full` may.
//!
//! Residuals: the templates draw over the battlefield after its objects, not
//! at each object's depth; zoom keeps their pixel size, as the health bar's.

use vera20k_gui::status::{
    Amount, ArtId, Band, Condition, Element, ObjectView, SourceId, StatusTemplate, World,
    WorldCmd,
};
use vera20k_gui::{Origin, Rect, Rgba};
use vera20k_wasm_ext::Visibility;

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::sim::ext_modules::MapUi;
use crate::sim::ext_modules::ModuleSource;
use crate::sim::ext_modules::vars::VarStore;

/// The list section, in `veraui.ini` and in a map.
pub(crate) const SECTION: &str = "VeraStatus";

/// Native families `HideNative=` hides.
pub(crate) mod hide {
    /// The health strip, its bracket and the Phobos shield bar.
    pub(crate) const HEALTH_BARS: u8 = 1;
    /// The pip scale (cargo, ammo, garrison, storage).
    pub(crate) const PIPS: u8 = 2;
    /// Rank insignia, self-heal, wrench, bomb clock and power-off marks.
    pub(crate) const MARKS: u8 = 4;
    /// Control-group numbers.
    pub(crate) const GROUPS: u8 = 8;
}

/// Who a template comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StatusOrigin {
    ModModule,
    MapModule,
    ModIni,
    MapIni,
}

impl StatusOrigin {
    const ALL: [Self; 4] = [Self::ModModule, Self::MapModule, Self::ModIni, Self::MapIni];

    fn gui(self) -> Origin {
        Origin::External(self as u16)
    }

    /// Whose variables a name without `mod.`/`map.` addresses.
    fn source(self) -> ModuleSource {
        match self {
            Self::ModModule | Self::ModIni => ModuleSource::Mod,
            Self::MapModule | Self::MapIni => ModuleSource::Map,
        }
    }

    /// The mod's own, and a map's under `MapUI=full`.
    fn may_hide(self, map_ui: MapUi) -> bool {
        matches!(self, Self::ModModule | Self::ModIni) || map_ui == MapUi::Full
    }

    pub(crate) fn of_module(source: vera20k_wasm_ext::client::Source) -> Self {
        match source {
            vera20k_wasm_ext::client::Source::Mod => Self::ModModule,
            vera20k_wasm_ext::client::Source::Map => Self::MapModule,
        }
    }
}

// The amounts' slots in an object's values.
const HEALTH: u16 = 0;
const SHIELD: u16 = 1;
const AMMO: u16 = 2;
const PASSENGERS: u16 = 3;
const RANK: u16 = 4;
/// The first variable's slot; each distinct variable binding takes one.
const FIRST_VAR: u16 = 16;
/// Distinct variable bindings a match holds.
const MAX_VARS: usize = 16;
/// Slots in an object's values.
pub(crate) const SLOTS: usize = FIRST_VAR as usize + MAX_VARS;

/// What a variable's bar is full at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VarMax {
    Fixed(i32),
    Var(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VarSource {
    var: i32,
    max: VarMax,
}

/// Why a template was not taken.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Problem {
    /// The text named something that does not exist or does not parse.
    Bad(String),
    /// Over a limit, or a key the source may not use.
    Refused(String),
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bad(why) | Self::Refused(why) => f.write_str(why),
        }
    }
}

/// What a template's names resolve against.
pub(crate) struct Names<'a> {
    /// A TechnoType's id as the objects carry it (`InternedId::index`).
    pub(crate) type_id: &'a dyn Fn(&str) -> Option<u32>,
    pub(crate) vars: &'a VarStore,
}

/// One object as the templates see it.
pub(crate) struct StatusObject<'a> {
    pub(crate) id: u64,
    pub(crate) anchor: (i32, i32),
    pub(crate) type_id: u32,
    pub(crate) category: crate::map::entities::EntityCategory,
    /// The local player's own object, an ally's or anyone else's.
    pub(crate) own: bool,
    pub(crate) allied: bool,
    pub(crate) selected: bool,
    pub(crate) hovered: bool,
    pub(crate) health: (i32, i32),
    pub(crate) shield: Option<(i32, i32)>,
    pub(crate) ammo: Option<(i32, i32)>,
    pub(crate) passengers: Option<(i32, i32)>,
    pub(crate) rank: Option<i32>,
    /// Its stored variable values (`StatusFacts::vars`).
    pub(crate) vars: &'a [(i32, i64)],
}

/// The match's status templates and what they name.
#[derive(Default)]
pub(crate) struct ModuleStatus {
    world: World,
    /// `ArtId(n)` is `arts[n]`.
    arts: Vec<String>,
    /// Pictures named since the last frame asked for them.
    new_art: Vec<String>,
    /// Slot `FIRST_VAR + n` is `vars[n]`.
    vars: Vec<VarSource>,
    /// The native families each (origin, template) hides; `None` for the
    /// list section's own `HideNative=`.
    hides: Vec<(StatusOrigin, Option<String>, u8)>,
    revision: u64,
}

impl ModuleStatus {
    pub(crate) fn is_empty(&self) -> bool {
        self.world.is_empty() && self.hides.is_empty()
    }

    pub(crate) fn has_templates(&self) -> bool {
        !self.world.is_empty()
    }

    /// Moves with every change, for the redraw judge.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// The native families hidden for the match (`hide::*`).
    pub(crate) fn hidden(&self) -> u8 {
        self.hides.iter().fold(0, |all, (_, _, bits)| all | bits)
    }

    pub(crate) fn art_name(&self, art: ArtId) -> Option<&str> {
        self.arts.get(art.0 as usize).map(String::as_str)
    }

    /// The pictures named since the last call.
    pub(crate) fn take_new_art(&mut self) -> Vec<String> {
        std::mem::take(&mut self.new_art)
    }

    /// A source's `[VeraStatus]` list and its template sections.
    pub(crate) fn add_ini(
        &mut self,
        origin: StatusOrigin,
        ini: &IniFile,
        map_ui: MapUi,
        names: &Names<'_>,
    ) -> Vec<String> {
        let mut problems = Vec::new();
        let Some(list) = find_section(ini, SECTION) else {
            return problems;
        };
        match list.read_name("HideNative", 256).map(hide_bits) {
            Some(Ok(bits)) if bits != 0 => {
                if origin.may_hide(map_ui) {
                    self.hides.push((origin, None, bits));
                    self.revision += 1;
                } else {
                    problems.push(format!("[{SECTION}] HideNative needs [VeraWasm] MapUI=full"));
                }
            }
            Some(Err(problem)) => problems.push(format!("[{SECTION}] {problem}")),
            _ => {}
        }
        let listed = list.read_name("Templates", 4096).unwrap_or_default().to_owned();
        for name in listed.split(',').map(str::trim).filter(|name| !name.is_empty()) {
            let section_name = format!("{SECTION}.{name}");
            let Some(section) = find_section(ini, &section_name) else {
                problems.push(format!("[{section_name}] is missing"));
                continue;
            };
            if let Err(problem) = self.register(origin, name, section, map_ui, names) {
                problems.push(format!("[{section_name}] {problem}"));
            }
        }
        problems
    }

    /// `client_status`: the template's keys as INI text, `Name=` first.
    pub(crate) fn add_text(
        &mut self,
        origin: StatusOrigin,
        text: &str,
        map_ui: MapUi,
        names: &Names<'_>,
    ) -> Result<(), Problem> {
        let mut bytes = Vec::with_capacity(text.len() + 8);
        bytes.extend_from_slice(b"[T]\n");
        bytes.extend_from_slice(text.as_bytes());
        bytes.push(b'\n');
        let ini = IniFile::from_bytes(&bytes).map_err(|e| Problem::Bad(e.to_string()))?;
        let section = ini.section("T").ok_or_else(|| Problem::Bad("no keys".into()))?;
        let name = section
            .read_name("Name", 64)
            .ok_or_else(|| Problem::Bad("no Name=".into()))?
            .to_owned();
        self.register(origin, &name, section, map_ui, names)
    }

    /// Everything `origin` added (its module stopped).
    pub(crate) fn remove_origin(&mut self, origin: StatusOrigin) {
        self.world.remove_origin(origin.gui());
        self.hides.retain(|(of, _, _)| *of != origin);
        self.revision += 1;
    }

    fn register(
        &mut self,
        origin: StatusOrigin,
        name: &str,
        section: &IniSection,
        map_ui: MapUi,
        names: &Names<'_>,
    ) -> Result<(), Problem> {
        let hidden = match section.read_name("HideNative", 256) {
            Some(text) => hide_bits(text).map_err(Problem::Bad)?,
            None => 0,
        };
        if hidden != 0 && !origin.may_hide(map_ui) {
            return Err(Problem::Refused("HideNative needs [VeraWasm] MapUI=full".into()));
        }
        // A hide-only definition has no GUI template to count toward its limit.
        if hidden != 0 && self.hides.len() >= origin.gui().share(vera20k_gui::status::MAX_TEMPLATES)
            && !self.hides.iter().any(|(of, held, _)| *of == origin && held.as_deref() == Some(name))
        {
            return Err(Problem::Refused("too many status hide definitions".into()));
        }
        let (arts, new_art, vars) = (self.arts.len(), self.new_art.len(), self.vars.len());
        let result = self.parse(origin, name, section, names).and_then(|template| {
            if template.elements.is_empty() {
                self.world.remove(origin.gui(), name);
                Ok(())
            } else {
                self.world.replace_template(origin.gui(), template)
                    .map_err(|refused| Problem::Refused(format!("over the template limits ({refused:?})")))
            }
        });
        if let Err(problem) = result {
            self.arts.truncate(arts);
            self.new_art.truncate(new_art);
            self.vars.truncate(vars);
            return Err(problem);
        }
        self.hides
            .retain(|(of, held, _)| !(*of == origin && held.as_deref() == Some(name)));
        if hidden != 0 {
            self.hides.push((origin, Some(name.to_owned()), hidden));
        }
        self.revision += 1;
        Ok(())
    }

    fn parse(
        &mut self,
        origin: StatusOrigin,
        name: &str,
        section: &IniSection,
        names: &Names<'_>,
    ) -> Result<StatusTemplate, Problem> {
        let mut template = StatusTemplate {
            name: name.to_owned(),
            ..Default::default()
        };
        let list = |key: &str| -> Vec<String> {
            section
                .read_name(key, 4096)
                .map(|text| {
                    text.split(',')
                        .map(|part| part.trim().to_owned())
                        .filter(|part| !part.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        };
        for kind in list("Types") {
            template.types.push(
                (names.type_id)(&kind).ok_or_else(|| Problem::Bad(format!("no type {kind}")))?,
            );
        }
        template.category = bits(&list("Category"), &CATEGORY_NAMES)?;
        template.relation = bits(&list("Relation"), &RELATION_NAMES)?;
        template.state = bits(&list("State"), &STATE_NAMES)?;
        for index in 1..=vera20k_gui::status::MAX_CONDITIONS_PER_TEMPLATE {
            let fields = list(&format!("When{index}"));
            if fields.is_empty() {
                continue;
            }
            template.when.push(self.condition(origin, &fields, names)?);
        }
        for index in 1..=vera20k_gui::status::MAX_ELEMENTS_PER_TEMPLATE {
            let fields = list(&format!("Element{index}"));
            if fields.is_empty() {
                continue;
            }
            template.elements.push(self.element(origin, &fields, names)?);
        }
        Ok(template)
    }

    fn condition(
        &mut self,
        origin: StatusOrigin,
        fields: &[String],
        names: &Names<'_>,
    ) -> Result<Condition, Problem> {
        let source = self.source(origin, field(fields, 1)?, names)?;
        Ok(match fields[0].to_ascii_lowercase().as_str() {
            "present" => Condition::Present(source),
            "belowmax" => Condition::BelowMax(source),
            "atleast" => Condition::AtLeast(source, int(field(fields, 2)?)?),
            "atmost" => Condition::AtMost(source, int(field(fields, 2)?)?),
            other => return Err(Problem::Bad(format!("no condition {other}"))),
        })
    }

    fn element(
        &mut self,
        origin: StatusOrigin,
        f: &[String],
        names: &Names<'_>,
    ) -> Result<Element, Problem> {
        let at = |x: usize| -> Result<Rect, Problem> {
            Ok(Rect::new(
                int(field(f, x)?)?,
                int(field(f, x + 1)?)?,
                int(field(f, x + 2)?)?.max(0),
                int(field(f, x + 3)?)?.max(0),
            ))
        };
        Ok(match f[0].to_ascii_lowercase().as_str() {
            "bar" => {
                let rect = at(1)?;
                let source = self.source(origin, field(f, 5)?, names)?;
                let background = color(field(f, 6)?)?;
                let mut bands = Vec::new();
                for band in f.iter().skip(7) {
                    let (percent, tint) = band
                        .split_once(':')
                        .ok_or_else(|| Problem::Bad(format!("band {band} is not percent:#colour")))?;
                    let percent = u8::try_from(int(percent)?.clamp(0, 100)).unwrap_or(0);
                    let color = color(tint)?.ok_or_else(|| Problem::Bad("a band needs a colour".into()))?;
                    bands.push(Band { percent, color });
                }
                Element::Bar {
                    rect,
                    source,
                    bands,
                    background,
                }
            }
            "segments" => Element::Segments {
                at: at(1)?,
                step: (int(field(f, 5)?)?, int(field(f, 6)?)?),
                count: u16::try_from(int(field(f, 7)?)?.max(0)).unwrap_or(u16::MAX),
                source: self.source(origin, field(f, 8)?, names)?,
                full: self.art(field(f, 9)?)?,
                empty: f.get(10).map(|name| self.art(name)).transpose()?,
            },
            // A row as long as its amount may take a whole object's share
            // of drawing commands, which only the engine's own templates
            // hold (`vera20k-gui`): a row of a known length is `Segments`.
            "pips" => {
                return Err(Problem::Refused(
                    "Pips is the engine's own; use Segments with a count".into(),
                ));
            }
            "icon" => Element::Icon {
                rect: at(1)?,
                art: self.art(field(f, 5)?)?,
            },
            "number" => Element::Number {
                rect: Rect::new(int(field(f, 1)?)?, int(field(f, 2)?)?, 0, 0),
                source: self.source(origin, field(f, 3)?, names)?,
                color: color(field(f, 4)?)?.unwrap_or(Rgba(255, 255, 255, 255)),
            },
            other => return Err(Problem::Bad(format!("no element {other}"))),
        })
    }

    fn art(&mut self, name: &str) -> Result<ArtId, Problem> {
        // At most two pictures per element. Replacing a template must not
        // grow the match's permanent picture cache without bound.
        const MAX_ART: usize = 2 * vera20k_gui::status::MAX_ELEMENTS_PER_TEMPLATE
            * vera20k_gui::status::MAX_TEMPLATES;
        let at = match self.arts.iter().position(|art| art.eq_ignore_ascii_case(name)) {
            Some(at) => at,
            None if self.arts.len() >= MAX_ART => {
                return Err(Problem::Refused("too many status pictures".into()));
            }
            None => {
                self.arts.push(name.to_owned());
                self.new_art.push(name.to_owned());
                self.arts.len() - 1
            }
        };
        Ok(ArtId(at as u32))
    }

    fn source(
        &mut self,
        origin: StatusOrigin,
        text: &str,
        names: &Names<'_>,
    ) -> Result<SourceId, Problem> {
        let slot = match text.to_ascii_lowercase().as_str() {
            "health" => HEALTH,
            "shield" => SHIELD,
            "ammo" => AMMO,
            "passengers" => PASSENGERS,
            "rank" => RANK,
            _ => {
                let rest = strip_prefix_ci(text, "Var:")
                    .ok_or_else(|| Problem::Bad(format!("no amount {text}")))?;
                let (name, max) = match rest.split_once('/') {
                    Some((name, max)) => (name, Some(max)),
                    None => (rest, None),
                };
                let var = object_var(origin, name, names)?;
                let max = match max {
                    None => VarMax::Fixed(0),
                    Some(max) => match strip_prefix_ci(max, "Var:") {
                        Some(name) => VarMax::Var(object_var(origin, name, names)?),
                        None => VarMax::Fixed(int(max)?),
                    },
                };
                let binding = VarSource { var, max };
                let at = match self.vars.iter().position(|held| *held == binding) {
                    Some(at) => at,
                    None if self.vars.len() < MAX_VARS => {
                        self.vars.push(binding);
                        self.vars.len() - 1
                    }
                    None => {
                        return Err(Problem::Refused(format!(
                            "more than {MAX_VARS} variable amounts in the match"
                        )));
                    }
                };
                FIRST_VAR + at as u16
            }
        };
        Ok(SourceId(slot))
    }

    /// The templates' commands for `objects`, each object's anchor moved by
    /// its commands' rects (whole pixels from the anchor).
    pub(crate) fn paint(
        &self,
        objects: &[StatusObject<'_>],
        vars: &VarStore,
        values: &mut Vec<[Option<Amount>; SLOTS]>,
        out: &mut Vec<WorldCmd>,
    ) {
        values.clear();
        values.extend(objects.iter().map(|object| self.values(object, vars)));
        let views: Vec<ObjectView<'_>> = objects
            .iter()
            .zip(values.iter())
            .map(|(object, values)| ObjectView {
                id: object.id,
                x: object.anchor.0,
                y: object.anchor.1,
                type_id: object.type_id,
                category: category_bit(object.category),
                relation: if object.own {
                    1
                } else if object.allied {
                    2
                } else {
                    4
                },
                state: u32::from(object.selected)
                    | u32::from(object.hovered) << 1
                    | u32::from(object.health.0 < object.health.1) << 2,
                values,
                slots: &[],
            })
            .collect();
        self.world.paint_status(&views, out);
    }

    fn values(&self, object: &StatusObject<'_>, vars: &VarStore) -> [Option<Amount>; SLOTS] {
        let mut values = [None; SLOTS];
        let amount = |(value, max): (i32, i32)| Some(Amount { value, max });
        values[usize::from(HEALTH)] = amount(object.health);
        values[usize::from(SHIELD)] = object.shield.and_then(amount);
        values[usize::from(AMMO)] = object.ammo.and_then(amount);
        values[usize::from(PASSENGERS)] = object.passengers.and_then(amount);
        // A disguise exposes only the worn identity and its health share.
        let Some(rank) = object.rank else {
            return values;
        };
        values[usize::from(RANK)] = amount((rank, 2));
        let read = |var: i32| -> Option<i32> {
            let (_, visibility, default) = vars.declared(var)?;
            let readable = match visibility {
                Visibility::Public | Visibility::InSight => true,
                Visibility::Allies => object.own || object.allied,
                Visibility::Private => object.own,
            };
            if !readable {
                return None;
            }
            let value = object
                .vars
                .iter()
                .find(|(id, _)| *id == var)
                .map_or(default, |(_, value)| *value);
            Some(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
        };
        for (slot, binding) in values[usize::from(FIRST_VAR)..].iter_mut().zip(&self.vars) {
            let Some(value) = read(binding.var) else {
                continue;
            };
            let max = match binding.max {
                VarMax::Fixed(max) => Some(max),
                VarMax::Var(var) => read(var),
            };
            *slot = max.map(|max| Amount { value, max });
        }
        values
    }
}

/// A `Var:` name that is a declared object-scope variable.
fn object_var(origin: StatusOrigin, name: &str, names: &Names<'_>) -> Result<i32, Problem> {
    let var = names
        .vars
        .find(origin.source(), name.trim())
        .ok_or_else(|| Problem::Bad(format!("no variable {name}")))?;
    match names.vars.declared(var) {
        Some((vera20k_wasm_ext::VarScope::Object, _, _)) => Ok(var),
        _ => Err(Problem::Bad(format!("{name} is not an object variable"))),
    }
}

const CATEGORY_NAMES: [&str; 4] = ["Vehicle", "Infantry", "Building", "Aircraft"];
const RELATION_NAMES: [&str; 3] = ["Own", "Ally", "Enemy"];
const STATE_NAMES: [&str; 3] = ["Selected", "Hovered", "Damaged"];
const HIDE_NAMES: [&str; 4] = ["HealthBars", "Pips", "Marks", "Groups"];

fn category_bit(category: crate::map::entities::EntityCategory) -> u32 {
    use crate::map::entities::EntityCategory as C;
    match category {
        C::Infantry => 2,
        C::Structure => 4,
        C::Aircraft => 8,
        _ => 1,
    }
}

fn bits(names: &[String], table: &[&str]) -> Result<u32, Problem> {
    names.iter().try_fold(0, |all, name| {
        table
            .iter()
            .position(|known| known.eq_ignore_ascii_case(name))
            .map(|at| all | 1 << at)
            .ok_or_else(|| Problem::Bad(format!("no {name} in {}", table.join(", "))))
    })
}

fn hide_bits(text: &str) -> Result<u8, String> {
    let names: Vec<String> = text.split(',').map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()).collect();
    bits(&names, &HIDE_NAMES)
        .map(|bits| bits as u8)
        .map_err(|problem| problem.to_string())
}

fn field(fields: &[String], at: usize) -> Result<&str, Problem> {
    fields
        .get(at)
        .map(String::as_str)
        .ok_or_else(|| Problem::Bad(format!("{} needs more than {at} values", fields[0])))
}

fn int(text: &str) -> Result<i32, Problem> {
    text.trim()
        .parse()
        .map_err(|_| Problem::Bad(format!("{text} is not a whole number")))
}

/// `#RRGGBB`, `#RRGGBBAA` or `none`.
fn color(text: &str) -> Result<Option<Rgba>, Problem> {
    let text = text.trim();
    if text.eq_ignore_ascii_case("none") {
        return Ok(None);
    }
    let hex = text
        .strip_prefix('#')
        .filter(|hex| matches!(hex.len(), 6 | 8))
        .ok_or_else(|| Problem::Bad(format!("{text} is not #RRGGBB")))?;
    let value =
        u32::from_str_radix(hex, 16).map_err(|_| Problem::Bad(format!("{text} is not #RRGGBB")))?;
    let value = if hex.len() == 6 { value << 8 | 0xFF } else { value };
    Ok(Some(Rgba(
        (value >> 24) as u8,
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
    )))
}

fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &text[prefix.len()..])
}

fn find_section<'a>(ini: &'a IniFile, name: &str) -> Option<&'a IniSection> {
    ini.section_names()
        .into_iter()
        .find(|held| held.eq_ignore_ascii_case(name))
        .and_then(|held| ini.section(held))
}

/// Whether `name` is a `[VeraStatus]` or `[VeraStatus.<name>]` section.
pub(crate) fn is_status_section(name: &str) -> bool {
    strip_prefix_ci(name, SECTION).is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use vera20k_gui::status::WorldOp;

    fn store() -> VarStore {
        VarStore::default()
    }

    fn names<'a>(vars: &'a VarStore, types: &'a dyn Fn(&str) -> Option<u32>) -> Names<'a> {
        Names { type_id: types, vars }
    }

    fn object(health: (i32, i32), own: bool) -> StatusObject<'static> {
        StatusObject {
            id: 7,
            anchor: (100, 50),
            type_id: 3,
            category: crate::map::entities::EntityCategory::Unit,
            own,
            allied: false,
            selected: false,
            hovered: false,
            health,
            shield: None,
            ammo: Some((2, 5)),
            passengers: None,
            rank: Some(0),
            vars: &[],
        }
    }

    fn paint(status: &ModuleStatus, objects: &[StatusObject<'_>]) -> Vec<WorldOp> {
        let mut out = Vec::new();
        status.paint(objects, &store(), &mut Vec::new(), &mut out);
        out.into_iter().map(|cmd| cmd.op).collect()
    }

    /// A template's bar fills by its amount in the band's colour from the
    /// anchor; its relation keeps it off another player's object; a
    /// replacement of the same name takes its place, and no elements
    /// removes it.
    #[test]
    fn a_text_template_paints_replaces_and_removes() {
        let vars = store();
        let types = |name: &str| (name == "MTNK").then_some(3);
        let names = names(&vars, &types);
        let mut status = ModuleStatus::default();
        status
            .add_text(
                StatusOrigin::MapModule,
                "Name=Hp\nRelation=Own\nTypes=MTNK\nElement1=Bar,-10,-20,20,2,Health,#000000,50:#00FF00,0:#FF0000",
                MapUi::Overlay,
                &names,
            )
            .expect("parses");
        assert_eq!(
            paint(&status, &[object((30, 40), true), object((30, 40), false)]),
            [
                WorldOp::Rect {
                    rect: Rect::new(90, 30, 20, 2),
                    color: Rgba(0, 0, 0, 255)
                },
                WorldOp::Rect {
                    rect: Rect::new(90, 30, 15, 2),
                    color: Rgba(0, 255, 0, 255)
                },
            ]
        );
        status
            .add_text(
                StatusOrigin::MapModule,
                "Name=Hp\nElement1=Number,0,0,Ammo,#FFFFFF",
                MapUi::Overlay,
                &names,
            )
            .expect("parses");
        assert!(matches!(
            status.add_text(
                StatusOrigin::MapModule,
                "Name=Hp\nHideNative=HealthBars\nElement1=Segments,0,0,1,1,1,0,65,Ammo,unknown.shp",
                MapUi::Full, &names,
            ),
            Err(Problem::Refused(_))
        ));
        assert_eq!(status.hidden(), 0);
        assert!(status.take_new_art().is_empty());
        assert_eq!(
            paint(&status, &[object((30, 40), false)]),
            [WorldOp::Number {
                rect: Rect::new(100, 50, 0, 0),
                value: 2,
                color: Rgba(255, 255, 255, 255)
            }]
        );
        status
            .add_text(StatusOrigin::MapModule, "Name=Hp", MapUi::Overlay, &names)
            .expect("parses");
        assert!(paint(&status, &[object((30, 40), true)]).is_empty());
    }

    /// `HideNative` is the mod's, and a map's only under `MapUI=full`; an
    /// unknown amount is a problem and keeps the template out.
    #[test]
    fn hiding_the_original_needs_the_mod_or_a_full_map() {
        let vars = store();
        let types = |_: &str| None;
        let names = names(&vars, &types);
        let mut status = ModuleStatus::default();
        let text = "Name=H\nHideNative=HealthBars,Marks";
        assert!(matches!(
            status.add_text(StatusOrigin::MapModule, text, MapUi::Overlay, &names),
            Err(Problem::Refused(_))
        ));
        assert_eq!(status.hidden(), 0);
        status
            .add_text(StatusOrigin::MapModule, text, MapUi::Full, &names)
            .expect("a full map may");
        assert_eq!(status.hidden(), hide::HEALTH_BARS | hide::MARKS);
        status.remove_origin(StatusOrigin::MapModule);
        assert_eq!(status.hidden(), 0);
        assert!(matches!(
            status.add_text(
                StatusOrigin::ModModule,
                "Name=X\nElement1=Number,0,0,Fuel,#FFFFFF",
                MapUi::Overlay,
                &names
            ),
            Err(Problem::Bad(_))
        ));
    }
}
