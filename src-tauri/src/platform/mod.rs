use crate::model::ClipboardFormat;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
mod ocr;
#[cfg(windows)]
pub use windows::*;

pub fn unicode_format(text: &str) -> ClipboardFormat {
    ClipboardFormat { name:"CF_UNICODETEXT".into(), bytes:text.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect() }
}

pub fn html_format(fragment: &str) -> ClipboardFormat {
    let prefix="<html><body><!--StartFragment-->";
    let suffix="<!--EndFragment--></body></html>";
    let placeholder="Version:1.0\r\nStartHTML:0000000000\r\nEndHTML:0000000000\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n";
    let start=placeholder.len(); let fs=start+prefix.len(); let fe=fs+fragment.len();
    let header=format!("Version:1.0\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{:010}\r\nEndFragment:{:010}\r\n",start,fe+suffix.len(),fs,fe);
    ClipboardFormat { name:"HTML Format".into(), bytes:format!("{header}{prefix}{fragment}{suffix}\0").into_bytes() }
}

pub fn html_fragment(bytes: &[u8]) -> Option<String> {
    let raw=String::from_utf8_lossy(bytes);
    let position=|key:&str|raw.lines().find_map(|line|line.strip_prefix(key).and_then(|n|n.trim().parse::<usize>().ok()));
    if let (Some(start),Some(end))=(position("StartFragment:"),position("EndFragment:")) {
        if start<=end && end<=bytes.len() { return String::from_utf8(bytes[start..end].to_vec()).ok(); }
    }
    None
}

#[cfg(not(windows))]
mod fallback {
    use super::*;
    pub fn sequence()->u32 {0}
    pub fn wait_for_capture_frame(){std::thread::sleep(std::time::Duration::from_millis(30));}
    pub fn prepare_capture_window(_: &tauri::WebviewWindow){}
    pub fn start_target_tracking()->Result<(),String>{Ok(())}
    pub fn stop_target_tracking(){}
    pub fn target_window()->Option<isize>{None}
    pub fn window_title(_:isize)->String{String::new()}
    pub fn read_clipboard()->Result<Vec<ClipboardFormat>,String>{Err("当前版本尚未接入此平台剪贴板".into())}
    pub fn write_clipboard(_: &[ClipboardFormat],_:isize)->Result<u32,String>{Err("当前版本仅支持 Windows 系统粘贴".into())}
    pub fn paste_to(_:isize)->Result<(),String>{Err("此平台的插入接口尚未实现".into())}
    pub fn recognize(_: &str)->Result<String,String>{Err("此平台的识字接口尚未实现".into())}
    pub fn recognize_image(_: image::RgbaImage)->Result<String,String>{Err("此平台的识字接口尚未实现".into())}
}
#[cfg(not(windows))]
pub use fallback::*;
