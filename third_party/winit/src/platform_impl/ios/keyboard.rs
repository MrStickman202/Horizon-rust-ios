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
    static WINDOW: std::cell::Cell<Option<RootWindowId>> = const { std::cell::Cell::new(None) };
    /// The keys GameController reports down, with the key each was sent as (a digit held
    /// with Cmd goes as an F key, and its release with it).
    static DOWN: std::cell::RefCell<Vec<(u32, KeyCode)>> = const { std::cell::RefCell::new(Vec::new()) };
    static OBSERVING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether a hardware key typed text in the last moment (the view skips its `insertText:`).
pub(crate) fn typed_by_key_just_now() -> bool {
    TYPED_AT.with(|t| t.get()).is_some_and(|at| at.elapsed() < std::time::Duration::from_millis(150))
}

/// Hardware keys come from two places. Key down and key up, the modifiers included, come
/// from the GameController framework's keyboard (`GCKeyboard`), which reports every key of
/// every attached keyboard whichever view has the focus: UIKit's presses can lose a key up on
/// the way through the responder chain (Shift stuck down). The text a key types comes from
/// UIKit's presses (the layout's characters), as a text-only key event, the way `insertText:`
/// sends it.
pub(crate) fn start(window_id: RootWindowId) {
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};
    WINDOW.with(|w| w.set(Some(window_id)));
    hook_keyboard();
    if OBSERVING.with(|o| o.replace(true)) {
        return;
    }
    let connected = block2::RcBlock::new(|_: std::ptr::NonNull<NSNotification>| hook_keyboard());
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(objc2_game_controller::GCKeyboardDidConnectNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &connected,
        )
    };
    std::mem::forget(observer);
}

fn hook_keyboard() {
    use objc2_game_controller::{GCControllerButtonInput, GCKeyCode, GCKeyboard, GCKeyboardInput};
    use std::ptr::NonNull;
    let Some(keyboard) = (unsafe { GCKeyboard::coalescedKeyboard() }) else {
        return;
    };
    let Some(input) = (unsafe { keyboard.keyboardInput() }) else {
        return;
    };
    let changed = block2::RcBlock::new(
        |_: NonNull<GCKeyboardInput>, _: NonNull<GCControllerButtonInput>, code: GCKeyCode, pressed: objc2::runtime::Bool| {
            key_changed(code as u32, pressed.as_bool());
        },
    );
    let ptr = &*changed as *const block2::Block<_> as *mut block2::Block<_>;
    unsafe { input.setKeyChangedHandler(ptr) };
    std::mem::forget(changed);
}

fn modifier_state(down: &[(u32, KeyCode)]) -> ModifiersState {
    let mut state = ModifiersState::empty();
    for (_, code) in down {
        match code {
            KeyCode::ShiftLeft | KeyCode::ShiftRight => state |= ModifiersState::SHIFT,
            KeyCode::ControlLeft | KeyCode::ControlRight => state |= ModifiersState::CONTROL,
            KeyCode::AltLeft | KeyCode::AltRight => state |= ModifiersState::ALT,
            KeyCode::SuperLeft | KeyCode::SuperRight => state |= ModifiersState::SUPER,
            _ => {}
        }
    }
    state
}

/// The iPad's Magic Keyboard and most keyboard cases have no function row: Cmd and a digit
/// (Cmd+1 to Cmd+0, Cmd+- and Cmd+=) is F1 to F12.
fn function_key(code: KeyCode, command: bool) -> KeyCode {
    use KeyCode::*;
    if !command {
        return code;
    }
    match code {
        Digit1 => F1,
        Digit2 => F2,
        Digit3 => F3,
        Digit4 => F4,
        Digit5 => F5,
        Digit6 => F6,
        Digit7 => F7,
        Digit8 => F8,
        Digit9 => F9,
        Digit0 => F10,
        Minus => F11,
        Equal => F12,
        other => other,
    }
}

