//! The client's input layer, below gameplay and screens: key bindings and their
//! names, toggle keys, mouse settings, controllers, the clipboard and window
//! options. horizon-client re-exports each module under its old path
//! (`horizon_client::keybinds`, ...).
#![cfg_attr(not(target_os = "ios"), forbid(unsafe_code))]
// iOS: only the clipboard (UIPasteboard, `clipboard.rs`) calls the Objective-C runtime.
#![cfg_attr(target_os = "ios", deny(unsafe_code))]
pub mod actions;
#[cfg(target_os = "ios")]
pub mod ios_open;
pub mod clipboard;
pub mod gamepad;
pub mod gameplay_guide;
pub mod held;
pub mod input;
pub mod input_queue;
pub mod input_source;
pub mod interaction_input;
pub mod key_names;
pub mod keybinds;
pub mod keymapping;
pub mod modifiers;
pub mod mouse;
pub mod radial;
pub mod snap;
pub mod window;
