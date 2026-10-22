// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

#[test]
fn serialize_unpair_machine_id_uses_hex_string() {
    // Arrange
    let machine = MachineId::try_from([0xab; MachineId::LEN].as_slice()).unwrap();
    let request = Request::Unpair { machine };

    // Act
    let json = serde_json::to_value(request).unwrap();
    let decoded: Request = serde_json::from_value(json.clone()).unwrap();

    // Assert
    assert_eq!(
        json,
        serde_json::json!({"type":"unpair", "machine":"abababababababababababababababab"})
    );
    assert!(matches!(decoded, Request::Unpair { machine: id } if id == machine));
}

#[test]
fn deserialize_pair_join_invalid_codes_are_rejected() {
    // Arrange
    let codes = ["12345", "1234567", "abcdef", "１２３４５６"];
    for code in codes {
        let json = format!(r#"{{"type":"pair_join","addr":"127.0.0.1:42042","code":"{code}"}}"#);

        // Act
        let result = serde_json::from_str::<Request>(&json);

        // Assert
        assert!(result.is_err());
    }
}

#[test]
fn deserialize_pair_join_leading_zeroes_are_preserved_and_debug_redacted() {
    // Arrange
    let json = r#"{"type":"pair_join","addr":"127.0.0.1:42042","code":"001234"}"#;

    // Act
    let request: Request = serde_json::from_str(json).unwrap();

    // Assert
    assert!(!format!("{request:?}").contains("001234"));
    assert_eq!(serde_json::to_value(request).unwrap()["code"], "001234");
}
