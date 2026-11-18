//! On-screen controls for playing by touch (the iOS app): where they are, which one a finger
//! is on, and how they look. The game (`touch_play`) decides when they show and what they do.
//! Positions are in GUI px, the HUD's units.
use crate::ui_theme::{Face, Ui, sizes};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Jump,
    Sneak,
    Attack,
    Use,
    Inventory,
    Chat,
    Pause,
}

impl Control {
    fn label(self) -> &'static str {
        match self {
            Control::Jump => "horizon.touch.jump",
            Control::Sneak => "horizon.touch.sneak",
            Control::Attack => "horizon.touch.attack",
            Control::Use => "horizon.touch.use",
            Control::Inventory => "horizon.touch.inventory",
            Control::Chat => "horizon.touch.chat",
            Control::Pause => "horizon.touch.pause",
        }
    }
}

/// Where the controls are on a `w` × `h` GUI px screen.
pub struct Layout {
    /// Where a finger that comes down starts the move stick.
    pub stick_zone: [f32; 4],
    /// The stick's reach from where the finger came down.
    pub stick_radius: f32,
    /// The buttons: centre and radius.
    pub buttons: Vec<(Control, [f32; 2], f32)>,
    /// The hotbar (vanilla's: 182 × 22 at the bottom middle), nine 20 px slots.
    pub hotbar: [f32; 4],
}

impl Layout {
    pub fn new(w: f32, h: f32) -> Self {
        let margin = 14.0;
        let big = 22.0;
        let small = 15.0;
        let tiny = 11.0;
        let jump = [w - margin - big, h - margin - big - 26.0];
        let buttons = vec![
            (Control::Jump, jump, big),
            (Control::Sneak, [jump[0], jump[1] - big * 2.0 - 10.0], small),
            (Control::Use, [jump[0] - big * 2.0 - 10.0, jump[1]], small),
            (Control::Attack, [jump[0] - big * 1.6, jump[1] - big * 1.6 - 4.0], small),
            (Control::Pause, [w - margin - tiny, margin + tiny], tiny),
            (Control::Chat, [w - margin - tiny * 3.0 - 8.0, margin + tiny], tiny),
            (Control::Inventory, [w - margin - tiny * 5.0 - 16.0, margin + tiny], tiny),
        ];
        Self {
            stick_zone: [0.0, h * 0.3, w * 0.4, h * 0.7],
            stick_radius: 30.0,
            buttons,
            hotbar: [((w - 182.0) / 2.0).round(), h - 22.0, 182.0, 22.0],
        }
    }

    /// The button under `at`, if any (a little larger than drawn, for fingers).
    pub fn button_at(&self, at: [f32; 2]) -> Option<Control> {
        self.buttons
            .iter()
            .find(|(_, c, r)| (at[0] - c[0]).hypot(at[1] - c[1]) <= r + 6.0)
            .map(|(control, ..)| *control)
    }

    /// The hotbar slot under `at`, if any.
    pub fn slot_at(&self, at: [f32; 2]) -> Option<u8> {
        let [x, y, w, h] = self.hotbar;
        (at[0] >= x && at[0] < x + w && at[1] >= y - 4.0 && at[1] < y + h)
            .then(|| (((at[0] - x - 1.0) / 20.0).clamp(0.0, 8.0)) as u8)
    }

    pub fn in_stick_zone(&self, at: [f32; 2]) -> bool {
        let [x, y, w, h] = self.stick_zone;
        at[0] >= x && at[0] < x + w && at[1] >= y && at[1] < y + h
    }
}

/// What the controls show this frame.
pub struct Shown<'a> {
    /// The stick, while a finger holds it: where it came down and where it is.
    pub stick: Option<([f32; 2], [f32; 2])>,
    pub held: &'a [Control],
    pub sneaking: bool,
}

const PLATE: u32 = 0x5C14101E;
const HELD: u32 = 0xB05B3FD6;
const INK: u32 = 0xF2FFFFFF;
const RIM: u32 = 0x40FFFFFF;

pub fn draw(ui: &mut Ui, layout: &Layout, shown: &Shown) {
    let s = sizes();
    if let Some((origin, knob)) = shown.stick {
        let r = layout.stick_radius;
        ui.rounded(origin[0] - r, origin[1] - r, r * 2.0, r * 2.0, r, PLATE);
        ui.border(origin[0] - r, origin[1] - r, r * 2.0, r * 2.0, r, RIM);
        let k = 12.0;
        ui.rounded(knob[0] - k, knob[1] - k, k * 2.0, k * 2.0, k, 0xC0FFFFFF);
    }
    for (control, [cx, cy], r) in &layout.buttons {
        let on = shown.held.contains(control) || (*control == Control::Sneak && shown.sneaking);
        ui.rounded(cx - r, cy - r, r * 2.0, r * 2.0, *r, if on { HELD } else { PLATE });
        ui.border(cx - r, cy - r, r * 2.0, r * 2.0, *r, RIM);
        let label = crate::lang::get(control.label());
        let size = if *r > 14.0 { s.caption } else { s.caption * 0.8 };
        let lw = ui.measure(Face::Medium, size, &label);
        ui.text_line(Face::Medium, size, r * 2.0, cx - lw / 2.0, cy - r, INK, &label);
    }
}
