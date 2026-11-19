//! iOS: the app's home screen, before the title (the game's `ios_launcher` shows it while the
//! resources wait). It follows the desktop launcher's glass look (`horizon_theme::glass`, the
//! night sky): a top bar with the account, a hero with Play and the online or offline choice,
//! and a card listing what is in the app's folder (worlds, mods, saved servers, packs,
//! screenshots, the log). Settings holds the offline name, the Microsoft account and the touch
//! controls. Only what works on iOS is here: no shop, wardrobe or version picker.
use crate::touch_controls::Scheme;
use crate::ui_theme::{Face, Ui, color, sizes};
use horizon_theme::night::NIGHT;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Window;

const NAME_MAX: usize = 16;
/// The page's outer margin, in dp.
const MARGIN: f32 = 28.0;
/// A list row, and a log line.
const ROW: f32 = 50.0;
const LOG_ROW: f32 = 18.0;
/// How far a finger moves before a press on the list scrolls instead of tapping.
const DRAG_SLOP: f32 = 8.0;
/// The log tab shows the end of `latest.log`.
const LOG_LINES: usize = 400;
/// `horizon.txt`: whether a signed-in player plays online (as the account) or offline.
const PLAY_ONLINE: &str = "ios_play_online";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Worlds,
    Mods,
    Servers,
    Packs,
    Shaders,
    Screenshots,
    Log,
}

impl Tab {
    const ALL: [Tab; 7] = [
        Tab::Worlds,
        Tab::Mods,
        Tab::Servers,
        Tab::Packs,
        Tab::Shaders,
        Tab::Screenshots,
        Tab::Log,
    ];
    fn label(self) -> &'static str {
        match self {
            Tab::Worlds => "horizon.ios.home.tab.worlds",
            Tab::Mods => "horizon.ios.home.tab.mods",
            Tab::Servers => "horizon.ios.home.tab.servers",
            Tab::Packs => "horizon.ios.home.tab.packs",
            Tab::Shaders => "horizon.ios.home.tab.shaders",
            Tab::Screenshots => "horizon.ios.home.tab.screenshots",
            Tab::Log => "horizon.ios.home.tab.log",
        }
    }
    fn empty(self) -> &'static str {
        match self {
            Tab::Worlds => "horizon.ios.home.empty.worlds",
            Tab::Mods => "horizon.ios.home.empty.mods",
            Tab::Servers => "horizon.ios.home.empty.servers",
            Tab::Packs => "horizon.ios.home.empty.packs",
            Tab::Shaders => "horizon.ios.home.empty.shaders",
            Tab::Screenshots => "horizon.ios.home.empty.screenshots",
            Tab::Log => "horizon.ios.home.empty.log",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Nav(Page),
    Account,
    Play,
    /// The online or offline choice (true: online).
    Mode(bool),
    /// Signed out: the offline note opens Settings to sign in.
    OfflineNote,
    Tab(Tab),
    /// A mod row's switch.
    Toggle(usize),
    Name,
    SignIn,
    Touch,
}

/// Whether a home screen holds the game's loading back, and whether Play was pressed.
static GATE: std::sync::Mutex<(bool, bool)> = std::sync::Mutex::new((false, false));
static OPENED: std::sync::Condvar = std::sync::Condvar::new();

/// The resource loader's first step: while a home screen shows, it waits for Play. Nothing
/// loads (no CPU, no heat) until the player starts the game.
pub fn wait_for_play() {
    let mut gate = GATE.lock().unwrap_or_else(|e| e.into_inner());
    while gate.0 && !gate.1 {
        gate = OPENED.wait(gate).unwrap_or_else(|e| e.into_inner());
    }
}

fn hold_loading() {
    GATE.lock().unwrap_or_else(|e| e.into_inner()).0 = true;
}

fn open_gate() {
    GATE.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
    OPENED.notify_all();
}

/// One row of the card's list.
#[derive(Clone, Debug, Default)]
struct Row {
    title: String,
    detail: String,
    side: String,
    /// A mod's id and whether it is on: the row has a switch.
    toggle: Option<(String, bool)>,
}

/// What the home screen read from the app's folder.
#[derive(Default)]
struct Library {
    worlds: usize,
    mods_on: usize,
    rows: Vec<Row>,
}

/// The list's place on screen and how far it scrolls, from the last frame drawn (the events
/// need them in dp).
#[derive(Clone, Copy, Default)]
struct Metrics {
    density: f32,
    list: [f32; 4],
    max_scroll: f32,
}

pub struct Launcher {
    name: String,
    /// The name being typed, while the field is open.
    editing: Option<String>,
    /// The pointer in physical pixels.
    pub pointer: (f64, f64),
    /// The Microsoft account: kept in the Keychain, or a sign-in under way.
    account: crate::ms_sign_in::SignIn,
    /// The on-screen controls' layout (`touch_controls`).
    scheme: Scheme,
    page: Page,
    tab: Tab,
    /// A signed-in player plays online (as the account); off, offline with the name.
    online: bool,
    library: Library,
    /// The list's scroll, in dp.
    scroll: f32,
    /// A press under way: where it began (physical px) and whether it became a drag.
    press: Option<((f64, f64), bool)>,
    metrics: Cell<Metrics>,
}

fn inside([x, y, w, h]: [f32; 4], (cx, cy): (f32, f32)) -> bool {
    cx >= x && cx < x + w && cy >= y && cy < y + h
}

