//! HD map previews: a sharp picture of the whole map in the Skirmish, LAN and
//! online setup screens' preview boxes, drawn by VERA's own tactical renderer.
//! VERA-only; gamemd shows the map's own `[PreviewPack]` picture, which stays
//! when Options' HD Map Previews is off and while a map has no picture yet.
//!
//! The first time a map is shown, the menus start a hidden run of this
//! executable (`--map-picture`, [`session`]) that loads the map as a quiet
//! Battle, draws its battlefield where the map's own preview shows it
//! ([`placement`]), marks its ore yellow and its gems purple as map library
//! previews do, and writes a PNG to `MapPreviews/<generation>/<sha256>.png`
//! in the settings folder ([`pictures`]). The picture is kept by the map's
//! SHA-256, so every map with the same bytes, under any name, shares it.
//! Every later build shows it until the pictures' version changes, so a new
//! build never waits for it; each game folder keeps its own
//! ([`generation_folder`]). Showing the
//! preview's place in the preview's shape lets the menus fit the picture in
//! the very rectangle the preview filled, so the map and its start markers
//! stay put when the picture replaces it.
//!
//! The run is a separate process because loading a map changes process-wide
//! tables a live game must not see, and it keeps the menus responsive.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};

pub(crate) mod canvas;
mod pictures;
mod placement;
pub(crate) mod session;

pub(crate) use pictures::{MapPictures, picture_for_map, poll, stop_for_exit};

/// The switch that starts a hidden map-picture run.
const FLAG: &str = "--map-picture";
/// The longest a run lives, however it hangs: past it the run ends itself,
/// so no run outlives the game that started it for long.
const LIFETIME: std::time::Duration = std::time::Duration::from_secs(150);
/// Hex digits naming a pictures folder ([`generation_folder`]).
const GENERATION_LEN: usize = 16;
/// The pictures' version. Bump it when a change to how VERA draws a picture
/// (its framing, size or marks) should redraw the pictures kept so far;
/// until then every build shows them.
const PICTURE_VERSION: u32 = 1;

/// One hidden run's job: `vera20k --map-picture <map> --sha256 <hex> --output
/// <png>`, and for tools/map_render.py `--area view`, `--max-size <w>x<h>` and
/// with `--area view` `--markers <json>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapPictureRequest {
    /// The map as the menus name it, or a path, found the way a game finds it.
    map: String,
    /// The SHA-256 of the map's bytes when the menus asked; other bytes draw
    /// nothing.
    sha256: String,
    /// Where the PNG goes.
    output: PathBuf,
    area: PictureArea,
    /// The largest picture; the menus' [`canvas::MAX_PICTURE`] unless asked.
    max_size: (u32, u32),
    /// Where the start markers' JSON goes, when asked ([`session`]).
    markers: Option<PathBuf>,
}

/// The world rectangle a picture shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PictureArea {
    /// Where the map's own preview shows the map ([`placement`]), in the
    /// preview's shape; when it matches no place, `[Header]`'s preview
    /// rectangle, over which the setup screens place the start markers, or
    /// on a map without one all the camera can show.
    Preview,
    /// All the camera can show (`--area view`).
    View,
}

impl MapPictureRequest {
    /// The menus' request: the preview's place at their largest size.
    fn for_menus(map: String, sha256: String, output: PathBuf) -> Self {
        Self {
            map,
            sha256,
            output,
            area: PictureArea::Preview,
            max_size: canvas::MAX_PICTURE,
            markers: None,
        }
    }
}

/// Whether these process arguments start a map-picture run. Checked before
/// anything else so the run logs to its own file.
pub fn is_invocation<I>(args: I) -> bool
where
    I: IntoIterator<Item = OsString>,
{
    args.into_iter().next().is_some_and(|arg| arg == FLAG)
}

