//! iOS: the app's home screen, before the title (the game's `ios_launcher` shows it while the
//! resources load). Drawn in Horizon's design language with the bundled fonts: the offline
//! name, the Microsoft account, the game version and one Play button.
use crate::touch_controls::Scheme;
use crate::ui_theme::{Face, Ui, color, sizes};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

/// The column, in dp.
const COLUMN: f32 = 420.0;
const SECTION: f32 = 28.0;
const NAME_MAX: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Name,
    SignIn,
    Touch,
    Play,
}

pub struct Launcher {
    name: String,
    /// The name being typed, while the field is open.
    editing: Option<String>,
    /// The pointer in physical pixels.
    pub pointer: (f64, f64),
    /// A line under the account row after Sign in.
    note: Option<String>,
    /// The on-screen controls' layout (`touch_controls`).
    scheme: Scheme,
}

struct Layout {
    x: f32,
    y: f32,
    width: f32,
    name_field: [f32; 4],
    sign_in: [f32; 4],
    touch: [f32; 4],
    play: [f32; 4],
}

fn inside([x, y, w, h]: [f32; 4], (cx, cy): (f32, f32)) -> bool {
    cx >= x && cx < x + w && cy >= y && cy < y + h
}

impl Launcher {
    /// The home screen on iOS; elsewhere the launcher is a program of its own.
    pub fn at_start() -> Option<Self> {
        if !cfg!(target_os = "ios") {
            return None;
        }
        Some(Self {
            name: crate::offline_name::load_or_create(&crate::logging::player_dir()),
            editing: None,
            pointer: (0.0, 0.0),
            note: None,
            scheme: Scheme::load(&crate::logging::player_dir()),
        })
    }

    fn layout(&self, ui: &Ui) -> Layout {
        let width = COLUMN.min(ui.w - 48.0).max(200.0);
        let control = sizes().control_large;
        let title_line = 40.0;
        let label_line = 20.0;
        let height = title_line + 22.0 + SECTION * 3.0 + (label_line + 6.0 + control) * 3.0
            + SECTION + control + 40.0;
        let x = ((ui.w - width) / 2.0).round();
        let y = ((ui.h - height) / 2.0).max(16.0).round();
        let mut at = y + title_line + 22.0 + SECTION;
        let name_field = [x, at + label_line + 6.0, width, control];
        at = name_field[1] + control + SECTION;
        let sign_in = [x, at + label_line + 6.0, width, control];
        at = sign_in[1] + control + 18.0 + SECTION;
        let touch = [x, at + label_line + 6.0, width, control];
        at = touch[1] + control + SECTION;
        let play = [x, at, width, control];
        Layout { x, y, width, name_field, sign_in, touch, play }
    }

    pub fn draw(&self, ui: &mut Ui, ready: bool) {
        let l = self.layout(ui);
        let s = sizes();
        ui.rounded(0.0, 0.0, ui.w, ui.h, 0.0, color::bg());
        ui.text_line(Face::SemiBold, s.title, 40.0, l.x, l.y, color::text(), "Horizon");
        let version = crate::lang::tr(
            "horizon.ios.home.version",
            &[&horizon_core::VANILLA_VERSION],
        );
        ui.text_line(Face::Regular, s.label, 22.0, l.x, l.y + 40.0, color::muted(), &version);

        // The offline name.
        let label = |ui: &mut Ui, rect: [f32; 4], key: &str| {
            ui.text_line(
                Face::Medium,
                s.caption,
                20.0,
                rect[0],
                rect[1] - 26.0,
                color::muted(),
                &crate::lang::get(key),
            );
        };
        label(ui, l.name_field, "horizon.ios.home.name");
        let [fx, fy, fw, fh] = l.name_field;
        let editing = self.editing.is_some();
        ui.rounded(fx, fy, fw, fh, s.radius, color::raised());
        if editing {
            ui.border(fx, fy, fw, fh, s.radius, color::accent());
        }
        let shown = match &self.editing {
            Some(text) => format!("{text}|"),
            None => self.name.clone(),
        };
        ui.text_line(Face::Regular, s.ui, fh, fx + 16.0, fy, color::text(), &shown);

        // The Microsoft account.
        label(ui, l.sign_in, "horizon.ios.home.account");
        let [bx, by, bw, bh] = l.sign_in;
        let hovered = inside(l.sign_in, ui.cursor);
        ui.rounded(bx, by, bw, bh, bh / 2.0, if hovered { color::hover() } else { color::raised() });
        let sign_in = crate::lang::get("horizon.ios.home.sign_in");
        let lw = ui.measure(Face::Medium, s.ui, &sign_in);
        ui.text_line(Face::Medium, s.ui, bh, bx + ((bw - lw) / 2.0).round(), by, color::text(), &sign_in);
        let note = self.note.clone().unwrap_or_else(|| crate::lang::get("horizon.ios.home.offline"));
        ui.text_line(Face::Regular, s.caption, 18.0, bx, by + bh + 8.0, color::muted(), &note);

        // The touch controls' layout: a tap picks the next one.
        label(ui, l.touch, "horizon.ios.home.touch");
        let [tx, ty, tw, th] = l.touch;
        let hovered = inside(l.touch, ui.cursor);
        ui.rounded(tx, ty, tw, th, s.radius, if hovered { color::hover() } else { color::raised() });
        ui.text_line(Face::Regular, s.ui, th, tx + 16.0, ty, color::text(), &self.scheme.name());
        let more = "›";
        let mw = ui.measure(Face::Medium, s.ui, more);
        ui.text_line(Face::Medium, s.ui, th, tx + tw - 16.0 - mw, ty, color::muted(), more);

        // Play.
        let [px, py, pw, ph] = l.play;
        let hovered = inside(l.play, ui.cursor);
        let plate = if hovered { color::with_alpha(color::accent(), 0.85) } else { color::accent() };
        ui.rounded(px, py, pw, ph, ph / 2.0, plate);
        let play = crate::lang::get(if ready {
            "horizon.ios.home.play"
        } else {
            "horizon.ios.home.play_loading"
        });
        let lw = ui.measure(Face::Medium, s.ui, &play);
        ui.text_line(Face::Medium, s.ui, ph, px + ((pw - lw) / 2.0).round(), py, color::on_accent(), &play);
        let _ = l.width;
    }

