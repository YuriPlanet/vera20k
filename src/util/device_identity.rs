//! The default LAN handle on Android.
//!
//! The stock default, `[New Player]`, is the same on every install, and a LAN
//! visit discards any advertisement whose name matches the local one. Two peers
//! left at the default therefore never see each other's games — and the Android
//! build has no text input to change the name with yet (the IME note in
//! `docs/android.md` records that P6 keeps text entry open).
//!
//! The activity hands over the last three hex digits of the Android ID, so the
//! default handle becomes `[New Player]-1A2`: the stock-looking name, unique per
//! install, stable across launches (the ID survives reinstall and app data
//! clears; it changes only on a factory reset), and still narrow enough for the
//! lobby's name box, which the native layout fixes at 134 px and the byte limit
//! only bounds from above.
//!
//! The hook keeps the platform lookup in the Android crate; everywhere else no
//! hook is installed and the stock handle is left exactly as gamemd names it.
//!
//! ## Dependency rules
//! - util/ has NO dependencies on other game modules.
//! - Any module may depend on util/.

use std::sync::OnceLock;

/// What the activity provides for [`handle_suffix`].
pub struct Hooks {
    /// Last three hex digits of the Android ID, upper-case, or `None` when the
    /// device does not supply one.
    pub handle_suffix: fn() -> Option<String>,
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

/// Install the activity hook. The Android crate calls this once, before the
/// event loop.
pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

/// The suffix the device reports, when this build installed a hook and the
/// device supplied one.
pub fn handle_suffix() -> Option<String> {
    HOOKS
        .get()
        .and_then(|hooks| (hooks.handle_suffix)())
        .map(|suffix| suffix.trim().to_owned())
        .filter(|suffix| !suffix.is_empty())
}

/// `base` with `-suffix` appended, cut to the native handle's `limit` bytes.
///
/// Returns `base` unchanged when there is no suffix or when appending one would
/// leave nothing of it inside the limit. The suffix is ASCII, so cutting bytes
/// cannot split a character.
pub fn suffixed_handle(base: &[u8], suffix: Option<&str>, limit: usize) -> Vec<u8> {
    let Some(suffix) = suffix.map(str::trim).filter(|suffix| !suffix.is_empty()) else {
        return base.to_vec();
    };
    let mut bytes = base.to_vec();
    bytes.push(b'-');
    bytes.extend_from_slice(suffix.as_bytes());
    bytes.truncate(limit);
    if bytes.len() > base.len() {
        bytes
    } else {
        base.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::suffixed_handle;

    const STOCK: &[u8] = b"[New Player]";
    const LIMIT: usize = 19;

    #[test]
    fn a_device_suffix_makes_the_stock_handle_unique() {
        let handle = suffixed_handle(STOCK, Some("8ED"), LIMIT);
        assert_eq!(handle, b"[New Player]-8ED".to_vec());
        // The lobby's name box is 134 px of 8 px glyphs: the whole handle fits.
        assert_eq!(handle.len(), 16);
    }

    #[test]
    fn without_a_suffix_the_stock_handle_is_unchanged() {
        assert_eq!(suffixed_handle(STOCK, None, LIMIT), STOCK.to_vec());
        assert_eq!(suffixed_handle(STOCK, Some("  "), LIMIT), STOCK.to_vec());
    }

    #[test]
    fn a_long_suffix_is_cut_to_the_handle_limit() {
        let handle = suffixed_handle(STOCK, Some("ABCDEFGHIJKLMNOP"), LIMIT);
        assert_eq!(handle.len(), LIMIT);
        assert_eq!(handle, b"[New Player]-ABCDEF".to_vec());
    }
}
