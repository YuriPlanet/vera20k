//! Binary serde format for save games and state round trips.
//!
//! Writes exactly the bytes `bincode::serialize` 1.3 wrote with its default
//! (legacy) options, which saves used before this module replaced the crate:
//!
//! - integers and floats are fixed-width little-endian; `bool` is one byte and
//!   only 0 and 1 decode;
//! - `char` is its UTF-8 bytes;
//! - strings, byte strings, sequences and maps start with a `u64` length;
//! - `Option` is a 0/1 tag byte, then the value;
//! - an enum variant is its `u32` index, then its fields;
//! - tuples, arrays and structs are their fields in order, without a length;
//!   unit types write nothing.
//!
//! The format does not describe itself: a struct decodes by reading every field
//! in declaration order, so `deserialize_any`, `deserialize_ignored_any` and
//! identifiers are refused. A value decodes from a prefix of its input; the
//! save header is read that way without decoding the body behind it.

use std::fmt;

use serde::de::{self, DeserializeSeed, Deserializer as _, IntoDeserializer, Visitor};
use serde::ser::{self, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unexpected end of input")]
    UnexpectedEnd,
    #[error("invalid bool byte {0}")]
    InvalidBool(u8),
    #[error("invalid Option tag {0}")]
    InvalidOptionTag(u8),
    #[error("invalid char encoding")]
    InvalidChar,
    #[error("invalid UTF-8 string: {0}")]
    InvalidUtf8(#[from] std::str::Utf8Error),
    #[error("length {0} does not fit in memory")]
    LengthOverflow(u64),
    #[error("a sequence or map must know its length before its first element")]
    UnknownLength,
    #[error("{0} needs a self-describing format")]
    NotSelfDescribing(&'static str),
    #[error("{0}")]
    Custom(String),
}

impl ser::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::Custom(message.to_string())
    }
}

impl de::Error for Error {
    fn custom<T: fmt::Display>(message: T) -> Self {
        Self::Custom(message.to_string())
    }
}

pub fn serialize<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, Error> {
    let mut serializer = Serializer { output: Vec::new() };
    value.serialize(&mut serializer)?;
    Ok(serializer.output)
}

/// Decode a `T` from the start of `bytes`; any bytes after it are ignored.
pub fn deserialize<'de, T: serde::Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Error> {
    T::deserialize(&mut Deserializer { input: bytes })
}

struct Serializer {
    output: Vec<u8>,
}

impl Serializer {
    fn write_len(&mut self, len: Option<usize>) -> Result<(), Error> {
        let len = len.ok_or(Error::UnknownLength)?;
        self.output.extend_from_slice(&(len as u64).to_le_bytes());
        Ok(())
    }

    fn write_variant(&mut self, index: u32) {
        self.output.extend_from_slice(&index.to_le_bytes());
    }
}

macro_rules! serialize_le {
    ($($method:ident: $ty:ty),* $(,)?) => {$(
        fn $method(self, value: $ty) -> Result<(), Error> {
            self.output.extend_from_slice(&value.to_le_bytes());
            Ok(())
        }
    )*};
}

impl ser::Serializer for &mut Serializer {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    serialize_le! {
        serialize_u8: u8, serialize_u16: u16, serialize_u32: u32, serialize_u64: u64,
        serialize_u128: u128, serialize_i8: i8, serialize_i16: i16, serialize_i32: i32,
        serialize_i64: i64, serialize_i128: i128, serialize_f32: f32, serialize_f64: f64,
    }

    fn serialize_bool(self, value: bool) -> Result<(), Error> {
        self.output.push(u8::from(value));
        Ok(())
    }

    fn serialize_char(self, value: char) -> Result<(), Error> {
        let mut utf8 = [0; 4];
        self.output
            .extend_from_slice(value.encode_utf8(&mut utf8).as_bytes());
        Ok(())
    }

    fn serialize_str(self, value: &str) -> Result<(), Error> {
        self.serialize_bytes(value.as_bytes())
    }

    fn serialize_bytes(self, value: &[u8]) -> Result<(), Error> {
        self.write_len(Some(value.len()))?;
        self.output.extend_from_slice(value);
        Ok(())
    }

