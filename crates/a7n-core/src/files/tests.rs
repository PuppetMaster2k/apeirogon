// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_proto::v1::FileEntry;

use super::*;

fn file(path: &str, size: u64) -> FileEntry {
    FileEntry {
        path: path.to_owned(),
        kind: FileKind::File.into(),
        size,
        sha256: vec![7; 32],
        ..Default::default()
    }
}

fn directory(path: &str) -> FileEntry {
    FileEntry {
        path: path.to_owned(),
        kind: FileKind::Directory.into(),
        ..Default::default()
    }
}

#[test]
fn manifest_path_unsafe_paths_are_rejected() {
    // Arrange
    let cases = [
        ("", PathError::Empty),
        ("/root", PathError::Absolute),
        ("a\\b", PathError::Backslash),
        ("C:relative", PathError::DrivePrefix),
        ("a//b", PathError::EmptySegment),
        ("a/", PathError::EmptySegment),
        ("a/./b", PathError::DotSegment),
        ("a/../b", PathError::DotSegment),
        ("a/\0b", PathError::Control),
        ("a/\u{85}", PathError::Control),
        ("e\u{301}.txt", PathError::NotNfc),
        ("a/coN.txt", PathError::Reserved),
        ("prn", PathError::Reserved),
        ("Aux", PathError::Reserved),
        ("NUL.txt", PathError::Reserved),
        ("com1", PathError::Reserved),
        ("COM9.txt", PathError::Reserved),
        ("lpt1", PathError::Reserved),
        ("LPT9.txt", PathError::Reserved),
        ("COM¹.txt", PathError::Reserved),
        ("com²", PathError::Reserved),
        ("Com³", PathError::Reserved),
        ("LPT¹", PathError::Reserved),
        ("lpt².txt", PathError::Reserved),
        ("Lpt³", PathError::Reserved),
        ("a./b", PathError::TrailingDotOrSpace),
        ("a /b", PathError::TrailingDotOrSpace),
        ("a<b", PathError::ForbiddenCharacter),
        ("a>b", PathError::ForbiddenCharacter),
        ("ab:c", PathError::ForbiddenCharacter),
        ("a\"b", PathError::ForbiddenCharacter),
        ("a|b", PathError::ForbiddenCharacter),
        ("a?b", PathError::ForbiddenCharacter),
        ("a*b", PathError::ForbiddenCharacter),
    ];
    for (path, error) in cases {
        // Act
        let result = ManifestPath::try_from(path);

        // Assert
        assert_eq!(result, Err(error), "{path:?}");
    }
}

#[test]
fn manifest_path_byte_limits_are_enforced() {
    // Arrange
    let longest_segment = "a".repeat(limits::MANIFEST_SEGMENT_MAX);
    let segment_count = limits::MANIFEST_PATH_MAX / (limits::MANIFEST_SEGMENT_MAX + 1);
    let longest_path = std::iter::repeat_n(longest_segment.as_str(), segment_count)
        .collect::<Vec<_>>()
        .join("/");
    let max_path = std::iter::repeat_n(longest_segment.as_str(), segment_count - 1)
        .collect::<Vec<_>>()
        .join("/")
        + "/"
        + &"b".repeat(limits::MANIFEST_SEGMENT_MAX - 1)
        + "/c";
    let cases = [
        (longest_segment.clone(), None),
        (longest_segment + "a", Some(PathError::SegmentTooLong)),
        ("é".repeat(128), Some(PathError::SegmentTooLong)),
        (longest_path, None),
        (max_path, None),
        (
            "a/".repeat(limits::MANIFEST_PATH_MAX / 2) + "a",
            Some(PathError::TooLong),
        ),
    ];
    for (path, error) in cases {
        // Act
        let result = ManifestPath::try_from(path.as_str());

        // Assert
        assert_eq!(result.err(), error);
    }
}

