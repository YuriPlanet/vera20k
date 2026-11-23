use super::build::{
    TerrainBuildProjection, TileMetadata, apply_canonical_land_to_metadata,
    apply_land_type_semantics, apply_selected_presentation_metadata,
    canonical_ramp_from_slope_type, classify_overlay_effects, cliff_back_normal_reclass_applies,
    height_in_pixels_from_tmp, materialize_map_load_cells, merge_tmp_file_metadata,
    merge_tmp_metadata, ordinary_variant_selection_enabled,
};
use super::cliffs::DestroyableCliffFamily;
use super::tubes::{
    AUTO_TUBE_DIRECTIONS, auto_tube_direction_for_tile, build_auto_low_bridge_tubes,
};
use super::*;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::assets::mix_hash::mix_hash;
use crate::assets::tmp_file::TmpTile;
use crate::map::overlay::{OverlayDataPack, TerrainObject};
use crate::map::tube_facts::TubeSource;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::rng::SimRng;
use crate::util::save_codec;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

struct Gsi0404AssetDirectory(PathBuf);

impl Gsi0404AssetDirectory {
    fn new() -> Self {
        static NEXT_DIRECTORY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock follows Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "vera20k-gsi-04-04-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        std::fs::create_dir(&path).expect("create GSI-04.04 asset directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.0.join(name), bytes).expect("write GSI-04.04 asset fixture");
    }
}

impl Drop for Gsi0404AssetDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn gsi_04_04_mix_bytes(entry_name: &str, body: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&(body.len() as u32).to_le_bytes());
    data.extend_from_slice(&mix_hash(entry_name).to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&(body.len() as u32).to_le_bytes());
    data.extend_from_slice(body);
    data
}

fn gsi_04_04_asset_manager_with_loose_tmp(
    tmp_filename: &str,
    tmp_bytes: &[u8],
) -> (Gsi0404AssetDirectory, AssetManager) {
    let directory = Gsi0404AssetDirectory::new();
    let empty_mix = gsi_04_04_mix_bytes("gsi0404.bin", b"fixture");
    for name in [
        "ra2md.mix",
        "ra2.mix",
        "cachemd.mix",
        "cache.mix",
        "localmd.mix",
        "local.mix",
        "conqmd.mix",
        "conquer.mix",
        "cameomd.mix",
        "cameo.mix",
        "mapsmd03.mix",
        "multimd.mix",
        "movmd03.mix",
    ] {
        directory.write(name, &empty_mix);
    }
    directory.write(tmp_filename, tmp_bytes);
    let manager = AssetManager::new(
        directory.path(),
        MediaArchiveMode::Numbered { media_index: 2 },
    )
    .expect("construct synthetic retail archive stack");
    (directory, manager)
}

fn gsi_04_04_sparse_tmp_bytes() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&2u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&88u32.to_le_bytes());
    data.extend_from_slice(&45u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data
}

fn gsi_04_02_last_tiles_tmp_bytes(
    terrain_type: u8,
    radar_left: [u8; 3],
    radar_right: [u8; 3],
) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&8u32.to_le_bytes());
    data.extend_from_slice(&4u32.to_le_bytes());
    data.extend_from_slice(&20u32.to_le_bytes());
    data.extend_from_slice(&[0u8; 40]);
    data.push(3);
    data.push(terrain_type);
    data.push(0);
    data.extend_from_slice(&radar_left);
    data.extend_from_slice(&radar_right);
    data.extend_from_slice(&[0u8; 3]);
    data.extend(1u8..=16);
    data
}

fn gsi_04_02_last_tiles_theater() -> TheaterData {
    synthetic_lat_theater_from_ini(
        b"[General]\nRoughTile=2\nClearToRoughLat=3\n\
          [TileSet0000]\nTilesInSet=3\nFileName=base\nSetName=Clear\n\
          [TileSet0001]\nTilesInSet=5\nLastTilesInSet=2\nFileName=old\nSetName=Clear\n\
          [TileSet0002]\nTilesInSet=1\nFileName=rough\nSetName=Rough\n\
          [TileSet0003]\nTilesInSet=16\nFileName=lat\nSetName=Rough LAT\n",
    )
}

