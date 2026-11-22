//! Finds a saved [`InternedId`](crate::sim::intern::InternedId) that the
//! saved interner does not hold.
//!
//! A snapshot carries its ids and its interner in one file, and
//! `StringInterner::resolve` indexes without a check, so one id past the
//! interner panics wherever a later step names it. The saved state holds ids
//! in hundreds of fields. Instead of listing them, this walks the state
//! through its own `Serialize` impls, which is the shape the file holds, and
//! looks at every `InternedId` on the way: a field added later is covered
//! without an edit here, and a field that is not saved is not walked. Nothing
//! is written; the walk costs one pass over the state, as a save does.

use serde::ser::{self, Serialize};

/// The serde name of `InternedId`, a derived newtype struct over `u32`. The
/// load refusal tests fail if the type is renamed or reshaped without this.
const INTERNED_ID: &str = "InternedId";

/// Why the walk stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SavedIdError {
    /// `id` is at or past the interner's length. `owner` is the innermost
    /// saved struct or enum around it and `field` the field of it being
    /// written.
    #[error(
        "this save is damaged and cannot be loaded: its saved {owner} field {field} names \
         interned id {id}, which the save's own name table does not hold"
    )]
    Unknown {
        owner: &'static str,
        field: &'static str,
        id: u32,
    },
    /// A `Serialize` impl of the saved state refused to write itself.
    #[error("saved state cannot be read back for its interned ids: {0}")]
    Unwritable(String),
}

impl ser::Error for SavedIdError {
    fn custom<T: std::fmt::Display>(message: T) -> Self {
        Self::Unwritable(message.to_string())
    }
}

/// Check every `InternedId` that `state` saves against an interner of
/// `interned` strings.
pub(super) fn check_saved_interned_ids<T: Serialize + ?Sized>(
    state: &T,
    interned: usize,
) -> Result<(), SavedIdError> {
    state.serialize(&mut Walk {
        interned,
        owner: "",
        field: "",
        at_id: false,
    })
}

struct Walk {
    interned: usize,
    owner: &'static str,
    field: &'static str,
    /// The next `u32` is the index of an `InternedId`.
    at_id: bool,
}

/// A compound value being walked. `outer` is the owner and field to return to
/// when it ends.
struct Scope<'a> {
    walk: &'a mut Walk,
    outer: (&'static str, &'static str),
}

impl Walk {
    fn scope(&mut self) -> Scope<'_> {
        let outer = (self.owner, self.field);
        Scope { walk: self, outer }
    }

    fn named_scope(&mut self, owner: &'static str) -> Scope<'_> {
        let scope = self.scope();
        scope.walk.owner = owner;
        scope
    }
}

impl Scope<'_> {
    fn value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SavedIdError> {
        value.serialize(&mut *self.walk)
    }

    fn field<T: Serialize + ?Sized>(
        &mut self,
        field: &'static str,
        value: &T,
    ) -> Result<(), SavedIdError> {
        self.walk.field = field;
        self.value(value)
    }

    fn finish(self) -> Result<(), SavedIdError> {
        (self.walk.owner, self.walk.field) = self.outer;
        Ok(())
    }
}

macro_rules! skip_scalars {
    ($($method:ident: $scalar:ty,)*) => {
        $(fn $method(self, _: $scalar) -> Result<(), SavedIdError> {
            Ok(())
        })*
    };
}

impl<'a> ser::Serializer for &'a mut Walk {
    type Ok = ();
    type Error = SavedIdError;
    type SerializeSeq = Scope<'a>;
    type SerializeTuple = Scope<'a>;
    type SerializeTupleStruct = Scope<'a>;
    type SerializeTupleVariant = Scope<'a>;
    type SerializeMap = Scope<'a>;
    type SerializeStruct = Scope<'a>;
    type SerializeStructVariant = Scope<'a>;

