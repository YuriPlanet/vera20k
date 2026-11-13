//! VERA extension: write a generated map as a multiplayer `.map` text
//! (`.yrm` on disk), in the sections [`crate::map::map_file::MapFile`]
//! parses back: `[Basic]`, `[Map]`, the map's own INI sections (lighting),
//! `[Waypoints]`, `[Terrain]`, the object lists, the three packs and an
//! optional `[Preview]` thumbnail.
//!
//! RESIDUAL: gamemd's own writer, `Save_Scenario_Map_File` 0x00687CE0 (its
//! one caller is `RandomMapSetupDialog__Proc` at 0x00596A8B), is not ported.
//! This writer emits the formats gamemd's readers take (`ReadIsoMapPack5`
//! 0x0056BB93, the overlay packs, `[Terrain]` and `[Waypoints]` at radix 1000
//! under `NewINIFormat=4`).

use std::fmt::Write as _;

use crate::map::entities::{EntityCategory, MapEntity};
use crate::map::map_file::MapFile;
use crate::util::base64::base64_encode;

/// Uncompressed bytes per pack block, as the retail packs hold.
const PACK_BLOCK: usize = 8192;
/// Base64 characters per numbered pack line, as retail maps write.
const PACK_LINE: usize = 70;
/// The overlay byte of a cell without one.
const NO_OVERLAY: u8 = 0xFF;
/// `[Terrain]` and `[Waypoints]` key radix under `NewINIFormat=4`.
const CELL_RADIX: u32 = 1000;

/// A map thumbnail for `[Preview]` / `[PreviewPack]`: `width * height` RGBA
/// pixels, rows top down.
pub struct ExportPreview<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba: &'a [u8],
}

/// The `.map` text of `map` as a standard multiplayer map named `name` for
/// `min_players..=max_players`, with `preview` as its Choose Map thumbnail
/// and `starts` as the `[Header]` the numbered start markers are drawn from.
pub fn multiplayer_map_text(
    map: &MapFile,
    name: &str,
    min_players: u32,
    max_players: u32,
    preview: Option<ExportPreview<'_>>,
    starts: Option<&crate::map::preview::PreviewSourceBounds>,
) -> String {
    let mut out = String::new();
    let name: String = name
        .chars()
        .filter(|c| !c.is_control() && *c != ';')
        .collect();
    let header = &map.header;
    write!(
        out,
        "[Basic]\nName={name}\nNewINIFormat=4\nMultiplayerOnly=1\nOfficial=no\n\
         GameMode=standard\nMinPlayer={min_players}\nMaxPlayer={max_players}\n\
         MinPlayers={min_players}\nMaxPlayers={max_players}\n\n\
         [Map]\nTheater={}\nSize=0,0,{},{}\nLocalSize={},{},{},{}\n\n",
        header.theater,
        header.width,
        header.height,
        header.local_left,
        header.local_top,
        header.local_width,
        header.local_height,
    )
    .unwrap();

    for section_name in map.ini.section_names() {
        let Some(section) = map.ini.section(section_name) else {
            continue;
        };
        writeln!(out, "[{section_name}]").unwrap();
        for (key, value) in section.raw_entries() {
            writeln!(out, "{key}={value}").unwrap();
        }
        out.push('\n');
    }

    // The start header the map list reads for its numbered markers
    // (`0x00689D64..0x00689E62`): the projected playfield and each start.
    if let Some(starts) = starts {
        write!(
            out,
            "[Header]\nStartX={}\nStartY={}\nWidth={}\nHeight={}\nNumberStartingPoints={}\n",
            starts.origin_x,
            starts.origin_y,
            starts.width,
            starts.height,
            starts.start_points.len(),
        )
        .unwrap();
        for (index, point) in starts.start_points.iter().enumerate() {
            writeln!(out, "Waypoint{}={},{}", index + 1, point.x, point.y).unwrap();
        }
        out.push('\n');
    }

    // The thumbnail `DecodePreviewPackToSurface @ 0x00641B00` reads: the
    // `Size` rect's last two fields, and 24-bit RGB rows through LZO blocks.
    if let Some(preview) = preview
        && preview.width > 0
        && preview.rgba.len() == preview.width as usize * preview.height as usize * 4
    {
        writeln!(
            out,
            "[Preview]\nSize=0,0,{},{}\n",
            preview.width, preview.height
        )
        .unwrap();
        let rgb: Vec<u8> = preview
            .rgba
            .chunks_exact(4)
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect();
        push_pack(
            &mut out,
            "PreviewPack",
            &crate::util::lzo::compress_chunks(&rgb, PACK_BLOCK),
        );
    }

    let mut waypoints: Vec<_> = map.waypoints.iter().collect();
    waypoints.sort_by_key(|(index, _)| **index);
    out.push_str("[Waypoints]\n");
    for (index, waypoint) in waypoints {
        let cell = u32::from(waypoint.ry) * CELL_RADIX + u32::from(waypoint.rx);
        writeln!(out, "{index}={cell}").unwrap();
    }
    out.push('\n');

    out.push_str("[Terrain]\n");
    for object in &map.terrain_objects {
        let cell = u32::from(object.ry) * CELL_RADIX + u32::from(object.rx);
        writeln!(out, "{cell}={}", object.name).unwrap();
    }
    out.push('\n');

    for (section, category) in [
        ("Structures", EntityCategory::Structure),
        ("Units", EntityCategory::Unit),
        ("Infantry", EntityCategory::Infantry),
        ("Aircraft", EntityCategory::Aircraft),
    ] {
        let mut lines = map
            .entities
            .iter()
            .filter(|entity| entity.category == category)
            .peekable();
        if lines.peek().is_none() {
            continue;
        }
        writeln!(out, "[{section}]").unwrap();
        for (index, entity) in lines.enumerate() {
            writeln!(out, "{index}={}", object_line(entity)).unwrap();
        }
        out.push('\n');
    }

    // The overlay planes: 512 bytes a row, 1024 once a coordinate needs it
    // (VERA's extended sizes), as `OverlayDataPack::from_cells` picks.
    let side = if map
        .overlays
        .iter()
        .any(|overlay| overlay.rx >= 512 || overlay.ry >= 512)
    {
        1024
    } else {
        512
    };
    let mut ids = vec![NO_OVERLAY; side * side];
    let mut frames = vec![0u8; side * side];
    for overlay in &map.overlays {
        let at = usize::from(overlay.ry) * side + usize::from(overlay.rx);
        ids[at] = overlay.overlay_id;
        frames[at] = overlay.frame;
    }
    push_pack(
        &mut out,
        "OverlayPack",
        &crate::util::lcw::compress_chunks(&ids, PACK_BLOCK),
    );
    push_pack(
        &mut out,
        "OverlayDataPack",
        &crate::util::lcw::compress_chunks(&frames, PACK_BLOCK),
    );

    let mut cells = Vec::with_capacity(map.cells.len() * 11);
    for cell in &map.cells {
        cells.extend_from_slice(&(cell.rx as i16).to_le_bytes());
        cells.extend_from_slice(&(cell.ry as i16).to_le_bytes());
        cells.extend_from_slice(&cell.tile_index.to_le_bytes());
        cells.extend_from_slice(&[cell.sub_tile, cell.z, cell.legacy_ice_byte]);
    }
    push_pack(
        &mut out,
        "IsoMapPack5",
        &crate::util::lzo::compress_chunks(&cells, PACK_BLOCK),
    );
    out
}

