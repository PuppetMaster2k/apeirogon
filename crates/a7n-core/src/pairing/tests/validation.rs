// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn confirmation_fixed_transcript_matches_independent_key_schedule() {
    // Arrange: independently calculated with Python hashlib/hmac, RFC 9382 sections 3-4.
    let expected_requester = [
        69, 28, 45, 9, 98, 131, 20, 166, 79, 167, 32, 243, 53, 46, 94, 175, 13, 39, 148, 172, 23,
        191, 200, 244, 32, 167, 137, 160, 131, 95, 201, 90,
    ];
    let expected_host = [
        116, 170, 219, 70, 164, 165, 234, 137, 142, 120, 90, 83, 165, 194, 200, 232, 235, 101, 87,
        83, 185, 226, 152, 238, 125, 83, 13, 166, 167, 68, 17, 135,
    ];
    let expected_stored = [
        162, 21, 56, 144, 57, 109, 48, 72, 145, 112, 133, 81, 137, 250, 131, 25, 39, 102, 156, 236,
        28, 182, 106, 104, 177, 136, 124, 67, 228, 73, 93, 99,
    ];

    // Act
    let confirmation = crypto::Confirmation::new(
        (&[1; 32], &[2; 32]),
        (&[3; 32], &[4; 32]),
        curve25519_dalek::constants::ED25519_BASEPOINT_POINT,
        &curve25519_dalek::scalar::Scalar::ONE,
        &[5; 32],
    )
    .unwrap();

    // Assert
    assert_eq!(confirmation.tag(false, false), expected_requester);
    assert_eq!(confirmation.tag(true, false), expected_host);
    assert_eq!(confirmation.tag(true, true), expected_stored);
}
