use crate::{clipboard,model::Record,AppState};
use serde::{Serialize,Deserialize};
use std::{sync::{Arc,atomic::Ordering},time::Duration};
use tauri::{AppHandle,Emitter,Manager,PhysicalPosition,PhysicalSize,WebviewWindow,WebviewWindowBuilder,WebviewUrl};

#[derive(Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct Session {pub id:String,pub mode:String,pub image_path:String,pub x:i32,pub y:i32,pub width:u32,pub height:u32,#[serde(skip)] pub ready:bool}
#[derive(Deserialize)]
pub struct Selection {pub x:u32,pub y:u32,pub width:u32,pub height:u32}
#[derive(Clone,Serialize)]
#[serde(rename_all="camelCase")]
pub struct PinData {pub record_id:String,pub image_path:String,pub width:u32,pub height:u32}
pub struct Pin {pub data:PinData,pub folded:bool,pub saved_size:PhysicalSize<u32>,pub temporary:bool,pub ready:bool,pub capture_image:Option<String>}

pub fn fit_webview(window:&WebviewWindow)->Result<(),String>{
    let view:&tauri::Webview=window.as_ref();
    view.set_bounds(tauri::Rect{position:PhysicalPosition::new(0,0).into(),size:window.inner_size().map_err(|e|e.to_string())?.into()}).map_err(|e|e.to_string())?;
    view.set_auto_resize(true).map_err(|e|e.to_string())
}

