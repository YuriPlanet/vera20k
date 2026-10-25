//! The layout [`super::deserialize`] reads for a type, traced from the type's
//! `Deserialize` impl rather than from a value.
//!
//! [`describe`] decodes a `T` from made-up input that answers whatever the impl
//! asks for: zero numbers, empty strings, one element in every sequence and
//! map, `Some` for every option. It repeats the decode, steering each enum to a
//! variant it has not taken yet, until every variant of every reachable enum
//! has been decoded once. The description lists what each request read, so it
//! covers every field and variant the codec can meet, not only those one value
//! holds. Two builds with the same description read the same bytes alike.
//!
//! Type, field and variant names are part of the description, as serde reports
//! them, so a rename changes it although the bytes would not. Renaming a field
//! is how to mark a change in what an unchanged field means. Each Rust type is
//! traced on its own; types that share a serde name, such as a generic type's
//! instances, are numbered in the order the description first reaches them.
//!
//! Inside a recursion the made-up input holds no elements and `None`, so the
//! decode ends; the outer occurrence describes the recursive type. A type whose
//! impl checks or rebuilds what it decoded (lengths that must agree, ids that
//! must be unique) cannot accept made-up values: while [`describing`] is true
//! it returns a placeholder once its wire form is read. An impl that branches
//! on a decoded value is described along the branch its made-up value takes.

use std::any::type_name;
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{self, DeserializeOwned, DeserializeSeed, IntoDeserializer, Visitor};

/// Containers nested deeper than this are a recursion that does not end.
const MAX_DEPTH: usize = 256;
/// Each pass takes a new variant or completes a layout; a type that needs
/// more passes than this is not settling.
const MAX_PASSES: usize = 10_000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{at}: {message}")]
    Custom { at: String, message: String },
    #[error("{at}: {method} needs a self-describing format")]
    NotSelfDescribing { at: String, method: &'static str },
    #[error("{0} recurses without end")]
    Unbounded(String),
    #[error("tracing stopped with unexplored layouts in {0}")]
    Unsettled(String),
}

impl Error {
    fn not_self_describing(method: &'static str) -> Self {
        Self::NotSelfDescribing {
            at: String::new(),
            method,
        }
    }

    /// Name the field, element or type the error came through, outermost
    /// first once it reaches the root.
    fn within(self, segment: impl fmt::Display) -> Self {
        let nest = |at: String| {
            if at.is_empty() {
                segment.to_string()
            } else {
                format!("{segment} > {at}")
            }
        };
        match self {
            Self::Custom { at, message } => Self::Custom {
                at: nest(at),
                message,
            },
            Self::NotSelfDescribing { at, method } => Self::NotSelfDescribing {
                at: nest(at),
                method,
            },
            other => other,
        }
    }
}

impl de::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::Custom {
            at: String::new(),
            message: message.to_string(),
        }
    }
}

thread_local! {
    static DESCRIBING: Cell<bool> = const { Cell::new(false) };
}

/// Whether this thread is inside [`describe`], decoding made-up values.
pub fn describing() -> bool {
    DESCRIBING.get()
}

/// Sets [`DESCRIBING`] for one [`describe`] and restores it, also on unwind.
struct Describing(bool);

impl Describing {
    fn enter() -> Self {
        Self(DESCRIBING.replace(true))
    }
}

impl Drop for Describing {
    fn drop(&mut self) {
        DESCRIBING.set(self.0);
    }
}

/// Describe the layout `save_codec` reads for `T`, one line for the root and
/// one per struct or enum, in name order.
pub fn describe<T: DeserializeOwned>() -> Result<String, Error> {
    let _describing = Describing::enter();
    let mut tracer = Tracer::default();
    let mut root = Layout::Unknown;
    for _ in 0..MAX_PASSES {
        tracer.plan();
        tracer.learned = false;
        T::deserialize(Describer::new(&mut tracer, &mut root, false))?;
        if !tracer.learned {
            return tracer.finish(&root);
        }
    }
    let pending = tracer.pending.iter().map(|key| tracer.nodes[key].name);
    Err(Error::Unsettled(names(pending)))
}

