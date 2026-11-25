//! Prompt-safe `tracing` output and the plain `logs/latest.log` file.
//!
//! Console lines are `[HH:MM:SS LEVEL] message`, Paper style, with a colored
//! level tag. The file copy is identical minus colors and `§` codes. When the
//! local date changes, `latest.log` is gzipped to
//! `logs/YYYY-MM-DD-N.log.gz`, matching the vanilla archive naming.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Local, NaiveDate};
use flate2::Compression;
use flate2::write::GzEncoder;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;

use crate::color::strip_minecraft;
use crate::output::ConsoleOutput;

const LATEST_LOG: &str = "latest.log";

/// Plain-text log file with vanilla-style daily gzip rotation.
pub struct LogFile {
    dir: PathBuf,
    latest: PathBuf,
    writer: BufWriter<File>,
    date: NaiveDate,
}

impl LogFile {
    /// Open `dir/latest.log`, archiving a non-empty file left by a previous
    /// run as `dir/YYYY-MM-DD-N.log.gz` first.
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        let latest = dir.join(LATEST_LOG);
        if let Ok(metadata) = fs::metadata(&latest)
            && metadata.len() > 0
        {
            let modified = metadata.modified().unwrap_or(std::time::SystemTime::now());
            let date = DateTime::<Local>::from(modified).date_naive();
            gzip_to(&latest, &next_archive_path(&dir, date))?;
        }
        let writer = BufWriter::new(File::create(&latest)?);
        Ok(Self {
            dir,
            latest,
            writer,
            date: Local::now().date_naive(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.latest
    }

    /// Append one plain line (without its newline); `§` codes are removed.
    pub fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.rotate_if_needed()?;
        let plain = strip_minecraft(line);
        for part in plain.split('\n') {
            self.writer.write_all(part.as_bytes())?;
            self.writer.write_all(b"\n")?;
        }
        self.writer.flush()
    }

    /// Rotate when the local date no longer matches the open file's date.
    pub fn rotate_if_needed(&mut self) -> io::Result<bool> {
        let today = Local::now().date_naive();
        if today == self.date {
            return Ok(false);
        }
        self.rotate(self.date)?;
        self.date = today;
        Ok(true)
    }

    /// Archive the current `latest.log` immediately and start a new one.
    pub fn force_rotate(&mut self) -> io::Result<PathBuf> {
        let archive = self.rotate(self.date)?;
        self.date = Local::now().date_naive();
        Ok(archive)
    }

    fn rotate(&mut self, date: NaiveDate) -> io::Result<PathBuf> {
        self.writer.flush()?;
        let archive = next_archive_path(&self.dir, date);
        gzip_to(&self.latest, &archive)?;
        self.writer = BufWriter::new(File::create(&self.latest)?);
        Ok(archive)
    }
}

/// `dir/YYYY-MM-DD-N.log.gz`, the vanilla archive name.
pub fn archive_path(dir: &Path, date: NaiveDate, index: u32) -> PathBuf {
    dir.join(format!("{}-{index}.log.gz", date.format("%Y-%m-%d")))
}

fn next_archive_path(dir: &Path, date: NaiveDate) -> PathBuf {
    let mut index = 1;
    loop {
        let path = archive_path(dir, date, index);
        if !path.exists() {
            return path;
        }
        index += 1;
    }
}

fn gzip_to(source: &Path, target: &Path) -> io::Result<()> {
    let mut input = File::open(source)?;
    let output = File::create(target)?;
    let mut encoder = GzEncoder::new(BufWriter::new(output), Compression::default());
    io::copy(&mut input, &mut encoder)?;
    let mut output = encoder.finish()?;
    output.flush()
}

/// `tracing` layer that mirrors events to the prompt-safe console and,
/// optionally, to a rotating plain log file.
pub struct LogLayer {
    console: Arc<ConsoleOutput>,
    file: Option<Arc<Mutex<LogFile>>>,
    reported_file_error: AtomicBool,
}

impl LogLayer {
    pub fn new(console: Arc<ConsoleOutput>) -> Self {
        Self {
            console,
            file: None,
            reported_file_error: AtomicBool::new(false),
        }
    }

    /// Also write every event to `file`.
    pub fn with_file(self, file: LogFile) -> Self {
        self.with_shared_file(Arc::new(Mutex::new(file)))
    }

    /// Also write every event to an already shared log file (for example so a
    /// shutdown path can force a final rotation).
    pub fn with_shared_file(mut self, file: Arc<Mutex<LogFile>>) -> Self {
        self.file = Some(file);
        self
    }

    fn report_file_error(&self, error: io::Error) {
        if !self.reported_file_error.swap(true, Ordering::Relaxed) {
            eprintln!("ferrite-console: failed to write {}: {error}", LATEST_LOG);
        }
    }
}

#[derive(Default)]
struct EventFields {
    message: String,
    pairs: Vec<(String, String)>,
}

impl Visit for EventFields {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message.push_str(value);
        } else {
            self.pairs.push((field.name().to_owned(), value.to_owned()));
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message.push_str(&format!("{value:?}"));
        } else {
            self.pairs
                .push((field.name().to_owned(), format!("{value:?}")));
        }
    }
}

