//! Road Rash evaluation (not upstream): minimal support for several window
//! classes and several windows. Upstream Theseus keeps a single class and a
//! single window (HWND 1); Road Rash registers 3 classes and creates dozens of
//! child windows that it identifies through Get/SetWindowWord.
//!
//! Only the top-level window is backed by a host window. Child windows are
//! bookkeeping only: they get their own HWND, class (and so window procedure)
//! and parent, so messages reach the right procedure.

use std::{cell::RefCell, collections::BTreeMap};

use runtime::Context;

use super::{HWND, WndClass};

pub struct ClassEntry {
    pub name: String,
    pub class: WndClass,
}

pub struct WindowEntry {
    pub class: usize,
    pub parent: u32,
    /// left, top, right, bottom, relative to the parent's client area.
    pub rect: [i32; 4],
    pub enabled: bool,
    pub visible: bool,
}

#[derive(Default)]
pub struct RrWindows {
    pub classes: Vec<ClassEntry>,
    pub windows: BTreeMap<u32, WindowEntry>,
    next_child: u32,
}

thread_local! {
    pub static RR: RefCell<RrWindows> = RefCell::new(RrWindows { next_child: 0x100, ..Default::default() });
}

/// Read a class name argument: either a pointer to a string or an atom.
pub fn class_name(ctx: &Context, ptr: u32) -> String {
    if ptr >> 16 == 0 {
        format!("#atom{ptr}")
    } else {
        ctx.memory.read_str(ptr).to_ascii_lowercase()
    }
}

pub fn register(name: String, class: WndClass) -> u16 {
    RR.with_borrow_mut(|rr| {
        rr.classes.retain(|c| c.name != name);
        rr.classes.push(ClassEntry { name, class });
        rr.classes.len() as u16
    })
}

fn find_class(rr: &RrWindows, name: &str) -> Option<usize> {
    rr.classes.iter().position(|c| c.name == name)
}

/// Record a window; returns the class's wndproc, if the class is known.
pub fn add_window(hwnd: u32, class: &str, parent: u32, rect: [i32; 4], style: u32) -> Option<runtime::Cont> {
    RR.with_borrow_mut(|rr| {
        let Some(class) = find_class(rr, class) else {
            log::warn!("rr: window {hwnd:#x} of unknown class {class:?}");
            return None;
        };
        let (enabled, visible) = (style & 0x0800_0000 == 0, style & 0x1000_0000 != 0 || parent == 0);
        rr.windows.insert(hwnd, WindowEntry { class, parent, rect, enabled, visible });
        Some(rr.classes[class].class.wndproc)
    })
}

pub fn new_child_hwnd() -> u32 {
    RR.with_borrow_mut(|rr| {
        rr.next_child += 1;
        rr.next_child
    })
}

pub fn wndproc_for(hwnd: u32) -> Option<runtime::Cont> {
    RR.with_borrow(|rr| {
        let w = rr.windows.get(&hwnd)?;
        Some(rr.classes[w.class].class.wndproc)
    })
}

pub fn parent_of(hwnd: u32) -> u32 {
    RR.with_borrow(|rr| rr.windows.get(&hwnd).map(|w| w.parent).unwrap_or(0))
}

/// Call a window procedure synchronously and return its result (SendMessage).
pub fn call_wndproc(ctx: &mut Context, wndproc: runtime::Cont, hwnd: u32, msg: u32, wparam: u32, lparam: u32) -> u32 {
    ctx.call32_x86(wndproc, vec![hwnd, msg, wparam, lparam]);
    ctx.cpu.regs.eax
}

/// Send WM_CREATE with a CREATESTRUCTA, as CreateWindowEx does on Windows.
pub fn send_create(ctx: &mut Context, wndproc: runtime::Cont, hwnd: u32, cs: [u32; 12]) {
    let addr = crate::kernel32::lock().process_heap.alloc(&mut ctx.memory, 48);
    for (i, v) in cs.iter().enumerate() {
        ctx.memory.write::<u32>(addr + 4 * i as u32, *v);
    }
    let ret = call_wndproc(ctx, wndproc, hwnd, 0x0001 /* WM_CREATE */, 0, addr);
    log::warn!("rr: WM_CREATE hwnd={hwnd:#x} -> {ret:#x}");
    crate::kernel32::lock().process_heap.free(&mut ctx.memory, addr);
}

pub fn hwnd_raw(h: HWND) -> u32 {
    h.to_raw()
}

