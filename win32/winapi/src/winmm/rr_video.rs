//! Road Rash evaluation (not upstream): MCI "avivideo" playback for the
//! game's cut-scenes (Cinepak video + IMA ADPCM audio in AVI files).
//!
//! The game opens a video with MCI_OPEN, attaches its window (MCI_WINDOW),
//! sets the destination rectangle (MCI_PUT), plays with MCI_NOTIFY, and waits
//! in its message loop for MM_MCINOTIFY. Here the AVI is parsed and decoded in
//! Rust; frames are drawn into the host window and audio goes to its own host
//! audio stream, both from the main thread's message loop (`pump`), which
//! also produces the MM_MCINOTIFY message when the video ends.

use std::sync::Mutex;

use runtime::Context;

const MM_MCINOTIFY: u32 = 0x3b9;
const MCI_NOTIFY_SUCCESSFUL: u32 = 1;
const MCI_NOTIFY_ABORTED: u32 = 4;

// ---- AVI -------------------------------------------------------------------

struct Avi {
    width: u32,
    height: u32,
    us_per_frame: u32,
    /// Compressed video frames, in order.
    frames: Vec<Vec<u8>>,
    /// Decoded audio, interleaved stereo i16 at `audio_rate`.
    audio: Vec<i16>,
    audio_rate: u32,
}

fn le16(d: &[u8], o: usize) -> u32 {
    u16::from_le_bytes([d[o], d[o + 1]]) as u32
}
fn le32(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

fn parse_avi(d: &[u8]) -> Option<Avi> {
    if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"AVI " {
        return None;
    }
    let mut avi = Avi { width: 0, height: 0, us_per_frame: 66_667, frames: vec![], audio: vec![], audio_rate: 22050 };
    let mut wfx: Option<(u32, u32, u32, u32)> = None; // (tag, channels, rate, block_align)
    let mut audio_chunks: Vec<&[u8]> = vec![];
    let mut stream_kind: Vec<[u8; 4]> = vec![];
    fn walk<'a>(d: &'a [u8], mut off: usize, end: usize, avi: &mut Avi, wfx: &mut Option<(u32, u32, u32, u32)>, audio: &mut Vec<&'a [u8]>, kinds: &mut Vec<[u8; 4]>) {
        while off + 8 <= end.min(d.len()) {
            let id: [u8; 4] = d[off..off + 4].try_into().unwrap();
            let size = le32(d, off + 4) as usize;
            let body = off + 8;
            let body_end = (body + size).min(d.len());
            match &id {
                b"RIFF" | b"LIST" => walk(d, body + 4, body_end, avi, wfx, audio, kinds),
                b"avih" => avi.us_per_frame = le32(d, body).max(1),
                b"strh" => kinds.push(d[body..body + 4].try_into().unwrap()),
                b"strf" => match kinds.last() {
                    Some(b"vids") => {
                        avi.width = le32(d, body + 4);
                        avi.height = le32(d, body + 8);
                    }
                    Some(b"auds") => {
                        *wfx = Some((le16(d, body), le16(d, body + 2), le32(d, body + 4), le16(d, body + 12)));
                    }
                    _ => {}
                },
                _ if id[2..4] == *b"dc" || id[2..4] == *b"db" => avi.frames.push(d[body..body_end].to_vec()),
                _ if id[2..4] == *b"wb" => audio.push(&d[body..body_end]),
                _ => {}
            }
            off = body + size + (size & 1);
        }
    }
    walk(d, 0, d.len(), &mut avi, &mut wfx, &mut audio_chunks, &mut stream_kind);
    if let Some((tag, channels, rate, align)) = wfx {
        avi.audio_rate = rate;
        for chunk in audio_chunks {
            match tag {
                0x11 => decode_ima_adpcm(chunk, channels as usize, align as usize, &mut avi.audio),
                1 => {
                    for s in chunk.chunks_exact(2) {
                        avi.audio.push(i16::from_le_bytes([s[0], s[1]]));
                    }
                }
                _ => {}
            }
        }
        if channels == 1 {
            avi.audio = avi.audio.iter().flat_map(|&s| [s, s]).collect();
        }
    }
    Some(avi)
}

