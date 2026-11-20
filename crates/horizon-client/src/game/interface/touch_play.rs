//! Playing by touch (the iOS app): on-screen controls while a world is open and the player
//! touches the screen without a keyboard or a controller (`horizon_client::touch_controls` lays
//! them out and draws them, Bedrock-style), and a finger dragged anywhere else turns the view,
//! with or without them; a tap on the world uses, a hold breaks. The joystick and the D-pad move
//! the player as a controller's stick does (`input_move`: analog, any direction); the tiles press
//! the keys and mouse buttons a keyboard would (`control_key_down`, `play_mouse`); Sneak and
//! Sprint hold their mapping as a mod's input does, so the keyboard's modifier state can't let
//! go of them (`heal_hold_latches`). The layout editor moves, resizes, hides and adds tiles.
use super::*;
use horizon_client::held::Source;
use horizon_client::touch_controls::{
    self, Config, Control, EditAction, KEYS, Layout, Placed, Scheme, Shown,
};
use std::collections::HashMap;
use winit::event::{MouseButton, Touch, TouchPhase};

/// How long after a key or a controller button the controls stay hidden.
const KEYBOARD_QUIET: Duration = Duration::from_secs(4);
/// Look: the view's mouse units per point of finger travel (the mouse sensitivity applies on top).
const LOOK_SCALE: f64 = 1.2;
/// A finger held this long on the world without moving breaks.
const HOLD_TO_BREAK: Duration = Duration::from_millis(300);
/// Points a finger may wander and still be a tap or a hold.
const TAP_SLOP: f32 = 10.0;
/// The joystick pushed this far forward sprints, as on Bedrock.
const SPRINT_AT: f32 = 0.9;
/// The editor: a tile let go faster than this (points a second) flies back to its place.
const FLICK_SPEED: f32 = 1400.0;
/// The editor: a tile dropped this close to its place snaps onto it.
const SNAP: f32 = 14.0;

#[derive(Clone, Copy, Debug)]
enum Role {
    /// The joystick, from where the finger came down.
    Stick {
        origin: [f32; 2],
    },
    /// The D-pad's arrows: the direction follows the finger.
    DPad,
    Button(Control),
    /// Turning the view: the finger's last position in physical px, where (points) and when it
    /// came down, whether it wandered off (no tap) and whether its hold breaks.
    Look {
        last: (f64, f64),
        start: [f32; 2],
        at: Instant,
        moved: bool,
        breaking: bool,
    },
    /// A tap on the hotbar.
    Slot,
    /// The editor: dragging a tile, from its centre's offset to the finger; the last two
    /// positions and times, for a flick.
    Drag {
        control: Control,
        offset: [f32; 2],
        last: ([f32; 2], Instant),
        before: ([f32; 2], Instant),
    },
    /// Nothing more until it lifts.
    Ignore,
    /// On the Back tile over a screen: its lift closes the screen.
    Back,
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
    /// The joystick at its rim holds sprint.
    rim_sprint: bool,
    held: Vec<Control>,
    sneaking: bool,
    sprinting: bool,
    /// The player's layout, read at the first touch.
    config: Option<Config>,
    /// The layout editor is open, and the tile it has selected.
    editing: Option<Option<Control>>,
    /// Horizon's own touch controls, for [`Scheme::Horizon`], with their saved layout.
    horizon: Option<Box<horizon_client_render::touch::TouchControls>>,
    /// Every finger down in a world and where it landed (x, in points), whatever took it: two
    /// at once, one on each half, bring the controls back straight away.
    down_x: HashMap<u64, f32>,
}

/// Horizon's own touch controls keep their layout here, beside `options.txt`.
const HORIZON_LAYOUT: &str = "horizon-touch-layout.json";

