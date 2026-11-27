//! ANSI file identities at retail Win32 boundaries.
//! Keep raw bytes through truncation and CreateFileA rather than round-tripping
//! a potentially split DBCS character through Rust UTF-8.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFileName(Vec<u8>);

impl NativeFileName {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(
            bytes
                .iter()
                .copied()
                .take_while(|byte| *byte != 0)
                .collect(),
        )
    }
    pub fn truncated(&self, limit: usize) -> Self {
        Self(self.0.iter().copied().take(limit).collect())
    }
    #[cfg(test)]
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }
    pub fn display_text(&self) -> String {
        super::native_string::acp_decode(&self.0)
    }
    pub fn is_ascii_name(&self, name: &[u8]) -> bool {
        self.0.eq_ignore_ascii_case(name)
    }
    #[cfg(windows)]
    fn full_path(&self, directory: &Path) -> std::io::Result<std::ffi::CString> {
        let mut bytes = super::native_string::acp_encode(&directory.to_string_lossy());
        if !bytes.ends_with(b"\\") && !bytes.ends_with(b"/") {
            bytes.push(b'\\');
        }
        bytes.extend_from_slice(&self.0);
        std::ffi::CString::new(bytes)
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "NUL in filename"))
    }
}
impl From<&str> for NativeFileName {
    fn from(text: &str) -> Self {
        Self::from_bytes(&super::native_string::acp_encode(text))
    }
}
impl From<String> for NativeFileName {
    fn from(text: String) -> Self {
        Self::from(text.as_str())
    }
}
impl std::fmt::Display for NativeFileName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display_text())
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileA(
        name: *const i8,
        access: u32,
        sharing: u32,
        security: *const std::ffi::c_void,
        creation: u32,
        attributes: u32,
        template: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn MoveFileExA(from: *const i8, to: *const i8, flags: u32) -> i32;
    fn DeleteFileA(name: *const i8) -> i32;
    fn FindFirstFileA(pattern: *const i8, data: *mut FindData) -> *mut std::ffi::c_void;
    fn FindNextFileA(handle: *mut std::ffi::c_void, data: *mut FindData) -> i32;
    fn FindClose(handle: *mut std::ffi::c_void) -> i32;
}

/// Open exactly the ANSI name, retaining truncated native action bytes.
#[cfg(windows)]
fn open_read(directory: &Path, name: &NativeFileName) -> std::io::Result<std::fs::File> {
    use std::os::windows::io::FromRawHandle;
    let path = name.full_path(directory)?;
    // SAFETY: path is NUL-terminated; optional pointers are null. Successful
    // ownership transfers once to File, whose Drop closes the returned handle.
    let handle = unsafe {
        CreateFileA(
            path.as_ptr(),
            0x80000000,
            3,
            std::ptr::null(),
            3,
            0x08000080,
            std::ptr::null_mut(),
        )
    };
    if handle as isize == -1 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: a valid handle CreateFileA just returned, owned by nothing else.
    Ok(unsafe { std::fs::File::from_raw_handle(handle) })
}

/// RawFile availability uses a read-only share probe, distinct from actual read.
#[cfg(windows)]
pub fn available(directory: &Path, name: &NativeFileName) -> bool {
    use std::os::windows::io::FromRawHandle;
    let Ok(path) = name.full_path(directory) else {
        return false;
    };
    // SAFETY: valid NUL-terminated path, null optional arguments; File owns and
    // closes a successful probe. Native RawFile 0x0065CBF0 uses these flags.
    let handle = unsafe {
        CreateFileA(
            path.as_ptr(),
            0x80000000,
            1,
            std::ptr::null(),
            3,
            0x80,
            std::ptr::null_mut(),
        )
    };
    if handle as isize == -1 {
        return false;
    }
    // SAFETY: a valid handle CreateFileA just returned, owned by nothing else.
    drop(unsafe { std::fs::File::from_raw_handle(handle) });
    true
}
#[cfg(not(windows))]
pub fn available(directory: &Path, name: &NativeFileName) -> bool {
    super::file_system::exists(directory.join(name.display_text()))
}

