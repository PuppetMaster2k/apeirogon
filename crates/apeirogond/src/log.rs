// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::io::{self, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_FILE_MAX: u64 = 1024 * 1024;
static WRITER: Mutex<()> = Mutex::new(());

macro_rules! event {
    ($($arg:tt)*) => { $crate::log::write(format_args!($($arg)*)) };
}
pub(crate) use event;

pub(crate) fn write(args: fmt::Arguments<'_>) {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let line = format!("{} {args}\n", timestamp(seconds));
    let _ = io::stderr().lock().write_all(line.as_bytes());
    let Ok(_guard) = WRITER.lock() else {
        return;
    };
    if let Ok(path) = a7n_platform::fs::log_path() {
        let _ = append(&path, line.as_bytes());
    }
}

fn rotate(path: &Path) -> io::Result<()> {
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > LOG_FILE_MAX) {
        let backup = path.with_extension("log.1");
        match std::fs::remove_file(&backup) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        std::fs::rename(path, backup)?;
    }
    Ok(())
}

fn append(path: &Path, line: &[u8]) -> io::Result<()> {
    rotate(path)?;
    {
        let mut file = a7n_platform::fs::append_user_only(path)?;
        file.write_all(line)?;
    }
    rotate(path)?;
    // Rotation leaves a fresh file ready for readers following the path.
    a7n_platform::fs::append_user_only(path)?;
    Ok(())
}

fn timestamp(seconds: u64) -> String {
    // Gregorian civil date from days since the Unix epoch (400-year eras).
    let days = seconds / 86_400 + 719_468;
    let era = days / 146_097;
    let day = days % 146_097;
    let year = (day - day / 1460 + day / 36_524 - day / 146_096) / 365;
    let day_of_year = day - (365 * year + year / 4 - year / 100);
    let month = (5 * day_of_year + 2) / 153;
    let date = day_of_year - (153 * month + 2) / 5 + 1;
    let (month, year) = if month < 10 {
        (month + 3, year + era * 400)
    } else {
        (month - 9, year + era * 400 + 1)
    };
    format!(
        "{year:04}-{month:02}-{date:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_limit_crossed_rotates_and_replaces_backup() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("apeirogond.log");
        let bytes = vec![b'x'; usize::try_from(LOG_FILE_MAX).unwrap()];
        append(&path, &bytes).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), LOG_FILE_MAX);
        std::fs::write(path.with_extension("log.1"), b"old").unwrap();

        // Act
        append(&path, b"y").unwrap();

        // Assert
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        assert_eq!(
            std::fs::metadata(path.with_extension("log.1"))
                .unwrap()
                .len(),
            LOG_FILE_MAX + 1
        );
    }

    #[test]
    fn timestamp_epoch_and_leap_day_formats_utc() {
        // Arrange
        let cases = [
            (0, "1970-01-01T00:00:00Z"),
            (951_827_696, "2000-02-29T12:34:56Z"),
        ];
        for (seconds, expected) in cases {
            // Act
            let result = timestamp(seconds);

            // Assert
            assert_eq!(result, expected);
        }
    }
}
