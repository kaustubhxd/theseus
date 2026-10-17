#[cfg(target_family = "wasm")]
use wasm_bindgen::prelude::*;

mod generated;

#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn main() {
    winapi::run(&generated::EXEDATA);
}

/// Road Rash evaluation: let the web page add game files to the in-memory filesystem.
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn mount(path: &str, data: Vec<u8>) {
    host::fs::mount(path, data);
}
