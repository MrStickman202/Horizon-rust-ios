//! On-screen controls for playing by touch (the iOS app), laid out and drawn after Bedrock
//! Edition's: translucent square tiles with white icons, a joystick or a D-pad at the bottom
//! left, Jump at the bottom right, Pause and Chat at the top. Where they are, which one a
//! finger is on, and how they look; the game (`touch_play`) decides when they show and what
//! they do. Positions are in screen points: physical pixels over [`density`].
use crate::hud::{HudAtlas, HudBatch};
use crate::ui_theme::{Face, Ui, sizes};

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
}

const FILE: &str = "horizon-touch.txt";

impl Scheme {
    pub const ALL: [Scheme; 3] = [Scheme::Joystick, Scheme::Crosshair, Scheme::DPad];
    fn key(self) -> &'static str {
        match self {
            Scheme::Joystick => "joystick",
            Scheme::Crosshair => "crosshair",
            Scheme::DPad => "dpad",
        }
    }
    /// Its name in the current language.
    pub fn name(self) -> String {
        crate::lang::get(match self {
            Scheme::Joystick => "horizon.touch.scheme.joystick",
            Scheme::Crosshair => "horizon.touch.scheme.crosshair",
            Scheme::DPad => "horizon.touch.scheme.dpad",
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
        let key = text.lines().find_map(|l| l.trim().strip_prefix("scheme:")).unwrap_or("");
        Self::ALL.into_iter().find(|s| s.key() == key).unwrap_or_default()
    }
    pub fn save(self, dir: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(dir.join(FILE), format!("scheme:{}\n", self.key()))
    }
}

/// Physical pixels per screen point: `ui_theme::Ui`'s own unit, so the controls are the same
/// size on every screen and drawn where they are hit.
pub fn density(physical: [f32; 2]) -> f32 {
    (physical[0] / 1040.0).min(physical[1] / 700.0).clamp(1.0, 4.0)
}

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

/// A tile: what it does, its centre and half its side, in points.
#[derive(Clone, Copy, Debug)]
pub struct Tile {
    pub control: Control,
    pub centre: [f32; 2],
    pub half: f32,
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
    /// Where a finger that comes down takes the joystick (the joystick schemes).
    pub stick_zone: [f32; 4],
    /// The joystick's resting place and its reach.
    pub stick_home: [f32; 2],
    pub stick_radius: f32,
    /// The D-pad's centre and its tiles' half side (the D-pad scheme).
    pub dpad: [f32; 2],
    pub dpad_half: f32,
    pub tiles: Vec<Tile>,
    /// The hotbar, in points.
    pub hotbar: [f32; 4],
}

impl Layout {
    /// `gui`: the GUI scale over [`density`], for the hotbar (vanilla's 182 × 22 GUI px).
    pub fn new(w: f32, h: f32, gui: f32, scheme: Scheme) -> Self {
        let m = 28.0;
        let big = 38.0;
        let small = 28.0;
        let top = 22.0;
        let jump = [w - m - big, h - m - big - 30.0];
        let mut tiles = vec![
            Tile { control: Control::Jump, centre: jump, half: big },
            Tile { control: Control::Pause, centre: [w / 2.0 + 90.0, m + top], half: top },
            Tile { control: Control::Chat, centre: [w / 2.0 + 90.0 + top * 2.0 + 10.0, m + top], half: top },
        ];
        let hotbar_w = 182.0 * gui;
        let hotbar = [((w - hotbar_w) / 2.0), h - 22.0 * gui, hotbar_w, 22.0 * gui];
        // Inventory: the "…" tile at the hotbar's right end, as on Bedrock.
        tiles.push(Tile {
            control: Control::Inventory,
            centre: [hotbar[0] + hotbar_w + 11.0 * gui + 4.0, hotbar[1] + 11.0 * gui],
            half: 11.0 * gui,
        });
        if scheme != Scheme::DPad {
            tiles.push(Tile {
                control: Control::Sneak,
                centre: [jump[0] - big - small - 16.0, jump[1] + big - small],
                half: small,
            });
        }
        if scheme == Scheme::Crosshair {
            tiles.push(Tile {
                control: Control::Attack,
                centre: [jump[0], jump[1] - big - small - 16.0],
                half: small,
            });
            tiles.push(Tile {
                control: Control::Use,
                centre: [jump[0] - big - small - 16.0, jump[1] - big - small - 16.0],
                half: small,
            });
        }
        let dpad_half = 30.0;
        let dpad = [m + dpad_half * 3.0 + 8.0, h - m - dpad_half * 3.0 - 8.0];
        Self {
            scheme,
            stick_zone: [0.0, h * 0.35, w * 0.45, h * 0.65],
            stick_home: [m + 90.0, h - m - 90.0],
            stick_radius: 70.0,
            dpad,
            dpad_half,
            tiles,
            hotbar,
        }
    }

    /// The tile under `at`, if any (a little larger than drawn, for fingers).
    pub fn tile_at(&self, at: [f32; 2]) -> Option<Control> {
        self.tiles.iter().find(|t| t.contains(at, 6.0)).map(|t| t.control)
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
}

/// What the controls show this frame.
pub struct Shown<'a> {
    /// The joystick: its base and its knob, while a finger holds it.
    pub stick: Option<([f32; 2], [f32; 2])>,
    /// Where the D-pad is pushed (`[right, down]`).
    pub dpad: [f32; 2],
    pub held: &'a [Control],
    pub sneaking: bool,
}

// Bedrock's tiles: dark glass, a light rim, white icons; pressed, lighter.
const TILE: u32 = 0x66000000;
const TILE_HELD: u32 = 0xA0FFFFFF;
const RIM: u32 = 0x55FFFFFF;
const ICON: u32 = 0xEEFFFFFF;
const ICON_HELD: u32 = 0xEE1B1530;

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
        let a = [c[0] - dx * size * 0.45 + px * size * 0.55, c[1] - dy * size * 0.45 + py * size * 0.55];
        let b = [c[0] - dx * size * 0.45 - px * size * 0.55, c[1] - dy * size * 0.45 - py * size * 0.55];
        icons.push(([[tip[0] * k, tip[1] * k], [a[0] * k, a[1] * k], [b[0] * k, b[1] * k]], color));
    };
    let mut ui = Ui::new(atlas, batch, (-1.0, -1.0));
    let s = sizes();
    let tile = |ui: &mut Ui, c: [f32; 2], half: f32, held: bool| {
        ui.rounded(c[0] - half, c[1] - half, half * 2.0, half * 2.0, 6.0, if held { TILE_HELD } else { TILE });
        ui.border(c[0] - half, c[1] - half, half * 2.0, half * 2.0, 6.0, RIM);
    };
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
                tile(ui_ref(&mut ui), c, h, held);
                tri(&mut icons, c, h * 0.9, dir, if held { ICON_HELD } else { ICON });
            }
            // Walking forward, Bedrock adds the two forward diagonals.
            if py < 0.0 {
                for side in [-1.0f32, 1.0] {
                    let c = [cx + side * step, cy - step];
                    let held = px == side;
                    tile(ui_ref(&mut ui), c, h * 0.8, held);
                    let d = std::f32::consts::FRAC_1_SQRT_2;
                    tri(&mut icons, c, h * 0.8, [side * d, -d], if held { ICON_HELD } else { ICON });
                }
            }
            // The middle: Sneak.
            tile(ui_ref(&mut ui), [cx, cy], h * 0.8, shown.sneaking);
            ui.rounded(cx - h * 0.3, cy - h * 0.3, h * 0.6, h * 0.6, h * 0.3, if shown.sneaking { ICON_HELD } else { ICON });
        }
        _ => {
            let (base, knob) = shown.stick.unwrap_or((layout.stick_home, layout.stick_home));
            let r = layout.stick_radius;
            ui.rounded(base[0] - r, base[1] - r, r * 2.0, r * 2.0, r, TILE);
            ui.border(base[0] - r, base[1] - r, r * 2.0, r * 2.0, r, RIM);
            let kr = r * 0.42;
            let knob_color = if shown.stick.is_some() { 0xD0FFFFFF } else { 0x88FFFFFF };
            ui.rounded(knob[0] - kr, knob[1] - kr, kr * 2.0, kr * 2.0, kr, knob_color);
        }
    }
    for t in &layout.tiles {
        let held = shown.held.contains(&t.control) || (t.control == Control::Sneak && shown.sneaking);
        let ink = if held { ICON_HELD } else { ICON };
        let [cx, cy] = t.centre;
        let h = t.half;
        tile(ui_ref(&mut ui), t.centre, h, held);
        match t.control {
            Control::Jump => tri(&mut icons, t.centre, h * 0.9, [0.0, -1.0], ink),
            Control::Sneak => {
                ui.rounded(cx - h * 0.35, cy - h * 0.35, h * 0.7, h * 0.7, h * 0.35, ink);
            }
            Control::Pause => {
                ui.rounded(cx - h * 0.4, cy - h * 0.45, h * 0.28, h * 0.9, 2.0, ink);
                ui.rounded(cx + h * 0.12, cy - h * 0.45, h * 0.28, h * 0.9, 2.0, ink);
            }
            Control::Inventory => {
                for i in -1..=1 {
                    let x = cx + i as f32 * h * 0.5;
                    ui.rounded(x - h * 0.13, cy - h * 0.13, h * 0.26, h * 0.26, h * 0.13, ink);
                }
            }
            Control::Chat | Control::Attack | Control::Use => {
                let key = match t.control {
                    Control::Chat => "horizon.touch.chat",
                    Control::Attack => "horizon.touch.attack",
                    _ => "horizon.touch.use",
                };
                let label = crate::lang::get(key);
                let size = s.caption;
                let lw = ui.measure(Face::Medium, size, &label);
                ui.text_line(Face::Medium, size, h * 2.0, cx - lw / 2.0, cy - h, ink, &label);
            }
        }
    }
    ui.finish();
    for (corners, color) in icons {
        batch.fill_triangle(atlas, corners, color);
    }
}

fn ui_ref<'u, 'a>(ui: &'u mut Ui<'a>) -> &'u mut Ui<'a> {
    ui
}
