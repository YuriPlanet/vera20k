//! The window, taskbar and Dock icon: Red Alert 2's red star, from the retail install.
//!
//! Not gamemd-derived. gamemd registers its own Yuri's Revenge icon for its window;
//! the user chose the original Red Alert 2 game icon instead: the red star with the
//! hammer and sickle built into RA2's `game.exe` (and `Ra2.exe`). No `.ico` file ships
//! with that design, so the icon group is read out of the executable in the configured
//! install at startup and no retail art is committed. Without it the platform default
//! icon stays.

use std::collections::HashMap;
use std::path::Path;

use winit::window::{Icon, WindowAttributes};

/// Retail executables carrying the red star, in order of preference.
const ICON_SOURCES: [&str; 2] = ["GAME.EXE", "RA2.EXE"];

/// Red Alert 2's red star, loaded from the retail install.
pub(super) struct RetailIcon {
    /// The icon group as an `.ico` file. macOS takes every entry and picks its own
    /// resolution.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    ico: Vec<u8>,
    /// The largest entry, for winit's window icon (Windows and X11).
    rgba: image::RgbaImage,
}

impl RetailIcon {
    pub(super) fn load(ra2_dir: &Path) -> Option<Self> {
        for name in ICON_SOURCES {
            let Some(path) = crate::util::case_insensitive_path::find(ra2_dir, name) else {
                continue;
            };
            let Some(ico) = std::fs::read(&path)
                .ok()
                .and_then(|exe| executable_icon(&exe))
            else {
                log::warn!("No icon found in {}", path.display());
                continue;
            };
            match Self::decode(ico) {
                Ok(icon) => return Some(icon),
                Err(err) => log::warn!("Icon in {} unreadable: {err}", path.display()),
            }
        }
        None
    }

    fn decode(ico: Vec<u8>) -> image::ImageResult<Self> {
        // The ICO decoder selects the file's largest entry.
        let rgba = image::load_from_memory_with_format(&ico, image::ImageFormat::Ico)?.into_rgba8();
        Ok(Self { ico, rgba })
    }

    /// The window icon, and on Windows the taskbar icon. winit ignores both on
    /// macOS; see [`Self::set_dock_icon`].
    pub(super) fn apply(&self, attributes: WindowAttributes) -> WindowAttributes {
        let icon = match Icon::from_rgba(
            self.rgba.as_raw().clone(),
            self.rgba.width(),
            self.rgba.height(),
        ) {
            Ok(icon) => icon,
            Err(err) => {
                log::warn!("Window icon rejected: {err}");
                return attributes;
            }
        };
        #[cfg(windows)]
        let attributes = {
            use winit::platform::windows::WindowAttributesExtWindows;
            attributes.with_taskbar_icon(Some(icon.clone()))
        };
        attributes.with_window_icon(Some(icon))
    }

    /// macOS has no per-window icon: the Dock and app switcher show the
    /// application's icon image. Call on the main thread once the window exists.
    #[cfg(target_os = "macos")]
    pub(super) fn set_dock_icon(&self) {
        use objc2::{AllocAnyThread, MainThreadMarker};
        use objc2_app_kit::{NSApplication, NSImage};
        use objc2_foundation::NSData;

        let Some(mtm) = MainThreadMarker::new() else {
            log::warn!("Dock icon not set: not on the main thread");
            return;
        };
        let data = NSData::with_bytes(&self.ico);
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
            log::warn!("Dock icon not set: AppKit could not read the icon");
            return;
        };
        // SAFETY: called on the main thread (checked above) with a valid image;
        // the setter's only documented hazard is passing `None`.
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}

const RT_ICON: u32 = 3;
const RT_GROUP_ICON: u32 = 14;

