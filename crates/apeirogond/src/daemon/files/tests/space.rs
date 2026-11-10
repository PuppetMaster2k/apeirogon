// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::atomic::{AtomicU64, Ordering};

use super::super::receive::{Reservation, check_space};
use super::*;

#[test]
fn check_space_free_bytes_cases_refuse_or_allow_receive() {
    // Arrange
    const MARGIN: u64 = 256 * 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    let cases = [
        // (total, resumed, free bytes or lookup failure, expected)
        ("insufficient", 1, 0, Some(MARGIN), Err(Failure::NoSpace)),
        ("resumed remainder", GIB, GIB - 7, Some(MARGIN + 7), Ok(())),
        ("lookup failure", 1, 0, None, Ok(())),
        (
            "overflow",
            u64::MAX,
            0,
            Some(u64::MAX),
            Err(Failure::NoSpace),
        ),
    ];
    let directory = std::path::Path::new("receive");

    for (name, total, resumed, free, expected) in cases {
        // Act
        let result = check_space(directory, total, resumed, |path| {
            assert_eq!(path, directory);
            free.ok_or_else(|| std::io::ErrorKind::Other.into())
        });

        // Assert
        assert_eq!(result, expected, "{name}");
    }
}

#[test]
fn reservation_parallel_receives_see_each_other_until_dropped() {
    // Arrange
    let reserved = AtomicU64::new(0);
    let first = Reservation::new(&reserved, 300);

    // Act
    let second = Reservation::new(&reserved, 200);
    let others = second.others;
    drop(first);
    drop(second);

    // Assert
    assert_eq!(others, 300);
    assert_eq!(reserved.load(Ordering::Relaxed), 0);
}
