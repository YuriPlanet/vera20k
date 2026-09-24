//! Sound and music playback via rodio.
//!
//! Handles game audio: sound effects (weapon fire, unit responses, explosions)
//! and background music (theme.mix tracks). Audio files use Westwood's custom
//! IMA ADPCM format (.aud) and are stored in .mix archives.
//!
//! ## Dependency rules
//! - audio/ depends on: assets/ (decodes .aud files), sim/ (triggers on game events)
//! - audio/ does NOT depend on: render/, ui/, sidebar/, net/

pub mod arbiter;
pub mod events;
pub mod movie_stream;
pub mod music;
pub mod sfx;
pub(crate) mod theme;
pub mod voice_queue;
pub mod vox;
