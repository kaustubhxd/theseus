//! Road Rash evaluation (not upstream): GDI text drawn onto DirectDraw surfaces.
//!
//! Road Rash draws all its menu, credits and HUD text with DrawTextExA on a
//! DC obtained from IDirectDrawSurface::GetDC, using TrueType fonts it ships
//! and registers with AddFontResourceA. This module keeps its own DCs (one per
//! surface) and fonts, and rasterizes text with fontdue straight into the
//! surface's pixels.

use std::{collections::HashMap, sync::Mutex};

use runtime::Context;

pub const DC_BASE: u32 = 0x5dc0_0000;
pub const FONT_BASE: u32 = 0x5f00_0000;

struct LoadedFont {
    family: String,
    full: String,
    bold: bool,
    font: fontdue::Font,
}

#[derive(Clone)]
struct LogFont {
    height: i32,
    weight: u32,
    face: String,
}

#[derive(Clone, Copy)]
struct SurfDc {
    surface: u32,
    font: u32,
    text_color: u32,
    bk_color: u32,
    bk_mode: u32,
    pen_color: u32,
    pos: (i32, i32),
}

#[derive(Default)]
struct State {
    fonts: Vec<LoadedFont>,
    logfonts: Vec<LogFont>,
    dcs: Vec<SurfDc>,
    by_surface: HashMap<u32, u32>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut s = STATE.lock().unwrap();
    f(s.get_or_insert_with(Default::default))
}

// ---- fonts ------------------------------------------------------------

/// (family, full name, subfamily) from a TrueType `name` table.
fn ttf_names(d: &[u8]) -> Option<(String, String, String)> {
    let be16 = |o: usize| -> Option<u16> { Some(u16::from_be_bytes(d.get(o..o + 2)?.try_into().ok()?)) };
    let be32 = |o: usize| -> Option<u32> { Some(u32::from_be_bytes(d.get(o..o + 4)?.try_into().ok()?)) };
    let n = be16(4)? as usize;
    let mut name_off = None;
    for i in 0..n {
        let rec = 12 + 16 * i;
        if d.get(rec..rec + 4)? == b"name" {
            name_off = Some(be32(rec + 8)? as usize);
        }
    }
    let off = name_off?;
    let count = be16(off + 2)? as usize;
    let strings = off + be16(off + 4)? as usize;
    let (mut family, mut sub, mut full) = (None, None, None);
    for i in 0..count {
        let r = off + 6 + 12 * i;
        let (pid, nid, len, o) = (be16(r)?, be16(r + 6)?, be16(r + 8)? as usize, be16(r + 10)? as usize);
        if pid != 3 {
            continue;
        }
        let raw = d.get(strings + o..strings + o + len)?;
        let s = String::from_utf16_lossy(&raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>());
        match nid {
            1 => family = Some(s),
            2 => sub = Some(s),
            4 => full = Some(s),
            _ => {}
        }
    }
    Some((family?, full.unwrap_or_default(), sub.unwrap_or_default()))
}

pub fn add_font_resource(ctx: &mut Context, lp_path: u32) -> u32 {
    let name = ctx.memory.read_str(lp_path);
    let path = crate::kernel32::resolve_path(&name);
    let Ok(bytes) = host::fs::read(&path) else {
        log::warn!("rr-text: AddFontResource({name:?}): cannot read {path:?}");
        return 0;
    };
    let Some((family, full, sub)) = ttf_names(&bytes) else {
        log::warn!("rr-text: AddFontResource({name:?}): not a TrueType font");
        return 0;
    };
    let font = match fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default()) {
        Ok(f) => f,
        Err(e) => {
            log::warn!("rr-text: AddFontResource({name:?}): {e}");
            return 0;
        }
    };
    let bold = sub.to_ascii_lowercase().contains("bold");
    log::warn!("rr-text: loaded font {full:?} (family {family:?}, bold={bold}) from {name:?}");
    with(|s| {
        s.fonts.push(LoadedFont { family: family.to_ascii_lowercase(), full: full.to_ascii_lowercase(), bold, font })
    });
    1
}