/// Screen rectangle of a window (top-level windows are at 0,0).
pub fn screen_rect(hwnd: u32) -> Option<[i32; 4]> {
    RR.with_borrow(|rr| {
        let mut r = rr.windows.get(&hwnd)?.rect;
        let mut p = rr.windows.get(&hwnd)?.parent;
        while let Some(w) = rr.windows.get(&p) {
            r = [r[0] + w.rect[0], r[1] + w.rect[1], r[2] + w.rect[0], r[3] + w.rect[1]];
            p = w.parent;
        }
        Some(r)
    })
}

/// MoveWindow for windows tracked here; false if unknown.
pub fn move_window(hwnd: u32, x: i32, y: i32, w: i32, h: i32) -> bool {
    RR.with_borrow_mut(|rr| match rr.windows.get_mut(&hwnd) {
        Some(e) => {
            e.rect = [x, y, x + w, y + h];
            true
        }
        None => false,
    })
}

// ---- keyboard hook (WH_KEYBOARD) and scripted keys ----------------------

thread_local! {
    static KB_HOOK: std::cell::Cell<Option<runtime::Cont>> = const { std::cell::Cell::new(None) };
}

pub fn set_windows_hook(ctx: &mut Context, kind: u32, proc_: u32) -> u32 {
    if kind == 2 {
        KB_HOOK.set(Some(ctx.indirect32(proc_)));
        log::warn!("rr: WH_KEYBOARD hook installed at {proc_:#x}");
        0x4b00_0002
    } else {
        log::warn!("rr: SetWindowsHookEx type {kind} ignored");
        0
    }
}

pub fn unhook(h: u32) -> u32 {
    if h == 0x4b00_0002 {
        KB_HOOK.set(None);
    }
    1
}

/// Windows calls WH_KEYBOARD hooks when GetMessage/PeekMessage is about to
/// return a keystroke message. A nonzero result discards the message; we
/// turn it into WM_NULL.
pub fn run_keyboard_hook(ctx: &mut Context, msg_ptr: u32, code: u32) {
    let Some(hook) = KB_HOOK.get() else { return };
    let message = ctx.memory.read::<u32>(msg_ptr + 4);
    if !(0x100..=0x105).contains(&message) {
        return;
    }
    let (wparam, lparam) = (ctx.memory.read::<u32>(msg_ptr + 8), ctx.memory.read::<u32>(msg_ptr + 12));
    let ret = call_wndproc_like(ctx, hook, code, wparam, lparam);
    if ret != 0 && code == 0 {
        ctx.memory.write::<u32>(msg_ptr + 4, 0);
    }
}

fn call_wndproc_like(ctx: &mut Context, f: runtime::Cont, a: u32, b: u32, c: u32) -> u32 {
    ctx.call32_x86(f, vec![a, b, c]);
    ctx.cpu.regs.eax
}

struct Scripted {
    at: u32,
    down: bool,
    key: host::KeyMessage,
}

/// Scripted raw messages and clicks: "ms:post/hwnd/msg/wparam/lparam" (hex) and
/// "ms:click/x/y" (decimal).
pub enum Extra {
    Post(u32, u32, u32, u32),
    Click(u32, u32, bool),
}

thread_local! {
    static EXTRA: RefCell<Option<(u32, Vec<(u32, Extra)>)>> = const { RefCell::new(None) };
}

pub fn due_extra() -> Vec<Extra> {
    let now = host::host().time();
    EXTRA.with_borrow_mut(|s| {
        let (t0, ev) = s.get_or_insert_with(|| {
            let mut v = vec![];
            for item in host::env("RR_KEYS").unwrap_or_default().split(',') {
                let Some((ms, rest)) = item.split_once(':') else { continue };
                let Ok(ms) = ms.trim().parse::<u32>() else { continue };
                let parts: Vec<&str> = rest.trim().split('/').collect();
                let hex = |i: usize| u32::from_str_radix(parts.get(i).copied().unwrap_or("0"), 16).unwrap_or(0);
                let dec = |i: usize| parts.get(i).and_then(|p| p.parse().ok()).unwrap_or(0);
                match parts[0] {
                    "post" => v.push((ms, Extra::Post(hex(1), hex(2), hex(3), hex(4)))),
                    "click" => {
                        v.push((ms, Extra::Click(dec(1), dec(2), true)));
                        v.push((ms + 120, Extra::Click(dec(1), dec(2), false)));
                    }
                    _ => {}
                }
            }
            v.sort_by_key(|e| e.0);
            (now, v)
        });
        let rel = now - *t0;
        let mut out = vec![];
        while ev.first().is_some_and(|e| e.0 <= rel) {
            out.push(ev.remove(0).1);
        }
        out
    })
}


thread_local! {
    static SCRIPT: RefCell<Option<(u32, Vec<Scripted>)>> = const { RefCell::new(None) };
}

