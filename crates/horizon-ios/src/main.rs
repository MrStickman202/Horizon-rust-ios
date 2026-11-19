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

/// The prepared resources: `Documents/26.3`, or else the first folder in Documents (three
/// levels down at most) holding a `horizon-resources.json`, so a folder copied one level too
/// deep (`Horizon/26.3/26.3`) or under another name still opens.
fn find_resources(documents: &Path) -> PathBuf {
    const MANIFEST: &str = "horizon-resources.json";
    let expected = documents.join("26.3");
    if expected.join(MANIFEST).is_file() {
        return expected;
    }
    let mut level = vec![documents.to_path_buf()];
    for _ in 0..3 {
        let mut next = Vec::new();
        for dir in level {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut dirs: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
                .collect();
            dirs.sort();
            for dir in dirs {
                if dir.join(MANIFEST).is_file() {
                    return dir;
                }
                next.push(dir);
            }
        }
        level = next;
    }
    // Say what is there, for the start log: the usual mistake is Minecraft's own 26.3 folder.
    if let Ok(entries) = std::fs::read_dir(&expected) {
        let names: Vec<String> = entries
            .flatten()
            .take(20)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        step(&format!("26.3 holds: {}", names.join(", ")));
    }
    expected
}

fn set(name: &str, value: &Path) {
    if std::env::var_os(name).is_none() {
        // SAFETY: called at the very start of `main`, before the game starts any thread.
        unsafe { std::env::set_var(name, value) };
    }
}

/// Where a start that never shows anything can be read from the Files app: `logs/ios-start.txt`
/// has the start's steps and anything the game printed (its standard error), and a panic's
/// message and place.
fn diagnostics(documents: &Path) {
    use std::io::Write;
    let logs = documents.join("logs");
    let _ = std::fs::create_dir_all(&logs);
    let path = logs.join("ios-start.txt");
    if let Ok(file) = std::fs::File::create(&path) {
        use std::os::fd::AsRawFd;
        // SAFETY: at the start of `main`, before any thread: standard output and error now
        // write to the file, which stays open for the life of the process.
        unsafe {
            libc::dup2(file.as_raw_fd(), 1);
            libc::dup2(file.as_raw_fd(), 2);
        }
        std::mem::forget(file);
    }
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        eprintln!(
            "PANIC on thread {}: {info}\n{}",
            thread.name().unwrap_or("?"),
            std::backtrace::Backtrace::force_capture()
        );
        let _ = std::io::stderr().flush();
    }));
    step("main");
}

/// A line in `ios-start.txt`, written at once.
fn step(what: &str) {
    use std::io::Write;
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    eprintln!("[{since}] step: {what}");
    let _ = std::io::stderr().flush();
}

/// Sound plays like a game's: the Playback category, so it is heard with the iPad's silent
/// mode on, mixed with whatever else plays (the player's music isn't paused), and the session
/// active before the game opens its audio output.
#[cfg(target_os = "ios")]
fn audio_session() {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    #[link(name = "AVFAudio", kind = "framework")]
    unsafe extern "C" {}
    // SAFETY: AVAudioSession's documented class methods, called on the main thread at start.
    unsafe {
        let session: *mut AnyObject = msg_send![class!(AVAudioSession), sharedInstance];
        if session.is_null() {
            step("audio: no audio session");
            return;
        }
        let category = NSString::from_str("AVAudioSessionCategoryPlayback");
        let none = std::ptr::null_mut::<*mut AnyObject>();
        // MixWithOthers (1): the player's music keeps playing under the game.
        let mix_with_others: usize = 1;
        let set: bool =
            msg_send![session, setCategory: &*category, withOptions: mix_with_others, error: none];
        let active: bool = msg_send![session, setActive: true, error: none];
        step(&format!("audio: playback category {set}, active {active}"));
    }
}

fn main() {
    let documents = documents();
    diagnostics(&documents);
    #[cfg(target_os = "ios")]
    audio_session();
    // The app starts in "/", which it can't write: the game keeps its files in the working
    // directory, and its logs and crash reports in HORIZON_HOME; both are Documents, so the
    // Files app shows them.
    let _ = std::env::set_current_dir(&documents);
    set("HORIZON_HOME", &documents);
    let resources = find_resources(&documents);
    step(&format!("resources: {}", resources.display()));
    set("HORIZON_RESOURCES", &resources);
    // A watchdog: a start stuck before its first frame still says how long it has been alive.
    std::thread::spawn(|| {
        let started = std::time::Instant::now();
        for wait in [5, 10, 15, 30] {
            std::thread::sleep(std::time::Duration::from_secs(wait));
            step(&format!("still running after {} s", started.elapsed().as_secs()));
        }
    });

    step("prepare stack");
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
    step("game main");
    horizon_game::main();
    step("game main returned");
}
