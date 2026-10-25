//! The traced types below are only ever built by `describe`.
#![allow(dead_code)]

use std::collections::BTreeMap;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer};

use super::{Error, describe, describing};

#[derive(Deserialize)]
struct Sample {
    flag: bool,
    word: u32,
    text: String,
    maybe: Option<i8>,
    list: Vec<u16>,
    pair: (u8, char),
    map: BTreeMap<u8, Pair>,
    shape: Shape,
    unit: Unit,
}

#[derive(Deserialize)]
struct Pair(u8, i16);

#[derive(Deserialize)]
struct Unit;

#[derive(Deserialize)]
enum Shape {
    Dot,
    Wrapped(u16),
    Pair(i8, bool),
    Corner { x: i32, inner: Inner },
}

/// Reachable only through `Shape::Corner`, the last variant.
#[derive(Deserialize)]
enum Inner {
    First,
    Second(f64),
}

#[test]
fn describes_every_field_and_every_variant_below_a_later_variant() {
    assert_eq!(
        describe::<Sample>().unwrap(),
        "Sample\n\
         Inner enum{First,Second(f64)}\n\
         Pair struct(u8,i16)\n\
         Sample struct{flag:bool,word:u32,text:str,maybe:option<i8>,list:seq<u16>,\
         pair:(u8,char),map:map<u8,Pair>,shape:Shape,unit:Unit}\n\
         Shape enum{Dot,Wrapped(u16),Pair(i8,bool),Corner{x:i32,inner:Inner}}\n\
         Unit struct\n"
    );
}

mod base {
    #[derive(serde::Deserialize)]
    #[serde(rename = "Record")]
    pub struct Record {
        pub a: u32,
        pub b: u16,
        pub kind: super::Kind,
    }
}

mod widened {
    #[derive(serde::Deserialize)]
    #[serde(rename = "Record")]
    pub struct Record {
        pub a: u64,
        pub b: u16,
        pub kind: super::Kind,
    }
}

mod reordered {
    #[derive(serde::Deserialize)]
    #[serde(rename = "Record")]
    pub struct Record {
        pub b: u16,
        pub a: u32,
        pub kind: super::Kind,
    }
}

mod renamed {
    #[derive(serde::Deserialize)]
    #[serde(rename = "Record")]
    pub struct Record {
        pub a: u32,
        pub c: u16,
        pub kind: super::Kind,
    }
}

mod extended {
    #[derive(serde::Deserialize)]
    #[serde(rename = "Record")]
    pub struct Record {
        pub a: u32,
        pub b: u16,
        pub kind: Kind,
    }

    #[derive(serde::Deserialize)]
    pub enum Kind {
        Plain,
        Tagged(u8),
        Extra,
    }
}

#[derive(Deserialize)]
enum Kind {
    Plain,
    Tagged(u8),
}

#[test]
fn a_field_or_variant_change_changes_the_description() {
    let base = describe::<base::Record>().unwrap();
    for changed in [
        describe::<widened::Record>(),
        describe::<reordered::Record>(),
        describe::<renamed::Record>(),
        describe::<extended::Record>(),
    ] {
        assert_ne!(changed.unwrap(), base);
    }
    assert_eq!(describe::<base::Record>().unwrap(), base);
}

#[derive(Deserialize)]
struct Tree {
    label: u8,
    children: Vec<Tree>,
    parent: Option<Box<Tree>>,
}

/// The first variant recurses, so a decode that always took it would not end.
#[derive(Deserialize)]
enum Expr {
    Not(Box<Expr>),
    Both(Box<Expr>, Box<Expr>),
    Leaf(u8),
}

#[test]
fn describes_recursive_types_once() {
    assert_eq!(
        describe::<Tree>().unwrap(),
        "Tree\nTree struct{label:u8,children:seq<Tree>,parent:option<Tree>}\n"
    );
    assert_eq!(
        describe::<Expr>().unwrap(),
        "Expr\nExpr enum{Not(Expr),Both(Expr,Expr),Leaf(u8)}\n"
    );
}

#[derive(Deserialize)]
struct Slot<T> {
    value: T,
}

#[derive(Deserialize)]
struct Holder {
    small: Slot<u8>,
    large: Slot<u64>,
    nested: Slot<Slot<Option<u8>>>,
}

/// `Holder` with its first two fields' instances swapped.
#[derive(Deserialize)]
#[serde(rename = "Holder")]
struct Swapped {
    small: Slot<u64>,
    large: Slot<u8>,
    nested: Slot<Slot<Option<u8>>>,
}

#[test]
fn tells_apart_types_that_share_a_name() {
    let holder = describe::<Holder>().unwrap();
    assert_eq!(
        holder,
        "Holder\n\
         Holder struct{small:Slot#1,large:Slot#2,nested:Slot#3}\n\
         Slot#1 struct{value:u8}\n\
         Slot#2 struct{value:u64}\n\
         Slot#3 struct{value:Slot#4}\n\
         Slot#4 struct{value:option<u8>}\n"
    );
    assert_ne!(describe::<Swapped>().unwrap(), holder);
}

/// Valid only with exactly three bytes, which made-up input never holds.
struct Checked(Vec<u8>);

impl<'de> Deserialize<'de> for Checked {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = Vec::<u8>::deserialize(deserializer)?;
        if describing() {
            return Ok(Self(Vec::new()));
        }
        if bytes.len() != 3 {
            return Err(D::Error::custom("needs exactly three bytes"));
        }
        Ok(Self(bytes))
    }
}

#[derive(Deserialize)]
struct HoldsChecked {
    checked: Checked,
}

struct Refuses;

impl<'de> Deserialize<'de> for Refuses {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        u8::deserialize(deserializer)?;
        Err(D::Error::custom("never valid"))
    }
}

#[derive(Deserialize)]
struct Outer {
    inner: Middle,
}

#[derive(Deserialize)]
struct Middle {
    list: Vec<Refuses>,
}

#[test]
fn a_checking_impl_returns_a_placeholder_while_describing() {
    assert!(!describing());
    assert_eq!(
        describe::<HoldsChecked>().unwrap(),
        "HoldsChecked\nHoldsChecked struct{checked:seq<u8>}\n"
    );
    assert!(!describing());
    assert_eq!(
        describe::<Outer>().unwrap_err().to_string(),
        "Outer > inner > Middle > list > []: never valid"
    );
    assert!(!describing());
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Loose {
    Number(u8),
    Text(String),
}

#[derive(Deserialize)]
struct HoldsLoose {
    loose: Loose,
}

#[test]
fn refuses_what_the_codec_refuses() {
    assert!(matches!(
        describe::<HoldsLoose>(),
        Err(Error::NotSelfDescribing {
            method: "deserialize_any",
            ..
        })
    ));
}
