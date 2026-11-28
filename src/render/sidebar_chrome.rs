//! Sidebar chrome atlas: loads original RA2 sidebar art, keeping
//! theme-specific radar/background authority separate from generic chrome
//! resolved through the active side MIX stack, then packs the decoded frames
//! into GPU textures for batched rendering.
//!
//! Generic SHP dimensions are side-specific retail data, so callers consume
//! the dimensions decoded from each selected archive rather than assuming the
//! Allied layout applies to Soviet or Yuri side-2 art.

use crate::assets::asset_manager::AssetManager;
use crate::assets::mix_archive::MixArchive;
use crate::assets::mix_hash::mix_hash;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture, SpriteInstance};
use crate::render::gpu::GpuContext;
use crate::render::shell_ui_style_assets::{
    ShellUiAssetRef, base_art_candidates, decode_ui_shp, mod_art_candidates, ui_art_candidates,
};
use crate::render::sidebar_smoothing::{self, SidebarPieces, SmoothCopy};
use crate::ui::match_layout::MatchLayout;
use crate::ui::sidebar::{ArtKey, SidebarPart};

/// Each part's pictures, by [`SidebarPart`] and [`ArtKey`]: every frame of
/// the file, `None` where the file lacks it.
type Pieces =
    [[Vec<Option<SidebarChromeEntry>>; ArtKey::ALL.len()]; SidebarPart::STOCK_ORDER.len()];

#[path = "sidebar_chrome_command_bar.rs"]
mod command_bar;
pub use command_bar::CommandBarArt;

#[path = "sidebar_chrome_in_game_shell.rs"]
mod in_game_shell;
pub use in_game_shell::InGameShellArt;
#[path = "sidebar_chrome_layout_art.rs"]
mod layout_art;
pub use layout_art::{LayoutArt, NamedPictures, PictureSource};
#[path = "sidebar_chrome_observer.rs"]
mod observer_art;
pub use observer_art::ObserverArt;

#[cfg(test)]
pub(crate) fn packed_command_bar_fixture(
    assets: &AssetManager,
    theme: SidebarTheme,
) -> (CommandBarArt<SidebarChromeEntry>, Vec<u8>, [u32; 2]) {
    let route = SidebarSideRoute::for_theme(assets, theme);
    let palette = decode_sidebar_palette(route.resolve(SIDEBAR_PALETTE).unwrap().bytes).unwrap();
    let rendered = command_bar::load(route, &palette, theme);
    let width = rendered.entries().map(|entry| entry.width).max().unwrap();
    let height = rendered
        .entries()
        .map(|entry| entry.height + CHROME_PADDING)
        .sum();
    let mut rgba = vec![0; (width * height * 4) as usize];
    let mut y = 0;
    let packed = rendered.map(|entry| {
        let packed = blit_entry(&mut rgba, width, height, y, entry);
        y += entry.height + CHROME_PADDING;
        packed
    });
    (packed, rgba, [width, height])
}

const CHROME_PADDING: u32 = 2;
// RadarClass::Draw 653100: frame 0 is offline, frame 32 is online.
const RADAR_DEFAULT_FRAME: usize = 0;
const ALLIED_SIDE_ARCHIVE_ORDER: &[&str] = &["sidec01md.mix", "sidec01.mix", "sidenc01.mix"];
const SIDE_TWO_ARCHIVE_ORDER: &[&str] = &["sidec02md.mix", "sidec02.mix", "sidenc02.mix"];
/// The sidebar's own palette, which its generic chrome is drawn with.
const SIDEBAR_PALETTE: &str = "SIDEBAR.PAL";

/// The built-in `veraui_default.ini`'s `key` of the part `node` for the side
/// whose pictures are `theme`: the file of a picture that a layout cannot
/// change yet (`LayoutProblemKind::NotYet` keeps the original's), and the
/// one the radar's Ares filename switch picks. SidebarClass::LoadSHPs
/// 6A5840 loads SIDE1..3, ADDON, GCLOCK2 and the button shapes (FreeSHPs
/// 6A5BF0 frees them), the file's pictures; its observer backgrounds and
/// flags are not side art, which `observer_art` finds.
fn built_in_name(node: &str, key: &str, theme: SidebarTheme) -> Option<&'static str> {
    MatchLayout::original(true).node_text(node, key, theme)
}

pub use crate::ui::sidebar::SidebarTheme;

/// One logical sidebar input and the archive selected by the existing resolver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarChromeAssetIdentity {
    pub logical_name: String,
    pub source_archive: Option<String>,
}

/// Immutable source identity captured when one theme atlas is constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SidebarChromeAtlasIdentity {
    pub atlas_theme: SidebarTheme,
    pub parent_archive: String,
    pub radar: SidebarChromeAssetIdentity,
    pub theme_palette: SidebarChromeAssetIdentity,
    pub generic_palette: SidebarChromeAssetIdentity,
    pub background_palette: Option<SidebarChromeAssetIdentity>,
    pub backgrounds: Option<[SidebarChromeAssetIdentity; 3]>,
}

/// Requested-versus-actual identity for one atlas lookup, including fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSidebarChromeIdentity {
    pub requested_theme: SidebarTheme,
    pub actual_theme: SidebarTheme,
    pub atlas: SidebarChromeAtlasIdentity,
}

/// Borrowed atlas resolution used to keep selection and provenance atomic.
pub struct ResolvedSidebarChrome<'a> {
    pub requested_theme: SidebarTheme,
    pub actual_theme: SidebarTheme,
    pub atlas: &'a SidebarChromeAtlas,
}

impl ResolvedSidebarChrome<'_> {
    pub fn identity(&self) -> ResolvedSidebarChromeIdentity {
        ResolvedSidebarChromeIdentity {
            requested_theme: self.requested_theme,
            actual_theme: self.actual_theme,
            atlas: self.atlas.source_identity().clone(),
        }
    }
}

#[derive(Clone, Copy)]
struct SidebarSideRoute<'a> {
    asset_manager: &'a AssetManager,
    archive_names: &'static [&'static str],
    registered_group: bool,
}

impl<'a> SidebarSideRoute<'a> {
    fn for_theme(asset_manager: &'a AssetManager, theme: SidebarTheme) -> Self {
        let archive_names = match theme {
            SidebarTheme::Allied => ALLIED_SIDE_ARCHIVE_ORDER,
            SidebarTheme::Soviet | SidebarTheme::Yuri => SIDE_TWO_ARCHIVE_ORDER,
        };
        Self {
            asset_manager,
            archive_names,
            registered_group: false,
        }
    }

    fn for_registered_group(asset_manager: &'a AssetManager) -> Self {
        Self {
            asset_manager,
            archive_names: &[],
            registered_group: true,
        }
    }