impl Game<'_> {
    /// The window's physical size, its points' density and the GUI scale in points.
    fn touch_metrics(&self) -> Option<([f32; 2], f32, f32)> {
        let r = self.renderer.as_ref()?;
        let physical = [r.config.width as f32, r.config.height as f32];
        let density = touch_controls::density(physical);
        let scale = HudBatch::new(
            [r.config.width, r.config.height],
            self.settings.gui_scale_inputs(),
        )
        .scale;
        Some((physical, density, scale / density))
    }

    fn touch_config(&mut self) -> &mut Config {
        self.touch_play
            .config
            .get_or_insert_with(|| Config::load(&horizon_client::logging::player_dir()))
    }

    fn touch_scheme(&self) -> Scheme {
        self.touch_play
            .config
            .as_ref()
            .map(|c| c.scheme)
            .unwrap_or_default()
    }

    fn touch_layout(&self) -> Option<Layout> {
        let (physical, density, gui) = self.touch_metrics()?;
        let default = Config::default();
        let config = self.touch_play.config.as_ref().unwrap_or(&default);
        Some(Layout::new(
            physical[0] / density,
            physical[1] / density,
            gui,
            config,
        ))
    }

    /// A key or a controller hides the controls for a while.
    pub(super) fn touch_note_keyboard(&mut self) {
        self.touch_play.last_keyboard = Some(Instant::now());
        if self.touch_play.shown && self.touch_play.editing.is_none() {
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

    /// A screen over the world with no button of its own to leave it (chat, inventories):
    /// the Back tile shows there for a touch player.
    fn touch_back_screen(&self) -> bool {
        self.title.is_none()
            && self.player.is_some()
            && matches!(
                self.screen,
                Screen::Chat | Screen::Inventory | Screen::Creative
            )
    }

    /// A finger is on the Back tile over a screen: the pointer UIKit makes of it doesn't click
    /// the screen underneath (an item held on the cursor would be thrown out).
    pub(super) fn touch_back_held(&self) -> bool {
        !self.touch_playing()
            && self
                .touch_play
                .fingers
                .values()
                .any(|r| matches!(r, Role::Back))
    }

    /// The controls' move vector (`[left, forward]`, -1..1), while a finger moves the player.
    pub(super) fn touch_move(&self) -> Option<[f32; 2]> {
        self.touch_play.moving.filter(|_| self.touch_playing())
    }

    /// Holds or lets go of `key` as a mod's input does: the mapping it is bound to (sneak,
    /// sprint) stays held whatever the keyboard's modifiers say.
    fn touch_hold_key(&mut self, key: KeyCode, down: bool) {
        if down {
            if self.keys.press(key, Source::Mod) {
                let _ = self.key_pressed(key, key);
            }
        } else {
            self.keys.release(&key, Source::Mod);
        }
    }

    /// A finger on the screen. True when it was taken here (the menus don't see it).
    pub(super) fn touch_event(&mut self, touch: &Touch) -> bool {
        self.touch_config();
        let Some((physical, density, _)) = self.touch_metrics() else {
            return false;
        };
        let at = [
            touch.location.x as f32 / density,
            touch.location.y as f32 / density,
        ];
        if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.touch_play.down_x.remove(&touch.id);
        }
        if !self.touch_playing() {
            // What the fingers held in the world lets go once a screen opens; the Back tile's
            // own finger stays, so its lift still closes the screen.
            if self
                .touch_play
                .fingers
                .values()
                .any(|r| !matches!(r, Role::Ignore | Role::Back))
            {
                self.touch_release_all();
            }
            self.touch_play
                .fingers
                .retain(|_, r| matches!(r, Role::Back));
            self.touch_play.editing = None;
            return self.touch_back_event(touch, at);
        }
        if self.touch_summon(touch, at, physical) {
            return true;
        }
        if self.touch_scheme() == Scheme::Horizon {
            // The map keeps only what the summon or the editor gave a finger here: its lift
            // forgets it (Horizon's controls track their own fingers).
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.touch_play.fingers.remove(&touch.id);
            }
            return self.touch_event_horizon(touch, physical);
        }
        if self.touch_play.editing.is_some() {
            self.touch_edit_event(touch, at);
            return true;
        }
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
                        if let Some(control) = layout.tile_at(at, false) {
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
                Some(Role::Look {
                    last,
                    start,
                    at: down_at,
                    moved,
                    breaking,
                }) => {
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
                    self.mouse.on_move(
                        0.0,
                        0.0,
                        dx / d * LOOK_SCALE,
                        dy / d * LOOK_SCALE,
                        true,
                        true,
                    );
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
            TouchPhase::Ended | TouchPhase::Cancelled => {
                match self.touch_play.fingers.remove(&id) {
                    Some(Role::Button(control)) => self.touch_press(control, false),
                    Some(Role::Stick { .. } | Role::DPad) => self.touch_stop_moving(),
                    Some(Role::Look {
                        moved, breaking, ..
                    }) => {
                        if breaking {
                            let _ = self.play_mouse(MouseButton::Left, false);
                        } else if !moved
                            && touch.phase == TouchPhase::Ended
                            && self.touch_taps_world()
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

    /// Two fingers down together, one on each half of the screen, while a keyboard has hidden
    /// the controls: they show at once (no waiting for the keyboard to go quiet). True when this
    /// finger did it; it is then kept from pressing anything.
    fn touch_summon(&mut self, touch: &Touch, at: [f32; 2], physical: [f32; 2]) -> bool {
        match touch.phase {
            TouchPhase::Started => {
                let half = physical[0] / touch_controls::density(physical) / 2.0;
                let other_side = self
                    .touch_play
                    .down_x
                    .iter()
                    .any(|(id, x)| *id != touch.id && (*x < half) != (at[0] < half));
                self.touch_play.down_x.insert(touch.id, at[0]);
                let hidden = !self.touch_play.shown || !self.touch_keyboard_quiet();
                if !(other_side && hidden) {
                    return false;
                }
                self.touch_play.last_keyboard = None;
                self.touch_play.shown = true;
                // The first finger was looking around: it doesn't tap the world when it lifts.
                for role in self.touch_play.fingers.values_mut() {
                    if let Role::Look { moved, .. } = role {
                        *moved = true;
                    }
                }
                self.touch_play.fingers.insert(touch.id, Role::Ignore);
                if let Some(controls) = self.touch_play.horizon.as_mut() {
                    controls.ignore(touch.id);
                }
                true
            }
            TouchPhase::Ended | TouchPhase::Cancelled | TouchPhase::Moved => false,
        }
    }

    /// Taps and holds on the world use and break: in the tap schemes, and in every scheme while
    /// the controls are hidden (a keyboard moves, a finger looks: Hit and Use aren't on screen).
    fn touch_taps_world(&self) -> bool {
        !self.touch_play.shown || self.touch_scheme().taps_the_world()
    }

    /// Over a screen with no button to leave it: the Back tile, for a touch player.
    fn touch_back_event(&mut self, touch: &Touch, at: [f32; 2]) -> bool {
        if !self.touch_back_screen() || !self.touch_keyboard_quiet() {
            return false;
        }
        let Some(layout) = self.touch_layout() else {
            return false;
        };
        let back = layout.back_tile();
        let on = (at[0] - back.centre[0]).abs() <= back.half + 6.0
            && (at[1] - back.centre[1]).abs() <= back.half + 6.0;
        match touch.phase {
            TouchPhase::Started if on => {
                self.touch_play.fingers.insert(touch.id, Role::Back);
                true
            }
            TouchPhase::Ended if self.touch_play.fingers.remove(&touch.id).is_some() => {
                if on {
                    self.touch_key(KeyCode::Escape, true);
                    self.touch_key(KeyCode::Escape, false);
                }
                true
            }
            TouchPhase::Cancelled => self.touch_play.fingers.remove(&touch.id).is_some(),
            _ => self.touch_play.fingers.contains_key(&touch.id),
        }
    }

    /// The layout editor: drag a tile to move it (a flick sends it back to its place), tap one
    /// to select it, and the toolbar and the selected tile's panel.
    fn touch_edit_event(&mut self, touch: &Touch, at: [f32; 2]) {
        let Some(layout) = self.touch_layout() else {
            return;
        };
        let selected = self.touch_play.editing.flatten();
        match touch.phase {
            TouchPhase::Started => {
                let button = touch_controls::editor_buttons(&layout, selected)
                    .into_iter()
                    .find(|(_, [x, y, w, h], _)| {
                        at[0] >= *x && at[0] < x + w && at[1] >= *y && at[1] < y + h
                    })
                    .map(|(action, ..)| action);
                if let Some(action) = button {
                    self.touch_edit_action(action, &layout);
                    self.touch_play.fingers.insert(touch.id, Role::Ignore);
                    return;
                }
                match layout.tile_at(at, true).filter(|c| *c != Control::Edit) {
                    Some(control) => {
                        let centre = layout.tile(control).map_or(at, |t| t.centre);
                        self.touch_play.editing = Some(Some(control));
                        let now = Instant::now();
                        self.touch_play.fingers.insert(
                            touch.id,
                            Role::Drag {
                                control,
                                offset: [centre[0] - at[0], centre[1] - at[1]],
                                last: (at, now),
                                before: (at, now),
                            },
                        );
                    }
                    None => {
                        self.touch_play.editing = Some(None);
                        self.touch_play.fingers.insert(touch.id, Role::Ignore);
                    }
                }
            }
            TouchPhase::Moved => {
                if let Some(Role::Drag {
                    control,
                    offset,
                    last,
                    ..
                }) = self.touch_play.fingers.get(&touch.id).copied()
                {
                    let centre = [
                        (at[0] + offset[0]).clamp(0.0, layout.w),
                        (at[1] + offset[1]).clamp(0.0, layout.h),
                    ];
                    self.touch_place(&layout, control, |p| {
                        p.x = centre[0] / layout.w;
                        p.y = centre[1] / layout.h;
                    });
                    self.touch_play.fingers.insert(
                        touch.id,
                        Role::Drag {
                            control,
                            offset,
                            last: (at, Instant::now()),
                            before: last,
                        },
                    );
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                if let Some(Role::Drag {
                    control,
                    last,
                    before,
                    ..
                }) = self.touch_play.fingers.remove(&touch.id)
                {
                    let seconds = last
                        .1
                        .duration_since(before.1)
                        .as_secs_f32()
                        .max(1.0 / 240.0);
                    let speed = (last.0[0] - before.0[0]).hypot(last.0[1] - before.0[1]) / seconds;
                    let recent = last.1.elapsed() < Duration::from_millis(80);
                    let home = layout.defaults.iter().find(|d| d.0 == control).map(|d| d.1);
                    let centre = layout.tile(control).map(|t| t.centre);
                    let near_home = home
                        .zip(centre)
                        .is_some_and(|(h, c)| (h[0] - c[0]).hypot(h[1] - c[1]) <= SNAP);
                    if let Some(home) = home
                        && ((recent && speed >= FLICK_SPEED) || near_home)
                    {
                        // Flicked, or dropped by its place: back where it belongs.
                        self.touch_place(&layout, control, |p| {
                            p.x = home[0] / layout.w;
                            p.y = home[1] / layout.h;
                        });
                    }
                } else {
                    self.touch_play.fingers.remove(&touch.id);
                }
            }
        }
    }

    /// Changes `control`'s entry in the layout, made from where it is now if it has none.
    fn touch_place(&mut self, layout: &Layout, control: Control, change: impl FnOnce(&mut Placed)) {
        let id = control.id();
        let centre = layout
            .tile(control)
            .map_or([layout.w / 2.0, layout.h / 2.0], |t| t.centre);
        let placed = self.touch_config().placed_mut();
        let entry = match placed.iter().position(|p| p.id == id) {
            Some(i) => &mut placed[i],
            None => {
                placed.push(Placed {
                    id,
                    x: centre[0] / layout.w,
                    y: centre[1] / layout.h,
                    size: 1.0,
                    hidden: false,
                });
                placed.last_mut().expect("just pushed")
            }
        };
        change(entry);
    }

    fn touch_edit_action(&mut self, action: EditAction, layout: &Layout) {
        let selected = self.touch_play.editing.flatten();
        match action {
            EditAction::Scheme => {
                self.touch_play.editing = None;
                let dir = horizon_client::logging::player_dir();
                if let Err(e) = self.touch_config().save(&dir) {
                    log::warn!("Horizon: could not keep the touch layout: {e}");
                }
                self.touch_next_scheme();
            }
            EditAction::Done => {
                self.touch_play.editing = None;
                let dir = horizon_client::logging::player_dir();
                if let Err(e) = self.touch_config().save(&dir) {
                    log::warn!("Horizon: could not keep the touch layout: {e}");
                }
            }
            EditAction::Reset => {
                self.touch_config().placed_mut().clear();
                self.touch_play.editing = Some(None);
            }
            EditAction::Add => {
                let used: Vec<String> = self
                    .touch_config()
                    .placed()
                    .iter()
                    .map(|p| p.id.clone())
                    .collect();
                if let Some(index) =
                    (0..KEYS.len()).find(|i| !used.contains(&Control::Key(*i).id()))
                {
                    self.touch_config().placed_mut().push(Placed {
                        id: Control::Key(index).id(),
                        x: 0.5,
                        y: 0.6,
                        size: 1.0,
                        hidden: false,
                    });
                    self.touch_play.editing = Some(Some(Control::Key(index)));
                }
            }
            EditAction::Smaller | EditAction::Bigger => {
                if let Some(control) = selected {
                    let factor = if action == EditAction::Bigger {
                        1.15
                    } else {
                        1.0 / 1.15
                    };
                    self.touch_place(layout, control, |p| {
                        p.size = (p.size * factor).clamp(0.4, 2.5)
                    });
                }
            }
            EditAction::Hide => {
                if let Some(control) = selected {
                    self.touch_place(layout, control, |p| p.hidden = !p.hidden);
                }
            }
            EditAction::Mode => match selected {
                Some(Control::Sneak) => {
                    let config = self.touch_config();
                    config.sneak_hold = !config.sneak_hold;
                }
                Some(Control::Sprint) => {
                    let config = self.touch_config();
                    config.sprint_hold = !config.sprint_hold;
                }
                _ => {}
            },
            EditAction::NextKey => {
                if let Some(Control::Key(index)) = selected {
                    let used: Vec<String> = self
                        .touch_config()
                        .placed()
                        .iter()
                        .map(|p| p.id.clone())
                        .collect();
                    let next = (1..KEYS.len())
                        .map(|step| (index + step) % KEYS.len())
                        .find(|i| !used.contains(&Control::Key(*i).id()));
                    if let Some(next) = next {
                        let old = Control::Key(index).id();
                        if let Some(p) = self
                            .touch_config()
                            .placed_mut()
                            .iter_mut()
                            .find(|p| p.id == old)
                        {
                            p.id = Control::Key(next).id();
                        }
                        self.touch_play.editing = Some(Some(Control::Key(next)));
                    }
                }
            }
            EditAction::Delete => {
                if let Some(control @ Control::Key(_)) = selected {
                    let id = control.id();
                    self.touch_config().placed_mut().retain(|p| p.id != id);
                    self.touch_play.editing = Some(None);
                }
            }
        }
    }

    /// The keyboard key a binding (`key.togglePerspective`) is on, if it is on one.
    fn touch_bound_key(&self, binding: &str) -> Option<KeyCode> {
        horizon_client::keybinds::key_code(self.settings.keys.get(binding))
    }

    /// Switches to the next touch scheme, keeps the choice and names it over the hotbar.
    fn touch_next_scheme(&mut self) {
        self.touch_release_all();
        if let Some(controls) = self.touch_play.horizon.as_mut() {
            controls.clear();
        }
        self.pad.inject_state(Default::default());
        let next = self.touch_scheme().next();
        self.touch_config().scheme = next;
        if let Err(e) = next.save(&horizon_client::logging::player_dir()) {
            log::warn!("Horizon: could not keep the touch scheme: {e}");
        }
        self.titles
            .set_overlay(horizon_client::text::Text::literal(next.name()), false);
    }

    /// [`Scheme::Horizon`]: the finger goes to Horizon's own touch controls, which turn it into
    /// the virtual controller's state, a camera turn and requests (chat).
    fn touch_event_horizon(&mut self, touch: &Touch, physical: [f32; 2]) -> bool {
        use horizon_client_render::touch::{Settings, TouchControls, camera_delta, hotbar_slot};
        if self.touch_keyboard_quiet() {
            self.touch_play.shown = true;
        }
        if !self.touch_play.shown {
            return false;
        }
        // GUI px per physical px, for the hotbar.
        let gui = self.touch_metrics().map_or(1.0, |(_, d, g)| g * d);
        let controls = self.touch_play.horizon.get_or_insert_with(|| {
            let mut controls = TouchControls::default();
            controls.settings =
                Settings::load(&horizon_client::logging::player_dir().join(HORIZON_LAYOUT));
            Box::new(controls)
        });
        let (w, h) = (physical[0].max(1.0), physical[1].max(1.0));
        controls.viewport(w as u32, h as u32);
        let point = [touch.location.x as f32 / w, touch.location.y as f32 / h];
        let started = touch.phase == TouchPhase::Started;
        let ended = matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled);
        // A tap on the hotbar picks the slot, as on Bedrock.
        if started && controls.control_at(point).is_none() {
            let gui_point = [touch.location.x as f32 / gui, touch.location.y as f32 / gui];
            if let Some(slot) = hotbar_slot(w / gui, h / gui, gui_point) {
                controls.ignore(touch.id);
                let _ = self.select_hotbar_slot(slot as u8);
                return true;
            }
        }
        let turn = controls.update(touch.id, point, started, ended);
        if turn != [0.0, 0.0] {
            let [yaw, pitch] = camera_delta(turn, w as u32, h as u32, &controls.settings);
            self.look_turn(f64::from(yaw) / 0.15, f64::from(pitch) / 0.15);
            self.last_input = Instant::now();
        }
        true
    }

    /// [`Scheme::Horizon`], each pass of the loop: the controls' controller state reaches the
    /// game, and their taps and requests act.
    fn touch_tick_horizon(&mut self) {
        let playing = self.touch_playing();
        let Some(controls) = self.touch_play.horizon.as_mut() else {
            return;
        };
        if !playing || !self.touch_play.shown {
            controls.clear();
            self.pad.inject_state(Default::default());
            return;
        }
        let state = controls.poll_state();
        let tap = controls.take_tap();
        let chat = std::mem::take(&mut controls.chat_requested);
        let debug = std::mem::take(&mut controls.debug_requested);
        let perspective = std::mem::take(&mut controls.perspective_requested);
        let settings = std::mem::take(&mut controls.settings_requested);
        let keys = controls.key_events();
        self.pad.inject_state(state);
        // The controls' key tokens are binding names (`key.swapOffhand`): the key bound to it.
        for (token, down) in keys {
            let name = token.strip_prefix("action:").unwrap_or(&token);
            if let Some(key) = self.touch_bound_key(name) {
                self.touch_key(key, down);
            }
        }
        for (asked, name, fallback) in [
            (debug, "key.debug.overlay", KeyCode::F3),
            (perspective, "key.togglePerspective", KeyCode::F5),
        ] {
            if asked {
                let key = self.touch_bound_key(name).unwrap_or(fallback);
                self.touch_key(key, true);
                self.touch_key(key, false);
            }
        }
        if settings {
            self.touch_next_scheme();
            return;
        }
        if tap.is_some() {
            let _ = self.play_mouse(MouseButton::Right, true);
            let _ = self.play_mouse(MouseButton::Right, false);
        }
        if chat {
            self.touch_key(KeyCode::KeyT, true);
            self.touch_key(KeyCode::KeyT, false);
        }
    }

    /// Each pass of the loop: a finger held still on the world long enough starts breaking.
    pub(super) fn touch_tick(&mut self) {
        if self.touch_scheme() == Scheme::Horizon {
            self.touch_tick_horizon();
            return;
        }
        if !self.touch_taps_world() || self.touch_play.editing.is_some() {
            return;
        }
        let starting: Vec<u64> = self
            .touch_play
            .fingers
            .iter()
            .filter_map(|(id, role)| match role {
                Role::Look {
                    at,
                    moved: false,
                    breaking: false,
                    ..
                } if at.elapsed() >= HOLD_TO_BREAK => Some(*id),
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
        let rim = moving[1] >= SPRINT_AT && self.touch_scheme() != Scheme::DPad;
        if rim != self.touch_play.rim_sprint {
            self.touch_play.rim_sprint = rim;
            self.touch_sync_sprint();
        }
    }

    /// Sprint is held while the joystick is at its rim or the Sprint tile holds it.
    fn touch_sync_sprint(&mut self) {
        let want = self.touch_play.rim_sprint || self.touch_play.sprinting;
        let held = self.keys.held_by(&KeyCode::ControlLeft, Source::Mod);
        if want != held {
            self.touch_hold_key(KeyCode::ControlLeft, want);
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
        let hold = self.touch_config().hold(control);
        match control {
            Control::Attack => {
                let _ = self.play_mouse(MouseButton::Left, down);
            }
            Control::Use => {
                let _ = self.play_mouse(MouseButton::Right, down);
            }
            Control::Edit => {
                if down {
                    self.touch_release_all();
                    self.touch_play.shown = true;
                    self.touch_play.editing = Some(None);
                }
            }
            Control::Sneak => {
                // Hold: while the tile is held. Toggle: one tap holds it, the next lets go.
                let now = if hold {
                    down
                } else if down {
                    !self.touch_play.sneaking
                } else {
                    self.touch_play.sneaking
                };
                if now != self.touch_play.sneaking {
                    self.touch_play.sneaking = now;
                    self.touch_hold_key(KeyCode::ShiftLeft, now);
                }
            }
            Control::Sprint => {
                let now = if hold {
                    down
                } else if down {
                    !self.touch_play.sprinting
                } else {
                    self.touch_play.sprinting
                };
                self.touch_play.sprinting = now;
                self.touch_sync_sprint();
            }
            other => {
                // A rebound key follows its binding, as the keyboard's would.
                let binding = match other {
                    Control::Debug => Some("key.debug.overlay"),
                    Control::Perspective => Some("key.togglePerspective"),
                    Control::Chat => Some("key.chat"),
                    Control::Inventory => Some("key.inventory"),
                    Control::Jump => Some("key.jump"),
                    _ => None,
                };
                if let Some(key) = binding
                    .and_then(|b| self.touch_bound_key(b))
                    .or(other.key())
                {
                    self.touch_key(key, down);
                }
            }
        }
    }

    /// A tile's key, down or up, the way the keyboard's goes: the debug keys hear it first
    /// (F3 toggles the overlay on its release, F3 with another key is a debug combination),
    /// then the open screen or the game.
    fn touch_key(&mut self, key: KeyCode, down: bool) {
        use horizon_client::debug_keys::KeyAction;
        if down {
            self.physical_keys.insert(key);
            if self.debug_key(key, KeyAction::Press) {
                return;
            }
            let _ = self.control_key_down(key, key);
        } else {
            self.physical_keys.remove(&key);
            let _ = self.debug_key(key, KeyAction::Release);
            self.control_key_up(key);
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
            self.touch_hold_key(KeyCode::ShiftLeft, false);
        }
        self.touch_play.sprinting = false;
        self.touch_play.rim_sprint = false;
        self.touch_sync_sprint();
        self.touch_play.held.clear();
    }

    /// The controls over the HUD, while they show in a world; the Back tile over a screen.
    pub(in crate::game) fn draw_touch_controls(&self, batch: &mut HudBatch) {
        let Some(layout) = self.touch_layout() else {
            return;
        };
        if !self.touch_playing() {
            if self.touch_play.shown && self.touch_back_screen() && self.touch_keyboard_quiet() {
                touch_controls::draw(
                    &self.gui.atlas,
                    batch,
                    &layout,
                    &Shown {
                        stick: None,
                        dpad: [0.0; 2],
                        held: &[],
                        sneaking: false,
                        sprinting: false,
                        editing: None,
                        back_only: true,
                    },
                );
            }
            return;
        }
        if !self.touch_play.shown {
            return;
        }
        if self.touch_scheme() == Scheme::Horizon {
            if let Some(controls) = &self.touch_play.horizon {
                controls.draw(batch, &self.gui.atlas);
            }
            return;
        }
        let stick = self
            .touch_play
            .fingers
            .values()
            .find_map(|r| match r {
                Role::Stick { origin } => Some(*origin),
                _ => None,
            })
            .zip(self.touch_play.knob);
        let dpad = if self
            .touch_play
            .fingers
            .values()
            .any(|r| matches!(r, Role::DPad))
        {
            self.touch_play
                .knob
                .map_or([0.0; 2], |at| layout.dpad_direction(at))
        } else {
            [0.0; 2]
        };
        let default = Config::default();
        let config = self.touch_play.config.as_ref().unwrap_or(&default);
        touch_controls::draw(
            &self.gui.atlas,
            batch,
            &layout,
            &Shown {
                stick,
                dpad,
                held: &self.touch_play.held,
                sneaking: self.touch_play.sneaking,
                sprinting: self.touch_play.sprinting || self.touch_play.rim_sprint,
                editing: self.touch_play.editing.map(|selected| (selected, config)),
                back_only: false,
            },
        );
    }
}
