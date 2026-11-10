// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::{self, Write as _};
use std::io::{self, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_FILE_MAX: u64 = 1024 * 1024;
const LOG_LINES_PER_SECOND: u32 = 50;
static WRITER: Mutex<State> = Mutex::new(State {
    second: 0,
    lines: 0,
    suppressed: 0,
});

#[derive(Default)]
struct State {
    second: u64,
    lines: u32,
    suppressed: u64,
}

impl State {
    fn admit(&mut self, second: u64) -> Option<u64> {
        if second > self.second {
            self.second = second;
            self.lines = 0;
        }
        if self.lines >= LOG_LINES_PER_SECOND {
            self.suppressed = self.suppressed.saturating_add(1);
            return None;
        }
        let suppressed = std::mem::take(&mut self.suppressed);
        self.lines += 1 + u32::from(suppressed > 0);
        Some(suppressed)
    }
}

macro_rules! event {
    ($($arg:tt)*) => { $crate::log::write(format_args!($($arg)*)) };
}
pub(crate) use event;

pub(crate) fn write(args: fmt::Arguments<'_>) {
    let Ok(mut state) = WRITER.lock() else {
        return;
    };
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let Some(suppressed) = state.admit(seconds) else {
        return;
    };
    let stamp = timestamp(seconds);
    let mut line = String::new();
    if suppressed > 0 {
        let _ = writeln!(line, "{stamp} kind=log_suppressed count={suppressed}");
    }
    let _ = writeln!(line, "{stamp} {args}");
    let _ = io::stderr().lock().write_all(line.as_bytes());
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
    fn admit_line_flood_limits_output_and_reports_suppressed() {
        // Arrange
        let mut state = State::default();

        // Act
        let admitted: Vec<_> = (0..LOG_LINES_PER_SECOND).map(|_| state.admit(1)).collect();
        let overflow = state.admit(1);
        let next = state.admit(2);
        let remaining = (0..LOG_LINES_PER_SECOND)
            .filter(|_| state.admit(2).is_some())
            .count();

        // Assert
        assert!(admitted.iter().all(|result| *result == Some(0)));
        assert_eq!(overflow, None);
        assert_eq!(next, Some(1));
        assert_eq!(remaining, LOG_LINES_PER_SECOND as usize - 2);
    }
}