// ---- IMA ADPCM -------------------------------------------------------------

const IMA_INDEX: [i32; 16] = [-1, -1, -1, -1, 2, 4, 6, 8, -1, -1, -1, -1, 2, 4, 6, 8];
const IMA_STEP: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118,
    130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060,
    1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484,
    7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// Microsoft IMA ADPCM (WAVE_FORMAT_IMA_ADPCM): per block, a 4-byte header per
/// channel, then 4-byte groups per channel of 8 nibbles (low nibble first).
fn decode_ima_adpcm(data: &[u8], channels: usize, block_align: usize, out: &mut Vec<i16>) {
    if channels == 0 || block_align < 4 * channels {
        return;
    }
    for block in data.chunks(block_align) {
        if block.len() < 4 * channels {
            break;
        }
        let mut pred = [0i32; 2];
        let mut index = [0i32; 2];
        for c in 0..channels.min(2) {
            pred[c] = i16::from_le_bytes([block[4 * c], block[4 * c + 1]]) as i32;
            index[c] = (block[4 * c + 2] as i32).clamp(0, 88);
        }
        let mut per_channel: Vec<Vec<i16>> = (0..channels).map(|c| vec![pred[c.min(1)] as i16]).collect();
        let mut pos = 4 * channels;
        while pos + 4 * channels <= block.len() {
            for c in 0..channels {
                for byte in &block[pos + 4 * c..pos + 4 * c + 4] {
                    for nib in [byte & 0x0f, byte >> 4] {
                        let ch = c.min(1);
                        let step = IMA_STEP[index[ch] as usize];
                        let mut diff = step >> 3;
                        if nib & 1 != 0 { diff += step >> 2; }
                        if nib & 2 != 0 { diff += step >> 1; }
                        if nib & 4 != 0 { diff += step; }
                        if nib & 8 != 0 { pred[ch] -= diff } else { pred[ch] += diff }
                        pred[ch] = pred[ch].clamp(-32768, 32767);
                        index[ch] = (index[ch] + IMA_INDEX[nib as usize]).clamp(0, 88);
                        per_channel[c].push(pred[ch] as i16);
                    }
                }
            }
            pos += 4 * channels;
        }
        let n = per_channel.iter().map(|v| v.len()).min().unwrap_or(0);
        for i in 0..n {
            for c in 0..channels {
                out.push(per_channel[c][i]);
            }
        }
    }
}

// ---- Cinepak -----------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Entry {
    y: [u8; 4],
    u: i8,
    v: i8,
}

#[derive(Clone)]
struct Strip {
    v4: [Entry; 256],
    v1: [Entry; 256],
}

struct Cinepak {
    width: usize,
    height: usize,
    /// RGB frame, kept between frames (inter frames only update some blocks).
    rgb: Vec<[u8; 3]>,
    strips: Vec<Strip>,
}

fn be16(d: &[u8], o: usize) -> usize {
    ((d[o] as usize) << 8) | d[o + 1] as usize
}
fn be24(d: &[u8], o: usize) -> usize {
    ((d[o] as usize) << 16) | ((d[o + 1] as usize) << 8) | d[o + 2] as usize
}
fn be32(d: &[u8], o: usize) -> u32 {
    u32::from_be_bytes(d[o..o + 4].try_into().unwrap())
}

impl Cinepak {
    fn new(width: usize, height: usize) -> Self {
        Cinepak { width, height, rgb: vec![[0; 3]; width * height], strips: vec![] }
    }

