//! Added for Horizon: game controllers on iOS (a DualSense, an Xbox pad, a Switch Pro or MFi pad,
//! over Bluetooth), read through the GameController framework's extended gamepad profile.
use objc2_game_controller::{GCController, GCControllerButtonInput, GCControllerDirectionPad};

/// A controller's state, in the standard layout: buttons in the order South, East, West, North,
/// left bumper, right bumper, left trigger, right trigger, Select, Start, left stick, right
/// stick, D-pad up, down, left, right; sticks with y up; triggers 0..1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gamepad {
    pub buttons: [bool; 16],
    pub left: [f32; 2],
    pub right: [f32; 2],
    pub triggers: [f32; 2],
}

fn pressed(button: &GCControllerButtonInput) -> bool {
    unsafe { button.isPressed() }
}

fn stick(pad: &GCControllerDirectionPad) -> [f32; 2] {
    unsafe { [pad.xAxis().value(), pad.yAxis().value()] }
}

/// The current controller (the last one used), if one is connected with an extended profile.
pub fn read() -> Option<Gamepad> {
    let controller = unsafe { GCController::current() }
        .or_else(|| unsafe { GCController::controllers() }.to_vec_retained().into_iter().next())?;
    let pad = unsafe { controller.extendedGamepad() }?;
    unsafe {
        let dpad = pad.dpad();
        let optional = |b: Option<objc2::rc::Retained<GCControllerButtonInput>>| {
            b.is_some_and(|b| pressed(&b))
        };
        Some(Gamepad {
            buttons: [
                pressed(&pad.buttonA()),
                pressed(&pad.buttonB()),
                pressed(&pad.buttonX()),
                pressed(&pad.buttonY()),
                pressed(&pad.leftShoulder()),
                pressed(&pad.rightShoulder()),
                pressed(&pad.leftTrigger()),
                pressed(&pad.rightTrigger()),
                optional(pad.buttonOptions()),
                pressed(&pad.buttonMenu()),
                optional(pad.leftThumbstickButton()),
                optional(pad.rightThumbstickButton()),
                pressed(&dpad.up()),
                pressed(&dpad.down()),
                pressed(&dpad.left()),
                pressed(&dpad.right()),
            ],
            left: stick(&pad.leftThumbstick()),
            right: stick(&pad.rightThumbstick()),
            triggers: [pad.leftTrigger().value(), pad.rightTrigger().value()],
        })
    }
}
