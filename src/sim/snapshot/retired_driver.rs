//! Save fields from the former native AI driver. Read the original types so
//! populated saves keep their field boundaries and the stock layout ID.
use std::marker::PhantomData;

use crate::util::native_x87::NativeF32Bits;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A wire field whose value is discarded on load and reset on save.
#[derive(Debug, Clone, Default)]
pub(in crate::sim) struct Discarded<T>(PhantomData<T>);

impl<T: Default + Serialize> Serialize for Discarded<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        T::default().serialize(serializer)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Discarded<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer)?;
        Ok(Self(PhantomData))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename = "DriverPolicy")]
pub(in crate::sim) struct SavedDriverPolicy {
    native_hard_grant: bool,
    credit_drip: Option<CreditDrip>,
    build_time_scale: Option<NativeF32Bits>,
    wallet_floor: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CreditDrip {
    amount: i32,
    interval_ticks: u32,
}
