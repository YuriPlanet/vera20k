//! Machine-readable output shapes for the `asset` tool.
//!
//! These DTOs are the tool's contract with its callers (AI agents). They are
//! deliberately *not* the parser structs: coupling the JSON schema to
//! `ShpFile`/`TmpFile`/... would let a parser field addition silently change
//! the agent-facing shape. Every verb serialises one of these.
//!
//! ## Dependency rules
//! - Depends on `serde` only. No parser, archive, or render types leak in here.

use serde::Serialize;

/// One place an asset was found, in a specific archive.
#[derive(Debug, Serialize)]
pub struct Located {
    /// Archive chain that holds it, e.g. `ra2.mix -> sidec02.mix`.
    pub archive: String,
    /// Hashed entry ID as it appears in the archive index.
    pub entry_id: String,
    pub size: u32,
    /// Short format tag from the sniffer, e.g. `shp`, `pal`, `unknown`.
    pub format: String,
    /// Human-readable structural detail, e.g. `SHP(TS) 16x2, 5 frames`.
    pub detail: String,
    /// Set when this hit is not reachable through normal name lookup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// `asset find` — where an asset lives and what shadows it.
#[derive(Debug, Serialize)]
pub struct FindReport {
    pub name: String,
    pub found: bool,
    pub hash_crc32: String,
    pub hash_westwood: String,
    /// The archive that normal lookup actually resolves to.
    pub winner: Option<Located>,
    /// True when a loose file on disk shadows every archive copy.
    pub loose_override: bool,
    /// Other archives holding the same entry ID, in search order after the winner.
    pub shadowed_by: Vec<Located>,
    /// Hits in catalogued-but-unregistered nested archives. These are real bytes
    /// that name lookup cannot reach — reported separately so a present asset is
    /// never mistaken for a missing one.
    pub catalog_only: Vec<Located>,
    pub name_db: NameDb,
}

/// Which filename dictionary backed a hash reverse-lookup. `IniOnly` means
/// unidentified rows are expected — it is degraded output, not a real miss.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NameDb {
    /// XCC global mix database merged with the built-in and INI-derived names.
    XccIni,
    /// XCC unavailable; built-in plus INI-derived names only.
    IniOnly,
}

/// One row of `asset ls`.
#[derive(Debug, Serialize)]
pub struct LsRow {
    pub index: usize,
    pub entry_id: String,
    pub size: u32,
    pub name: String,
    /// False when `name` is a placeholder because no dictionary entry matched.
    pub identified: bool,
    pub format: String,
    pub detail: String,
}

/// `asset ls` — a paged listing of one archive.
#[derive(Debug, Serialize)]
pub struct LsReport {
    pub archive: String,
    /// Entries in the archive, before any filter.
    pub entry_count: usize,
    /// Entries left after `--filter`/`--format`. Equals `entry_count` when
    /// neither was passed.
    pub matched: usize,
    pub total_bytes: usize,
    /// How many of the archive's entries the dictionary could name, before
    /// filtering. A low ratio means degraded naming, not a small archive.
    pub identified_count: usize,
    pub shown: usize,
    pub offset: usize,
    pub limit: usize,
    pub name_db: NameDb,
    pub entries: Vec<LsRow>,
}

/// `asset archives` support shape: one mounted archive.
#[derive(Debug, Serialize)]
pub struct ArchiveRow {
    pub name: String,
    pub entry_count: usize,
    pub total_bytes: usize,
    /// False for catalogued nested archives that name lookup cannot reach.
    pub name_lookup_reachable: bool,
}

/// Per-frame geometry from an SHP header.
#[derive(Debug, Serialize)]
pub struct ShpFrameInfo {
    pub index: usize,
    /// Frame origin inside the file canvas.
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
    /// Raw byte-8 format bitfield; bit 1 selects row-framed RLE-Zero.
    pub format: u8,
    pub compressed: bool,
    /// Per-frame radar/minimap colour baked into the frame header.
    pub radar_color: [u8; 3],
    pub pixel_count: usize,
    /// Count of index-0 (transparent) pixels.
    pub index0_count: usize,
    pub nonzero_count: usize,
}

/// Per-tile fields from a TMP template.
#[derive(Debug, Serialize)]
pub struct TmpTileInfo {
    pub index: usize,
    pub present: bool,
    pub height: u8,
    pub terrain_type: u8,
    pub ramp_type: u8,
    pub radar_left: [u8; 3],
    pub radar_right: [u8; 3],
    pub pixel_w: u32,
    pub pixel_h: u32,
    pub offset_x: i32,
    pub offset_y: i32,
    pub has_damaged_data: bool,
}