fn names(names: impl IntoIterator<Item = &'static str>) -> String {
    let names: BTreeSet<&'static str> = names.into_iter().collect();
    names.into_iter().collect::<Vec<_>>().join(", ")
}

/// A struct's or enum's identity while tracing: the type name of the visitor
/// its impl passes, distinct for each Rust type and each generic instance. It
/// never enters the description.
type Key = &'static str;

/// One field's, element's or option's layout.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Layout {
    /// Not met yet: inside a recursion, collections are empty and options `None`.
    Unknown,
    Leaf(&'static str),
    Option(Box<Layout>),
    Seq(Box<Layout>),
    Map(Box<Layout>, Box<Layout>),
    Tuple(Vec<Layout>),
    /// A struct or enum, described once.
    Named(Key),
}

impl Layout {
    /// Fold in what another decode learned: `None` when the two disagree,
    /// otherwise whether anything new was learned.
    fn merge(&mut self, other: &Layout) -> Option<bool> {
        if *other == Layout::Unknown {
            return Some(false);
        }
        if *self == Layout::Unknown {
            *self = other.clone();
            return Some(true);
        }
        match (self, other) {
            (Layout::Leaf(this), Layout::Leaf(other)) => (this == other).then_some(false),
            (Layout::Named(this), Layout::Named(other)) => (this == other).then_some(false),
            (Layout::Option(this), Layout::Option(other))
            | (Layout::Seq(this), Layout::Seq(other)) => this.merge(other),
            (Layout::Map(key, value), Layout::Map(other_key, other_value)) => {
                Some(key.merge(other_key)? | value.merge(other_value)?)
            }
            (Layout::Tuple(this), Layout::Tuple(other)) => merge_all(this, other),
            _ => None,
        }
    }

    fn has_unknown(&self) -> bool {
        match self {
            Layout::Unknown => true,
            Layout::Leaf(_) | Layout::Named(_) => false,
            Layout::Option(inner) | Layout::Seq(inner) => inner.has_unknown(),
            Layout::Map(key, value) => key.has_unknown() || value.has_unknown(),
            Layout::Tuple(elements) => elements.iter().any(Layout::has_unknown),
        }
    }

    /// Whether any container this layout holds is in `keys`.
    fn refers_to(&self, keys: &BTreeSet<Key>) -> bool {
        match self {
            Layout::Unknown | Layout::Leaf(_) => false,
            Layout::Named(key) => keys.contains(key),
            Layout::Option(inner) | Layout::Seq(inner) => inner.refers_to(keys),
            Layout::Map(key, value) => key.refers_to(keys) || value.refers_to(keys),
            Layout::Tuple(elements) => elements.iter().any(|element| element.refers_to(keys)),
        }
    }

    /// Whether decoding this layout can reach another struct or enum.
    fn holds_container(&self) -> bool {
        match self {
            Layout::Unknown | Layout::Leaf(_) => false,
            Layout::Named(_) => true,
            Layout::Option(inner) | Layout::Seq(inner) => inner.holds_container(),
            Layout::Map(key, value) => key.holds_container() || value.holds_container(),
            Layout::Tuple(elements) => elements.iter().any(Layout::holds_container),
        }
    }
}

fn merge_all(this: &mut [Layout], other: &[Layout]) -> Option<bool> {
    if this.len() != other.len() {
        return None;
    }
    let mut learned = false;
    for (this, other) in this.iter_mut().zip(other) {
        learned |= this.merge(other)?;
    }
    Some(learned)
}

