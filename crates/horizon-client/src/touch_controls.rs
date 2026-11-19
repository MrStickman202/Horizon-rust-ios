//! On-screen controls for playing by touch (the iOS app), laid out and drawn after Bedrock
//! Edition's: translucent square tiles with white icons, a joystick or a D-pad at the bottom
//! left, Jump at the bottom right, the top bar (layout editor, F3, perspective, chat, pause)
//! centred at the top. The player can move, resize, hide and add tiles in the layout editor
//! ([`Config`], kept in `horizon-touch.json`). Where the controls are, which one a finger is on,
//! and how they look; the game (`touch_play`) decides when they show and what they do.
//! Positions are in screen points: physical pixels over [`density`].
use crate::hud::{HudAtlas, HudBatch};
use crate::ui_theme::{Face, Ui, sizes};
use serde::{Deserialize, Serialize};
use std::path::Path;
use winit::keyboard::KeyCode;

/// The three layouts Bedrock Edition offers on phones and tablets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    /// A move stick; tap the world to use, hold it to break.
    #[default]
    Joystick,
    /// A move stick, a crosshair and Hit and Use buttons.
    Crosshair,
    /// Four arrow buttons; tap to use, hold to break.
    DPad,
    /// Horizon's own touch controls (from the Starling Android port: `horizon_client_render::touch`),
    /// with its layout file and its gestures.
    Horizon,
}

const FILE: &str = "horizon-touch.txt";

impl Scheme {
    pub const ALL: [Scheme; 4] = [
        Scheme::Joystick,
        Scheme::Crosshair,
        Scheme::DPad,
        Scheme::Horizon,
    ];
    fn key(self) -> &'static str {
        match self {
            Scheme::Joystick => "joystick",
            Scheme::Crosshair => "crosshair",
            Scheme::DPad => "dpad",
            Scheme::Horizon => "horizon",
        }
    }
    /// Its name in the current language.
    pub fn name(self) -> String {
        crate::lang::get(match self {
            Scheme::Joystick => "horizon.touch.scheme.joystick",
            Scheme::Crosshair => "horizon.touch.scheme.crosshair",
            Scheme::DPad => "horizon.touch.scheme.dpad",
            Scheme::Horizon => "horizon.touch.scheme.horizon",
        })
    }
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
    /// Tapping and holding the world uses and breaks (no Hit or Use button).
    pub fn taps_the_world(self) -> bool {
        self != Scheme::Crosshair
    }
    /// The player's choice, kept beside `options.txt`.
    pub fn load(dir: &std::path::Path) -> Self {
        let text = std::fs::read_to_string(dir.join(FILE)).unwrap_or_default();
        let key = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("scheme:"))
            .unwrap_or("");
        Self::ALL
            .into_iter()
            .find(|s| s.key() == key)
            .unwrap_or_default()
    }
    pub fn save(self, dir: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(dir.join(FILE), format!("scheme:{}\n", self.key()))
    }
}

/// Physical pixels per screen point: `ui_theme::Ui`'s own unit, so the controls are the same
/// size on every screen and drawn where they are hit.
pub fn density(physical: [f32; 2]) -> f32 {
    (physical[0] / 1040.0)
        .min(physical[1] / 700.0)
        .clamp(1.0, 4.0)
}

/// The keys a tile the player adds can press: the keyboard keys a touch player has no other
/// way to reach.
pub const KEYS: &[(&str, KeyCode)] = &[
    ("Q", KeyCode::KeyQ),
    ("F", KeyCode::KeyF),
    ("F1", KeyCode::F1),
    ("F2", KeyCode::F2),
    ("F3", KeyCode::F3),
    ("F5", KeyCode::F5),
    ("Tab", KeyCode::Tab),
    ("/", KeyCode::Slash),
    ("L", KeyCode::KeyL),
    ("1", KeyCode::Digit1),
    ("2", KeyCode::Digit2),
    ("3", KeyCode::Digit3),
    ("B", KeyCode::KeyB),
    ("G", KeyCode::KeyG),
    ("H", KeyCode::KeyH),
    ("J", KeyCode::KeyJ),
    ("K", KeyCode::KeyK),
    ("R", KeyCode::KeyR),
    ("V", KeyCode::KeyV),
    ("X", KeyCode::KeyX),
    ("Z", KeyCode::KeyZ),
    ("C", KeyCode::KeyC),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Jump,
    Sneak,
    Sprint,
    Attack,
    Use,
    Inventory,
    Chat,
    Pause,
    /// F3: the debug screen.
    Debug,
    /// F5: the camera perspective.
    Perspective,
    /// Opens the layout editor.
    Edit,
    /// A screen's Back (Escape), over screens with no button for it (chat, inventories).
    Back,
    /// A tile the player added: presses `KEYS[index]`.
    Key(usize),
}

