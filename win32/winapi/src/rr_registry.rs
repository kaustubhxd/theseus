//! Road Rash evaluation (not upstream): a small persistent registry and the
//! common Open/Save file dialogs.
//!
//! Upstream's registry calls are stubs (keys open without a handle, every
//! value is "not found"). Road Rash keeps its settings under
//! HKLM\SOFTWARE\Electronic Arts\RoadRash 95. Here keys and values live in
//! memory and are written to `C:\Saves\registry.txt` (the game folder
//! natively; IndexedDB on the web, via the host filesystem).
//!
//! The file dialogs pick a file without any UI: Save uses the name the game
//! proposes (or ROADRASH.RRS) in `C:\Saves`; Open picks the most recently
//! written matching file there.

use std::collections::BTreeMap;
use std::sync::Mutex;

use runtime::Context;

const STORE: &str = "C:/Saves/registry.txt";
const SAVES_DIR: &str = "C:/Saves";
const HANDLE_BASE: u32 = 0x7e00_0000;

const ERROR_SUCCESS: u32 = 0;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_MORE_DATA: u32 = 234;
const ERROR_NO_MORE_ITEMS: u32 = 259;
const ERROR_INVALID_HANDLE: u32 = 6;

#[derive(Default)]
struct Registry {
    loaded: bool,
    /// lowercase key path -> (lowercase value name -> (name, type, data))
    keys: BTreeMap<String, BTreeMap<String, (String, u32, Vec<u8>)>>,
    /// open handles -> key path
    handles: Vec<String>,
}

static REG: Mutex<Option<Registry>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    let mut g = REG.lock().unwrap();
    let r = g.get_or_insert_with(Default::default);
    if !r.loaded {
        r.loaded = true;
        load(r);
    }
    f(r)
}

fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2).filter_map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()).collect()
}

fn load(r: &mut Registry) {
    let path = crate::kernel32::resolve_path(STORE);
    let Ok(bytes) = host::fs::read(&path) else { return };
    let text = String::from_utf8_lossy(&bytes);
    let mut key = String::new();
    for line in text.lines() {
        if let Some(k) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            key = k.to_string();
            r.keys.entry(key.clone()).or_default();
        } else if let [name, ty, data] = line.splitn(3, '\t').collect::<Vec<_>>()[..] {
            let ty = ty.parse().unwrap_or(3);
            r.keys.entry(key.clone()).or_default().insert(name.to_ascii_lowercase(), (name.to_string(), ty, unhex(data)));
        }
    }
    log::warn!("rr-reg: loaded {} keys from {path:?}", r.keys.len());
}

fn save(r: &Registry) {
    let mut text = String::new();
    for (key, values) in &r.keys {
        text += &format!("[{key}]\n");
        for (name, ty, data) in values.values() {
            text += &format!("{name}\t{ty}\t{}\n", hex(data));
        }
    }
    write_file(STORE, text.as_bytes());
}

/// Write a whole file through the host filesystem (persisted on the web).
fn write_file(win_path: &str, data: &[u8]) {
    use std::io::Write;
    let path = crate::kernel32::resolve_path(win_path);
    #[cfg(not(target_family = "wasm"))]
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut opts = host::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    match opts.open(&path) {
        Ok(mut f) => {
            let _ = f.write_all(data);
        }
        Err(e) => log::warn!("rr-reg: can't write {path:?}: {e}"),
    }
}

fn base_name(hkey: u32, r: &Registry) -> Option<String> {
    Some(match hkey {
        0x8000_0000 => "hkcr".into(),
        0x8000_0001 => "hkcu".into(),
        0x8000_0002 => "hklm".into(),
        0x8000_0003 => "hku".into(),
        h if h >= HANDLE_BASE => r.handles.get((h - HANDLE_BASE) as usize)?.clone(),
        _ => return None,
    })
}

fn join(base: &str, sub: &str) -> String {
    let sub = sub.trim_matches('\\').to_ascii_lowercase();
    if sub.is_empty() { base.to_string() } else { format!("{base}\\{sub}") }
}

fn read_opt_str(ctx: &Context, ptr: u32) -> String {
    if ptr == 0 { String::new() } else { ctx.memory.read_str(ptr) }
}

fn new_handle(r: &mut Registry, path: String) -> u32 {
    r.handles.push(path);
    HANDLE_BASE + r.handles.len() as u32 - 1
}

/// RegOpenKeyEx always succeeds (creating the key in memory): on a real
/// install the setup program would have created it, and the game expects it.
pub fn open_key(ctx: &mut Context, hkey: u32, sub: u32, phk: u32) -> u32 {
    let sub = read_opt_str(ctx, sub);
    with(|r| {
        let Some(base) = base_name(hkey, r) else { return ERROR_INVALID_HANDLE };
        let path = join(&base, &sub);
        r.keys.entry(path.clone()).or_default();
        let h = new_handle(r, path);
        ctx.memory.write::<u32>(phk, h);
        ERROR_SUCCESS
    })
}