    pub fn hit(&self, ui: &Ui) -> Option<Hit> {
        let l = self.layout(ui);
        [
            (l.name_field, Hit::Name),
            (l.sign_in, Hit::SignIn),
            (l.touch, Hit::Touch),
            (l.play, Hit::Play),
        ]
            .into_iter()
            .find(|(rect, _)| inside(*rect, ui.cursor))
            .map(|(_, hit)| hit)
    }

    /// Ends name editing: a valid name is kept, anything else goes back to the old one.
    fn commit(&mut self, window: Option<&Window>) {
        if let Some(text) = self.editing.take() {
            let text = text.trim().to_owned();
            if crate::offline_name::is_valid(&text) {
                match crate::offline_name::save(&crate::logging::player_dir(), &text) {
                    Ok(()) => self.name = text,
                    Err(e) => log::warn!("Horizon: could not keep the offline name: {e}"),
                }
            }
        }
        if let Some(window) = window {
            window.set_ime_allowed(false);
        }
    }

    /// A window event while the home screen shows. `with_ui` gives the click a Ui of the
    /// window's size. True when Play was pressed: the title takes over.
    pub fn event(
        &mut self,
        window: Option<&Window>,
        event: &WindowEvent,
        hit_at: impl Fn((f64, f64), &Self) -> Option<Hit>,
    ) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = (position.x, position.y);
                false
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                self.click(window, hit_at(self.pointer, self))
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(text) = &mut self.editing {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape) => {
                            self.commit(window);
                        }
                        PhysicalKey::Code(KeyCode::Backspace) => {
                            text.pop();
                        }
                        _ => {
                            if let Some(typed) = event.text.as_deref() {
                                for c in typed.chars() {
                                    if (c.is_ascii_alphanumeric() || c == '_') && text.len() < NAME_MAX {
                                        text.push(c);
                                    }
                                }
                            }
                        }
                    }
                    false
                } else {
                    matches!(event.physical_key, PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter))
                        && {
                            self.commit(window);
                            true
                        }
                }
            }
            _ => false,
        }
    }

    fn click(&mut self, window: Option<&Window>, hit: Option<Hit>) -> bool {
        match hit {
            Some(Hit::Name) => {
                if self.editing.is_none() {
                    self.editing = Some(self.name.clone());
                    if let Some(window) = window {
                        window.set_ime_allowed(true);
                    }
                }
                false
            }
            Some(Hit::SignIn) => {
                self.commit(window);
                self.note = Some(crate::lang::get("horizon.ios.home.sign_in_soon"));
                false
            }
            Some(Hit::Touch) => {
                self.commit(window);
                self.scheme = self.scheme.next();
                if let Err(e) = self.scheme.save(&crate::logging::player_dir()) {
                    log::warn!("Horizon: could not keep the touch controls: {e}");
                }
                false
            }
            Some(Hit::Play) => {
                self.commit(window);
                true
            }
            None => {
                self.commit(window);
                false
            }
        }
    }
}

