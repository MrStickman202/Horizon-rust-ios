//! Playing by touch (the iOS app): on-screen controls while a world is open and the player
//! touches the screen without a keyboard or a controller (`horizon_client::touch_controls` lays
//! them out and draws them, Bedrock-style), and a finger dragged anywhere else turns the view,
//! with or without them. The joystick and the D-pad move the player as a controller's stick
//! does (`input_move`: analog, any direction); the buttons press the keys and mouse buttons a
//! keyboard would (`control_key_down`, `play_mouse`); a hotbar slot is picked as a number key
//! picks it.
use super::*;
use horizon_client::touch_controls::{self, Control, Layout, Scheme, Shown};
use std::collections::HashMap;
use winit::event::{MouseButton, Touch, TouchPhase};

/// How long after a key or a controller button the controls stay hidden.
const KEYBOARD_QUIET: Duration = Duration::from_secs(4);
/// Look: the view's mouse units per point of finger travel (the mouse sensitivity applies on top).
const LOOK_SCALE: f64 = 1.2;
/// A finger held this long on the world without moving breaks (the tap schemes).
const HOLD_TO_BREAK: Duration = Duration::from_millis(300);
/// Points a finger may wander and still be a tap or a hold.
const TAP_SLOP: f32 = 10.0;
/// The joystick pushed this far forward sprints, as on Bedrock.
const SPRINT_AT: f32 = 0.9;

#[derive(Clone, Copy, Debug)]
enum Role {
    /// The joystick, from where the finger came down.
    Stick { origin: [f32; 2] },
    /// The D-pad's arrows: the direction follows the finger.
    DPad,
    Button(Control),
    /// Turning the view: the finger's last position in physical px, where (points) and when it
    /// came down, whether it wandered off (no tap) and whether its hold breaks.
    Look { last: (f64, f64), start: [f32; 2], at: Instant, moved: bool, breaking: bool },
    /// A tap on the hotbar.
    Slot,
}

#[derive(Default)]
pub(super) struct TouchPlay {
    /// The controls show: the last input was a finger.
    shown: bool,
    last_keyboard: Option<Instant>,
    fingers: HashMap<u64, Role>,
    /// Where the joystick's or the D-pad's finger is, in points.
    knob: Option<[f32; 2]>,
    /// The move vector the controls give (`[left, forward]`), while a finger moves.
    moving: Option<[f32; 2]>,
    sprinting: bool,
    held: Vec<Control>,
    sneaking: bool,
    /// The layout, read from `horizon-touch.txt` at the first touch.
    scheme: Option<Scheme>,
}

fn key_of(control: Control) -> Option<KeyCode> {
    Some(match control {
        Control::Jump => KeyCode::Space,
        Control::Inventory => KeyCode::KeyE,
        Control::Chat => KeyCode::KeyT,
        Control::Pause => KeyCode::Escape,
        _ => return None,
    })
}