impl Control {
    /// The name the layout file keeps it under.
    pub fn id(self) -> String {
        match self {
            Control::Jump => "jump".into(),
            Control::Sneak => "sneak".into(),
            Control::Sprint => "sprint".into(),
            Control::Attack => "attack".into(),
            Control::Use => "use".into(),
            Control::Inventory => "inventory".into(),
            Control::Chat => "chat".into(),
            Control::Pause => "pause".into(),
            Control::Debug => "debug".into(),
            Control::Perspective => "perspective".into(),
            Control::Edit => "edit".into(),
            Control::Back => "back".into(),
            Control::Key(i) => format!("key:{}", KEYS.get(i).map_or("", |k| k.0)),
        }
    }
    /// The key it presses, for the plain key tiles.
    pub fn key(self) -> Option<KeyCode> {
        Some(match self {
            Control::Jump => KeyCode::Space,
            Control::Inventory => KeyCode::KeyE,
            Control::Chat => KeyCode::KeyT,
            Control::Pause | Control::Back => KeyCode::Escape,
            Control::Debug => KeyCode::F3,
            Control::Perspective => KeyCode::F5,
            Control::Key(i) => KEYS.get(i)?.1,
            _ => return None,
        })
    }
    /// Sneak and Sprint can hold or toggle.
    pub fn has_mode(self) -> bool {
        matches!(self, Control::Sneak | Control::Sprint)
    }
}

/// One tile's place as the player set it: its centre as fractions of the screen and its size
/// over the default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placed {
    pub id: String,
    pub x: f32,
    pub y: f32,
    #[serde(default = "one")]
    pub size: f32,
    #[serde(default)]
    pub hidden: bool,
}

fn one() -> f32 {
    1.0
}

/// The player's touch controls: the scheme, Sneak's and Sprint's modes and the layout edits,
/// kept beside `options.txt`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    #[serde(skip)]
    pub scheme: Scheme,
    /// Sneak holds while the tile is held (else a tap toggles it).
    pub sneak_hold: bool,
    /// Sprint holds while the tile is held (else a tap toggles it).
    pub sprint_hold: bool,
    /// Per scheme, the tiles the player moved, resized, hid or added.
    pub joystick: Vec<Placed>,
    pub crosshair: Vec<Placed>,
    pub dpad: Vec<Placed>,
}

const CONFIG: &str = "horizon-touch.json";

impl Config {
    pub fn load(dir: &Path) -> Self {
        let mut config: Config = std::fs::read_to_string(dir.join(CONFIG))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        config.scheme = Scheme::load(dir);
        config
    }
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        self.scheme.save(dir)?;
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(dir.join(CONFIG), text)
    }
    pub fn placed(&self) -> &Vec<Placed> {
        match self.scheme {
            Scheme::Crosshair => &self.crosshair,
            Scheme::DPad => &self.dpad,
            _ => &self.joystick,
        }
    }
    pub fn placed_mut(&mut self) -> &mut Vec<Placed> {
        match self.scheme {
            Scheme::Crosshair => &mut self.crosshair,
            Scheme::DPad => &mut self.dpad,
            _ => &mut self.joystick,
        }
    }
    pub fn hold(&self, control: Control) -> bool {
        match control {
            Control::Sneak => self.sneak_hold,
            Control::Sprint => self.sprint_hold,
            _ => true,
        }
    }
}

/// A tile: what it does, its centre and half its side, in points, and whether the player hid it
/// (drawn only in the editor).
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub control: Control,
    pub centre: [f32; 2],
    pub half: f32,
    pub hidden: bool,
}