fn gsi_04_02_asset_manager_with_loose_tmps(
    files: &[(&str, &[u8])],
) -> (Gsi0404AssetDirectory, AssetManager) {
    let directory = Gsi0404AssetDirectory::new();
    let empty_mix = gsi_04_04_mix_bytes("gsi0402.bin", b"fixture");
    for name in [
        "ra2md.mix",
        "ra2.mix",
        "cachemd.mix",
        "cache.mix",
        "localmd.mix",
        "local.mix",
        "conqmd.mix",
        "conquer.mix",
        "cameomd.mix",
        "cameo.mix",
        "mapsmd03.mix",
        "multimd.mix",
        "movmd03.mix",
    ] {
        directory.write(name, &empty_mix);
    }
    for &(name, bytes) in files {
        directory.write(name, bytes);
    }
    let manager = AssetManager::new(
        directory.path(),
        MediaArchiveMode::Numbered { media_index: 2 },
    )
    .expect("construct compatibility-translation asset stack");
    (directory, manager)
}

fn make_map(
    cells: Vec<MapCell>,
    overlays: Vec<OverlayEntry>,
    terrain_objects: Vec<TerrainObject>,
) -> MapFile {
    MapFile {
        header: crate::map::map_file::MapHeader {
            theater: "TEMPERATE".to_string(),
            fill: "Clear".to_string(),
            level: 0,
            width: 4,
            height: 4,
            local_left: 0,
            local_top: 0,
            local_width: 4,
            local_height: 4,
        },
        basic: crate::map::basic::BasicSection::default(),
        ranking: crate::map::ranking::RankingSection::default(),
        preview: crate::map::preview::PreviewSection::default(),
        cells,
        iso_map_pack_lookups: Vec::new(),
        entities: Vec::new(),
        overlays,
        authored_overlay_packs: crate::map::map_file::AuthoredOverlayPackSlot::empty(),
        smudges: Vec::new(),
        terrain_objects,
        waypoints: HashMap::new(),
        tags: HashMap::new(),
        triggers: HashMap::new(),
        events: HashMap::new(),
        actions: HashMap::new(),
        local_variables: HashMap::new(),
        special_flags: crate::map::basic::SpecialFlagsSection::default(),
        ini: IniFile::from_str(""),
    }
}

mod cliff_back_tests;
mod grid_allocation_tests;
mod land_rows_tests;
mod load_fill_tests;
mod overlay_land_tests;
mod tile_anims_tests;
mod tile_presentation_tests;
mod tubes_tests;

fn synthetic_theater_with_wood_bridge_set() -> TheaterData {
    let ini = b"[TileSet0000]\nTilesInSet=10\nFileName=clear\nSetName=Clear\n\n\
                [TileSet0001]\nTilesInSet=20\nFileName=wood\nSetName=Wood Bridge\n";
    let lookup = crate::map::theater::parse_tileset_ini(ini, "tem").unwrap();
    let empty_palette = crate::assets::pal_file::Palette::from_bytes(&[0u8; 768])
        .expect("768-byte zero palette parses");
    TheaterData {
        lookup,
        iso_palette: empty_palette.clone(),
        unit_palette: empty_palette.clone(),
        tiberium_palette: empty_palette,
        extension: "tem",
        ini_data: Vec::new(),
        bridge_set: None,
        wood_bridge_set: Some(1),
        slope_set_pieces: None,
        high_bridge_rim_tiles: crate::map::bridge_rim_tiles::HighBridgeRimTiles::ABSENT,
        tunnels: None,
        track_tunnels: None,
        dirt_tunnels: None,
        dirt_track_tunnels: None,
        automatic_tube_bases: [-1; 4],
        cliff_ranges: crate::map::theater::TheaterCliffRanges::default(),
        rmg_tiles: crate::map::theater::RmgTileKeys::default(),
    }
}

fn synthetic_theater_from_ini(ini: &[u8]) -> TheaterData {
    let lookup = crate::map::theater::parse_tileset_ini(ini, "tem").unwrap();
    let empty_palette = crate::assets::pal_file::Palette::from_bytes(&[0u8; 768])
        .expect("768-byte zero palette parses");
    TheaterData {
        lookup,
        iso_palette: empty_palette.clone(),
        unit_palette: empty_palette.clone(),
        tiberium_palette: empty_palette,
        extension: "tem",
        ini_data: ini.to_vec(),
        bridge_set: None,
        wood_bridge_set: None,
        slope_set_pieces: None,
        high_bridge_rim_tiles: crate::map::bridge_rim_tiles::HighBridgeRimTiles::ABSENT,
        tunnels: None,
        track_tunnels: None,
        dirt_tunnels: None,
        dirt_track_tunnels: None,
        automatic_tube_bases: [-1; 4],
        cliff_ranges: crate::map::theater::TheaterCliffRanges::default(),
        rmg_tiles: crate::map::theater::RmgTileKeys::default(),
    }
}

