mod model;
mod store;
mod platform;
mod clipboard;
mod capture;
mod conversion;
mod components;

use model::{Record,Settings};
use std::{collections::HashMap,path::PathBuf,sync::{Arc,Mutex,atomic::{AtomicBool,AtomicIsize,AtomicU32,Ordering}}};
use tauri::{AppHandle,Emitter,Manager,State,WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt,ShortcutState};

pub struct AppState {
    store:Mutex<store::Store>,settings:Mutex<Settings>,root:PathBuf,
    target:AtomicIsize,owner:AtomicIsize,own_sequence:AtomicU32,clipboard_io:Mutex<()>,
    captures:Mutex<Vec<capture::Session>>,capturing:AtomicBool,capture_finish:Mutex<()>,capture_windows:Mutex<()>,pins:Mutex<HashMap<String,capture::Pin>>,
    jobs:Mutex<Vec<conversion::Job>>,stopping:AtomicBool,worker_finished:AtomicBool,
    settings_busy:AtomicBool,
    component_download:Mutex<components::DownloadStatus>,component_cancel:AtomicBool,
}
type Shared<'a>=State<'a,Arc<AppState>>;
fn guard(window:&WebviewWindow)->Result<(),String>{if window.label()=="main"{Ok(())}else{Err("此操作仅适用于主窗口".into())}}
pub fn show_main(app:&AppHandle){
    if app.try_state::<Arc<AppState>>().map_or(false,|state|state.capturing.load(Ordering::SeqCst)){return;}
    if let Some(window)=app.get_webview_window("main"){
        if !window.is_visible().unwrap_or(false){let _=window.show();}
        if window.is_minimized().unwrap_or(false){let _=window.unminimize();}let _=window.set_focus();
    }
}
fn emit_records(app:&AppHandle){let _=app.emit("records-changed",());}