fn key_changed(usage: u32, pressed: bool) {
    let Some(window_id) = WINDOW.with(|w| w.get()) else {
        return;
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(physical) = key_code(usage) else {
        return;
    };
    // iPadOS can hold back a key's release while Cmd is down: when Cmd goes up, the F keys it
    // made (Cmd+3 is F3) go up with it, so F3's overlay toggles and nothing stays held.
    let orphans: Vec<KeyCode> = if !pressed && matches!(physical, KeyCode::SuperLeft | KeyCode::SuperRight) {
        DOWN.with(|down| {
            let mut down = down.borrow_mut();
            let mut gone = Vec::new();
            down.retain(|(u, sent)| {
                let made = key_code(*u).is_some_and(|own| own != *sent);
                if made {
                    gone.push(*sent);
                }
                !made
            });
            gone
        })
    } else {
        Vec::new()
    };
    let (sent, modifiers) = DOWN.with(|down| {
        let mut down = down.borrow_mut();
        let sent = if pressed {
            let command = modifier_state(&down).super_key();
            let sent = function_key(physical, command);
            down.retain(|(u, _)| *u != usage);
            down.push((usage, sent));
            sent
        } else {
            let at = down.iter().position(|(u, _)| *u == usage);
            at.map_or(physical, |i| down.remove(i).1)
        };
        (sent, modifier_state(&down))
    });
    let state = if pressed { ElementState::Pressed } else { ElementState::Released };
    let logical_key = match named_key(sent) {
        Some(named) => Key::Named(named),
        None => match us_character(sent) {
            Some(c) => Key::Character(smol_str::SmolStr::new(c)),
            None => Key::Unidentified(crate::keyboard::NativeKey::Unidentified),
        },
    };
    let mut events = Vec::new();
    for code in orphans {
        let logical_key = named_key(code).map_or(
            Key::Unidentified(crate::keyboard::NativeKey::Unidentified),
            Key::Named,
        );
        events.push(EventWrapper::StaticEvent(Event::WindowEvent {
            window_id,
            event: WindowEvent::KeyboardInput {
                device_id: DEVICE_ID,
                event: KeyEvent {
                    physical_key: PhysicalKey::Code(code),
                    logical_key,
                    text: None,
                    location: location(code),
                    state: ElementState::Released,
                    repeat: false,
                    platform_specific: KeyEventExtra {},
                },
                is_synthetic: false,
            },
        }));
    }
    if matches!(logical_key, Key::Named(NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Super)) {
        events.push(EventWrapper::StaticEvent(Event::WindowEvent {
            window_id,
            event: WindowEvent::ModifiersChanged(Modifiers::from(modifiers)),
        }));
    }
    events.push(EventWrapper::StaticEvent(Event::WindowEvent {
        window_id,
        event: WindowEvent::KeyboardInput {
            device_id: DEVICE_ID,
            event: KeyEvent {
                physical_key: PhysicalKey::Code(sent),
                logical_key,
                text: None,
                location: location(sent),
                state,
                repeat: false,
                platform_specific: KeyEventExtra {},
            },
            is_synthetic: false,
        },
    }));
    app_state::handle_nonuser_events(mtm, events);
}

/// The character a key gives on a US layout, unshifted: the key's logical name for bindings
/// (`key.keyboard.a`); the text typed comes from UIKit's layout.
fn us_character(code: KeyCode) -> Option<&'static str> {
    use KeyCode::*;
    Some(match code {
        KeyA => "a", KeyB => "b", KeyC => "c", KeyD => "d", KeyE => "e", KeyF => "f", KeyG => "g",
        KeyH => "h", KeyI => "i", KeyJ => "j", KeyK => "k", KeyL => "l", KeyM => "m", KeyN => "n",
        KeyO => "o", KeyP => "p", KeyQ => "q", KeyR => "r", KeyS => "s", KeyT => "t", KeyU => "u",
        KeyV => "v", KeyW => "w", KeyX => "x", KeyY => "y", KeyZ => "z",
        Digit1 => "1", Digit2 => "2", Digit3 => "3", Digit4 => "4", Digit5 => "5",
        Digit6 => "6", Digit7 => "7", Digit8 => "8", Digit9 => "9", Digit0 => "0",
        Minus => "-", Equal => "=", BracketLeft => "[", BracketRight => "]", Backslash => "\\",
        Semicolon => ";", Quote => "'", Backquote => "`", Comma => ",", Period => ".", Slash => "/",
        _ => return None,
    })
}

/// UIKit's presses: only the text a key types (the keys themselves come from [`key_changed`]).
/// Returns true when any press was a keyboard key, so the caller doesn't pass it on.
pub(crate) fn handle_presses(
    mtm: MainThreadMarker,
    window_id: RootWindowId,
    presses: &NSSet<UIPress>,
    state: ElementState,
) -> bool {
    let mut keyboard = false;
    let mut events = Vec::new();
    for press in presses.iter() {
        let Some(key) = (unsafe { press.key(mtm) }) else {
            continue;
        };
        keyboard = true;
        if state != ElementState::Pressed {
            continue;
        }
        if let Some(text) = typed_text(&key) {
            TYPED_AT.with(|t| t.set(Some(std::time::Instant::now())));
            events.push(EventWrapper::StaticEvent(Event::WindowEvent {
                window_id,
                event: WindowEvent::KeyboardInput {
                    device_id: DEVICE_ID,
                    event: KeyEvent {
                        physical_key: PhysicalKey::Unidentified(
                            crate::keyboard::NativeKeyCode::Unidentified,
                        ),
                        logical_key: Key::Character(text.clone()),
                        text: Some(text),
                        location: KeyLocation::Standard,
                        state: ElementState::Pressed,
                        repeat: false,
                        platform_specific: KeyEventExtra {},
                    },
                    is_synthetic: false,
                },
            }));
        } else if unsafe { key.keyCode() }.0 == 0x2A || unsafe { key.keyCode() }.0 == 0x28 {
            // Backspace and Enter type nothing, but UIKit's own deleteBackward or newline
            // must not repeat them.
            TYPED_AT.with(|t| t.set(Some(std::time::Instant::now())));
        }
    }
    if !events.is_empty() {
        app_state::handle_nonuser_events(mtm, events);
    }
    keyboard
}

/// What a press types: its characters, a space for Space; nothing for a shortcut (Cmd or Ctrl
/// held) or a key that types no character.
fn typed_text(key: &Retained<UIKey>) -> Option<smol_str::SmolStr> {
    let flags = unsafe { key.modifierFlags() };
    if flags.intersects(UIKeyModifierFlags::UIKeyModifierCommand | UIKeyModifierFlags::UIKeyModifierControl) {
        return None;
    }
    let usage = unsafe { key.keyCode() }.0 as u32;
    if key_code(usage) == Some(KeyCode::Space) {
        return Some(" ".into());
    }
    let characters = unsafe { key.characters() }.to_string();
    let mut chars = characters.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if !c.is_control() && !('\u{F700}'..='\u{F8FF}').contains(&c) => {
            Some(smol_str::SmolStr::new(&characters))
        }
        _ => None,
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
