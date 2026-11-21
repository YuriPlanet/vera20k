//! Saves keep the string path older readers expect. Native path units are
//! optional so another OS can reopen the world using the portable basename.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::indexing_slicing
    )
)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize, Serializer};

fn component(name: &str) -> Option<&str> {
    (!name.contains(['/', '\\']) && super::validate_network_map_name(name).is_ok()).then_some(name)
}

pub(super) fn basename(path: &Path) -> Option<&str> {
    component(path.file_name()?.to_str()?)
}

pub(super) fn serialize<S: Serializer>(path: &Path, serializer: S) -> Result<S::Ok, S::Error> {
    #[derive(Serialize)]
    struct Fields<'a> {
        path: &'a str,
        #[cfg(any(unix, windows))]
        #[serde(skip_serializing_if = "Option::is_none")]
        native_path: Option<&'a std::ffi::OsStr>,
    }

    let text = path.to_str();
    Fields {
        path: text.or_else(|| basename(path)).unwrap_or_default(),
        #[cfg(any(unix, windows))]
        native_path: text.is_none().then_some(path.as_os_str()),
    }
    .serialize(serializer)
}

pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<PathBuf, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NativePath {
        #[cfg(any(unix, windows))]
        Local(std::ffi::OsString),
        Unavailable(serde::de::IgnoredAny),
    }

    #[derive(Deserialize)]
    struct Fields {
        path: String,
        native_path: Option<NativePath>,
    }

    let Fields { path, native_path } = Fields::deserialize(deserializer)?;
    Ok(match native_path {
        #[cfg(any(unix, windows))]
        Some(NativePath::Local(native)) => native.into(),
        Some(NativePath::Unavailable(_)) => component(&path).unwrap_or_default().into(),
        None => path.into(),
    })
}
