use super::ClipboardFormat;
use std::{ptr, time::Duration};
use windows_sys::Win32::{Foundation::*, System::{DataExchange::*, Memory::*}, UI::{WindowsAndMessaging::*, Input::KeyboardAndMouse::*}};
use std::sync::atomic::{AtomicIsize,Ordering};
use windows_sys::Win32::UI::Accessibility::{SetWinEventHook,UnhookWinEvent,HWINEVENTHOOK};
static LAST_TARGET:AtomicIsize=AtomicIsize::new(0);
static FOREGROUND_HOOK:AtomicIsize=AtomicIsize::new(0);

unsafe extern "system" fn foreground_event(_:HWINEVENTHOOK,_:u32,window:HWND,_:i32,_:i32,_:u32,_:u32){remember_window(window);}
unsafe fn remember_window(window:HWND){
    if window.is_null(){return;}let mut pid=0;GetWindowThreadProcessId(window,&mut pid);
    if pid==std::process::id(){return;}
    let mut name=[0u16;128];let length=GetClassNameW(window,name.as_mut_ptr(),128);
    let class=String::from_utf16_lossy(&name[..length.max(0) as usize]);
    if matches!(class.as_str(),"Shell_TrayWnd"|"Shell_SecondaryTrayWnd"|"Progman"|"WorkerW"){return;}
    LAST_TARGET.store(window as isize,Ordering::SeqCst);
}
pub fn start_target_tracking()->Result<(),String>{unsafe{
    remember_window(GetForegroundWindow());
    // 在 Tauri 主线程注册，由其 Windows 消息循环接收前台切换事件。
    let hook=SetWinEventHook(EVENT_SYSTEM_FOREGROUND,EVENT_SYSTEM_FOREGROUND,std::ptr::null_mut(),Some(foreground_event),0,0,WINEVENT_OUTOFCONTEXT|WINEVENT_SKIPOWNPROCESS);
    if hook.is_null(){return Err("无法监听目标窗口切换，将使用轮询记录目标窗口".into());}
    FOREGROUND_HOOK.store(hook as isize,Ordering::SeqCst);Ok(())
}}
pub fn stop_target_tracking(){let hook=FOREGROUND_HOOK.swap(0,Ordering::SeqCst);if hook!=0{unsafe{UnhookWinEvent(hook as HWINEVENTHOOK);}}}