/// One voxel limb (section).
#[derive(Debug, Serialize)]
pub struct VxlLimbInfo {
    pub index: usize,
    pub name: String,
    pub size: [u8; 3],
    pub scale: f32,
    pub bounds: [f32; 6],
    pub transform: [f32; 12],
    pub normals_mode: u8,
    pub voxel_count: usize,
}

/// The paired `.hva` animation for a voxel, when one resolves.
#[derive(Debug, Serialize)]
pub struct HvaInfo {
    pub name: String,
    pub frame_count: u32,
    pub section_count: u32,
    pub section_names: Vec<String>,
    /// Section names present in the HVA but absent from the VXL limb list
    /// (and vice versa) — a mismatch here breaks voxel animation.
    pub unmatched_hva_sections: Vec<String>,
    pub unmatched_vxl_limbs: Vec<String>,
}

/// A grid of palette indices for one small frame. Cheaper and more precise
/// than an image for the tiny sidebar/chrome sprites.
#[derive(Debug, Serialize)]
pub struct AsciiGrid {
    pub frame: usize,
    pub w: u16,
    pub h: u16,
    /// One string per row; cells are two hex digits separated by a space,
    /// with `..` for index 0.
    pub rows: Vec<String>,
}

/// Format-specific body of an `asset info` report.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum InfoBody {
    Shp {
        canvas: [u16; 2],
        frame_count: usize,
        frames_shown: usize,
        frames: Vec<ShpFrameInfo>,
        #[serde(skip_serializing_if = "Option::is_none")]
        ascii: Option<AsciiGrid>,
    },
    Tmp {
        template: [u32; 2],
        tile: [u32; 2],
        tile_count: usize,
        present_count: usize,
        tiles_shown: usize,
        tiles: Vec<TmpTileInfo>,
    },
    Vxl {
        limb_count: u32,
        body_size: u32,
        limbs: Vec<VxlLimbInfo>,
        #[serde(skip_serializing_if = "Option::is_none")]
        hva: Option<HvaInfo>,
    },
    Hva {
        frame_count: u32,
        section_count: u32,
        section_names: Vec<String>,
    },
    Pal {
        /// All 256 entries as `#RRGGBB`.
        colors: Vec<String>,
        /// The house-colour remap band, `[16, 32)`.
        remap_band: [usize; 2],
        /// True when every byte was <= 63, i.e. an unscaled VGA 6-bit palette.
        vga_6bit: bool,
        unique_colors: usize,
    },
    Csf {
        version: u32,
        language: u32,
        entry_count: usize,
    },
    Aud {
        sample_rate: u16,
        channels: u16,
        is_16bit: bool,
        format: u8,
        data_size: u32,
        output_size: u32,
    },
    Pcx {
        w: u16,
        h: u16,
        /// PCX carries its own palette; no external .pal is needed.
        embedded_palette: bool,
    },
    Fnt {
        cell_height: u32,
        bitmap_rows: u32,
        bytes_per_row: u32,
        glyph_stride: u32,
    },
    Vpl {
        first_remap: u32,
        last_remap: u32,
        num_sections: u32,
    },
    /// Parsed by no Phase-1 reader; the sniffer's detail line is all there is.
    Opaque {},
}

