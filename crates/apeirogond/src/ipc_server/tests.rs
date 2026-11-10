// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn error_response_daemon_failures_maps_safe_categories() {
    // Arrange
    let cases = [
        (
            DaemonError::Layout(a7n_core::layout::LayoutError::Overlap),
            ErrorKind::InvalidLayout,
        ),
        (DaemonError::PairingBusy, ErrorKind::Busy),
        (DaemonError::NotPaired, ErrorKind::NotPaired),
        (DaemonError::NoSpace, ErrorKind::NoSpace),
        (
            DaemonError::Pairing(crate::pairing::PairingError::Refused),
            ErrorKind::PairingRefused,
        ),
        (DaemonError::TrustPoisoned, ErrorKind::Internal),
        (
            DaemonError::Pins(crate::pin_store::PinStoreError::BadHex),
            ErrorKind::Internal,
        ),
        (
            DaemonError::Tls(crate::tls::TlsError::Rustls(rustls::Error::General(
                "TLS configuration failed".into(),
            ))),
            ErrorKind::Internal,
        ),
    ];

    for (error, expected) in cases {
        // Act
        let response = error_response(&error);

        // Assert
        let Response::Error { kind, message } = response else {
            panic!("expected an error")
        };
        assert_eq!(kind, expected);
        assert_eq!(
            message,
            if expected == ErrorKind::PairingRefused {
                "pairing refused".into()
            } else {
                error.to_string()
            }
        );
    }
}
