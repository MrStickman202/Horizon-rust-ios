//! Asks for the game's main-thread stack, as the desktop executables do; iOS may give less.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        println!("cargo:rustc-link-arg-bin=Horizon=-Wl,-stack_size,0x8800000");
    }
}