/// `asset info` — everything known about one asset without rendering it.
#[derive(Debug, Serialize)]
pub struct InfoReport {
    pub name: String,
    pub source_archive: String,
    pub entry_id: String,
    pub bytes: usize,
    pub format: String,
    pub detail: String,
    pub body: InfoBody,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// How a palette was selected, and how much to trust it.
#[derive(Debug, Clone, Serialize)]
pub struct PaletteChoice {
    pub name: String,
    /// Why this palette won — see `asset_tools::palette::PaletteReason`.
    pub reason: String,
    /// `production` when a real engine code path binds this palette to this
    /// asset class, `declared` when art.ini named it, `heuristic` otherwise.
    pub confidence: String,
    /// For `production`, the code path the binding was read from. This is the
    /// difference between "the engine does this" and "this looked plausible".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub production_site: Option<String>,
}

/// One archive's copy of a name, for `asset compare`.
#[derive(Debug, Serialize)]
pub struct CompareVariant {
    pub archive: String,
    pub entry_id: String,
    pub size: u32,
    pub format: String,
    pub detail: String,
    /// False when only the catalogue sweep found it — the engine's own lookup
    /// would not resolve this copy by name.
    pub reachable: bool,
    /// Structural fields, for the field-by-field diff.
    pub fields: std::collections::BTreeMap<String, String>,
    /// Rendered PNG for this variant, when the format could be drawn.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub png: Option<String>,
}

/// `asset compare` — every archive's copy of one name, side by side.
#[derive(Debug, Serialize)]
pub struct CompareReport {
    pub name: String,
    pub variant_count: usize,
    /// True when the variants are not all structurally identical.
    pub differ: bool,
    /// Field-by-field differences, one line per field that varies.
    pub differences: Vec<String>,
    pub variants: Vec<CompareVariant>,
    pub outputs: RenderOutputs,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// One rejected or untried palette candidate, with why it was proposed.
#[derive(Debug, Serialize)]
pub struct PaletteCandidate {
    pub name: String,
    pub reason: String,
    /// False when the name does not resolve to any loaded archive entry.
    pub exists: bool,
}

/// `asset palette-for` — the inference chain, exposed.
#[derive(Debug, Serialize)]
pub struct PaletteReport {
    pub asset: String,
    pub source_archive: String,
    pub chosen: Option<PaletteChoice>,
    pub candidates: Vec<PaletteCandidate>,
    /// Always present: a palette that merely renders is not evidence it is right.
    pub caveat: String,
}

/// Files written by `asset render`.
#[derive(Debug, Serialize)]
pub struct RenderOutputs {
    pub dir: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet: Option<String>,
    pub frames: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<String>,
}

/// `asset render` — always emitted, so the verb answers its question even
/// when the caller cannot look at an image.
///
/// One shape covers every renderable format; `kind` says how to read the
/// count fields. For `shp` they are frames, for `tmp` tiles, for `vxl` facings,
/// and for `pal` the 256 palette entries.
#[derive(Debug, Serialize)]
pub struct RenderReport {
    pub kind: String,
    pub asset: String,
    pub source_archive: String,
    pub palette: Option<PaletteChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub house_color: Option<u8>,
    pub canvas: [u32; 2],
    pub frame_count: usize,
    pub frames_rendered: Vec<usize>,
    /// Integer upscale applied. Dimensions read off the PNG must be divided by it.
    pub scale: u32,
    /// `canvas` draws each frame in the full file canvas; `crop` draws the bare
    /// frame sub-rect and therefore hides frame_x/frame_y placement.
    pub mode: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub outputs: RenderOutputs,
}

/// `asset extract` — raw bytes written to disk, with their provenance.
#[derive(Debug, Serialize)]
pub struct ExtractReport {
    pub asset: String,
    pub source_archive: String,
    pub entry_id: String,
    pub size: usize,
    pub format: String,
    pub detail: String,
    pub path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// One string-table entry.
#[derive(Debug, Serialize)]
pub struct CsfEntry {
    pub key: String,
    /// The value after the parser's load-time normalisation — what the game shows.
    pub value: String,
    /// Present only when normalisation changed the stored bytes. Retail carries
    /// hundreds of such strings, so a lookup that hid this would misreport them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    pub changed_by_normalization: bool,
}

/// `asset csf-get` / `asset csf-grep`.
#[derive(Debug, Serialize)]
pub struct CsfReport {
    pub source: String,
    pub source_archive: String,
    pub version: u32,
    pub language: u32,
    pub entry_count: usize,
    pub query: String,
    pub matched: usize,
    pub shown: usize,
    pub entries: Vec<CsfEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// One sound entry from the audio bag index.
#[derive(Debug, Serialize)]
pub struct SoundEntry {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    pub sample_rate: u32,
    pub channels: u16,
    pub is_16bit: bool,
    pub is_ima_adpcm: bool,
    pub chunk_size: u32,
    /// Filled only when the entry was decoded, which `bag-ls` does not do.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoded_samples: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Path to a written .wav, when `--wav` was passed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wav: Option<String>,
}

/// `asset sound` / `asset bag-ls`.
#[derive(Debug, Serialize)]
pub struct SoundReport {
    /// Independent consumed IDX/BAG identities, absent when no pair opened.
    pub sources: Option<crate::assets::asset_manager::AudioIndexSources>,
    /// Further admitted Ares BAG pairs in append order. Stock JSON is unchanged.
    #[cfg(feature = "ares")]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub additional_sources: Vec<crate::assets::asset_manager::AudioIndexSources>,
    pub bag: String,
    pub entry_count: usize,
    pub matched: usize,
    pub shown: usize,
    pub entries: Vec<SoundEntry>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// One candidate art filename and whether it resolves.
#[derive(Debug, Serialize)]
pub struct ArtCandidate {
    pub name: String,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_archive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// One building-animation slot declared in art.ini, resolved to its art.
///
/// A building's overlay anims are separate art sections with their own image
/// ids and theater conventions, so "which SHP is playing" is a different
/// question from "which SHP is the building".
#[derive(Debug, Serialize)]
pub struct AnimSlot {
    /// The declared anim type id, e.g. `GAPOWR_A`.
    pub slot: String,
    /// Active, Idle, Super, Special or Production.
    pub kind: String,
    /// False for the suffixed secondary slots (`ActiveAnimTwo` and friends).
    pub is_primary: bool,
    /// Draw offset relative to the building origin.
    pub offset: [i32; 2],
    pub rate: u16,
    /// Slot played while the building is damaged; this slot itself when the
    /// art declares no damaged variant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub damaged_variant: Option<String>,
    /// Slot played while the building is garrisoned; this slot itself when the
    /// art declares no garrisoned variant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub garrisoned_variant: Option<String>,
    pub candidates: Vec<ArtCandidate>,
}

/// `asset art-for` — rules/art id to the files that actually back it.
#[derive(Debug, Serialize)]
pub struct ArtForReport {
    pub type_id: String,
    pub theater: String,
    /// `Image=` as declared, before theater substitution.
    pub declared_image: String,
    pub effective_image_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declared_palette: Option<String>,
    pub cameo_id: String,
    /// Building footprint from art.ini `Foundation=`, e.g. `3x2`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foundation: Option<String>,
    /// `BibShape=`: the ground pad drawn under a building, resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bib_shape: Option<ArtCandidate>,
    pub shp_candidates: Vec<ArtCandidate>,
    pub cameo_candidates: Vec<ArtCandidate>,
    pub voxel_candidates: Vec<ArtCandidate>,
    /// Overlay animation slots, each resolved to its own art.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub building_anims: Vec<AnimSlot>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// One corpus-wide `asset scan` hit.
#[derive(Debug, Serialize)]
pub struct ScanRow {
    pub name: String,
    pub identified: bool,
    pub archive: String,
    pub entry_id: String,
    pub size: u32,
    pub format: String,
    pub detail: String,
    /// The queryable fields this row exposed, so a caller can see what a
    /// predicate would have matched against.
    pub fields: std::collections::BTreeMap<String, String>,
}

/// `asset scan` — search every mounted archive by format and field predicates.
#[derive(Debug, Serialize)]
pub struct ScanReport {
    pub scanned_archives: usize,
    pub scanned_entries: usize,
    pub matched: usize,
    pub shown: usize,
    pub offset: usize,
    pub limit: usize,
    /// The predicates as parsed, echoed so a silent typo is visible.
    pub predicates: Vec<String>,
    pub name_db: NameDb,
    pub rows: Vec<ScanRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// One entry whose parser rejected it.
#[derive(Debug, Serialize)]
pub struct ParseFailure {
    pub archive: String,
    pub entry_id: String,
    pub name: String,
    pub size: usize,
    pub error: String,
}

/// Per-format tally from `asset parse-check`.
#[derive(Debug, Serialize)]
pub struct ParseCheckFormat {
    pub format: String,
    pub ok: u32,
    pub failed: u32,
    pub total_bytes: u64,
    /// A capped sample, not the full list — the count above is authoritative.
    pub failures: Vec<ParseFailure>,
}

/// `asset parse-check` — does every retail entry still parse?
///
/// "ok" means the parser returned Ok: structural validity only, never a
/// statement about matching gamemd semantics.
#[derive(Debug, Serialize)]
pub struct ParseCheckReport {
    pub scanned_archives: usize,
    pub scanned_entries: usize,
    /// Entries the sniffer declined to name; not a failure, just uncovered.
    pub unsniffed: usize,
    pub unsniffed_bytes: u64,
    pub formats: Vec<ParseCheckFormat>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

/// Failure shape. Written to stdout so a caller parsing JSON always gets JSON.
#[derive(Debug, Serialize)]
pub struct ErrorReport {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

/// Serialise any report as pretty JSON. Pretty is deliberate: the consumer is
/// a language model reading stdout, not a byte-counting pipe.
pub fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value)
        .unwrap_or_else(|err| format!("{{\"error\":\"serialisation failed: {err}\"}}"))
}
