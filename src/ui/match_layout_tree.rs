//! `veraui.ini` as a tree: each `[Layout.Part]` section is a node, each key
//! a property, with what a layout starts from written in.
//!
//! The files lie in layers: the built-in `veraui_default.ini` at the bottom,
//! then the mod's and the player's. A layout starts from its `Base=` layout,
//! or, with none, from the position table its side uses (`GDI` or `Nod`, the
//! built-in layouts, which start from nothing). A node's `Base=` starts it
//! from another node of the layout. `Key@Allied`, `Key@Soviet` and
//! `Key@Yuri` hold for that side's pictures only.
//!
//! This module knows no part: it reads names and texts. `ui::match_layout`
//! reads the parts' properties out of the tree.
//!
//! ## Dependency rules
//! - Part of ui/ — reads the rules/ INI store; no app/ or render/.

use crate::rules::ini_parser::IniFile;

/// The built-in layouts: the two position tables.
pub(super) const TABLES: [&str; 2] = ["GDI", "Nod"];

/// The sides' pictures a key can hold for alone, as `Key@Side` writes them.
const THEMES: [&str; 3] = ["Allied", "Soviet", "Yuri"];

/// ReadString capacity of one value: a name or a list of names.
pub(super) const VALUE_CAPACITY: usize = 0x800;

/// How far `Base=` lines are followed before they count as a loop.
const MOST_BASES: usize = 16;

/// One value a file writes for a key.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Written {
    /// The side's pictures it holds for (an index of [`THEMES`]), or every
    /// side's.
    pub theme: Option<usize>,
    pub text: String,
    /// The section that writes it, for messages about it.
    pub section: String,
    /// A file over the built-in one wrote it: a value the player is told
    /// about when it cannot be used.
    pub own: bool,
}

/// One part of a layout: its name and its keys, each with every value
/// written for it, the later over the earlier.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Node {
    pub name: String,
    pub keys: Vec<(String, Vec<Written>)>,
    /// The built-in file defines this node under its own name.
    built_in: bool,
}

impl Node {
    /// Every value written for `key`, the last the one that holds.
    pub fn values(&self, key: &str) -> &[Written] {
        self.keys
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map_or(&[], |(_, values)| values.as_slice())
    }

    /// The text that holds for `key` on every side.
    pub fn text(&self, key: &str) -> Option<&str> {
        self.values(key)
            .iter()
            .rev()
            .find(|value| value.theme.is_none())
            .map(|value| value.text.as_str())
    }

    /// The text that holds for `key` on the side whose pictures are theme
    /// `theme` (an index of [`THEMES`]): its own, else every side's.
    pub fn text_for(&self, key: &str, theme: usize) -> Option<&str> {
        let values = self.values(key);
        values
            .iter()
            .rev()
            .find(|value| value.theme == Some(theme))
            .or_else(|| values.iter().rev().find(|value| value.theme.is_none()))
            .map(|value| value.text.as_str())
    }

    /// Whether a built-in node: the original interface has it.
    pub fn built_in(&self) -> bool {
        self.built_in
    }

    /// The section that wrote the node, as messages name it: its first own
    /// value's.
    pub fn section(&self) -> Option<&str> {
        self.keys
            .iter()
            .flat_map(|(_, values)| values)
            .find(|value| value.own)
            .map(|value| value.section.as_str())
    }

    fn write(&mut self, key: &str, value: Written) {
        match self
            .keys
            .iter_mut()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
        {
            Some((_, values)) => values.push(value),
            None => self.keys.push((key.to_owned(), vec![value])),
        }
    }
}

/// Why the tree of a layout could not be read as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TreeProblem {
    /// `Base=` names a layout or a node there is none of.
    MissingBase { section: String },
    /// `Base=` lines come back to where they started.
    Loop { section: String },
    /// A key of a side whose name is not one of [`THEMES`].
    UnknownSide { section: String, key: String },
}

/// One layout with its bases written in.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct Tree {
    pub nodes: Vec<Node>,
}

impl Tree {
    pub fn node(&self, name: &str) -> Option<&Node> {
        self.nodes
            .iter()
            .find(|node| node.name.eq_ignore_ascii_case(name))
    }

    /// Layout `layout` of `files`, the bottom layer first; `table` is the
    /// position table it starts from when it names no base.
    pub fn read(
        files: &[&IniFile],
        layout: &str,
        table: &str,
        problems: &mut Vec<TreeProblem>,
    ) -> Self {
        let mut tree = Self::default();
        for name in chain(files, layout, table, problems).iter().rev() {
            for (layer, file) in files.iter().enumerate() {
                let names = file.section_names();
                for (position, &section_name) in names.iter().enumerate() {
                    // A repeated section name reads as its first copy, once.
                    if names[..position].contains(&section_name) {
                        continue;
                    }
                    let Some(node) = part_of(section_name, name).filter(|node| !node.is_empty())
                    else {
                        continue;
                    };
                    let Some(section) = file.section(section_name) else {
                        continue;
                    };
                    let index = match tree
                        .nodes
                        .iter()
                        .position(|known| known.name.eq_ignore_ascii_case(node))
                    {
                        Some(index) => index,
                        None => {
                            tree.nodes.push(Node {
                                name: node.to_owned(),
                                keys: Vec::new(),
                                built_in: false,
                            });
                            tree.nodes.len() - 1
                        }
                    };
                    let node = &mut tree.nodes[index];
                    node.built_in |= layer == 0;
                    for key in section.keys() {
                        let text = section.read_name(key, VALUE_CAPACITY).unwrap_or_default();
                        let (key, theme) = match key.split_once('@') {
                            None => (key, None),
                            Some((key, side)) => match THEMES
                                .iter()
                                .position(|theme| theme.eq_ignore_ascii_case(side.trim()))
                            {
                                Some(theme) => (key, Some(theme)),
                                None => {
                                    problems.push(TreeProblem::UnknownSide {
                                        section: section_name.to_owned(),
                                        key: key.to_owned(),
                                    });
                                    continue;
                                }
                            },
                        };
                        node.write(
                            key.trim(),
                            Written {
                                theme,
                                text: text.trim().to_owned(),
                                section: section_name.to_owned(),
                                own: layer > 0,
                            },
                        );
                    }
                }
            }
        }
        tree.inherit(problems);
        tree
    }