    fn resolve(&self, asset_name: &str) -> Option<ResolvedSidebarAsset<'a>> {
        if self.registered_group {
            // The selected Side MIX group is registered by AssetManager.
            // Loose/earlier expansion overrides keep CCFile first-match order.
            let selected = self.asset_manager.resolve_ref(asset_name)?;
            return Some(ResolvedSidebarAsset {
                archive_name: selected.source_archive,
                bytes: selected.bytes,
            });
        }
        resolve_sidebar_asset_in_order(self.side_archives(), asset_name)
    }

    fn side_archives(&self) -> impl Iterator<Item = (&'static str, &'a MixArchive)> {
        self.archive_names.iter().filter_map(|&archive_name| {
            self.asset_manager
                .archive(archive_name)
                .map(|archive| (archive_name, archive))
        })
    }

    fn decode<T>(
        &self,
        asset_name: &str,
        mut decode: impl FnMut(ShellUiAssetRef<'a>) -> Option<T>,
    ) -> Option<T> {
        if self.registered_group {
            return ui_art_candidates(self.asset_manager, asset_name).find_map(decode);
        }
        if self.asset_manager.mod_dir().is_none() {
            let selected = self.resolve(asset_name)?;
            return decode(ShellUiAssetRef {
                bytes: selected.bytes,
                source: selected.archive_name,
            });
        }
        // An inactive Side MIX is catalogued off the ordinary file search.
        // Use its own side's ordered chain after loose and registered mod art.
        mod_art_candidates(self.asset_manager, asset_name)
            .find_map(&mut decode)
            .or_else(|| {
                self.side_archives().find_map(|(source, archive)| {
                    let bytes = archive.get_by_name(asset_name)?;
                    decode(ShellUiAssetRef { bytes, source })
                })
            })
    }

    fn render(&self, name: &str, palette: &Palette, frame: usize) -> Option<RenderedChromeEntry> {
        self.decode(name, |asset| {
            let shp = parse_sidebar_shape(self.asset_manager, asset.bytes)?;
            render_shp(&shp, palette, frame)
        })
    }
}

#[derive(Clone, Copy)]
struct ResolvedSidebarAsset<'a> {
    archive_name: &'a str,
    bytes: &'a [u8],
}

fn resolve_sidebar_asset_in_order<'a>(
    archives: impl IntoIterator<Item = (&'static str, &'a MixArchive)>,
    asset_name: &str,
) -> Option<ResolvedSidebarAsset<'a>> {
    archives.into_iter().find_map(|(archive_name, archive)| {
        archive
            .get_by_name(asset_name)
            .map(|bytes| ResolvedSidebarAsset {
                archive_name,
                bytes,
            })
    })
}

/// UV coordinates and pixel dimensions for one chrome piece in the atlas.
#[derive(Debug, Clone, Copy)]
pub struct SidebarChromeEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_size: [f32; 2],
}

impl SidebarChromeEntry {
    /// Source columns `x..x + w` as a piece of their own; all of them are
    /// this piece unchanged.
    pub fn columns(self, x: f32, w: f32) -> Self {
        if x == 0.0 && w == self.pixel_size[0] {
            return self;
        }
        let per_pixel = self.uv_size[0] / self.pixel_size[0];
        Self {
            uv_origin: [self.uv_origin[0] + x * per_pixel, self.uv_origin[1]],
            uv_size: [w * per_pixel, self.uv_size[1]],
            pixel_size: [w, self.pixel_size[1]],
        }
    }
}

#[derive(Debug, Clone)]
pub struct SidebarChromeExtraEntry {
    pub id: i32,
    pub label: String,
    pub frame_count: usize,
    pub entry: SidebarChromeEntry,
}

pub(crate) struct SidebarDarkenMask {
    pub texture: BatchTexture,
    pub frame_origin: [f32; 2],
}

impl SidebarDarkenMask {
    pub(crate) fn from_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        bytes: &[u8],
        palette: &Palette,
    ) -> Option<Self> {
        let shp = ShpFile::from_bytes(bytes).ok()?;
        let frame = shp.frames.first()?;
        let width = u32::from(frame.frame_width);
        let height = u32::from(frame.frame_height);
        if width == 0 || height == 0 {
            return None;
        }
        // Unavailable496250 uses indices alone; flash4963F0 uses this same
        // frame with the sidebar's ordinary N=1 palette conversion.
        let rgba = shp.frame_to_rgba(0, palette).ok()?;
        Some(Self {
            texture: batch.create_texture_on_device(
                device,
                queue,
                &rgba,
                width,
                height,
                Some(&frame.pixels),
            ),
            frame_origin: [f32::from(frame.frame_x), f32::from(frame.frame_y)],
        })
    }

    pub(crate) fn load(
        gpu: &GpuContext,
        batch: &BatchRenderer,
        assets: &AssetManager,
        palette: &Palette,
        theme: SidebarTheme,
    ) -> Option<Self> {
        // SidebarClass::One_Time 6A5000 (6A500B..6A5018): literal DARKEN.SHP
        // (the cameo grid's `Darken=`) through LoadFileFromMIX, not the
        // side-only generic chrome resolver.
        let name = built_in_name(SidebarPart::Cameos.name(), "Darken", theme)?;
        let source = assets.load_file_from_mix(name)?;
        log::info!("Sidebar {name} resolved from {}", source.source_archive);
        Self::from_bytes(&gpu.device, &gpu.queue, batch, &source.bytes, palette)
    }
}

/// All sidebar chrome art for one faction theme, packed into a single texture.
pub struct SidebarChromeAtlas {
    source_identity: SidebarChromeAtlasIdentity,
    pub texture: BatchTexture,
    pub command_bar: CommandBarArt<SidebarChromeEntry>,
    pub in_game_shell: InGameShellArt<SidebarChromeEntry>,
    /// The pictures of the buttons the layout file adds (`Art=`).
    pub layout_art: LayoutArt<SidebarChromeEntry>,
    /// The observer strip's backgrounds and flags.
    pub observer_art: ObserverArt<SidebarChromeEntry>,
    pub background_large: Option<SidebarChromeEntry>,
    pub background_medium: Option<SidebarChromeEntry>,
    pub background_small: Option<SidebarChromeEntry>,
    /// All pre-rendered RGBA frames of radar.shp for the opening/closing animation.
    /// Frame 0 = closed/offline housing; frame 32 is the opening endpoint.
    pub radar_frames: Vec<Vec<u8>>,
    /// Pixel dimensions of each radar frame (width, height).
    pub radar_frame_size: [u32; 2],
    /// Fixed native aperture from RadarClass One_Time (`652CF0`).
    /// [left, top, right, bottom] in unscaled pixels. The minimap fills the area
    /// inside these insets. Derived automatically — no manual tuning needed.
    pub radar_content_insets: [u32; 4],
    /// The pictures `ui::match_layout::sidebar_art` lists, by part and
    /// picture key, each with its frames; see [`SidebarChromeAtlas::art`].
    pieces: Pieces,
    pub extra_entries: Vec<SidebarChromeExtraEntry>,
    /// GCLOCK2.SHP progress clock overlay — all frames packed into a grid texture.
    /// Each entry maps a frame index to UV coordinates in gclock_texture.
    pub gclock_texture: Option<BatchTexture>,
    pub gclock_frames: Vec<SidebarChromeEntry>,
    /// Global DARKEN.SHP source, converted with this side's ordinary N=1
    /// palette (SidebarLoadSHPs6A588C). Its resolver is LoadFileFromMIX.
    pub(crate) darken: Option<SidebarDarkenMask>,
    /// The pieces the sidebar's column and command bar draw, kept for VERA's
    /// smooth sidebar (`render::sidebar_smoothing`).
    sidebar_pieces: SidebarPieces,
    /// Their copy at the sidebar's draw scale while it is on.
    smooth: Option<SmoothCopy>,
}

impl SidebarChromeAtlas {
    /// Frame `frame` of picture `key` (`Art`, `RowArt`, `EndArt`,
    /// `FootArt`, ...) of the `veraui.ini` part `node`, as
    /// `ui::match_layout::sidebar_art` lists them. `None` when the part has
    /// no such picture or the file no such frame.
    pub fn art(&self, part: SidebarPart, key: ArtKey, frame: usize) -> Option<SidebarChromeEntry> {
        self.pieces[part as usize][key as usize]
            .get(frame)
            .copied()
            .flatten()
    }

    pub fn source_identity(&self) -> &SidebarChromeAtlasIdentity {
        &self.source_identity
    }