/// Rebuilds an `.ico` file from a Windows executable's application icon: the icon
/// group holding the biggest image. Retail `game.exe` also carries three 16×16 extras.
///
/// Icon groups (GRPICONDIR) list their images by RT_ICON id; an `.ico` file lists
/// the same 12-byte headers followed by file offsets. Formats:
/// <https://learn.microsoft.com/en-us/windows/win32/menurc/resource-file-formats>,
/// <https://learn.microsoft.com/en-us/previous-versions/ms997538(v=msdn.10)>.
fn executable_icon(exe: &[u8]) -> Option<Vec<u8>> {
    let resources = Resources::parse(exe)?;
    let images: HashMap<u32, &[u8]> = resources.of_type(RT_ICON).into_iter().collect();
    let entries = |group: &[u8]| -> Vec<([u8; 8], &[u8])> {
        let count = usize::from(u16_at(group, 4).unwrap_or(0));
        (0..count)
            .filter_map(|i| {
                let entry = group.get(6 + 14 * i..6 + 14 * (i + 1))?;
                let id = u16::from_le_bytes([entry[12], entry[13]]);
                // Width, height, colors, reserved, planes, bit count.
                Some((entry[..8].try_into().ok()?, *images.get(&u32::from(id))?))
            })
            .collect()
    };
    // A zero width byte means 256 pixels.
    let largest = |group: &[([u8; 8], &[u8])]| {
        group
            .iter()
            .map(|(header, _)| {
                if header[0] == 0 {
                    256
                } else {
                    u32::from(header[0])
                }
            })
            .max()
            .unwrap_or(0)
    };
    let mut best: Option<Vec<([u8; 8], &[u8])>> = None;
    for (_, group) in resources.of_type(RT_GROUP_ICON) {
        let group = entries(group);
        if best
            .as_deref()
            .is_none_or(|best| largest(&group) > largest(best))
        {
            best = Some(group);
        }
    }
    let group = best.filter(|group| !group.is_empty())?;

    let mut ico = vec![0, 0, 1, 0];
    ico.extend_from_slice(&u16::try_from(group.len()).ok()?.to_le_bytes());
    let mut offset = 6 + 16 * group.len();
    for (header, image) in &group {
        ico.extend_from_slice(header);
        ico.extend_from_slice(&u32::try_from(image.len()).ok()?.to_le_bytes());
        ico.extend_from_slice(&u32::try_from(offset).ok()?.to_le_bytes());
        offset += image.len();
    }
    for (_, image) in &group {
        ico.extend_from_slice(image);
    }
    Some(ico)
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// The resource section of a PE image, read as far as icons need.
/// Layout: <https://learn.microsoft.com/en-us/windows/win32/debug/pe-format#the-rsrc-section>.
struct Resources<'a> {
    exe: &'a [u8],
    /// `(virtual address, size, file offset)` of each section.
    sections: Vec<(u32, u32, u32)>,
    /// File offset of the root resource directory.
    root: usize,
}

impl<'a> Resources<'a> {
    fn parse(exe: &'a [u8]) -> Option<Self> {
        if exe.get(..2)? != b"MZ" {
            return None;
        }
        let pe = usize::try_from(u32_at(exe, 0x3C)?).ok()?;
        if exe.get(pe..pe + 4)? != b"PE\0\0" {
            return None;
        }
        let section_count = usize::from(u16_at(exe, pe + 6)?);
        let optional = pe + 24;
        let optional_size = usize::from(u16_at(exe, pe + 20)?);
        let data_directories = optional
            + match u16_at(exe, optional)? {
                0x10B => 96,  // PE32
                0x20B => 112, // PE32+
                _ => return None,
            };
        // Data directory 2 is the resource table.
        let resource_rva = u32_at(exe, data_directories + 2 * 8)?;
        let sections = (0..section_count)
            .map(|i| {
                let header = optional + optional_size + 40 * i;
                let virtual_size = u32_at(exe, header + 8)?;
                let virtual_address = u32_at(exe, header + 12)?;
                let raw_size = u32_at(exe, header + 16)?;
                let raw_offset = u32_at(exe, header + 20)?;
                Some((virtual_address, virtual_size.max(raw_size), raw_offset))
            })
            .collect::<Option<Vec<_>>>()?;
        let mut resources = Self {
            exe,
            sections,
            root: 0,
        };
        resources.root = resources.file_offset(resource_rva)?;
        Some(resources)
    }

    fn file_offset(&self, rva: u32) -> Option<usize> {
        let (start, _, raw) = self
            .sections
            .iter()
            .find(|(start, size, _)| rva >= *start && rva - start < *size)?;
        usize::try_from(raw.checked_add(rva - start)?).ok()
    }