impl Tile {
    fn contains(&self, at: [f32; 2], slop: f32) -> bool {
        (at[0] - self.centre[0]).abs() <= self.half + slop
            && (at[1] - self.centre[1]).abs() <= self.half + slop
    }
}

/// Where the controls are on a `w` × `h` point screen.
pub struct Layout {
    pub scheme: Scheme,
    pub w: f32,
    pub h: f32,
    /// Where a finger that comes down takes the joystick (the joystick schemes).
    pub stick_zone: [f32; 4],
    /// The joystick's resting place and its reach.
    pub stick_home: [f32; 2],
    pub stick_radius: f32,
    /// The D-pad's centre and its tiles' half side (the D-pad scheme).
    pub dpad: [f32; 2],
    pub dpad_half: f32,
    pub tiles: Vec<Tile>,
    /// Each tile's place before the player moved it, for the editor's snap back.
    pub defaults: Vec<(Control, [f32; 2], f32)>,
    /// The hotbar, in points.
    pub hotbar: [f32; 4],
}

impl Layout {
    /// `gui`: the GUI scale over [`density`], for the hotbar (vanilla's 182 × 22 GUI px).
    pub fn new(w: f32, h: f32, gui: f32, config: &Config) -> Self {
        let scheme = config.scheme;
        let m = 28.0;
        let big = 38.0;
        let small = 28.0;
        let top = 20.0;
        let jump = [w - m - big, h - m - big - 30.0];
        let mut defaults = vec![(Control::Jump, jump, big)];
        // The top bar, centred: edit, F3, F5, chat, pause.
        let bar = [
            Control::Edit,
            Control::Debug,
            Control::Perspective,
            Control::Chat,
            Control::Pause,
        ];
        let step = top * 2.0 + 10.0;
        let first = w / 2.0 - step * (bar.len() as f32 - 1.0) / 2.0;
        for (i, control) in bar.into_iter().enumerate() {
            defaults.push((control, [first + step * i as f32, m + top], top));
        }
        let hotbar_w = 182.0 * gui;
        let hotbar = [((w - hotbar_w) / 2.0), h - 22.0 * gui, hotbar_w, 22.0 * gui];
        // Inventory: the "…" tile at the hotbar's right end, as on Bedrock.
        defaults.push((
            Control::Inventory,
            [
                hotbar[0] + hotbar_w + 11.0 * gui + 4.0,
                hotbar[1] + 11.0 * gui,
            ],
            11.0 * gui,
        ));
        let beside = [jump[0] - big - small - 16.0, jump[1] + big - small];
        if scheme != Scheme::DPad {
            defaults.push((Control::Sneak, beside, small));
        }
        defaults.push((
            Control::Sprint,
            [beside[0] - small * 2.0 - 12.0, beside[1]],
            small,
        ));
        if scheme == Scheme::Crosshair {
            defaults.push((
                Control::Attack,
                [jump[0], jump[1] - big - small - 16.0],
                small,
            ));
            defaults.push((
                Control::Use,
                [beside[0], jump[1] - big - small - 16.0],
                small,
            ));
        }
        // The player's edits: moved, resized, hidden; and the tiles they added.
        let mut tiles = Vec::new();
        for (control, centre, half) in &defaults {
            let placed = config.placed().iter().find(|p| p.id == control.id());
            tiles.push(match placed {
                Some(p) => Tile {
                    control: *control,
                    centre: [p.x * w, p.y * h],
                    half: half * p.size.clamp(0.4, 2.5),
                    hidden: p.hidden,
                },
                None => Tile {
                    control: *control,
                    centre: *centre,
                    half: *half,
                    hidden: false,
                },
            });
        }
        for p in config.placed() {
            let Some(name) = p.id.strip_prefix("key:") else {
                continue;
            };
            let Some(index) = KEYS.iter().position(|k| k.0 == name) else {
                continue;
            };
            if tiles.iter().any(|t| t.control == Control::Key(index)) {
                continue;
            }
            tiles.push(Tile {
                control: Control::Key(index),
                centre: [p.x * w, p.y * h],
                half: small * p.size.clamp(0.4, 2.5),
                hidden: p.hidden,
            });
        }
        let dpad_half = 30.0;
        let dpad = [m + dpad_half * 3.0 + 8.0, h - m - dpad_half * 3.0 - 8.0];
        Self {
            scheme,
            w,
            h,
            stick_zone: [0.0, h * 0.35, w * 0.45, h * 0.65],
            stick_home: [m + 90.0, h - m - 90.0],
            stick_radius: 70.0,
            dpad,
            dpad_half,
            tiles,
            defaults,
            hotbar,
        }
    }

