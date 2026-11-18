//! Playing by touch (the iOS app): on-screen controls while a world is open and the player
//! touches the screen without a keyboard or a controller (`horizon_client::touch_controls` draws
//! them), and a finger dragged anywhere else turns the view, with or without them. The
//! controls press the same keys and mouse buttons a keyboard would (`control_key_down`,
//! `play_mouse`); a hotbar slot is picked as a number key picks it.
use super::*;
use horizon_client::touch_controls::{Control, Layout, Scheme, Shown};
use std::collections::HashMap;
use winit::event::{MouseButton, Touch, TouchPhase};

/// How long after a key or a controller button the controls stay hidden.
const KEYBOARD_QUIET: Duration = Duration::from_secs(4);
/// Look: GUI px of finger travel to the view's mouse units.
const LOOK_SCALE: f64 = 2.2;
/// A finger held this long on the world without moving breaks (the tap schemes).
const HOLD_TO_BREAK: Duration = Duration::from_millis(300);
/// GUI px a finger may wander and still be a tap or a hold.
const TAP_SLOP: f32 = 8.0;

#[derive(Clone, Copy, Debug)]
enum Role {
    /// The move stick, from where the finger came down.
    Stick { origin: [f32; 2] },
    Button(Control),
    /// Turning the view: the finger's last position in physical px, where (GUI px) and when it
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
    /// The stick's finger position, GUI px.
    knob: Option<[f32; 2]>,
    /// The keys the stick holds (W, A, S, D, sprint).
    stick_keys: Vec<KeyCode>,
    held: Vec<Control>,
    sneaking: bool,
    /// The layout, read from `horizon-touch.txt` at the first touch.
    scheme: Option<Scheme>,
}

fn key_of(control: Control) -> Option<KeyCode> {
    Some(match control {
        Control::Forward => KeyCode::KeyW,
        Control::Back => KeyCode::KeyS,
        Control::Left => KeyCode::KeyA,
        Control::Right => KeyCode::KeyD,
        Control::Jump => KeyCode::Space,
        Control::Inventory => KeyCode::KeyE,
        Control::Chat => KeyCode::KeyT,
        Control::Pause => KeyCode::Escape,
        _ => return None,
    })
}

impl Game<'_> {
    fn touch_scale(&self) -> f32 {
        self.renderer.as_ref().map_or(1.0, |r| {
            HudBatch::new([r.config.width, r.config.height], self.settings.gui_scale_inputs()).scale
        })
    }

    fn touch_scheme(&self) -> Scheme {
        self.touch_play.scheme.unwrap_or_default()
    }

    fn touch_layout(&self) -> Option<Layout> {
        let r = self.renderer.as_ref()?;
        let batch = HudBatch::new([r.config.width, r.config.height], self.settings.gui_scale_inputs());
        Some(Layout::new(batch.width, batch.height, self.touch_scheme()))
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
            && !self.pad.in_use
    }

    /// Whether the finger events go to the controls and the view: in a world, nothing open.
    fn touch_playing(&self) -> bool {
        self.screen == Screen::Playing && self.title.is_none() && self.player.is_some()
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
        let scale = self.touch_scale();
        let at = [touch.location.x as f32 / scale, touch.location.y as f32 / scale];
        let look = Role::Look {
            last: (touch.location.x, touch.location.y),
            start: at,
            at: Instant::now(),
            moved: false,
            breaking: false,
        };
        let id = touch.id;
        match touch.phase {
            TouchPhase::Started => {
                if self.touch_keyboard_quiet() {
                    self.touch_play.shown = true;
                }
                let layout = self.touch_layout();
                let role = match (self.touch_play.shown, layout) {
                    (true, Some(layout)) => {
                        if let Some(control) = layout.button_at(at) {
                            self.touch_press(control, true);
                            Role::Button(control)
                        } else if let Some(slot) = layout.slot_at(at) {
                            let _ = self.select_hotbar_slot(slot);
                            Role::Slot
                        } else if layout.in_stick_zone(at)
                            && !self
                                .touch_play
                                .fingers
                                .values()
                                .any(|r| matches!(r, Role::Stick { .. }))
                        {
                            self.touch_play.knob = Some(at);
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
                    let s = f64::from(scale).max(1.0);
                    self.mouse.on_move(0.0, 0.0, dx / s * LOOK_SCALE, dy / s * LOOK_SCALE, true, true);
                    self.last_input = Instant::now();
                }
                Some(Role::Stick { origin }) => {
                    self.touch_play.knob = Some(at);
                    self.touch_stick(origin, at);
                }
                _ => {}
            },
            TouchPhase::Ended | TouchPhase::Cancelled => {
                match self.touch_play.fingers.remove(&id) {
                    Some(Role::Button(control)) => self.touch_press(control, false),
                    Some(Role::Stick { .. }) => {
                        self.touch_play.knob = None;
                        self.touch_set_stick_keys(Vec::new());
                    }
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
                }
            }
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
                Role::Look { at, moved: false, breaking: false, .. }
                    if at.elapsed() >= HOLD_TO_BREAK =>
                {
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

    fn touch_stick(&mut self, origin: [f32; 2], at: [f32; 2]) {
        let reach = self.touch_layout().map_or(30.0, |l| l.stick_radius);
        let (dx, dy) = ((at[0] - origin[0]) / reach, (at[1] - origin[1]) / reach);
        let mut keys = Vec::new();
        if dy < -0.35 {
            keys.push(KeyCode::KeyW);
        }
        if dy > 0.35 {
            keys.push(KeyCode::KeyS);
        }
        if dx < -0.35 {
            keys.push(KeyCode::KeyA);
        }
        if dx > 0.35 {
            keys.push(KeyCode::KeyD);
        }
        // Pushed to the rim, forward: sprint.
        if dy < -0.9 {
            keys.push(KeyCode::ControlLeft);
        }
        self.touch_set_stick_keys(keys);
    }

    fn touch_set_stick_keys(&mut self, keys: Vec<KeyCode>) {
        let before = std::mem::take(&mut self.touch_play.stick_keys);
        for key in before.iter().filter(|k| !keys.contains(k)) {
            self.control_key_up(*key);
        }
        for key in keys.iter().filter(|k| !before.contains(k)) {
            let _ = self.control_key_down(*key, *key);
        }
        self.touch_play.stick_keys = keys;
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
        self.touch_play.knob = None;
        self.touch_set_stick_keys(Vec::new());
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
        let layout = Layout::new(batch.width, batch.height, self.touch_scheme());
        let stick = self
            .touch_play
            .fingers
            .values()
            .find_map(|r| match r {
                Role::Stick { origin } => Some(*origin),
                _ => None,
            })
            .zip(self.touch_play.knob)
            .map(|(origin, knob)| {
                let r = layout.stick_radius;
                let (dx, dy) = (knob[0] - origin[0], knob[1] - origin[1]);
                let d = dx.hypot(dy);
                let k = if d > r { r / d } else { 1.0 };
                (origin, [origin[0] + dx * k, origin[1] + dy * k])
            });
        let mut ui = horizon_client::ui_theme::Ui::new(&self.gui.atlas, batch, self.cursor);
        horizon_client::touch_controls::draw(
            &mut ui,
            &layout,
            &Shown { stick, held: &self.touch_play.held, sneaking: self.touch_play.sneaking },
        );
        ui.finish();
    }
}