/// One object-list line in the column order the scenario readers take.
fn object_line(entity: &MapEntity) -> String {
    let MapEntity {
        owner,
        type_id,
        health,
        cell_x,
        cell_y,
        facing,
        sub_cell,
        ..
    } = entity;
    let high = u8::from(entity.high);
    let group = entity.group;
    let veterancy = entity.veterancy.unwrap_or(0);
    let base = format!("{owner},{type_id},{health},{cell_x},{cell_y}");
    match entity.category {
        // ...,Facing,Tag,AISellable,AIRebuildable,Powered,Upgrades,SpotLight,
        // Upgrade1..3,AIRepairable,Nominal
        EntityCategory::Structure => {
            format!("{base},{facing},None,1,0,1,0,0,None,None,None,1,0")
        }
        // ...,SubCell,Mission,Facing,Tag,Veterancy,Group,High,Recruitable A/B
        EntityCategory::Infantry => {
            format!("{base},{sub_cell},None,{facing},None,{veterancy},{group},{high},1,0")
        }
        // ...,Facing,Mission,Tag,Veterancy,Group,High,FollowsIndex,Recruitable A/B
        EntityCategory::Unit => {
            format!("{base},{facing},None,None,{veterancy},{group},{high},-1,1,0")
        }
        // ...,Facing,Mission,Tag,Veterancy,Group,Recruitable A/B
        EntityCategory::Aircraft => {
            format!("{base},{facing},None,None,{veterancy},{group},1,0")
        }
    }
}

/// A pack section: the base64 of `bytes` over numbered lines.
fn push_pack(out: &mut String, section: &str, bytes: &[u8]) {
    let encoded = base64_encode(bytes);
    writeln!(out, "[{section}]").unwrap();
    for (index, line) in encoded.as_bytes().chunks(PACK_LINE).enumerate() {
        // Base64 is ASCII, so any byte split is a character split.
        writeln!(out, "{}={}", index + 1, std::str::from_utf8(line).unwrap()).unwrap();
    }
    out.push('\n');
}