/// [`synthetic_theater_from_ini`] with the RMG/LAT globals theater init
/// resolves from its `[General]` section.
fn synthetic_lat_theater_from_ini(ini: &[u8]) -> TheaterData {
    let mut theater = synthetic_theater_from_ini(ini);
    let parsed = IniFile::from_bytes(ini).expect("synthetic theater INI parses");
    theater.rmg_tiles = crate::map::theater::resolve_rmg_tile_keys(
        &theater.lookup,
        parsed.section("General"),
        None,
    );
    theater
}

fn synthetic_automatic_shell_theater() -> TheaterData {
    let mut theater = synthetic_theater_from_ini(
        b"[TileSet0000]\nTilesInSet=5\nFileName=tunnel\nSetName=Tunnels\n\n\
          [TileSet0001]\nTilesInSet=5\nFileName=track\nSetName=Track Tunnels\n\n\
          [TileSet0002]\nTilesInSet=5\nFileName=dirt\nSetName=Dirt Tunnels\n\n\
          [TileSet0003]\nTilesInSet=5\nFileName=dtunn\nSetName=Dirt Track Tunnels\n",
    );
    theater.tunnels = Some(0);
    theater.track_tunnels = Some(1);
    theater.dirt_tunnels = Some(2);
    theater.dirt_track_tunnels = Some(3);
    theater.automatic_tube_bases = [0, 5, 10, 15];
    theater
}

fn make_test_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    test_flat_cell(rx, ry)
}

#[derive(Default)]
struct LoadRecalcTestEffects {
    tube_requests: Vec<AutomaticTubeRequest>,
    tube_allocation: Option<AutomaticTubeAllocation>,
    requests: Vec<TerrainTileAnimation>,
    fail: bool,
}

impl LoadCellRecalcEffects for LoadRecalcTestEffects {
    type Error = &'static str;

    fn allocate_automatic_tube(
        &mut self,
        request: AutomaticTubeRequest,
    ) -> Result<AutomaticTubeAllocation, Self::Error> {
        self.tube_requests.push(request);
        Ok(self
            .tube_allocation
            .unwrap_or(AutomaticTubeAllocation::AllocationNull))
    }

    fn construct_terrain_attached_anim(
        &mut self,
        request: &TerrainTileAnimation,
    ) -> Result<(), Self::Error> {
        self.requests.push(request.clone());
        if self.fail {
            Err("fixture failure")
        } else {
            Ok(())
        }
    }
}

include!("recalc_native_tests.rs");
include!("saved_terrain_tests.rs");

/// Land rows for the CliffBack fixtures: Clear and Road authored, every
/// SpeedType at the 1.0 default, and Rock unauthored, so the CliffBack Rock
/// takes the zero row: wheel-impassable, ground-blocked and unbuildable.
fn clear_and_road_rules() -> TerrainRules {
    TerrainRules::from_ini(&IniFile::from_str(
        "[Clear]\nBuildable=yes\n[Road]\nBuildable=yes\n",
    ))
}

/// Loose 1x1 TMPs reading Clear (byte 0) and Road (byte 11) land, for the
/// `clear` and `road` tilesets these CliffBack fixtures register.
fn clear_and_road_tmp_assets(theater: &TheaterData) -> (Gsi0404AssetDirectory, AssetManager) {
    let tmps: Vec<(String, Vec<u8>)> = (0..theater.lookup.len() as u16)
        .map(|tile| {
            let filename = theater
                .lookup
                .filename_for_variant(tile, 0)
                .expect("registered tile filename")
                .to_string();
            let land_byte = if filename.starts_with("road") { 11 } else { 0 };
            let tmp = gsi_04_02_last_tiles_tmp_bytes(land_byte, [1, 2, 3], [4, 5, 6]);
            (filename, tmp)
        })
        .collect();
    let files: Vec<(&str, &[u8])> = tmps
        .iter()
        .map(|(name, bytes)| (name.as_str(), bytes.as_slice()))
        .collect();
    gsi_04_02_asset_manager_with_loose_tmps(&files)
}

fn anim_cell(rx: u16, ry: u16, tile_index: i32, sub_tile: u8, z: u8) -> MapCell {
    MapCell {
        legacy_ice_byte: 0,
        rx,
        ry,
        tile_index,
        sub_tile,
        z,
    }
}