pub fn create_font(ctx: &mut Context, height: u32, weight: u32, face_ptr: u32) -> u32 {
    let face = if face_ptr != 0 { ctx.memory.read_str(face_ptr) } else { String::new() };
    let lf = LogFont { height: height as i32, weight, face: face.to_ascii_lowercase() };
    log::info!("rr-text: CreateFont({height}, weight {weight}, {face:?})");
    with(|s| {
        s.logfonts.push(lf);
        FONT_BASE + s.logfonts.len() as u32
    })
}

/// Pick a loaded font for a logical font: same family/full name, bold-ness by weight.
fn pick(s: &State, lf: &LogFont) -> Option<usize> {
    let want_bold = lf.weight >= 600;
    let cands: Vec<usize> = (0..s.fonts.len())
        .filter(|&i| s.fonts[i].family == lf.face || s.fonts[i].full == lf.face)
        .collect();
    let pool = if cands.is_empty() { (0..s.fonts.len()).collect() } else { cands };
    pool.iter().copied().find(|&i| s.fonts[i].bold == want_bold).or(pool.first().copied())
}

/// Pixel size (em) for the logical font's height.
fn px_for(font: &fontdue::Font, height: i32) -> f32 {
    if height < 0 {
        -height as f32
    } else if height == 0 {
        16.0
    } else {
        let m = font.horizontal_line_metrics(100.0).unwrap();
        height as f32 * 100.0 / (m.ascent - m.descent)
    }
}

// ---- DCs --------------------------------------------------------------

pub fn surface_dc(surface: u32) -> u32 {
    with(|s| {
        if let Some(&h) = s.by_surface.get(&surface) {
            return h;
        }
        s.dcs.push(SurfDc { surface, font: 0, text_color: 0, bk_color: 0xffffff, bk_mode: 2, pen_color: 0, pos: (0, 0) });
        let h = DC_BASE + s.dcs.len() as u32;
        s.by_surface.insert(surface, h);
        h
    })
}

fn pen_color(h: u32) -> Option<u32> {
    let gdi = crate::gdi32::lock();
    match gdi.objects.get(crate::gdi32::HGDIOBJ::from_raw(h))? {
        crate::gdi32::Object::Pen(p) => Some(p.0.as_win32()),
        _ => None,
    }
}

pub fn dc_index(h: u32) -> Option<usize> {
    if h > DC_BASE && h < DC_BASE + 0x10000 { Some((h - DC_BASE - 1) as usize) } else { None }
}

pub fn is_font(h: u32) -> bool {
    h > FONT_BASE && h < FONT_BASE + 0x10000
}

/// SelectObject hook: Some(previous) if handled here.
pub fn select_object(hdc: u32, h: u32) -> Option<u32> {
    match dc_index(hdc) {
        Some(i) => Some(with(|s| {
            let dc = &mut s.dcs[i];
            if is_font(h) {
                std::mem::replace(&mut dc.font, h)
            } else if let Some(color) = pen_color(h) {
                dc.pen_color = color;
                0
            } else {
                0 // brushes etc. on surface DCs: ignored
            }
        })),
        None if is_font(h) => Some(0), // our font into a real GDI DC: ignored
        None => None,
    }
}

fn set_field(hdc: u32, f: impl FnOnce(&mut SurfDc) -> u32) -> u32 {
    match dc_index(hdc) {
        Some(i) => with(|s| f(&mut s.dcs[i])),
        None => 0,
    }
}
pub fn set_text_color(hdc: u32, c: u32) -> u32 {
    set_field(hdc, |d| std::mem::replace(&mut d.text_color, c))
}
pub fn set_bk_color(hdc: u32, c: u32) -> u32 {
    set_field(hdc, |d| std::mem::replace(&mut d.bk_color, c))
}
pub fn set_bk_mode(hdc: u32, m: u32) -> u32 {
    set_field(hdc, |d| std::mem::replace(&mut d.bk_mode, m))
}

// ---- text -------------------------------------------------------------

/// Windows-1252 byte to char.
pub fn cp1252(b: u8) -> char {
    const HI: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x8D,
        0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A,
        0x0153, 0x9D, 0x017E, 0x0178,
    ];
    if (0x80..0xa0).contains(&b) { char::from_u32(HI[(b - 0x80) as usize] as u32).unwrap() } else { b as char }
}

