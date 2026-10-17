# Generates stub implementations for Road Rash imports missing from Theseus winapi.
# name: (nargs, return value or raw Rust body)
import os
W = os.path.expanduser('~/theseus-roadrash/theseus/win32/winapi/src')
S = {
 'kernel32': {
  'BuildCommDCBA':(2,0),'CreateFileMappingA':(6,'{ crate::kernel32::rr_mapping::create(a0) }'),'CreateProcessA':(10,0),'CreateSemaphoreA':(4,'{ crate::kernel32::CreateEventA(ctx, crate::Ptr::new(0), false, a1 > 0, crate::Ptr::new(0)).to_raw() }'),
  'DeleteCriticalSection':(1,0),'DeviceIoControl':(8,0),'EnterCriticalSection':(1,'{ crate::rr_resdll::crit::enter(a0, ctx.thread_id); 0 }'),'GetCommState':(2,0),
  'GetDriveTypeA':(1,'{ log::warn!("GetDriveTypeA({:?})", ctx.memory.read_str(a0)); 5 }'),'GetExitCodeThread':(2,0),'GetFullPathNameA':(4,0),'GetLocalTime':(1,0),
  'GetLocaleInfoA':(4,'{ if a1 == 3 && a3 >= 4 { for (i, b) in b"ENU\\0".iter().enumerate() { ctx.memory.write::<u8>(a2 + i as u32, *b); } 4 } else { 0 } }'),'GetOverlappedResult':(4,0),'GetThreadLocale':(0,'0x409'),'InitializeCriticalSection':(1,0),
  'IsBadCodePtr':(1,0),'IsBadReadPtr':(2,0),'IsBadWritePtr':(2,0),'LeaveCriticalSection':(1,'{ crate::rr_resdll::crit::leave(a0, ctx.thread_id); 0 }'),
  'LocalSize':(1,0),'MapViewOfFile':(5,'{ crate::kernel32::rr_mapping::map_view(ctx, a0, a3, a4) }'),'ReleaseSemaphore':(3,'{ if a2 != 0 { ctx.memory.write::<u32>(a2, 0); } crate::kernel32::SetEvent(ctx, crate::HANDLE::from_raw(a0)) as u32 }'),'SetCommMask':(2,0),'SetCommState':(2,0),
  'SetCommTimeouts':(2,0),'SetLastError':(1,0),'SetUnhandledExceptionFilter':(1,0),'SetupComm':(3,0),
  'TerminateThread':(2,1),'UnmapViewOfFile':(1,'{ crate::kernel32::rr_mapping::unmap(ctx, a0) }'),
  'LocalAlloc':(2,'{ let p = crate::kernel32::lock().process_heap.alloc(&mut ctx.memory, a1); if a0 & 0x40 != 0 { ctx.memory[p..][..a1 as usize].fill(0); } p }'),
  'LocalFree':(1,'{ crate::kernel32::lock().process_heap.free(&mut ctx.memory, a0); 0 }'),
  'QueryPerformanceFrequency':(1,'{ ctx.memory.write::<u64>(a0, 1_000_000); 1 }'),
  'QueryPerformanceCounter':(1,'{ ctx.memory.write::<u64>(a0, crate::rr_resdll::micros()); 1 }'),
  'GetFileAttributesA':(1,'{ let s = ctx.memory.read_str(a0); let p = crate::kernel32::resolve_path(&s); log::warn!("GetFileAttributesA({s:?} => {p:?})"); if host::fs::exists(&p) { if host::fs::read_dir(&p).is_ok() { 0x10 } else { 0x80 } } else { 0xffff_ffff } }'),
  'GetFileSize':(2,'{ match crate::kernel32::rr_file_size(a0) { Some(n) => { if a1 != 0 { ctx.memory.write::<u32>(a1, (n >> 32) as u32); } n as u32 } None => 0xffff_ffff } }'),
 },
 'user32': {**{'LoadStringA':(4,'{ crate::rr_resdll::load_string(ctx, a0, a1, a2, a3) }'),'GetWindowWord':(2,'{ *crate::rr_resdll::WINDOW_WORDS.lock().unwrap().get(&(a0, a1)).unwrap_or(&0) as u32 }'),'SendMessageA':(4,'{ match crate::user32::rr_windows::wndproc_for(a0) { Some(p) => crate::user32::rr_windows::call_wndproc(ctx, p, a0, a1, a2, a3), None => 0 } }'),'DrawTextExA':(6,'{ crate::rr_text::draw_text(ctx, a0, a1, a2, a3, a4, a5) }'),'DrawTextA':(5,'{ crate::rr_text::draw_text(ctx, a0, a1, a2, a3, a4, 0) }'),'GetWindowRect':(2,'{ match crate::user32::rr_windows::screen_rect(a0) { Some(r) => { ctx.memory.write::<[i32; 4]>(a1, r); 1 } None => 0 } }'),'SetWindowsHookExA':(4,'{ crate::user32::rr_windows::set_windows_hook(ctx, a0, a1) }'),'UnhookWindowsHookEx':(1,'{ crate::user32::rr_windows::unhook(a0) }'),'PostThreadMessageA':(4,'{ crate::user32::rr_windows::post_thread_message(a0, a1, a2, a3) }'),'RegisterWindowMessageA':(1,'{ crate::user32::rr_windows::register_window_message(&ctx.memory.read_str(a0)) }'),'GetFocus':(0,'{ crate::user32::rr_windows::FOCUS.load(std::sync::atomic::Ordering::Relaxed) }'),'GetParent':(1,'{ crate::user32::rr_windows::parent_of(a0) }'),'SetWindowWord':(3,'{ crate::rr_resdll::WINDOW_WORDS.lock().unwrap().insert((a0, a1), a2 as u16).unwrap_or(0) as u32 }')}, **{n:(a,0) for n,a in dict(CallNextHookEx=4,CheckRadioButton=4,DrawTextA=5,DrawTextExA=6,FillRect=3,GetDlgItemTextA=4,
   GetFocus=0,GetKeyboardLayoutNameA=1,GetParent=1,GetUpdateRgn=3,GetWindowRect=2,GetWindowWord=2,IsWindowEnabled=1,
   LoadStringA=4,MessageBeep=1,PostThreadMessageA=4,RegisterWindowMessageA=1,SendMessageA=4,SetDlgItemTextA=3,
   SetWindowWord=3,SetWindowsHookExA=4,SystemParametersInfoA=4,UnhookWindowsHookEx=1).items() if n not in ('LoadStringA','GetWindowWord','SetWindowWord','SendMessageA','GetParent','DrawTextExA','DrawTextA','GetWindowRect','SetWindowsHookExA','UnhookWindowsHookEx','PostThreadMessageA','RegisterWindowMessageA','GetFocus')}},
 'gdi32': {n:(a,r) for n,a,r in [('AddFontResourceA',1,'{ crate::rr_text::add_font_resource(ctx, a0) }'),('CreateFontA',14,'{ crate::rr_text::create_font(ctx, a0, a4, a13) }'),('CreateRectRgn',4,0),
   ('CreateRectRgnIndirect',1,0),('GetRegionData',3,0),('GetTextExtentPoint32A',4,'{ crate::rr_text::text_extent(ctx, a0, a1, a2, a3) }'),('RemoveFontResourceA',1,1),
   ('SetBkColor',2,'{ crate::rr_text::set_bk_color(a0, a1) }'),('SetBkMode',2,'{ crate::rr_text::set_bk_mode(a0, a1) }'),('SetTextColor',2,'{ crate::rr_text::set_text_color(a0, a1) }'),('TextOutA',5,'{ crate::rr_text::text_out(ctx, a0, a1 as i32, a2 as i32, a3, a4) }')]},
 'winmm': {n:(a,r) for n,a,r in [('joyGetDevCapsA',3,165),('joyGetPosEx',2,167),('joyReleaseCapture',1,0),('joySetCapture',4,165),
   ('mciGetDeviceIDA',1,0),
   ('midiOutPrepareHeader',3,'{ crate::winmm::rr_midi::prepare(ctx, a1, true) }'),('midiOutReset',1,'{ crate::winmm::rr_midi::reset(ctx) }'),
   ('midiOutSetVolume',2,'{ crate::winmm::rr_midi::set_volume(a1) }'),('midiOutUnprepareHeader',3,'{ crate::winmm::rr_midi::prepare(ctx, a1, false) }'),
   ('midiStreamClose',1,'{ crate::winmm::rr_midi::stream_close() }'),('midiStreamOpen',6,'{ crate::winmm::rr_midi::stream_open(ctx, a0, a3, a4, a5) }'),
   ('midiStreamOut',3,'{ crate::winmm::rr_midi::stream_out(ctx, a1) }'),('midiStreamPause',1,'{ crate::winmm::rr_midi::set_playing(false) }'),
   ('midiStreamProperty',3,'{ crate::winmm::rr_midi::stream_property(ctx, a1, a2) }'),('midiStreamRestart',1,'{ crate::winmm::rr_midi::set_playing(true) }')]},
 'advapi32': {'RegCreateKeyExA':(9,'{ crate::rr_registry::create_key(ctx, a0, a1, a7, a8) }'),
   'RegDeleteKeyA':(2,'{ crate::rr_registry::delete_key(ctx, a0, a1) }'),
   'RegEnumKeyExA':(8,'{ crate::rr_registry::enum_key(ctx, a0, a1, a2, a3) }'),
   'RegEnumValueA':(8,'{ crate::rr_registry::enum_value(ctx, a0, a1, a2, a3, a5, a6, a7) }'),
   'RegFlushKey':(1,0),
   'RegSetValueExA':(6,'{ crate::rr_registry::set_value(ctx, a0, a1, a3, a4, a5) }')},
 'tapi32': {n:(a,'0x8000_0048') for n,a in dict(lineSetDevConfig=4,lineGetCallStatus=2,lineShutdown=1,lineGetAddressStatus=3,lineDeallocateCall=1,
   lineDrop=3,lineClose=1,lineGetCountry=3,lineTranslateAddress=7,lineGetTranslateCaps=3,lineSetCurrentLocation=2,lineGetIcon=3,
   lineTranslateDialog=5,lineGetAddressCaps=6,lineAnswer=3,lineMakeCall=5,lineGetDevCaps=5,lineNegotiateAPIVersion=6,lineGetID=6,
   lineOpen=9,lineDial=3,lineInitialize=5,lineSetStatusMessages=3).items()},
 'wsock32': {n:(a,r) for n,a,r in [('gethostbyname',1,0),('ntohs',1,'a0 & 0xffff'),('WSAGetLastError',0,10050),('recv',4,'0xffff_ffff'),
   ('WSACleanup',0,0),('WSAStartup',2,10091),('gethostname',2,'0xffff_ffff'),('setsockopt',5,'0xffff_ffff'),('getsockname',3,'0xffff_ffff'),
   ('connect',3,'0xffff_ffff'),('ioctlsocket',3,'0xffff_ffff'),('htonl',1,'a0.swap_bytes()'),('bind',3,'0xffff_ffff'),('sendto',6,'0xffff_ffff'),
   ('closesocket',1,0),('send',4,'0xffff_ffff'),('socket',3,'0xffff_ffff'),('htons',1,'(a0 as u16).swap_bytes() as u32'),('recvfrom',6,'0xffff_ffff')]},
 'comdlg32': {'GetSaveFileNameA':(1,'{ crate::rr_registry::get_save_file_name(ctx, a0) }'),'GetOpenFileNameA':(1,'{ crate::rr_registry::get_open_file_name(ctx, a0) }')},
 'comctl32': {'ordinal17':(0,0)},
 'aweman32': {'ordinal2':(4,1)},  # AWEManager (stdcall, 4 args). Nonzero = error: no AWE32 card
}
ORD={'bind':2,'closesocket':3,'connect':4,'getsockname':6,'htonl':8,'htons':9,'ioctlsocket':12,'ntohs':15,'recv':16,'recvfrom':17,'send':19,'sendto':20,'setsockopt':21,'socket':23,'gethostbyname':52,'gethostname':57,'WSAGetLastError':111,'WSAStartup':115,'WSACleanup':116}
S['wsock32']={f'ordinal{ORD[k]}':v for k,v in S['wsock32'].items()}  # wsock32 is imported by ordinal
QUIET={'midiStreamOut','midiOutPrepareHeader','midiOutUnprepareHeader','QueryPerformanceCounter','DrawTextExA','DrawTextA','TextOutA','SetTextColor','SetBkColor','SetBkMode','GetTextExtentPoint32A','EnterCriticalSection','LeaveCriticalSection','SetLastError','GetWindowWord','SendMessageA'}
for dll, fns in S.items():
    out = ['//! Road Rash evaluation stubs (auto-generated by gen_stubs.py; not upstream).',
           '#![allow(unused_variables, unused_mut, unused_braces)]','use runtime::Context;','']
    for name,(n,ret) in sorted(fns.items()):
        args = ''.join(f', a{i}: u32' for i in range(n))
        body = ret if isinstance(ret,str) and ret.startswith('{') else f'{{ {ret} }}'
        out.append('#[win32_derive::dllexport]')
        logline = '' if name in QUIET else f'log::warn!("rr-stub: {dll}::{name}"); '
        out.append(f'pub fn {name}(ctx: &mut Context{args}) -> u32 {{ {logline}{body} }}')
    open(f'{W}/rr_stubs_{dll}.rs','w').write('\n'.join(out)+'\n')
print('ok', sum(len(f) for f in S.values()))
