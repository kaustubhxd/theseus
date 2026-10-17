//! Road Rash evaluation shim (not upstream): load resource-only language DLLs
//! (e.g. ENU.DLL) on the host side so LoadStringA can read their string tables.
//! The DLL's code is never run; only its .rsrc data is used.

use std::sync::Mutex;

use runtime::Context;

/// Fake module handle base for host-loaded resource DLLs.
pub const RESDLL_BASE: u32 = 0x0e5d_0000;

struct ResDll {
    /// The DLL image laid out at its virtual addresses (RVA-indexed).
    image: Vec<u8>,
    rsrc: std::ops::Range<u32>,
}

static DLLS: Mutex<Vec<ResDll>> = Mutex::new(Vec::new());

pub fn load(filename: &str) -> u32 {
    let path = crate::kernel32::resolve_path(filename);
    let buf = match host::fs::read(&path) { Ok(b) => b, Err(e) => { log::warn!("rr: read {path:?}: {e}"); return 0 } };
    let pe = match exe::parse(&buf) { Ok(exe::Parse::PE(p)) => p, _ => { log::warn!("rr: parse {path:?} failed"); return 0 } };
    let Some(dir) = pe.get_data_directory(exe::pe::IMAGE_DIRECTORY_ENTRY::RESOURCE) else { return 0 };
    let rsrc = dir.VirtualAddress..dir.VirtualAddress + dir.Size;
    let mut image = vec![0u8; pe.opt_header.SizeOfImage as usize];
    for s in pe.sections.iter() {
        let n = s.SizeOfRawData.min(s.VirtualSize.max(s.SizeOfRawData)) as usize;
        let src = &buf[s.PointerToRawData as usize..][..n.min(buf.len() - s.PointerToRawData as usize)];
        image[s.VirtualAddress as usize..][..src.len()].copy_from_slice(src);
    }
    let mut dlls = DLLS.lock().unwrap();
    dlls.push(ResDll { image, rsrc });
    let h = RESDLL_BASE + ((dlls.len() as u32 - 1) << 12);
    log::warn!("rr: loaded resource DLL {path:?} as {h:#x} (code not run)");
    h
}

/// Find string-table block `block` (RT_STRING = 6) for module `hinst`.
fn string_block(ctx: &Context, hinst: u32, block: u32) -> Option<Vec<u8>> {
    use exe::ResourceName::Id;
    if hinst >= RESDLL_BASE {
        let dlls = DLLS.lock().unwrap();
        let d = dlls.get(((hinst - RESDLL_BASE) >> 12) as usize)?;
        let section = &d.image[d.rsrc.start as usize..d.rsrc.end as usize];
        let span = exe::find_resource(section, Id(6), Id(block))?;
        Some(d.image[span.start as usize..span.end as usize].to_vec())
    } else {
        crate::kernel32::lock().find_resource(ctx, Id(6), Id(block)).map(|s| s.to_vec())
    }
}

pub fn load_string(ctx: &mut Context, hinst: u32, id: u32, buf: u32, max: u32) -> u32 {
    let Some(data) = string_block(ctx, hinst, id / 16 + 1) else { return 0 };
    let mut ofs = 0usize;
    for i in 0..16 {
        let len = u16::from_le_bytes([data[ofs], data[ofs + 1]]) as usize;
        ofs += 2;
        if i == id % 16 {
            if max == 0 { return 0; }
            let n = len.min(max as usize - 1);
            for j in 0..n {
                let c = u16::from_le_bytes([data[ofs + 2 * j], data[ofs + 2 * j + 1]]);
                ctx.memory.write::<u8>(buf + j as u32, crate::rr_text::to_cp1252(char::from_u32(c as u32).unwrap_or('?')));
            }
            ctx.memory.write::<u8>(buf + n as u32, 0);
            return n as u32;
        }
        ofs += 2 * len;
    }
    0
}

/// Road Rash evaluation: storage for Get/SetWindowWord, keyed by (hwnd, index).
pub static WINDOW_WORDS: Mutex<std::collections::BTreeMap<(u32, u32), u16>> =
    Mutex::new(std::collections::BTreeMap::new());

/// Road Rash evaluation: re-entrant critical sections keyed by the
/// CRITICAL_SECTION address (upstream had none; the game has 3+ threads).
pub mod crit {
    use std::collections::HashMap;
    use std::sync::{Condvar, LazyLock, Mutex};
    static CS: LazyLock<(Mutex<HashMap<u32, (u32, u32)>>, Condvar)> = LazyLock::new(Default::default);
    pub fn enter(addr: u32, tid: u32) {
        let (m, c) = &*CS;
        let mut map = m.lock().unwrap();
        loop {
            let e = map.entry(addr).or_insert((0, 0));
            if e.0 == 0 || e.0 == tid {
                e.0 = tid;
                e.1 += 1;
                return;
            }
            map = c.wait(map).unwrap();
        }
    }
    pub fn leave(addr: u32, tid: u32) {
        let (m, c) = &*CS;
        let mut map = m.lock().unwrap();
        if let Some(e) = map.get_mut(&addr) {
            if e.0 == tid && e.1 > 0 {
                e.1 -= 1;
                if e.1 == 0 {
                    e.0 = 0;
                    c.notify_all();
                }
            }
        }
    }
}

/// Road Rash evaluation: microsecond clock for QueryPerformanceCounter.
pub fn micros() -> u64 {
    #[cfg(not(target_family = "wasm"))]
    {
        static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        T0.get_or_init(std::time::Instant::now).elapsed().as_micros() as u64
    }
    #[cfg(target_family = "wasm")]
    {
        host::host().time() as u64 * 1000
    }
}