    /// Give each node with a `Base=` the keys of that node, and of its base
    /// in turn, that it does not write itself.
    fn inherit(&mut self, problems: &mut Vec<TreeProblem>) {
        for index in 0..self.nodes.len() {
            let mut seen = vec![index];
            let mut from = index;
            while let Some(base) = self.nodes[from].text("Base") {
                let section = self.nodes[from]
                    .values("Base")
                    .last()
                    .map(|value| value.section.clone())
                    .unwrap_or_default();
                let Some(next) = self
                    .nodes
                    .iter()
                    .position(|node| node.name.eq_ignore_ascii_case(base))
                else {
                    problems.push(TreeProblem::MissingBase { section });
                    break;
                };
                if seen.contains(&next) || seen.len() > MOST_BASES {
                    problems.push(TreeProblem::Loop { section });
                    break;
                }
                seen.push(next);
                let inherited: Vec<(String, Vec<Written>)> = self.nodes[next]
                    .keys
                    .iter()
                    .filter(|(key, _)| {
                        !key.eq_ignore_ascii_case("Base")
                            && self.nodes[index].values(key).is_empty()
                    })
                    .cloned()
                    .collect();
                self.nodes[index].keys.extend(inherited);
                from = next;
            }
        }
    }
}

/// The layouts `layout` starts from, itself first: each `Base=` in turn,
/// and the position table `table` after a layout that names none.
fn chain(
    files: &[&IniFile],
    layout: &str,
    table: &str,
    problems: &mut Vec<TreeProblem>,
) -> Vec<String> {
    let mut chain: Vec<String> = Vec::new();
    let mut next = Some(layout.to_owned());
    while let Some(name) = next.take() {
        // The two stock tables can still supply defaults at the depth limit
        // (Nod itself starts from GDI). Each can enter the chain only once.
        let a_table = is_table(&name);
        if chain.iter().any(|seen| seen.eq_ignore_ascii_case(&name))
            || (!a_table && chain.len() > MOST_BASES)
        {
            problems.push(TreeProblem::Loop {
                section: chain.last().cloned().unwrap_or_default(),
            });
            next = next_stock_base(&chain, table);
            continue;
        }
        let base = files.iter().rev().find_map(|file| {
            file.section_names()
                .into_iter()
                .find(|section| section.eq_ignore_ascii_case(&name))
                .and_then(|section| file.section(section))
                .and_then(|section| {
                    section
                        .keys()
                        .find(|key| key.eq_ignore_ascii_case("Base"))
                        .and_then(|key| section.read_name(key, VALUE_CAPACITY))
                        .map(|base| base.trim().to_owned())
                })
        });
        let base = match base {
            Some(base) if !exists(files, &base) => {
                problems.push(TreeProblem::MissingBase {
                    section: name.clone(),
                });
                None
            }
            other => other,
        };
        chain.push(name);
        next = base.or_else(|| next_stock_base(&chain, table));
    }
    chain
}

/// Custom layouts default to the side's table. Nod only contains the ways
/// it differs from GDI, even when a mod gives it a different `Base=`.
fn next_stock_base(chain: &[String], table: &str) -> Option<String> {
    let [gdi, nod] = TABLES;
    let contains = |table: &str| chain.iter().any(|name| name.eq_ignore_ascii_case(table));
    if contains(gdi) {
        None
    } else if contains(nod) {
        Some(gdi.to_owned())
    } else {
        Some(table.to_owned())
    }
}

fn is_table(layout: &str) -> bool {
    TABLES
        .iter()
        .any(|known| known.eq_ignore_ascii_case(layout))
}

/// Whether any of `files` has a section of layout `layout`.
pub(super) fn exists(files: &[&IniFile], layout: &str) -> bool {
    files.iter().any(|file| {
        file.section_names()
            .into_iter()
            .any(|section| part_of(section, layout).is_some())
    })
}

/// Split `Layout.Part` section names of `layout`; the layout's own section
/// gives an empty part.
pub(super) fn part_of<'a>(section: &'a str, layout: &str) -> Option<&'a str> {
    let head = section.get(..layout.len())?;
    if !head.eq_ignore_ascii_case(layout) {
        return None;
    }
    match &section[layout.len()..] {
        "" => Some(""),
        rest => rest.strip_prefix('.'),
    }
}