impl<S: Subscriber> Layer<S> for LogLayer {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut fields = EventFields::default();
        event.record(&mut fields);
        let level = *event.metadata().level();
        let time = Local::now().format("%H:%M:%S").to_string();
        self.console.print_line(&format_line(
            &time,
            level,
            &fields.message,
            &fields.pairs,
            self.console.is_tty(),
        ));
        if let Some(file) = &self.file {
            let line = format_line(&time, level, &fields.message, &fields.pairs, false);
            let mut file = file.lock().unwrap_or_else(|error| error.into_inner());
            if let Err(error) = file.write_line(&line) {
                drop(file);
                self.report_file_error(error);
            }
        }
    }
}

fn format_line(
    time: &str,
    level: Level,
    message: &str,
    pairs: &[(String, String)],
    colored: bool,
) -> String {
    let mut line = String::with_capacity(message.len() + pairs.len() * 16 + 32);
    if colored {
        use fmt::Write as _;
        let (lvl_color, msg_color) = level_colors(level);
        let _ = write!(
            line,
            "\x1b[90m[\x1b[0m\x1b[37m{time}\x1b[0m {lvl_color}{}\x1b[0m\x1b[90m]\x1b[0m",
            level.as_str()
        );
        if !message.is_empty() {
            let _ = write!(line, " {msg_color}{message}\x1b[0m");
        }
        for (k, v) in pairs {
            let _ = write!(line, " \x1b[36m{k}\x1b[90m=\x1b[37m{v}\x1b[0m");
        }
    } else {
        use fmt::Write as _;
        let _ = write!(line, "[{time} {}]", level.as_str());
        if !message.is_empty() {
            line.push(' ');
            line.push_str(message);
        }
        for (k, v) in pairs {
            let _ = write!(line, " {k}={v}");
        }
    }
    line
}

fn level_colors(level: Level) -> (&'static str, &'static str) {
    match level {
        Level::TRACE => ("\x1b[1;35m", "\x1b[90m"),
        Level::DEBUG => ("\x1b[1;34m", "\x1b[90m"),
        Level::INFO => ("\x1b[1;32m", "\x1b[0m"),
        Level::WARN => ("\x1b[1;33m", "\x1b[33m"),
        Level::ERROR => ("\x1b[1;31m", "\x1b[31m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::atomic::AtomicU32;

    static NEXT: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let unique = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "ferrite-console-{name}-{}-{unique}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn read_gzip(path: &Path) -> String {
        let file = File::open(path).unwrap();
        let mut decoder = flate2::read::GzDecoder::new(file);
        let mut contents = String::new();
        decoder.read_to_string(&mut contents).unwrap();
        contents
    }

    #[test]
    fn archive_names_follow_vanilla_pattern() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 28).unwrap();
        assert_eq!(
            archive_path(Path::new("logs"), date, 1),
            PathBuf::from("logs/2026-10-28-1.log.gz")
        );
        assert_eq!(
            archive_path(Path::new("logs"), date, 12),
            PathBuf::from("logs/2026-10-28-12.log.gz")
        );
    }

    #[test]
    fn writes_plain_lines_and_rotates_gzip() {
        let dir = TempDir::new("log-rotate");
        let mut file = LogFile::open(&dir.0).unwrap();
        file.write_line("§aHello §lWorld").unwrap();
        file.write_line("§7second").unwrap();
        assert_eq!(
            fs::read_to_string(dir.0.join(LATEST_LOG)).unwrap(),
            "Hello World\nsecond\n"
        );

        let first = file.force_rotate().unwrap();
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        assert_eq!(
            first.file_name().unwrap().to_string_lossy(),
            format!("{today}-1.log.gz")
        );
        assert_eq!(read_gzip(&first), "Hello World\nsecond\n");
        assert_eq!(fs::read_to_string(dir.0.join(LATEST_LOG)).unwrap(), "");

        file.write_line("third").unwrap();
        let second = file.force_rotate().unwrap();
        assert_eq!(
            second.file_name().unwrap().to_string_lossy(),
            format!("{today}-2.log.gz")
        );
        assert_eq!(read_gzip(&second), "third\n");
    }

    #[test]
    fn opening_archives_previous_latest_log() {
        let dir = TempDir::new("log-open");
        fs::write(dir.0.join(LATEST_LOG), "old\n").unwrap();
        let _file = LogFile::open(&dir.0).unwrap();
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let archive = dir.0.join(format!("{today}-1.log.gz"));
        assert_eq!(read_gzip(&archive), "old\n");
        assert_eq!(fs::read_to_string(dir.0.join(LATEST_LOG)).unwrap(), "");
    }

    #[test]
    fn formats_paper_style_lines() {
        let colored = format_line("12:34:56", Level::INFO, "Listening", &[], true);
        assert!(colored.contains("\x1b[1;32mINFO\x1b[0m"));
        assert!(colored.contains("Listening"));
        let plain = format_line(
            "12:34:56",
            Level::WARN,
            "§cCareful",
            &[("key".to_owned(), "value".to_owned())],
            false,
        );
        assert_eq!(plain, "[12:34:56 WARN] §cCareful key=value");
        assert_eq!(strip_minecraft(&plain), "[12:34:56 WARN] Careful key=value");
        let error = format_line("01:02:03", Level::ERROR, "boom", &[], true);
        assert!(error.contains("\x1b[1;31mERROR\x1b[0m"));
        assert!(error.contains("\x1b[31mboom\x1b[0m"));
    }
}
