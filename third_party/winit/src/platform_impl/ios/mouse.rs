//! Added for Horizon: a trackpad or mouse with the pointer locked (iPadOS 14+, `prefersPointerLocked`).
//! UIKit sends no touches for a locked pointer; the GameController framework's `GCMouse` reports
//! its movement, buttons and scrolling, which become winit's mouse motion, buttons and wheel.
use std::cell::{Cell, RefCell};
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_foundation::MainThreadMarker;
use objc2_game_controller::{
    GCControllerButtonInput, GCControllerDirectionPad, GCMouse, GCMouseInput,
};

use super::app_state::{self, EventWrapper};
use crate::event::{
    DeviceEvent, ElementState, Event, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use crate::platform_impl::platform::DEVICE_ID;
use crate::window::WindowId as RootWindowId;

thread_local! {
    static LOCKED: Cell<bool> = const { Cell::new(false) };
    static WINDOW: Cell<Option<RootWindowId>> = const { Cell::new(None) };
    /// The mice whose handlers are set, kept so a mouse isn't hooked twice.
    static HOOKED: RefCell<Vec<Retained<GCMouse>>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn locked() -> bool {
    LOCKED.with(Cell::get)
}

/// The window locks or frees the pointer. Every mouse or trackpad is hooked: those connected
/// now, and those GameController reports later (it lists them only some time after the start).
pub(crate) fn set_locked(window_id: RootWindowId, locked: bool) {
    LOCKED.with(|l| l.set(locked));
    WINDOW.with(|w| w.set(Some(window_id)));
    observe_connections();
    hook_all();
}

fn hook_all() {
    for mouse in unsafe { GCMouse::mice() }.to_vec_retained() {
        hook(&mouse);
    }
}

fn observe_connections() {
    thread_local! {
        static OBSERVING: Cell<bool> = const { Cell::new(false) };
    }
    if OBSERVING.with(|o| o.replace(true)) {
        return;
    }
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue};
    let connected = RcBlock::new(|_: NonNull<NSNotification>| hook_all());
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(objc2_game_controller::GCMouseDidConnectNotification),
            None,
            Some(&NSOperationQueue::mainQueue()),
            &connected,
        )
    };
    // The observer stays for the life of the app.
    std::mem::forget(observer);
}

fn send(event: EventWrapper) {
    if let Some(mtm) = MainThreadMarker::new() {
        app_state::handle_nonuser_event(mtm, event);
    }
}

fn window_event(event: WindowEvent) {
    if let Some(window_id) = WINDOW.with(Cell::get) {
        send(EventWrapper::StaticEvent(Event::WindowEvent { window_id, event }));
    }
}

fn hook(mouse: &Retained<GCMouse>) {
    let known = HOOKED.with(|h| h.borrow().iter().any(|m| Retained::as_ptr(m) == Retained::as_ptr(mouse)));
    if known {
        return;
    }
    HOOKED.with(|h| h.borrow_mut().push(mouse.clone()));
    let Some(input) = (unsafe { mouse.mouseInput() }) else {
        return;
    };
    let moved = RcBlock::new(|_: NonNull<GCMouseInput>, dx: f32, dy: f32| {
        if locked() {
            // GameController's y grows upward; winit's downward.
            send(EventWrapper::StaticEvent(Event::DeviceEvent {
                device_id: DEVICE_ID,
                event: DeviceEvent::MouseMotion { delta: (dx as f64, -dy as f64) },
            }));
        }
    });
    unsafe { input.setMouseMovedHandler(block_ptr(&moved)) };
    std::mem::forget(moved);
    let buttons = [
        (Some(unsafe { input.leftButton() }), MouseButton::Left),
        (unsafe { input.rightButton() }, MouseButton::Right),
        (unsafe { input.middleButton() }, MouseButton::Middle),
    ];
    for (button, which) in buttons {
        let Some(button) = button else {
            continue;
        };
        let changed = RcBlock::new(move |_: NonNull<GCControllerButtonInput>, _: f32, pressed: Bool| {
            if locked() {
                let state =
                    if pressed.as_bool() { ElementState::Pressed } else { ElementState::Released };
                window_event(WindowEvent::MouseInput { device_id: DEVICE_ID, state, button: which });
            }
        });
        unsafe { button.setPressedChangedHandler(block_ptr(&changed)) };
        std::mem::forget(changed);
    }
    let scroll = unsafe { input.scroll() };
    let scrolled = RcBlock::new(|_: NonNull<GCControllerDirectionPad>, x: f32, y: f32| {
        if locked() && (x != 0.0 || y != 0.0) {
            window_event(WindowEvent::MouseWheel {
                device_id: DEVICE_ID,
                delta: MouseScrollDelta::LineDelta(x / 10.0, y / 10.0),
                phase: TouchPhase::Moved,
            });
        }
    });
    unsafe { scroll.setValueChangedHandler(block_ptr(&scrolled)) };
    std::mem::forget(scrolled);
}

/// The block as the raw pointer the handler setters take; they copy it.
fn block_ptr<F: ?Sized>(block: &RcBlock<F>) -> *mut block2::Block<F> {
    &**block as *const block2::Block<F> as *mut block2::Block<F>
}
