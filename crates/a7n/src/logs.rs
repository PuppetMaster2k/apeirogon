// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::VecDeque;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub(crate) async fn run(lines: usize, follow: bool) -> Result<()> {
    let path = a7n_platform::fs::log_path()?;
    let mut offset = 0;
    let mut previous = None;
    match File::open(&path) {
        Ok(file) => {
            let mut reader = BufReader::new(file);
            let tail = read_tail(&mut reader, lines)?;
            offset = reader.stream_position()?;
            previous = Some(reader.into_inner());
            let mut output = io::stdout().lock();
            for line in tail {
                output.write_all(&line)?;
            }
            output.flush()?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("cannot read daemon log"),
    }
    if !follow {
        return Ok(());
    }
    let mut bytes = Vec::new();
    loop {
        tokio::time::sleep(POLL_INTERVAL).await;
        read_new(&path, &mut previous, &mut offset, &mut bytes)?;
        let mut output = io::stdout().lock();
        output.write_all(&bytes)?;
        output.flush()?;
        bytes.clear();
    }
}

fn read_tail(reader: &mut impl BufRead, lines: usize) -> io::Result<VecDeque<Vec<u8>>> {
    let mut tail = VecDeque::new();
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line)? != 0 {
        if lines != 0 {
            if tail.len() == lines {
                tail.pop_front();
            }
            tail.push_back(std::mem::take(&mut line));
        }
        line.clear();
    }
    Ok(tail)
}

// shortcut: intermediate rotations between polls can be lost; retain more backups if lossless following is needed.
fn read_new(
    path: &Path,
    previous: &mut Option<File>,
    offset: &mut u64,
    bytes: &mut Vec<u8>,
) -> io::Result<()> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if let Some(old) = previous {
        if !a7n_platform::fs::same_file(old, &file)? {
            // Drain the renamed file before switching to the replacement.
            old.seek(io::SeekFrom::Start(*offset))?;
            old.read_to_end(bytes)?;
            *offset = 0;
        }
    }
    if file.metadata()?.len() < *offset {
        *offset = 0;
    }
    file.seek(io::SeekFrom::Start(*offset))?;
    file.read_to_end(bytes)?;
    *offset = file.stream_position()?;
    *previous = Some(file);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_tail_line_limits_returns_only_requested_lines() {
        // Arrange
        for (count, expected) in [
            (0, ""),
            (1, "third"),
            (2, "second\nthird"),
            (4, "first\nsecond\nthird"),
        ] {
            let mut reader = io::Cursor::new(b"first\nsecond\nthird");

            // Act
            let result = read_tail(&mut reader, count).unwrap();

            // Assert
            assert_eq!(
                result.into_iter().flatten().collect::<Vec<_>>(),
                expected.as_bytes()
            );
        }
    }

    #[test]
    fn read_new_rotation_drains_old_file_and_reads_larger_replacement() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("apeirogond.log");
        std::fs::write(&path, b"old\n").unwrap();
        let mut previous = Some(File::open(&path).unwrap());
        let mut offset = 4;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"last\n").unwrap();
        drop(file);
        std::fs::rename(&path, path.with_extension("log.1")).unwrap();
        std::fs::write(&path, b"replacement\n").unwrap();
        let mut bytes = Vec::new();

        // Act
        read_new(&path, &mut previous, &mut offset, &mut bytes).unwrap();

        // Assert
        assert_eq!(bytes, b"last\nreplacement\n");
        assert_eq!(offset, 12);
    }
}