    fn put(&mut self, x: usize, y: usize, yy: u8, u: i8, v: i8) {
        if x >= self.width || y >= self.height {
            return;
        }
        let (yy, u, v) = (yy as i32, u as i32, v as i32);
        let clip = |c: i32| c.clamp(0, 255) as u8;
        self.rgb[y * self.width + x] = [clip(yy + v * 2), clip(yy - u / 2 - v), clip(yy + u * 2)];
    }

    fn decode_frame(&mut self, d: &[u8]) {
        if d.len() < 10 {
            return;
        }
        let flags = d[0];
        let num_strips = be16(d, 8);
        let mut pos = 10;
        let mut y0 = 0usize;
        for i in 0..num_strips {
            if pos + 12 > d.len() {
                break;
            }
            if self.strips.len() <= i {
                let fresh = Strip { v4: [Entry::default(); 256], v1: [Entry::default(); 256] };
                self.strips.push(self.strips.last().cloned().unwrap_or(fresh));
            } else if i > 0 && flags & 1 == 0 {
                // Codebooks carry over from the previous strip.
                self.strips[i] = self.strips[i - 1].clone();
            }
            let strip_size = be24(d, pos + 1).saturating_sub(12);
            let y1 = y0 + be16(d, pos + 8);
            let start = pos + 12;
            let end = (start + strip_size).min(d.len());
            self.decode_strip(i, y0, y1, &d[start..end]);
            pos = end;
            y0 = y1;
        }
    }

    fn decode_strip(&mut self, s: usize, y0: usize, y1: usize, d: &[u8]) {
        let mut pos = 0;
        while pos + 4 <= d.len() {
            let id = d[pos];
            let size = be24(d, pos + 1).saturating_sub(4);
            let start = pos + 4;
            let end = (start + size).min(d.len());
            let chunk = &d[start..end];
            match id {
                0x20 | 0x21 | 0x24 | 0x25 => decode_codebook(&mut self.strips[s].v4, id, chunk),
                0x22 | 0x23 | 0x26 | 0x27 => decode_codebook(&mut self.strips[s].v1, id, chunk),
                0x30 | 0x31 | 0x32 => self.decode_vectors(s, id, y0, y1, chunk),
                _ => {}
            }
            pos = end;
        }
    }

    fn decode_vectors(&mut self, s: usize, id: u8, y0: usize, y1: usize, d: &[u8]) {
        let (mut pos, mut flag, mut mask) = (0usize, 0u32, 0u32);
        let next_bit = |pos: &mut usize, flag: &mut u32, mask: &mut u32| -> Option<bool> {
            *mask >>= 1;
            if *mask == 0 {
                if *pos + 4 > d.len() {
                    return None;
                }
                *flag = be32(d, *pos);
                *pos += 4;
                *mask = 0x8000_0000;
            }
            Some(*flag & *mask != 0)
        };
        let mut y = y0;
        while y < y1 {
            let mut x = 0;
            while x < self.width {
                let coded = if id & 1 != 0 {
                    match next_bit(&mut pos, &mut flag, &mut mask) {
                        Some(b) => b,
                        None => return,
                    }
                } else {
                    true
                };
                if coded {
                    let v4 = if id & 2 != 0 {
                        false
                    } else {
                        match next_bit(&mut pos, &mut flag, &mut mask) {
                            Some(b) => b,
                            None => return,
                        }
                    };
                    if !v4 {
                        if pos >= d.len() {
                            return;
                        }
                        let e = self.strips[s].v1[d[pos] as usize];
                        pos += 1;
                        // Each of the 4 luma values covers a 2x2 square.
                        for (k, yy) in e.y.iter().enumerate() {
                            let (bx, by) = (x + (k & 1) * 2, y + (k >> 1) * 2);
                            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                                self.put(bx + dx, by + dy, *yy, e.u, e.v);
                            }
                        }
                    } else {
                        if pos + 4 > d.len() {
                            return;
                        }
                        for q in 0..4 {
                            let e = self.strips[s].v4[d[pos + q] as usize];
                            let (bx, by) = (x + (q & 1) * 2, y + (q >> 1) * 2);
                            for (k, yy) in e.y.iter().enumerate() {
                                self.put(bx + (k & 1), by + (k >> 1), *yy, e.u, e.v);
                            }
                        }
                        pos += 4;
                    }
                }
                x += 4;
            }
            y += 4;
        }
    }
}

