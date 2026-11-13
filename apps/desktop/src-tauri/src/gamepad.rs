//! Controllers (Xbox, `PlayStation`, Switch, Steam Deck …) for the TV mode.
//!
//! The core reads them itself (`gilrs`): the webview's Gamepad API is often
//! switched off in `WebKitGTK`. Buttons become actions (`gamepad-input`), face
//! buttons by position (bottom = confirm, right = back), so the UI only has
//! to show the right label for the controller (`ControllerKind`). D-pad and
//! left stick repeat while held. While a game runs, or another window has the
//! focus, Cove ignores the controller; holding the Guide/PS/Home button brings
//! Cove to the front.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gilrs::{Axis, Button, EventType, Gilrs};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::models::{ControllerKind, GamepadInfo, GamepadInput, PadAction};

pub const INPUT_EVENT: &str = "gamepad-input";
pub const CHANGED_EVENT: &str = "gamepads-changed";

/// First repeat of a held direction, then every `REPEAT_EVERY`.
const REPEAT_AFTER: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(110);
/// A stick counts as a direction beyond this.
const STICK_THRESHOLD: f32 = 0.6;
/// Holding the Guide button this long brings Cove to the front.
const GUIDE_HOLD: Duration = Duration::from_millis(700);
const POLL: Duration = Duration::from_millis(30);

const SONY: u16 = 0x054c;
const MICROSOFT: u16 = 0x045e;
const NINTENDO: u16 = 0x057e;
const VALVE: u16 = 0x28de;

/// Which labels a controller has, from its USB vendor and its name.
pub fn kind_of(vendor: Option<u16>, name: &str) -> ControllerKind {
    let name = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| name.contains(w));
    match vendor {
        Some(SONY) => ControllerKind::Playstation,
        Some(NINTENDO) => ControllerKind::Switch,
        Some(MICROSOFT) => ControllerKind::Xbox,
        Some(VALVE) => ControllerKind::Steam,
        _ if has(&[
            "dualsense",
            "dualshock",
            "playstation",
            "ps4",
            "ps5",
            "sony",
        ]) =>
        {
            ControllerKind::Playstation
        }
        _ if has(&["switch", "joy-con", "nintendo", "pro controller"]) => ControllerKind::Switch,
        _ if has(&["steam deck", "steam controller", "valve"]) => ControllerKind::Steam,
        _ if has(&["xbox", "x-box", "xinput", "microsoft"]) => ControllerKind::Xbox,
        _ => ControllerKind::Generic,
    }
}

/// What a button does (None = not used by Cove).
pub fn action_of(button: Button) -> Option<PadAction> {
    Some(match button {
        Button::DPadUp => PadAction::Up,
        Button::DPadDown => PadAction::Down,
        Button::DPadLeft => PadAction::Left,
        Button::DPadRight => PadAction::Right,
        Button::South => PadAction::Confirm,
        Button::East => PadAction::Back,
        Button::West => PadAction::Secondary,
        Button::North => PadAction::Tertiary,
        Button::LeftTrigger => PadAction::PrevTab,
        Button::RightTrigger => PadAction::NextTab,
        Button::Start => PadAction::Menu,
        Button::Select => PadAction::View,
        Button::Mode => PadAction::Home,
        _ => return None,
    })
}

fn is_direction(action: PadAction) -> bool {
    matches!(
        action,
        PadAction::Up | PadAction::Down | PadAction::Left | PadAction::Right
    )
}

/// The direction of a stick position (gilrs: Y up is positive).
pub fn stick_direction(x: f32, y: f32) -> Option<PadAction> {
    if x.abs() < STICK_THRESHOLD && y.abs() < STICK_THRESHOLD {
        return None;
    }
    Some(if x.abs() > y.abs() {
        if x > 0.0 {
            PadAction::Right
        } else {
            PadAction::Left
        }
    } else if y > 0.0 {
        PadAction::Up
    } else {
        PadAction::Down
    })
}

/// Repeats a held direction: once at once, after a pause again and again.
#[derive(Debug, Default)]
pub struct Repeater {
    held: Option<(PadAction, Instant, Option<Instant>)>,
}

