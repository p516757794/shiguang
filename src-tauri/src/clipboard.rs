use crate::{model::{now, Record}, platform, AppState};
use std::{sync::{Arc, atomic::Ordering}, time::Duration};
use tauri::{AppHandle, Emitter};

pub fn empty_record(kind: &str, text: String, source: String) -> Record {
    Record { id:uuid::Uuid::new_v4().to_string(), name:None, kind:kind.into(), text, html:None, image_path:None,
        created_at:now(), updated_at:now(), favorite:false, history_deleted:false, source,
        sort_order:0, merge_count:1, width:0, height:0 }
}

pub fn image_record(state: &AppState, image: image::RgbaImage, source: &str) -> Result<Record,String> {
    let mut record=empty_record("image", "截图".into(), source.into());
    let path=state.root.join("images").join(format!("{}.png",record.id));
    image.save(&path).map_err(|e|e.to_string())?;
    record.width=image.width(); record.height=image.height(); record.image_path=Some(path.to_string_lossy().into());
    Ok(record)
}

pub fn write_record(state: &AppState, id: &str, formatted: bool) -> Result<(),String> {
    let (record, mut formats)={let store=state.store.lock().map_err(|e|e.to_string())?; (store.get(id)?,store.formats(id)?)};
    let _io=state.clipboard_io.lock().map_err(|e|e.to_string())?;
    if record.kind=="image" && formats.is_empty() {
        let path=record.image_path.ok_or("图片文件已不存在")?;
        let image=image::open(path).map_err(|e|e.to_string())?.into_rgba8();
        arboard::Clipboard::new().map_err(|e|e.to_string())?.set_image(arboard::ImageData {
            width:image.width() as usize,height:image.height() as usize,bytes:std::borrow::Cow::Owned(image.into_raw())
        }).map_err(|e|e.to_string())?;
        state.own_sequence.store(platform::sequence(),Ordering::SeqCst);
        return Ok(());
    }
    if record.kind!="image" && !formatted { formats=vec![platform::unicode_format(&record.text)]; }
    if formats.is_empty() { formats.push(platform::unicode_format(&record.text)); }
    let sequence=platform::write_clipboard(&formats,state.owner.load(Ordering::SeqCst))?;
    state.own_sequence.store(sequence,Ordering::SeqCst);
    Ok(())
}

pub fn start(app: AppHandle, state: Arc<AppState>) {
    std::thread::spawn(move || {
        let mut last=platform::sequence(); let mut ticks=0u32;
        while !state.stopping.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(120));
            if let Some(target)=platform::target_window() { state.target.store(target,Ordering::SeqCst); }
            ticks=ticks.wrapping_add(1);
            let settings=match state.settings.lock() {Ok(s)=>s.clone(),Err(_)=>break};
            if ticks%500==0 {
                if let Ok(store)=state.store.lock() { let _=store.cleanup(&settings); }
                let _=app.emit("records-changed",());
            }
            let seq=platform::sequence();
            if seq==last { continue; }
            if settings.paused || seq==state.own_sequence.load(Ordering::SeqCst) { last=seq; continue; }
            let source=platform::window_title(state.target.load(Ordering::SeqCst));
            if settings.ignored_apps.iter().any(|s|!s.is_empty() && source.to_lowercase().contains(&s.to_lowercase())) { last=seq;continue; }
            let result=(|| -> Result<(),String> {
                let _io=state.clipboard_io.lock().map_err(|e|e.to_string())?;
                if seq==state.own_sequence.load(Ordering::SeqCst) { return Ok(()); }
                let formats=platform::read_clipboard()?;
                let plain=formats.iter().find(|f|f.name=="CF_UNICODETEXT").map(|f|{
                    let units=f.bytes.chunks_exact(2).map(|b|u16::from_le_bytes([b[0],b[1]])).take_while(|c|*c!=0).collect::<Vec<_>>();String::from_utf16_lossy(&units)
                });
                let has_text=plain.as_ref().map_or(false,|s|!s.trim().is_empty());
                let is_image=!has_text && formats.iter().any(|f|matches!(f.name.as_str(),"PNG"|"CF_DIB"|"CF_DIBV5"));
                let mut record=if is_image {
                    let picture=arboard::Clipboard::new().map_err(|e|e.to_string())?.get_image().map_err(|e|e.to_string())?;
                    let rgba=image::RgbaImage::from_raw(picture.width as u32,picture.height as u32,picture.bytes.into_owned()).ok_or("图片数据不完整")?;
                    image_record(&state,rgba,&source)?
                } else {
                    let Some(text)=plain else { return Ok(()); };if text.trim().is_empty(){return Ok(());}
                    let mut item=empty_record("text",text,source.clone());
                    item.html=formats.iter().find(|f|f.name=="HTML Format").and_then(|f|platform::html_fragment(&f.bytes)); item
                };
                if platform::sequence()!=seq {
                    if let Some(path)=record.image_path.take() { let _=std::fs::remove_file(path); }
                    return Err("复制内容正在变化".into());
                }
                let mut store=state.store.lock().map_err(|e|e.to_string())?;
                let saved=store.insert(record,formats,settings.merge_duplicates)?;store.cleanup(&settings)?;
                if saved.kind=="image" && store.get(&saved.id).is_err(){let _=app.emit("app-error","图片超出存储上限，未保留此图片记录；可在设置中调整上限");}
                Ok(())
            })();
            if result.is_ok() { last=seq; let _=app.emit("records-changed",()); }
            // 剪贴板可能短暂被 Word 等程序占用；保留序列号，下次轮询重试。
        }
    });
}