fn decode_codebook(cb: &mut [Entry; 256], id: u8, d: &[u8]) {
    let n = if id & 0x04 != 0 { 4 } else { 6 };
    let selective = id & 0x01 != 0;
    let (mut pos, mut flag, mut mask) = (0usize, 0u32, 0u32);
    for e in cb.iter_mut() {
        if selective {
            mask >>= 1;
            if mask == 0 {
                if pos + 4 > d.len() {
                    break;
                }
                flag = be32(d, pos);
                pos += 4;
                mask = 0x8000_0000;
            }
            if flag & mask == 0 {
                continue;
            }
        }
        if pos + n > d.len() {
            break;
        }
        e.y.copy_from_slice(&d[pos..pos + 4]);
        if n == 6 {
            e.u = d[pos + 4] as i8;
            e.v = d[pos + 5] as i8;
        } else {
            e.u = 0;
            e.v = 0;
        }
        pos += n;
    }
}

// ---- MCI device + playback -------------------------------------------------

struct Playing {
    avi: Avi,
    decoder: Cinepak,
    /// Microsecond clock at frame 0.
    start_us: u64,
    next_frame: usize,
    audio_pos: usize,
    notify_hwnd: u32,
    device: u32,
    surface: Option<host::Surface>,
    audio: Option<host::AudioStream>,
}

struct Device {
    path: Option<std::path::PathBuf>,
    dest: [i32; 4],
    playing: Option<Playing>,
}

// The host window/surface/audio are main-thread objects; MCI calls and the
// pump both run on the game's main thread.
struct Devices(Vec<Device>);
unsafe impl Send for Devices {}
static DEVICES: Mutex<Devices> = Mutex::new(Devices(Vec::new()));
/// Notifications for the message queue: (hwnd, wParam, lParam).
static PENDING: Mutex<Vec<(u32, u32, u32)>> = Mutex::new(Vec::new());

pub fn active() -> bool {
    DEVICES.lock().unwrap().0.iter().any(|d| d.playing.is_some())
}

/// MCI notifications ready to be posted: (hwnd, MM_MCINOTIFY, wParam, lParam).
pub fn take_notifications() -> Vec<(u32, u32, u32, u32)> {
    PENDING.lock().unwrap().drain(..).map(|(h, w, l)| (h, MM_MCINOTIFY, w, l)).collect()
}

fn notify(hwnd: u32, code: u32, device: u32) {
    if hwnd != 0 {
        PENDING.lock().unwrap().push((hwnd, code, device));
    }
}