/// char to Windows-1252 byte ('?' if unmappable).
pub fn to_cp1252(c: char) -> u8 {
    let u = c as u32;
    if u < 0x80 || (0xa0..0x100).contains(&u) {
        return u as u8;
    }
    (0x80u8..0xa0).find(|&b| cp1252(b) == c).unwrap_or(b'?')
}

struct Run<'a> {
    /// Wingdings requested: we have no such font; known symbols are drawn by hand.
    symbol: bool,
    font: &'a fontdue::Font,
    px: f32,
    ascent: f32,
    line_h: f32,
    tab_w: f32,
}

impl Run<'_> {
    fn advance(&self, c: char) -> f32 {
        self.font.metrics(c, self.px).advance_width
    }
    /// Width of a line, with tabs expanded relative to `origin`.
    fn width(&self, line: &[char]) -> f32 {
        let mut x = 0.0;
        for &c in line {
            x = if c == '\t' { ((x / self.tab_w).floor() + 1.0) * self.tab_w } else { x + self.advance(c) };
        }
        x
    }
}

struct Target {
    addr: u32,
    width: u32,
    height: u32,
    bpp: u32,
}

fn target_for(ctx: &mut Context, surface: u32) -> Option<Target> {
    let surf = crate::ddraw::state().surf.borrow().get(&surface)?.clone();
    let mut surf = surf.borrow_mut();
    let addr = surf.lock(&mut ctx.memory);
    Some(Target { addr, width: surf.width, height: surf.height, bpp: surf.bytes_per_pixel })
}

fn put(ctx: &mut Context, t: &Target, x: i32, y: i32, color: u32, a: u8, clip: (i32, i32, i32, i32)) {
    if x < clip.0 || y < clip.1 || x >= clip.2 || y >= clip.3 || x < 0 || y < 0 || x >= t.width as i32 || y >= t.height as i32 || a == 0 {
        return;
    }
    let (r, g, b) = ((color & 0xff) as u32, ((color >> 8) & 0xff) as u32, ((color >> 16) & 0xff) as u32);
    let a = a as u32;
    let at = t.addr + (y as u32 * t.width + x as u32) * t.bpp;
    match t.bpp {
        2 => {
            let old = ctx.memory.read::<u16>(at) as u32;
            let (or, og, ob) = (((old >> 11) & 0x1f) << 3, ((old >> 5) & 0x3f) << 2, (old & 0x1f) << 3);
            let mix = |n: u32, o: u32| (n * a + o * (255 - a)) / 255;
            let (nr, ng, nb) = (mix(r, or), mix(g, og), mix(b, ob));
            ctx.memory.write::<u16>(at, ((nr >> 3) << 11 | (ng >> 2) << 5 | (nb >> 3)) as u16);
        }
        4 => {
            let old: [u8; 4] = ctx.memory.read(at);
            let mix = |n: u32, o: u8| ((n * a + o as u32 * (255 - a)) / 255) as u8;
            ctx.memory.write::<[u8; 4]>(at, [mix(r, old[0]), mix(g, old[1]), mix(b, old[2]), old[3]]);
        }
        _ => {}
    }
}

const DT_CENTER: u32 = 0x1;
const DT_RIGHT: u32 = 0x2;
const DT_VCENTER: u32 = 0x4;
const DT_BOTTOM: u32 = 0x8;
const DT_WORDBREAK: u32 = 0x10;
const DT_SINGLELINE: u32 = 0x20;
const DT_NOCLIP: u32 = 0x100;
const DT_CALCRECT: u32 = 0x400;

fn read_text(ctx: &Context, ptr: u32, len: u32) -> Vec<char> {
    let bytes: Vec<u8> = if len == 0xffff_ffff {
        ctx.memory.read_cstr(ptr).to_vec()
    } else {
        ctx.memory[ptr..][..len as usize].to_vec()
    };
    bytes.into_iter().map(cp1252).collect()
}