/// What a struct, or one enum variant, holds.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shape {
    Unit,
    Newtype(Layout),
    Tuple(Vec<Layout>),
    Struct(Vec<(&'static str, Layout)>),
}

impl Shape {
    fn merge(&mut self, other: &Shape) -> Option<bool> {
        match (self, other) {
            (Shape::Unit, Shape::Unit) => Some(false),
            (Shape::Newtype(this), Shape::Newtype(other)) => this.merge(other),
            (Shape::Tuple(this), Shape::Tuple(other)) => merge_all(this, other),
            (Shape::Struct(this), Shape::Struct(other)) => {
                if this.len() != other.len()
                    || this
                        .iter()
                        .zip(other)
                        .any(|((this, _), (other, _))| this != other)
                {
                    return None;
                }
                let mut learned = false;
                for ((_, this), (_, other)) in this.iter_mut().zip(other) {
                    learned |= this.merge(other)?;
                }
                Some(learned)
            }
            _ => None,
        }
    }

    fn layouts(&self) -> Vec<&Layout> {
        match self {
            Shape::Unit => Vec::new(),
            Shape::Newtype(inner) => vec![inner],
            Shape::Tuple(elements) => elements.iter().collect(),
            Shape::Struct(fields) => fields.iter().map(|(_, layout)| layout).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Container {
    Struct(Shape),
    Enum {
        variants: &'static [&'static str],
        shapes: BTreeMap<u32, Shape>,
    },
}

impl Container {
    fn merge(&mut self, other: &Container) -> Option<bool> {
        match (self, other) {
            (Container::Struct(this), Container::Struct(other)) => this.merge(other),
            (
                Container::Enum { variants, shapes },
                Container::Enum {
                    variants: other_variants,
                    shapes: other_shapes,
                },
            ) if variants == other_variants => {
                let mut learned = false;
                for (index, shape) in other_shapes {
                    match shapes.get_mut(index) {
                        Some(known) => learned |= known.merge(shape)?,
                        None => {
                            shapes.insert(*index, shape.clone());
                            learned = true;
                        }
                    }
                }
                Some(learned)
            }
            _ => None,
        }
    }

    fn shapes(&self) -> Vec<&Shape> {
        match self {
            Container::Struct(shape) => vec![shape],
            Container::Enum { shapes, .. } => shapes.values().collect(),
        }
    }

    /// Whether some variant or part of this container's own layout is unknown.
    fn incomplete(&self) -> bool {
        let unexplored = match self {
            Container::Struct(_) => false,
            Container::Enum { variants, shapes } => shapes.len() < variants.len(),
        };
        unexplored
            || self
                .shapes()
                .into_iter()
                .flat_map(Shape::layouts)
                .any(Layout::has_unknown)
    }

    fn refers_to(&self, keys: &BTreeSet<Key>) -> bool {
        self.shapes()
            .into_iter()
            .flat_map(Shape::layouts)
            .any(|layout| layout.refers_to(keys))
    }
}

/// One struct or enum type met while tracing.
struct Node {
    /// The serde name, which the description uses.
    name: &'static str,
    container: Container,
}

#[derive(Default)]
struct Tracer {
    nodes: BTreeMap<Key, Node>,
    /// The variants each enum has taken at least once.
    chosen: BTreeMap<Key, BTreeSet<u32>>,
    /// Containers with something left to learn in or below them, as of the
    /// start of this pass.
    pending: BTreeSet<Key>,
    /// The containers being decoded, outermost first.
    stack: Vec<(Key, &'static str)>,
    /// Whether this pass took a new variant or learned a layout.
    learned: bool,
}

impl Tracer {
    fn record(&mut self, key: Key, name: &'static str, container: Container) -> Result<(), Error> {
        let Some(node) = self.nodes.get_mut(key) else {
            self.nodes.insert(key, Node { name, container });
            self.learned = true;
            return Ok(());
        };
        // Only a value-dependent impl reads two layouts. The trace then
        // fails, so a half-merged node is never described.
        let learned = node
            .container
            .merge(&container)
            .ok_or_else(|| <Error as de::Error>::custom("two decodes read different layouts"))?;
        self.learned |= learned;
        Ok(())
    }

    /// Mark the containers that have something left to learn, and those
    /// that hold one of them.
    fn plan(&mut self) {
        let mut pending: BTreeSet<Key> = self
            .nodes
            .iter()
            .filter(|(_, node)| node.container.incomplete())
            .map(|(key, _)| *key)
            .collect();
        loop {
            let holders: Vec<Key> = self
                .nodes
                .iter()
                .filter(|(key, node)| !pending.contains(*key) && node.container.refers_to(&pending))
                .map(|(key, _)| *key)
                .collect();
            if holders.is_empty() {
                break;
            }
            pending.extend(holders);
        }
        self.pending = pending;
    }

    /// The variant this decode of the enum `key` takes: one it has not taken
    /// yet, else one with something left to learn below it, else one that
    /// holds no other container. Inside a recursion only the last applies,
    /// and before any is known each nesting level takes the next variant, so
    /// a recursive first variant still reaches one that ends.
    fn choose(&mut self, key: Key, count: usize, quiet: bool) -> u32 {
        let count = u32::try_from(count).unwrap_or(u32::MAX);
        if !quiet {
            let chosen = self.chosen.entry(key).or_default();
            if let Some(index) = (0..count).find(|index| !chosen.contains(index)) {
                chosen.insert(index);
                self.learned = true;
                return index;
            }
            let pending = &self.pending;
            if let Some(index) = self.variant_where(key, |shape| {
                shape
                    .layouts()
                    .into_iter()
                    .any(|layout| layout.has_unknown() || layout.refers_to(pending))
            }) {
                return index;
            }
        }
        self.variant_where(key, |shape| {
            !shape.layouts().into_iter().any(Layout::holds_container)
        })
        .unwrap_or_else(|| {
            let enclosing = self.stack.iter().filter(|(outer, _)| *outer == key).count();
            u32::try_from(enclosing.saturating_sub(1)).unwrap_or(0) % count
        })
    }

    fn variant_where(&self, key: Key, wanted: impl Fn(&Shape) -> bool) -> Option<u32> {
        match &self.nodes.get(key)?.container {
            Container::Enum { shapes, .. } => shapes
                .iter()
                .find(|(_, shape)| wanted(shape))
                .map(|(index, _)| *index),
            Container::Struct(_) => None,
        }
    }

    fn finish(&self, root: &Layout) -> Result<String, Error> {
        let incomplete = self
            .nodes
            .values()
            .filter(|node| node.container.incomplete())
            .map(|node| node.name);
        let incomplete = names(incomplete);
        if !incomplete.is_empty() || root.has_unknown() {
            return Err(Error::Unsettled(incomplete));
        }
        let labels = Labels::new(root, &self.nodes);
        let mut lines: Vec<String> = labels
            .0
            .iter()
            .map(|(key, label)| {
                let container = labels.container(&self.nodes[key].container);
                format!("{label} {container}\n")
            })
            .collect();
        lines.sort();
        Ok(format!("{}\n{}", labels.layout(root), lines.concat()))
    }
}

/// Decodes made-up input and records what each request read into `slot`.
struct Describer<'a> {
    tracer: &'a mut Tracer,
    slot: &'a mut Layout,
    /// Inside a recursion: collections are empty and options `None`, so the
    /// decode ends.
    quiet: bool,
}

impl<'a> Describer<'a> {
    fn new(tracer: &'a mut Tracer, slot: &'a mut Layout, quiet: bool) -> Self {
        Self {
            tracer,
            slot,
            quiet,
        }
    }

    /// Decode a struct or enum and record it under `key`, quietly when it
    /// already encloses itself.
    fn container<T>(
        self,
        key: Key,
        name: &'static str,
        decode: impl FnOnce(&mut Tracer, bool) -> Result<(T, Container), Error>,
    ) -> Result<T, Error> {
        let Self {
            tracer,
            slot,
            quiet,
        } = self;
        *slot = Layout::Named(key);
        if tracer.stack.len() >= MAX_DEPTH {
            let names: Vec<&str> = tracer.stack.iter().map(|(_, name)| *name).collect();
            return Err(Error::Unbounded(names.join(" > ")));
        }
        let quiet = quiet || tracer.stack.iter().any(|(outer, _)| *outer == key);
        tracer.stack.push((key, name));
        let decoded = decode(tracer, quiet);
        tracer.stack.pop();
        let (value, container) = decoded.map_err(|error| error.within(name))?;
        tracer
            .record(key, name, container)
            .map_err(|error| error.within(name))?;
        Ok(value)
    }
}

macro_rules! leaves {
    ($($method:ident => $visit:ident($value:expr): $name:literal),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            *self.slot = Layout::Leaf($name);
            visitor.$visit($value)
        }
    )*};
}

impl<'de> de::Deserializer<'de> for Describer<'_> {
    type Error = Error;

    leaves! {
        deserialize_bool => visit_bool(false): "bool",
        deserialize_u8 => visit_u8(0): "u8", deserialize_u16 => visit_u16(0): "u16",
        deserialize_u32 => visit_u32(0): "u32", deserialize_u64 => visit_u64(0): "u64",
        deserialize_u128 => visit_u128(0): "u128", deserialize_i8 => visit_i8(0): "i8",
        deserialize_i16 => visit_i16(0): "i16", deserialize_i32 => visit_i32(0): "i32",
        deserialize_i64 => visit_i64(0): "i64", deserialize_i128 => visit_i128(0): "i128",
        deserialize_f32 => visit_f32(0.0): "f32", deserialize_f64 => visit_f64(0.0): "f64",
        deserialize_char => visit_char('\0'): "char",
        deserialize_str => visit_borrowed_str(""): "str",
        deserialize_string => visit_string(String::new()): "str",
        deserialize_bytes => visit_borrowed_bytes(&[]): "bytes",
        deserialize_byte_buf => visit_byte_buf(Vec::new()): "bytes",
    }

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::not_self_describing("deserialize_any"))
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::not_self_describing("deserialize_ignored_any"))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::not_self_describing("deserialize_identifier"))
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let Self {
            tracer,
            slot,
            quiet,
        } = self;
        let mut inner = Layout::Unknown;
        let value = if quiet {
            visitor.visit_none()
        } else {
            visitor.visit_some(Describer::new(tracer, &mut inner, false))
        };
        *slot = Layout::Option(Box::new(inner));
        value
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        *self.slot = Layout::Leaf("unit");
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.container(type_name::<V>(), name, |_, _| {
            Ok((visitor.visit_unit()?, Container::Struct(Shape::Unit)))
        })
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.container(type_name::<V>(), name, |tracer, quiet| {
            let mut inner = Layout::Unknown;
            let value = visitor.visit_newtype_struct(Describer::new(tracer, &mut inner, quiet))?;
            Ok((value, Container::Struct(Shape::Newtype(inner))))
        })
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let Self {
            tracer,
            slot,
            quiet,
        } = self;
        let mut element = Layout::Unknown;
        let value = visitor.visit_seq(Elements {
            tracer,
            slot: (!quiet).then_some(&mut element),
        });
        *slot = Layout::Seq(Box::new(element));
        value
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        let Self {
            tracer,
            slot,
            quiet,
        } = self;
        let mut elements = vec![Layout::Unknown; len];
        let value = visitor.visit_seq(Fields::new(tracer, &mut elements, None, quiet));
        *slot = Layout::Tuple(elements);
        value
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.container(type_name::<V>(), name, |tracer, quiet| {
            let mut elements = vec![Layout::Unknown; len];
            let value = visitor.visit_seq(Fields::new(tracer, &mut elements, None, quiet))?;
            Ok((value, Container::Struct(Shape::Tuple(elements))))
        })
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let Self {
            tracer,
            slot,
            quiet,
        } = self;
        let (mut key, mut value) = (Layout::Unknown, Layout::Unknown);
        let map = visitor.visit_map(Entries {
            tracer,
            key: (!quiet).then_some(&mut key),
            value: Some(&mut value),
        });
        *slot = Layout::Map(Box::new(key), Box::new(value));
        map
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.container(type_name::<V>(), name, |tracer, quiet| {
            let mut layouts = vec![Layout::Unknown; fields.len()];
            let value =
                visitor.visit_seq(Fields::new(tracer, &mut layouts, Some(fields), quiet))?;
            let fields = fields.iter().copied().zip(layouts).collect();
            Ok((value, Container::Struct(Shape::Struct(fields))))
        })
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        let key = type_name::<V>();
        self.container(key, name, |tracer, quiet| {
            if variants.is_empty() {
                return Err(de::Error::custom("an enum without variants has no value"));
            }
            let index = tracer.choose(key, variants.len(), quiet);
            let mut shape = Shape::Unit;
            let value = visitor
                .visit_enum(Choice {
                    tracer,
                    index,
                    shape: &mut shape,
                    quiet,
                })
                .map_err(|error| error.within(variants[index as usize]))?;
            let shapes = BTreeMap::from([(index, shape)]);
            Ok((value, Container::Enum { variants, shapes }))
        })
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