pub fn create_key(ctx: &mut Context, hkey: u32, sub: u32, phk: u32, disposition: u32) -> u32 {
    let sub = read_opt_str(ctx, sub);
    let (h, created) = with(|r| {
        let Some(base) = base_name(hkey, r) else { return (0, false) };
        let path = join(&base, &sub);
        let created = !r.keys.contains_key(&path);
        r.keys.entry(path.clone()).or_default();
        if created {
            save(r);
        }
        (new_handle(r, path), created)
    });
    if h == 0 {
        return ERROR_INVALID_HANDLE;
    }
    ctx.memory.write::<u32>(phk, h);
    if disposition != 0 {
        ctx.memory.write::<u32>(disposition, if created { 1 } else { 2 });
    }
    ERROR_SUCCESS
}

pub fn query_value(ctx: &mut Context, hkey: u32, name: u32, ptype: u32, pdata: u32, pcb: u32) -> u32 {
    let name = read_opt_str(ctx, name).to_ascii_lowercase();
    let found = with(|r| {
        let path = base_name(hkey, r)?;
        r.keys.get(&path)?.get(&name).map(|(_, t, d)| (*t, d.clone()))
    });
    let Some((ty, data)) = found else { return ERROR_FILE_NOT_FOUND };
    if ptype != 0 {
        ctx.memory.write::<u32>(ptype, ty);
    }
    if pcb != 0 {
        let cap = ctx.memory.read::<u32>(pcb);
        ctx.memory.write::<u32>(pcb, data.len() as u32);
        if pdata != 0 {
            if (cap as usize) < data.len() {
                return ERROR_MORE_DATA;
            }
            ctx.memory[pdata..][..data.len()].copy_from_slice(&data);
        }
    }
    ERROR_SUCCESS
}

pub fn set_value(ctx: &mut Context, hkey: u32, name: u32, ty: u32, pdata: u32, cb: u32) -> u32 {
    let name = read_opt_str(ctx, name);
    let data = ctx.memory[pdata..][..cb as usize].to_vec();
    with(|r| {
        let Some(path) = base_name(hkey, r) else { return ERROR_INVALID_HANDLE };
        r.keys.entry(path).or_default().insert(name.to_ascii_lowercase(), (name, ty, data));
        save(r);
        ERROR_SUCCESS
    })
}

pub fn delete_key(ctx: &mut Context, hkey: u32, sub: u32) -> u32 {
    let sub = read_opt_str(ctx, sub);
    with(|r| {
        let Some(base) = base_name(hkey, r) else { return ERROR_INVALID_HANDLE };
        let path = join(&base, &sub);
        if r.keys.remove(&path).is_none() {
            return ERROR_FILE_NOT_FOUND;
        }
        save(r);
        ERROR_SUCCESS
    })
}

/// RegEnumValueA(hKey, index, name, pcchName, reserved, type, data, pcbData)
pub fn enum_value(ctx: &mut Context, hkey: u32, index: u32, pname: u32, pcch: u32, ptype: u32, pdata: u32, pcb: u32) -> u32 {
    let item = with(|r| {
        let path = base_name(hkey, r)?;
        r.keys.get(&path)?.values().nth(index as usize).cloned()
    });
    let Some((name, ty, data)) = item else { return ERROR_NO_MORE_ITEMS };
    if pname != 0 && pcch != 0 {
        let cap = ctx.memory.read::<u32>(pcch) as usize;
        if cap <= name.len() {
            return ERROR_MORE_DATA;
        }
        ctx.memory[pname..][..name.len()].copy_from_slice(name.as_bytes());
        ctx.memory.write::<u8>(pname + name.len() as u32, 0);
        ctx.memory.write::<u32>(pcch, name.len() as u32);
    }
    if ptype != 0 {
        ctx.memory.write::<u32>(ptype, ty);
    }
    if pcb != 0 {
        let cap = ctx.memory.read::<u32>(pcb) as usize;
        ctx.memory.write::<u32>(pcb, data.len() as u32);
        if pdata != 0 && cap >= data.len() {
            ctx.memory[pdata..][..data.len()].copy_from_slice(&data);
        }
    }
    ERROR_SUCCESS
}

/// RegEnumKeyExA(hKey, index, name, pcchName, ...)
pub fn enum_key(ctx: &mut Context, hkey: u32, index: u32, pname: u32, pcch: u32) -> u32 {
    let item = with(|r| {
        let path = base_name(hkey, r)?;
        let prefix = format!("{path}\\");
        let mut subs: Vec<String> = r
            .keys
            .keys()
            .filter_map(|k| k.strip_prefix(&prefix).map(|rest| rest.split('\\').next().unwrap().to_string()))
            .collect();
        subs.dedup();
        subs.into_iter().nth(index as usize)
    });
    let Some(name) = item else { return ERROR_NO_MORE_ITEMS };
    let cap = ctx.memory.read::<u32>(pcch) as usize;
    if cap <= name.len() {
        return ERROR_MORE_DATA;
    }
    ctx.memory[pname..][..name.len()].copy_from_slice(name.as_bytes());
    ctx.memory.write::<u8>(pname + name.len() as u32, 0);
    ctx.memory.write::<u32>(pcch, name.len() as u32);
    ERROR_SUCCESS
}