    /// Follow the smooth sidebar's scale; None draws the atlas's own pixels.
    pub fn set_smooth_scale(
        &mut self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        scale: Option<u32>,
    ) {
        let source = self.sidebar_pieces.source();
        sidebar_smoothing::follow_scale(&mut self.smooth, source, scale, gpu, batch);
    }

    /// The smooth copy of the sidebar's pieces, when there is one.
    pub fn smooth_texture(&self) -> Option<&BatchTexture> {
        self.smooth.as_ref().and_then(SmoothCopy::texture)
    }

    /// Point `instances`, drawn from this atlas by the sidebar's column or
    /// command bar, at the same pixels of the smooth copy, and say whether
    /// they were. They stay on the atlas when there is no copy, or one draws
    /// a piece it lacks.
    pub fn retarget_to_smooth(&self, instances: &mut [SpriteInstance]) -> bool {
        self.smooth_texture().is_some() && self.sidebar_pieces.retarget(instances)
    }
}

/// The selected side is a runtime index, not another hardcoded faction enum.
/// Retail keeps its three prebuilt themes; Ares prepares the current scenario's
/// selected group through the same atlas builder and resolver.
pub struct SidebarChromeSet {
    atlases: Vec<(i32, SidebarChromeAtlas)>,
    /// Stock themes whose art the archives lack, so that
    /// [`SidebarChromeSet::ensure_side`] asks for each once.
    missing: Vec<SidebarTheme>,
    /// Whether [`build_sidebar_chrome_set`]'s Ares arm built it (the scenario
    /// side's registered Side MIX group) rather than its stock arm (the stock
    /// themes, built as sides draw them). Resolution follows the arm that
    /// built the set.
    ares: bool,
    /// The `veraui.ini` layout whose pictures each stock theme is built from
    /// (by `SidebarTheme as usize`): the layout of the side that draws it,
    /// `None` for the built-in pictures.
    layouts: [Option<MatchLayout>; 3],
}

impl SidebarChromeSet {
    /// Whether the Ares arm built this set; see [`sidebar_theme_for_side`].
    pub fn ares(&self) -> bool {
        self.ares
    }

    pub fn for_theme(&self, theme: SidebarTheme) -> Option<&SidebarChromeAtlas> {
        self.resolve_theme(theme).map(|resolved| resolved.atlas)
    }

    pub fn resolve_theme(&self, theme: SidebarTheme) -> Option<ResolvedSidebarChrome<'_>> {
        self.resolve_side(stock_theme_side(theme))
    }

    pub fn resolve_side(&self, side_index: i32) -> Option<ResolvedSidebarChrome<'_>> {
        let (actual_theme, index) = self.resolve_index(side_index)?;
        Some(ResolvedSidebarChrome {
            requested_theme: sidebar_theme_for_side(side_index, self.ares),
            actual_theme,
            atlas: &self.atlases[index].1,
        })
    }

    /// The theme and the place in `atlases` of the atlas `side_index` draws.
    fn resolve_index(&self, side_index: i32) -> Option<(SidebarTheme, usize)> {
        let position = |side| self.atlases.iter().position(|(s, _)| *s == side);
        let resolved = if self.ares {
            let index = position(side_index)?;
            (self.atlases[index].1.source_identity.atlas_theme, index)
        } else {
            let (allied, soviet, yuri) = (position(0), position(1), position(2));
            let (theme, &index) = select_sidebar_theme(
                sidebar_theme_for_side(side_index, self.ares),
                allied.as_ref(),
                soviet.as_ref(),
                yuri.as_ref(),
            )?;
            (theme, index)
        };
        Some(resolved)
    }

    /// Build the atlas `side_index` draws when the set lacks it: the side's
    /// theme, else the first of its fallbacks the archives hold
    /// ([`theme_fallbacks`]). A stock match builds the local player's side at
    /// load and another side here, on the frame a change of local house first
    /// draws it, instead of all three at load: each is a 25 MiB texture, and
    /// a shared-memory chip's few hundred MiB hold the whole match. The Ares
    /// arm built its one side at load and builds nothing here.
    pub fn ensure_side(
        &mut self,
        side_index: i32,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        asset_manager: &AssetManager,
    ) {
        if self.ares {
            return;
        }
        let requested = sidebar_theme_for_side(side_index, false);
        while let Some(theme) = next_theme_to_build(
            requested,
            |theme| {
                self.atlases
                    .iter()
                    .any(|(side, _)| *side == stock_theme_side(theme))
            },
            &self.missing,
        ) {
            let (archive, yuri) = stock_theme_source(theme);
            let layout = self.layouts[theme as usize]
                .as_ref()
                .unwrap_or(MatchLayout::original(true));
            match build_theme_atlas(
                gpu,
                batch,
                asset_manager,
                theme,
                archive,
                SidebarArtNames::for_yuri(yuri),
                false,
                layout,
            ) {
                Some(atlas) => self.atlases.push((stock_theme_side(theme), atlas)),
                None => self.missing.push(theme),
            }
        }
    }

    /// Follow the smooth sidebar's scale with the atlas `side_index` draws;
    /// the others drop their copies.
    pub fn set_smooth_scale(
        &mut self,
        side_index: i32,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        scale: Option<u32>,
    ) {
        let current = self.resolve_index(side_index).map(|(_, index)| index);
        for (index, (_, atlas)) in self.atlases.iter_mut().enumerate() {
            let scale = scale.filter(|_| current == Some(index));
            atlas.set_smooth_scale(gpu, batch, scale);
        }
    }
}

/// A file as Full_Init finds it once `InitSideMixFiles` (`0x00534FA0`, from
/// `0x0068781F`) has mounted the side's MIX group at the tail of the search
/// list: the ordinary search first, then the group. Side 2 reads side 1's
/// group (`0x00534FB1`). Stock VERA keeps every side's group off the ordinary
/// search and reads it by side, so a file only the group carries, such as
/// PBEACON.SHP, is found here.
pub(crate) fn side_mix_file<'a>(
    asset_manager: &'a AssetManager,
    side_index: i32,
    ares: bool,
    name: &str,
) -> Option<&'a [u8]> {
    if let Some(bytes) = asset_manager.get_ref(name) {
        return Some(bytes);
    }
    let route = if ares {
        SidebarSideRoute::for_registered_group(asset_manager)
    } else {
        SidebarSideRoute::for_theme(asset_manager, sidebar_theme_for_side(side_index, false))
    };
    route.resolve(name).map(|resolved| resolved.bytes)
}

/// Sidebar geometry6A5090 is side-zero versus nonzero. The Yuri label keeps
/// existing stock provenance; with `ares`, the arm of
/// [`build_sidebar_chrome_set`] that drew the side's own Side MIX group
/// ([`SidebarChromeSet::ares`]), an extra side uses the same nonzero geometry,
/// independently of its MIX index and Yuri filename switch. The stock sidebar
/// draws an extra side as Allied.
pub(crate) fn sidebar_theme_for_side(side_index: i32, ares: bool) -> SidebarTheme {
    match side_index {
        0 => SidebarTheme::Allied,
        1 => SidebarTheme::Soviet,
        2 => SidebarTheme::Yuri,
        _ if ares => SidebarTheme::Soviet,
        _ => SidebarTheme::Allied,
    }
}

