//! Messages and notifications between a simulation module and the client
//! modules of its source (RFC 0001 section 10 "Communication", appendix 1.1
//! #22 `send_notify`, 1.2 `on_message`).
//!
//! - A client module sends bytes with `client::Client::send_message`; after
//!   the command delay they reach this source's simulation module's
//!   [`on_message!`](crate::on_message!) handler on every machine, with the
//!   sender's house, which the engine fills in. The bytes are untrusted:
//!   check them all.
//! - The simulation module notifies a house's player (or the observers) with
//!   [`Notify::send`]; each client module there receives a `Notify` client
//!   event. The module declares every name it sends with
//!   [`notify_names!`](crate::notify_names!), which writes its `vera.notify`
//!   section at build time; `lookup` knows only declared names.
//!
//! ```ignore
//! vera20k_sdk::notify_names! {
//!     pub static REINFORCED: Notify = "Reinforced";
//! }
//!
//! // Not named `on_message`: the macro exports that symbol.
//! fn buy(sender: House, data: &[u8]) {
//!     if data == [1] {
//!         let _ = REINFORCED.send(sender, &[]);
//!     }
//! }
//! vera20k_sdk::on_message!(buy);
//! ```

use core::sync::atomic::{AtomicI32, Ordering};

use crate::{House, NO_HANDLE, NameKind, Result, check, lookup, sys};

/// The largest message (RFC 15.4 "Messages"): what `on_message` receives at
/// most, and what fits the SDK's delivery area.
pub const MAX_MESSAGE_BYTES: usize = 96;
/// The largest notification (RFC 15.4 "Notifications").
pub const MAX_NOTIFY_BYTES: usize = 256;

/// A notification name this module declared with
/// [`notify_names!`](crate::notify_names!). Its ID (the name's FNV-1a hash,
/// RFC 10) is resolved with `lookup(Notify, name)` on first use.
pub struct Notify {
    name: &'static str,
    id: AtomicI32,
}

/// No ID resolved yet: −1 is no declared name's hash (RFC 3, step 7).
const UNRESOLVED: i32 = -1;

impl Notify {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            id: AtomicI32::new(UNRESOLVED),
        }
    }

    /// The name as declared.
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// The notification's ID: the same on every machine and in the client
    /// world, where a `Notify` event's `id` carries it.
    pub fn id(&self) -> Result<i32> {
        let id = self.id.load(Ordering::Relaxed);
        if id != UNRESOLVED {
            return Ok(id);
        }
        let id = lookup(NameKind::Notify, self.name)?;
        self.id.store(id, Ordering::Relaxed);
        Ok(id)
    }

    /// Notifies the player of `to` with `data` (at most
    /// [`MAX_NOTIFY_BYTES`]). `Err(Refused)` over the size or per-frame
    /// count limit.
    pub fn send(&self, to: House, data: &[u8]) -> Result<()> {
        send_raw(to.0, self.id()?, data)
    }

    /// Notifies every observer client with `data`.
    pub fn send_observers(&self, data: &[u8]) -> Result<()> {
        send_raw(NO_HANDLE, self.id()?, data)
    }
}

/// `send_notify(to, id, ptr, len)`: `to` 0 addresses the observers.
pub fn send_raw(to: i64, id: i32, data: &[u8]) -> Result<()> {
    check(unsafe { sys::send_notify(to, id, data.as_ptr(), data.len() as i32) })
}

/// The `vera.notify` section's byte length for `names`.
pub const fn section_len(names: &[&str]) -> usize {
    let mut len = 1 + 4;
    let mut i = 0;
    while i < names.len() {
        len += 2 + names[i].len();
        i += 1;
    }
    len
}

/// The `vera.notify` section (appendix "Custom sections"): format 1, count,
/// then each name's `u16` length and ASCII bytes. Names must be unique
/// ignoring case and not hash to −1; the engine checks at load.
pub const fn section<const N: usize>(names: &[&str]) -> [u8; N] {
    let mut out = [0u8; N];
    out[0] = 1;
    let count = (names.len() as u32).to_le_bytes();
    let mut at = 1;
    let mut i = 0;
    while i < 4 {
        out[at + i] = count[i];
        i += 1;
    }
    at += 4;
    let mut n = 0;
    while n < names.len() {
        let name = names[n].as_bytes();
        let len = (name.len() as u16).to_le_bytes();
        out[at] = len[0];
        out[at + 1] = len[1];
        at += 2;
        let mut b = 0;
        while b < name.len() {
            out[at + b] = name[b];
            b += 1;
        }
        at += name.len();
        n += 1;
    }
    out
}

/// Declares every notification name the module sends: writes its one
/// `vera.notify` section and a static [`Notify`] per name. A module has at
/// most one such list (a second would append a second blob, which the
/// engine rejects).
///
/// ```ignore
/// vera20k_sdk::notify_names! {
///     pub static REINFORCED: Notify = "Reinforced";
///     pub static REFUSED: Notify = "Refused";
/// }
/// ```
#[macro_export]
macro_rules! notify_names {
    ($($v:vis static $ident:ident : Notify = $name:literal;)*) => {
        #[used]
        #[unsafe(link_section = "vera.notify")]
        static __VERA_NOTIFY: [u8; $crate::notify::section_len(&[$($name),*])] =
            $crate::notify::section(&[$($name),*]);
        $($v static $ident: $crate::notify::Notify = $crate::notify::Notify::new($name);)*
    };
}

/// Exports `on_message(sender, ptr, len)`, calling `$f(sender: House, data:
/// &[u8])` once per message a client module of this source sent (appendix
/// 1.2), in the module phase before `on_events`. The engine writes the
/// bytes into the delivery area, which the macro declares; a module that
/// also uses [`crate::on_events!`] or [`crate::on_decide!`], which declare
/// it too, writes `on_message!(shared_delivery: f)`.
#[macro_export]
macro_rules! on_message {
    (shared_delivery: $f:path) => {
        #[unsafe(no_mangle)]
        /// # Safety
        /// Only the engine may call this export with its current message bytes.
        pub unsafe extern "C" fn on_message(sender: i64, ptr: i32, len: i32) {
            // SAFETY: the engine's own arguments: `len` bytes at `ptr` in the
            // delivery area, read only during this call.
            let data = unsafe { $crate::notify::message(ptr, len) };
            let handler: for<'msg> fn($crate::House, &'msg [u8]) = $f;
            handler($crate::House(sender), data)
        }
    };
    ($f:path) => {
        #[used]
        #[unsafe(link_section = "vera.delivery")]
        static __VERA_DELIVERY: [u8; 9] = $crate::DELIVERY_SECTION;

        $crate::on_message!(shared_delivery: $f);
    };
}

/// The message `on_message(sender, ptr, len)` received.
///
/// # Safety
/// `ptr` and `len` must be the arguments the engine passed to `on_message`,
/// and the slice must not outlive that call.
pub unsafe fn message<'a>(ptr: i32, len: i32) -> &'a [u8] {
    if len <= 0 {
        return &[];
    }
    // SAFETY: the engine wrote `len` bytes at `ptr` in module memory.
    unsafe { core::slice::from_raw_parts(ptr as usize as *const u8, len as usize) }
}
