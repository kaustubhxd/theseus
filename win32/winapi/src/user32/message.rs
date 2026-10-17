use std::{cell::RefCell, collections::VecDeque, rc::Rc, sync::LazyLock};

use runtime::Context;

use crate::{
    POINT, Ptr,
    dllexport::win32flags,
    stub, trace,
    user32::{HACCEL, HWND, Window, char_for_key, message_vkey, state},
};

/// If THESEUS_TRACE includes "wm", log all Windows messages.
static LOG_MESSAGES: LazyLock<bool> =
    LazyLock::new(|| !matches!(trace::get_uncached("wm"), trace::Trace::None));

pub type WPARAM = u32;
pub type LPARAM = u32;

#[derive(win32_derive::ABIEnum, Debug)]
pub enum WM {
    ACTIVATE = 0x6,
    SETFOCUS = 0x7,
    PAINT = 0xf,
    QUIT = 0x12,
    SHOWWINDOW = 0x18,
    ACTIVATEAPP = 0x1c,
    KEYDOWN = 0x100,
    KEYUP = 0x101,
    CHAR = 0x102,
    SYSKEYDOWN = 0x104,
    SYSKEYUP = 0x105,
    MOUSEMOVE = 0x200,
    LBUTTONDOWN = 0x201,
    LBUTTONUP = 0x202,
    RBUTTONDOWN = 0x204,
    RBUTTONUP = 0x205,
    MBUTTONDOWN = 0x207,
    MBUTTONUP = 0x208,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, zerocopy::FromBytes, zerocopy::IntoBytes, zerocopy::Immutable)]
pub struct MSG {
    hwnd: HWND,
    message: u32,
    wParam: WPARAM,
    lParam: LPARAM,
    time: u32,
    pt: POINT,
}

#[derive(Default)]
pub struct MessageQueue {
    pub window: Option<Rc<RefCell<Window>>>,
    messages: VecDeque<MSG>,
    quit: Option<MSG>,
}

win32flags! {
    pub struct MK {
        const LBUTTON = 0x0001;
        const RBUTTON = 0x0002;
        const SHIFT   = 0x0004;
        const CONTROL = 0x0008;
        const MBUTTON = 0x0010;
    }
}

fn mouse_button_to_wm(is_down: bool, message: &host::MouseMessage) -> WM {
    // Can't use a match here because MouseButton is a bitfield, not an enum.
    if message.button == host::MouseButton::Left {
        if is_down {
            return WM::LBUTTONDOWN;
        } else {
            return WM::LBUTTONUP;
        }
    } else if message.button == host::MouseButton::Right {
        if is_down {
            return WM::RBUTTONDOWN;
        } else {
            return WM::RBUTTONUP;
        }
    } else if message.button == host::MouseButton::Middle {
        if is_down {
            return WM::MBUTTONDOWN;
        } else {
            return WM::MBUTTONUP;
        }
    } else {
        return WM::MOUSEMOVE;
    }
}

fn mouse_msg(wm: WM, hwnd: HWND, message: &host::MouseMessage) -> MSG {
    let mut wParam = MK::empty();
    if message.buttons.contains(host::MouseButton::Left) {
        wParam |= MK::LBUTTON;
    }
    if message.buttons.contains(host::MouseButton::Middle) {
        wParam |= MK::MBUTTON;
    }
    if message.buttons.contains(host::MouseButton::Right) {
        wParam |= MK::RBUTTON;
    }

    MSG {
        hwnd,
        message: wm as u32,
        wParam: wParam.bits(),
        lParam: (message.y as u16 as u32) << 16 | message.x as u16 as u32,
        time: 0, // todo
        // TODO: screen coordinates
        pt: POINT {
            x: message.x as i32,
            y: message.y as i32,
        },
    }
}

