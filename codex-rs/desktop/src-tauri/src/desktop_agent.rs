use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use enigo::Button;
use enigo::Coordinate;
use enigo::Direction;
use enigo::Enigo;
use enigo::Keyboard;
use enigo::Mouse;
use enigo::Settings;
use serde::Serialize;
use tauri::State;

const MAX_DESKTOP_TEXT_BYTES: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopStatus {
    pub available: bool,
    pub enabled: bool,
    pub platform: &'static str,
    pub display_size: Option<(i32, i32)>,
    pub error: Option<String>,
}

pub struct DesktopAgent {
    controller: Mutex<Option<Enigo>>,
    initialization_error: Option<String>,
    enabled: AtomicBool,
}

impl DesktopAgent {
    pub fn new() -> Self {
        match Enigo::new(&Settings::default()) {
            Ok(controller) => Self {
                controller: Mutex::new(Some(controller)),
                initialization_error: None,
                enabled: AtomicBool::new(false),
            },
            Err(error) => Self {
                controller: Mutex::new(None),
                initialization_error: Some(error.to_string()),
                enabled: AtomicBool::new(false),
            },
        }
    }

    fn with_controller<T>(
        &self,
        operation: impl FnOnce(&mut Enigo) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut controller = self
            .controller
            .lock()
            .map_err(|_| "desktop controller lock was poisoned".to_string())?;
        let controller = controller.as_mut().ok_or_else(|| {
            self.initialization_error
                .clone()
                .unwrap_or_else(|| "desktop control is unavailable".to_string())
        })?;
        operation(controller)
    }

    fn with_input_controller<T>(
        &self,
        operation: impl FnOnce(&mut Enigo) -> Result<T, String>,
    ) -> Result<T, String> {
        if !self.enabled.load(Ordering::Acquire) {
            return Err("desktop Work Mode is disabled".to_string());
        }
        self.with_controller(operation)
    }

    fn set_enabled(&self, enabled: bool) -> Result<DesktopStatus, String> {
        if enabled {
            self.with_controller(|controller| {
                controller.main_display().map_err(|error| error.to_string())
            })?;
        }
        self.enabled.store(enabled, Ordering::Release);
        Ok(self.status())
    }

    pub fn status(&self) -> DesktopStatus {
        match self.with_controller(|controller| {
            controller.main_display().map_err(|error| error.to_string())
        }) {
            Ok(display_size) => DesktopStatus {
                available: true,
                enabled: self.enabled.load(Ordering::Acquire),
                platform: std::env::consts::OS,
                display_size: Some(display_size),
                error: None,
            },
            Err(error) => DesktopStatus {
                available: false,
                enabled: false,
                platform: std::env::consts::OS,
                display_size: None,
                error: Some(error),
            },
        }
    }
}

#[tauri::command]
pub fn desktop_status(state: State<'_, DesktopAgent>) -> DesktopStatus {
    state.status()
}

#[tauri::command]
pub fn enable_desktop_work_mode(state: State<'_, DesktopAgent>) -> Result<DesktopStatus, String> {
    state.set_enabled(true)
}

#[tauri::command]
pub fn disable_desktop_work_mode(state: State<'_, DesktopAgent>) -> Result<DesktopStatus, String> {
    state.set_enabled(false)
}

#[tauri::command]
pub fn desktop_click(state: State<'_, DesktopAgent>, x: i32, y: i32) -> Result<(), String> {
    state.with_input_controller(|controller| {
        let display_size = controller
            .main_display()
            .map_err(|error| error.to_string())?;
        validate_coordinates(x, y, display_size)?;
        controller
            .move_mouse(x, y, Coordinate::Abs)
            .map_err(|error| error.to_string())?;
        controller
            .button(Button::Left, Direction::Click)
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub fn desktop_type(state: State<'_, DesktopAgent>, text: String) -> Result<(), String> {
    validate_desktop_text(&text)?;
    state.with_input_controller(|controller| {
        controller.text(&text).map_err(|error| error.to_string())
    })
}

fn validate_coordinates(x: i32, y: i32, display_size: (i32, i32)) -> Result<(), String> {
    let (width, height) = display_size;
    if width <= 0 || height <= 0 {
        return Err("desktop display reported invalid dimensions".to_string());
    }
    if !(0..width).contains(&x) || !(0..height).contains(&y) {
        return Err(format!(
            "desktop click ({x}, {y}) is outside the {width}x{height} main display"
        ));
    }
    Ok(())
}

fn validate_desktop_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Err("desktop text cannot be empty".to_string());
    }
    if text.len() > MAX_DESKTOP_TEXT_BYTES {
        return Err(format!(
            "desktop text exceeds the {MAX_DESKTOP_TEXT_BYTES}-byte limit"
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "desktop_agent_tests.rs"]
mod tests;