/// A tuple's or struct's fields: exactly one per layout.
struct Fields<'a> {
    tracer: &'a mut Tracer,
    layouts: std::slice::IterMut<'a, Layout>,
    names: Option<&'static [&'static str]>,
    index: usize,
    quiet: bool,
}

impl<'a> Fields<'a> {
    fn new(
        tracer: &'a mut Tracer,
        layouts: &'a mut [Layout],
        names: Option<&'static [&'static str]>,
        quiet: bool,
    ) -> Self {
        Self {
            tracer,
            layouts: layouts.iter_mut(),
            names,
            index: 0,
            quiet,
        }
    }
}

impl<'de> de::SeqAccess<'de> for Fields<'_> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        let Some(slot) = self.layouts.next() else {
            return Ok(None);
        };
        let index = self.index;
        self.index += 1;
        seed.deserialize(Describer::new(&mut *self.tracer, slot, self.quiet))
            .map(Some)
            .map_err(|error| match self.names {
                Some(names) => error.within(names[index]),
                None => error.within(index),
            })
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.layouts.len())
    }
}

/// A sequence's elements: one, or none inside a recursion.
struct Elements<'a> {
    tracer: &'a mut Tracer,
    slot: Option<&'a mut Layout>,
}

impl<'de> de::SeqAccess<'de> for Elements<'_> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        let Some(slot) = self.slot.take() else {
            return Ok(None);
        };
        seed.deserialize(Describer::new(&mut *self.tracer, slot, false))
            .map(Some)
            .map_err(|error| error.within("[]"))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(usize::from(self.slot.is_some()))
    }
}

