// SPDX-License-Identifier: GPL-3.0-or-later

//! File manifest validation, target names and resume offsets without I/O.

use std::collections::HashMap;

use a7n_proto::limits;
use a7n_proto::v1::{FileKind, FileManifest, FileResume};

mod path;
pub use path::{ManifestPath, PathError};

const NAME_ATTEMPTS_MAX: usize = 1000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ManifestError {
    #[error("manifest exceeds the entry limit")]
    TooManyEntries,
    #[error("entry {index} has an invalid path: {source}")]
    Path { index: usize, source: PathError },
    #[error("entry {index} duplicates a path")]
    DuplicatePath { index: usize },
    #[error("entry {index} precedes a parent directory")]
    ParentOrder { index: usize },
    #[error("entry {index} has a file as a parent")]
    FileParent { index: usize },
    #[error("entry {index} has an unspecified or unknown kind")]
    Kind { index: usize },
    #[error("entry {index} has an invalid SHA-256 length")]
    Digest { index: usize },
    #[error("directory entry {index} has nonzero size")]
    DirectorySize { index: usize },
    #[error("directory entry {index} has a digest")]
    DirectoryDigest { index: usize },
    #[error("file size sum overflows at entry {index}")]
    SizeOverflow { index: usize },
    #[error("file size sum does not match total size")]
    TotalSize,
}

/// File metadata that cannot represent an unspecified kind or malformed digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    File { size: u64, sha256: [u8; 32] },
    Directory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: ManifestPath,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidManifest {
    entries: Vec<Entry>,
}

impl ValidManifest {
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

impl TryFrom<&FileManifest> for ValidManifest {
    type Error = ManifestError;

    fn try_from(manifest: &FileManifest) -> Result<Self, Self::Error> {
        if manifest.entries.len() > limits::MANIFEST_ENTRIES_MAX {
            return Err(ManifestError::TooManyEntries);
        }
        let mut entries = Vec::with_capacity(manifest.entries.len());
        let mut paths = HashMap::with_capacity(manifest.entries.len());
        let mut total = 0_u64;
        for (index, entry) in manifest.entries.iter().enumerate() {
            let path = ManifestPath::try_from(entry.path.as_str())
                .map_err(|source| ManifestError::Path { index, source })?;
            let kind = validate_kind(entry, index)?;
            if let EntryKind::File { size, .. } = kind {
                total = total
                    .checked_add(size)
                    .ok_or(ManifestError::SizeOverflow { index })?;
            }
            if paths.insert(path.as_str().to_lowercase(), index).is_some() {
                return Err(ManifestError::DuplicatePath { index });
            }
            entries.push(Entry { path, kind });
        }
        validate_parents(&entries, &paths)?;
        if total != manifest.total_size {
            return Err(ManifestError::TotalSize);
        }
        Ok(Self { entries })
    }
}

fn validate_kind(
    entry: &a7n_proto::v1::FileEntry,
    index: usize,
) -> Result<EntryKind, ManifestError> {
    match FileKind::try_from(entry.kind) {
        Ok(FileKind::File) => Ok(EntryKind::File {
            size: entry.size,
            sha256: entry
                .sha256
                .as_slice()
                .try_into()
                .map_err(|_| ManifestError::Digest { index })?,
        }),
        Ok(FileKind::Directory) => {
            if entry.size != 0 {
                return Err(ManifestError::DirectorySize { index });
            }
            if !entry.sha256.is_empty() {
                return Err(ManifestError::DirectoryDigest { index });
            }
            Ok(EntryKind::Directory)
        }
        _ => Err(ManifestError::Kind { index }),
    }
}

fn validate_parents(
    entries: &[Entry],
    paths: &HashMap<String, usize>,
) -> Result<(), ManifestError> {
    for (index, entry) in entries.iter().enumerate() {
        let folded = entry.path.as_str().to_lowercase();
        for (end, _) in folded.match_indices('/') {
            let Some(&parent) = paths.get(&folded[..end]) else {
                continue;
            };
            if entries[parent].kind != EntryKind::Directory {
                return Err(ManifestError::FileParent { index });
            }
            if parent > index {
                return Err(ManifestError::ParentOrder { index });
            }
        }
    }
    Ok(())
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NameError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("target name must be a single path segment")]
    Nested,
    #[error("target name search exhausted")]
    Exhausted,
}

/// Choose a free top-level name; retain nested segments inside that renamed root.
/// The caller must reserve the name and create files without following symlinks
/// or overwriting existing files; this check alone cannot prevent filesystem races.
///
/// # Errors
/// Rejects invalid or nested names, or exhaustion of the bounded name search.
pub fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> Result<String, NameError> {
    let path = ManifestPath::try_from(name)?;
    if path.segments().count() != 1 {
        return Err(NameError::Nested);
    }
    if !taken(name) {
        return Ok(name.to_owned());
    }
    let (stem, extension) = name
        .rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map_or((name, String::new()), |(stem, ext)| {
            (stem, format!(".{ext}"))
        });
    for number in 2..=NAME_ATTEMPTS_MAX {
        let candidate = format!("{stem} ({number}){extension}");
        if ManifestPath::try_from(candidate.as_str()).is_ok() && !taken(&candidate) {
            return Ok(candidate);
        }
    }
    Err(NameError::Exhausted)
}

/// Length measured by the daemon after re-hashing a partial file on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Partial {
    pub file_index: u32,
    pub len: u64,
}

/// Return plausible nonzero offsets; omitted entries restart at zero.
/// A prefix hash cannot be checked against a full-file manifest hash, so final
/// per-file SHA-256 verification remains mandatory even after resuming.
#[must_use]
pub fn resume_plan(manifest: &ValidManifest, partial: &[Partial]) -> Vec<FileResume> {
    partial
        .iter()
        .filter_map(|part| {
            let index = usize::try_from(part.file_index).ok()?;
            let EntryKind::File { size, .. } = manifest.entries.get(index)?.kind else {
                return None;
            };
            (part.len > 0 && part.len < size).then_some(FileResume {
                file_index: part.file_index,
                offset: part.len,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