/// mciSendCommandA for the avivideo device. Returns the MCI error (0 = ok).
pub fn send_command(ctx: &mut Context, id: u32, msg: u32, flags: u32, params: u32) -> u32 {
    let mut devs = DEVICES.lock().unwrap();
    let devs = &mut devs.0;
    let rd = |o: u32| ctx.memory.read::<u32>(params + o);
    match msg {
        0x803 => {
            // MCI_OPEN: MCI_OPEN_PARMS { callback, wDeviceID, type, element, alias, [style, parent] }
            let path = if flags & 0x200 != 0 && rd(12) != 0 {
                let name = ctx.memory.read_str(rd(12));
                Some(crate::kernel32::resolve_path(&name))
            } else {
                None
            };
            devs.push(Device { path, dest: [0, 0, 0, 0], playing: None });
            let new_id = devs.len() as u32;
            ctx.memory.write::<u32>(params + 4, new_id);
            log::warn!("rr-video: MCI_OPEN -> device {new_id}");
            0
        }
        _ => {
            let Some(dev) = (id as usize).checked_sub(1).and_then(|i| devs.get_mut(i)) else {
                return 0; // e.g. id 0 before any open: accept and ignore
            };
            match msg {
                0x842 => {
                    // MCI_PUT: destination rect (left, top, width, height)
                    if flags & 0x40000 != 0 {
                        dev.dest = [rd(4) as i32, rd(8) as i32, rd(12) as i32, rd(16) as i32];
                    }
                    0
                }
                0x843 => {
                    // MCI_WHERE: report the video size as the source rect.
                    let (w, h) = dev.path.as_ref().and_then(|p| host::fs::read(p).ok()).and_then(|d| parse_avi(&d)).map(|a| (a.width, a.height)).unwrap_or((320, 192));
                    for (i, v) in [0, 0, w, h].iter().enumerate() {
                        ctx.memory.write::<u32>(params + 4 + 4 * i as u32, *v);
                    }
                    0
                }
                0x814 => {
                    // MCI_STATUS: LENGTH (1), POSITION (2), MODE (4)
                    let item = rd(8);
                    let value = match (item, &dev.playing) {
                        (1, Some(p)) => p.avi.frames.len() as u32,
                        (1, None) => dev.path.as_ref().and_then(|p| host::fs::read(p).ok()).and_then(|d| parse_avi(&d)).map(|a| a.frames.len() as u32).unwrap_or(0),
                        (2, Some(p)) => p.next_frame as u32,
                        (4, Some(_)) => 526, // MCI_MODE_PLAY
                        (4, None) => 525,    // MCI_MODE_STOP
                        _ => 0,
                    };
                    ctx.memory.write::<u32>(params + 4, value);
                    0
                }
                0x806 => {
                    // MCI_PLAY { callback, from, to }
                    let notify_hwnd = if flags & 1 != 0 { rd(0) & 0xffff_ffff } else { 0 };
                    let Some(path) = dev.path.clone() else { return 0 };
                    let Some(avi) = host::fs::read(&path).ok().and_then(|d| parse_avi(&d)) else {
                        log::warn!("rr-video: can't parse {path:?}");
                        notify(notify_hwnd, MCI_NOTIFY_SUCCESSFUL, id);
                        return 0;
                    };
                    log::warn!(
                        "rr-video: playing {:?}: {}x{}, {} frames at {:.1} fps, {} audio samples at {} Hz",
                        path.file_name().unwrap_or_default(),
                        avi.width,
                        avi.height,
                        avi.frames.len(),
                        1e6 / avi.us_per_frame as f64,
                        avi.audio.len() / 2,
                        avi.audio_rate
                    );
                    let decoder = Cinepak::new(avi.width as usize, avi.height as usize);
                    dev.playing = Some(Playing {
                        avi,
                        decoder,
                        start_us: crate::rr_resdll::micros(),
                        next_frame: 0,
                        audio_pos: 0,
                        notify_hwnd,
                        device: id,
                        surface: None,
                        audio: None,
                    });
                    0
                }
                0x808 | 0x804 | 0x809 => {
                    // MCI_STOP / MCI_CLOSE / MCI_PAUSE
                    if let Some(p) = dev.playing.take() {
                        notify(p.notify_hwnd, MCI_NOTIFY_ABORTED, p.device);
                    }
                    0
                }
                _ => 0, // MCI_WINDOW, MCI_CUE, MCI_SEEK, ...: accepted
            }
        }
    }
}