    /// The tile under `at`, if any (a little larger than drawn, for fingers). Hidden tiles
    /// count only in the editor.
    pub fn tile_at(&self, at: [f32; 2], editing: bool) -> Option<Control> {
        self.tiles
            .iter()
            .rev()
            .filter(|t| editing || !t.hidden)
            .find(|t| t.contains(at, 6.0))
            .map(|t| t.control)
    }

    pub fn tile(&self, control: Control) -> Option<&Tile> {
        self.tiles.iter().find(|t| t.control == control)
    }

    /// The hotbar slot under `at`, if any.
    pub fn slot_at(&self, at: [f32; 2]) -> Option<u8> {
        let [x, y, w, h] = self.hotbar;
        (at[0] >= x && at[0] < x + w && at[1] >= y - 6.0 && at[1] < y + h)
            .then(|| ((at[0] - x) / (w / 9.0)).clamp(0.0, 8.0) as u8)
    }

    pub fn in_stick_zone(&self, at: [f32; 2]) -> bool {
        let [x, y, w, h] = self.stick_zone;
        self.scheme != Scheme::DPad && at[0] >= x && at[0] < x + w && at[1] >= y && at[1] < y + h
    }

    /// On the D-pad (its 3 × 3 square, a little larger), the D-pad scheme only.
    pub fn on_dpad(&self, at: [f32; 2]) -> bool {
        let reach = self.dpad_half * 3.0 + 12.0;
        self.scheme == Scheme::DPad
            && (at[0] - self.dpad[0]).abs() <= reach
            && (at[1] - self.dpad[1]).abs() <= reach
    }

    /// The D-pad's middle tile (Sneak, as on Bedrock).
    pub fn on_dpad_centre(&self, at: [f32; 2]) -> bool {
        self.on_dpad(at)
            && (at[0] - self.dpad[0]).abs() <= self.dpad_half
            && (at[1] - self.dpad[1]).abs() <= self.dpad_half
    }

    /// Where a finger on the D-pad pushes: `[right, down]`, each -1, 0 or 1, by the eighth of
    /// the circle it is in, so sliding the finger round changes the direction.
    pub fn dpad_direction(&self, at: [f32; 2]) -> [f32; 2] {
        let (dx, dy) = (at[0] - self.dpad[0], at[1] - self.dpad[1]);
        if dx.hypot(dy) < self.dpad_half * 0.6 {
            return [0.0, 0.0];
        }
        let eighth = (dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() as i32;
        const DIRS: [[f32; 2]; 8] = [
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [-1.0, 1.0],
            [-1.0, 0.0],
            [-1.0, -1.0],
            [0.0, -1.0],
            [1.0, -1.0],
        ];
        DIRS[eighth.rem_euclid(8) as usize]
    }

    /// The Back tile over a screen: the top-left corner.
    pub fn back_tile(&self) -> Tile {
        Tile {
            control: Control::Back,
            centre: [28.0 + 22.0, 28.0 + 22.0],
            half: 22.0,
            hidden: false,
        }
    }
}

/// The layout editor's buttons: the toolbar's, and the selected tile's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditAction {
    Add,
    Reset,
    Done,
    Smaller,
    Bigger,
    Hide,
    /// Sneak's or Sprint's mode: hold or toggle.
    Mode,
    /// A tile the player added: the next key.
    NextKey,
    Delete,
    /// The next touch scheme (the Horizon scheme's Touch button cycles back).
    Scheme,
}