struct ClipboardGuard;
impl Drop for ClipboardGuard { fn drop(&mut self) { unsafe { CloseClipboard(); } } }
fn open(owner:isize)->Result<ClipboardGuard,String>{
    for _ in 0..10 { if unsafe{OpenClipboard(owner as HWND)}!=0{return Ok(ClipboardGuard);} std::thread::sleep(Duration::from_millis(15)); }
    Err("剪贴板正被其他软件占用，请重试".into())
}
fn format_id(name:&str)->u32{
    match name { "CF_UNICODETEXT"=>13,"CF_DIB"=>8,"CF_DIBV5"=>17,_=>{
        let wide=name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();unsafe{RegisterClipboardFormatW(wide.as_ptr())}
    }}
}
pub fn sequence()->u32{unsafe{GetClipboardSequenceNumber()}}
pub fn wait_for_capture_frame(){unsafe{let _=windows_sys::Win32::Graphics::Dwm::DwmFlush();}}
pub fn prepare_capture_window(window:&tauri::WebviewWindow){
    if let Ok(handle)=window.hwnd(){unsafe{
        let disabled: i32=1;
        let _=windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(handle.0 as HWND,
            windows_sys::Win32::Graphics::Dwm::DWMWA_TRANSITIONS_FORCEDISABLED as u32,
            &disabled as *const _ as *const std::ffi::c_void,std::mem::size_of::<i32>() as u32);
    }}
}
pub fn target_window()->Option<isize>{unsafe{
    remember_window(GetForegroundWindow());let target=LAST_TARGET.load(Ordering::SeqCst);if target==0{None}else{Some(target)}
}}
pub fn window_title(window:isize)->String{unsafe{
    let mut text=[0u16;512];let length=GetWindowTextW(window as HWND,text.as_mut_ptr(),text.len() as i32);
    String::from_utf16_lossy(&text[..length.max(0) as usize])
}}
pub fn read_clipboard()->Result<Vec<ClipboardFormat>,String>{
    let _guard=open(0)?;let mut formats=vec![];
    for name in ["CF_UNICODETEXT","HTML Format","Rich Text Format","PNG","CF_DIBV5","CF_DIB"] {
        let id=format_id(name);if id==0 || unsafe{IsClipboardFormatAvailable(id)}==0{continue;}
        unsafe{
            let handle=GetClipboardData(id);if handle.is_null(){continue;}
            let size=GlobalSize(handle);if size==0 || size>64*1024*1024{continue;}
            let data=GlobalLock(handle);if data.is_null(){continue;}
            let mut bytes=std::slice::from_raw_parts(data as *const u8,size).to_vec();GlobalUnlock(handle);
            // 不把系统分配器的尾部填充当作复制内容，否则相同文字可能无法合并。
            if name=="CF_UNICODETEXT" {if let Some(index)=bytes.chunks_exact(2).position(|b|b==[0,0]){bytes.truncate((index+1)*2);}}
            if name=="Rich Text Format" {if let Some(index)=bytes.iter().position(|b|*b==0){bytes.truncate(index+1);}}
            formats.push(ClipboardFormat{name:name.into(),bytes});
        }
    }
    Ok(formats)
}
pub fn write_clipboard(formats:&[ClipboardFormat],owner:isize)->Result<u32,String>{
    if formats.is_empty(){return Err("这条记录没有可复制的数据".into());}
    let _guard=open(owner)?;
    if unsafe{EmptyClipboard()}==0{return Err("无法清空剪贴板".into());}
    for format in formats {
        let id=format_id(&format.name);if id==0{return Err("无法注册剪贴板格式".into());}
        unsafe{
            let block=GlobalAlloc(GMEM_MOVEABLE,format.bytes.len());if block.is_null(){return Err("剪贴板内存不足".into());}
            let output=GlobalLock(block);if output.is_null(){GlobalFree(block);return Err("无法锁定剪贴板内存".into());}
            ptr::copy_nonoverlapping(format.bytes.as_ptr(),output as *mut u8,format.bytes.len());GlobalUnlock(block);
            if SetClipboardData(id,block).is_null(){GlobalFree(block);return Err("写入剪贴板失败".into());}
            // 成功后由 Windows 管理内存，不能再次释放。
        }
    }
    Ok(sequence())
}
pub fn paste_to(target:isize)->Result<(),String>{unsafe{
    if target==0 || IsWindow(target as HWND)==0{return Err("原目标窗口已关闭；内容已复制，可手动粘贴".into());}
    SetForegroundWindow(target as HWND);std::thread::sleep(Duration::from_millis(90));
    if GetForegroundWindow()!=target as HWND{return Err("无法切回目标软件；内容已复制，请手动粘贴".into());}
    let key=|code:u16,flags:u32|INPUT{r#type:INPUT_KEYBOARD,Anonymous:INPUT_0{ki:KEYBDINPUT{wVk:code,wScan:0,dwFlags:flags,time:0,dwExtraInfo:0}}};
    let input=[key(VK_CONTROL,0),key(0x56,0),key(0x56,KEYEVENTF_KEYUP),key(VK_CONTROL,KEYEVENTF_KEYUP)];
    if SendInput(input.len() as u32,input.as_ptr(),std::mem::size_of::<INPUT>() as i32)!=input.len() as u32{
        return Err("目标软件可能以管理员权限运行，无法自动插入；请手动粘贴".into());
    }
    Ok(())
}}

pub fn recognize(path:&str)->Result<String,String>{
    super::ocr::recognize(path)
}
pub fn recognize_image(image:image::RgbaImage)->Result<String,String>{super::ocr::recognize_image(image)}