fn key_msg(hwnd: HWND, key: &host::KeyMessage, down: bool) -> MSG {
    // lParam packs the key's physical details, as documented for WM_KEYDOWN.
    let mut lParam = 1; // repeat count; the host reports repeats one at a time
    lParam |= (key.scancode as u32) << 16;
    if key.extended {
        lParam |= 1 << 24;
    }
    // Bit 29 is set while alt is held, bit 30 holds the previous key state,
    // bit 31 marks the release.
    let alt = state().input.borrow().key_down(0x12); // VK_MENU
    if alt {
        lParam |= 1 << 29;
    }
    if key.repeat || !down {
        lParam |= 1 << 30;
    }
    if !down {
        lParam |= 1 << 31;
    }

    // Keys pressed with alt held are "system" keys, as is alt itself.
    let system = alt || key.vkey == 0xa4 || key.vkey == 0xa5;
    let message = match (down, system) {
        (true, false) => WM::KEYDOWN,
        (false, false) => WM::KEYUP,
        (true, true) => WM::SYSKEYDOWN,
        (false, true) => WM::SYSKEYUP,
    };

    MSG {
        hwnd,
        message: message as u32,
        wParam: message_vkey(key.vkey) as u32,
        lParam,
        time: host::host().time(),
        pt: POINT::default(),
    }
}

/// Post a message to the application's queue (e.g. synthetic activation
/// messages from ShowWindow).
pub fn post_message(hwnd: HWND, message: u32, wParam: WPARAM, lParam: LPARAM) {
    let mut queue = state().message_queue.borrow_mut();
    queue.messages.push_back(MSG {
        hwnd,
        message,
        wParam,
        lParam,
        time: 0,
        pt: POINT::default(),
    });
}

#[win32_derive::dllexport]
pub fn WaitMessage(_ctx: &mut Context) -> bool {
    let mut queue = state().message_queue.borrow_mut();
    if queue.peek().is_none() {
        queue.wait_host();
    }
    true
}

impl MessageQueue {
    /// Road Rash evaluation: scripted key presses for automated runs, from
    /// RR_KEYS="ms:key,ms:key" (ms since the first message poll; key = enter,
    /// esc, space, up, down, left, right, or a hex VK like 0x41).
    fn rr_inject(&mut self) {
        // Road Rash evaluation: cut-scene playback and its MM_MCINOTIFY messages.
        crate::winmm::rr_video::pump();
        for (hwnd, msg, w, l) in crate::winmm::rr_video::take_notifications() {
            self.messages.push_back(MSG { hwnd: HWND::from_raw(hwnd), message: msg, wParam: w, lParam: l, time: 0, pt: POINT::default() });
        }
        for (down, key) in super::rr_windows::due_keys() {
            self.enqueue_message(if down { host::Message::KeyDown(key) } else { host::Message::KeyUp(key) });
        }
        for e in super::rr_windows::due_extra() {
            match e {
                super::rr_windows::Extra::Post(hwnd, msg, w, l) => {
                    log::warn!("rr-key: post hwnd={hwnd:#x} msg={msg:#x} w={w:#x} l={l:#x}");
                    self.messages.push_back(MSG { hwnd: HWND::from_raw(hwnd), message: msg, wParam: w, lParam: l, time: 0, pt: POINT::default() });
                }
                super::rr_windows::Extra::Click(x, y, down) => {
                    log::warn!("rr-key: click {} at {x},{y}", if down { "down" } else { "up" });
                    let b = host::MouseButton::Left;
                    let m = host::MouseMessage { x, y, button: b, buttons: if down { b } else { host::MouseButton::empty() } };
                    self.enqueue_message(host::Message::MouseMove(host::MouseMessage { x, y, button: b, buttons: host::MouseButton::empty() }));
                    self.enqueue_message(if down { host::Message::MouseDown(m) } else { host::Message::MouseUp(m) });
                }
            }
        }
    }

    fn paint_msg(&self) -> Option<MSG> {
        let window = self.window.as_ref()?.borrow();
        if !window.dirty {
            return None;
        }

        Some(MSG {
            hwnd: window.hwnd,
            message: WM::PAINT as u32,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: POINT::default(),
        })
    }

    fn peek(&mut self) -> Option<MSG> {
        self.rr_inject();
        if let Some(msg) = self.messages.front() {
            Some(*msg)
        } else if self.quit.is_some() {
            self.quit
        } else if let Some(msg) = self.paint_msg() {
            Some(msg)
        } else {
            rr_timer_due(false)
        }
    }