/// The editor's toolbar and the panel for `selected`, in points: each button's action, rect
/// and label key.
pub fn editor_buttons(
    layout: &Layout,
    selected: Option<Control>,
) -> Vec<(EditAction, [f32; 4], &'static str)> {
    let (bw, bh, gap) = (120.0, 40.0, 10.0);
    let mut out = Vec::new();
    let toolbar = [
        (EditAction::Scheme, "horizon.touch.edit.scheme"),
        (EditAction::Add, "horizon.touch.edit.add"),
        (EditAction::Reset, "horizon.touch.edit.reset"),
        (EditAction::Done, "horizon.touch.edit.done"),
    ];
    let total = toolbar.len() as f32 * bw + (toolbar.len() as f32 - 1.0) * gap;
    let mut x = (layout.w - total) / 2.0;
    let y = layout.h * 0.32;
    for (action, label) in toolbar {
        out.push((action, [x, y, bw, bh], label));
        x += bw + gap;
    }
    if let Some(control) = selected {
        let mut panel = vec![
            (EditAction::Smaller, "horizon.touch.edit.smaller"),
            (EditAction::Bigger, "horizon.touch.edit.bigger"),
            (EditAction::Hide, "horizon.touch.edit.hide"),
        ];
        if control.has_mode() {
            panel.push((EditAction::Mode, "horizon.touch.edit.mode"));
        }
        if let Control::Key(_) = control {
            panel.push((EditAction::NextKey, "horizon.touch.edit.key"));
            panel.push((EditAction::Delete, "horizon.touch.edit.delete"));
        }
        let total = panel.len() as f32 * bw + (panel.len() as f32 - 1.0) * gap;
        let mut x = (layout.w - total) / 2.0;
        let y = y + bh + 14.0;
        for (action, label) in panel {
            out.push((action, [x, y, bw, bh], label));
            x += bw + gap;
        }
    }
    out
}

/// What the controls show this frame.
pub struct Shown<'a> {
    /// The joystick: its base and its knob, while a finger holds it.
    pub stick: Option<([f32; 2], [f32; 2])>,
    /// Where the D-pad is pushed (`[right, down]`).
    pub dpad: [f32; 2],
    pub held: &'a [Control],
    pub sneaking: bool,
    pub sprinting: bool,
    /// The layout editor is open: every tile shows, hidden ones dimmed, the selected one ringed.
    pub editing: Option<(Option<Control>, &'a Config)>,
    /// Only the Back tile, over an open screen.
    pub back_only: bool,
}

// Bedrock's tiles: dark glass, a light rim, white icons; pressed, lighter.
const TILE: u32 = 0x66000000;
const TILE_HELD: u32 = 0xA0FFFFFF;
const RIM: u32 = 0x55FFFFFF;
const ICON: u32 = 0xEEFFFFFF;
const ICON_HELD: u32 = 0xEE1B1530;
const SELECTED: u32 = 0xFF8069EF;