fn parse_key(name: &str) -> Option<(u8, u8, bool)> {
    // (scancode, vkey, extended)
    Some(match name {
        "enter" => (0x1c, 0x0d, false),
        "esc" => (0x01, 0x1b, false),
        "space" => (0x39, 0x20, false),
        "up" => (0x48, 0x26, true),
        "down" => (0x50, 0x28, true),
        "left" => (0x4b, 0x25, true),
        "right" => (0x4d, 0x27, true),
        "lctrl" => (0x1d, 0xa2, false),
        "lshift" => (0x2a, 0xa0, false),
        "ins" => (0x52, 0x2d, true),
        "n" => (0x31, b'N', false),
        "d" => (0x20, b'D', false),
        "kpplus" => (0x4e, 0x6b, false),
        "kpminus" => (0x4a, 0x6d, false),
        h if h.starts_with("0x") => (0, u8::from_str_radix(&h[2..], 16).ok()?, false),
        _ => return None,
    })
}

pub fn due_keys() -> Vec<(bool, host::KeyMessage)> {
    let now = host::host().time();
    SCRIPT.with_borrow_mut(|s| {
        let (t0, events) = s.get_or_insert_with(|| {
            let mut v = vec![];
            for item in host::env("RR_KEYS").unwrap_or_default().split(',') {
                let Some((ms, name)) = item.split_once(':') else { continue };
                let (Ok(ms), Some((sc, vk, ext))) = (ms.trim().parse::<u32>(), parse_key(name.trim().split('*').next().unwrap())) else { continue };
                // A press longer than a frame or two, so per-frame polling sees it.
                let hold = if let Some((_, h)) = name.split_once('*') { h.parse().unwrap_or(150) } else { 150 };
                for (at, down) in [(ms, true), (ms + hold, false)] {
                    v.push(Scripted { at, down, key: host::KeyMessage { scancode: sc, vkey: vk, extended: ext, repeat: false, text_vkey: 0 } });
                }
            }
            v.sort_by_key(|e| e.at);
            (now, v)
        });
        let rel = now - *t0;
        let mut out = vec![];
        while events.first().is_some_and(|e| e.at <= rel) {
            let e = events.remove(0);
            log::warn!("rr-key: {} vk={:#x} at {rel} ms", if e.down { "down" } else { "up" }, e.key.vkey);
            out.push((e.down, e.key));
        }
        out
    })
}

/// EnableWindow for tracked windows: returns Some(was_disabled).
pub fn enable_window(hwnd: u32, enable: bool) -> Option<bool> {
    RR.with_borrow_mut(|rr| {
        let w = rr.windows.get_mut(&hwnd)?;
        let was_disabled = !w.enabled;
        w.enabled = enable;
        Some(was_disabled)
    })
}

/// Topmost (most recently created) enabled, visible child window under a
/// screen point, with the point in its client coordinates.
pub fn hit_test(x: i32, y: i32) -> Option<(u32, i32, i32)> {
    let candidates: Vec<u32> = RR.with_borrow(|rr| {
        rr.windows.iter().filter(|(h, w)| **h >= 0x100 && w.enabled && w.visible).map(|(h, _)| *h).collect()
    });
    for h in candidates.into_iter().rev() {
        let r = screen_rect(h)?;
        if r[2] > r[0] && r[3] > r[1] && x >= r[0] && x < r[2] && y >= r[1] && y < r[3] {
            return Some((h, x - r[0], y - r[1]));
        }
    }
    None
}

/// Scripted input still to come (so the message loop must poll, not block).
pub fn script_pending() -> bool {
    SCRIPT.with_borrow(|s| s.as_ref().is_some_and(|(_, v)| !v.is_empty()))
        || EXTRA.with_borrow(|s| s.as_ref().is_some_and(|(_, v)| !v.is_empty()))
}

// ---- per-thread message queues for worker threads -----------------------

struct ThreadQueues {
    queues: std::sync::Mutex<std::collections::HashMap<u32, std::collections::VecDeque<[u32; 4]>>>,
    cond: std::sync::Condvar,
}

static TQ: std::sync::LazyLock<ThreadQueues> = std::sync::LazyLock::new(|| ThreadQueues {
    queues: Default::default(),
    cond: Default::default(),
});

/// PostThreadMessage to a worker thread (the main thread uses the host-driven queue).
pub fn post_thread_message(tid: u32, msg: u32, w: u32, l: u32) -> u32 {
    if tid == 1 {
        super::post_message(HWND::null(), msg, w, l);
        return 1;
    }
    TQ.queues.lock().unwrap().entry(tid).or_default().push_back([0, msg, w, l]);
    TQ.cond.notify_all();
    1
}

