use std::fs::{create_dir_all, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Logger {
    file: Option<Mutex<File>>,
    log_path: PathBuf,
}

impl Logger {
    pub fn init(custom_log_path: Option<PathBuf>) -> Self {
        let path = custom_log_path.unwrap_or_else(|| {
            if let Some(home) = std::env::var_os("HOME") {
                PathBuf::from(home).join(".audio-fixer").join("audio-fixer.log")
            } else {
                PathBuf::from("/tmp").join("audio-fixer.log")
            }
        });

        if let Some(parent) = path.parent() {
            let _ = create_dir_all(parent);
        }

        let file_res = OpenOptions::new().create(true).append(true).open(&path);
        let file = file_res.ok().map(Mutex::new);

        Logger {
            file,
            log_path: path,
        }
    }

    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    pub fn log(&self, msg: &str) {
        let timestamp = current_timestamp();
        let plain_msg = strip_ansi(msg);
        let line = format!("[{}] {}\n", timestamp, plain_msg);

        if let Some(ref file_mutex) = self.file {
            if let Ok(mut f) = file_mutex.lock() {
                let _ = f.write_all(line.as_bytes());
            }
        }
    }
}

fn current_timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let seconds = secs % 60;
    let minutes = (secs / 60) % 60;
    let hours = (secs / 3600) % 24;

    // Epoch days conversion
    let days_since_epoch = secs / 86400;
    let (year, month, day) = epoch_days_to_ymd(days_since_epoch);

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}

fn epoch_days_to_ymd(days: u64) -> (u64, u64, u64) {
    // Simple civil calendar calculation from Unix epoch days
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_escape = false;

    for c in input.chars() {
        if c == '\x1b' {
            in_escape = true;
        } else if in_escape {
            if c == 'm' || c == 'K' || c == 'H' || c == 'J' {
                in_escape = false;
            }
        } else {
            out.push(c);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi() {
        let colored = "\x1b[32mHello World\x1b[0m";
        assert_eq!(strip_ansi(colored), "Hello World");
    }

    #[test]
    fn test_current_timestamp() {
        let ts = current_timestamp();
        assert_eq!(ts.len(), 19);
        assert!(ts.contains('-'));
        assert!(ts.contains(':'));
    }
}