fn ensure_window(app:&AppHandle,state:&AppState,label:&str)->Result<(),String>{
    let _pool=state.capture_windows.lock().map_err(|e|e.to_string())?;
    if app.get_webview_window(label).is_none(){
        let window=WebviewWindowBuilder::new(app,label,WebviewUrl::App("index.html?mode=capture".into()))
            .title("拾光 · 框选").decorations(false).shadow(false).transparent(true)
            .no_redirection_bitmap(true).auto_resize().zoom_hotkeys_enabled(false)
            .always_on_top(true).skip_taskbar(true).resizable(false).visible(false).focused(false)
            .disable_drag_drop_handler().build().map_err(|e|e.to_string())?;
        crate::platform::prepare_capture_window(&window);
    }Ok(())
}
pub fn prewarm(app:&AppHandle,state:&AppState)->Result<(),String>{
    for screen in screenshots::Screen::all().map_err(|e|e.to_string())?{
        ensure_window(app,state,&format!("capture-{}",screen.display_info.id))?;
    }Ok(())
}
pub fn ready(state:&AppState,window:&WebviewWindow,image_path:&str)->Result<(),String>{
    let _finish=state.capture_finish.lock().map_err(|e|e.to_string())?;
    let mut sessions=state.captures.lock().map_err(|e|e.to_string())?;
    let Some(session)=sessions.iter_mut().find(|s|s.id==window.label()&&s.image_path==image_path) else{return Ok(());};
    if session.ready{return Ok(());}session.ready=true;drop(sessions);
    if let Err(error)=window.show().and_then(|_|window.set_focus()){
        if let Ok(mut sessions)=state.captures.lock(){if let Some(session)=sessions.iter_mut().find(|s|s.id==window.label()&&s.image_path==image_path){session.ready=false;}}
        return Err(error.to_string());
    }Ok(())
}
pub fn begin(app:&AppHandle,state:&Arc<AppState>,mode:&str)->Result<(),String>{
    if !matches!(mode,"copy"|"stick"|"text"){return Err("框选模式无效".into());}
    if state.capturing.swap(true,Ordering::SeqCst){return Err("请先完成当前框选".into());}
    if let Some(target)=crate::platform::target_window(){state.target.store(target,Ordering::SeqCst);}
    let result=(||{
        if let Some(main)=app.get_webview_window("main"){
            let (send,receive)=std::sync::mpsc::channel();
            app.run_on_main_thread(move||{
                crate::platform::prepare_capture_window(&main);
                let _=send.send(main.hide().map_err(|e|e.to_string()));
            }).map_err(|e|e.to_string())?;
            receive.recv().map_err(|e|e.to_string())??;
        }
        crate::platform::wait_for_capture_frame();
        let screens=screenshots::Screen::all().map_err(|e|e.to_string())?;
        if screens.is_empty(){return Err("未找到可截图的显示器".into());}
        let directory=state.root.join("capture-temp");std::fs::create_dir_all(&directory).map_err(|e|e.to_string())?;
        // 先拍摄所有屏幕，再创建遮罩，避免遮罩被其他显示器的截图采集进去。
        let mut sessions=vec![];
        for screen in screens {
            let image=screen.capture().map_err(|e|format!("无法截取屏幕：{e}"))?;
            let id=format!("capture-{}",screen.display_info.id);let path=directory.join(format!("{}.png",uuid::Uuid::new_v4()));
            use image::ImageEncoder;
            let writer=std::io::BufWriter::new(std::fs::File::create(&path).map_err(|e|e.to_string())?);
            image::codecs::png::PngEncoder::new_with_quality(writer,image::codecs::png::CompressionType::Fast,image::codecs::png::FilterType::Sub)
                .write_image(image.as_raw(),image.width(),image.height(),image::ColorType::Rgba8).map_err(|e|e.to_string())?;
            sessions.push(Session{id,mode:mode.into(),image_path:path.to_string_lossy().into(),x:screen.display_info.x,y:screen.display_info.y,width:image.width(),height:image.height(),ready:false});
        }
        *state.captures.lock().map_err(|e|e.to_string())?=sessions.clone();
        for session in &sessions {
            ensure_window(app,state,&session.id)?;
            let window=app.get_webview_window(&session.id).ok_or("框选窗口未就绪")?;
            window.set_position(PhysicalPosition::new(session.x,session.y)).map_err(|e|e.to_string())?;
            window.set_size(PhysicalSize::new(session.width,session.height)).map_err(|e|e.to_string())?;
            fit_webview(&window)?;
            window.emit("capture-prepare",session).map_err(|e|e.to_string())?;
        }
        // A hidden webview must never leave the application stuck in capture mode.
        let handle=app.clone();let state=state.clone();
        std::thread::spawn(move||{
            std::thread::sleep(Duration::from_secs(10));
            let Ok(_finish)=state.capture_finish.lock() else{return;};
            let waiting=state.captures.lock().map(|current|current.iter().any(|s|!s.ready&&sessions.iter().any(|old|old.image_path==s.image_path))).unwrap_or(false);
            if waiting{clear(&handle,&state,None);let _=handle.emit("app-error","框选画面加载超时，请重试");}
        });Ok(())
    })();
    if result.is_err(){let _=cancel(app,state);}
    result
}
fn clear(app:&AppHandle,state:&AppState,pin:Option<WebviewWindow>){
    // 先释放会话锁，再调用原生窗口接口，避免与主线程会话读取互相等待。
    let sessions=state.captures.lock().map(|mut items|std::mem::take(&mut *items)).unwrap_or_default();
    // 在同一轮原生消息处理中交接窗口，避免先露出桌面再弹出工具 / 贴图。
    let overlays=sessions.iter().filter_map(|session|app.get_webview_window(&session.id)).collect::<Vec<_>>();
    let main=app.get_webview_window("main");let topmost=state.settings.lock().map(|settings|settings.always_on_top).unwrap_or(false);
    let (send,receive)=std::sync::mpsc::channel();
    if app.run_on_main_thread(move||{
        if let Some(main)=&main{let _=main.set_always_on_top(topmost);if main.is_minimized().unwrap_or(false){let _=main.unminimize();}let _=main.show();}
        if let Some(pin)=&pin{let _=pin.show();}
        for overlay in overlays{let _=overlay.hide();}
        if let Some(main)=&main{let _=main.set_focus();}
        let _=send.send(());
    }).is_ok(){let _=receive.recv();}
    for session in sessions {
        if let Some(window)=app.get_webview_window(&session.id){let _=window.emit("capture-clear",());}
        let path=std::path::Path::new(&session.image_path);
        if path.parent()==Some(state.root.join("capture-temp").as_path()){let _=std::fs::remove_file(path);}
    }state.capturing.store(false,Ordering::SeqCst);
}
pub fn cancel(app:&AppHandle,state:&AppState)->Result<(),String>{let _finish=state.capture_finish.lock().map_err(|e|e.to_string())?;clear(app,state,None);Ok(())}
pub fn finish(app:&AppHandle,state:&AppState,label:&str,selection:Selection)->Result<(),String>{
    let _finish=state.capture_finish.lock().map_err(|e|e.to_string())?;
    let session=state.captures.lock().map_err(|e|e.to_string())?.iter().find(|s|s.id==label).cloned().ok_or("框选已结束")?;
    if selection.width<2||selection.height<2||selection.x.checked_add(selection.width).map_or(true,|v|v>session.width)
        ||selection.y.checked_add(selection.height).map_or(true,|v|v>session.height){return Err("请选择有效的截图区域".into());}
    let source=image::open(&session.image_path).map_err(|e|e.to_string())?;
    let image=source.crop_imm(selection.x,selection.y,selection.width,selection.height).into_rgba8();
    match session.mode.as_str(){
        "copy"=>{
            let record=clipboard::image_record(state,image,"拾光截图")?;
            let merge=state.settings.lock().map_err(|e|e.to_string())?.merge_duplicates;
            let record=state.store.lock().map_err(|e|e.to_string())?.insert(record,vec![],merge)?;
            let result=clipboard::write_record(state,&record.id,true);let _=app.emit("records-changed",());clear(app,state,None);result
        },
        "stick"=>{
            let mut record=clipboard::empty_record("image","贴图".into(),"拾光贴图".into());
            let directory=state.root.join("pin-temp");std::fs::create_dir_all(&directory).map_err(|e|e.to_string())?;
            let path=directory.join(format!("{}.png",record.id));image.save(&path).map_err(|e|e.to_string())?;
            record.width=image.width();record.height=image.height();record.image_path=Some(path.to_string_lossy().into());
            create_pin(app,state,&record,Some((session.x+selection.x as i32,session.y+selection.y as i32)),true,Some(session.image_path))
        },
        _=>{clear(app,state,None);drop(_finish);let text=crate::platform::recognize_image(image)?;
            let _=app.emit_to("main","ocr-result",text);Ok(())}
    }
}
pub fn open_pin(app:&AppHandle,state:&AppState,record:&Record,position:Option<(i32,i32)>)->Result<(),String>{
    let _finish=state.capture_finish.lock().map_err(|e|e.to_string())?;
    create_pin(app,state,record,position,false,None)
}
fn create_pin(app:&AppHandle,state:&AppState,record:&Record,position:Option<(i32,i32)>,temporary:bool,capture_image:Option<String>)->Result<(),String>{
    let path=record.image_path.as_ref().ok_or("仅图片记录可以贴图")?;
    let label=format!("pin-{}",uuid::Uuid::new_v4());
    if !temporary{state.store.lock().map_err(|e|e.to_string())?.protected.insert(record.id.clone());}
    let data=PinData{record_id:record.id.clone(),image_path:path.clone(),width:record.width,height:record.height};
    state.pins.lock().map_err(|e|e.to_string())?.insert(label.clone(),Pin{data,folded:false,saved_size:PhysicalSize::new(record.width,record.height),temporary,ready:false,capture_image:capture_image.clone()});
    let result=(||{
        let window=WebviewWindowBuilder::new(app,&label,WebviewUrl::App("index.html?mode=pin".into()))
            .title("拾光贴图").decorations(false).transparent(true).no_redirection_bitmap(true).auto_resize().zoom_hotkeys_enabled(false)
            .shadow(true).always_on_top(true).skip_taskbar(true).resizable(false).visible(false).focused(false).build().map_err(|e|e.to_string())?;
        crate::platform::prepare_capture_window(&window);
        if let Some((x,y))=position{window.set_position(PhysicalPosition::new(x,y)).map_err(|e|e.to_string())?;}else{window.center().map_err(|e|e.to_string())?;}
        window.set_size(PhysicalSize::new(record.width,record.height)).map_err(|e|e.to_string())?;
        fit_webview(&window)?;Ok(())
    })();
    if result.is_err(){if let Some(window)=app.get_webview_window(&label){let _=window.destroy();}release_pin(state,&label)?;}
    else{
        let handle=app.clone();let state=app.state::<Arc<AppState>>().inner().clone();
        std::thread::spawn(move||{
            std::thread::sleep(Duration::from_secs(10));
            let Ok(_finish)=state.capture_finish.lock() else{return;};
            let active=capture_image.as_ref().map_or(false,|image|state.captures.lock().map(|sessions|sessions.iter().any(|s|&s.image_path==image)).unwrap_or(false));
            let pending=state.pins.lock().map(|pins|pins.get(&label).map_or(active,|pin|!pin.ready)).unwrap_or(false);
            if !pending{return;}
            if let Some(window)=handle.get_webview_window(&label){let _=window.destroy();}let _=release_pin(&state,&label);
            if active{clear(&handle,&state,None);}
            let _=handle.emit("app-error","贴图画面加载失败，请重试");
        });
    }
    result
}
pub fn pin_ready(app:&AppHandle,state:&AppState,window:&WebviewWindow)->Result<(),String>{
    let _finish=state.capture_finish.lock().map_err(|e|e.to_string())?;
    let mut pins=state.pins.lock().map_err(|e|e.to_string())?;
    let pin=pins.get_mut(window.label()).ok_or("贴图不存在")?;
    if pin.ready{return Ok(());}let capture_image=pin.capture_image.clone();drop(pins);
    fit_webview(window)?;
    if let Some(image)=capture_image{
        let active=state.captures.lock().map_err(|e|e.to_string())?.iter().any(|session|session.image_path==image);
        if active{clear(app,state,Some(window.clone()));}else{window.destroy().map_err(|e|e.to_string())?;release_pin(state,window.label())?;}
    }else{window.show().map_err(|e|e.to_string())?;}
    if let Ok(mut pins)=state.pins.lock(){if let Some(pin)=pins.get_mut(window.label()){pin.ready=true;}}Ok(())
}
pub fn copy_pin(app:&AppHandle,state:&AppState,label:&str)->Result<(),String>{
    let (data,temporary)={let pins=state.pins.lock().map_err(|e|e.to_string())?;let pin=pins.get(label).ok_or("贴图不存在")?;(pin.data.clone(),pin.temporary)};
    if !temporary{return clipboard::write_record(state,&data.record_id,true);}
    // 手动复制时才生成独立的历史图片，关闭临时贴图不影响这条记录。
    let image=image::open(data.image_path).map_err(|e|e.to_string())?.into_rgba8();
    let record=clipboard::image_record(state,image,"拾光贴图 · 手动复制")?;
    let merge=state.settings.lock().map_err(|e|e.to_string())?.merge_duplicates;
    let record=state.store.lock().map_err(|e|e.to_string())?.insert(record,vec![],merge)?;
    let result=clipboard::write_record(state,&record.id,true);let _=app.emit("records-changed",());result
}
pub fn release_pin(state:&AppState,label:&str)->Result<(),String>{
    let mut pins=state.pins.lock().map_err(|e|e.to_string())?;
    if let Some(pin)=pins.remove(label){if pin.temporary{
        let path=std::path::Path::new(&pin.data.image_path);if path.parent()==Some(state.root.join("pin-temp").as_path()){let _=std::fs::remove_file(path);}
    }else if !pins.values().any(|p|p.data.record_id==pin.data.record_id){
        let mut store=state.store.lock().map_err(|e|e.to_string())?;store.protected.remove(&pin.data.record_id);
        if let Ok(record)=store.get(&pin.data.record_id){if record.history_deleted&&!record.favorite{store.delete(&[record.id],"clipboard")?;}}
    }}Ok(())
}
pub fn fold(state:&AppState,window:&WebviewWindow,folded:bool)->Result<(),String>{
    let mut pins=state.pins.lock().map_err(|e|e.to_string())?;
    let pin=pins.get_mut(window.label()).ok_or("贴图不存在")?;if pin.folded==folded{return Ok(());}
    let scale=window.scale_factor().map_err(|e|e.to_string())?;
    let mut position=window.outer_position().map_err(|e|e.to_string())?;
    let button=(28.0*scale).round() as u32;let inset=(6.0*scale).round() as i32;
    if folded {
        pin.saved_size=window.inner_size().map_err(|e|e.to_string())?;
        position.x+=pin.saved_size.width as i32-inset-button as i32;position.y+=inset;
        window.set_size(PhysicalSize::new(button,button)).map_err(|e|e.to_string())?;
    }else{
        position.x-=pin.saved_size.width as i32-inset-button as i32;position.y-=inset;
        window.set_size(pin.saved_size).map_err(|e|e.to_string())?;
    }
    window.set_position(position).map_err(|e|e.to_string())?;fit_webview(window)?;pin.folded=folded;Ok(())
}