/// The themes `theme` draws, in order: its own art, else an earlier side's.
/// gamemd's side 2 reads side 1's group (`0x00534FB1`), and a game whose
/// archives lack that falls back to Allied.
const fn theme_fallbacks(theme: SidebarTheme) -> &'static [SidebarTheme] {
    match theme {
        SidebarTheme::Allied => &[SidebarTheme::Allied],
        SidebarTheme::Soviet => &[SidebarTheme::Soviet, SidebarTheme::Allied],
        SidebarTheme::Yuri => &[
            SidebarTheme::Yuri,
            SidebarTheme::Soviet,
            SidebarTheme::Allied,
        ],
    }
}

/// The stock archive and Yuri filename switch `theme` is built from, as
/// `InitSideMixFiles` (`0x00534FA0`) mounts them.
const fn stock_theme_source(theme: SidebarTheme) -> (&'static str, bool) {
    match theme {
        SidebarTheme::Allied => ("sidec01.mix", false),
        SidebarTheme::Soviet => ("sidec02.mix", false),
        SidebarTheme::Yuri => ("sidec02md.mix", true),
    }
}

/// The stock side index that draws `theme` ([`sidebar_theme_for_side`]).
const fn stock_theme_side(theme: SidebarTheme) -> i32 {
    match theme {
        SidebarTheme::Allied => 0,
        SidebarTheme::Soviet => 1,
        SidebarTheme::Yuri => 2,
    }
}

/// The theme [`SidebarChromeSet::ensure_side`] builds next for `requested`:
/// the first of its themes ([`theme_fallbacks`]) that is not `missing`,
/// unless that one is `built`, in which case nothing is left to build.
fn next_theme_to_build(
    requested: SidebarTheme,
    built: impl Fn(SidebarTheme) -> bool,
    missing: &[SidebarTheme],
) -> Option<SidebarTheme> {
    let theme = *theme_fallbacks(requested)
        .iter()
        .find(|theme| !missing.contains(theme))?;
    (!built(theme)).then_some(theme)
}

pub(crate) fn select_sidebar_theme<'a, T>(
    requested_theme: SidebarTheme,
    allied: Option<&'a T>,
    soviet: Option<&'a T>,
    yuri: Option<&'a T>,
) -> Option<(SidebarTheme, &'a T)> {
    theme_fallbacks(requested_theme).iter().find_map(|&theme| {
        match theme {
            SidebarTheme::Allied => allied,
            SidebarTheme::Soviet => soviet,
            SidebarTheme::Yuri => yuri,
        }
        .map(|atlas| (theme, atlas))
    })
}

/// Ares1002EAA0/1002FA60/1002FA70 consume one independent filename
/// switch, not the actual side index or the selected MIX number.
#[derive(Clone, Copy)]
struct SidebarArtNames {
    /// The switch: the Yuri file names.
    yuri: bool,
    background_palette: &'static str,
    backgrounds: (&'static str, &'static str, &'static str),
}

impl SidebarArtNames {
    const fn for_yuri(yuri: bool) -> Self {
        if yuri {
            Self {
                yuri,
                background_palette: "UIBKGDY.PAL",
                backgrounds: ("bkgdlgy.shp", "bkgdmdy.shp", "bkgdsmy.shp"),
            }
        } else {
            Self {
                yuri,
                background_palette: "UIBKGD.PAL",
                backgrounds: ("bkgdlg.shp", "bkgdmd.shp", "bkgdsm.shp"),
            }
        }
    }

    /// The pictures whose radar entries of the built-in `veraui_default.ini`
    /// (`Art@Yuri=`, `Palette@Yuri=`) the radar of `theme` draws with. The
    /// one override of the file's own choice: Ares picks the radar's file
    /// and palette by the switch, not by the side's pictures, so an Ares
    /// side 1 with the switch on draws Yuri's radar in a Soviet sidebar, and
    /// side 2 with it off the Soviet radar. Without Ares the switch is the
    /// Yuri theme's own (`stock_theme_source`), so this is `theme`.
    fn radar_theme(self, theme: SidebarTheme) -> SidebarTheme {
        match (self.yuri, theme) {
            (true, _) => SidebarTheme::Yuri,
            (false, SidebarTheme::Yuri) => SidebarTheme::Soviet,
            (false, theme) => theme,
        }
    }

