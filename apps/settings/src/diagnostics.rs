use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Mutex;

struct FileLogger(Mutex<File>);

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
            || (metadata.level() == log::Level::Info
                && (metadata.target() == "settings" || metadata.target().starts_with("gpui_")))
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata())
            && let Ok(mut file) = self.0.lock()
        {
            let _ = writeln!(
                file,
                "[{} {}] {}",
                record.level(),
                record.target(),
                record.args()
            );
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.0.lock() {
            let _ = file.flush();
        }
    }
}

/// A launcher may discard stderr. Keep renderer diagnostics opt-in and private.
pub fn init() {
    let Some(path) = std::env::var_os("DENIAL_SETTINGS_LOG") else {
        return;
    };
    let file = match OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
    {
        Ok(file) => file,
        Err(error) => {
            eprintln!("settings: could not open diagnostics log: {error}");
            return;
        }
    };
    let logger = Box::leak(Box::new(FileLogger(Mutex::new(file))));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}