impl Repeater {
    /// A direction was pressed (or the stick moved to one): fires at once.
    pub fn press(&mut self, action: PadAction, now: Instant) -> Option<PadAction> {
        if self.held.is_some_and(|(held, _, _)| held == action) {
            return None;
        }
        self.held = Some((action, now, None));
        Some(action)
    }

    pub fn release(&mut self, action: Option<PadAction>) {
        if action.is_none() || self.held.is_some_and(|(held, _, _)| Some(held) == action) {
            self.held = None;
        }
    }

    /// Called regularly: the repeats that are due.
    pub fn tick(&mut self, now: Instant) -> Option<PadAction> {
        let (action, since, last) = self.held?;
        let due = match last {
            None => now.duration_since(since) >= REPEAT_AFTER,
            Some(last) => now.duration_since(last) >= REPEAT_EVERY,
        };
        if due {
            self.held = Some((action, since, Some(now)));
            Some(action)
        } else {
            None
        }
    }
}

/// Connected controllers, for `gamepad_list`.
#[derive(Default)]
pub struct Gamepads {
    connected: Mutex<Vec<GamepadInfo>>,
}

impl Gamepads {
    pub fn list(&self) -> Vec<GamepadInfo> {
        self.connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn set(&self, list: Vec<GamepadInfo>) {
        *self
            .connected
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = list;
    }
}

fn snapshot(gilrs: &Gilrs) -> Vec<GamepadInfo> {
    gilrs
        .gamepads()
        .map(|(id, pad)| GamepadInfo {
            id: u32::try_from(usize::from(id)).unwrap_or(u32::MAX),
            name: pad.name().to_owned(),
            kind: kind_of(pad.vendor_id(), pad.name()),
        })
        .collect()
}

/// Whether Cove may react: its window has the focus and no game runs.
fn listening<R: Runtime>(app: &AppHandle<R>) -> bool {
    let focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    let game_running = app
        .try_state::<crate::state::AppState>()
        .is_some_and(|s| !s.launcher.running().is_empty());
    focused && !game_running
}

fn bring_to_front<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Reads the controllers on a thread of its own for as long as Cove runs.
pub fn watch<R: Runtime>(app: AppHandle<R>, gamepads: Arc<Gamepads>) {
    let spawned = std::thread::Builder::new()
        .name("cove-gamepads".into())
        .spawn(move || {
            let mut gilrs = match Gilrs::new() {
                Ok(gilrs) => gilrs,
                Err(error) => {
                    eprintln!("[gamepad] Controller können nicht gelesen werden: {error}");
                    return;
                }
            };
            gamepads.set(snapshot(&gilrs));
            let mut repeater = Repeater::default();
            let mut sticks: HashMap<gilrs::GamepadId, (f32, f32)> = HashMap::new();
            let mut guide_down: Option<Instant> = None;
            let mut kind = ControllerKind::Generic;
            loop {
                let now = Instant::now();
                let mut fired: Vec<PadAction> = Vec::new();
                if let Some(event) = gilrs.next_event_blocking(Some(POLL)) {
                    if let Some(pad) = gilrs.connected_gamepad(event.id) {
                        kind = kind_of(pad.vendor_id(), pad.name());
                    }
                    match event.event {
                        EventType::Connected | EventType::Disconnected => {
                            let list = snapshot(&gilrs);
                            gamepads.set(list.clone());
                            let _ = app.emit(CHANGED_EVENT, list);
                        }
                        EventType::ButtonPressed(Button::Mode, _) => guide_down = Some(now),
                        EventType::ButtonReleased(Button::Mode, _) => {
                            if guide_down.take().is_some() {
                                fired.push(PadAction::Home);
                            }
                        }
                        EventType::ButtonPressed(button, _) => match action_of(button) {
                            Some(action) if is_direction(action) => {
                                fired.extend(repeater.press(action, now));
                            }
                            Some(action) => fired.push(action),
                            None => {}
                        },
                        EventType::ButtonReleased(button, _) => {
                            if let Some(action) = action_of(button).filter(|a| is_direction(*a)) {
                                repeater.release(Some(action));
                            }
                        }
                        EventType::AxisChanged(
                            axis @ (Axis::LeftStickX | Axis::LeftStickY),
                            value,
                            _,
                        ) => {
                            let stick = sticks.entry(event.id).or_insert((0.0, 0.0));
                            if axis == Axis::LeftStickX {
                                stick.0 = value;
                            } else {
                                stick.1 = value;
                            }
                            match stick_direction(stick.0, stick.1) {
                                Some(action) => fired.extend(repeater.press(action, now)),
                                None => repeater.release(None),
                            }
                        }
                        _ => {}
                    }
                }
                fired.extend(repeater.tick(now));
                // Holding Guide brings Cove back, also during a game.
                if guide_down.is_some_and(|since| now.duration_since(since) >= GUIDE_HOLD) {
                    guide_down = None;
                    bring_to_front(&app);
                    fired.push(PadAction::Home);
                }
                if fired.is_empty() || !listening(&app) {
                    continue;
                }
                for action in fired {
                    let _ = app.emit(INPUT_EVENT, GamepadInput { action, kind });
                }
            }
        });
    if let Err(error) = spawned {
        eprintln!("[gamepad] {error}");
    }
}

/// The Steam Deck's gaming mode or another gamescope session: a TV setup.
pub fn tv_hint() -> bool {
    let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
    set("SteamDeck")
        || set("SteamGamepadUI")
        || set("GAMESCOPE_WAYLAND_DISPLAY")
        || std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.eq_ignore_ascii_case("gamescope"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controllers_are_told_apart() {
        assert_eq!(
            kind_of(Some(0x054c), "Wireless Controller"),
            ControllerKind::Playstation
        );
        assert_eq!(
            kind_of(Some(0x057e), "Pro Controller"),
            ControllerKind::Switch
        );
        assert_eq!(
            kind_of(Some(0x045e), "Xbox Series X Controller"),
            ControllerKind::Xbox
        );
        assert_eq!(kind_of(Some(0x28de), "Steam Deck"), ControllerKind::Steam);
        // Bluetooth or unusual drivers: the name decides.
        assert_eq!(
            kind_of(None, "DualSense Wireless Controller"),
            ControllerKind::Playstation
        );
        assert_eq!(
            kind_of(Some(0x1234), "Nintendo Switch Joy-Con (L/R)"),
            ControllerKind::Switch
        );
        assert_eq!(
            kind_of(Some(0x1234), "8BitDo Ultimate"),
            ControllerKind::Generic
        );
    }

    #[test]
    fn face_buttons_count_by_position() {
        assert_eq!(action_of(Button::South), Some(PadAction::Confirm));
        assert_eq!(action_of(Button::East), Some(PadAction::Back));
        assert_eq!(action_of(Button::LeftTrigger), Some(PadAction::PrevTab));
        assert_eq!(action_of(Button::RightTrigger2), None);
        assert_eq!(stick_direction(0.1, 0.9), Some(PadAction::Up));
        assert_eq!(stick_direction(-0.8, 0.3), Some(PadAction::Left));
        assert_eq!(stick_direction(0.2, -0.4), None);
    }

    #[test]
    fn held_directions_repeat_after_a_pause() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut repeater = Repeater::default();
        assert_eq!(
            repeater.press(PadAction::Down, at(0)),
            Some(PadAction::Down)
        );
        // The stick wobbling in the same direction doesn't fire again.
        assert_eq!(repeater.press(PadAction::Down, at(50)), None);
        assert_eq!(repeater.tick(at(300)), None);
        assert_eq!(repeater.tick(at(400)), Some(PadAction::Down));
        assert_eq!(repeater.tick(at(450)), None);
        assert_eq!(repeater.tick(at(510)), Some(PadAction::Down));
        repeater.release(Some(PadAction::Up));
        assert_eq!(repeater.tick(at(700)), Some(PadAction::Down));
        repeater.release(Some(PadAction::Down));
        assert_eq!(repeater.tick(at(1000)), None);
    }
}
