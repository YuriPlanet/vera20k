//! Logging initialization helpers.
//!
//! Binaries route `log` records into a file under `logs/` in the current
//! working directory, or to stderr. Each record is one line,
//! `[2026-10-18T20:44:36Z INFO  module::path] message`, with the time in UTC.
//!
//! `RUST_LOG` filters records as env_logger did: comma-separated directives,
//! each a level (`warn`), a target prefix (`vera20k_core::sim`, all levels) or
//! both (`wgpu_hal=error`); the longest matching prefix wins. A `/text` suffix
//! keeps only messages containing `text`.

use std::backtrace::Backtrace;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use crate::util::time::{SystemTime, UNIX_EPOCH};

use log::{LevelFilter, Log, Metadata, Record};

static EXCEPTION_DUMP_ACTIVE: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "telemetry")]
#[path = "logging_telemetry.rs"]
mod telemetry;
#[cfg(feature = "telemetry")]
pub use telemetry::{enable_telemetry_capture, ring};

/// Install a logger that appends to `logs/<name>.log`.
///
/// Honors `RUST_LOG` and defaults to `info` when the variable is unset.
/// Returns the resolved log file path on success.
pub fn init_file_logger(name: &str) -> io::Result<PathBuf> {
    let mut log_dir = std::env::current_dir()?;
    log_dir.push("logs");
    fs::create_dir_all(&log_dir)?;

    let mut log_path = log_dir;
    log_path.push(format!("{name}.log"));

    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    // Suppress wgpu_hal's per-frame "Suboptimal present" warnings which
    // flood the log (~100K+ lines per session on AMD Vulkan).
    install("info,wgpu_hal=error", Destination::File(Mutex::new(file)))
        .map_err(|error| io::Error::other(error.to_string()))?;

    Ok(log_path)
}

/// Install a logger that writes to stderr, filtered by `RUST_LOG` or else
/// `default_filter`. Fails when a logger is already installed.
pub fn init_stderr_logger(default_filter: &str) -> Result<(), log::SetLoggerError> {
    install(default_filter, Destination::Stderr)
}

fn install(default_filter: &str, destination: Destination) -> Result<(), log::SetLoggerError> {
    let spec = std::env::var("RUST_LOG").unwrap_or_else(|_| default_filter.to_owned());
    let filter = Filter::parse(&spec);
    let max_level = filter.max_level();
    log::set_logger(Box::leak(Box::new(Logger {
        filter,
        destination,
    })))?;
    log::set_max_level(max_level);
    Ok(())
}

enum Destination {
    Stderr,
    File(Mutex<File>),
}

struct Logger {
    filter: Filter,
    destination: Destination,
}

impl Log for Logger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= self.filter.level_for(metadata.target())
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let message = record.args().to_string();
        if let Some(text) = &self.filter.message_contains
            && !message.contains(text.as_str())
        {
            return;
        }
        #[cfg(feature = "telemetry")]
        telemetry::record(record.level(), record.target(), &message);
        let line = format!(
            "[{} {:<5} {}] {message}\n",
            utc_timestamp(SystemTime::now()),
            record.level(),
            record.target()
        );
        // A failed log write has nowhere to be reported.
        let _ = match &self.destination {
            Destination::Stderr => io::stderr().lock().write_all(line.as_bytes()),
            Destination::File(file) => match file.lock() {
                Ok(mut file) => file.write_all(line.as_bytes()),
                Err(_) => Ok(()),
            },
        };
    }

    fn flush(&self) {
        let _ = match &self.destination {
            Destination::Stderr => io::stderr().flush(),
            Destination::File(file) => file.lock().map_or(Ok(()), |mut file| file.flush()),
        };
    }
}

/// A parsed `RUST_LOG` value.
struct Filter {
    /// `(target prefix, level)`, shortest prefix first; `None` is the default.
    directives: Vec<(Option<String>, LevelFilter)>,
    message_contains: Option<String>,
}

