use runtime::Context;

use crate::kernel32;

pub struct Timer {
    pub period: u32,
    pub next: u32,
    pub callback: u32,
    pub user_data: u32,
}

#[win32_derive::dllexport]
pub fn timeGetTime(_ctx: &mut Context) -> u32 {
    host::host().time()
}

#[derive(Debug)]
pub struct TIME {
    periodic: bool,
    #[allow(unused)]
    event: (), // todo
}

impl crate::dllexport::FromABIParam for TIME {
    fn from_abi(val: u32) -> Self {
        // kind of a bitfield, kind of an enum
        let periodic = (val & 0xF) != 0;
        assert_eq!(periodic, true);
        let event = match val & 0xF0 {
            0x00 => (),      // FUNCTION
            0x10 => todo!(), // EVENT_SET
            0x20 => todo!(), // EVENT_PULSE
            _ => unimplemented!(),
        };
        TIME { periodic, event }
    }
}

/// Road Rash evaluation: any number of multimedia timers, each on its own
/// thread (upstream allowed exactly one, and no timeKillEvent).
static RR_MMTIMERS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
static RR_NEXT_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

#[win32_derive::dllexport]
pub fn timeSetEvent(
    ctx: &mut Context,
    uDelay: u32,
    _uResolution: u32,
    lpTimeProc: u32,
    dwUser: u32,
    fuEvent: TIME,
) -> u32 {
    let id = RR_NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    RR_MMTIMERS.lock().unwrap().push(id);
    let periodic = fuEvent.periodic;
    let period = uDelay.max(1);
    kernel32::lock().create_thread(ctx, format!("winmm timer {id}"), move |ctx| {
        let func = ctx.indirect32(lpTimeProc);
        let mut next = host::host().time() + period;
        loop {
            let now = host::host().time();
            if now < next {
                std::thread::sleep(std::time::Duration::from_millis((next - now) as u64));
            }
            if !RR_MMTIMERS.lock().unwrap().contains(&id) {
                return;
            }
            next += period;
            // Don't try to catch up after a long stall.
            if next + period * 4 < host::host().time() {
                next = host::host().time() + period;
            }
            // LPTIMECALLBACK(uTimerID, uMsg, dwUser, dw1, dw2)
            ctx.call32_x86(func, vec![id, 0, dwUser, 0, 0]);
            if !periodic {
                RR_MMTIMERS.lock().unwrap().retain(|t| *t != id);
                return;
            }
        }
    });
    id
}

#[win32_derive::dllexport]
pub fn timeKillEvent(_ctx: &mut Context, _uTimerID: u32) -> u32 {
    let mut timers = RR_MMTIMERS.lock().unwrap();
    let before = timers.len();
    timers.retain(|t| *t != _uTimerID);
    if timers.len() < before { 0 } else { 97 /* TIMERR_NOCANDO */ }
}