    /// The radar's built-in picture and palette for `theme`.
    fn radar(self, theme: SidebarTheme) -> (&'static str, &'static str) {
        let theme = self.radar_theme(theme);
        let name = |key| {
            built_in_name(SidebarPart::Radar.name(), key, theme)
                .expect("veraui_default.ini names the radar's picture and palette")
        };
        (name("Art"), name("Palette"))
    }
}

/// The scenario side's retained Side sidebar configuration (its MIX index and
/// Yuri filename switch) when Ares plays in the game (`RuleSet::ares`), which
/// selects the side's MIX group (Ares1002FA00). A side without one draws the
/// stock sidebar: a game whose Rules were not read the Ares way has no Side
/// extensions.
pub(crate) fn ares_sidebar_config(
    rules: &crate::rules::ruleset::RuleSet,
    side_index: i32,
) -> Option<(i32, bool)> {
    let side = crate::rules::ruleset::SideIdx(u8::try_from(side_index).ok()?);
    if !rules.ares() {
        return None;
    }
    Some((
        rules.side_sidebar_mix_file_index(side)?,
        rules.side_sidebar_yuri_file_names(side)?,
    ))
}

/// The match's sidebar chrome: the scenario side's own Side MIX group when
/// Ares plays in the game (`side_index`), else the stock theme of the side
/// the local player's house draws (`local_side`,
/// `sidebar_render::project_sidebar_side`), with another side's built on the
/// frame that first draws it ([`SidebarChromeSet::ensure_side`]). `None` when
/// the archives hold no sidebar art at all.
///
/// `layout_file` is the mod's `veraui.ini`: each theme's pictures are those
/// of the layout `[Layouts]` gives the side that draws it (its `[Sides]`
/// name, else `Default=`), over the built-in ones.
pub fn build_sidebar_chrome_set(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    asset_manager: &mut AssetManager,
    rules: &crate::rules::ruleset::RuleSet,
    side_index: i32,
    local_side: i32,
    layout_file: Option<&crate::rules::ini_parser::IniFile>,
) -> Result<Option<SidebarChromeSet>, crate::assets::error::AssetError> {
    let layout_file = layout_file.map(crate::ui::match_layout::MatchLayoutFile::from_ini);
    // The layout of side `side`, over the position table its theme uses.
    let layout_of = |side: i32, theme: SidebarTheme| -> Option<MatchLayout> {
        let name = u8::try_from(side)
            .ok()
            .and_then(|side| rules.side_name(crate::rules::ruleset::SideIdx(side)));
        layout_file
            .as_ref()?
            .for_side(name, theme == SidebarTheme::Allied)
            .cloned()
    };
    if let Some((mix_index, yuri_names)) = ares_sidebar_config(rules, side_index) {
        if !asset_manager.activate_sidebar_archives(mix_index)? {
            // Full_Init687824 and saved Load67E766 abort when534FA0 fails.
            return Err(crate::assets::error::AssetError::ParseError {
                format: "sidebar".into(),
                detail: format!(
                    "side {side_index}: sidebar MIX initialization failed for index {mix_index}"
                ),
            });
        }
        let base = format!("sidec{mix_index:02}.mix");
        let md = format!("sidec{mix_index:02}md.mix");
        let primary = if asset_manager.archive(&md).is_some() {
            &md
        } else {
            &base
        };
        let theme = sidebar_theme_for_side(side_index, true);
        let layout = layout_of(side_index, theme);
        let atlas = build_theme_atlas(
            gpu,
            batch,
            asset_manager,
            theme,
            primary,
            SidebarArtNames::for_yuri(yuri_names),
            true,
            layout.as_ref().unwrap_or(MatchLayout::original(true)),
        )
        .ok_or_else(|| crate::assets::error::AssetError::AssetNotFound {
            name: format!("required sidebar shapes or palettes for side {side_index} in {primary}"),
        })?;
        return Ok(Some(SidebarChromeSet {
            atlases: vec![(side_index, atlas)],
            missing: Vec::new(),
            ares: true,
            layouts: Default::default(),
        }));
    }
    let mut set = SidebarChromeSet {
        atlases: Vec::new(),
        missing: Vec::new(),
        ares: false,
        layouts: [
            SidebarTheme::Allied,
            SidebarTheme::Soviet,
            SidebarTheme::Yuri,
        ]
        .map(|theme| layout_of(stock_theme_side(theme), theme)),
    };
    set.ensure_side(local_side, gpu, batch, asset_manager);
    Ok((!set.atlases.is_empty()).then_some(set))
}

struct CpuGclockAtlas {
    rgba: Vec<u8>,
    indices: Vec<u8>,
    width: u32,
    height: u32,
    frames: Vec<SidebarChromeEntry>,
    source_archive: String,
}

/// Resolve all frames of the progress clock `name` (GCLOCK2, the cameo
/// grid's `Clock=`) from the active side stack and pack them into a CPU
/// atlas. The GPU wrapper below consumes this exact output.
fn build_gclock_cpu_atlas(
    route: SidebarSideRoute<'_>,
    name: &str,
    palette: &Palette,
) -> Option<CpuGclockAtlas> {
    route.decode(name, |asset| {
        let shp = parse_sidebar_shape(route.asset_manager, asset.bytes)?;
        build_gclock_cpu_atlas_from_shape(&shp, palette, asset.source)
    })
}

fn build_gclock_cpu_atlas_from_shape(
    shp: &ShpFile,
    palette: &Palette,
    source_archive: &str,
) -> Option<CpuGclockAtlas> {
    let frame_count = shp.frames.len();
    if frame_count == 0 {
        return None;
    }

    let frame_w = shp.width as u32;
    let frame_h = shp.height as u32;
    let mut rendered_frames: Vec<RenderedChromeEntry> = Vec::with_capacity(frame_count);
    for i in 0..frame_count {
        // Use render_shp() to composite onto the full canvas, handling frame_x/frame_y
        // offsets and sub-canvas frame sizes correctly.
        // Retain original indices: compressed 4963F0 skips source zero,
        // including when its palette colour would otherwise be opaque black.
        rendered_frames.push(render_shp(shp, palette, i)?);
    }

    // Pack into a grid texture. 8 columns keeps it roughly square for typical 54-frame SHPs.
    let cols = 8u32.min(frame_count as u32);
    let rows = (frame_count as u32).div_ceil(cols);
    let tex_w = cols * frame_w;
    let tex_h = rows * frame_h;
    let mut atlas_rgba = vec![0u8; (tex_w * tex_h * 4) as usize];
    let mut atlas_indices = vec![0u8; (tex_w * tex_h) as usize];

    let mut entries = Vec::with_capacity(frame_count);
    for (i, frame) in rendered_frames.iter().enumerate() {
        let col = (i as u32) % cols;
        let row = (i as u32) / cols;
        let dst_x = col * frame_w;
        let dst_y = row * frame_h;

        for y in 0..frame_h {
            let src_start = (y * frame_w * 4) as usize;
            let dst_start = (((dst_y + y) * tex_w + dst_x) * 4) as usize;
            let len = (frame_w * 4) as usize;
            if src_start + len <= frame.rgba.len() && dst_start + len <= atlas_rgba.len() {
                atlas_rgba[dst_start..dst_start + len]
                    .copy_from_slice(&frame.rgba[src_start..src_start + len]);
                atlas_indices[dst_start / 4..(dst_start + len) / 4]
                    .copy_from_slice(&frame.indices[src_start / 4..(src_start + len) / 4]);
            }
        }

        entries.push(SidebarChromeEntry {
            uv_origin: [dst_x as f32 / tex_w as f32, dst_y as f32 / tex_h as f32],
            uv_size: [frame_w as f32 / tex_w as f32, frame_h as f32 / tex_h as f32],
            pixel_size: [frame_w as f32, frame_h as f32],
        });
    }

    log::info!(
        "GCLOCK2 atlas for {}: {}x{} px, {} frames ({}x{} grid)",
        source_archive,
        tex_w,
        tex_h,
        frame_count,
        cols,
        rows,
    );

    Some(CpuGclockAtlas {
        rgba: atlas_rgba,
        indices: atlas_indices,
        width: tex_w,
        height: tex_h,
        frames: entries,
        source_archive: source_archive.to_owned(),
    })
}

/// Upload the production CPU atlas without duplicating resolver or decode logic.
fn build_gclock_texture(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    route: SidebarSideRoute<'_>,
    name: &str,
    palette: &Palette,
) -> Option<(BatchTexture, Vec<SidebarChromeEntry>)> {
    let atlas = build_gclock_cpu_atlas(route, name, palette)?;
    log::debug!(
        "Uploading GCLOCK2 from {} ({}x{})",
        atlas.source_archive,
        atlas.width,
        atlas.height
    );
    let texture = batch.create_texture_with_indices(
        gpu,
        &atlas.rgba,
        atlas.width,
        atlas.height,
        Some(&atlas.indices),
    );
    Some((texture, atlas.frames))
}

// Original72ADE0 expands six-bit PAL (the one palette conversion); ordinary
// Convert48E740->4BBB00 N=1 stores exact R5G6B5. Precompose once through the
// enrolled retail output profile; the shared RGBA shader must not apply
// LightConvert row lighting.
fn decode_sidebar_palette(bytes: &[u8]) -> Result<Palette, crate::assets::error::AssetError> {
    let palette = Palette::from_bytes(bytes)?;
    Ok(Palette::from_rgb(|index| {
        let color = palette.color(index);
        let [r, g, b, _] = crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION
            .quantize_rgba8([color.r, color.g, color.b, color.a]);
        [r, g, b]
    }))
}

/// Radar and dialog art have their own theme palettes and a primary Side MIX.
/// The Ares arm uses the currently registered Side group for both inputs.
#[derive(Clone, Copy)]
struct SidebarThemeRoute<'a> {
    assets: &'a AssetManager,
    mix: &'a MixArchive,
    mix_name: &'a str,
    ares: bool,
}

impl<'a> SidebarThemeRoute<'a> {
    fn decode<T>(
        self,
        name: &str,
        mut decode: impl FnMut(&[u8]) -> Option<T>,
    ) -> Option<(T, &'a str)> {
        let mut read = |asset: ShellUiAssetRef<'a>| Some((decode(asset.bytes)?, asset.source));
        if self.ares {
            // Palette72ADE0 reads from current MIX lookup5B4430, so the
            // newly registered Side group can provide its suffixed palettes.
            return ui_art_candidates(self.assets, name).find_map(read);
        }
        mod_art_candidates(self.assets, name)
            .find_map(&mut read)
            .or_else(|| {
                read(ShellUiAssetRef {
                    bytes: self.mix.get_by_name(name)?,
                    source: self.mix_name,
                })
            })
            .or_else(|| base_art_candidates(self.assets, name).find_map(read))
    }

    fn palette(self, name: &str) -> Option<(Palette, &'a str)> {
        self.decode(name, |bytes| decode_sidebar_palette(bytes).ok())
    }

    fn shape(self, name: &str, frame: usize) -> Option<(ShpFile, &'a str)> {
        self.decode(name, |bytes| {
            let shape = parse_sidebar_shape(self.assets, bytes)?;
            shape.frames.get(frame)?;
            Some(shape)
        })
    }

