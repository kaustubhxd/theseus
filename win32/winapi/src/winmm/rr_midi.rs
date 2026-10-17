//! Road Rash evaluation (not upstream): the midiStream* API, played through
//! the rustysynth General MIDI synthesizer and mixed into the DirectSound
//! output.
//!
//! The game queues MIDIHDR buffers of MIDIEVENTs with midiStreamOut; a player
//! thread walks them in real time (ticks -> microseconds via the stream's time
//! division and tempo), feeds short messages to the synthesizer, and reports
//! each finished buffer to the game's callback with MOM_DONE so it refills.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, LazyLock, Mutex};

use runtime::Context;

const SOUNDFONT: &str = "_theseus/TimGM6mb.sf2";
const STREAM_HANDLE: u32 = 0x3d10_0001;
const MOM_DONE: u32 = 0x3c9;
const MOM_POSITIONCB: u32 = 0x3ca;
const MHDR_DONE: u32 = 1;
const MHDR_PREPARED: u32 = 2;
const MHDR_INQUEUE: u32 = 4;

struct Stream {
    callback: u32,
    instance: u32,
    timediv: u32,
    /// Microseconds per quarter note.
    tempo: u32,
    queue: VecDeque<u32>,
    playing: bool,
    closed: bool,
    /// Bumped by midiOutReset so the player abandons the buffer it's in.
    generation: u32,
}

static STREAM: LazyLock<(Mutex<Option<Stream>>, Condvar)> = LazyLock::new(Default::default);

struct Overlay {
    synth: rustysynth::Synthesizer,
    /// (bank, program) pairs this bank defines.
    presets: Vec<(i32, i32)>,
}

struct Synth {
    synth: rustysynth::Synthesizer,
    volume: f32,
    /// The game's own AWE32 instruments for the current song (GRUNGEn.SBK,
    /// converted to SF2), layered over General MIDI like the AWE32 did.
    song: Option<Overlay>,
    song_name: String,
    /// The game's drum kit (RRDRUMS.SBK), on MIDI channel 10.
    drums: Option<Overlay>,
    program: [i32; 16],
}
static SYNTH: Mutex<Option<Synth>> = Mutex::new(None);

/// The .MID file the game most recently opened (set from CreateFileA).
static SONG: Mutex<String> = Mutex::new(String::new());

pub fn note_midi_file(name: &str) {
    let base = name.rsplit(['\\', '/']).next().unwrap_or(name);
    let stem = base.split('.').next().unwrap_or(base).to_ascii_uppercase();
    *SONG.lock().unwrap() = stem;
}

fn load_overlay(file: &str) -> Option<Overlay> {
    let path = crate::kernel32::resolve_path(&format!("_theseus/{file}"));
    let bytes = host::fs::read(&path).ok()?;
    let sf = Arc::new(rustysynth::SoundFont::new(&mut &bytes[..]).ok()?);
    let presets = sf.get_presets().iter().map(|p| (p.get_bank_number(), p.get_patch_number())).collect();
    let synth = rustysynth::Synthesizer::new(&sf, &rustysynth::SynthesizerSettings::new(44100)).ok()?;
    log::warn!("rr-midi: loaded original AWE32 bank {file}");
    Some(Overlay { synth, presets })
}

/// Make the song overlay match the .MID being played.
fn sync_song(s: &mut Synth) {
    let song = SONG.lock().unwrap().clone();
    if song != s.song_name {
        s.song = load_overlay(&format!("{song}.sf2"));
        s.song_name = song;
    }
}

fn load_synth() {
    let mut s = SYNTH.lock().unwrap();
    if s.is_some() {
        return;
    }
    let path = crate::kernel32::resolve_path(SOUNDFONT);
    let bytes = match host::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("rr-midi: no soundfont at {path:?}: {e}; MIDI will be silent");
            return;
        }
    };
    let sf = match rustysynth::SoundFont::new(&mut &bytes[..]) {
        Ok(sf) => Arc::new(sf),
        Err(e) => {
            log::warn!("rr-midi: bad soundfont: {e:?}");
            return;
        }
    };
    match rustysynth::Synthesizer::new(&sf, &rustysynth::SynthesizerSettings::new(44100)) {
        Ok(synth) => {
            log::warn!("rr-midi: synthesizer ready ({} bytes of soundfont)", bytes.len());
            *s = Some(Synth { synth, volume: 1.0, song: None, song_name: String::new(), drums: load_overlay("RRDRUMS.sf2"), program: [0; 16] });
        }
        Err(e) => log::warn!("rr-midi: synthesizer: {e:?}"),
    }
}