    /// `(id, target)` of each entry in the directory at `offset` from the root.
    fn directory(&self, offset: u32) -> Option<Vec<(u32, u32)>> {
        let at = self.root.checked_add(usize::try_from(offset).ok()?)?;
        let count =
            usize::from(u16_at(self.exe, at + 12)?) + usize::from(u16_at(self.exe, at + 14)?);
        (0..count)
            .map(|i| {
                Some((
                    u32_at(self.exe, at + 16 + 8 * i)?,
                    u32_at(self.exe, at + 20 + 8 * i)?,
                ))
            })
            .collect()
    }

    /// `(id, data)` of every resource of type `kind`, in its first listed language.
    fn of_type(&self, kind: u32) -> Vec<(u32, &'a [u8])> {
        const SUBDIRECTORY: u32 = 0x8000_0000;
        let names = self.directory(0).and_then(|types| {
            let (_, target) = types
                .into_iter()
                .find(|(id, target)| *id == kind && target & SUBDIRECTORY != 0)?;
            self.directory(target & !SUBDIRECTORY)
        });
        names
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(id, mut target)| {
                // Name level, then language level; a well-formed tree ends at a data entry.
                for _ in 0..2 {
                    if target & SUBDIRECTORY == 0 {
                        break;
                    }
                    target = self.directory(target & !SUBDIRECTORY)?.first()?.1;
                }
                if target & SUBDIRECTORY != 0 {
                    return None;
                }
                let entry = self.root.checked_add(usize::try_from(target).ok()?)?;
                let start = self.file_offset(u32_at(self.exe, entry)?)?;
                let size = usize::try_from(u32_at(self.exe, entry + 4)?).ok()?;
                Some((id, self.exe.get(start..start.checked_add(size)?)?))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;
    use image::codecs::png::PngEncoder;

    fn png(side: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        let pixels = vec![255u8; (side * side * 4) as usize];
        PngEncoder::new(&mut bytes)
            .write_image(&pixels, side, side, image::ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }

    /// A minimal PE32 image whose resources hold `icons` (RT_ICON id, side) and
    /// `groups` (RT_GROUP_ICON id, member icon ids). Name entries point straight
    /// at data entries, without a language level.
    fn pe_with_icons(icons: &[(u16, u32)], groups: &[(u16, &[u16])]) -> Vec<u8> {
        let side_of = |id: u16| icons.iter().find(|(i, _)| *i == id).unwrap().1;
        let mut blobs: Vec<(u32, u32, Vec<u8>)> = Vec::new(); // (type, id, data)
        for &(id, side) in icons {
            blobs.push((RT_ICON, u32::from(id), png(side)));
        }
        for &(id, members) in groups {
            let mut group = vec![0, 0, 1, 0];
            group.extend_from_slice(&(members.len() as u16).to_le_bytes());
            for &member in members {
                let side = side_of(member);
                let size = png(side).len() as u32;
                group.extend_from_slice(&[(side % 256) as u8, (side % 256) as u8, 0, 0]);
                group.extend_from_slice(&1u16.to_le_bytes());
                group.extend_from_slice(&32u16.to_le_bytes());
                group.extend_from_slice(&size.to_le_bytes());
                group.extend_from_slice(&member.to_le_bytes());
            }
            blobs.push((RT_GROUP_ICON, u32::from(id), group));
        }

        // Resource section at RVA 0x1000, file offset 0x200: root, one directory per
        // type, data entries, then the data.
        const RVA: u32 = 0x1000;
        let types = [RT_ICON, RT_GROUP_ICON];
        let dir_len = |n: usize| 16 + 8 * n;
        let count = |kind: u32| blobs.iter().filter(|b| b.0 == kind).count();
        let mut type_dirs = Vec::new();
        let mut at = dir_len(types.len());
        for kind in types {
            type_dirs.push(at);
            at += dir_len(count(kind));
        }
        let entries_at = at;
        let data_at = entries_at + 16 * blobs.len();
        let mut rsrc = vec![0u8; data_at];
        let directory = |rsrc: &mut Vec<u8>, at: usize, entries: &[(u32, u32)]| {
            rsrc[at + 14..at + 16].copy_from_slice(&(entries.len() as u16).to_le_bytes());
            for (i, (id, target)) in entries.iter().enumerate() {
                let e = at + 16 + 8 * i;
                rsrc[e..e + 4].copy_from_slice(&id.to_le_bytes());
                rsrc[e + 4..e + 8].copy_from_slice(&target.to_le_bytes());
            }
        };
        let root: Vec<(u32, u32)> = types
            .iter()
            .zip(&type_dirs)
            .map(|(kind, at)| (*kind, 0x8000_0000 | *at as u32))
            .collect();
        directory(&mut rsrc, 0, &root);
        for (kind, dir_at) in types.iter().zip(&type_dirs) {
            let named: Vec<(u32, u32)> = blobs
                .iter()
                .enumerate()
                .filter(|(_, b)| b.0 == *kind)
                .map(|(i, b)| (b.1, (entries_at + 16 * i) as u32))
                .collect();
            directory(&mut rsrc, *dir_at, &named);
        }
        for (i, (_, _, data)) in blobs.iter().enumerate() {
            let entry = entries_at + 16 * i;
            let data_rva = RVA + rsrc.len() as u32;
            rsrc[entry..entry + 4].copy_from_slice(&data_rva.to_le_bytes());
            rsrc[entry + 4..entry + 8].copy_from_slice(&(data.len() as u32).to_le_bytes());
            rsrc.extend_from_slice(data);
        }

        let mut exe = vec![0u8; 0x200];
        exe[..2].copy_from_slice(b"MZ");
        exe[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        exe[0x40..0x44].copy_from_slice(b"PE\0\0");
        exe[0x46..0x48].copy_from_slice(&1u16.to_le_bytes()); // one section
        exe[0x54..0x56].copy_from_slice(&224u16.to_le_bytes()); // PE32 optional header
        exe[0x58..0x5A].copy_from_slice(&0x10Bu16.to_le_bytes());
        let resource_directory = 0x58 + 96 + 2 * 8;
        exe[resource_directory..resource_directory + 4].copy_from_slice(&RVA.to_le_bytes());
        let section = 0x58 + 224;
        let len = rsrc.len() as u32;
        exe[section + 8..section + 12].copy_from_slice(&len.to_le_bytes());
        exe[section + 12..section + 16].copy_from_slice(&RVA.to_le_bytes());
        exe[section + 16..section + 20].copy_from_slice(&len.to_le_bytes());
        exe[section + 20..section + 24].copy_from_slice(&0x200u32.to_le_bytes());
        exe.extend_from_slice(&rsrc);
        exe
    }

    #[test]
    fn executable_icon_takes_the_group_with_the_biggest_image() {
        // Like retail game.exe: a small extra group beside the application icon.
        let exe = pe_with_icons(
            &[(1, 16), (2, 48), (3, 16), (4, 16)],
            &[(93, &[3, 2]), (160, &[4]), (7, &[1])],
        );
        let ico = executable_icon(&exe).unwrap();
        assert_eq!(u16_at(&ico, 4), Some(2));
        let icon = RetailIcon::decode(ico).unwrap();
        assert_eq!(icon.rgba.dimensions(), (48, 48));
    }

    #[test]
    fn executable_icon_rejects_non_executables() {
        assert!(executable_icon(b"not an executable").is_none());
        assert!(executable_icon(&pe_with_icons(&[], &[])).is_none());
    }

    #[test]
    fn load_finds_the_executable_whatever_its_case() {
        let dir = std::env::temp_dir().join(format!("vera20k-window-icon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Game.exe"),
            pe_with_icons(&[(1, 32)], &[(93, &[1])]),
        )
        .unwrap();

        let icon = RetailIcon::load(&dir);

        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(icon.map(|icon| icon.rgba.dimensions()), Some((32, 32)));
    }

    #[test]
    #[ignore = "requires RA2_DIR with the retail Red Alert 2 install"]
    fn retail_game_exe_yields_the_red_star() {
        let dir = std::env::var("RA2_DIR").expect("set RA2_DIR to the retail RA2 install");
        let icon = RetailIcon::load(Path::new(&dir)).expect("game.exe carries an icon");
        // Sizes differ between installs (the development install's game.exe holds a
        // 512×512 PNG in a 256-pixel slot), so check only for a sane square icon.
        let (width, height) = icon.rgba.dimensions();
        assert_eq!(width, height);
        assert!(width >= 32, "icon {width}×{height}");
    }
}
