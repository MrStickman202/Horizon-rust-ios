//! Hardware keyboards (an iPad's keyboard case, a Bluetooth keyboard): UIKit's presses as winit
//! key events with physical key codes, key up and modifiers. Added for Horizon; upstream 0.30
//! reports a hardware keyboard only as typed text through `UIKeyInput`.
use objc2::rc::Retained;
use objc2_foundation::{MainThreadMarker, NSSet};
use objc2_ui_kit::{UIKey, UIKeyModifierFlags, UIPress};

use super::app_state::{self, EventWrapper};
use crate::event::{ElementState, Event, KeyEvent, Modifiers, WindowEvent};
use crate::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey, PhysicalKey};
use crate::platform_impl::platform::DEVICE_ID;
use crate::platform_impl::KeyEventExtra;
use crate::window::WindowId as RootWindowId;

thread_local! {
    /// When a hardware key last typed text: UIKit may also hand the same character to the
    /// view's `insertText:` while it takes text, which [`typed_by_key_just_now`] lets it skip.
    static TYPED_AT: std::cell::Cell<Option<std::time::Instant>> = const { std::cell::Cell::new(None) };
}

/// Whether a hardware key typed text in the last moment (the view skips its `insertText:`).
pub(crate) fn typed_by_key_just_now() -> bool {
    TYPED_AT.with(|t| t.get()).is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(150))
}

/// The presses' keys as key events for `window_id`. Returns true when any press was a keyboard
/// key (the caller doesn't pass those on). The events carry the typed text.
pub(crate) fn handle_presses(
    mtm: MainThreadMarker,
    window_id: RootWindowId,
    presses: &NSSet<UIPress>,
    state: ElementState,
) -> bool {
    let mut events = Vec::new();
    for press in presses.iter() {
        let Some(key) = (unsafe { press.key(mtm) }) else {
            continue;
        };
        let flags = unsafe { key.modifierFlags() };
        events.push(EventWrapper::StaticEvent(Event::WindowEvent {
            window_id,
            event: WindowEvent::ModifiersChanged(Modifiers::from(modifiers(flags))),
        }));
        events.push(EventWrapper::StaticEvent(Event::WindowEvent {
            window_id,
            event: WindowEvent::KeyboardInput {
                device_id: DEVICE_ID,
                event: key_event(&key, state, true),
                is_synthetic: false,
            },
        }));
    }
    let handled = !events.is_empty();
    if handled {
        app_state::handle_nonuser_events(mtm, events);
    }
    handled
}

fn modifiers(flags: UIKeyModifierFlags) -> ModifiersState {
    let mut state = ModifiersState::empty();
    state.set(ModifiersState::SHIFT, flags.contains(UIKeyModifierFlags::UIKeyModifierShift));
    state.set(ModifiersState::CONTROL, flags.contains(UIKeyModifierFlags::UIKeyModifierControl));
    state.set(ModifiersState::ALT, flags.contains(UIKeyModifierFlags::UIKeyModifierAlternate));
    state.set(ModifiersState::SUPER, flags.contains(UIKeyModifierFlags::UIKeyModifierCommand));
    state
}

fn key_event(key: &Retained<UIKey>, state: ElementState, with_text: bool) -> KeyEvent {
    let usage = unsafe { key.keyCode() }.0 as u32;
    let code = key_code(usage);
    let characters = unsafe { key.characters() }.to_string();
    let named = code.and_then(named_key);
    let logical_key = match named {
        Some(named) => Key::Named(named),
        None => match characters.chars().next() {
            Some(c) if !c.is_control() => Key::Character(smol_str::SmolStr::new(&characters)),
            _ => Key::Unidentified(crate::keyboard::NativeKey::Unidentified),
        },
    };
    let text: Option<smol_str::SmolStr> = match (&logical_key, state) {
        (Key::Character(text), ElementState::Pressed) if with_text => Some(text.clone()),
        (Key::Named(NamedKey::Enter), ElementState::Pressed) if with_text => Some("\r".into()),
        _ => None,
    };
    if state == ElementState::Pressed {
        TYPED_AT.with(|t| t.set(Some(std::time::Instant::now())));
    }
    KeyEvent {
        physical_key: code.map_or(
            PhysicalKey::Unidentified(crate::keyboard::NativeKeyCode::Unidentified),
            PhysicalKey::Code,
        ),
        logical_key,
        text,
        location: code.map_or(KeyLocation::Standard, location),
        state,
        repeat: false,
        platform_specific: KeyEventExtra {},
    }
}

fn location(code: KeyCode) -> KeyLocation {
    use KeyCode::*;
    match code {
        ShiftLeft | ControlLeft | AltLeft | SuperLeft => KeyLocation::Left,
        ShiftRight | ControlRight | AltRight | SuperRight => KeyLocation::Right,
        Numpad0 | Numpad1 | Numpad2 | Numpad3 | Numpad4 | Numpad5 | Numpad6 | Numpad7
        | Numpad8 | Numpad9 | NumpadAdd | NumpadSubtract | NumpadMultiply | NumpadDivide
        | NumpadDecimal | NumpadEnter | NumpadEqual | NumLock => KeyLocation::Numpad,
        _ => KeyLocation::Standard,
    }
}

