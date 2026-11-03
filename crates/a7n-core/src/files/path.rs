// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::limits;
use unicode_normalization::is_nfc;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("empty path")]
    Empty,
    #[error("absolute path")]
    Absolute,
    #[error("backslash in path")]
    Backslash,
    #[error("drive prefix in path")]
    DrivePrefix,
    #[error("path exceeds byte limit")]
    TooLong,
    #[error("path is not NFC")]
    NotNfc,
    #[error("empty segment")]
    EmptySegment,
    #[error("dot or parent segment")]
    DotSegment,
    #[error("control character in segment")]
    Control,
    #[error("reserved device name")]
    Reserved,
    #[error("segment ends with a dot or space")]
    TrailingDotOrSpace,
    #[error("forbidden character in segment")]
    ForbiddenCharacter,
    #[error("segment exceeds byte limit")]
    SegmentTooLong,
}

/// A portable relative NFC path. Constructed only through validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestPath(String);

impl ManifestPath {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/')
    }
}

impl TryFrom<&str> for ManifestPath {
    type Error = PathError;

    fn try_from(path: &str) -> Result<Self, Self::Error> {
        if path.is_empty() {
            return Err(PathError::Empty);
        }
        if path.len() > limits::MANIFEST_PATH_MAX {
            return Err(PathError::TooLong);
        }
        if path.starts_with('/') {
            return Err(PathError::Absolute);
        }
        if path.contains('\\') {
            return Err(PathError::Backslash);
        }
        if path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic() {
            return Err(PathError::DrivePrefix);
        }
        if !is_nfc(path) {
            return Err(PathError::NotNfc);
        }
        for segment in path.split('/') {
            validate_segment(segment)?;
        }
        Ok(Self(path.to_owned()))
    }
}

fn validate_segment(segment: &str) -> Result<(), PathError> {
    if segment.is_empty() {
        return Err(PathError::EmptySegment);
    }
    if segment.len() > limits::MANIFEST_SEGMENT_MAX {
        return Err(PathError::SegmentTooLong);
    }
    if matches!(segment, "." | "..") {
        return Err(PathError::DotSegment);
    }
    if segment.chars().any(char::is_control) {
        return Err(PathError::Control);
    }
    if segment.ends_with(['.', ' ']) {
        return Err(PathError::TrailingDotOrSpace);
    }
    if segment.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Err(PathError::ForbiddenCharacter);
    }
    if reserved_name(segment) {
        return Err(PathError::Reserved);
    }
    Ok(())
}

fn reserved_name(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or_default().to_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    stem.strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(
                suffix,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}