/// Draws the controls. `atlas` and `batch` are the HUD's.
pub fn draw(atlas: &HudAtlas, batch: &mut HudBatch, layout: &Layout, shown: &Shown) {
    let scale = batch.scale;
    let density = density([batch.width * scale, batch.height * scale]);
    // Points to the batch's GUI px.
    let k = density / scale;
    let mut icons: Vec<([[f32; 2]; 3], u32)> = Vec::new();
    let tri = |icons: &mut Vec<_>, c: [f32; 2], size: f32, dir: [f32; 2], color: u32| {
        // An arrowhead pointing along `dir` (unit), `size` points long.
        let [dx, dy] = dir;
        let (px, py) = (-dy, dx);
        let tip = [c[0] + dx * size * 0.55, c[1] + dy * size * 0.55];
        let a = [
            c[0] - dx * size * 0.45 + px * size * 0.55,
            c[1] - dy * size * 0.45 + py * size * 0.55,
        ];
        let b = [
            c[0] - dx * size * 0.45 - px * size * 0.55,
            c[1] - dy * size * 0.45 - py * size * 0.55,
        ];
        icons.push((
            [
                [tip[0] * k, tip[1] * k],
                [a[0] * k, a[1] * k],
                [b[0] * k, b[1] * k],
            ],
            color,
        ));
    };
    let mut ui = Ui::new(atlas, batch, (-1.0, -1.0));
    let s = sizes();
    let tile = |ui: &mut Ui, c: [f32; 2], half: f32, held: bool| {
        ui.rounded(
            c[0] - half,
            c[1] - half,
            half * 2.0,
            half * 2.0,
            6.0,
            if held { TILE_HELD } else { TILE },
        );
        ui.border(c[0] - half, c[1] - half, half * 2.0, half * 2.0, 6.0, RIM);
    };
    let label = |ui: &mut Ui, c: [f32; 2], half: f32, text: &str, ink: u32| {
        let size = if half > 16.0 {
            s.caption
        } else {
            s.caption * 0.85
        };
        let lw = ui.measure(Face::Medium, size, text);
        ui.text_line(
            Face::Medium,
            size,
            half * 2.0,
            c[0] - lw / 2.0,
            c[1] - half,
            ink,
            text,
        );
    };
    if shown.back_only {
        let t = layout.back_tile();
        tile(&mut ui, t.centre, t.half, false);
        tri(&mut icons, t.centre, t.half * 0.9, [-1.0, 0.0], ICON);
        ui.finish();
        for (corners, color) in icons {
            batch.fill_triangle(atlas, corners, color);
        }
        return;
    }
    let editing = shown.editing.is_some();
    if editing {
        // The editor: the world dimmed behind the controls.
        ui.rounded(0.0, 0.0, layout.w, layout.h, 0.0, 0x80000000);
    }
    // Movement.
    match layout.scheme {
        Scheme::DPad => {
            let [cx, cy] = layout.dpad;
            let h = layout.dpad_half;
            let step = h * 2.0 + 4.0;
            let [px, py] = shown.dpad;
            for dir in [[0.0f32, -1.0f32], [0.0, 1.0], [-1.0, 0.0], [1.0, 0.0]] {
                let c = [cx + dir[0] * step, cy + dir[1] * step];
                let held = (dir[0] != 0.0 && dir[0] == px) || (dir[1] != 0.0 && dir[1] == py);
                tile(&mut ui, c, h, held);
                tri(
                    &mut icons,
                    c,
                    h * 0.9,
                    dir,
                    if held { ICON_HELD } else { ICON },
                );
            }
            // Walking forward, Bedrock adds the two forward diagonals.
            if py < 0.0 {
                for side in [-1.0f32, 1.0] {
                    let c = [cx + side * step, cy - step];
                    let held = px == side;
                    tile(&mut ui, c, h * 0.8, held);
                    let d = std::f32::consts::FRAC_1_SQRT_2;
                    tri(
                        &mut icons,
                        c,
                        h * 0.8,
                        [side * d, -d],
                        if held { ICON_HELD } else { ICON },
                    );
                }
            }
            // The middle: Sneak.
            tile(&mut ui, [cx, cy], h * 0.8, shown.sneaking);
            let ink = if shown.sneaking { ICON_HELD } else { ICON };
            ui.rounded(cx - h * 0.3, cy - h * 0.3, h * 0.6, h * 0.6, h * 0.3, ink);
        }
        _ => {
            let (base, knob) = shown
                .stick
                .unwrap_or((layout.stick_home, layout.stick_home));
            let r = layout.stick_radius;
            ui.rounded(base[0] - r, base[1] - r, r * 2.0, r * 2.0, r, TILE);
            ui.border(base[0] - r, base[1] - r, r * 2.0, r * 2.0, r, RIM);
            let kr = r * 0.42;
            let knob_color = if shown.stick.is_some() {
                0xD0FFFFFF
            } else {
                0x88FFFFFF
            };
            ui.rounded(
                knob[0] - kr,
                knob[1] - kr,
                kr * 2.0,
                kr * 2.0,
                kr,
                knob_color,
            );
        }
    }
    let selected = shown.editing.and_then(|(selected, _)| selected);
    for t in &layout.tiles {
        if t.hidden && !editing {
            continue;
        }
        if t.control == Control::Edit && editing {
            continue;
        }
        let held = shown.held.contains(&t.control)
            || (t.control == Control::Sneak && shown.sneaking)
            || (t.control == Control::Sprint && shown.sprinting);
        let ink = if held {
            ICON_HELD
        } else if t.hidden {
            0x66FFFFFF
        } else {
            ICON
        };
        let [cx, cy] = t.centre;
        let h = t.half;
        tile(&mut ui, t.centre, h, held);
        if selected == Some(t.control) {
            ui.border(
                cx - h - 3.0,
                cy - h - 3.0,
                h * 2.0 + 6.0,
                h * 2.0 + 6.0,
                8.0,
                SELECTED,
            );
        }
        match t.control {
            Control::Jump => tri(&mut icons, t.centre, h * 0.9, [0.0, -1.0], ink),
            Control::Sneak => {
                ui.rounded(
                    cx - h * 0.35,
                    cy - h * 0.35,
                    h * 0.7,
                    h * 0.7,
                    h * 0.35,
                    ink,
                );
            }
            Control::Sprint => {
                tri(&mut icons, [cx - h * 0.18, cy], h * 0.6, [1.0, 0.0], ink);
                tri(&mut icons, [cx + h * 0.22, cy], h * 0.6, [1.0, 0.0], ink);
            }
            Control::Pause => {
                ui.rounded(cx - h * 0.4, cy - h * 0.45, h * 0.28, h * 0.9, 2.0, ink);
                ui.rounded(cx + h * 0.12, cy - h * 0.45, h * 0.28, h * 0.9, 2.0, ink);
            }
            Control::Inventory => {
                for i in -1..=1 {
                    let x = cx + i as f32 * h * 0.5;
                    ui.rounded(
                        x - h * 0.13,
                        cy - h * 0.13,
                        h * 0.26,
                        h * 0.26,
                        h * 0.13,
                        ink,
                    );
                }
            }
            Control::Back => tri(&mut icons, t.centre, h * 0.9, [-1.0, 0.0], ink),
            Control::Debug => label(&mut ui, t.centre, h, "F3", ink),
            Control::Perspective => label(&mut ui, t.centre, h, "F5", ink),
            Control::Key(i) => label(&mut ui, t.centre, h, KEYS.get(i).map_or("?", |k| k.0), ink),
            Control::Chat | Control::Attack | Control::Use | Control::Edit => {
                let key = match t.control {
                    Control::Chat => "horizon.touch.chat",
                    Control::Attack => "horizon.touch.attack",
                    Control::Use => "horizon.touch.use",
                    _ => "horizon.touch.edit",
                };
                label(&mut ui, t.centre, h, &crate::lang::get(key), ink);
            }
        }
    }
    if let Some((selected, config)) = shown.editing {
        for (action, [x, y, w, h], key) in editor_buttons(layout, selected) {
            let primary = action == EditAction::Done;
            ui.rounded(
                x,
                y,
                w,
                h,
                h / 2.0,
                if primary { 0xFF5B3FD6 } else { 0xE0201A33 },
            );
            let mut text = crate::lang::get(key);
            if action == EditAction::Mode
                && let Some(control) = selected
            {
                text = crate::lang::get(if config.hold(control) {
                    "horizon.touch.edit.mode_hold"
                } else {
                    "horizon.touch.edit.mode_toggle"
                });
            }
            if action == EditAction::Hide
                && let Some(control) = selected
                && layout.tile(control).is_some_and(|t| t.hidden)
            {
                text = crate::lang::get("horizon.touch.edit.show");
            }
            let lw = ui.measure(Face::Medium, s.caption, &text);
            ui.text_line(
                Face::Medium,
                s.caption,
                h,
                x + (w - lw) / 2.0,
                y,
                0xFFFFFFFF,
                &text,
            );
        }
        let hint = crate::lang::get("horizon.touch.edit.hint");
        let lw = ui.measure(Face::Regular, s.caption, &hint);
        ui.text_line(
            Face::Regular,
            s.caption,
            20.0,
            (layout.w - lw) / 2.0,
            layout.h * 0.32 - 28.0,
            0xFFFFFFFF,
            &hint,
        );
    }
    ui.finish();
    for (corners, color) in icons {
        batch.fill_triangle(atlas, corners, color);
    }
}