/// A map's entries: one, or none inside a recursion.
struct Entries<'a> {
    tracer: &'a mut Tracer,
    key: Option<&'a mut Layout>,
    value: Option<&'a mut Layout>,
}

impl<'de> de::MapAccess<'de> for Entries<'_> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        let Some(slot) = self.key.take() else {
            return Ok(None);
        };
        seed.deserialize(Describer::new(&mut *self.tracer, slot, false))
            .map(Some)
            .map_err(|error| error.within("{key}"))
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        let slot = self.value.take().ok_or_else(|| {
            <Error as de::Error>::custom("a map value was requested without its key")
        })?;
        seed.deserialize(Describer::new(&mut *self.tracer, slot, false))
            .map_err(|error| error.within("{value}"))
    }

    fn size_hint(&self) -> Option<usize> {
        Some(usize::from(self.key.is_some()))
    }
}

/// The variant one enum decode takes, recording what it holds into `shape`.
struct Choice<'a> {
    tracer: &'a mut Tracer,
    index: u32,
    shape: &'a mut Shape,
    quiet: bool,
}

impl<'de> de::EnumAccess<'de> for Choice<'_> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), Error> {
        let variant: Result<V::Value, Error> = seed.deserialize(self.index.into_deserializer());
        Ok((variant?, self))
    }
}

