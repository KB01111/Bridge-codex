use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use pretty_assertions::assert_eq;

use super::DesktopAgent;
use super::validate_coordinates;
use super::validate_desktop_text;

#[test]
fn desktop_coordinates_must_be_inside_the_main_display() {
    assert_eq!(validate_coordinates(0, 0, (1920, 1080)), Ok(()));
    assert_eq!(
        validate_coordinates(1920, 100, (1920, 1080)),
        Err("desktop click (1920, 100) is outside the 1920x1080 main display".to_string())
    );
    assert_eq!(
        validate_coordinates(0, 0, (0, 1080)),
        Err("desktop display reported invalid dimensions".to_string())
    );
}

#[test]
fn desktop_text_is_non_empty_and_bounded() {
    assert_eq!(
        validate_desktop_text(""),
        Err("desktop text cannot be empty".to_string())
    );
    assert_eq!(validate_desktop_text("hello"), Ok(()));
    assert!(validate_desktop_text(&"x".repeat(32 * 1024 + 1)).is_err());
}

#[test]
fn desktop_input_fails_closed_before_work_mode_is_enabled() {
    let agent = DesktopAgent {
        controller: Mutex::new(None),
        initialization_error: None,
        enabled: AtomicBool::new(false),
    };

    let result = agent.with_input_controller(|_| Ok(()));

    assert_eq!(result, Err("desktop Work Mode is disabled".to_string()));
}