    fn render(
        self,
        name: &str,
        palette: &Palette,
        frame: usize,
    ) -> Option<(RenderedChromeEntry, &'a str)> {
        let (shape, source) = self.shape(name, frame)?;
        Some((render_shp(&shape, palette, frame)?, source))
    }
}

/// One sidebar picture's frames, and its shape when drawn as theme art: in
/// `themed` from the theme route (the radar, a part with a `Palette=`), else
/// in SIDEBAR.PAL from the side route. A layout's `own` file is any SHP the
/// game can find, a mod's loose file first.
fn render_piece<'a>(
    theme_route: SidebarThemeRoute<'a>,
    side_route: SidebarSideRoute<'a>,
    file: &str,
    frames: usize,
    themed: Option<&Palette>,
    generic_palette: &Palette,
    own: bool,
) -> (Vec<Option<RenderedChromeEntry>>, Option<(ShpFile, &'a str)>) {
    if let Some(palette) = themed {
        let shape = theme_route.shape(file, RADAR_DEFAULT_FRAME);
        let rendered = (0..frames)
            .map(|frame| render_shp(&shape.as_ref()?.0, palette, frame))
            .collect();
        return (rendered, shape);
    }
    if own {
        let decode =
            |asset: ShellUiAssetRef<'_>| parse_sidebar_shape(side_route.asset_manager, asset.bytes);
        let shape = ui_art_candidates(side_route.asset_manager, file)
            .find_map(decode)
            .or_else(|| side_route.decode(file, decode));
        let rendered = (0..frames)
            .map(|frame| render_shp(shape.as_ref()?, generic_palette, frame))
            .collect();
        return (rendered, None);
    }
    // The built-in file's generic pictures, through mod art and the active
    // side archive chain.
    let rendered = (0..frames)
        .map(|frame| side_route.render(file, generic_palette, frame))
        .collect();
    (rendered, None)
}

fn build_theme_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    asset_manager: &AssetManager,
    theme: SidebarTheme,
    mix_name: &str,
    names: SidebarArtNames,
    ares: bool,
    layout: &MatchLayout,
) -> Option<SidebarChromeAtlas> {
    let (radar_name, palette_name) = names.radar(theme);
    let mix = asset_manager.archive(mix_name)?;
    let theme_route = SidebarThemeRoute {
        assets: asset_manager,
        mix,
        mix_name,
        ares,
    };
    let (theme_palette, theme_palette_source_archive) = theme_route.palette(palette_name)?;
    let side_route = if ares {
        SidebarSideRoute::for_registered_group(asset_manager)
    } else {
        SidebarSideRoute::for_theme(asset_manager, theme)
    };
    let generic_palette_name = SIDEBAR_PALETTE;
    let (generic_palette, generic_palette_source) = side_route
        .decode(generic_palette_name, |asset| {
            Some((decode_sidebar_palette(asset.bytes).ok()?, asset.source))
        })?;
    let command_bar_art = command_bar::load(side_route, &generic_palette, theme);
    let in_game_shell_art = in_game_shell::load(side_route, &generic_palette);
    let layout_art_rendered = layout_art::load(side_route, &generic_palette);
    let observer_art_rendered = observer_art::load(theme_route);
    log::debug!(
        "{theme:?} generic sidebar palette resolved from {}",
        generic_palette_source
    );
    // The pictures the side's veraui.ini layout gives the sidebar's parts,
    // the built-in file's where it names none. The radar is theme art: its
    // own MIX, the theme palette and, for the built-in picture, the file the
    // Ares filename switch names (`SidebarArtNames`). Every other part is
    // generic chrome of the side route in SIDEBAR.PAL; 652E90 + 653010 draw
    // even Yuri's diplomacy and options buttons with it. A part with a
    // Palette= is drawn as the radar is. A frame the file lacks stays None,
    // and the draw falls back to frame 0 or skips it. A picture or palette
    // of the layout's own that is not found leaves the built-in one.
    let mut radar = None;
    let mut excluded_extra_ids = std::collections::HashSet::new();
    let mut rendered_pieces = Vec::new();
    for art in crate::ui::match_layout::sidebar_art(layout, theme) {
        let is = |part, key| art.part == part && art.key == key;
        let radar_part = art.part == SidebarPart::Radar;
        let built_in = if radar_part {
            radar_name
        } else {
            art.built_in.as_deref().unwrap_or(art.file.as_str())
        };
        let own_palette = art
            .palette
            .as_deref()
            .filter(|_| art.own_palette)
            .and_then(|name| {
                let found = theme_route.palette(name).map(|(palette, _)| palette);
                if found.is_none() {
                    log::warn!(
                        "{} [{}] Palette={name}: no such palette; the side's own draws",
                        crate::ui::match_layout::FILE_NAME,
                        art.part.name()
                    );
                }
                found
            });
        let palette = (radar_part || art.palette.is_some())
            .then(|| own_palette.as_ref().unwrap_or(&theme_palette));
        let piece = |file: &str, own: bool| {
            render_piece(
                theme_route,
                side_route,
                file,
                art.frames,
                palette,
                &generic_palette,
                own,
            )
        };
        let own = art
            .own
            .then(|| piece(art.file.as_str(), true))
            .filter(|(frames, _)| {
                let found = frames.first().is_some_and(Option::is_some);
                if !found {
                    log::warn!(
                        "{} [{}] {}={}: no such picture; the built-in {built_in} draws",
                        crate::ui::match_layout::FILE_NAME,
                        art.part.name(),
                        art.key.name(),
                        art.file
                    );
                }
                found
            });
        let (file, (frames, shape)) = match own {
            Some(rendered) => (art.file.as_str(), rendered),
            None => (built_in, piece(built_in, false)),
        };
        excluded_extra_ids.insert(mix_hash(file));
        if radar_part && radar.is_none() {
            radar = shape.map(|(shape, source)| {
                (
                    shape,
                    source,
                    file.to_owned(),
                    palette.unwrap_or(&theme_palette).clone(),
                )
            });
        }
        // Without these the theme is skipped. Residual: with no side1 shape
        // gamemd loads SIDEGDI1..3.SHP in place of all three
        // (0x006ABD47..0x006ABD7E; a missing side2 or side3 alone has no
        // fallback). No retail archive holds them, so only a mod that ships
        // them but not side1.shp would see them; VERA skips the theme.
        let required = radar_part
            || is(SidebarPart::Middle, ArtKey::Art)
            || is(SidebarPart::Cameos, ArtKey::RowArt)
            || is(SidebarPart::Cameos, ArtKey::EndArt);
        if required && frames.first().is_none_or(Option::is_none) {
            return None;
        }
        for (frame, entry) in frames.iter().enumerate() {
            if entry.is_none() {
                log::warn!("{file} frame {frame} missing in the sidebar's archives");
            }
        }
        rendered_pieces.push((art.part, art.key, frames));
    }
    let (radar_shape, radar_source, radar_file, radar_palette) = radar?;
    let radar_identity = SidebarChromeAssetIdentity {
        logical_name: radar_file,
        source_archive: Some(radar_source.to_owned()),
    };
    // Pre-render all radar.shp frames for the opening/closing animation.
    let (radar_frames, radar_frame_size, radar_content_insets) =
        render_all_radar_frames(&radar_shape, &radar_palette);

    let (
        [background_large, background_medium, background_small],
        background_identities,
        background_palette,
    ) = in_game_shell::load_backgrounds(theme_route, names.backgrounds, names.background_palette)
        .map(|(entries, identities, palette)| (entries, Some(identities), Some(palette)))
        .unwrap_or(([None, None, None], None, None));
    let source_identity = SidebarChromeAtlasIdentity {
        atlas_theme: theme,
        parent_archive: mix_name.to_string(),
        radar: radar_identity,
        theme_palette: SidebarChromeAssetIdentity {
            logical_name: palette_name.to_string(),
            source_archive: Some(theme_palette_source_archive.to_owned()),
        },
        generic_palette: SidebarChromeAssetIdentity {
            logical_name: generic_palette_name.to_string(),
            source_archive: Some(generic_palette_source.to_owned()),
        },
        backgrounds: background_identities,
        background_palette,
    };
    // For sidebar inspection/fidelity work, decode every other SHP of the
    // theme's MIX with its main sidebar palette so unknown pieces are
    // comparable under one consistent color treatment.
    let extra_rendered = collect_extra_entries(mix, &theme_palette, &excluded_extra_ids);

    // GCLOCK2.SHP — production progress clock overlay (separate texture).
    let clock = built_in_name(SidebarPart::Cameos.name(), "Clock", theme);
    let (gclock_texture, gclock_frames) = match clock
        .and_then(|clock| build_gclock_texture(gpu, batch, side_route, clock, &generic_palette))
    {
        Some((tex, entries)) => (Some(tex), entries),
        None => {
            log::warn!(
                "{clock:?} not found in {:?} side route, progress overlay disabled",
                side_route.archive_names
            );
            (None, Vec::new())
        }
    };

    // Collect all pieces to pack into the atlas, one below the other.
    let backgrounds = [&background_large, &background_medium, &background_small];
    let mut all_entries: Vec<&RenderedChromeEntry> = Vec::new();
    all_entries.extend(command_bar_art.entries());
    all_entries.extend(in_game_shell_art.entries());
    all_entries.extend(layout_art_rendered.entries());
    all_entries.extend(observer_art_rendered.entries());
    all_entries.extend(backgrounds.into_iter().flatten());
    all_entries.extend(
        rendered_pieces
            .iter()
            .flat_map(|(_, _, frames)| frames.iter().flatten()),
    );
    all_entries.extend(extra_rendered.iter().map(|extra| &extra.rendered));

    let atlas_width = all_entries.iter().map(|e| e.width).max().unwrap_or(1);
    let atlas_height: u32 = all_entries
        .iter()
        .map(|e| e.height + CHROME_PADDING)
        .sum::<u32>();
    let mut rgba = vec![0u8; (atlas_width * atlas_height * 4) as usize];
    let mut y = 0u32;
    let mut pack = |entry: &RenderedChromeEntry| -> SidebarChromeEntry {
        let uv = blit_entry(&mut rgba, atlas_width, atlas_height, y, entry);
        y += entry.height + CHROME_PADDING;
        uv
    };
    let command_bar = command_bar_art.map(&mut pack);
    let in_game_shell = in_game_shell_art.map(&mut pack);
    let layout_art = layout_art_rendered.map(&mut pack);
    let observer_art = observer_art_rendered.map(&mut pack);
    let [background_large, background_medium, background_small] =
        backgrounds.map(|entry| entry.as_ref().map(&mut pack));
    let mut pieces: Pieces = Default::default();
    for (part, key, frames) in &rendered_pieces {
        let frames = frames.iter().map(|entry| entry.as_ref().map(&mut pack));
        pieces[*part as usize][*key as usize] = frames.collect();
    }
    let mut extra_entries = Vec::with_capacity(extra_rendered.len());
    for extra in &extra_rendered {
        extra_entries.push(SidebarChromeExtraEntry {
            id: extra.id,
            label: extra.label.clone(),
            frame_count: extra.frame_count,
            entry: pack(&extra.rendered),
        });
    }

    log::info!(
        "Sidebar chrome atlas for {}: {}x{} px, {} pieces",
        mix_name,
        atlas_width,
        atlas_height,
        all_entries.len()
    );

    let texture = batch.create_texture(gpu, &rgba, atlas_width, atlas_height);
    // VERA's smooth sidebar scales what the column and the command bar draw:
    // every piece but the pause menu's panel, buttons and backgrounds, and
    // the extras nothing draws.
    let sidebar_pieces: Vec<[u32; 4]> = command_bar
        .entries()
        .chain(layout_art.entries())
        .chain(observer_art.entries())
        .chain(
            pieces
                .iter()
                .flatten()
                .flat_map(|frames| frames.iter().flatten()),
        )
        .map(|entry| {
            [
                (entry.uv_origin[0] * atlas_width as f32).round() as u32,
                (entry.uv_origin[1] * atlas_height as f32).round() as u32,
                entry.pixel_size[0] as u32,
                entry.pixel_size[1] as u32,
            ]
        })
        .collect();
    let sidebar_pieces = SidebarPieces::gather(
        &rgba,
        [atlas_width, atlas_height],
        &sidebar_pieces,
        CHROME_PADDING,
        gpu.device.limits().max_texture_dimension_2d,
    );
    Some(SidebarChromeAtlas {
        source_identity,
        texture,
        command_bar,
        in_game_shell,
        layout_art,
        observer_art,
        background_large,
        background_medium,
        background_small,
        radar_frames,
        radar_frame_size,
        radar_content_insets,
        pieces,
        extra_entries,
        gclock_texture,
        gclock_frames,
        darken: SidebarDarkenMask::load(gpu, batch, asset_manager, &generic_palette, theme),
        sidebar_pieces,
        smooth: None,
    })
}

