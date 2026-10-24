// SPDX-License-Identifier: GPL-3.0-or-later

use a7n_ipc::{MachineId, PlacedMachine};

use super::*;

fn machine(id: u8, x: i32, y: i32) -> PlacedMachine {
    PlacedMachine {
        machine: MachineId::try_from([id; MachineId::LEN].as_slice()).unwrap(),
        name: format!("machine-{id}"),
        x,
        y,
        width: 1920,
        height: 1080,
    }
}

#[test]
fn handle_move_into_overlap_refuses_save() {
    // Arrange
    let mut editor = Editor::new(LayoutInfo {
        placed: vec![machine(1, 0, 0), machine(2, 1920, 0)],
        unplaced: Vec::new(),
        lost_edit: false,
    })
    .unwrap();

    // Act
    editor.handle(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    let action = editor.handle(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));

    // Assert
    assert_eq!(editor.placements[0].x, MOVE_STEP);
    assert!(validate(&editor.placements).is_err());
    assert!(matches!(action, Action::Continue));
}

#[test]
fn handle_save_valid_layout_emits_placements() {
    // Arrange
    let placed = machine(1, -100, 200);
    let mut editor = Editor::new(LayoutInfo {
        placed: vec![placed.clone()],
        unplaced: Vec::new(),
        lost_edit: true,
    })
    .unwrap();

    // Act
    let action = editor.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    // Assert
    let Action::Save(placements) = action else {
        panic!("valid layout must save")
    };
    assert_eq!(
        placements,
        vec![LayoutPlacement {
            machine: placed.machine,
            x: placed.x,
            y: placed.y,
            width: placed.width,
            height: placed.height,
        }]
    );
}

#[test]
fn handle_selection_and_fine_move_wraps_and_uses_ten_units() {
    // Arrange
    let mut editor = Editor::new(LayoutInfo {
        placed: vec![machine(1, 0, 0), machine(2, 1920, 0)],
        unplaced: Vec::new(),
        lost_edit: false,
    })
    .unwrap();

    // Act
    editor.handle(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    editor.handle(KeyEvent::new(KeyCode::Up, KeyModifiers::SHIFT));
    editor.handle(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));

    // Assert
    assert_eq!(editor.placements[1].y, -FINE_MOVE_STEP);
    assert_eq!(editor.selected, 0);
}
