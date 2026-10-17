use std::sync::{Arc, Condvar, Mutex};

use runtime::Context;

use crate::{HANDLE, Ptr, kernel32::lock};

pub enum Object {
    Thread,
    Event(Arc<Event>),
    Mutex,
    File(host::fs::File),
    FindHandle(crate::kernel32::FindHandle),
}

pub struct Event {
    _name: String,
    manual_reset: bool,
    signaled: Mutex<bool>,
    cond: Condvar,
}

#[win32_derive::dllexport]
pub fn WaitForSingleObject(_ctx: &mut Context, hHandle: HANDLE, dwMilliseconds: u32) -> u32 /* WAIT_EVENT */
{
    // Road Rash evaluation: thread handles (CreateThread returns the small
    // thread id) are signalled once the thread has exited.
    let raw = hHandle.to_raw();
    if raw > 1 && raw < 0x100 {
        let start = host::host().time();
        loop {
            if rr_threads::finished(raw) {
                return 0; // WAIT_OBJECT_0
            }
            if dwMilliseconds != u32::MAX && host::host().time().wrapping_sub(start) >= dwMilliseconds {
                return 0x102; // WAIT_TIMEOUT
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
    let event = {
        let kernel32 = lock();
        let Object::Event(event) = kernel32.objects.get(hHandle).unwrap() else {
            panic!()
        };
        event.clone()
    };

    let mut signaled = event.signaled.lock().unwrap();
    // Road Rash evaluation: honour finite timeouts (it used to wait forever).
    let start = host::host().time();
    while !*signaled {
        if dwMilliseconds != u32::MAX && host::host().time().wrapping_sub(start) >= dwMilliseconds {
            return 0x102; // WAIT_TIMEOUT
        }
        signaled = event
            .cond
            .wait_timeout(signaled, std::time::Duration::from_millis((dwMilliseconds as u64).min(50)))
            .unwrap()
            .0;
    }
    if !event.manual_reset {
        *signaled = false;
    }
    0
}

#[win32_derive::dllexport]
pub fn CreateEventA(
    ctx: &mut Context,
    _lpEventAttributes: Ptr<()>,
    bManualReset: bool,
    bInitialState: bool,
    lpName: Ptr<u8>,
) -> HANDLE {
    let name = ctx.memory.read_str(lpName.addr);
    let event = Event {
        _name: name.to_string(),
        manual_reset: bManualReset,
        signaled: Mutex::new(bInitialState),
        cond: Condvar::new(),
    };
    let mut kernel32 = lock();
    kernel32.objects.add(Object::Event(Arc::new(event)))
}

#[win32_derive::dllexport]
pub fn CreateMutexA(
    _ctx: &mut Context,
    _lpMutexAttributes: Ptr<()>,
    _bInitialOwner: bool,
    _lpName: Ptr<u8>,
) -> HANDLE {
    // Single x86 thread; no contention possible.
    lock().objects.add(Object::Mutex)
}

#[win32_derive::dllexport]
pub fn ReleaseMutex(_ctx: &mut Context, _hMutex: HANDLE) -> bool {
    true
}

#[win32_derive::dllexport]
pub fn WaitForMultipleObjects(
    _ctx: &mut Context,
    _nCount: u32,
    _lpHandles: Ptr<u32>,
    _bWaitAll: bool,
    _dwMilliseconds: u32,
) -> u32 /* WAIT_EVENT */ {
    crate::stub!(0) // WAIT_OBJECT_0
}

#[win32_derive::dllexport]
pub fn SetEvent(_ctx: &mut Context, hEvent: HANDLE) -> bool {
    let kernel32 = lock();
    let Object::Event(event) = kernel32.objects.get(hEvent).unwrap() else {
        panic!()
    };
    *event.signaled.lock().unwrap() = true;
    // TODO: the number of threads notified are different between manual reset and auto reset events!
    assert!(!event.manual_reset);
    event.cond.notify_one();
    true
}

/// Road Rash evaluation: which CreateThread threads have exited.
pub mod rr_threads {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static DONE: Mutex<Option<HashSet<u32>>> = Mutex::new(None);
    pub fn mark_finished(id: u32) {
        DONE.lock().unwrap().get_or_insert_with(Default::default).insert(id);
    }
    pub fn finished(id: u32) -> bool {
        DONE.lock().unwrap().as_ref().is_some_and(|d| d.contains(&id))
    }
}