impl Filter {
    fn parse(spec: &str) -> Self {
        let (directives_text, message_contains) = match spec.split_once('/') {
            Some((directives, text)) => (directives, Some(text.to_owned())),
            None => (spec, None),
        };
        let mut directives: Vec<(Option<String>, LevelFilter)> = Vec::new();
        for directive in directives_text.split(',').map(str::trim) {
            if directive.is_empty() {
                continue;
            }
            let (target, level) = match directive.split_once('=') {
                None => match directive.parse() {
                    Ok(level) => (None, level),
                    Err(_) => (Some(directive), LevelFilter::Trace),
                },
                Some((target, level)) if level.trim().is_empty() => {
                    (Some(target.trim()), LevelFilter::Trace)
                }
                Some((target, level)) => match level.trim().parse() {
                    Ok(level) => (Some(target.trim()), level),
                    Err(_) => {
                        eprintln!("warning: invalid logging spec '{level}', ignoring it");
                        continue;
                    }
                },
            };
            let target = target.map(str::to_owned);
            // A repeated target keeps its last level.
            directives.retain(|(existing, _)| *existing != target);
            directives.push((target, level));
        }
        if directives.is_empty() {
            directives.push((None, LevelFilter::Error));
        }
        directives.sort_by_key(|(target, _)| target.as_ref().map_or(0, String::len));
        Self {
            directives,
            message_contains,
        }
    }

    /// The level of the longest directive prefix of `target`; `Off` when none match.
    fn level_for(&self, target: &str) -> LevelFilter {
        let level = self
            .directives
            .iter()
            .rev()
            .find(|(prefix, _)| {
                prefix
                    .as_deref()
                    .is_none_or(|prefix| target.starts_with(prefix))
            })
            .map_or(LevelFilter::Off, |&(_, level)| level);
        // ureq TRACE includes unredacted HTTP wire data. This logger owns
        // every destination (including telemetry); RUST_LOG must never expose
        // account passwords, bearer tokens or relay grants. Keep diagnostic
        // errors without accepting dependency wire/debug logs.
        // https://docs.rs/ureq/3.5.0/ureq/#unversioned
        if matches!(target.split("::").next(), Some("ureq" | "ureq_proto")) {
            level.min(LevelFilter::Info)
        } else {
            level
        }
    }

    fn max_level(&self) -> LevelFilter {
        self.directives
            .iter()
            .map(|&(_, level)| level)
            .max()
            .unwrap_or(LevelFilter::Off)
    }
}

/// `YYYY-MM-DDTHH:MM:SSZ` for `time`, in UTC; the epoch for earlier times.
fn utc_timestamp(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, second_of_day) = (seconds / 86_400, seconds % 86_400);
    // Gregorian date from days since 1970-01-01 (Howard Hinnant's civil_from_days).
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3_600,
        second_of_day / 60 % 60,
        second_of_day % 60
    )
}

/// Register a panic hook that writes the panic info and backtrace to the log
/// file when one is available. Each outermost panic creates or replaces
/// retail's `except.txt` report in the current working directory; recursive
/// hook entry is suppressed. The default stderr hook is preserved so terminal
/// users still see output.
pub fn install_panic_hook(log_path: Option<&Path>) {
    let prev_hook = std::panic::take_hook();
    let log_path = log_path.map(Path::to_owned);

    std::panic::set_hook(Box::new(move |info| {
        let Some(_guard) = claim_exception_dump(&EXCEPTION_DUMP_ACTIVE) else {
            prev_hook(info);
            return;
        };

        // Capture while the panic stack is still live.
        let backtrace = Backtrace::force_capture();

        if let Some(log_path) = &log_path
            && let Ok(mut file) = OpenOptions::new().append(true).open(log_path)
        {
            let _ = writeln!(file, "\n========== PANIC ==========");
            let _ = writeln!(file, "{info}");
            let _ = writeln!(file, "\n{backtrace}");
            let _ = writeln!(file, "===========================");
            let _ = file.flush();
        }

        let _ = write_exception_dump(info, &backtrace);

        // Preserve default stderr output for terminal users.
        prev_hook(info);
    }));
}