// ---- GetOpenFileNameA / GetSaveFileNameA --------------------------------

/// OPENFILENAMEA field offsets.
const OFN_FILE: u32 = 28;
const OFN_MAX_FILE: u32 = 32;
const OFN_FILE_TITLE: u32 = 36;
const OFN_MAX_FILE_TITLE: u32 = 40;
const OFN_FILE_OFFSET: u32 = 56;
const OFN_FILE_EXTENSION: u32 = 58;

fn write_choice(ctx: &mut Context, ofn: u32, file_name: &str) -> u32 {
    let full = format!("C:\\Saves\\{file_name}");
    let buf = ctx.memory.read::<u32>(ofn + OFN_FILE);
    let max = ctx.memory.read::<u32>(ofn + OFN_MAX_FILE) as usize;
    if buf == 0 || full.len() >= max {
        return 0;
    }
    ctx.memory[buf..][..full.len()].copy_from_slice(full.as_bytes());
    ctx.memory.write::<u8>(buf + full.len() as u32, 0);
    let title = ctx.memory.read::<u32>(ofn + OFN_FILE_TITLE);
    let max_title = ctx.memory.read::<u32>(ofn + OFN_MAX_FILE_TITLE) as usize;
    if title != 0 && file_name.len() < max_title {
        ctx.memory[title..][..file_name.len()].copy_from_slice(file_name.as_bytes());
        ctx.memory.write::<u8>(title + file_name.len() as u32, 0);
    }
    let offset = (full.len() - file_name.len()) as u16;
    ctx.memory.write::<u16>(ofn + OFN_FILE_OFFSET, offset);
    let ext = file_name.rfind('.').map(|i| offset + i as u16 + 1).unwrap_or(0);
    ctx.memory.write::<u16>(ofn + OFN_FILE_EXTENSION, ext);
    1
}

fn saves_dir_entries() -> Vec<String> {
    let dir = crate::kernel32::resolve_path(SAVES_DIR);
    let mut names: Vec<(String, std::time::SystemTime)> = Vec::new();
    #[cfg(not(target_family = "wasm"))]
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let t = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            names.push((name, t));
        }
    }
    #[cfg(target_family = "wasm")]
    if let Ok(rd) = host::fs::read_dir(&dir) {
        // No timestamps in the in-memory FS: fall back to name order.
        for e in rd {
            names.push((e.name, std::time::UNIX_EPOCH));
        }
    }
    names.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
    names.into_iter().map(|n| n.0).filter(|n| n.to_ascii_lowercase().ends_with(".rrs")).collect()
}

/// GetSaveFileNameA: save into C:\Saves under the proposed name.
pub fn get_save_file_name(ctx: &mut Context, ofn: u32) -> u32 {
    let buf = ctx.memory.read::<u32>(ofn + OFN_FILE);
    let proposed = if buf != 0 { ctx.memory.read_str(buf) } else { String::new() };
    let name = proposed.rsplit(['\\', '/', ':']).next().unwrap_or("").to_string();
    let name = if name.is_empty() { "ROADRASH.RRS".to_string() } else if name.contains('.') { name } else { format!("{name}.RRS") };
    // The page's picker, when there is one (web).
    let name = match host::choose_file(true, &saves_dir_entries(), &name) {
        Ok(Some(chosen)) => {
            let chosen: String = chosen.chars().filter(|c| c.is_ascii_alphanumeric() || " -_.".contains(*c)).collect();
            if chosen.is_empty() { return 0 }
            if chosen.to_ascii_uppercase().ends_with(".RRS") { chosen } else { format!("{chosen}.RRS") }
        }
        Ok(None) => return 0, // cancelled
        Err(()) => name,
    };
    #[cfg(not(target_family = "wasm"))]
    let _ = std::fs::create_dir_all(crate::kernel32::resolve_path(SAVES_DIR));
    log::warn!("rr-save: GetSaveFileName -> C:\\Saves\\{name}");
    write_choice(ctx, ofn, &name)
}

/// GetOpenFileNameA: the most recent save in C:\Saves, or cancel if none.
pub fn get_open_file_name(ctx: &mut Context, ofn: u32) -> u32 {
    let saves = saves_dir_entries();
    if let Ok(choice) = host::choose_file(false, &saves, saves.first().map(String::as_str).unwrap_or("")) {
        // The page's picker (web).
        return match choice {
            Some(name) if saves.contains(&name) => {
                log::warn!("rr-save: GetOpenFileName (picked) -> C:\\Saves\\{name}");
                write_choice(ctx, ofn, &name)
            }
            _ => 0,
        };
    }
    let Some(name) = saves.into_iter().next() else {
        log::warn!("rr-save: GetOpenFileName: no saves in C:\\Saves");
        return 0;
    };
    log::warn!("rr-save: GetOpenFileName -> C:\\Saves\\{name}");
    write_choice(ctx, ofn, &name)
}