    fn serialize_none(self) -> Result<(), Error> {
        self.output.push(0);
        Ok(())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), Error> {
        self.output.push(1);
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<(), Error> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
    ) -> Result<(), Error> {
        self.write_variant(index);
        Ok(())
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        self.write_variant(index);
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self, Error> {
        self.write_len(len)?;
        Ok(self)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self, Error> {
        Ok(self)
    }

    fn serialize_tuple_struct(self, _name: &'static str, _len: usize) -> Result<Self, Error> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self, Error> {
        self.write_variant(index);
        Ok(self)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self, Error> {
        self.write_len(len)?;
        Ok(self)
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self, Error> {
        Ok(self)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self, Error> {
        self.write_variant(index);
        Ok(self)
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

impl ser::SerializeSeq for &mut Serializer {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeTuple for &mut Serializer {
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeTupleStruct for &mut Serializer {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeTupleVariant for &mut Serializer {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeMap for &mut Serializer {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), Error> {
        key.serialize(&mut **self)
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeStruct for &mut Serializer {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

impl ser::SerializeStructVariant for &mut Serializer {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: Serialize + ?Sized>(
        &mut self,
        _key: &'static str,
        value: &T,
    ) -> Result<(), Error> {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<(), Error> {
        Ok(())
    }
}

struct Deserializer<'de> {
    input: &'de [u8],
}

impl<'de> Deserializer<'de> {
    fn take(&mut self, len: usize) -> Result<&'de [u8], Error> {
        let (head, tail) = self
            .input
            .split_at_checked(len)
            .ok_or(Error::UnexpectedEnd)?;
        self.input = tail;
        Ok(head)
    }

    fn take_array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let (head, tail) = self
            .input
            .split_first_chunk::<N>()
            .ok_or(Error::UnexpectedEnd)?;
        self.input = tail;
        Ok(*head)
    }

    fn read_len(&mut self) -> Result<usize, Error> {
        let len = u64::from_le_bytes(self.take_array()?);
        usize::try_from(len).map_err(|_| Error::LengthOverflow(len))
    }
}

macro_rules! deserialize_le {
    ($($method:ident => $visit:ident: $ty:ty),* $(,)?) => {$(
        fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
            visitor.$visit(<$ty>::from_le_bytes(self.take_array()?))
        }
    )*};
}

impl<'de> de::Deserializer<'de> for &mut Deserializer<'de> {
    type Error = Error;

    deserialize_le! {
        deserialize_u8 => visit_u8: u8, deserialize_u16 => visit_u16: u16,
        deserialize_u32 => visit_u32: u32, deserialize_u64 => visit_u64: u64,
        deserialize_u128 => visit_u128: u128, deserialize_i8 => visit_i8: i8,
        deserialize_i16 => visit_i16: i16, deserialize_i32 => visit_i32: i32,
        deserialize_i64 => visit_i64: i64, deserialize_i128 => visit_i128: i128,
        deserialize_f32 => visit_f32: f32, deserialize_f64 => visit_f64: f64,
    }

    fn deserialize_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::NotSelfDescribing("deserialize_any"))
    }

    fn deserialize_ignored_any<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::NotSelfDescribing("deserialize_ignored_any"))
    }

    fn deserialize_identifier<V: Visitor<'de>>(self, _visitor: V) -> Result<V::Value, Error> {
        Err(Error::NotSelfDescribing("deserialize_identifier"))
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.take_array::<1>()? {
            [0] => visitor.visit_bool(false),
            [1] => visitor.visit_bool(true),
            [other] => Err(Error::InvalidBool(other)),
        }
    }

    fn deserialize_char<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let [first] = self.take_array()?;
        let width = match first {
            0x00..=0x7F => 1,
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => return Err(Error::InvalidChar),
        };
        let mut utf8 = [first, 0, 0, 0];
        let rest = self.take(width - 1).map_err(|_| Error::InvalidChar)?;
        utf8[1..width].copy_from_slice(rest);
        let value = std::str::from_utf8(&utf8[..width])
            .ok()
            .and_then(|text| text.chars().next())
            .ok_or(Error::InvalidChar)?;
        visitor.visit_char(value)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let len = self.read_len()?;
        visitor.visit_borrowed_str(std::str::from_utf8(self.take(len)?)?)
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let len = self.read_len()?;
        visitor.visit_string(std::str::from_utf8(self.take(len)?)?.to_owned())
    }

    fn deserialize_bytes<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let len = self.read_len()?;
        visitor.visit_borrowed_bytes(self.take(len)?)
    }

    fn deserialize_byte_buf<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let len = self.read_len()?;
        visitor.visit_byte_buf(self.take(len)?.to_vec())
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        match self.take_array::<1>()? {
            [0] => visitor.visit_none(),
            [1] => visitor.visit_some(self),
            [tag] => Err(Error::InvalidOptionTag(tag)),
        }
    }

    fn deserialize_unit<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let len = self.read_len()?;
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_seq(Elements {
            de: self,
            remaining: len,
        })
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let len = self.read_len()?;
        visitor.visit_map(Elements {
            de: self,
            remaining: len,
        })
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(fields.len(), visitor)
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        visitor.visit_enum(self)
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

/// The remaining elements of a sequence, tuple, struct or map.
struct Elements<'a, 'de> {
    de: &'a mut Deserializer<'de>,
    remaining: usize,
}

impl<'de> de::SeqAccess<'de> for Elements<'_, 'de> {
    type Error = Error;

    fn next_element_seed<T: DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, Error> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        seed.deserialize(&mut *self.de).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

impl<'de> de::MapAccess<'de> for Elements<'_, 'de> {
    type Error = Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        if self.remaining == 0 {
            return Ok(None);
        }
        self.remaining -= 1;
        seed.deserialize(&mut *self.de).map(Some)
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        seed.deserialize(&mut *self.de)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

impl<'de> de::EnumAccess<'de> for &mut Deserializer<'de> {
    type Error = Error;
    type Variant = Self;

    fn variant_seed<V: DeserializeSeed<'de>>(self, seed: V) -> Result<(V::Value, Self), Error> {
        let index = u32::from_le_bytes(self.take_array()?);
        let variant: Result<V::Value, Error> = seed.deserialize(index.into_deserializer());
        Ok((variant?, self))
    }
}

impl<'de> de::VariantAccess<'de> for &mut Deserializer<'de> {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Error> {
        Ok(())
    }