impl Game<'_> {
    /// The window's physical size, its points' density and the GUI scale in points.
    fn touch_metrics(&self) -> Option<([f32; 2], f32, f32)> {
        let r = self.renderer.as_ref()?;
        let physical = [r.config.width as f32, r.config.height as f32];
        let density = touch_controls::density(physical);
        let scale = HudBatch::new([r.config.width, r.config.height], self.settings.gui_scale_inputs()).scale;
        Some((physical, density, scale / density))
    }

    fn touch_scheme(&self) -> Scheme {
        self.touch_play.scheme.unwrap_or_default()
    }

    fn touch_layout(&self) -> Option<Layout> {
        let (physical, density, gui) = self.touch_metrics()?;
        Some(Layout::new(physical[0] / density, physical[1] / density, gui, self.touch_scheme()))
    }

    /// A key or a controller hides the controls for a while.
    pub(super) fn touch_note_keyboard(&mut self) {
        self.touch_play.last_keyboard = Some(Instant::now());
        if self.touch_play.shown {
            self.touch_play.shown = false;
            self.touch_release_all();
        }
    }

    fn touch_keyboard_quiet(&self) -> bool {
        self.touch_play
            .last_keyboard
            .is_none_or(|at| at.elapsed() >= KEYBOARD_QUIET)
    }

    /// Whether the finger events go to the controls and the view: in a world, nothing open.
    fn touch_playing(&self) -> bool {
        self.screen == Screen::Playing && self.title.is_none() && self.player.is_some()
    }

    /// The controls' move vector (`[left, forward]`, -1..1), while a finger moves the player.
    pub(super) fn touch_move(&self) -> Option<[f32; 2]> {
        self.touch_play.moving.filter(|_| self.touch_playing())
    }

    /// A finger on the screen. True when it was taken here (the menus don't see it).
    pub(super) fn touch_event(&mut self, touch: &Touch) -> bool {
        if !self.touch_playing() {
            if !self.touch_play.fingers.is_empty() {
                self.touch_release_all();
            }
            return false;
        }
        if self.touch_play.scheme.is_none() {
            self.touch_play.scheme = Some(Scheme::load(&horizon_client::logging::player_dir()));
        }
        let Some((_, density, _)) = self.touch_metrics() else {
            return false;
        };
        let at = [touch.location.x as f32 / density, touch.location.y as f32 / density];
        let id = touch.id;
        let look = Role::Look {
            last: (touch.location.x, touch.location.y),
            start: at,
            at: Instant::now(),
            moved: false,
            breaking: false,
        };
        match touch.phase {
            TouchPhase::Started => {
                if self.touch_keyboard_quiet() {
                    self.touch_play.shown = true;
                }
                let moving_taken = self
                    .touch_play
                    .fingers
                    .values()
                    .any(|r| matches!(r, Role::Stick { .. } | Role::DPad));
                let role = match (self.touch_play.shown, self.touch_layout()) {
                    (true, Some(layout)) => {
                        if let Some(control) = layout.tile_at(at) {
                            self.touch_press(control, true);
                            Role::Button(control)
                        } else if let Some(slot) = layout.slot_at(at) {
                            let _ = self.select_hotbar_slot(slot);
                            Role::Slot
                        } else if layout.on_dpad_centre(at) {
                            self.touch_press(Control::Sneak, true);
                            Role::Button(Control::Sneak)
                        } else if layout.on_dpad(at) && !moving_taken {
                            self.touch_dpad(&layout, at);
                            Role::DPad
                        } else if layout.in_stick_zone(at) && !moving_taken {
                            self.touch_play.knob = Some(at);
                            self.touch_play.moving = Some([0.0, 0.0]);
                            Role::Stick { origin: at }
                        } else {
                            look
                        }
                    }
                    _ => look,
                };
                self.touch_play.fingers.insert(id, role);
            }
            TouchPhase::Moved => match self.touch_play.fingers.get(&id).copied() {
                Some(Role::Look { last, start, at: down_at, moved, breaking }) => {
                    let (dx, dy) = (touch.location.x - last.0, touch.location.y - last.1);
                    let moved = moved || (at[0] - start[0]).hypot(at[1] - start[1]) > TAP_SLOP;
                    self.touch_play.fingers.insert(
                        id,
                        Role::Look {
                            last: (touch.location.x, touch.location.y),
                            start,
                            at: down_at,
                            moved,
                            breaking,
                        },
                    );
                    let d = f64::from(density);
                    self.mouse.on_move(0.0, 0.0, dx / d * LOOK_SCALE, dy / d * LOOK_SCALE, true, true);
                    self.last_input = Instant::now();
                }
                Some(Role::Stick { origin }) => self.touch_stick(origin, at),
                Some(Role::DPad) => {
                    if let Some(layout) = self.touch_layout() {
                        self.touch_dpad(&layout, at);
                    }
                }
                _ => {}
            },
            TouchPhase::Ended | TouchPhase::Cancelled => match self.touch_play.fingers.remove(&id) {
                Some(Role::Button(control)) => self.touch_press(control, false),
                Some(Role::Stick { .. } | Role::DPad) => self.touch_stop_moving(),
                Some(Role::Look { moved, breaking, .. }) => {
                    if breaking {
                        let _ = self.play_mouse(MouseButton::Left, false);
                    } else if !moved
                        && touch.phase == TouchPhase::Ended
                        && self.touch_play.shown
                        && self.touch_scheme().taps_the_world()
                    {
                        // A tap on the world: use (place, open, eat) what the crosshair is on.
                        let _ = self.play_mouse(MouseButton::Right, true);
                        let _ = self.play_mouse(MouseButton::Right, false);
                    }
                }
                _ => {}
            },
        }
        true
    }

    /// Each pass of the loop: a finger held still on the world long enough starts breaking.
    pub(super) fn touch_tick(&mut self) {
        if !self.touch_play.shown || !self.touch_scheme().taps_the_world() {
            return;
        }
        let starting: Vec<u64> = self
            .touch_play
            .fingers
            .iter()
            .filter_map(|(id, role)| match role {
                Role::Look { at, moved: false, breaking: false, .. } if at.elapsed() >= HOLD_TO_BREAK => {
                    Some(*id)
                }
                _ => None,
            })
            .collect();
        for id in starting {
            if let Some(Role::Look { breaking, .. }) = self.touch_play.fingers.get_mut(&id) {
                *breaking = true;
            }
            let _ = self.play_mouse(MouseButton::Left, true);
        }
    }

    /// The joystick: an analog direction and how far, like a controller's stick.
    fn touch_stick(&mut self, origin: [f32; 2], at: [f32; 2]) {
        let reach = self.touch_layout().map_or(70.0, |l| l.stick_radius);
        let (mut x, mut y) = ((at[0] - origin[0]) / reach, (at[1] - origin[1]) / reach);
        let length = x.hypot(y);
        if length > 1.0 {
            (x, y) = (x / length, y / length);
        }
        self.touch_play.knob = Some([origin[0] + x * reach, origin[1] + y * reach]);
        self.touch_set_moving([-x, -y]);
    }

    fn touch_dpad(&mut self, layout: &Layout, at: [f32; 2]) {
        let [x, y] = layout.dpad_direction(at);
        self.touch_play.knob = Some(at);
        // Diagonals move at full speed along each axis, as two held keys do.
        self.touch_set_moving([-x, -y]);
    }

    fn touch_set_moving(&mut self, moving: [f32; 2]) {
        self.touch_play.moving = Some(moving);
        let sprint = moving[1] >= SPRINT_AT && self.touch_scheme() != Scheme::DPad;
        if sprint != self.touch_play.sprinting {
            self.touch_play.sprinting = sprint;
            if sprint {
                let _ = self.control_key_down(KeyCode::ControlLeft, KeyCode::ControlLeft);
            } else {
                self.control_key_up(KeyCode::ControlLeft);
            }
        }
    }

    fn touch_stop_moving(&mut self) {
        self.touch_set_moving([0.0, 0.0]);
        self.touch_play.moving = None;
        self.touch_play.knob = None;
    }

    fn touch_press(&mut self, control: Control, down: bool) {
        if down {
            self.touch_play.held.push(control);
        } else {
            self.touch_play.held.retain(|c| *c != control);
        }
        match control {
            Control::Attack => {
                let _ = self.play_mouse(MouseButton::Left, down);
            }
            Control::Use => {
                let _ = self.play_mouse(MouseButton::Right, down);
            }
            Control::Sneak => {
                // A toggle: one tap holds sneak, the next lets go.
                if down {
                    self.touch_play.sneaking = !self.touch_play.sneaking;
                    if self.touch_play.sneaking {
                        let _ = self.control_key_down(KeyCode::ShiftLeft, KeyCode::ShiftLeft);
                    } else {
                        self.control_key_up(KeyCode::ShiftLeft);
                    }
                }
            }
            other => {
                if let Some(key) = key_of(other) {
                    if down {
                        let _ = self.control_key_down(key, key);
                    } else {
                        self.control_key_up(key);
                    }
                }
            }
        }
    }

    /// Lets go of everything the fingers hold (a screen opened, a key took over).
    fn touch_release_all(&mut self) {
        for (_, role) in std::mem::take(&mut self.touch_play.fingers) {
            match role {
                Role::Button(control) => self.touch_press(control, false),
                Role::Look { breaking: true, .. } => {
                    let _ = self.play_mouse(MouseButton::Left, false);
                }
                _ => {}
            }
        }
        self.touch_stop_moving();
        if std::mem::take(&mut self.touch_play.sneaking) {
            self.control_key_up(KeyCode::ShiftLeft);
        }
        self.touch_play.held.clear();
    }

    /// The controls over the HUD, while they show in a world.
    pub(in crate::game) fn draw_touch_controls(&self, batch: &mut HudBatch) {
        if !self.touch_play.shown || !self.touch_playing() {
            return;
        }
        let Some(layout) = self.touch_layout() else {
            return;
        };
        let stick = self
            .touch_play
            .fingers
            .values()
            .find_map(|r| match r {
                Role::Stick { origin } => Some(*origin),
                _ => None,
            })
            .zip(self.touch_play.knob);
        let dpad = if self.touch_play.fingers.values().any(|r| matches!(r, Role::DPad)) {
            self.touch_play.knob.map_or([0.0; 2], |at| layout.dpad_direction(at))
        } else {
            [0.0; 2]
        };
        touch_controls::draw(
            &self.gui.atlas,
            batch,
            &layout,
            &Shown { stick, dpad, held: &self.touch_play.held, sneaking: self.touch_play.sneaking },
        );
    }
}