/// Called by the DirectSound mixer for every chunk (samples in i16 units).
pub fn mix_into(mixed: &mut [(f32, f32)]) {
    let Ok(mut s) = SYNTH.try_lock() else { return };
    let Some(s) = s.as_mut() else { return };
    let n = mixed.len();
    let (mut left, mut right) = (vec![0f32; n], vec![0f32; n]);
    s.synth.render(&mut left, &mut right);
    for o in [s.song.as_mut(), s.drums.as_mut()].into_iter().flatten() {
        let (mut l2, mut r2) = (vec![0f32; n], vec![0f32; n]);
        o.synth.render(&mut l2, &mut r2);
        for i in 0..n {
            left[i] += l2[i];
            right[i] += r2[i];
        }
    }
    let gain = 32767.0 * s.volume;
    for (i, slot) in mixed.iter_mut().enumerate() {
        slot.0 += left[i] * gain;
        slot.1 += right[i] * gain;
    }
}

fn synth_message(event: u32) {
    let status = (event & 0xff) as i32;
    if status < 0x80 {
        return;
    }
    let (channel, command) = (status & 0x0f, status & 0xf0);
    let (d1, d2) = (((event >> 8) & 0x7f) as i32, ((event >> 16) & 0x7f) as i32);
    let mut g = SYNTH.lock().unwrap();
    let Some(s) = g.as_mut() else { return };
    sync_song(s);
    match command {
        0x80 | 0x90 | 0xa0 => {
            // Notes go to the bank that owns the channel's sound.
            let program = s.program[channel as usize];
            let target = if channel == 9 {
                s.drums.as_mut().map(|o| &mut o.synth)
            } else {
                s.song.as_mut().filter(|o| o.presets.contains(&(0, program))).map(|o| &mut o.synth)
            };
            match target {
                Some(t) => t.process_midi_message(channel, command, d1, d2),
                None => s.synth.process_midi_message(channel, command, d1, d2),
            }
        }
        _ => {
            // Program, controller, pitch bend...: every synth keeps the channel state.
            if command == 0xc0 {
                s.program[channel as usize] = d1;
            }
            s.synth.process_midi_message(channel, command, d1, d2);
            for o in [s.song.as_mut(), s.drums.as_mut()].into_iter().flatten() {
                o.synth.process_midi_message(channel, command, d1, d2);
            }
        }
    }
}

fn notify() {
    STREAM.1.notify_all();
}

pub fn stream_open(ctx: &mut Context, phms: u32, callback: u32, instance: u32, fdw_open: u32) -> u32 {
    load_synth();
    let (m, _) = &*STREAM;
    *m.lock().unwrap() = Some(Stream {
        callback: if fdw_open & 0x0007_0000 == 0x0003_0000 { callback } else { 0 }, // CALLBACK_FUNCTION only
        instance,
        timediv: 96,
        tempo: 500_000,
        queue: VecDeque::new(),
        playing: false,
        closed: false,
        generation: 0,
    });
    ctx.memory.write::<u32>(phms, STREAM_HANDLE);
    crate::kernel32::lock().create_thread(ctx, "midi player".into(), player);
    log::warn!("rr-midi: stream opened (callback {callback:#x}, flags {fdw_open:#x})");
    0
}

pub fn stream_close() -> u32 {
    if let Some(s) = STREAM.0.lock().unwrap().as_mut() {
        s.closed = true;
    }
    notify();
    if let Some(s) = SYNTH.lock().unwrap().as_mut() {
        s.synth.note_off_all(true);
        for o in [s.song.as_mut(), s.drums.as_mut()].into_iter().flatten() {
            o.synth.note_off_all(true);
        }
    }
    0
}

pub fn stream_property(ctx: &mut Context, prop_data: u32, property: u32) -> u32 {
    let mut g = STREAM.0.lock().unwrap();
    let Some(s) = g.as_mut() else { return 5 };
    let (set, get) = (property & 0x8000_0000 != 0, property & 0x4000_0000 != 0);
    let field = if property & 1 != 0 { &mut s.timediv } else if property & 2 != 0 { &mut s.tempo } else { return 0 };
    if set {
        *field = ctx.memory.read::<u32>(prop_data + 4).max(1);
    } else if get {
        ctx.memory.write::<u32>(prop_data + 4, *field);
    }
    0
}

pub fn prepare(ctx: &mut Context, hdr: u32, prepare: bool) -> u32 {
    let flags = ctx.memory.read::<u32>(hdr + 16);
    let flags = if prepare { flags | MHDR_PREPARED } else { flags & !MHDR_PREPARED };
    ctx.memory.write::<u32>(hdr + 16, flags);
    0
}

pub fn stream_out(ctx: &mut Context, hdr: u32) -> u32 {
    let flags = ctx.memory.read::<u32>(hdr + 16);
    ctx.memory.write::<u32>(hdr + 16, (flags | MHDR_INQUEUE) & !MHDR_DONE);
    ctx.memory.write::<u32>(hdr + 28, 0); // dwOffset
    if let Some(s) = STREAM.0.lock().unwrap().as_mut() {
        s.queue.push_back(hdr);
    }
    notify();
    0
}

pub fn set_playing(playing: bool) -> u32 {
    if let Some(s) = STREAM.0.lock().unwrap().as_mut() {
        s.playing = playing;
    }
    if !playing {
        if let Some(s) = SYNTH.lock().unwrap().as_mut() {
            s.synth.note_off_all(false);
        }
    }
    notify();
    0
}