/// GetMessage on a worker thread: block on that thread's own queue.
pub fn worker_get_message(ctx: &mut Context, lp_msg: u32) -> i32 {
    let tid = ctx.thread_id;
    let mut q = TQ.queues.lock().unwrap();
    let m = loop {
        if let Some(m) = q.entry(tid).or_default().pop_front() {
            break m;
        }
        q = TQ.cond.wait(q).unwrap();
    };
    drop(q);
    // MSG: hwnd, message, wParam, lParam, time, pt
    for (i, v) in [m[0], m[1], m[2], m[3], host::host().time(), 0, 0].iter().enumerate() {
        ctx.memory.write::<u32>(lp_msg + 4 * i as u32, *v);
    }
    if m[1] == 0x12 { 0 } else { 1 } // WM_QUIT ends the loop
}

/// PeekMessage on a worker thread.
pub fn worker_peek_message(ctx: &mut Context, lp_msg: u32, remove: bool) -> bool {
    let tid = ctx.thread_id;
    let mut q = TQ.queues.lock().unwrap();
    let queue = q.entry(tid).or_default();
    let Some(&m) = queue.front() else { return false };
    if remove {
        queue.pop_front();
    }
    drop(q);
    for (i, v) in [m[0], m[1], m[2], m[3], host::host().time(), 0, 0].iter().enumerate() {
        ctx.memory.write::<u32>(lp_msg + 4 * i as u32, *v);
    }
    true
}

/// RegisterWindowMessage: a unique id (0xC000..) per name, as on Windows.
pub fn register_window_message(name: &str) -> u32 {
    static NAMES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap();
    let key = name.to_ascii_lowercase();
    let i = names.iter().position(|n| *n == key).unwrap_or_else(|| {
        names.push(key);
        names.len() - 1
    });
    0xc000 + i as u32
}

/// Set once a DirectDraw display mode is active: the window's GDI layer must
/// then not be flushed over the DirectDraw output.
pub static DDRAW_OWNS_WINDOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// DestroyWindow for tracked windows (and their children); false if unknown.
pub fn destroy_window(hwnd: u32) -> bool {
    RR.with_borrow_mut(|rr| {
        if rr.windows.remove(&hwnd).is_none() {
            return false;
        }
        let children: Vec<u32> = rr.windows.iter().filter(|(_, w)| w.parent == hwnd).map(|(h, _)| *h).collect();
        for c in children {
            rr.windows.remove(&c);
        }
        true
    })
}

/// Keyboard focus (SetFocus/GetFocus): keystrokes go to this window.
pub static FOCUS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_focus(hwnd: u32) -> u32 {
    FOCUS.swap(hwnd, std::sync::atomic::Ordering::Relaxed)
}

/// True while the game takes typed text: it moves keyboard focus to a field
/// in the right-hand column of a menu screen (the Restroom's player name box
/// sits at 316,282). Guess: the game has no other marker for text fields; the
/// bottom menu bars (y >= 420) are excluded.
pub fn text_entry() -> bool {
    let f = FOCUS.load(std::sync::atomic::Ordering::Relaxed);
    RR.with_borrow(|rr| {
        let Some(w) = rr.windows.get(&f) else { return false };
        rr.classes.get(w.class).is_some_and(|c| c.name == "rashbutton") && w.rect[0] >= 300 && w.rect[1] < 400
    })
}

thread_local! {
    /// Physical key -> the key the game saw at key down, so the key up matches
    /// even if the WASD remap was switched on or off in between.
    static HELD_KEYS: RefCell<std::collections::HashMap<u8, (u8, u8, bool)>> = Default::default();
}

/// Road Rash evaluation: apply the WASD layout (host::rr_wasd) to a key from
/// the host, except while the game takes typed text.
pub fn map_key(key: host::KeyMessage, down: bool) -> host::KeyMessage {
    if key.text_vkey == 0 {
        return key;
    }
    let physical = key.text_vkey;
    let fresh = || {
        if text_entry() {
            (key.scancode, key.vkey, key.extended)
        } else {
            host::rr_wasd(key.scancode, key.vkey, key.extended)
        }
    };
    let (scancode, vkey, extended) = HELD_KEYS.with_borrow_mut(|held| {
        if down {
            *held.entry(physical).or_insert_with(fresh)
        } else {
            held.remove(&physical).unwrap_or_else(fresh)
        }
    });
    host::KeyMessage { scancode, vkey, extended, ..key }
}

/// Where a keystroke goes: the focused window if it still exists.
pub fn key_target() -> Option<u32> {
    let f = FOCUS.load(std::sync::atomic::Ordering::Relaxed);
    if f == 0 { return None; }
    RR.with_borrow(|rr| rr.windows.contains_key(&f)).then_some(f)
}
