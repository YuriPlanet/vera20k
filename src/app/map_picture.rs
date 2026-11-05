//! HD map previews: a sharp picture of the whole map in the Skirmish, LAN and
//! online setup screens' preview boxes, drawn by VERA's own tactical renderer.
//! VERA-only; gamemd shows the map's own `[PreviewPack]` picture, which stays
//! when Options' HD Map Previews is off and while a map has no picture yet.
//!
//! The first time a map is shown, the menus start a hidden run of this
//! executable (`--map-picture`, [`session`]) that loads the map as a quiet
//! Battle, draws its battlefield across `[Header]`'s preview rectangle and
//! writes a PNG to `MapPreviews/<sha256>.png` in the settings folder
//! ([`pictures`]). The picture is kept by the map's SHA-256, so every map
//! with the same bytes, under any name, shares it. Covering `[Header]`'s
//! rectangle keeps the setup screens' start markers on their starts.
//!
//! The run is a separate process because loading a map changes process-wide
//! tables a live game must not see, and it keeps the menus responsive.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

pub(crate) mod canvas;
mod pictures;
pub(crate) mod session;

pub(crate) use pictures::{MapPictures, picture_for_map, poll};

/// The switch that starts a hidden map-picture run.
const FLAG: &str = "--map-picture";

/// One hidden run's job: `vera20k --map-picture <map> --sha256 <hex> --output <png>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapPictureRequest {
    /// The map as the menus name it, found the way a game finds it.
    map: String,
    /// The SHA-256 of the map's bytes when the menus asked; other bytes draw
    /// nothing.
    sha256: String,
    /// Where the PNG goes.
    output: PathBuf,
}

/// Whether these process arguments start a map-picture run. Checked before
/// anything else so the run logs to its own file.
pub fn is_invocation<I>(args: I) -> bool
where
    I: IntoIterator<Item = OsString>,
{
    args.into_iter().next().is_some_and(|arg| arg == FLAG)
}

/// The run's arguments, flag first, as the menus write them.
pub(crate) fn parse_args(args: Vec<OsString>) -> Result<MapPictureRequest> {
    let mut args = args.into_iter();
    ensure!(
        args.next().as_deref() == Some(OsStr::new(FLAG)),
        "a map-picture run starts with {FLAG}"
    );
    let map = args
        .next()
        .context("missing map after --map-picture")?
        .into_string()
        .map_err(|_| anyhow::anyhow!("the map name is not valid UTF-8"))?;
    let (mut sha256, mut output) = (None, None);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .with_context(|| format!("missing value after {}", flag.to_string_lossy()))?;
        let slot = match flag.to_str() {
            Some("--sha256") => &mut sha256,
            Some("--output") => &mut output,
            _ => bail!("unknown map-picture option {}", flag.to_string_lossy()),
        };
        ensure!(
            slot.replace(value).is_none(),
            "duplicate map-picture option {}",
            flag.to_string_lossy()
        );
    }
    let sha256 = sha256
        .context("missing --sha256")?
        .into_string()
        .map_err(|_| anyhow::anyhow!("--sha256 is not valid UTF-8"))?;
    ensure!(
        crate::map::source::valid_source_sha256(&sha256),
        "--sha256 is not a SHA-256"
    );
    Ok(MapPictureRequest {
        map,
        sha256,
        output: output.context("missing --output")?.into(),
    })
}

impl MapPictureRequest {
    fn args(&self) -> [&OsStr; 6] {
        [
            OsStr::new(FLAG),
            OsStr::new(&self.map),
            OsStr::new("--sha256"),
            OsStr::new(&self.sha256),
            OsStr::new("--output"),
            self.output.as_os_str(),
        ]
    }
}

/// Where a map's picture is kept.
fn picture_path(folder: &Path, sha256: &str) -> PathBuf {
    folder.join(format!("{sha256}.png"))
}

/// Write packed RGB as a PNG, whole or not at all.
fn write_png(path: &Path, width: u32, height: u32, rgb: &[u8]) -> Result<()> {
    use image::ImageEncoder;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(
        rgb,
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )?;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).with_context(|| format!("create {}", folder.display()))?;
    }
    let part = path.with_extension(format!("png.{}.part", std::process::id()));
    std::fs::write(&part, &png).with_context(|| format!("write {}", part.display()))?;
    if let Err(error) = std::fs::rename(&part, path) {
        let _ = std::fs::remove_file(&part);
        return Err(error).with_context(|| format!("keep {}", path.display()));
    }
    Ok(())
}

/// A kept picture as RGBA, or None when it is missing or unreadable.
fn read_png(path: &Path) -> Option<crate::map::preview::DecodedPreview> {
    let bytes = std::fs::read(path).ok()?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .inspect_err(|error| log::warn!("Unreadable map picture {}: {error}", path.display()))
        .ok()?
        .into_rgba8();
    Some(crate::map::preview::DecodedPreview {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_arguments_round_trip_and_refuse_a_bad_digest() {
        let request = MapPictureRequest {
            map: "8_Normandy_2054.yrm".into(),
            sha256: "a".repeat(64),
            output: PathBuf::from("MapPreviews").join("x.png"),
        };
        let args: Vec<OsString> = request.args().iter().map(|arg| arg.into()).collect();
        assert!(is_invocation(args.clone()));
        assert_eq!(parse_args(args.clone()).unwrap(), request);

        let mut bad = args;
        bad[3] = "not-a-digest".into();
        assert!(parse_args(bad).is_err());
    }
}
