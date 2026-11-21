//! Background jobs whose one result a frame polls for: the online map
//! library's reads and the online login page's counts.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;

use anyhow::Result;

/// Whether this build starts worker threads. The browser build does not, so
/// the browser uses asynchronous fetch for the login page counts.
pub(crate) const THREADS: bool = !cfg!(all(target_arch = "wasm32", target_os = "unknown"));

/// One worker thread whose result arrives on the receiver; `work` gets
/// `cancel`. `None` when the thread cannot start, or never can ([`THREADS`]).
pub(crate) fn spawn<T: Send + 'static>(
    name: &str,
    cancel: Arc<AtomicBool>,
    work: impl FnOnce(&AtomicBool) -> Result<T> + Send + 'static,
) -> Option<Receiver<Result<T>>> {
    if !THREADS {
        return None;
    }
    let (output, input) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || {
            let _ = output.send(work(&cancel));
        })
        .ok()?;
    Some(input)
}
