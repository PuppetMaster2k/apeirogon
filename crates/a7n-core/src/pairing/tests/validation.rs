// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn parse_invalid_codes_refuses_non_ascii_or_wrong_length() {
    // Arrange
    let inputs = ["12345", "1234567", " 12345", "１２３４５６", "12345a", ""];
    for input in inputs {
        // Act
        let result = PairingCode::parse(input);

        // Assert
        assert!(result.is_err());
    }
}

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

#[test]
fn generate_random_code_preserves_six_ascii_digits_and_leading_zeroes() {
    // Arrange
    let mut rng = TestRng(42);

    // Act
    let generated = PairingCode::generate(&mut rng);
    let leading_zeroes = PairingCode::parse("000001").unwrap();

    // Assert
    assert_eq!(generated.to_string().len(), limits::PAIRING_CODE_LENGTH);
    assert!(
        generated
            .to_string()
            .bytes()
            .all(|byte| byte.is_ascii_digit())
    );
    assert_eq!(leading_zeroes.to_string(), "000001");
    assert!(!format!("{leading_zeroes:?}").contains("000001"));
}

#[test]
fn done_without_verified_answer_refuses_pairing() {
    // Arrange
    let requester = requester(code());

    // Act
    let result = requester.done(PairDone {
        error: PairError::None.into(),
        tag: vec![0; 32],
    });

    // Assert
    assert!(matches!(result, Err(PairError::Refused)));
}

#[test]
fn answer_malformed_fields_refuses_without_retry() {
    // Arrange
    let now = Instant::now();
    for field in ["share", "tag", "info", "error"] {
        let mut host = host(now);
        let mut requester = requester(code());
        let good = answer(&mut host, &requester, now);
        let mut bad = good.clone();
        match field {
            "share" => bad.share.clear(),
            "tag" => bad.tag.clear(),
            "info" => bad.info = None,
            _ => bad.error = PairError::Refused.into(),
        }

        // Act
        let result = requester.answer(bad, pin(2), [3; 32]);
        let retry = requester.answer(good, pin(2), [3; 32]);

        // Assert
        assert_eq!(result, Err(PairError::Refused));
        assert_eq!(retry, Err(PairError::Refused));
    }
}

#[test]
fn start_repeated_during_exchange_discards_original_pending_proof() {
    // Arrange
    let now = Instant::now();
    let mut host = host(now);
    let mut requester = requester(code());
    let answer = answer(&mut host, &requester, now);
    let confirm = requester.answer(answer, pin(2), [3; 32]).unwrap();

    // Act
    let retry = host.start(
        requester.start(),
        pin(1),
        [3; 32],
        info("host"),
        now,
        &mut TestRng(2),
    );
    let original = host.confirm(confirm, now);

    // Assert
    assert_eq!(retry, Err(PairError::NotPairing));
    assert!(matches!(original, Err(PairError::Refused)));
}