pub fn read_limited(
    directory: &Path,
    name: &NativeFileName,
    limit: usize,
) -> std::io::Result<Vec<u8>> {
    let too_large =
        || std::io::Error::new(std::io::ErrorKind::InvalidData, "file exceeds size limit");
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    {
        let bytes = super::file_system::read_shared(directory.join(name.display_text()))?;
        return if bytes.len() <= limit {
            Ok(bytes.to_vec())
        } else {
            Err(too_large())
        };
    }
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    {
        use std::io::Read;
        #[cfg(windows)]
        let file = open_read(directory, name)?;
        #[cfg(not(windows))]
        let file = std::fs::File::open(directory.join(name.display_text()))?;
        if file.metadata()?.len() > limit as u64 {
            return Err(too_large());
        }
        let mut bytes = Vec::new();
        file.take((limit as u64).saturating_add(1))
            .read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(too_large());
        }
        Ok(bytes)
    }
}

#[cfg(windows)]
pub fn delete(directory: &Path, name: &NativeFileName) -> std::io::Result<()> {
    let path = name.full_path(directory)?;
    // SAFETY: path remains a live NUL-terminated ANSI string for the call.
    if unsafe { DeleteFileA(path.as_ptr()) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
pub fn delete(directory: &Path, name: &NativeFileName) -> std::io::Result<()> {
    super::file_system::remove_file(directory.join(name.display_text()))
}

/// Publish the entire file at once. A new slot cannot overwrite another
/// writer's file if both games happened to allocate the same name.
pub fn write(
    directory: &Path,
    name: &NativeFileName,
    bytes: &[u8],
    overwrite: bool,
) -> std::io::Result<()> {
    #[cfg(not(windows))]
    {
        let path = directory.join(name.display_text());
        if !overwrite {
            return super::file_system::write_new(path, bytes);
        }
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        return super::file_system::write(path, bytes);
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        super::file_system::write_staged(directory, bytes, |stage| std::fs::rename(stage, path))
    }
    #[cfg(windows)]
    super::file_system::write_staged(directory, bytes, |stage| {
        let staged =
            std::ffi::CString::new(super::native_string::acp_encode(&stage.to_string_lossy()))?;
        let target = name.full_path(directory)?;
        // SAFETY: both paths are NUL terminated for the duration of the call.
        // MOVEFILE_REPLACE_EXISTING (1) is only set on confirmed overwrite.
        if unsafe { MoveFileExA(staged.as_ptr(), target.as_ptr(), u32::from(overwrite)) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    })
}

#[cfg(windows)]
#[repr(C)]
struct FindData {
    attributes: u32,
    creation: [u32; 2],
    access: [u32; 2],
    write: [u32; 2],
    size_high: u32,
    size_low: u32,
    reserved: [u32; 2],
    name: [u8; 260],
    alternate: [u8; 14],
}

/// Native FindFirstFileA enumeration order, names and FILETIME values.
/// API contract: https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-findfirstfilea
#[cfg(windows)]
pub fn enumerate(directory: &Path, pattern: &str) -> Vec<(NativeFileName, u32, u64)> {
    let Ok(pattern) = NativeFileName::from(pattern).full_path(directory) else {
        return Vec::new();
    };
    // SAFETY: FindData is the C-layout WIN32_FIND_DATAA with initialized storage.
    let mut data: FindData = unsafe { std::mem::zeroed() };
    // SAFETY: `pattern` is NUL-terminated and `data` is a writable record.
    let handle = unsafe { FindFirstFileA(pattern.as_ptr(), &mut data) };
    if handle as isize == -1 {
        return Vec::new();
    }
    struct FindHandle(*mut std::ffi::c_void);
    impl Drop for FindHandle {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful FindFirstFileA and
            // is closed once, here.
            unsafe {
                FindClose(self.0);
            }
        }
    }
    let handle = FindHandle(handle);
    let mut entries = Vec::new();
    loop {
        entries.push((
            NativeFileName::from_bytes(&data.name),
            data.attributes,
            u64::from(data.write[0]) | (u64::from(data.write[1]) << 32),
        ));
        // SAFETY: handle is live and data is a writable WIN32_FIND_DATAA.
        if unsafe { FindNextFileA(handle.0, &mut data) } == 0 {
            break;
        }
    }
    entries
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::*;
    #[test]
    fn action_limit_preserves_bytes_even_at_a_split_multibyte_character() {
        let mut bytes = vec![b'a'; 31];
        bytes.extend([0x81, 0x40, b'.', b'S', b'E', b'D']);
        let full = NativeFileName::from_bytes(&bytes);
        assert_eq!(full.truncated(32).bytes(), &bytes[..32]);
        assert_eq!(full.bytes(), bytes);
    }
}