#[test]
fn valid_manifest_invalid_entries_are_rejected() {
    // Arrange
    let mut bad_digest = file("a", 1);
    bad_digest.sha256.pop();
    let mut bad_directory_size = directory("a");
    bad_directory_size.size = 1;
    let mut bad_directory_digest = directory("a");
    bad_directory_digest.sha256 = vec![7; 32];
    let mut unspecified = file("a", 1);
    unspecified.kind = FileKind::Unspecified.into();
    let mut unknown = file("a", 1);
    unknown.kind = 99;
    let cases = [
        (
            vec![file("A", 1), file("a", 1)],
            2,
            ManifestError::DuplicatePath { index: 1 },
        ),
        (
            vec![file("É", 1), file("é", 1)],
            2,
            ManifestError::DuplicatePath { index: 1 },
        ),
        (
            vec![file("a/b/c", 1), directory("A")],
            1,
            ManifestError::ParentOrder { index: 0 },
        ),
        (
            vec![directory("a/b"), directory("a")],
            0,
            ManifestError::ParentOrder { index: 0 },
        ),
        (
            vec![file("a", 1), file("A/b", 1)],
            2,
            ManifestError::FileParent { index: 1 },
        ),
        (vec![bad_digest], 1, ManifestError::Digest { index: 0 }),
        (
            vec![bad_directory_size],
            0,
            ManifestError::DirectorySize { index: 0 },
        ),
        (
            vec![bad_directory_digest],
            0,
            ManifestError::DirectoryDigest { index: 0 },
        ),
        (vec![unspecified], 1, ManifestError::Kind { index: 0 }),
        (vec![unknown], 1, ManifestError::Kind { index: 0 }),
        (vec![file("a", 1)], 2, ManifestError::TotalSize),
        (
            vec![file("a", u64::MAX), file("b", 1)],
            0,
            ManifestError::SizeOverflow { index: 1 },
        ),
        (
            vec![file("../secret", 1)],
            1,
            ManifestError::Path {
                index: 0,
                source: PathError::DotSegment,
            },
        ),
    ];
    for (entries, total_size, error) in cases {
        let manifest = FileManifest {
            entries,
            total_size,
        };

        // Act
        let result = ValidManifest::try_from(&manifest);

        // Assert
        assert_eq!(result, Err(error));
    }
}

#[test]
fn valid_manifest_entry_limit_is_enforced() {
    // Arrange
    let entries: Vec<_> = (0..limits::MANIFEST_ENTRIES_MAX)
        .map(|index| directory(&format!("dir{index}")))
        .collect();
    for extra in [false, true] {
        let mut manifest = FileManifest {
            entries: entries.clone(),
            total_size: 0,
        };
        if extra {
            manifest.entries.push(directory("extra"));
        }

        // Act
        let result = ValidManifest::try_from(&manifest);

        // Assert
        assert_eq!(result.err(), extra.then_some(ManifestError::TooManyEntries));
    }
}

#[test]
fn unique_name_collisions_select_first_free_name_or_reject_invalid() {
    // Arrange: `taken` of None means every candidate collides
    let cases = [
        ("report.txt".into(), Some(vec![]), Ok("report.txt")),
        (
            "report.txt".into(),
            Some(vec!["report.txt"]),
            Ok("report (2).txt"),
        ),
        (
            "report.txt".into(),
            Some(vec!["report.txt", "report (2).txt", "report (3).txt"]),
            Ok("report (4).txt"),
        ),
        (".bashrc".into(), Some(vec![".bashrc"]), Ok(".bashrc (2)")),
        (
            "archive.tar.gz".into(),
            Some(vec!["archive.tar.gz"]),
            Ok("archive.tar (2).gz"),
        ),
        ("name".into(), None, Err(NameError::Exhausted)),
        ("a/b".into(), None, Err(NameError::Nested)),
        (
            "../a".into(),
            None,
            Err(NameError::Path(PathError::DotSegment)),
        ),
        (
            "a".repeat(limits::MANIFEST_SEGMENT_MAX),
            None,
            Err(NameError::Exhausted),
        ),
    ];
    for (name, taken, expected) in cases {
        // Act
        let result = unique_name(&name, |candidate| {
            taken
                .as_ref()
                .is_none_or(|taken| taken.contains(&candidate))
        });

        // Assert
        assert_eq!(result, expected.map(str::to_owned));
    }
}

#[test]
fn resume_plan_partial_lengths_select_only_plausible_offsets() {
    // Arrange
    let manifest = ValidManifest::try_from(&FileManifest {
        entries: vec![
            directory("folder"),
            file("folder/file", 10),
            file("empty", 0),
        ],
        total_size: 10,
    })
    .unwrap();
    let cases = [
        (1, 4, Some(4)),
        (1, 10, None),
        (1, 11, None),
        (1, 0, None),
        (0, 1, None),
        (2, 0, None),
        (u32::MAX, 1, None),
    ];
    for (file_index, len, expected) in cases {
        // Act
        let result = resume_plan(&manifest, &[Partial { file_index, len }]);

        // Assert
        assert_eq!(
            result,
            expected
                .map(|offset| FileResume { file_index, offset })
                .into_iter()
                .collect::<Vec<_>>()
        );
    }
}