    skip_scalars! {
        serialize_bool: bool,
        serialize_i8: i8,
        serialize_i16: i16,
        serialize_i32: i32,
        serialize_i64: i64,
        serialize_i128: i128,
        serialize_u8: u8,
        serialize_u16: u16,
        serialize_u64: u64,
        serialize_u128: u128,
        serialize_f32: f32,
        serialize_f64: f64,
        serialize_char: char,
        serialize_str: &str,
        serialize_bytes: &[u8],
    }

    fn serialize_u32(self, value: u32) -> Result<(), SavedIdError> {
        let known = usize::try_from(value).is_ok_and(|index| index < self.interned);
        if std::mem::take(&mut self.at_id) && !known {
            return Err(SavedIdError::Unknown {
                owner: self.owner,
                field: self.field,
                id: value,
            });
        }
        Ok(())
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        name: &'static str,
        value: &T,
    ) -> Result<(), SavedIdError> {
        self.at_id = name == INTERNED_ID;
        value.serialize(self)
    }

    fn serialize_none(self) -> Result<(), SavedIdError> {
        Ok(())
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> Result<(), SavedIdError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<(), SavedIdError> {
        Ok(())
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result<(), SavedIdError> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<(), SavedIdError> {
        Ok(())
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        value: &T,
    ) -> Result<(), SavedIdError> {
        value.serialize(self)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Scope<'a>, SavedIdError> {
        Ok(self.scope())
    }

    fn serialize_tuple(self, _: usize) -> Result<Scope<'a>, SavedIdError> {
        Ok(self.scope())
    }

    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Scope<'a>, SavedIdError> {
        Ok(self.scope())
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Scope<'a>, SavedIdError> {
        Ok(self.scope())
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Scope<'a>, SavedIdError> {
        Ok(self.scope())
    }

    fn serialize_struct(self, name: &'static str, _: usize) -> Result<Scope<'a>, SavedIdError> {
        Ok(self.named_scope(name))
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Scope<'a>, SavedIdError> {
        Ok(self.named_scope(name))
    }

    fn collect_str<T: std::fmt::Display + ?Sized>(self, _: &T) -> Result<(), SavedIdError> {
        Ok(())
    }

    /// The save codec is binary, and a type may write another shape for a
    /// readable one.
    fn is_human_readable(&self) -> bool {
        false
    }
}

macro_rules! walk_elements {
    ($($compound:ident: $method:ident,)*) => {
        $(impl ser::$compound for Scope<'_> {
            type Ok = ();
            type Error = SavedIdError;

            fn $method<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SavedIdError> {
                self.value(value)
            }

            fn end(self) -> Result<(), SavedIdError> {
                self.finish()
            }
        })*
    };
}

walk_elements! {
    SerializeSeq: serialize_element,
    SerializeTuple: serialize_element,
    SerializeTupleStruct: serialize_field,
    SerializeTupleVariant: serialize_field,
}

impl ser::SerializeMap for Scope<'_> {
    type Ok = ();
    type Error = SavedIdError;

    fn serialize_key<T: Serialize + ?Sized>(&mut self, key: &T) -> Result<(), SavedIdError> {
        self.value(key)
    }

    fn serialize_value<T: Serialize + ?Sized>(&mut self, value: &T) -> Result<(), SavedIdError> {
        self.value(value)
    }

    fn end(self) -> Result<(), SavedIdError> {
        self.finish()
    }
}

macro_rules! walk_fields {
    ($($compound:ident,)*) => {
        $(impl ser::$compound for Scope<'_> {
            type Ok = ();
            type Error = SavedIdError;

            fn serialize_field<T: Serialize + ?Sized>(
                &mut self,
                key: &'static str,
                value: &T,
            ) -> Result<(), SavedIdError> {
                self.field(key, value)
            }

            fn end(self) -> Result<(), SavedIdError> {
                self.finish()
            }
        })*
    };
}

walk_fields! {
    SerializeStruct,
    SerializeStructVariant,
}