struct ExceptionDumpGuard<'a> {
    active: &'a AtomicBool,
}

impl Drop for ExceptionDumpGuard<'_> {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
    }
}

#[inline]
fn claim_exception_dump(active: &AtomicBool) -> Option<ExceptionDumpGuard<'_>> {
    active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .ok()
        .map(|_| ExceptionDumpGuard { active })
}

fn write_exception_dump(
    info: &std::panic::PanicHookInfo<'_>,
    backtrace: &Backtrace,
) -> io::Result<()> {
    let report = normalize_crlf(&format!(
        "{info}\nInternal Version {}\n\n{backtrace}\n",
        crate::util::version::retail_internal_version()
    ));

    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0).attributes(0x80);
    }

    let mut file = options.open("except.txt")?;
    file.write_all(report.as_bytes())?;
    file.flush()
}

fn normalize_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, UNIX_EPOCH};

    use log::LevelFilter;

    use super::{Filter, claim_exception_dump, utc_timestamp};

    #[test]
    fn rust_log_directives_pick_the_longest_matching_target() {
        let filter =
            Filter::parse("info,wgpu_hal=error, vera20k_core::sim=debug,vera20k_core::sim::miner");
        assert_eq!(filter.level_for("vera20k::app"), LevelFilter::Info);
        assert_eq!(filter.level_for("wgpu_hal::metal"), LevelFilter::Error);
        assert_eq!(
            filter.level_for("vera20k_core::sim::world"),
            LevelFilter::Debug
        );
        assert_eq!(
            filter.level_for("vera20k_core::sim::miner::system"),
            LevelFilter::Trace
        );
        assert_eq!(filter.max_level(), LevelFilter::Trace);
        assert_eq!(filter.message_contains, None);

        let only_rules = Filter::parse("vera20k_core::rules=warn/refused");
        assert_eq!(
            only_rules.level_for("vera20k_core::rules::ini"),
            LevelFilter::Warn
        );
        assert_eq!(only_rules.level_for("vera20k_core::sim"), LevelFilter::Off);
        assert_eq!(only_rules.message_contains.as_deref(), Some("refused"));

        // A repeated target keeps its last level; an empty spec logs errors.
        assert_eq!(
            Filter::parse("debug,warn").level_for("any"),
            LevelFilter::Warn
        );
        assert_eq!(Filter::parse("").level_for("any"), LevelFilter::Error);
        assert_eq!(Filter::parse("off").max_level(), LevelFilter::Off);
    }

    #[test]
    fn http_wire_logging_cannot_expose_credentials_even_with_explicit_trace() {
        let filter = Filter::parse("trace,ureq=trace,ureq_proto::client=trace");
        assert_eq!(filter.level_for("ureq::request"), LevelFilter::Info);
        assert_eq!(filter.level_for("ureq_proto::client"), LevelFilter::Info);
        assert_eq!(filter.level_for("vera20k::app"), LevelFilter::Trace);
        assert_eq!(Filter::parse("off").level_for("ureq"), LevelFilter::Off);
    }

    #[test]
    fn timestamps_are_utc_calendar_dates() {
        let at = |seconds| utc_timestamp(UNIX_EPOCH + Duration::from_secs(seconds));
        assert_eq!(at(0), "1970-01-01T00:00:00Z");
        assert_eq!(at(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(at(1_792_355_076), "2026-10-18T20:24:36Z");
        assert_eq!(at(4_107_542_399), "2100-02-28T23:59:59Z");
        assert_eq!(at(4_107_542_400), "2100-03-01T00:00:00Z");
    }

    #[test]
    fn exception_dump_guard_blocks_recursion_then_rearms() {
        let active = AtomicBool::new(false);
        let outer = claim_exception_dump(&active).expect("first handler entry");
        assert!(
            claim_exception_dump(&active).is_none(),
            "recursive handler entry must not overwrite the active report"
        );

        drop(outer);
        assert!(
            claim_exception_dump(&active).is_some(),
            "a later independent handler entry must replace except.txt"
        );
    }
}
