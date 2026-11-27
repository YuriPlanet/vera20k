//! Reads HD art pack pictures off the render thread.
//!
//! VERA-only presentation extension, part of HD art (`render::hd_switch`).
//! A pack picture (`<hash>.png`) costs a file open, which a virus scanner
//! can make several milliseconds the first time, and a PNG decode. Read on
//! the render thread, a screen of new buildings or ground stalls the frames
//! that meet it; here a few worker threads read and decode them, and the
//! passes (`render::hd_art`, `render::hd_terrain`) take the finished
//! pictures at the start of a frame. A picture not ready yet draws native
//! until it is. Nothing reaches `sim/`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

/// Reader threads of one pass: file opens wait on the disk and the scanner
/// more than on the processor, so a few overlap them.
#[cfg(not(target_arch = "wasm32"))]
const WORKERS: usize = 3;

/// A finished read: the picture's RGBA, or `None` when the pack has no
/// picture of the expected size.
pub(crate) struct Loaded {
    pub key: u64,
    pub rgba: Option<Vec<u8>>,
}

struct Job {
    key: u64,
    path: PathBuf,
    size: [u32; 2],
}

/// `path`'s PNG as RGBA when it is `size` pixels, else `None`.
pub(crate) fn read_picture(path: &Path, size: [u32; 2]) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .ok()?
        .into_rgba8();
    (image.width() == size[0] && image.height() == size[1]).then(|| image.into_raw())
}

/// One pass's readers. Dropping it ends the threads once their current
/// picture is read.
pub(crate) struct HdLoader {
    /// `None` where no thread runs (a browser build, or a failed spawn): the
    /// picture is then read at once.
    jobs: Option<Sender<Job>>,
    finished_tx: Sender<Loaded>,
    finished: Receiver<Loaded>,
    in_flight: usize,
}

impl HdLoader {
    pub(crate) fn new(name: &str) -> Self {
        let (finished_tx, finished) = channel();
        Self {
            jobs: spawn_workers(name, &finished_tx),
            finished_tx,
            finished,
            in_flight: 0,
        }
    }

    /// Pictures asked for and not yet taken.
    pub(crate) fn in_flight(&self) -> usize {
        self.in_flight
    }

    /// Read `path` (expected `size` pixels) for `key`.
    pub(crate) fn request(&mut self, key: u64, path: PathBuf, size: [u32; 2]) {
        self.in_flight += 1;
        let job = Job { key, path, size };
        let job = match &self.jobs {
            Some(jobs) => match jobs.send(job) {
                Ok(()) => return,
                Err(failed) => failed.0,
            },
            None => job,
        };
        let rgba = read_picture(&job.path, job.size);
        let _ = self.finished_tx.send(Loaded { key: job.key, rgba });
    }

    /// A finished picture, if one is ready.
    pub(crate) fn take(&mut self) -> Option<Loaded> {
        let loaded = self.finished.try_recv().ok()?;
        self.in_flight = self.in_flight.saturating_sub(1);
        Some(loaded)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn_workers(name: &str, finished: &Sender<Loaded>) -> Option<Sender<Job>> {
    use std::sync::{Arc, Mutex};
    let (jobs, queue) = channel::<Job>();
    let queue = Arc::new(Mutex::new(queue));
    let mut spawned = 0;
    for index in 0..WORKERS {
        let queue = Arc::clone(&queue);
        let finished = finished.clone();
        let worker = std::thread::Builder::new()
            .name(format!("{name} {index}"))
            .spawn(move || {
                loop {
                    let job = match queue.lock() {
                        Ok(queue) => queue.recv(),
                        Err(_) => return,
                    };
                    let Ok(job) = job else {
                        return;
                    };
                    let rgba = read_picture(&job.path, job.size);
                    if finished.send(Loaded { key: job.key, rgba }).is_err() {
                        return;
                    }
                }
            });
        spawned += usize::from(worker.is_ok());
    }
    (spawned > 0).then_some(jobs)
}

#[cfg(target_arch = "wasm32")]
fn spawn_workers(_name: &str, _finished: &Sender<Loaded>) -> Option<Sender<Job>> {
    None
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::HdLoader;

    #[test]
    fn a_missing_picture_comes_back_empty_and_the_count_settles() {
        let mut loader = HdLoader::new("HD test");
        let dir = std::env::temp_dir().join("vera20k-hd-loader-test-missing");
        loader.request(7, dir.join("0000000000000007.png"), [4, 2]);
        assert_eq!(loader.in_flight(), 1);
        let loaded = loop {
            if let Some(loaded) = loader.take() {
                break loaded;
            }
            std::thread::yield_now();
        };
        assert_eq!(loaded.key, 7);
        assert!(loaded.rgba.is_none());
        assert_eq!(loader.in_flight(), 0);
    }

    #[test]
    fn a_picture_of_the_wrong_size_is_refused() {
        let dir = std::env::temp_dir().join("vera20k-hd-loader-test-size");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("picture.png");
        image::RgbaImage::new(4, 2).save(&path).unwrap();
        assert_eq!(
            super::read_picture(&path, [4, 2]).map(|rgba| rgba.len()),
            Some(32)
        );
        assert!(super::read_picture(&path, [2, 4]).is_none());
    }
}
