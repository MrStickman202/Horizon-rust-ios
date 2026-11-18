//! Horizon on iPhone and iPad.
//!
//! The app has no command line, so it points the game at its own sandbox before starting it:
//! the working directory (saves, `options.txt`, caches) and `HORIZON_HOME` (logs, crash
//! reports) are in the app's Documents folder, which the Files app shows as "Horizon", and the
//! prepared resources are the `26.3` folder the player copies there from a computer.
//!
//! Of Horizon's own in-game parts it registers only the official server list entry: the
//! anonymous playing ping, Discord presence and Play with Friends stay out of the iOS app.
//! Nothing of the person who built it is in the app: each player's sign-in lives in their own
//! device's Keychain.

use horizon_platform::{hooks, mod_links};
use std::path::{Path, PathBuf};

static GAME_IDENTITY: [u8; horizon_game::GAME_IDENTITY.len()] = horizon_game::GAME_IDENTITY;

static MOD_LINKS: [mod_links::Links; 2] = [
    mod_links::Links {
        id: "minecraft",
        license: None,
        homepage: Some(horizon_addresses::MINECRAFT_WEBSITE),
        issues: Some(horizon_addresses::MINECRAFT_ISSUES),
    },
    mod_links::Links {
        id: "horizon",
        license: Some(horizon_addresses::LICENSE),
        homepage: Some(horizon_addresses::WEBSITE),
        issues: Some(horizon_addresses::ISSUES),
    },
];

/// The app's Documents folder: `$HOME/Documents` inside the app's sandbox.
fn documents() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Documents")
}

fn set(name: &str, value: &Path) {
    if std::env::var_os(name).is_none() {
        // SAFETY: called at the very start of `main`, before the game starts any thread.
        unsafe { std::env::set_var(name, value) };
    }
}

fn main() {
    let documents = documents();
    let home = documents.join(".horizon");
    let _ = std::fs::create_dir_all(&home);
    // The app starts in "/", which it can't write: the game keeps its files in the working directory.
    let _ = std::env::set_current_dir(&documents);
    set("HORIZON_HOME", &home);
    set("HORIZON_RESOURCES", &documents.join("26.3"));

    horizon_game::prepare_main_stack();
    std::hint::black_box(&GAME_IDENTITY);
    if let Err(e) = hooks::register(&horizon_official_servers::HOOKS) {
        log::warn!("Horizon: the server list entry did not register: {e}");
    }
    for links in &MOD_LINKS {
        if let Err(e) = mod_links::register(links) {
            log::warn!("Horizon: mod links did not register: {e}");
        }
    }
    horizon_game::main();
}
