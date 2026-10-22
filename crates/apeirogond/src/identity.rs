// SPDX-License-Identifier: GPL-3.0-or-later

//! The device's key, self-signed certificate, machine id and pin
//! (PROTOCOL.md 3.1).

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use a7n_platform::fs::write_user_only;
use a7n_proto::{MachineId, Pin, ProtoError};
use rcgen::{
    CertificateParams, DistinguishedName, DnType, KeyPair, PublicKeyData, SerialNumber,
    date_time_ymd,
};
use sha2::{Digest, Sha256};
use x509_parser::prelude::{FromDer, X509Certificate};

const KEY_FILE: &str = "identity.key";
const CERT_FILE: &str = "identity.crt";
const SUBJECT_COMMON_NAME: &str = "apeirogon";
/// Fixed so that re-issuing from the same key gives the same bytes.
const CERT_SERIAL: u64 = 1;
/// Validity of about 100 years; expiry is not a security mechanism here
/// (PROTOCOL.md 3.1).
const NOT_BEFORE: (i32, u8, u8) = (2026, 1, 1);
const NOT_AFTER: (i32, u8, u8) = (2126, 1, 1);

/// Why loading or creating the identity failed.
#[derive(Debug, thiserror::Error)]
pub(crate) enum IdentityError {
    #[error("identity file error: {0}")]
    Io(#[from] io::Error),
    #[error("key or certificate generation failed: {0}")]
    Rcgen(#[from] rcgen::Error),
    #[error("certificate is not valid X.509: {0}")]
    X509(String),
    #[error("stored certificate does not belong to the stored key")]
    CertKeyMismatch,
    #[error(transparent)]
    Proto(#[from] ProtoError),
}

/// The device's own identity. The private key never appears in `Debug`.
pub(crate) struct Identity {
    pub(crate) key: KeyPair,
    pub(crate) cert_der: Vec<u8>,
    pub(crate) machine_id: MachineId,
    pub(crate) pin: Pin,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Identity")
            .field("machine_id", &self.machine_id)
            .field("pin", &self.pin)
            .finish_non_exhaustive()
    }
}

fn machine_id_of_spki(spki_der: &[u8]) -> Result<MachineId, IdentityError> {
    let digest = Sha256::digest(spki_der);
    Ok(MachineId::try_from(&digest[..MachineId::LEN])?)
}

/// Machine id of a certificate: first 16 bytes of SHA-256 of its
/// `SubjectPublicKeyInfo` DER.
pub(crate) fn machine_id_of(cert_der: &[u8]) -> Result<MachineId, IdentityError> {
    let (_, cert) = X509Certificate::from_der(cert_der)
        .map_err(|error| IdentityError::X509(error.to_string()))?;
    machine_id_of_spki(cert.tbs_certificate.subject_pki.raw)
}

/// Pin of a certificate: SHA-256 of its DER.
pub(crate) fn pin_of(cert_der: &[u8]) -> Result<Pin, IdentityError> {
    Ok(Pin::try_from(&Sha256::digest(cert_der)[..])?)
}

/// Issues the deterministic self-signed certificate for `key`.
pub(crate) fn issue_certificate(key: &KeyPair) -> Result<Vec<u8>, IdentityError> {
    let mut params = CertificateParams::new(Vec::new())?;
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, SUBJECT_COMMON_NAME);
    params.distinguished_name = name;
    params.serial_number = Some(SerialNumber::from(CERT_SERIAL));
    params.not_before = date_time_ymd(NOT_BEFORE.0, NOT_BEFORE.1, NOT_BEFORE.2);
    params.not_after = date_time_ymd(NOT_AFTER.0, NOT_AFTER.1, NOT_AFTER.2);
    Ok(params.self_signed(key)?.der().to_vec())
}

/// Loads the identity from `dir`, creating key and certificate on first start.
///
/// The certificate is stored too: a pin is the hash of its exact bytes, and a
/// loaded P-256 key signs non-deterministically.
pub(crate) fn load_or_create(dir: &Path) -> Result<Identity, IdentityError> {
    let key = load_or_create_key(&dir.join(KEY_FILE))?;
    let cert_der = load_or_issue_certificate(&dir.join(CERT_FILE), &key)?;
    let machine_id = machine_id_of(&cert_der)?;
    if machine_id != machine_id_of_spki(&key.subject_public_key_info())? {
        return Err(IdentityError::CertKeyMismatch);
    }
    let pin = pin_of(&cert_der)?;
    Ok(Identity {
        key,
        cert_der,
        machine_id,
        pin,
    })
}

fn load_or_create_key(path: &Path) -> Result<KeyPair, IdentityError> {
    match fs::read_to_string(path) {
        Ok(pem) => Ok(KeyPair::from_pem(&pem)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let key = KeyPair::generate_for(&rcgen::PKCS_ED25519)?;
            write_user_only(path, key.serialize_pem().as_bytes())?;
            Ok(key)
        }
        Err(error) => Err(error.into()),
    }
}

fn load_or_issue_certificate(path: &Path, key: &KeyPair) -> Result<Vec<u8>, IdentityError> {
    match fs::read(path) {
        Ok(der) => Ok(der),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let der = issue_certificate(key)?;
            write_user_only(path, &der)?;
            Ok(der)
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_certificate_same_key_reissued_keeps_machine_id() {
        // Arrange
        let key = KeyPair::generate_for(&rcgen::PKCS_ED25519).unwrap();
        let first = issue_certificate(&key).unwrap();

        // Act
        let second = issue_certificate(&key).unwrap();

        // Assert
        assert_eq!(
            machine_id_of(&first).unwrap(),
            machine_id_of(&second).unwrap()
        );
    }

    #[test]
    fn load_or_create_second_start_returns_same_identity() {
        // Arrange
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create(dir.path()).unwrap();

        // Act
        let second = load_or_create(dir.path()).unwrap();

        // Assert
        assert_eq!(first.machine_id, second.machine_id);
        assert_eq!(first.pin, second.pin);
    }
}
