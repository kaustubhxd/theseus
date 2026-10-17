//! Interface for the host environment, for APIs like "create window" or "play sound".
//! Implemented using SDL or web technologies.

#![cfg_attr(target_family = "wasm", feature(stdarch_wasm_atomic_wait))]

use std::sync::LazyLock;

pub mod fs;
#[cfg(not(target_family = "wasm"))]
mod sdl;
mod single_thread;
#[cfg(not(target_family = "wasm"))]
pub use sdl::*;
pub use single_thread::SingleThreader;

#[cfg(target_family = "wasm")]
mod wasm;
#[cfg(target_family = "wasm")]
pub use wasm::*;

static HOST: LazyLock<Host> = LazyLock::new(Host::new);

#[derive(Clone, Copy)]
pub struct AudioSpec {
    pub sample_rate: u32,
    pub channels: u32,
}

bitflags::bitflags! {
    #[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
    pub struct MouseButton: u16 {
        const Left = 1 << 0;
        const Middle = 1 << 1;
        const Right = 1 << 2;
    }
}

pub struct MouseMessage {
    pub x: u32,
    pub y: u32,
    /// In a click, which button triggered the click.
    pub button: MouseButton,
    /// Bitfield, which buttons are pressed.
    pub buttons: MouseButton,
}

/// A key press/release, described the way DOS-era Windows apps expect: by
/// PC/AT scan code (what DirectInput calls DIK_*) plus virtual key code.
pub struct KeyMessage {
    /// PC "set 1" scan code, without the 0xe0 prefix of extended keys.
    pub scancode: u8,
    /// Windows VK_* code. Uses the side-specific code (VK_RSHIFT, not VK_SHIFT)
    /// where one exists; user32 widens it for window messages.
    pub vkey: u8,
    /// Key from the extended (0xe0-prefixed) part of the keyboard: arrows,
    /// right ctrl/alt, keypad enter. Distinguishes e.g. arrow up from keypad 8,
    /// which share scan code 0x48.
    pub extended: bool,
    /// Event produced by auto-repeat rather than a fresh press.
    pub repeat: bool,
    /// Road Rash evaluation: the VK_* code of the physical key before the WASD
    /// remap; typed characters (WM_CHAR) come from this one. Hosts set it to
    /// `vkey`; 0 marks a key that must not be remapped (scripted test input).
    pub text_vkey: u8,
}

pub enum Message {
    #[cfg(not(target_family = "wasm"))] // no "quit" menu on web
    Quit,
    #[cfg(not(target_family = "wasm"))] // no paint on web, browser does painting
    Paint,
    MouseDown(MouseMessage),
    MouseUp(MouseMessage),
    MouseMove(MouseMessage),
    KeyDown(KeyMessage),
    KeyUp(KeyMessage),
}

/// Which winapi calls to trace, in the syntax `trace::init` parses.
#[cfg(not(target_family = "wasm"))]
pub fn trace_spec() -> String {
    std::env::var("THESEUS_TRACE").unwrap_or_default()
}

/// The web build has no environment to read a spec from, so the page sets one
/// with `set_trace` before starting the program. Tracing every call is slow
/// enough to change how a program behaves, so it is off unless asked for.
#[cfg(target_family = "wasm")]
static TRACE_SPEC: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

#[cfg(target_family = "wasm")]
pub fn trace_spec() -> String {
    TRACE_SPEC.lock().unwrap().clone()
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn set_trace(spec: &str) {
    *TRACE_SPEC.lock().unwrap() = spec.to_string();
}

pub fn init() {
    logger::init();
    LazyLock::force(&HOST);
}

pub fn host() -> &'static Host {
    &*HOST
}

/// Road Rash evaluation: an environment variable natively; on the web, a value
/// from the page's URL query (set by worker.ts as `self.rr_env`).
pub fn env(name: &str) -> Option<String> {
    #[cfg(not(target_family = "wasm"))]
    {
        std::env::var(name).ok()
    }
    #[cfg(target_family = "wasm")]
    {
        use wasm_bindgen::JsCast;
        let g = js_sys::global();
        let env = js_sys::Reflect::get(&g, &"rr_env".into()).ok()?;
        if env.is_undefined() {
            return None;
        }
        let v = js_sys::Reflect::get(&env, &name.into()).ok()?;
        v.dyn_into::<js_sys::JsString>().ok().map(String::from)
    }
}

/// Road Rash evaluation: a save/load file picker. Native has none (callers
/// fall back to an automatic choice): Err(()) means "no picker available".
pub fn choose_file(save: bool, names: &[String], proposed: &str) -> Result<Option<String>, ()> {
    #[cfg(target_family = "wasm")]
    {
        Ok(wasm::choose_file(save, names, proposed))
    }
    #[cfg(not(target_family = "wasm"))]
    {
        let _ = (save, names, proposed);
        Err(())
    }
}

/// Road Rash evaluation: the WASD control layout, applied to the PC key the
/// game sees: W/A/S/D -> arrows, Cmd -> Ins (punch), Space -> Enter (kick),
/// X -> Space (backhand), Left Shift -> N (nitro), F -> D (dismount).
/// Disabled with RR_KEYS_CLASSIC=1 (natively) or ?keys=classic (web page).
pub fn rr_wasd(scancode: u8, vkey: u8, extended: bool) -> (u8, u8, bool) {
    thread_local! {
        static CLASSIC: bool = env("RR_KEYS_CLASSIC").is_some_and(|v| v == "1");
    }
    if CLASSIC.with(|c| *c) {
        return (scancode, vkey, extended);
    }
    match vkey {
        b'W' => (0x48, 0x26, true),
        b'A' => (0x4b, 0x25, true),
        b'S' => (0x50, 0x28, true),
        b'D' => (0x4d, 0x27, true),
        0x5b | 0x5c => (0x52, 0x2d, true), // Cmd (Win key) -> Ins, punch
        0x20 => (0x1c, 0x0d, false),       // Space -> Enter, kick
        b'X' => (0x39, 0x20, false),       // X -> Space, backhand
        0xa0 => (0x31, b'N', false),
        b'F' => (0x20, b'D', false),
        _ => (scancode, vkey, extended),
    }
}