fn named_key(code: KeyCode) -> Option<NamedKey> {
    use KeyCode::*;
    Some(match code {
        Enter | NumpadEnter => NamedKey::Enter,
        Escape => NamedKey::Escape,
        Backspace => NamedKey::Backspace,
        Tab => NamedKey::Tab,
        Space => NamedKey::Space,
        ArrowUp => NamedKey::ArrowUp,
        ArrowDown => NamedKey::ArrowDown,
        ArrowLeft => NamedKey::ArrowLeft,
        ArrowRight => NamedKey::ArrowRight,
        Home => NamedKey::Home,
        End => NamedKey::End,
        PageUp => NamedKey::PageUp,
        PageDown => NamedKey::PageDown,
        Delete => NamedKey::Delete,
        Insert => NamedKey::Insert,
        ShiftLeft | ShiftRight => NamedKey::Shift,
        ControlLeft | ControlRight => NamedKey::Control,
        AltLeft | AltRight => NamedKey::Alt,
        SuperLeft | SuperRight => NamedKey::Super,
        CapsLock => NamedKey::CapsLock,
        F1 => NamedKey::F1,
        F2 => NamedKey::F2,
        F3 => NamedKey::F3,
        F4 => NamedKey::F4,
        F5 => NamedKey::F5,
        F6 => NamedKey::F6,
        F7 => NamedKey::F7,
        F8 => NamedKey::F8,
        F9 => NamedKey::F9,
        F10 => NamedKey::F10,
        F11 => NamedKey::F11,
        F12 => NamedKey::F12,
        _ => return None,
    })
}

/// The USB HID keyboard page usage UIKit reports (`UIKeyboardHIDUsage`) as winit's key code.
fn key_code(usage: u32) -> Option<KeyCode> {
    use KeyCode::*;
    const LETTERS: [KeyCode; 26] = [
        KeyA, KeyB, KeyC, KeyD, KeyE, KeyF, KeyG, KeyH, KeyI, KeyJ, KeyK, KeyL, KeyM, KeyN, KeyO,
        KeyP, KeyQ, KeyR, KeyS, KeyT, KeyU, KeyV, KeyW, KeyX, KeyY, KeyZ,
    ];
    const DIGITS: [KeyCode; 10] =
        [Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0];
    const FUNCTION: [KeyCode; 12] = [F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12];
    const PAD_DIGITS: [KeyCode; 10] =
        [Numpad1, Numpad2, Numpad3, Numpad4, Numpad5, Numpad6, Numpad7, Numpad8, Numpad9, Numpad0];
    Some(match usage {
        0x04..=0x1D => LETTERS[(usage - 0x04) as usize],
        0x1E..=0x27 => DIGITS[(usage - 0x1E) as usize],
        0x28 => Enter,
        0x29 => Escape,
        0x2A => Backspace,
        0x2B => Tab,
        0x2C => Space,
        0x2D => Minus,
        0x2E => Equal,
        0x2F => BracketLeft,
        0x30 => BracketRight,
        0x31 => Backslash,
        0x32 => IntlBackslash,
        0x33 => Semicolon,
        0x34 => Quote,
        0x35 => Backquote,
        0x36 => Comma,
        0x37 => Period,
        0x38 => Slash,
        0x39 => CapsLock,
        0x3A..=0x45 => FUNCTION[(usage - 0x3A) as usize],
        0x46 => PrintScreen,
        0x47 => ScrollLock,
        0x48 => Pause,
        0x49 => Insert,
        0x4A => Home,
        0x4B => PageUp,
        0x4C => Delete,
        0x4D => End,
        0x4E => PageDown,
        0x4F => ArrowRight,
        0x50 => ArrowLeft,
        0x51 => ArrowDown,
        0x52 => ArrowUp,
        0x53 => NumLock,
        0x54 => NumpadDivide,
        0x55 => NumpadMultiply,
        0x56 => NumpadSubtract,
        0x57 => NumpadAdd,
        0x58 => NumpadEnter,
        0x59..=0x62 => PAD_DIGITS[(usage - 0x59) as usize],
        0x63 => NumpadDecimal,
        0x64 => IntlBackslash,
        0x65 => ContextMenu,
        0x67 => NumpadEqual,
        0x87 => IntlRo,
        0x89 => IntlYen,
        0xE0 => ControlLeft,
        0xE1 => ShiftLeft,
        0xE2 => AltLeft,
        0xE3 => SuperLeft,
        0xE4 => ControlRight,
        0xE5 => ShiftRight,
        0xE6 => AltRight,
        0xE7 => SuperRight,
        _ => return None,
    })
}
