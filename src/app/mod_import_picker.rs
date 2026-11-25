//! The desktop's chooser for Add Mod. The Mods page owns the selection and
//! passes it to the same importer as files dropped on the window. Android
//! picks through the activity instead (`App::browse_mod_parts`).

#[cfg(not(target_os = "android"))]
use std::{io, path::PathBuf};

#[cfg(not(target_os = "android"))]
use winit::window::Window;

/// Android: the system file manager, through the activity
/// (`util::mod_actions`); its copy reaches Add Mod later.
#[cfg(target_os = "android")]
pub(super) fn available() -> bool {
    crate::util::mod_actions::available()
}

#[cfg(any(windows, target_os = "macos"))]
pub(super) const fn available() -> bool {
    true
}

#[cfg(target_os = "linux")]
pub(super) fn available() -> bool {
    super::system_dialog::DialogTool::installed().is_some()
}

#[cfg(target_os = "macos")]
pub(super) fn pick(_window: &Window, title: &str) -> io::Result<Vec<PathBuf>> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{NSArray, NSString};

    let main = MainThreadMarker::new()
        .ok_or_else(|| io::Error::other("the file chooser needs the main thread"))?;
    let panel = NSOpenPanel::openPanel(main);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(true);
    panel.setAllowsMultipleSelection(true);
    panel.setResolvesAliases(false);
    panel.setTitle(Some(&NSString::from_str(title)));
    let message = crate::i18n::tr("VERA:ModImportMacMultiSelect");
    panel.setMessage(Some(&NSString::from_str(&message)));
    let types = NSArray::from_retained_slice(&[
        NSString::from_str("zip"),
        NSString::from_str("public.folder"),
    ]);
    // AppKit accepts filename extensions and UTIs here with the existing
    // Foundation bindings. Keep ordinary folders available alongside ZIPs.
    #[expect(
        deprecated,
        reason = "takes extensions; its UTType replacement is not bound here"
    )]
    panel.setAllowedFileTypes(Some(&types));
    if panel.runModal() != NSModalResponseOK {
        return Ok(Vec::new());
    }
    panel
        .URLs()
        .to_vec()
        .into_iter()
        .map(|url| {
            url.to_file_path()
                .ok_or_else(|| io::Error::other("the chooser returned an item without a file path"))
        })
        .collect()
}

#[cfg(any(windows, target_os = "linux"))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Selection {
    Zips,
    Folders,
}

#[cfg(any(windows, target_os = "linux"))]
pub(super) fn pick(window: &Window, title: &str) -> io::Result<Vec<PathBuf>> {
    pick_kind(window, title, Selection::Zips)
}

#[cfg(any(windows, target_os = "linux"))]
pub(super) fn pick_folders(window: &Window, title: &str) -> io::Result<Vec<PathBuf>> {
    pick_kind(window, title, Selection::Folders)
}

#[cfg(windows)]
fn pick_kind(window: &Window, title: &str, selection: Selection) -> io::Result<Vec<PathBuf>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::{E_UNEXPECTED, ERROR_CANCELLED, HWND};
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FOS_ALLOWMULTISELECT, FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PICKFOLDERS,
        FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };
    use windows::core::{HRESULT, HSTRING, w};

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: the guard is created after a successful CoInitializeEx
            // and stays on the UI thread, until its dialog has been dropped.
            unsafe { CoUninitialize() };
        }
    }

    let handle = window.window_handle().map_err(io::Error::other)?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return Err(io::Error::other("the chooser needs a Win32 window"));
    };
    let owner = HWND(handle.hwnd.get() as *mut std::ffi::c_void);

    // The modern dialog preserves the game's working directory.
    // https://learn.microsoft.com/windows/win32/shell/common-file-dialog
    let choose = || -> windows::core::Result<Vec<PathBuf>> {
        // SAFETY: the apartment and its interfaces stay on the calling
        // thread, the parent window remains live, and the Shell returns
        // null-terminated, COM-allocated strings that we free after copying.
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            let _apartment = Apartment;
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            let mut options =
                dialog.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_ALLOWMULTISELECT | FOS_NOCHANGEDIR;
            if selection == Selection::Folders {
                options |= FOS_PICKFOLDERS;
            } else {
                dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                    pszName: w!("ZIP"),
                    pszSpec: w!("*.zip"),
                }])?;
            }
            dialog.SetOptions(options)?;
            dialog.SetTitle(&HSTRING::from(title))?;
            match dialog.Show(Some(owner)) {
                Err(error) if error.code() == HRESULT::from_win32(ERROR_CANCELLED.0) => {
                    return Ok(Vec::new());
                }
                shown => shown?,
            }
            let results = dialog.GetResults()?;
            (0..results.GetCount()?)
                .map(|index| {
                    let path = results
                        .GetItemAt(index)?
                        .GetDisplayName(SIGDN_FILESYSPATH)?;
                    if path.is_null() {
                        return Err(windows::core::Error::from_hresult(E_UNEXPECTED));
                    }
                    let copied = OsString::from_wide(path.as_wide());
                    CoTaskMemFree(Some(path.as_ptr().cast()));
                    Ok(PathBuf::from(copied))
                })
                .collect()
        }
    };
    choose().map_err(io::Error::other)
}

#[cfg(target_os = "linux")]
fn pick_kind(_window: &Window, title: &str, selection: Selection) -> io::Result<Vec<PathBuf>> {
    use super::system_dialog::DialogTool;
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::process::Command;

    let tool = DialogTool::installed().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no desktop file chooser is installed",
        )
    })?;
    let (program, mut command, separator) = match tool {
        DialogTool::Zenity => {
            let program = "zenity";
            let mut command = Command::new(program);
            command
                .args(["--file-selection", "--multiple", "--separator=\x1f"])
                .arg(format!("--title={title}"));
            match selection {
                Selection::Zips => command.arg("--file-filter=ZIP | *.[zZ][iI][pP]"),
                Selection::Folders => command.arg("--directory"),
            };
            (program, command, Some(b'\x1f'))
        }
        DialogTool::Kdialog => {
            let program = "kdialog";
            let mut command = Command::new(program);
            command.args(["--title", title]);
            let separator = match selection {
                Selection::Zips => {
                    command.args([
                        "--multiple",
                        "--separate-output",
                        "--getopenfilename",
                        ".",
                        "*.[zZ][iI][pP]",
                    ]);
                    Some(b'\n')
                }
                // KDE's folder picker returns one path at a time.
                Selection::Folders => {
                    command.args(["--getexistingdirectory", "."]);
                    None
                }
            };
            (program, command, separator)
        }
    };
    let output = command.output()?;
    match output.status.code() {
        Some(0) => {}
        Some(1) => return Ok(Vec::new()),
        _ => return Err(io::Error::other(format!("{program}: {}", output.status))),
    }
    let names = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
    names
        .split(|byte| separator == Some(*byte))
        .map(|name| {
            let path = PathBuf::from(OsStr::from_bytes(name));
            if !path.is_absolute() || name.contains(&0) {
                return Err(io::Error::other(
                    "the chooser returned an invalid file path",
                ));
            }
            Ok(path)
        })
        .collect()
}