fn dir() -> PathBuf {
    crate::logging::player_dir()
}

fn rgb(c: u32) -> u32 {
    color::hex(c, 255)
}

fn rgba(c: u32, a: f32) -> u32 {
    color::hex(c, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
}

fn glass() -> &'static horizon_theme::glass::Glass {
    static GLASS: std::sync::OnceLock<horizon_theme::glass::Glass> = std::sync::OnceLock::new();
    GLASS.get_or_init(horizon_theme::glass::Glass::default_look)
}

/// `a` to `b` by `t`, channel by channel.
fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 255) as f32, ((b >> s) & 255) as f32);
        ((x + (y - x) * t).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// "Today", "Yesterday", "3 days ago".
fn when(time: SystemTime) -> String {
    let days = SystemTime::now()
        .duration_since(time)
        .map_or(0, |d| d.as_secs() / 86_400);
    match days {
        0 => crate::lang::get("horizon.ios.home.when.today"),
        1 => crate::lang::get("horizon.ios.home.when.yesterday"),
        n => crate::lang::tr("horizon.ios.home.when.days", &[&n]),
    }
}

fn size_text(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        let mb = format!("{:.1}", bytes as f64 / (1024.0 * 1024.0));
        crate::lang::tr("horizon.ios.home.size_mb", &[&mb])
    } else {
        let kb = bytes.div_ceil(1024).max(1);
        crate::lang::tr("horizon.ios.home.size_kb", &[&kb])
    }
}

/// The files and folders in `dir`, newest first: (name, is a folder, size, modified).
fn entries(dir: &Path) -> Vec<(String, bool, u64, SystemTime)> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let meta = e.metadata().ok()?;
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            Some((name, meta.is_dir(), meta.len(), modified))
        })
        .collect();
    out.sort_by(|a, b| b.3.cmp(&a.3));
    out
}

