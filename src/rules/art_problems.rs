//! VERA-only, experimental (`docs/picture-model-art.md`): what is wrong with
//! the declared picture and model art a load reads, for the player.
//!
//! Each problem is one line naming the art section, the key or file, what is
//! wrong and what the game does instead. The rules code (`art_data`) and the
//! render code (`picture_atlas`, `sprite_atlas`) report here as they meet a
//! problem; their log lines stay as they are. A match load starts a new list
//! (`app::loading::init_helpers::bind_rules_art_metadata`) and the match
//! shows it once it has loaded (`app::input::messages::post_art_problems`).
//!
//! Only declared `Picture=` and `Model=` art reports, and only when something
//! is wrong, so retail data never touches the list and no hot path allocates
//! for it. The list is process-wide because the reports come from the
//! loading threads and the render thread alike.

use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};

/// The problems since the last [`begin`]: each distinct line once, in the
/// order first reported.
struct Problems {
    lines: Vec<String>,
    seen: BTreeSet<String>,
}

static PROBLEMS: Mutex<Problems> = Mutex::new(Problems {
    lines: Vec::new(),
    seen: BTreeSet::new(),
});

fn problems() -> std::sync::MutexGuard<'static, Problems> {
    PROBLEMS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Note that art section `section`'s `subject` (a key such as `PictureSize=`,
/// or a file) has `problem`, so the game does `instead`. The same line
/// reported again is kept once.
pub fn report(section: &str, subject: &str, problem: &str, instead: &str) {
    let line = format!("[{section}] {subject}: {problem}; {instead}.");
    let mut problems = problems();
    if !problems.seen.contains(&line) {
        problems.seen.insert(line.clone());
        problems.lines.push(line);
    }
}

/// Start a new list: a load that reads the art again reports again.
pub fn begin() {
    let mut problems = problems();
    problems.lines.clear();
    problems.seen.clear();
}

/// The lines reported since [`begin`], leaving the list empty.
pub fn take() -> Vec<String> {
    let mut problems = problems();
    problems.seen.clear();
    std::mem::take(&mut problems.lines)
}

/// The lines reported since [`begin`], left in the list.
#[cfg(test)]
pub(crate) fn lines() -> Vec<String> {
    problems().lines.clone()
}