struct RenderedChromeEntry {
    rgba: Vec<u8>,
    indices: Vec<u8>,
    width: u32,
    height: u32,
}

struct ExtraRenderedEntry {
    id: i32,
    label: String,
    frame_count: usize,
    rendered: RenderedChromeEntry,
}

fn collect_extra_entries(
    mix: &MixArchive,
    palette: &Palette,
    excluded_ids: &std::collections::HashSet<i32>,
) -> Vec<ExtraRenderedEntry> {
    let mut entries = Vec::new();
    for mix_entry in mix.entries() {
        if excluded_ids.contains(&mix_entry.id) {
            continue;
        }
        let Some(shp_bytes) = mix.get_by_id(mix_entry.id) else {
            continue;
        };
        let Ok(shp) = ShpFile::from_bytes(shp_bytes) else {
            continue;
        };
        let Some(rendered) = render_shp(&shp, palette, 0) else {
            continue;
        };
        entries.push(ExtraRenderedEntry {
            id: mix_entry.id,
            label: format!(
                "id {:#010X} ({}x{}, {}f)",
                mix_entry.id as u32,
                shp.width,
                shp.height,
                shp.frames.len()
            ),
            frame_count: shp.frames.len(),
            rendered,
        });
    }
    entries
}

/// Pre-render all frames of radar.shp to RGBA buffers for animation.
/// Returns (Vec of RGBA buffers, [width, height], content insets).
/// Content insets come from the native fixed radar aperture.
fn render_all_radar_frames(shp: &ShpFile, palette: &Palette) -> (Vec<Vec<u8>>, [u32; 2], [u32; 4]) {
    let frame_count: usize = shp.frames.len();
    let canvas_w: u32 = shp.width as u32;
    let canvas_h: u32 = shp.height as u32;

    // Preserve each frame's skipped pixels. The animation surface owns backing
    // history; aperture geometry comes from Radar One_Time/Init_For_House.
    let content_insets = [16, 1, 12, 1];
    let frames: Vec<Vec<u8>> = (0..frame_count)
        .map(|i| {
            render_shp(shp, palette, i)
                .map(|entry| entry.rgba)
                .unwrap_or_else(|| vec![0u8; (canvas_w * canvas_h * 4) as usize])
        })
        .collect();
    log::info!(
        "Pre-rendered {} radar animation frames ({}x{}, content insets: l={} t={} r={} b={})",
        frames.len(),
        canvas_w,
        canvas_h,
        content_insets[0],
        content_insets[1],
        content_insets[2],
        content_insets[3],
    );
    (frames, [canvas_w, canvas_h], content_insets)
}