/// The installed content mods (`horizon-mods/<id>.hmod`) and whether each is on.
fn mods(dir: &Path) -> Vec<(String, bool, u64)> {
    let list = crate::mod_list::ModList::load(dir);
    let mut out: Vec<_> = entries(&dir.join(crate::mod_list::DIR))
        .into_iter()
        .filter_map(|(name, folder, size, _)| {
            let id = name.strip_suffix(".hmod").filter(|_| !folder)?.to_owned();
            let on = list.enabled.contains(&id);
            Some((id, on, size))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn game_mode(mode: i32, hardcore: bool) -> String {
    if hardcore {
        return crate::lang::get("horizon.ios.home.mode_hardcore");
    }
    crate::lang::get(match mode {
        1 => "horizon.ios.home.mode_creative",
        2 => "horizon.ios.home.mode_adventure",
        3 => "horizon.ios.home.mode_spectator",
        _ => "horizon.ios.home.mode_survival",
    })
}

impl Library {
    fn scan(tab: Tab) -> Library {
        let dir = dir();
        let worlds = crate::title::list_worlds(&dir.join("saves"));
        let installed = mods(&dir);
        let mods_on = installed.iter().filter(|m| m.1).count();
        let rows = match tab {
            Tab::Worlds => worlds
                .iter()
                .map(|w| Row {
                    title: w.name.clone(),
                    detail: game_mode(w.game_mode, w.hardcore),
                    side: when(w.modified),
                    toggle: None,
                })
                .collect(),
            Tab::Mods => installed
                .into_iter()
                .map(|(id, on, size)| Row {
                    title: id.clone(),
                    detail: size_text(size),
                    side: String::new(),
                    toggle: Some((id, on)),
                })
                .collect(),
            Tab::Servers => {
                let file = crate::multiplayer::servers_file(&dir.join("saves"));
                horizon_protocol::servers::load(&file)
                    .into_iter()
                    .map(|s| Row {
                        title: s.name,
                        detail: s.ip,
                        ..Row::default()
                    })
                    .collect()
            }
            Tab::Packs | Tab::Shaders => {
                let folder = if tab == Tab::Packs {
                    "resourcepacks"
                } else {
                    "shaderpacks"
                };
                entries(&dir.join(folder))
                    .into_iter()
                    .map(|(name, is_dir, size, modified)| Row {
                        title: name.strip_suffix(".zip").unwrap_or(&name).to_owned(),
                        detail: if is_dir {
                            String::new()
                        } else {
                            size_text(size)
                        },
                        side: when(modified),
                        toggle: None,
                    })
                    .collect()
            }
            Tab::Screenshots => entries(&dir.join("screenshots"))
                .into_iter()
                .filter(|(name, is_dir, ..)| !is_dir && name.ends_with(".png"))
                .map(|(name, _, size, modified)| Row {
                    title: name,
                    detail: size_text(size),
                    side: when(modified),
                    toggle: None,
                })
                .collect(),
            Tab::Log => {
                let file = crate::logging::logs_dir(&dir).join("latest.log");
                let text = std::fs::read_to_string(file).unwrap_or_default();
                let lines: Vec<&str> = text.lines().collect();
                lines[lines.len().saturating_sub(LOG_LINES)..]
                    .iter()
                    .map(|l| Row {
                        title: (*l).to_owned(),
                        ..Row::default()
                    })
                    .collect()
            }
        };
        Library {
            worlds: worlds.len(),
            mods_on,
            rows,
        }
    }
}

/// `text` cut to fit `room` dp, with an ellipsis when it was cut.
fn clip(ui: &Ui, face: Face, size: f32, text: &str, room: f32) -> String {
    if room <= 0.0 {
        return String::new();
    }
    if ui.measure(face, size, text) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if ui.measure(face, size, &candidate) <= room {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

/// Where everything is, in dp, for one screen size: drawing and taps share it.
struct Layout {
    nav: Vec<(Page, [f32; 4])>,
    account: [f32; 4],
    // Home.
    hero_y: f32,
    play: [f32; 4],
    mode: [f32; 4],
    card: [f32; 4],
    tabs: Vec<(Tab, [f32; 4])>,
    list: [f32; 4],
    // Settings.
    column: [f32; 4],
    name_field: [f32; 4],
    sign_in: [f32; 4],
    touch: [f32; 4],
    files_y: f32,
}

impl Launcher {
    /// The home screen on iOS; elsewhere the launcher is a program of its own.
    pub fn at_start() -> Option<Self> {
        if !cfg!(target_os = "ios") {
            return None;
        }
        hold_loading();
        let online = crate::ui_theme::preference(&dir(), PLAY_ONLINE).is_none_or(|v| v != "false");
        Some(Self {
            name: crate::offline_name::load_or_create(&dir()),
            editing: None,
            pointer: (0.0, 0.0),
            account: crate::ms_sign_in::SignIn::at_start(),
            scheme: Scheme::load(&dir()),
            page: Page::Home,
            tab: Tab::Worlds,
            online,
            library: Library::scan(Tab::Worlds),
            scroll: 0.0,
            press: None,
            metrics: Cell::new(Metrics::default()),
        })
    }

    fn signed_in(&self) -> Option<crate::ms_sign_in::Account> {
        match self.account.state() {
            crate::ms_sign_in::State::SignedIn(account) => Some(account),
            _ => None,
        }
    }

    /// The account chip's two lines: who plays, and how.
    fn chip_text(&self) -> (String, String) {
        match self.signed_in() {
            Some(account) if self.online => (
                account.name,
                crate::lang::get("horizon.ios.home.account_ms"),
            ),
            _ => (
                self.name.clone(),
                crate::lang::get("horizon.ios.home.account_offline"),
            ),
        }
    }

    fn layout(&self, ui: &Ui) -> Layout {
        let s = sizes();
        let (w, h) = (ui.w, ui.h);
        let bar_y = 18.0;
        // The top bar: the pages on the left, the account on the right (the name is the hero's).
        let mut x = MARGIN - 14.0;
        let mut nav = Vec::new();
        for (page, key) in [
            (Page::Home, "horizon.ios.home.nav.home"),
            (Page::Settings, "horizon.ios.home.nav.settings"),
        ] {
            let width = ui.measure(Face::Medium, s.ui, &crate::lang::get(key)) + 28.0;
            nav.push((page, [x, bar_y + 6.0, width, 32.0]));
            x += width + 6.0;
        }
        let (name, sub) = self.chip_text();
        let text_w = ui
            .measure(Face::Medium, s.ui, &name)
            .max(ui.measure(Face::Regular, s.caption, &sub))
            .min(220.0);
        let chip_w = 6.0 + 32.0 + 10.0 + text_w + 18.0;
        let account = [w - MARGIN - chip_w, bar_y, chip_w, 44.0];

        // Home: the hero and the card.
        let hero_y = bar_y + 44.0 + (h * 0.06).clamp(18.0, 56.0);
        let play_y = hero_y + 64.0 + 30.0;
        let play = [MARGIN + 4.0, play_y, 200.0, 54.0];
        let mode = [play[0] + play[2] + 16.0, play_y + 5.0, 240.0, 44.0];
        let card_y = play_y + 54.0 + 28.0;
        let card = [
            MARGIN,
            card_y,
            w - 2.0 * MARGIN,
            (h - MARGIN - card_y).max(120.0),
        ];
        let mut tabs = Vec::new();
        let mut tx = card[0] + 20.0;
        for tab in Tab::ALL {
            let width = ui.measure(Face::Medium, s.ui, &crate::lang::get(tab.label())) + 20.0;
            tabs.push((tab, [tx, card_y + 12.0, width, 36.0]));
            tx += width + 4.0;
        }
        let list = [
            card[0] + 20.0,
            card_y + 58.0,
            card[2] - 40.0,
            card[3] - 58.0 - 12.0,
        ];

        // Settings: one column.
        let col_w = 560.0f32.min(w - 2.0 * MARGIN);
        let col_x = ((w - col_w) / 2.0).round();
        let col_y = bar_y + 44.0 + 28.0;
        let control = s.control_large.max(40.0);
        let section = 26.0 + control + 34.0;
        let pad = 24.0;
        let name_field = [col_x + pad, col_y + pad + 26.0, col_w - 2.0 * pad, control];
        let sign_in = [
            col_x + pad,
            name_field[1] + section,
            col_w - 2.0 * pad,
            control,
        ];
        let touch = [
            col_x + pad,
            sign_in[1] + section,
            col_w - 2.0 * pad,
            control,
        ];
        let files_y = touch[1] + control + 34.0;
        let column = [col_x, col_y, col_w, files_y + 26.0 + 44.0 + pad - col_y];
        Layout {
            nav,
            account,
            hero_y,
            play,
            mode,
            card,
            tabs,
            list,
            column,
            name_field,
            sign_in,
            touch,
            files_y,
        }
    }

    /// The night sky the launcher's glass sits on: the theme's gradient and a field of stars.
    fn draw_sky(ui: &mut Ui) {
        let sky = NIGHT.sky;
        let bands = 48;
        let band = ui.h / bands as f32;
        for i in 0..bands {
            let t = i as f32 / (bands - 1) as f32;
            // Night at the top, the violet glow only near the bottom edge.
            let c = if t < 0.6 {
                mix(sky[0], sky[1], t / 0.6)
            } else if t < 0.9 {
                mix(sky[1], sky[2], (t - 0.6) / 0.3)
            } else {
                mix(sky[2], sky[3], ((t - 0.9) / 0.1) * 0.45)
            };
            ui.rounded(0.0, i as f32 * band, ui.w, band + 1.0, 0.0, rgb(c));
        }
        // Stars where they'd be: a fixed field (a cheap hash), denser near the top.
        let mut seed: u32 = 0x9e37_79b9;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed & 0xffff) as f32 / 65535.0
        };
        let count = ((ui.w * ui.h) / 7000.0) as usize;
        for _ in 0..count {
            let x = next() * ui.w;
            let y = next().powf(1.6) * ui.h * 0.8;
            let size = 1.0 + next() * 1.4;
            let alpha = 0.25 + next() * 0.6;
            ui.rounded(x, y, size, size, size / 2.0, rgba(NIGHT.art.star, alpha));
        }
    }

    fn plate(ui: &mut Ui, [x, y, w, h]: [f32; 4], radius: f32) {
        let g = glass();
        ui.rounded(x, y, w, h, radius, rgba(g.plate, g.plate_alpha.max(0.18)));
        ui.border(
            x,
            y,
            w,
            h,
            radius,
            rgba(NIGHT.stroke, NIGHT.stroke_alpha + 0.04),
        );
    }

    pub fn draw(&self, ui: &mut Ui, ready: bool) {
        let _ = ready;
        let l = self.layout(ui);
        let s = sizes();
        let ink = rgb(NIGHT.ink);
        Self::draw_sky(ui);

        // The top bar.
        for (page, rect) in &l.nav {
            let key = match page {
                Page::Home => "horizon.ios.home.nav.home",
                Page::Settings => "horizon.ios.home.nav.settings",
            };
            let active = *page == self.page;
            let [x, y, w, h] = *rect;
            if active {
                ui.rounded(x, y, w, h, h / 2.0, rgba(NIGHT.ink, 0.12));
            } else if inside(*rect, ui.cursor) {
                ui.rounded(x, y, w, h, h / 2.0, rgba(NIGHT.ink, 0.06));
            }
            let text = crate::lang::get(key);
            let colour = if active { ink } else { rgb(NIGHT.ink_soft) };
            ui.text_line(Face::Medium, s.ui, h, x + 14.0, y, colour, &text);
        }
        self.draw_chip(ui, l.account);

        match self.page {
            Page::Home => self.draw_home(ui, &l),
            Page::Settings => self.draw_settings(ui, &l),
        }
    }

    fn draw_chip(&self, ui: &mut Ui, rect: [f32; 4]) {
        let s = sizes();
        let [x, y, w, h] = rect;
        Self::plate(ui, rect, h / 2.0);
        if inside(rect, ui.cursor) {
            ui.rounded(x, y, w, h, h / 2.0, rgba(NIGHT.ink, 0.06));
        }
        let (name, sub) = self.chip_text();
        let online = self.online && self.signed_in().is_some();
        // A monogram: the accent for an account, a plate for an offline player.
        let (ax, ay, ad) = (x + 6.0, y + 6.0, 32.0);
        let avatar = if online {
            rgb(glass().accent)
        } else {
            rgba(NIGHT.ink, 0.16)
        };
        ui.rounded(ax, ay, ad, ad, ad / 2.0, avatar);
        let initial: String = name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default();
        let iw = ui.measure(Face::SemiBold, s.ui, &initial);
        ui.text_line(
            Face::SemiBold,
            s.ui,
            ad,
            ax + ((ad - iw) / 2.0).round(),
            ay,
            rgb(NIGHT.ink),
            &initial,
        );
        let room = w - (ad + 6.0 + 10.0 + 18.0);
        let tx = ax + ad + 10.0;
        let name = clip(ui, Face::Medium, s.ui, &name, room);
        ui.text_line(Face::Medium, s.ui, 20.0, tx, y + 4.0, rgb(NIGHT.ink), &name);
        // A green dot reads "online" before the words do.
        let sub_x = if online {
            ui.rounded(tx, y + 27.0, 6.0, 6.0, 3.0, rgb(NIGHT.success));
            tx + 10.0
        } else {
            tx
        };
        let sub = clip(ui, Face::Regular, s.caption, &sub, room - (sub_x - tx));
        ui.text_line(
            Face::Regular,
            s.caption,
            16.0,
            sub_x,
            y + 22.0,
            rgb(NIGHT.muted),
            &sub,
        );
    }

    fn draw_home(&self, ui: &mut Ui, l: &Layout) {
        let s = sizes();
        let ink = rgb(NIGHT.ink);
        let muted = rgb(NIGHT.muted);
        let soft = rgb(NIGHT.ink_soft);

        // The hero: the name, what's in the folder, Play.
        ui.text_line(Face::SemiBold, 56.0, 64.0, MARGIN, l.hero_y, ink, "Horizon");
        let worlds = match self.library.worlds {
            1 => crate::lang::get("horizon.ios.home.stat.world"),
            n => crate::lang::tr("horizon.ios.home.stat.worlds", &[&n]),
        };
        let mods = match self.library.mods_on {
            1 => crate::lang::get("horizon.ios.home.stat.mod"),
            n => crate::lang::tr("horizon.ios.home.stat.mods", &[&n]),
        };
        let version = crate::lang::tr(
            "horizon.ios.home.version",
            &[&horizon_core::VANILLA_VERSION],
        );
        let stats = format!("{version}  ·  {worlds}  ·  {mods}");
        ui.text_line(
            Face::Regular,
            s.label,
            22.0,
            MARGIN + 2.0,
            l.hero_y + 66.0,
            soft,
            &stats,
        );

        let [px, py, pw, ph] = l.play;
        let accent = glass().accent;
        let plate = if inside(l.play, ui.cursor) {
            mix(accent, 0xffffff, 0.12)
        } else {
            accent
        };
        ui.rounded(px, py, pw, ph, 14.0, rgb(plate));
        let play = crate::lang::get("horizon.ios.home.play");
        let lw = ui.measure(Face::SemiBold, 18.0, &play);
        ui.text_line(
            Face::SemiBold,
            18.0,
            ph,
            px + ((pw - lw) / 2.0).round(),
            py,
            rgb(glass().on_accent),
            &play,
        );

        // Online or offline: a choice once signed in; signed out, a note that leads to sign-in.
        let [mx, my, mw, mh] = l.mode;
        if self.signed_in().is_some() {
            Self::plate(ui, l.mode, mh / 2.0);
            let half = mw / 2.0;
            for (i, (online, key)) in [
                (true, "horizon.ios.home.mode.online"),
                (false, "horizon.ios.home.mode.offline"),
            ]
            .into_iter()
            .enumerate()
            {
                let sx = mx + half * i as f32;
                let active = online == self.online;
                if active {
                    ui.rounded(
                        sx + 4.0,
                        my + 4.0,
                        half - 8.0,
                        mh - 8.0,
                        (mh - 8.0) / 2.0,
                        rgba(NIGHT.ink, 0.16),
                    );
                }
                let text = crate::lang::get(key);
                let tw = ui.measure(Face::Medium, s.ui, &text);
                let colour = if active { ink } else { muted };
                ui.text_line(
                    Face::Medium,
                    s.ui,
                    mh,
                    sx + ((half - tw) / 2.0).round(),
                    my,
                    colour,
                    &text,
                );
            }
        } else {
            let note = crate::lang::get("horizon.ios.home.offline_hint");
            let room = (ui.w - MARGIN - mx).max(0.0);
            let note = clip(ui, Face::Regular, s.label, &note, room);
            let colour = if inside(self.offline_note_rect(ui, l), ui.cursor) {
                ink
            } else {
                soft
            };
            ui.text_line(Face::Regular, s.label, mh, mx, my, colour, &note);
        }

        // The card: tabs over a list.
        let [cx, cy, cw, ch] = l.card;
        ui.rounded(cx, cy, cw, ch, 18.0, rgba(NIGHT.card, NIGHT.card_alpha));
        ui.border(
            cx,
            cy,
            cw,
            ch,
            18.0,
            rgba(NIGHT.stroke, NIGHT.stroke_alpha + 0.04),
        );
        for (tab, rect) in &l.tabs {
            let [x, y, w, h] = *rect;
            if x + w > cx + cw - 12.0 {
                break;
            }
            let active = *tab == self.tab;
            let text = crate::lang::get(tab.label());
            let colour = if active {
                ink
            } else if inside(*rect, ui.cursor) {
                soft
            } else {
                muted
            };
            ui.text_line(Face::Medium, s.ui, h, x + 10.0, y, colour, &text);
            if active {
                ui.rounded(
                    x + 10.0,
                    y + h - 3.0,
                    w - 20.0,
                    2.0,
                    1.0,
                    rgb(glass().accent_soft),
                );
            }
        }
        ui.rounded(
            cx + 1.0,
            cy + 52.0,
            cw - 2.0,
            1.0,
            0.0,
            rgba(NIGHT.stroke, NIGHT.rule_alpha),
        );
        self.draw_list(ui, l.list);
    }

    fn offline_note_rect(&self, ui: &Ui, l: &Layout) -> [f32; 4] {
        let [mx, my, _, mh] = l.mode;
        [mx, my, (ui.w - MARGIN - mx).max(0.0), mh]
    }

    fn row_height(&self) -> f32 {
        if self.tab == Tab::Log { LOG_ROW } else { ROW }
    }

    fn draw_list(&self, ui: &mut Ui, list: [f32; 4]) {
        let s = sizes();
        let [x, y, w, h] = list;
        let row = self.row_height();
        let rows = &self.library.rows;
        let content = rows.len() as f32 * row;
        let max_scroll = (content - h).max(0.0);
        self.metrics.set(Metrics {
            density: ui.density,
            list,
            max_scroll,
        });
        if rows.is_empty() {
            let text = crate::lang::get(self.tab.empty());
            let text = clip(ui, Face::Regular, s.label, &text, w);
            ui.text_line(
                Face::Regular,
                s.label,
                40.0,
                x,
                y + 8.0,
                rgb(NIGHT.muted),
                &text,
            );
            return;
        }
        // Whole rows only: a row at the edge would spill over the card.
        let first = (self.scroll.min(max_scroll) / row).round() as usize;
        let fits = (h / row).floor().max(1.0) as usize;
        for (i, r) in rows.iter().enumerate().skip(first).take(fits) {
            let ry = y + (i - first) as f32 * row;
            if self.tab == Tab::Log {
                let line = clip(ui, Face::Mono, s.micro, &r.title, w - 8.0);
                ui.text_line(Face::Mono, s.micro, row, x, ry, rgb(NIGHT.ink_soft), &line);
                continue;
            }
            if i > first {
                ui.rounded(x, ry, w, 1.0, 0.0, rgba(NIGHT.stroke, NIGHT.rule_alpha));
            }
            let mut right = x + w;
            if let Some((_, on)) = &r.toggle {
                let (sw, sh) = (44.0, 26.0);
                let (sx, sy) = (right - sw, ry + (row - sh) / 2.0);
                let track = if *on {
                    rgb(glass().accent)
                } else {
                    rgba(NIGHT.ink, 0.18)
                };
                ui.rounded(sx, sy, sw, sh, sh / 2.0, track);
                let knob = sh - 6.0;
                let kx = if *on { sx + sw - knob - 3.0 } else { sx + 3.0 };
                ui.rounded(kx, sy + 3.0, knob, knob, knob / 2.0, rgb(0xffffff));
                right = sx - 16.0;
            }
            if !r.side.is_empty() {
                let sw = ui.measure(Face::Regular, s.caption, &r.side);
                ui.text_line(
                    Face::Regular,
                    s.caption,
                    row,
                    right - sw,
                    ry,
                    rgb(NIGHT.muted),
                    &r.side,
                );
                right -= sw + 16.0;
            }
            let room = right - x;
            let title = clip(ui, Face::Medium, s.ui, &r.title, room);
            if r.detail.is_empty() {
                ui.text_line(Face::Medium, s.ui, row, x, ry, rgb(NIGHT.ink), &title);
            } else {
                ui.text_line(
                    Face::Medium,
                    s.ui,
                    22.0,
                    x,
                    ry + 5.0,
                    rgb(NIGHT.ink),
                    &title,
                );
                let detail = clip(ui, Face::Regular, s.caption, &r.detail, room);
                ui.text_line(
                    Face::Regular,
                    s.caption,
                    18.0,
                    x,
                    ry + 27.0,
                    rgb(NIGHT.muted),
                    &detail,
                );
            }
        }
        // Where the list is, when it's longer than the card.
        if max_scroll > 0.0 {
            let bar_h = (h * h / content).max(24.0);
            let at = self.scroll.min(max_scroll) / max_scroll;
            ui.rounded(
                x + w + 10.0,
                y + (h - bar_h) * at,
                3.0,
                bar_h,
                1.5,
                rgba(NIGHT.ink, 0.25),
            );
        }
    }

    fn draw_settings(&self, ui: &mut Ui, l: &Layout) {
        let s = sizes();
        let ink = rgb(NIGHT.ink);
        let muted = rgb(NIGHT.muted);
        let [cx, cy, cw, ch] = l.column;
        ui.rounded(cx, cy, cw, ch, 18.0, rgba(NIGHT.card, NIGHT.card_alpha));
        ui.border(
            cx,
            cy,
            cw,
            ch,
            18.0,
            rgba(NIGHT.stroke, NIGHT.stroke_alpha + 0.04),
        );
        let label = |ui: &mut Ui, rect: [f32; 4], key: &str| {
            let text = crate::lang::get(key);
            ui.text_line(
                Face::Medium,
                s.caption,
                20.0,
                rect[0],
                rect[1] - 26.0,
                muted,
                &text,
            );
        };
        let field = |ui: &mut Ui, rect: [f32; 4], focused: bool| {
            let [x, y, w, h] = rect;
            let hover = inside(rect, ui.cursor);
            ui.rounded(
                x,
                y,
                w,
                h,
                10.0,
                rgba(NIGHT.ink, if hover { 0.12 } else { 0.08 }),
            );
            let edge = if focused {
                rgb(glass().accent_soft)
            } else {
                rgba(NIGHT.stroke, 0.22)
            };
            ui.border(x, y, w, h, 10.0, edge);
        };

        // The offline name.
        label(ui, l.name_field, "horizon.ios.home.name");
        field(ui, l.name_field, self.editing.is_some());
        let [fx, fy, fw, fh] = l.name_field;
        let shown = match &self.editing {
            Some(text) => format!("{text}|"),
            None => self.name.clone(),
        };
        let shown = clip(ui, Face::Regular, s.ui, &shown, fw - 32.0);
        ui.text_line(Face::Regular, s.ui, fh, fx + 16.0, fy, ink, &shown);

        // The Microsoft account.
        label(ui, l.sign_in, "horizon.ios.home.account");
        use crate::ms_sign_in::State;
        let (button, note, primary) = match self.account.state() {
            State::SignedOut => (
                "horizon.ios.home.sign_in",
                crate::lang::get("horizon.ios.home.offline"),
                true,
            ),
            State::Failed(error) => ("horizon.ios.home.sign_in", error, true),
            State::Starting => (
                "horizon.ios.home.cancel",
                crate::lang::get("horizon.ios.home.starting"),
                false,
            ),
            State::Code { code, uri } => {
                let site = uri
                    .split_once("://")
                    .map_or(uri.as_str(), |(_, rest)| rest)
                    .to_owned();
                (
                    "horizon.ios.home.open_link",
                    crate::lang::tr("horizon.ios.home.code", &[&code, &site]),
                    true,
                )
            }
            State::Working(message) => ("horizon.ios.home.cancel", message, false),
            State::SignedIn(account) => (
                "horizon.ios.home.sign_out",
                crate::lang::tr("horizon.ios.home.signed_in", &[&account.name]),
                false,
            ),
        };
        let [bx, by, bw, bh] = l.sign_in;
        let hover = inside(l.sign_in, ui.cursor);
        if primary {
            let accent = glass().accent;
            let fill = if hover {
                mix(accent, 0xffffff, 0.12)
            } else {
                accent
            };
            ui.rounded(bx, by, bw, bh, 10.0, rgb(fill));
        } else {
            field(ui, l.sign_in, false);
        }
        let text = crate::lang::get(button);
        let tw = ui.measure(Face::Medium, s.ui, &text);
        let colour = if primary { rgb(glass().on_accent) } else { ink };
        ui.text_line(
            Face::Medium,
            s.ui,
            bh,
            bx + ((bw - tw) / 2.0).round(),
            by,
            colour,
            &text,
        );
        let note = clip(ui, Face::Regular, s.caption, &note, bw);
        ui.text_line(
            Face::Regular,
            s.caption,
            18.0,
            bx,
            by + bh + 8.0,
            muted,
            &note,
        );

        // The touch controls: a tap picks the next layout.
        label(ui, l.touch, "horizon.ios.home.touch");
        field(ui, l.touch, false);
        let [tx, ty, tw, th] = l.touch;
        ui.text_line(
            Face::Regular,
            s.ui,
            th,
            tx + 16.0,
            ty,
            ink,
            &self.scheme.name(),
        );
        let more = "›";
        let mw = ui.measure(Face::Medium, s.ui, more);
        ui.text_line(Face::Medium, s.ui, th, tx + tw - 16.0 - mw, ty, muted, more);

        // Where the game's files are.
        let files = [tx, l.files_y + 26.0, tw, 44.0];
        label(ui, files, "horizon.ios.home.files");
        let wh = crate::lang::get("horizon.ios.files.where");
        let wh = clip(ui, Face::Regular, s.caption, &wh, tw);
        ui.text_line(
            Face::Regular,
            s.caption,
            20.0,
            tx,
            files[1],
            rgb(NIGHT.ink_soft),
            &wh,
        );
        let version = crate::lang::tr(
            "horizon.ios.home.version",
            &[&horizon_core::VANILLA_VERSION],
        );
        ui.text_line(
            Face::Regular,
            s.caption,
            20.0,
            tx,
            files[1] + 22.0,
            muted,
            &version,
        );
    }

    /// The signed-in account's session for the game, when the player chose to play online
    /// and the token is valid. Offline, the game plays with the offline name.
    pub fn session(&self) -> Option<crate::auth::AuthSession> {
        if !self.online {
            return None;
        }
        self.signed_in().and_then(|account| account.session())
    }

    pub fn hit(&self, ui: &Ui) -> Option<Hit> {
        let l = self.layout(ui);
        let c = ui.cursor;
        if let Some((page, _)) = l.nav.iter().find(|(_, r)| inside(*r, c)) {
            return Some(Hit::Nav(*page));
        }
        if inside(l.account, c) {
            return Some(Hit::Account);
        }
        match self.page {
            Page::Home => {
                if inside(l.play, c) {
                    return Some(Hit::Play);
                }
                if self.signed_in().is_some() {
                    if inside(l.mode, c) {
                        return Some(Hit::Mode(c.0 < l.mode[0] + l.mode[2] / 2.0));
                    }
                } else if inside(self.offline_note_rect(ui, &l), c) {
                    return Some(Hit::OfflineNote);
                }
                let card_right = l.card[0] + l.card[2] - 12.0;
                if let Some((tab, _)) = l
                    .tabs
                    .iter()
                    .find(|(_, r)| r[0] + r[2] <= card_right && inside(*r, c))
                {
                    return Some(Hit::Tab(*tab));
                }
                if self.tab == Tab::Mods && inside(l.list, c) {
                    let row = self.row_height();
                    let max_scroll = (self.library.rows.len() as f32 * row - l.list[3]).max(0.0);
                    let first = (self.scroll.min(max_scroll) / row).round() as usize;
                    let i = first + ((c.1 - l.list[1]) / row).floor() as usize;
                    // The whole row turns the mod on or off: a bigger target than the switch.
                    if self.library.rows.get(i).is_some_and(|r| r.toggle.is_some()) {
                        return Some(Hit::Toggle(i));
                    }
                }
                None
            }
            Page::Settings => [
                (l.name_field, Hit::Name),
                (l.sign_in, Hit::SignIn),
                (l.touch, Hit::Touch),
            ]
            .into_iter()
            .find(|(rect, _)| inside(*rect, c))
            .map(|(_, hit)| hit),
        }
    }

    /// Ends name editing: a valid name is kept, anything else goes back to the old one.
    fn commit(&mut self, window: Option<&Window>) {
        if let Some(text) = self.editing.take() {
            let text = text.trim().to_owned();
            if crate::offline_name::is_valid(&text) {
                match crate::offline_name::save(&dir(), &text) {
                    Ok(()) => self.name = text,
                    Err(e) => log::warn!("Horizon: could not keep the offline name: {e}"),
                }
            }
            if let Some(window) = window {
                window.set_ime_allowed(false);
            }
        }
    }

    fn show_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.scroll = 0.0;
        self.library = Library::scan(tab);
        // The log opens at its end, where the latest lines are.
        if tab == Tab::Log {
            self.scroll = f32::MAX;
        }
    }

    fn scroll_by(&mut self, dp: f32) {
        let max = self.metrics.get().max_scroll;
        self.scroll = (self.scroll.min(max) + dp).clamp(0.0, max);
    }

    /// A window event while the home screen shows. `hit_at` gives the hit under a pointer
    /// position. True when Play was pressed: the title takes over.
    pub fn event(
        &mut self,
        window: Option<&Window>,
        event: &WindowEvent,
        hit_at: impl Fn((f64, f64), &Self) -> Option<Hit>,
    ) -> bool {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let moved = (position.x - self.pointer.0, position.y - self.pointer.1);
                self.pointer = (position.x, position.y);
                // A finger (or a held button) moving on the list scrolls it.
                if let Some((start, dragging)) = self.press {
                    let m = self.metrics.get();
                    let density = f64::from(m.density.max(1.0));
                    let far = ((position.x - start.0).hypot(position.y - start.1) / density) as f32;
                    let on_list = self.page == Page::Home && {
                        let at = ((start.0 / density) as f32, (start.1 / density) as f32);
                        inside(m.list, at)
                    };
                    if on_list && (dragging || far > DRAG_SLOP) {
                        self.press = Some((start, true));
                        self.scroll_by(-(moved.1 / density) as f32);
                    }
                }
                false
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.press = Some((self.pointer, false));
                false
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                let dragged = self.press.take().is_some_and(|(_, dragging)| dragging);
                if dragged {
                    return false;
                }
                self.click(window, hit_at(self.pointer, self))
            }
            WindowEvent::MouseWheel { delta, .. } if self.page == Page::Home => {
                let dp = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -y * self.row_height(),
                    MouseScrollDelta::PixelDelta(p) => {
                        -(p.y as f32) / self.metrics.get().density.max(1.0)
                    }
                };
                self.scroll_by(dp);
                false
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let Some(text) = &mut self.editing {
                    match event.physical_key {
                        PhysicalKey::Code(
                            KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape,
                        ) => {
                            self.commit(window);
                        }
                        PhysicalKey::Code(KeyCode::Backspace) => {
                            text.pop();
                        }
                        _ => {
                            if let Some(typed) = event.text.as_deref() {
                                for c in typed.chars() {
                                    if (c.is_ascii_alphanumeric() || c == '_')
                                        && text.len() < NAME_MAX
                                    {
                                        text.push(c);
                                    }
                                }
                            }
                        }
                    }
                    false
                } else {
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                            open_gate();
                            true
                        }
                        PhysicalKey::Code(KeyCode::ArrowDown) => {
                            self.scroll_by(self.row_height());
                            false
                        }
                        PhysicalKey::Code(KeyCode::ArrowUp) => {
                            self.scroll_by(-self.row_height());
                            false
                        }
                        _ => false,
                    }
                }
            }
            _ => false,
        }
    }

    fn set_online(&mut self, online: bool) {
        self.online = online;
        crate::ui_theme::set_preference(
            &dir(),
            PLAY_ONLINE,
            Some(if online { "true" } else { "false" }),
        );
    }

    fn click(&mut self, window: Option<&Window>, hit: Option<Hit>) -> bool {
        if hit != Some(Hit::Name) {
            self.commit(window);
        }
        match hit {
            Some(Hit::Nav(page)) => {
                self.page = page;
                if page == Page::Home {
                    self.show_tab(self.tab);
                }
                false
            }
            Some(Hit::Account | Hit::OfflineNote) => {
                self.page = Page::Settings;
                false
            }
            Some(Hit::Mode(online)) => {
                self.set_online(online);
                false
            }
            Some(Hit::Tab(tab)) => {
                self.show_tab(tab);
                false
            }
            Some(Hit::Toggle(i)) => {
                let Some((id, on)) = self.library.rows.get(i).and_then(|r| r.toggle.clone()) else {
                    return false;
                };
                let dir = dir();
                let mut list = crate::mod_list::ModList::load(&dir);
                if on {
                    list.enabled.remove(&id);
                } else {
                    list.enabled.insert(id.clone());
                }
                list.save(&dir);
                let scroll = self.scroll;
                self.library = Library::scan(Tab::Mods);
                self.scroll = scroll;
                false
            }
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
                use crate::ms_sign_in::State;
                match self.account.state() {
                    State::SignedOut | State::Failed(_) => self.account.start(),
                    State::Code { code, uri } => {
                        // The code on the clipboard, the page in Safari: paste, then come back.
                        crate::clipboard::SystemClipboard::default().set_text(&code);
                        #[cfg(target_os = "ios")]
                        horizon_client_input::ios_open::open_url(&uri);
                        #[cfg(not(target_os = "ios"))]
                        let _ = uri;
                    }
                    State::SignedIn(_) | State::Starting | State::Working(_) => {
                        self.account.sign_out()
                    }
                }
                false
            }
            Some(Hit::Touch) => {
                self.scheme = self.scheme.next();
                if let Err(e) = self.scheme.save(&dir()) {
                    log::warn!("Horizon: could not keep the touch controls: {e}");
                }
                false
            }
            Some(Hit::Play) => {
                open_gate();
                true
            }
            None => false,
        }
    }
}