impl<'de> de::VariantAccess<'de> for Choice<'_> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        *self.shape = Shape::Unit;
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        let mut inner = Layout::Unknown;
        let value = seed.deserialize(Describer::new(self.tracer, &mut inner, self.quiet));
        *self.shape = Shape::Newtype(inner);
        value
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        let mut elements = vec![Layout::Unknown; len];
        let value = visitor.visit_seq(Fields::new(self.tracer, &mut elements, None, self.quiet));
        *self.shape = Shape::Tuple(elements);
        value
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        let mut layouts = vec![Layout::Unknown; fields.len()];
        let value = visitor.visit_seq(Fields::new(
            self.tracer,
            &mut layouts,
            Some(fields),
            self.quiet,
        ));
        *self.shape = Shape::Struct(fields.iter().copied().zip(layouts).collect());
        value
    }
}

/// Each container's name in the description: its serde name, numbered when
/// several types share it, in the order a walk from the root first reaches them.
struct Labels(BTreeMap<Key, String>);

impl Labels {
    fn new(root: &Layout, nodes: &BTreeMap<Key, Node>) -> Self {
        let mut order = Vec::new();
        reach(root, nodes, &mut BTreeSet::new(), &mut order);
        let mut sharing = BTreeMap::<&str, usize>::new();
        for key in &order {
            *sharing.entry(nodes[key].name).or_default() += 1;
        }
        let mut numbered = BTreeMap::<&str, usize>::new();
        let labels = order
            .into_iter()
            .map(|key| {
                let name = nodes[key].name;
                if sharing[name] == 1 {
                    return (key, name.to_string());
                }
                let number = numbered.entry(name).or_default();
                *number += 1;
                (key, format!("{name}#{number}"))
            })
            .collect();
        Self(labels)
    }

