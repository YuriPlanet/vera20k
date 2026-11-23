//! The JSON Lines loop: one request line in, one event line out.

use std::io::{BufRead, Read, Write};

use super::command::Request;
use super::observe::Event;
use super::session::Session;

/// The largest request line the protocol accepts, including its newline.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// What the loop ended on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The match ended: a quit, the end of the input, the tick limit or a
    /// result. The driver may start another match.
    Finished,
    /// A fatal error ended the session; the driver must not continue.
    Failed(String),
}

/// Write one event as a single flushed line.
pub fn write_event(output: &mut impl Write, event: &Event) -> Result<(), String> {
    serde_json::to_writer(&mut *output, event).map_err(|error| error.to_string())?;
    output
        .write_all(b"\n")
        .and_then(|()| output.flush())
        .map_err(|error| error.to_string())
}

/// Read requests from `input` until the session ends, writing one event per
/// request to `output`.
///
/// A blank line is ignored. A line longer than [`MAX_LINE_BYTES`] ends the
/// session: the rest of it was never read, so the stream cannot be trusted to
/// hold a whole request. A line that is not a request is answered with a
/// non-fatal error, so a driver that sent one malformed line can continue.
pub fn run<R: BufRead, W: Write>(
    session: &mut Session,
    mut input: R,
    mut output: W,
) -> Result<Outcome, String> {
    let mut line = Vec::new();
    loop {
        line.clear();
        (&mut input)
            .take((MAX_LINE_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|error| error.to_string())?;
        let event = if line.is_empty() {
            session.eof()
        } else if line.len() > MAX_LINE_BYTES {
            Event::Error {
                message: format!("request exceeds {MAX_LINE_BYTES} bytes"),
                fatal: true,
            }
        } else if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        } else {
            match serde_json::from_slice::<Request>(&line) {
                Ok(request) => session.handle(request),
                Err(error) => Event::Error {
                    message: format!("invalid request: {error}"),
                    fatal: false,
                },
            }
        };
        write_event(&mut output, &event)?;
        match event {
            Event::Finished { .. } => return Ok(Outcome::Finished),
            Event::Error {
                message,
                fatal: true,
            } => return Ok(Outcome::Failed(message)),
            _ => {}
        }
    }
}
