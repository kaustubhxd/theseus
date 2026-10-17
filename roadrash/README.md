# Road Rash (Win95) on Theseus

This branch is [Theseus](https://github.com/evmar/theseus) plus what it took to run EA's *Road Rash* (1996, Windows 95) natively on macOS and in the browser as WebAssembly. The write-up (how it works, loading and caching, gotchas, controls) is in the README of the hosted build: **https://github.com/kaustubhxd/roadrash**

**No game content is included.** The translated game code (`out/roadrash/src/generated*`), its data sections (`out/roadrash/data`), the WebAssembly and the game files are all left out. You need your own copy of the game, translated with `tc` yourself.

## What's here

- **`win32/winapi`:** the Win32 work, mostly in new `rr_*.rs` files:
  - windows, focus and keyboard (`user32/rr_windows.rs`);
  - text with TrueType fonts (`rr_text.rs`);
  - resource DLLs and strings (`rr_resdll.rs`);
  - registry and file dialogs (`rr_registry.rs`);
  - MIDI streaming with rustysynth (`winmm/rr_midi.rs`);
  - MCI video with Cinepak and IMA ADPCM decoders (`winmm/rr_video.rs`);
  - generated stubs for the long tail (`rr_stubs_*.rs`);
  - fixes in ddraw, dsound, kernel32, gdi32 and user32.
- **`host`:** SDL and browser hosts, threads as Web Workers, audio ring buffer, lazily fetched files, the WASD remap.
- **`web`:** the page:
  - side panels, loading screen and in-game loading indicator;
  - the save picker, saved to IndexedDB;
  - the service worker cache (`public/rr-sw.js`) and background downloads (`prefetch.ts`);
  - optional encryption for public hosting (`crypt.ts`).
- **`tc`, `runtime`:** small fixes (80-bit floats, codegen formatting).
- **`roadrash/`:**
  - `scripts/`: the stub generator, native and wasm builds, a local server;
  - `convert/`: `.BOB` → PNG, and AWE32 `.SBK` → `.SF2` (`sbk2sf2.c` builds on awesfx and is GPL like it);
  - `pages/`: the encrypted GitHub Pages build.

The scripts assume the layout I used (`~/theseus-roadrash/{theseus,game/ROADRASH,tools}`), so expect to adjust paths. The changes were made for one game and are not a general Win32 implementation: things were added until Road Rash stopped hitting them.

## Rough steps

1. Copy the game's files off your disc into `game/ROADRASH`.
2. Translate `ROADRASH.EXE` with `tc` into `out/roadrash` (see upstream Theseus).
3. Regenerate the stubs: `python3 roadrash/scripts/gen_stubs.py`.
4. Build natively with `cargo build --release -p roadrash`, then test with `roadrash/scripts/run.sh` (headless and muted by default).
5. For the browser, run `roadrash/scripts/build-wasm-rr.sh` (nightly Rust with atomics and build-std), then `roadrash/scripts/play.sh` (vite, http://127.0.0.1:8880/).