    fn pop(&mut self) -> Option<MSG> {
        self.rr_inject();
        if let Some(msg) = self.messages.pop_front() {
            Some(msg)
        } else if self.quit.is_some() {
            self.quit.take()
        } else if let Some(msg) = self.paint_msg() {
            Some(msg)
        } else {
            rr_timer_due(true)
        }
    }

    /// Pop one message, waiting for a new one if necessary.
    fn read(&mut self) -> MSG {
        loop {
            if let Some(msg) = self.pop() {
                return msg;
            }
            if RR_TIMERS.lock().unwrap().is_empty() && !super::rr_windows::script_pending() && !crate::winmm::rr_video::active() {
                self.wait_host();
            } else {
                // Road Rash evaluation: timers pending, so poll instead of blocking.
                self.poll_host();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }

    /// Read one pending host message, if any available.
    fn poll_host(&mut self) {
        let Some(message) = host::host().poll() else {
            return;
        };
        self.enqueue_message(message);
    }

    /// Read every pending host message. DirectInput calls this to refresh
    /// input state without going through the window message queue.
    pub fn poll_host_all(&mut self) {
        while let Some(message) = host::host().poll() {
            self.enqueue_message(message);
        }
    }

    /// Wait for a new message to arrive.
    fn wait_host(&mut self) {
        let message = host::host().wait();
        self.enqueue_message(message);
    }

    fn enqueue_message(&mut self, msg: host::Message) {
        #[cfg(not(target_family = "wasm"))]
        if matches!(msg, host::Message::Paint) {
            if let Some(window) = &self.window {
                window.borrow_mut().dirty = true;
            }
            return;
        }

        // Every host input event updates the shared input state, whether or not
        // the app reads it through the message queue: DirectInput reads the
        // same state, and this is the only place host events are consumed.
        // Road Rash evaluation: the WASD layout, unless the game is taking text.
        let msg = match msg {
            host::Message::KeyDown(key) => host::Message::KeyDown(super::rr_windows::map_key(key, true)),
            host::Message::KeyUp(key) => host::Message::KeyUp(super::rr_windows::map_key(key, false)),
            msg => msg,
        };
        {
            let mut input = state().input.borrow_mut();
            match &msg {
                host::Message::KeyDown(key) => input.on_key(key, true),
                host::Message::KeyUp(key) => input.on_key(key, false),
                host::Message::MouseDown(mouse)
                | host::Message::MouseUp(mouse)
                | host::Message::MouseMove(mouse) => input.on_mouse(mouse),
                _ => {}
            }
        }

        let mut msg = self.msg_from_message(msg);
        // Road Rash evaluation: mouse messages go to the child window under the cursor.
        // Road Rash evaluation: keystrokes go to the window with keyboard focus.
        if (0x100..=0x109).contains(&msg.message) {
            if let Some(h) = super::rr_windows::key_target() {
                msg.hwnd = HWND::from_raw(h);
            }
        }
        if (0x200..=0x20a).contains(&msg.message) {
            let (x, y) = ((msg.lParam & 0xffff) as i16 as i32, (msg.lParam >> 16) as i16 as i32);
            if let Some((h, cx, cy)) = super::rr_windows::hit_test(x, y) {
                msg.hwnd = HWND::from_raw(h);
                msg.lParam = ((cy as u32 & 0xffff) << 16) | (cx as u32 & 0xffff);
            }
        }
        if *LOG_MESSAGES {
            log::info!("{:#x?}", msg);
        }

        // PAINT/TIMER/QUIT are in special queues.
        if msg.message == WM::QUIT as u32 {
            self.quit = Some(msg);
        } else {
            self.messages.push_back(msg);
        }
    }

    fn msg_from_message(&self, message: host::Message) -> MSG {
        use host::Message::*;
        let hwnd = self.window.as_ref().unwrap().borrow().hwnd;
        match message {
            MouseDown(mouse) => mouse_msg(mouse_button_to_wm(true, &mouse), hwnd, &mouse),
            MouseUp(mouse) => mouse_msg(mouse_button_to_wm(false, &mouse), hwnd, &mouse),
            MouseMove(mouse) => mouse_msg(WM::MOUSEMOVE, hwnd, &mouse),
            KeyDown(key) => key_msg(hwnd, &key, true),
            KeyUp(key) => key_msg(hwnd, &key, false),
            #[cfg(not(target_family = "wasm"))]
            Paint => unreachable!(),
            #[cfg(not(target_family = "wasm"))]
            Quit => {
                MSG {
                    hwnd,
                    message: WM::QUIT as u32,
                    wParam: 0, // todo
                    lParam: 0, // todo
                    time: 0,   // todo
                    pt: POINT::default(),
                }
            }
        }
    }
}

#[win32_derive::dllexport]
pub fn DispatchMessageA(ctx: &mut Context, lpMsg: Ptr<MSG>) -> u32 {
    DispatchMessageW(ctx, lpMsg)
}

#[win32_derive::dllexport]
pub fn DispatchMessageW(ctx: &mut Context, lpMsg: Ptr<MSG>) -> u32 {
    let msg = lpMsg.read(&ctx.memory).unwrap();
    // Road Rash evaluation: route to the window's own class procedure.
    let wndproc = super::rr_windows::wndproc_for(msg.hwnd.to_raw())
        .unwrap_or_else(|| state().wndclass.borrow().as_ref().unwrap().wndproc.clone());
    // WNDPROC
    ctx.call32_x86(
        wndproc,
        vec![msg.hwnd.to_raw(), msg.message, msg.wParam, msg.lParam],
    );
    0
}

#[win32_derive::dllexport]
pub fn TranslateMessage(ctx: &mut Context, lpMsg: Ptr<MSG>) -> bool {
    let Some(msg) = lpMsg.read(&ctx.memory) else {
        return false;
    };
    if msg.message != WM::KEYDOWN as u32 {
        return false;
    }
    let Some(ch) = char_for_key(msg.wParam as u8) else {
        return false;
    };
    // The character message follows the key message in the queue, so the app
    // sees it on its next pump.
    post_message(msg.hwnd, WM::CHAR as u32, ch as u32, msg.lParam);
    true
}

#[win32_derive::dllexport]
pub fn PeekMessageA(
    ctx: &mut Context,
    lpMsg: Ptr<MSG>,
    hWnd: HWND,
    _wMsgFilterMin: u32,
    _wMsgFilterMax: u32,
    wRemoveMsg: u32, /* PEEK_MESSAGE_REMOVE_TYPE */
) -> bool {
    if ctx.thread_id != 1 {
        return super::rr_windows::worker_peek_message(ctx, lpMsg.addr, wRemoveMsg & 1 != 0);
    }
    let remove = match wRemoveMsg {
        0 => false,   // PM_NOREMOVE
        1 => true,    // PM_REMOVE
        _ => todo!(), // e.g. PM_NOYIELD
    };
    // Games poll for messages every frame; keep the audio mixer fed from here
    // too, in case the app renders without flipping.
    crate::dsound::pump(ctx);

    let mut queue = state().message_queue.borrow_mut();
    queue.poll_host();
    let Some(msg) = queue.peek() else {
        return false;
    };

    if hWnd.is_null() {
    } else if hWnd.is_invalid() {
        // TODO: only null hwnd messages
        assert!(msg.hwnd.is_null());
    } else {
        // TODO: only matching messages
        assert_eq!(msg.hwnd, hWnd);
    }
    lpMsg.write(&mut ctx.memory, msg).unwrap();
    if remove {
        queue.pop();
    }
    drop(queue);
    super::rr_windows::run_keyboard_hook(ctx, lpMsg.addr, if remove { 0 } else { 3 });
    true
}

#[win32_derive::dllexport]
pub fn PeekMessageW(
    ctx: &mut Context,
    lpMsg: Ptr<MSG>,
    hWnd: HWND,
    wMsgFilterMin: u32,
    wMsgFilterMax: u32,
    wRemoveMsg: u32, /* PEEK_MESSAGE_REMOVE_TYPE */
) -> bool {
    PeekMessageA(ctx, lpMsg, hWnd, wMsgFilterMin, wMsgFilterMax, wRemoveMsg)
}

#[win32_derive::dllexport]
pub fn GetMessageA(
    ctx: &mut Context,
    lpMsg: Ptr<MSG>,
    hWnd: HWND,
    wMsgFilterMin: u32,
    wMsgFilterMax: u32,
) -> i32 {
    if ctx.thread_id != 1 {
        // Road Rash evaluation: worker threads get their own message queue
        // (fed by PostThreadMessage), not the host-driven main queue.
        return super::rr_windows::worker_get_message(ctx, lpMsg.addr);
    }
    let msg_addr = lpMsg.addr;
    let ret = GetMessageW(ctx, lpMsg, hWnd, wMsgFilterMin, wMsgFilterMax);
    super::rr_windows::run_keyboard_hook(ctx, msg_addr, 0);
    ret
}

#[win32_derive::dllexport]
pub fn GetMessageW(
    ctx: &mut Context,
    lpMsg: Ptr<MSG>,
    hWnd: HWND,
    _wMsgFilterMin: u32,
    _wMsgFilterMax: u32,
) -> i32 {
    let msg = state().message_queue.borrow_mut().read();
    if msg.message == WM::QUIT as u32 {
        // Road Rash evaluation: WM_QUIT must still be written out; programs read
        // the exit code from its wParam (Road Rash passes the chosen course).
        lpMsg.write(&mut ctx.memory, msg).unwrap();
        return 0;
    }

    if hWnd.is_null() {
    } else if hWnd.is_invalid() {
        // TODO: only null hwnd messages
        assert!(msg.hwnd.is_null());
    } else {
        // TODO: only matching messages
        assert_eq!(msg.hwnd, hWnd);
    }
    lpMsg.write(&mut ctx.memory, msg).unwrap();

    1 // no error, no WM_QUIT
}

#[win32_derive::dllexport]
pub fn TranslateAcceleratorW(
    _ctx: &mut Context,
    _hWnd: HWND,
    _hAccTable: HACCEL,
    _lpMsg: Ptr<MSG>,
) -> i32 {
    stub!(0) // no translation
}

#[win32_derive::dllexport]
pub fn PostQuitMessage(_ctx: &mut Context, nExitCode: i32) {
    let mut queue = state().message_queue.borrow_mut();
    queue.quit = Some(MSG {
        hwnd: HWND::null(),
        message: WM::QUIT as u32,
        wParam: nExitCode as u32,
        lParam: 0,
        time: 0,
        pt: POINT::default(),
    });
}

#[win32_derive::dllexport]
pub fn PostMessageW(
    _ctx: &mut Context,
    hWnd: HWND,
    Msg: u32,
    wParam: WPARAM,
    lParam: LPARAM,
) -> bool {
    post_message(hWnd, Msg, wParam, lParam);
    true
}

#[win32_derive::dllexport]
pub fn PostMessageA(
    _ctx: &mut Context,
    hWnd: HWND,
    Msg: u32,
    wParam: WPARAM,
    lParam: LPARAM,
) -> bool {
    post_message(hWnd, Msg, wParam, lParam);
    true
}

#[win32_derive::dllexport]
pub fn SendMessageW(
    _ctx: &mut Context,
    _hWnd: HWND,
    _Msg: u32,
    _wParam: WPARAM,
    _lParam: LPARAM,
) -> u32 {
    todo!()
}

/// Road Rash evaluation: minimal SetTimer support (WM_TIMER posted to the window).
pub struct RrTimer {
    pub hwnd: HWND,
    pub id: u32,
    pub period: u32,
    pub next: u32,
}
pub static RR_TIMERS: std::sync::Mutex<Vec<RrTimer>> = std::sync::Mutex::new(Vec::new());

fn rr_timer_due(consume: bool) -> Option<MSG> {
    let now = host::host().time();
    let mut timers = RR_TIMERS.lock().unwrap();
    let t = timers.iter_mut().find(|t| now >= t.next)?;
    if consume {
        t.next = now + t.period.max(1);
    }
    Some(MSG { hwnd: t.hwnd, message: 0x113 /* WM_TIMER */, wParam: t.id, lParam: 0, time: now, pt: POINT::default() })
}