/// End this run after [`LIFETIME`], whatever it is doing then. The run's own
/// time limit is checked only between frames, after the map has loaded.
pub fn watch_lifetime() {
    std::thread::spawn(|| {
        std::thread::sleep(LIFETIME);
        log::error!("The map picture run ran for {LIFETIME:?}; ending it");
        log::logger().flush();
        std::process::exit(3);
    });
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
    let (mut sha256, mut output, mut area, mut max_size, mut markers) =
        (None, None, None, None, None);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .with_context(|| format!("missing value after {}", flag.to_string_lossy()))?;
        let slot = match flag.to_str() {
            Some("--sha256") => &mut sha256,
            Some("--output") => &mut output,
            Some("--area") => &mut area,
            Some("--max-size") => &mut max_size,
            Some("--markers") => &mut markers,
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
    let area = match area.as_ref().map(|area| area.to_str()) {
        None | Some(Some("preview")) => PictureArea::Preview,
        Some(Some("view")) => PictureArea::View,
        Some(_) => bail!("--area is preview or view"),
    };
    // The markers' JSON describes the whole drawing, which only a view keeps.
    ensure!(
        markers.is_none() || area == PictureArea::View,
        "--markers needs --area view"
    );
    let max_size = match max_size {
        None => canvas::MAX_PICTURE,
        Some(size) => size
            .to_str()
            .and_then(|size| size.split_once('x'))
            .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)))
            .filter(|&(width, height): &(u32, u32)| width > 0 && height > 0)
            .context("--max-size is <width>x<height>")?,
    };
    Ok(MapPictureRequest {
        map,
        sha256,
        output: output.context("missing --output")?.into(),
        area,
        max_size,
        markers: markers.map(PathBuf::from),
    })
}

impl MapPictureRequest {
    fn args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = vec![
            FLAG.into(),
            self.map.clone().into(),
            "--sha256".into(),
            self.sha256.clone().into(),
            "--output".into(),
            self.output.clone().into(),
        ];
        if self.area == PictureArea::View {
            args.extend(["--area".into(), "view".into()]);
        }
        if self.max_size != canvas::MAX_PICTURE {
            let (width, height) = self.max_size;
            args.extend(["--max-size".into(), format!("{width}x{height}").into()]);
        }
        if let Some(markers) = &self.markers {
            args.extend(["--markers".into(), markers.clone().into()]);
        }
        args
    }
}

/// Where a map's picture is kept in its build's folder.
fn picture_path(folder: &Path, sha256: &str) -> PathBuf {
    folder.join(format!("{sha256}.png"))
}

/// The folder in `pictures` for the [`PICTURE_VERSION`] pictures of maps
/// drawn with the art in `ra2_dir`: every build of that version shares it,
/// and another game folder draws its own. The folders of other versions and
/// game folders, which this build never shows, are removed.
fn generation_folder(pictures: &Path, ra2_dir: &Path) -> PathBuf {
    let key = format!("{PICTURE_VERSION}\0{}", ra2_dir.to_string_lossy());
    let name = crate::util::sha256::sha256_hex(key.as_bytes())[..GENERATION_LEN].to_owned();
    for entry in std::fs::read_dir(pictures).into_iter().flatten().flatten() {
        let other = entry.file_name().to_str().is_some_and(|other| {
            other != name
                && other.len() == GENERATION_LEN
                && other.bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        if other
            && entry.file_type().is_ok_and(|kind| kind.is_dir())
            && let Err(error) = std::fs::remove_dir_all(entry.path())
        {
            log::debug!("Could not remove {}: {error}", entry.path().display());
        }
    }
    pictures.join(name)
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
        let menus = MapPictureRequest::for_menus(
            "8_Normandy_2054.yrm".into(),
            "a".repeat(64),
            PathBuf::from("MapPreviews").join("x.png"),
        );
        let args = menus.args();
        assert_eq!(args.len(), 6, "the menus ask for nothing more");
        assert!(is_invocation(args.clone()));
        assert_eq!(parse_args(args.clone()).unwrap(), menus);
        let tool = MapPictureRequest {
            area: PictureArea::View,
            max_size: (1200, 1200),
            markers: Some(PathBuf::from("markers.json")),
            ..menus
        };
        assert_eq!(parse_args(tool.args()).unwrap(), tool);
        let preview_markers = MapPictureRequest {
            area: PictureArea::Preview,
            ..tool
        };
        assert!(parse_args(preview_markers.args()).is_err());

        let mut bad = args;
        bad[3] = "not-a-digest".into();
        assert!(parse_args(bad).is_err());
    }

    #[test]
    fn each_picture_version_and_game_folder_keeps_its_own_pictures() {
        let pictures =
            std::env::temp_dir().join(format!("vera20k-map-pictures-{}", std::process::id()));
        let older = pictures.join("0123456789abcdef");
        let not_ours = pictures.join("Notes");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::create_dir_all(&not_ours).unwrap();
        let folder = generation_folder(&pictures, Path::new("/games/yr"));
        // Named by the pictures' version 1 and the game folder alone, so
        // every build of that version shows the same pictures.
        assert_eq!(folder, pictures.join("15dae2fa124079a9"));
        assert_ne!(
            folder,
            generation_folder(&pictures, Path::new("/games/modded"))
        );
        assert!(!older.exists(), "another version's pictures stayed");
        assert!(not_ours.exists());
        std::fs::remove_dir_all(&pictures).unwrap();
    }
}