/// DrawTextA / DrawTextExA. Returns the text height, as Windows does.
pub fn draw_text(ctx: &mut Context, hdc: u32, text: u32, len: u32, rect_ptr: u32, format: u32, params: u32) -> u32 {
    let Some(i) = dc_index(hdc) else { return 0 };
    let dc = with(|s| s.dcs[i]);
    let chars = read_text(ctx, text, len);
    let rect: [i32; 4] = ctx.memory.read(rect_ptr);
    let (tab_len, left_margin, right_margin) = if params != 0 {
        let p: [i32; 4] = ctx.memory.read(params);
        (if p[1] > 0 { p[1] } else { 8 }, p[2], p[3])
    } else {
        (8, 0, 0)
    };

    let mut st = STATE.lock().unwrap();
    let s = st.get_or_insert_with(Default::default);
    let lf = if is_font(dc.font) { s.logfonts[(dc.font - FONT_BASE - 1) as usize].clone() } else { LogFont { height: 16, weight: 400, face: String::new() } };
    let Some(fi) = pick(s, &lf) else { return 0 };
    let font = &s.fonts[fi].font;
    let px = px_for(font, lf.height);
    let m = font.horizontal_line_metrics(px).unwrap();
    let mut run = Run { symbol: lf.face == "wingdings", font, px, ascent: m.ascent, line_h: (m.ascent - m.descent).ceil(), tab_w: 1.0 };
    run.tab_w = (run.advance('x') * tab_len as f32).max(1.0);

    // Lines (split on newlines unless single-line), then word wrap.
    let avail = (rect[2] - rect[0] - left_margin - right_margin) as f32;
    let mut lines: Vec<Vec<char>> = vec![];
    let raw: Vec<Vec<char>> = if format & DT_SINGLELINE != 0 {
        vec![chars.iter().copied().filter(|c| *c != '\r' && *c != '\n').collect()]
    } else {
        chars.split(|c| *c == '\n').map(|l| l.iter().copied().filter(|c| *c != '\r').collect()).collect()
    };
    for line in raw {
        if format & DT_WORDBREAK == 0 || format & DT_SINGLELINE != 0 || run.width(&line) <= avail {
            lines.push(line);
            continue;
        }
        let mut cur: Vec<char> = vec![];
        for word in line.split_inclusive(|c| *c == ' ') {
            let mut try_line = cur.clone();
            try_line.extend_from_slice(word);
            if !cur.is_empty() && run.width(&try_line) > avail {
                lines.push(std::mem::take(&mut cur));
                cur.extend_from_slice(word);
            } else {
                cur = try_line;
            }
        }
        lines.push(cur);
    }

    let total_h = run.line_h as i32 * lines.len() as i32;
    if format & DT_CALCRECT != 0 {
        let w = lines.iter().map(|l| run.width(l).ceil() as i32).max().unwrap_or(0);
        ctx.memory.write::<[i32; 4]>(rect_ptr, [rect[0], rect[1], rect[0] + w + left_margin + right_margin, rect[1] + total_h]);
        return total_h as u32;
    }

    let mut top = rect[1];
    if format & DT_SINGLELINE != 0 {
        if format & DT_VCENTER != 0 {
            top = rect[1] + (rect[3] - rect[1] - total_h) / 2;
        } else if format & DT_BOTTOM != 0 {
            top = rect[3] - total_h;
        }
    }
    let clip = if format & DT_NOCLIP != 0 { (i32::MIN, i32::MIN, i32::MAX, i32::MAX) } else { (rect[0], rect[1], rect[2], rect[3]) };
    let Some(target) = target_for(ctx, dc.surface) else { return total_h as u32 };
    for (n, line) in lines.iter().enumerate() {
        let w = run.width(line);
        let mut x0 = (rect[0] + left_margin) as f32;
        if format & DT_CENTER != 0 {
            x0 = rect[0] as f32 + ((rect[2] - rect[0]) as f32 - w) / 2.0;
        } else if format & DT_RIGHT != 0 {
            x0 = (rect[2] - right_margin) as f32 - w;
        }
        let ly = top + n as i32 * run.line_h as i32;
        if dc.bk_mode == 2 {
            for y in ly..ly + run.line_h as i32 {
                for x in x0 as i32..(x0 + w).ceil() as i32 {
                    put(ctx, &target, x, y, dc.bk_color, 255, clip);
                }
            }
        }
        draw_line(ctx, &target, &run, line, x0, ly as f32 + run.ascent, dc.text_color, clip);
    }
    total_h as u32
}