fn render_shp(shp: &ShpFile, palette: &Palette, frame_index: usize) -> Option<RenderedChromeEntry> {
    if frame_index >= shp.frames.len() {
        return None;
    }
    let frame = &shp.frames[frame_index];
    let frame_rgba = shp.frame_to_rgba(frame_index, palette).ok()?;
    let canvas_w = shp.width as u32;
    let canvas_h = shp.height as u32;
    let fw = frame.frame_width as u32;
    let fh = frame.frame_height as u32;
    let fx = frame.frame_x as u32;
    let fy = frame.frame_y as u32;
    if canvas_w == fw && canvas_h == fh && fx == 0 && fy == 0 {
        return Some(RenderedChromeEntry {
            rgba: frame_rgba,
            indices: frame.pixels.clone(),
            width: canvas_w,
            height: canvas_h,
        });
    }
    let mut canvas = vec![0u8; (canvas_w * canvas_h * 4) as usize];
    let mut indices = vec![0u8; (canvas_w * canvas_h) as usize];
    for row in 0..fh {
        let src_off = (row * fw * 4) as usize;
        let dst_off = (((fy + row) * canvas_w + fx) * 4) as usize;
        let len = (fw * 4) as usize;
        if src_off + len <= frame_rgba.len() && dst_off + len <= canvas.len() {
            canvas[dst_off..dst_off + len].copy_from_slice(&frame_rgba[src_off..src_off + len]);
            indices[dst_off / 4..(dst_off + len) / 4]
                .copy_from_slice(&frame.pixels[src_off / 4..(src_off + len) / 4]);
        }
    }
    Some(RenderedChromeEntry {
        rgba: canvas,
        indices,
        width: canvas_w,
        height: canvas_h,
    })
}

fn parse_sidebar_shape(assets: &AssetManager, bytes: &[u8]) -> Option<ShpFile> {
    if assets.mod_dir().is_some() {
        decode_ui_shp(bytes).ok()
    } else {
        ShpFile::from_bytes(bytes).ok()
    }
}

fn blit_entry(
    atlas_rgba: &mut [u8],
    atlas_width: u32,
    atlas_height: u32,
    dst_y: u32,
    entry: &RenderedChromeEntry,
) -> SidebarChromeEntry {
    for row in 0..entry.height {
        let src_start = (row * entry.width * 4) as usize;
        let dst_row = dst_y + row;
        if dst_row >= atlas_height {
            break;
        }
        let dst_start = ((dst_row * atlas_width) * 4) as usize;
        let len = (entry.width * 4) as usize;
        if src_start + len <= entry.rgba.len() && dst_start + len <= atlas_rgba.len() {
            atlas_rgba[dst_start..dst_start + len]
                .copy_from_slice(&entry.rgba[src_start..src_start + len]);
        }
    }

    SidebarChromeEntry {
        uv_origin: [0.0, dst_y as f32 / atlas_height as f32],
        uv_size: [
            entry.width as f32 / atlas_width as f32,
            entry.height as f32 / atlas_height as f32,
        ],
        pixel_size: [entry.width as f32, entry.height as f32],
    }
}

#[cfg(all(test, tests_render_s_z))]
#[path = "sidebar_chrome_tests.rs"]
mod tests;

#[cfg(all(test, tests_render_s_z))]
mod provenance_tests {
    use super::{SidebarTheme, next_theme_to_build, select_sidebar_theme};

    #[test]
    fn ares_sidebar_filename_switch_matches_native_independent_side_controls() {
        let corpus = crate::test_fixture::json("tools/rules_oracle/ares_side_sidebar.json");
        for row in corpus["selector"]["rows"].as_array().unwrap() {
            let flag = row["input"]["yuri_file_names"].as_bool().unwrap();
            let names = super::SidebarArtNames::for_yuri(flag);
            let expected = &corpus["filename_sets"][if flag { "yuri" } else { "plain" }];
            // The switch, not the side's pictures, picks the radar's files.
            for theme in [SidebarTheme::Soviet, SidebarTheme::Yuri] {
                let (radar, radar_palette) = names.radar(theme);
                assert_eq!(radar.to_ascii_uppercase(), expected["radar"]);
                assert_eq!(
                    radar_palette.to_ascii_uppercase(),
                    expected["radar_palette"]
                );
            }
            assert_eq!(
                names.background_palette.to_ascii_uppercase(),
                expected["background_palette"]
            );
            for (actual, expected) in [
                names.backgrounds.0,
                names.backgrounds.1,
                names.backgrounds.2,
            ]
            .iter()
            .zip(expected["backgrounds"].as_array().unwrap())
            {
                assert_eq!(actual.to_ascii_uppercase(), expected.as_str().unwrap());
            }
        }
    }

    /// A stock set builds the side's own theme first, then the fallbacks the
    /// archives hold, one at a time, and nothing once the drawn one exists.
    #[test]
    fn a_side_builds_its_theme_then_the_fallbacks_it_lacks() {
        use SidebarTheme::{Allied, Soviet, Yuri};
        let built = |themes: &'static [SidebarTheme]| move |theme| themes.contains(&theme);
        assert_eq!(next_theme_to_build(Yuri, built(&[]), &[]), Some(Yuri));
        assert_eq!(next_theme_to_build(Yuri, built(&[Allied]), &[]), Some(Yuri));
        assert_eq!(next_theme_to_build(Yuri, built(&[]), &[Yuri]), Some(Soviet));
        assert_eq!(next_theme_to_build(Yuri, built(&[Soviet]), &[Yuri]), None);
        assert_eq!(
            next_theme_to_build(Allied, built(&[Soviet, Yuri]), &[]),
            Some(Allied)
        );
        assert_eq!(
            next_theme_to_build(Soviet, built(&[]), &[Soviet, Allied]),
            None
        );
    }

    #[test]
    fn atlas_resolution_reports_actual_fallback_without_changing_order() {
        let allied_source = "sidec01.mix";
        let soviet_source = "sidec02.mix";
        let yuri_source = "sidec02md.mix";

        let (actual, source) = select_sidebar_theme(
            SidebarTheme::Yuri,
            Some(&allied_source),
            Some(&soviet_source),
            Some(&yuri_source),
        )
        .expect("Yuri atlas should resolve");
        assert_eq!(actual, SidebarTheme::Yuri);
        assert_eq!(*source, yuri_source);

        let (actual, source) = select_sidebar_theme(
            SidebarTheme::Yuri,
            Some(&allied_source),
            Some(&soviet_source),
            None,
        )
        .expect("Yuri should fall back to Soviet before Allied");
        assert_eq!(actual, SidebarTheme::Soviet);
        assert_eq!(*source, soviet_source);

        let (actual, source) =
            select_sidebar_theme(SidebarTheme::Soviet, Some(&allied_source), None, None)
                .expect("Soviet should fall back to Allied");
        assert_eq!(actual, SidebarTheme::Allied);
        assert_eq!(*source, allied_source);

        assert!(
            select_sidebar_theme(
                SidebarTheme::Allied,
                None,
                Some(&soviet_source),
                Some(&yuri_source),
            )
            .is_none(),
            "Allied must not reverse-fallback to another faction",
        );
    }
}