/// Called from the main thread's message loop: decode and show due frames,
/// keep the audio queue topped up, and finish videos.
pub fn pump() {
    let mut devs = DEVICES.lock().unwrap();
    for dev in devs.0.iter_mut() {
        let Some(p) = dev.playing.as_mut() else { continue };
        let now = crate::rr_resdll::micros();
        let elapsed = now.saturating_sub(p.start_us);
        let due = ((elapsed / p.avi.us_per_frame.max(1) as u64) as usize + 1).min(p.avi.frames.len());

        // Audio: keep ~200 ms queued, resampled to the host's 44.1 kHz stereo.
        if p.audio.is_none() && !p.avi.audio.is_empty() {
            let stream = host::host().create_audio_stream(host::AudioSpec { sample_rate: 44100, channels: 2 });
            stream.resume();
            p.audio = Some(stream);
        }
        if let Some(stream) = p.audio.as_ref() {
            if stream.is_open() {
                let ratio = 44100.0 / p.avi.audio_rate as f64;
                let target_out = (elapsed as f64 / 1e6 * 44100.0) as usize + 44100 / 5;
                let target_in = ((target_out as f64 / ratio) as usize * 2).min(p.avi.audio.len());
                if target_in > p.audio_pos {
                    let mut bytes = Vec::new();
                    let mut i = p.audio_pos as f64 / 2.0;
                    while ((i as usize) * 2) < target_in {
                        let k = (i as usize) * 2;
                        for s in [p.avi.audio[k], p.avi.audio[k + 1]] {
                            bytes.extend_from_slice(&s.to_le_bytes());
                        }
                        i += 1.0 / ratio;
                    }
                    p.audio_pos = target_in;
                    stream.put_data(&bytes);
                }
            }
        }

        if due > p.next_frame {
            while p.next_frame < due {
                let frame = std::mem::take(&mut p.avi.frames[p.next_frame]);
                p.decoder.decode_frame(&frame);
                p.next_frame += 1;
            }
            show(p, dev.dest);
        }
        if p.next_frame >= p.avi.frames.len() && elapsed >= p.avi.frames.len() as u64 * p.avi.us_per_frame as u64 {
            let (hwnd, device) = (p.notify_hwnd, p.device);
            dev.playing = None;
            notify(hwnd, MCI_NOTIFY_SUCCESSFUL, device);
            log::warn!("rr-video: finished");
        }
    }
}

/// Draw the current frame into the game window (640x480), scaled into the
/// MCI destination rectangle (or centred at 2x when none was set).
fn show(p: &mut Playing, dest: [i32; 4]) {
    let Some(window) = crate::user32::state().window.borrow().clone() else { return };
    let (ww, wh) = (640usize, 480usize);
    let (vw, vh) = (p.decoder.width.max(1), p.decoder.height.max(1));
    let (dx, dy, dw, dh) = if dest[2] > 0 && dest[3] > 0 {
        (dest[0].max(0) as usize, dest[1].max(0) as usize, dest[2] as usize, dest[3] as usize)
    } else {
        let (dw, dh) = (vw * 2, vh * 2);
        ((ww.saturating_sub(dw)) / 2, (wh.saturating_sub(dh)) / 2, dw, dh)
    };
    let mut rgba = vec![0u8; ww * wh * 4];
    for y in 0..dh.min(wh - dy.min(wh)) {
        let sy = y * vh / dh;
        for x in 0..dw.min(ww - dx.min(ww)) {
            let sx = x * vw / dw;
            let c = p.decoder.rgb[sy * vw + sx];
            let o = ((dy + y) * ww + dx + x) * 4;
            rgba[o..o + 3].copy_from_slice(&c);
        }
    }
    let mut w = window.borrow_mut();
    if p.surface.is_none() {
        p.surface = Some(w.host.create_surface(ww as u32, wh as u32));
    }
    let surface = p.surface.as_mut().unwrap();
    surface.set_pixels(&rgba, (ww * 4) as u32);
    w.host.render(surface);
    if let Ok(path) = std::env::var("RR_VIDEO_DUMP") {
        // Evaluation aid: save one frame per second of video as PPM.
        if p.next_frame % 15 == 1 {
            let mut out = format!("P6\n{ww} {wh}\n255\n").into_bytes();
            for px in rgba.chunks_exact(4) {
                out.extend_from_slice(&px[..3]);
            }
            let _ = std::fs::write(format!("{path}/video{:05}.ppm", p.next_frame), out);
        }
    }
}