#[tauri::command]
fn bootstrap(window:WebviewWindow,state:Shared<'_>)->Result<serde_json::Value,String>{
    guard(&window)?;let store=state.store.lock().map_err(|e|e.to_string())?;
    // 锁顺序固定为 jobs → store；此处先取记录，再释放 store。
    let records=store.list()?;let bytes=store.storage_bytes()?;drop(store);
    let settings=state.settings.lock().map_err(|e|e.to_string())?.clone();let jobs=state.jobs.lock().map_err(|e|e.to_string())?.clone();
    Ok(serde_json::json!({"records":records,"settings":settings,"jobs":jobs,"storageBytes":bytes}))
}
#[tauri::command]
fn list_records(window:WebviewWindow,state:Shared<'_>)->Result<Vec<Record>,String>{guard(&window)?;let store=state.store.lock().map_err(|e|e.to_string())?;store.list()}
#[tauri::command]
fn storage_usage(window:WebviewWindow,state:Shared<'_>)->Result<u64,String>{guard(&window)?;let store=state.store.lock().map_err(|e|e.to_string())?;store.storage_bytes()}
#[tauri::command]
fn clear_images(window:WebviewWindow,state:Shared<'_>,app:AppHandle)->Result<(),String>{guard(&window)?;
    let store=state.store.lock().map_err(|e|e.to_string())?;let ids=store.list()?.iter().filter(|r|r.kind=="image"&&!r.favorite).map(|r|r.id.clone()).collect::<Vec<_>>();store.delete(&ids,"clipboard")?;drop(store);emit_records(&app);Ok(())}
#[tauri::command]
fn favorite_record(window:WebviewWindow,state:Shared<'_>,app:AppHandle,id:String,favorite:bool)->Result<(),String>{
    guard(&window)?;let store=state.store.lock().map_err(|e|e.to_string())?;let mut record=store.get(&id)?;record.favorite=favorite;store.put(&record)?;drop(store);emit_records(&app);Ok(())
}
#[tauri::command]
fn delete_records(window:WebviewWindow,state:Shared<'_>,app:AppHandle,ids:Vec<String>,view:String)->Result<(),String>{guard(&window)?;
    state.store.lock().map_err(|e|e.to_string())?.delete(&ids,&view)?;emit_records(&app);Ok(())}
#[tauri::command]
fn reorder_records(window:WebviewWindow,state:Shared<'_>,app:AppHandle,ids:Vec<String>)->Result<(),String>{guard(&window)?;
    state.store.lock().map_err(|e|e.to_string())?.reorder(&ids)?;emit_records(&app);Ok(())}
#[tauri::command]
fn save_record(window:WebviewWindow,state:Shared<'_>,app:AppHandle,id:Option<String>,text:String,html:Option<String>,phrase:bool,name:Option<String>)->Result<(),String>{
    guard(&window)?;if text.len()>8*1024*1024||html.as_ref().map_or(false,|h|h.len()>16*1024*1024){return Err("编辑内容过大".into());}
    let mut store=state.store.lock().map_err(|e|e.to_string())?;
    let mut record=if let Some(ref key)=id{store.get(key)?}else{clipboard::empty_record(if phrase{"phrase"}else{"text"},String::new(),"手动编辑".into())};
    if record.kind=="image"{return Err("图片记录不支持文字编辑".into());}
    record.text=text;record.html=html;record.updated_at=model::now();
    if record.kind=="phrase" {
        let name=name.unwrap_or_default().trim().to_string();
        if name.is_empty()||name.chars().count()>60||record.text.trim().is_empty(){return Err("请填写短语名称（最多 60 字）和内容".into());}
        record.name=Some(name);
    }
    let mut formats=vec![platform::unicode_format(&record.text)];if let Some(html)=&record.html{formats.push(platform::html_format(html));}
    if id.is_some(){store.replace_formats(&record,formats)?;}else{store.insert(record,formats,false)?;}
    drop(store);emit_records(&app);Ok(())
}
#[tauri::command]
async fn paste_record(window:WebviewWindow,state:Shared<'_>,id:String,formatted:bool)->Result<(),String>{
    guard(&window)?;let state=state.inner().clone();let target=platform::target_window().unwrap_or_else(||state.target.load(Ordering::SeqCst));
    tauri::async_runtime::spawn_blocking(move||{clipboard::write_record(&state,&id,formatted)?;platform::paste_to(target)}).await.map_err(|e|e.to_string())?
}
#[tauri::command]
async fn copy_record(state:Shared<'_>,id:String,formatted:bool)->Result<(),String>{let state=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||clipboard::write_record(&state,&id,formatted)).await.map_err(|e|e.to_string())?}
#[tauri::command]
fn copy_text(app:AppHandle,state:Shared<'_>,text:String)->Result<(),String>{
    let _io=state.clipboard_io.lock().map_err(|e|e.to_string())?;
    let sequence=platform::write_clipboard(&[platform::unicode_format(&text)],state.owner.load(Ordering::SeqCst))?;
    state.own_sequence.store(sequence,Ordering::SeqCst);
    let merge=state.settings.lock().map_err(|e|e.to_string())?.merge_duplicates;
    let record=clipboard::empty_record("text",text.clone(),"拾光 · 手动复制".into());
    state.store.lock().map_err(|e|e.to_string())?.insert(record,vec![platform::unicode_format(&text)],merge)?;
    emit_records(&app);Ok(())
}
fn register_shortcuts(app:&AppHandle,settings:&Settings)->Result<(),String>{
    app.global_shortcut().unregister_all().map_err(|e|e.to_string())?;
    for (action,key) in &settings.shortcuts {
        if key.trim().is_empty(){continue;}
        let action=action.clone();
        app.global_shortcut().on_shortcut(key.as_str(),move|app,_,event|{
            if event.state()!=ShortcutState::Pressed{return;}
            if action=="clipboard"{show_main(app);}else{
                let mode=match action.as_str(){"capture"=>"copy","stick"=>"stick",_=>"text"};
                let app=app.clone();std::thread::spawn(move||{
                    let state=app.state::<Arc<AppState>>();if let Err(error)=capture::begin(&app,state.inner(),mode){let _=app.emit("app-error",error);}
                });
            }
        }).map_err(|e|format!("快捷键 {key} 注册失败：{e}"))?;
    }Ok(())
}
#[tauri::command]
async fn save_settings(window:WebviewWindow,state:Shared<'_>,app:AppHandle,mut settings:Settings)->Result<(),String>{
    struct Release<'a>(&'a AtomicBool);impl Drop for Release<'_>{fn drop(&mut self){self.0.store(false,Ordering::SeqCst);}}
    if state.settings_busy.swap(true,Ordering::SeqCst){return Err("设置正在保存，请稍后重试".into());}let _release=Release(&state.settings_busy);
    guard(&window)?;settings.retention_days=7;settings.image_budget_mb=settings.image_budget_mb.clamp(50,4096);
    let old=state.settings.lock().map_err(|e|e.to_string())?.clone();
    let new=settings.clone();let handle=app.clone();let previous=old.clone();
    tauri::async_runtime::spawn_blocking(move||{if let Err(error)=register_shortcuts(&handle,&new){
        let rollback=register_shortcuts(&handle,&previous);return Err(format!("{error}{}",if rollback.is_err(){"；旧快捷键恢复失败，请检查设置"}else{"；已恢复旧快捷键"}));}Ok(())}).await.map_err(|e|e.to_string())??;
    let result=(||{window.set_always_on_top(settings.always_on_top).map_err(|e|e.to_string())?;
        state.store.lock().map_err(|e|e.to_string())?.save_settings(&settings)?;Ok::<(),String>(())})();
    if let Err(error)=result {let _=window.set_always_on_top(old.always_on_top);let handle=app.clone();tauri::async_runtime::spawn_blocking(move||register_shortcuts(&handle,&old)).await.map_err(|e|e.to_string())??;return Err(error);}
    *state.settings.lock().map_err(|e|e.to_string())?=settings.clone();let _=app.emit("settings-changed",settings);Ok(())
}
#[tauri::command]
async fn start_capture(window:WebviewWindow,app:AppHandle,state:Shared<'_>,mode:String)->Result<(),String>{guard(&window)?;let state=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||capture::begin(&app,&state,&mode)).await.map_err(|e|e.to_string())?}
#[tauri::command]
fn capture_session(window:WebviewWindow,state:Shared<'_>)->Result<capture::Session,String>{let sessions=state.captures.lock().map_err(|e|e.to_string())?;sessions.iter().find(|s|s.id==window.label()).cloned().ok_or("框选已结束".into())}
#[tauri::command]
async fn capture_ready(window:WebviewWindow,state:Shared<'_>,image_path:String)->Result<(),String>{let state=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||capture::ready(&state,&window,&image_path)).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn finish_capture(window:WebviewWindow,app:AppHandle,state:Shared<'_>,selection:capture::Selection)->Result<(),String>{let label=window.label().to_string();let state=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||{let result=capture::finish(&app,&state,&label,selection);if let Err(error)=&result{let _=app.emit("app-error",error);}result}).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn cancel_capture(app:AppHandle,state:Shared<'_>)->Result<(),String>{let state=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||capture::cancel(&app,&state)).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn open_pin(window:WebviewWindow,app:AppHandle,state:Shared<'_>,id:String)->Result<(),String>{guard(&window)?;let state=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||{let record=state.store.lock().map_err(|e|e.to_string())?.get(&id)?;capture::open_pin(&app,&state,&record,None)}).await.map_err(|e|e.to_string())?}
#[tauri::command]
fn pin_data(window:WebviewWindow,state:Shared<'_>)->Result<capture::PinData,String>{let pins=state.pins.lock().map_err(|e|e.to_string())?;pins.get(window.label()).map(|p|p.data.clone()).ok_or("贴图不存在".into())}
#[tauri::command]
async fn pin_ready(window:WebviewWindow,app:AppHandle,state:Shared<'_>)->Result<(),String>{let state=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||capture::pin_ready(&app,&state,&window)).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn copy_pin(window:WebviewWindow,app:AppHandle,state:Shared<'_>)->Result<(),String>{let state=state.inner().clone();
    tauri::async_runtime::spawn_blocking(move||capture::copy_pin(&app,&state,window.label())).await.map_err(|e|e.to_string())?}
#[tauri::command]
async fn recognize_pin(window:WebviewWindow,state:Shared<'_>)->Result<String,String>{
    let path={let pins=state.pins.lock().map_err(|e|e.to_string())?;pins.get(window.label()).ok_or("贴图不存在")?.data.image_path.clone()};
    tauri::async_runtime::spawn_blocking(move||platform::recognize(&path)).await.map_err(|e|e.to_string())?}
#[tauri::command]
fn fold_pin(window:WebviewWindow,state:Shared<'_>,folded:bool)->Result<(),String>{capture::fold(&state,&window,folded)}
#[tauri::command]
fn resize_pin(window:WebviewWindow,state:Shared<'_>,width:u32,text_height:u32)->Result<(),String>{
    let mut pins=state.pins.lock().map_err(|e|e.to_string())?;let pin=pins.get_mut(window.label()).ok_or("贴图不存在")?;
    if pin.folded{return Ok(());}let width=width.clamp(32,12000);let scale=window.scale_factor().map_err(|e|e.to_string())?;
    let height=(f64::from(width)*f64::from(pin.data.height)/f64::from(pin.data.width.max(1))).round() as u32+(f64::from(text_height.min(400))*scale).round() as u32;
    window.set_size(tauri::PhysicalSize::new(width,height)).map_err(|e|e.to_string())?;capture::fit_webview(&window)?;pin.saved_size=tauri::PhysicalSize::new(width,height);Ok(())
}
#[tauri::command]
fn close_pin(window:WebviewWindow,state:Shared<'_>)->Result<(),String>{if !window.label().starts_with("pin-"){return Err("这不是贴图窗口".into());}
    capture::release_pin(&state,window.label())?;window.destroy().map_err(|e|e.to_string())}
#[tauri::command]
async fn recognize_record(state:Shared<'_>,id:String)->Result<String,String>{let path=state.store.lock().map_err(|e|e.to_string())?.get(&id)?.image_path.ok_or("这不是图片记录")?;
    tauri::async_runtime::spawn_blocking(move||platform::recognize(&path)).await.map_err(|e|e.to_string())?}
#[tauri::command]
fn component_status(window:WebviewWindow,state:Shared<'_>)->Result<components::DownloadStatus,String>{guard(&window)?;Ok(state.component_download.lock().map_err(|e|e.to_string())?.clone())}
#[tauri::command]
fn download_components(window:WebviewWindow,app:AppHandle,state:Shared<'_>)->Result<(),String>{guard(&window)?;components::start(app,state.inner().clone())}
#[tauri::command]
fn cancel_component_download(window:WebviewWindow,state:Shared<'_>)->Result<(),String>{guard(&window)?;if components::busy(&state){state.component_cancel.store(true,Ordering::SeqCst);}Ok(())}
#[tauri::command]
fn list_jobs(window:WebviewWindow,state:Shared<'_>)->Result<Vec<conversion::Job>,String>{guard(&window)?;Ok(state.jobs.lock().map_err(|e|e.to_string())?.clone())}
#[tauri::command]
fn queue_conversion(window:WebviewWindow,app:AppHandle,state:Shared<'_>,paths:Vec<String>)->Result<(),String>{guard(&window)?;conversion::enqueue(&app,&state,paths)}
#[tauri::command]
fn control_conversion(window:WebviewWindow,app:AppHandle,state:Shared<'_>,id:String,action:String)->Result<(),String>{guard(&window)?;conversion::control(&app,&state,&id,&action)}
#[tauri::command]
fn delete_conversion_records(window:WebviewWindow,app:AppHandle,state:Shared<'_>,ids:Vec<String>)->Result<(),String>{guard(&window)?;conversion::remove(&app,&state,&ids)}
#[tauri::command]
fn exit_application(window:WebviewWindow,state:Shared<'_>)->Result<(),String>{guard(&window)?;
    let mut jobs=state.jobs.lock().map_err(|e|e.to_string())?;
    for job in jobs.iter_mut().filter(|j|matches!(j.status.as_str(),"running"|"queued"|"paused")){job.status="cancelled".into();job.progress=None;}
    conversion::persist(&state,&jobs)?;state.stopping.store(true,Ordering::SeqCst);Ok(())}

pub fn run(){
    let app=tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app,_,_|show_main(app)))
        .plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![bootstrap,list_records,storage_usage,clear_images,favorite_record,delete_records,reorder_records,save_record,paste_record,copy_record,copy_text,save_settings,
            start_capture,capture_session,capture_ready,finish_capture,cancel_capture,open_pin,pin_data,pin_ready,copy_pin,recognize_pin,fold_pin,resize_pin,close_pin,recognize_record,component_status,download_components,cancel_component_download,list_jobs,queue_conversion,control_conversion,delete_conversion_records,exit_application])
        .setup(|app|{
            let root=app.path().app_data_dir()?;let store=store::Store::open(root.clone()).map_err(std::io::Error::other)?;let settings=store.settings().map_err(std::io::Error::other)?;
            store.cleanup(&settings).map_err(std::io::Error::other)?;let jobs=conversion::load(&store).map_err(std::io::Error::other)?;
            std::fs::write(root.join("conversion-worker.py"),include_str!("../../components/conversion-worker.py"))?;
            let component_status=components::initial(&root);
            let state=Arc::new(AppState{store:Mutex::new(store),settings:Mutex::new(settings.clone()),root,
                target:AtomicIsize::new(platform::target_window().unwrap_or(0)),owner:AtomicIsize::new(0),own_sequence:AtomicU32::new(0),clipboard_io:Mutex::new(()),
                captures:Mutex::new(vec![]),capturing:AtomicBool::new(false),capture_finish:Mutex::new(()),capture_windows:Mutex::new(()),pins:Mutex::new(HashMap::new()),jobs:Mutex::new(jobs),stopping:AtomicBool::new(false),worker_finished:AtomicBool::new(false),settings_busy:AtomicBool::new(true),component_download:Mutex::new(component_status),component_cancel:AtomicBool::new(false)});
            if let Some(main)=app.get_webview_window("main"){
                main.set_always_on_top(settings.always_on_top)?;
                #[cfg(windows)] state.owner.store(main.hwnd()?.0 as isize,Ordering::SeqCst);
            }
            app.manage(state.clone());
            if let Err(error)=platform::start_target_tracking(){let _=app.emit("app-error",error);}
            use tauri::{menu::{Menu,MenuItem},tray::TrayIconBuilder};
            let show=MenuItem::with_id(app,"show","打开拾光",true,None::<&str>)?;
            let pause=MenuItem::with_id(app,"pause","暂停 / 恢复记录",true,None::<&str>)?;
            let exit=MenuItem::with_id(app,"exit","退出拾光",true,None::<&str>)?;
            let menu=Menu::with_items(app,&[&show,&pause,&exit])?;
            let mut tray=TrayIconBuilder::new().menu(&menu).tooltip("拾光").on_menu_event(|app,event|{match event.id.as_ref(){
                "show"=>show_main(app),"pause"=>{let state=app.state::<Arc<AppState>>();if let Ok(mut settings)=state.settings.lock(){
                    let mut updated=settings.clone();updated.paused=!updated.paused;
                    if let Ok(store)=state.store.lock(){if store.save_settings(&updated).is_ok(){*settings=updated.clone();let _=app.emit("settings-changed",updated);}}
                };},"exit"=>{let state=app.state::<Arc<AppState>>();let active=state.jobs.lock().map(|j|j.iter().any(|j|matches!(j.status.as_str(),"running"|"queued"|"paused"))).unwrap_or(true);
                    if active{show_main(app);let _=app.emit_to("main","exit-request",());}else{state.stopping.store(true,Ordering::SeqCst);}},_=>{}
            }});
            if let Some(icon)=app.default_window_icon(){tray=tray.icon(icon.clone());}tray.build(app)?;
            let handle=app.handle().clone();let shortcuts=settings.clone();let startup=state.clone();std::thread::spawn(move||{if let Err(error)=register_shortcuts(&handle,&shortcuts){let _=handle.emit("app-error",error);}startup.settings_busy.store(false,Ordering::SeqCst);});
            clipboard::start(app.handle().clone(),state.clone());conversion::start(app.handle().clone(),state.clone());
            let handle=app.handle().clone();let warm_state=state.clone();std::thread::spawn(move||{let _=capture::prewarm(&handle,&warm_state);});
            // 等待转换线程完成进程树清理后再结束进程，避免残留 LibreOffice。
            let handle=app.handle().clone();std::thread::spawn(move||{while !state.stopping.load(Ordering::SeqCst){std::thread::sleep(std::time::Duration::from_millis(100));}
                while !state.worker_finished.load(Ordering::SeqCst)||components::busy(&state){std::thread::sleep(std::time::Duration::from_millis(100));}handle.exit(0);
            });Ok(())
        }).on_window_event(|window,event|{
            if window.label()=="main"{if let tauri::WindowEvent::CloseRequested{api,..}=event{api.prevent_close();let _=window.hide();}}
            if window.label().starts_with("pin-")&&matches!(event,tauri::WindowEvent::Destroyed){if let Some(state)=window.app_handle().try_state::<Arc<AppState>>(){let _=capture::release_pin(&state,window.label());}}
        })
        .build(tauri::generate_context!()).expect("无法创建拾光桌面应用");
    app.run(|app,event|{if let tauri::RunEvent::ExitRequested{api,..}=event{if let Some(state)=app.try_state::<Arc<AppState>>(){state.stopping.store(true,Ordering::SeqCst);if !state.worker_finished.load(Ordering::SeqCst)||components::busy(&state){api.prevent_exit();}else{platform::stop_target_tracking();let _=app.global_shortcut().unregister_all();}}}});
}