/// midiOutReset: stop and hand every queued buffer back as done.
pub fn reset(ctx: &mut Context) -> u32 {
    let (done, callback, instance) = {
        let mut g = STREAM.0.lock().unwrap();
        let Some(s) = g.as_mut() else { return 0 };
        s.generation += 1;
        (std::mem::take(&mut s.queue), s.callback, s.instance)
    };
    if let Some(s) = SYNTH.lock().unwrap().as_mut() {
        s.synth.note_off_all(true);
    }
    for hdr in done {
        finish(ctx, hdr, callback, instance);
    }
    notify();
    0
}

pub fn set_volume(vol: u32) -> u32 {
    if let Some(s) = SYNTH.lock().unwrap().as_mut() {
        s.volume = (vol & 0xffff) as f32 / 65535.0;
    }
    0
}

fn finish(ctx: &mut Context, hdr: u32, callback: u32, instance: u32) {
    let flags = ctx.memory.read::<u32>(hdr + 16);
    ctx.memory.write::<u32>(hdr + 16, (flags | MHDR_DONE) & !MHDR_INQUEUE);
    if callback != 0 {
        let f = ctx.indirect32(callback);
        ctx.call32_x86(f, vec![STREAM_HANDLE, MOM_DONE, instance, hdr, 0]);
    }
}

/// The player thread: plays queued buffers in real time.
fn player(ctx: &mut Context) {
    let mut next_us = crate::rr_resdll::micros();
    loop {
        // Wait for something to play.
        let (hdr, generation) = {
            let (m, c) = &*STREAM;
            let mut g = m.lock().unwrap();
            loop {
                match g.as_ref() {
                    None => return,
                    Some(s) if s.closed => return,
                    Some(s) if s.playing && !s.queue.is_empty() => break,
                    _ => {}
                }
                g = c.wait_timeout(g, std::time::Duration::from_millis(50)).unwrap().0;
                next_us = crate::rr_resdll::micros();
            }
            let s = g.as_ref().unwrap();
            (*s.queue.front().unwrap(), s.generation)
        };

        let data = ctx.memory.read::<u32>(hdr);
        let recorded = ctx.memory.read::<u32>(hdr + 8);
        let mut ofs = ctx.memory.read::<u32>(hdr + 28);
        let mut abandoned = false;
        while ofs + 12 <= recorded {
            let delta = ctx.memory.read::<u32>(data + ofs);
            let event = ctx.memory.read::<u32>(data + ofs + 8);
            let (timediv, tempo) = match STREAM.0.lock().unwrap().as_ref() {
                Some(s) => (s.timediv, s.tempo),
                None => return,
            };
            next_us += delta as u64 * tempo as u64 / timediv as u64;
            // Sleep until the event is due, staying responsive to pause/reset/close.
            loop {
                let now = crate::rr_resdll::micros();
                {
                    let g = STREAM.0.lock().unwrap();
                    match g.as_ref() {
                        None => return,
                        Some(s) if s.closed => return,
                        Some(s) if s.generation != generation => {
                            abandoned = true;
                            break;
                        }
                        Some(s) if !s.playing => {
                            next_us = now;
                        }
                        _ => {}
                    }
                }
                if now >= next_us {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_micros((next_us - now).min(5_000)));
            }
            if abandoned {
                break;
            }
            let kind = (event >> 24) & 0x3f;
            match kind {
                0x00 => synth_message(event & 0x00ff_ffff),
                0x01 => {
                    if let Some(s) = STREAM.0.lock().unwrap().as_mut() {
                        s.tempo = (event & 0x00ff_ffff).max(1);
                    }
                }
                _ => {} // NOP, comments, version, sysex: ignored
            }
            if event & 0x4000_0000 != 0 {
                // MEVT_F_CALLBACK: report the position.
                ctx.memory.write::<u32>(hdr + 28, ofs);
                let (callback, instance) = STREAM.0.lock().unwrap().as_ref().map(|s| (s.callback, s.instance)).unwrap_or((0, 0));
                if callback != 0 {
                    let f = ctx.indirect32(callback);
                    ctx.call32_x86(f, vec![STREAM_HANDLE, MOM_POSITIONCB, instance, hdr, 0]);
                }
            }
            ofs += 12;
            if event & 0x8000_0000 != 0 {
                ofs += ((event & 0x00ff_ffff) + 3) & !3; // long event parameters
            }
            ctx.memory.write::<u32>(hdr + 28, ofs);
        }
        if abandoned {
            continue; // reset already returned the buffers
        }
        // Buffer finished: dequeue it and tell the game.
        let (callback, instance) = {
            let mut g = STREAM.0.lock().unwrap();
            let Some(s) = g.as_mut() else { return };
            if s.queue.front() == Some(&hdr) {
                s.queue.pop_front();
            }
            (s.callback, s.instance)
        };
        finish(ctx, hdr, callback, instance);
    }
}