    fn layout(&self, layout: &Layout) -> String {
        match layout {
            Layout::Unknown => "?".to_string(),
            Layout::Leaf(name) => (*name).to_string(),
            Layout::Named(key) => self.0[key].clone(),
            Layout::Option(inner) => format!("option<{}>", self.layout(inner)),
            Layout::Seq(inner) => format!("seq<{}>", self.layout(inner)),
            Layout::Map(key, value) => format!("map<{},{}>", self.layout(key), self.layout(value)),
            Layout::Tuple(elements) => format!("({})", self.list(elements)),
        }
    }

    /// Comma-separated, without spaces, so descriptions compare as plain text.
    fn list(&self, layouts: &[Layout]) -> String {
        let layouts: Vec<String> = layouts.iter().map(|layout| self.layout(layout)).collect();
        layouts.join(",")
    }

    fn shape(&self, shape: &Shape) -> String {
        match shape {
            Shape::Unit => String::new(),
            Shape::Newtype(inner) => format!("({})", self.layout(inner)),
            Shape::Tuple(elements) => format!("({})", self.list(elements)),
            Shape::Struct(fields) => {
                let fields: Vec<String> = fields
                    .iter()
                    .map(|(name, layout)| format!("{name}:{}", self.layout(layout)))
                    .collect();
                format!("{{{}}}", fields.join(","))
            }
        }
    }

    fn container(&self, container: &Container) -> String {
        match container {
            Container::Struct(shape) => format!("struct{}", self.shape(shape)),
            Container::Enum { variants, shapes } => {
                let variants: Vec<String> = variants
                    .iter()
                    .zip(0u32..)
                    .map(|(name, index)| match shapes.get(&index) {
                        Some(shape) => format!("{name}{}", self.shape(shape)),
                        None => format!("{name}?"),
                    })
                    .collect();
                format!("enum{{{}}}", variants.join(","))
            }
        }
    }
}

/// Append each container `layout` reaches to `order`, depth first, with
/// fields and variants in order.
fn reach(
    layout: &Layout,
    nodes: &BTreeMap<Key, Node>,
    seen: &mut BTreeSet<Key>,
    order: &mut Vec<Key>,
) {
    match layout {
        Layout::Unknown | Layout::Leaf(_) => {}
        Layout::Named(key) => {
            if seen.insert(*key) {
                order.push(*key);
                for shape in nodes[key].container.shapes() {
                    for layout in shape.layouts() {
                        reach(layout, nodes, seen, order);
                    }
                }
            }
        }
        Layout::Option(inner) | Layout::Seq(inner) => reach(inner, nodes, seen, order),
        Layout::Map(key, value) => {
            reach(key, nodes, seen, order);
            reach(value, nodes, seen, order);
        }
        Layout::Tuple(elements) => {
            for element in elements {
                reach(element, nodes, seen, order);
            }
        }
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