    fn newtype_variant_seed<T: DeserializeSeed<'de>>(self, seed: T) -> Result<T::Value, Error> {
        seed.deserialize(self)
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_tuple(len, visitor)
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.deserialize_tuple(fields.len(), visitor)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::{Deserialize, Serialize};

    use super::{Error, deserialize, serialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    enum Shape {
        Unit,
        Newtype(u16),
        Tuple(i8, bool),
        Struct { corner: char },
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Unit;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Pair(u8, i16);

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Sample {
        flag: bool,
        small: i8,
        word: u32,
        wide: i128,
        ratio: f32,
        text: String,
        letter: char,
        missing: Option<u8>,
        present: Option<u8>,
        list: Vec<u16>,
        array: [u8; 2],
        map: BTreeMap<u8, bool>,
        unit: Unit,
        pair: Pair,
        shapes: Vec<Shape>,
    }

    fn sample() -> Sample {
        Sample {
            flag: true,
            small: -2,
            word: 0x0403_0201,
            wide: -1,
            ratio: 1.5,
            text: "hé".to_string(),
            letter: '€',
            missing: None,
            present: Some(7),
            list: vec![1, 0x0302],
            array: [9, 8],
            map: BTreeMap::from([(1, false), (2, true)]),
            unit: Unit,
            pair: Pair(5, -3),
            shapes: vec![
                Shape::Unit,
                Shape::Newtype(0x0102),
                Shape::Tuple(-1, false),
                Shape::Struct { corner: 'A' },
            ],
        }
    }

    /// The layout bincode 1.3's `serialize` wrote; existing saves are these bytes.
    #[rustfmt::skip]
    const SAMPLE_BYTES: &[u8] = &[
        1,                                              // flag
        0xFE,                                           // small
        1, 2, 3, 4,                                     // word
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, // wide
        0, 0, 0xC0, 0x3F,                               // ratio
        3, 0, 0, 0, 0, 0, 0, 0, b'h', 0xC3, 0xA9,       // text: u64 length, UTF-8
        0xE2, 0x82, 0xAC,                               // letter: UTF-8, no length
        0,                                              // missing
        1, 7,                                           // present
        2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 2, 3,             // list
        9, 8,                                           // array: no length
        2, 0, 0, 0, 0, 0, 0, 0, 1, 0, 2, 1,             // map
                                                        // unit: nothing
        5, 0xFD, 0xFF,                                  // pair
        4, 0, 0, 0, 0, 0, 0, 0,                         // shapes
        0, 0, 0, 0,                                     // Unit
        1, 0, 0, 0, 2, 1,                               // Newtype
        2, 0, 0, 0, 0xFF, 0,                            // Tuple
        3, 0, 0, 0, b'A',                               // Struct
    ];

    #[test]
    fn writes_the_bincode_1_legacy_layout() {
        assert_eq!(serialize(&sample()).unwrap(), SAMPLE_BYTES);
        assert_eq!(deserialize::<Sample>(SAMPLE_BYTES).unwrap(), sample());
    }

    #[test]
    fn decodes_a_prefix_and_leaves_trailing_bytes() {
        let mut bytes = serialize(&(7u32, "header")).unwrap();
        bytes.extend_from_slice(b"body");
        assert_eq!(deserialize::<u32>(&bytes).unwrap(), 7);
        assert_eq!(deserialize::<(u32, &str)>(&bytes).unwrap(), (7, "header"));
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(matches!(
            deserialize::<bool>(&[2]),
            Err(Error::InvalidBool(2))
        ));
        assert!(matches!(
            deserialize::<Option<u8>>(&[3, 0]),
            Err(Error::InvalidOptionTag(3))
        ));
        assert!(matches!(
            deserialize::<char>(&[0x80]),
            Err(Error::InvalidChar)
        ));
        assert!(matches!(
            deserialize::<u32>(&[1, 2, 3]),
            Err(Error::UnexpectedEnd)
        ));
        assert!(matches!(
            deserialize::<String>(&[2, 0, 0, 0, 0, 0, 0, 0, 0xC3, 0x28]),
            Err(Error::InvalidUtf8(_))
        ));
        assert!(matches!(
            deserialize::<Shape>(&[9, 0, 0, 0]),
            Err(Error::Custom(_))
        ));
    }
}