fn draw_line(ctx: &mut Context, t: &Target, run: &Run, line: &[char], x0: f32, baseline: f32, color: u32, clip: (i32, i32, i32, i32)) {
    let mut pen = 0.0f32;
    for &c in line {
        if c == '\t' {
            pen = ((pen / run.tab_w).floor() + 1.0) * run.tab_w;
            continue;
        }
        if run.symbol && c == '\u{e8}' {
            // Wingdings 0xE8: a right-pointing arrow (shaft + head), em-sized.
            let em = run.px;
            let (x0i, mid) = ((x0 + pen).round() as i32, (baseline - em * 0.35).round() as i32);
            let (len, half) = ((em * 0.8) as i32, (em * 0.3) as i32);
            let head = half * 2;
            for x in 0..len {
                let h = if x < len - head { (em * 0.08).max(1.0) as i32 } else { (len - x) * half / head };
                for y in -h..=h {
                    put(ctx, t, x0i + x, mid + y, color, 255, clip);
                }
            }
            pen += em;
            continue;
        }
        let (gm, cov) = run.font.rasterize(c, run.px);
        let gx = (x0 + pen).round() as i32 + gm.xmin;
        let gy = baseline.round() as i32 - gm.ymin - gm.height as i32;
        for yy in 0..gm.height {
            for xx in 0..gm.width {
                put(ctx, t, gx + xx as i32, gy + yy as i32, color, cov[yy * gm.width + xx], clip);
            }
        }
        pen += gm.advance_width;
    }
}

pub fn text_out(ctx: &mut Context, hdc: u32, x: i32, y: i32, text: u32, len: u32) -> u32 {
    let rect = [x, y, x + 4096, y + 4096];
    let tmp = crate::kernel32::lock().process_heap.alloc(&mut ctx.memory, 16);
    ctx.memory.write::<[i32; 4]>(tmp, rect);
    draw_text(ctx, hdc, text, len, tmp, DT_NOCLIP | DT_SINGLELINE, 0);
    crate::kernel32::lock().process_heap.free(&mut ctx.memory, tmp);
    1
}

pub fn text_extent(ctx: &mut Context, hdc: u32, text: u32, len: u32, size_ptr: u32) -> u32 {
    let tmp = crate::kernel32::lock().process_heap.alloc(&mut ctx.memory, 16);
    ctx.memory.write::<[i32; 4]>(tmp, [0, 0, 0, 0]);
    let h = draw_text(ctx, hdc, text, len, tmp, DT_CALCRECT | DT_SINGLELINE, 0);
    let r: [i32; 4] = ctx.memory.read(tmp);
    crate::kernel32::lock().process_heap.free(&mut ctx.memory, tmp);
    ctx.memory.write::<[i32; 2]>(size_ptr, [r[2], h as i32]);
    1
}

/// MoveToEx on a surface DC. Returns false if `hdc` isn't ours.
pub fn move_to(ctx: &mut Context, hdc: u32, x: i32, y: i32, old: u32) -> bool {
    let Some(i) = dc_index(hdc) else { return false };
    let prev = with(|s| std::mem::replace(&mut s.dcs[i].pos, (x, y)));
    if old != 0 {
        ctx.memory.write::<[i32; 2]>(old, [prev.0, prev.1]);
    }
    true
}

/// LineTo on a surface DC (last point excluded, as in GDI).
pub fn line_to(ctx: &mut Context, hdc: u32, x1: i32, y1: i32) -> bool {
    let Some(i) = dc_index(hdc) else { return false };
    let dc = with(|s| {
        let before = s.dcs[i];
        s.dcs[i].pos = (x1, y1);
        before
    });
    let Some(t) = target_for(ctx, dc.surface) else { return true };
    let (mut x, mut y) = dc.pos;
    let (dx, dy) = ((x1 - x).abs(), -(y1 - y).abs());
    let (sx, sy) = (if x < x1 { 1 } else { -1 }, if y < y1 { 1 } else { -1 });
    let mut err = dx + dy;
    let clip = (i32::MIN, i32::MIN, i32::MAX, i32::MAX);
    while (x, y) != (x1, y1) {
        put(ctx, &t, x, y, dc.pen_color, 255, clip);
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    true
}
