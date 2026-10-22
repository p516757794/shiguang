use crate::AppState;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, process::{Command, Stdio}, sync::{Arc, atomic::Ordering}, time::{Duration, Instant}};
use tauri::{AppHandle, Emitter};

#[derive(Deserialize)]
struct Asset { name:String, filename:String, url:String, sha256:String, size:u64,
    #[serde(default,rename="fallbackUrls")] fallback_urls:Vec<String> }
#[derive(Deserialize)]
struct Extractor { bootstrap:Asset, package:Asset }
#[derive(Deserialize)]
struct Manifest { id:String, python:Asset, office:Asset, wheels:Vec<Asset>, extractor:Extractor }
#[derive(Clone, Serialize)]
#[serde(rename_all="camelCase")]
pub struct DownloadStatus {
    pub status:String, pub message:String, pub downloaded:u64, pub total:u64,
    pub config_path:String, pub error:Option<String>, pub available:bool,
}
impl DownloadStatus { pub fn busy(&self)->bool {matches!(self.status.as_str(),"downloading"|"installing")} }
fn manifest()->Result<Manifest,String>{serde_json::from_str(include_str!(concat!(env!("OUT_DIR"),"/component-manifest.json"))).map_err(|e|e.to_string())}
fn managed(root:&Path)->PathBuf{root.join("conversion-components").join("config.json")}
fn valid_config(path:&Path)->bool{
    let Ok(bytes)=fs::read(path) else{return false;};let Ok(value)=serde_json::from_slice::<serde_json::Value>(&bytes) else{return false;};
    ["python","libreOffice"].iter().all(|key|value[*key].as_str().map_or(false,|path|Path::new(path).is_absolute()&&Path::new(path).is_file()))
}
pub fn initial(root:&Path)->DownloadStatus{
    let ready=valid_config(&managed(root));let total=manifest().map(|m|m.python.size+m.office.size+m.wheels.iter().map(|a|a.size).sum::<u64>()+m.extractor.bootstrap.size+m.extractor.package.size).unwrap_or(0);
    DownloadStatus{status:if ready{"ready"}else{"idle"}.into(),message:if ready{"离线组件已就绪"}else{"首次下载后即可离线转换"}.into(),downloaded:if ready{total}else{0},total,
        config_path:if ready{managed(root).to_string_lossy().into()}else{String::new()},error:None,available:cfg!(all(windows,target_arch="x86_64"))}
}
fn update(app:&AppHandle,state:&AppState,change:impl FnOnce(&mut DownloadStatus)){
    if let Ok(mut status)=state.component_download.lock(){change(&mut status);let _=app.emit("components-changed",status.clone());}
}
fn check(state:&AppState)->Result<(),String>{if state.stopping.load(Ordering::SeqCst)||state.component_cancel.load(Ordering::SeqCst){Err("已取消组件下载".into())}else{Ok(())}}
pub fn busy(state:&AppState)->bool{state.component_download.lock().map(|s|s.busy()).unwrap_or(true)}
fn digest(path:&Path,state:&AppState)->Result<String,String>{
    let mut file=fs::File::open(path).map_err(|e|e.to_string())?;let mut hash=Sha256::new();let mut bytes=[0u8;65536];
    loop{check(state)?;let count=file.read(&mut bytes).map_err(|e|e.to_string())?;if count==0{break;}hash.update(&bytes[..count]);}Ok(format!("{:x}",hash.finalize()))
}
fn download(app:&AppHandle,state:&AppState,client:&Client,asset:&Asset,cache:&Path,base:u64)->Result<PathBuf,String>{
    if Path::new(&asset.filename).components().count()!=1||!asset.sha256.chars().all(|c|c.is_ascii_hexdigit())||asset.sha256.len()!=64{return Err("组件清单无效".into());}
    let urls=std::iter::once(&asset.url).chain(&asset.fallback_urls).map(|raw|{
        let url=reqwest::Url::parse(raw).map_err(|e|e.to_string())?;
        if url.scheme()!="https"||!url.username().is_empty()||url.password().is_some()||!matches!(url.host_str(),Some("www.python.org"|"files.pythonhosted.org"|"download.documentfoundation.org"|"www.7-zip.org"|"github.com"|"gh-proxy.com")){return Err("组件下载来源无效".into());}
        Ok(url)
    }).collect::<Result<Vec<_>,String>>()?;
    if urls.len()>4{return Err("组件下载来源过多".into());}
    let path=cache.join(&asset.filename);
    update(app,state,|s|{s.status="downloading".into();s.message=format!("下载 {}",asset.name);s.downloaded=base;});
    if path.is_file()&&fs::metadata(&path).map(|m|m.len()).unwrap_or(0)==asset.size&&digest(&path,state)?==asset.sha256{
        update(app,state,|s|s.downloaded=base+asset.size);return Ok(path);
    }
    // 仅复用已完成且通过 SHA-256 校验的包，残缺下载下次重新开始。
    let partial=cache.join(format!("{}.part",asset.filename));
    let mut last_error=String::new();
    for round in 0..2 {
        for (index,url) in urls.iter().enumerate(){
            check(state)?;
            update(app,state,|s|{s.downloaded=base;s.message=format!("下载 {}{}{}",asset.name,
                if index==0{String::new()}else{format!(" · 备用源 {index}")},if round==0{""}else{" · 自动重试"});});
            // 小包不等待半小时；失败后切换同版本的固定备用地址。
            let timeout=if asset.size<10*1024*1024{120}else{1800};
            let result:Result<PathBuf,(String,bool)>=tauri::async_runtime::block_on(async {
                let mut response=client.get(url.clone()).timeout(Duration::from_secs(timeout)).send().await.and_then(|r|r.error_for_status()).map_err(|e|(network_error(&e),true))?;
                let mut file=fs::File::create(&partial).map_err(|e|(format!("保存组件失败：{e}"),false))?;let mut hash=Sha256::new();let mut count=0u64;let mut tick=Instant::now();
                loop{check(state).map_err(|e|(e,false))?;let Some(bytes)=response.chunk().await.map_err(|e|(network_error(&e),true))? else{break;};
                    count+=bytes.len() as u64;if count>asset.size{return Err(("下载大小与固定组件清单不一致".into(),true));}
                    file.write_all(&bytes).map_err(|e|(format!("保存组件失败：{e}"),false))?;hash.update(&bytes);
                    if tick.elapsed()>Duration::from_millis(150){update(app,state,|s|s.downloaded=base+count);tick=Instant::now();}
                }
                file.sync_all().map_err(|e|(e.to_string(),false))?;drop(file);
                check(state).map_err(|e|(e,false))?;
                if count!=asset.size||format!("{:x}",hash.finalize())!=asset.sha256{return Err(("下载文件未通过固定 SHA-256 和大小校验".into(),true));}
                if path.exists(){fs::remove_file(&path).map_err(|e|(e.to_string(),false))?;}fs::rename(&partial,&path).map_err(|e|(e.to_string(),false))?;
                update(app,state,|s|s.downloaded=base+count);Ok(path.clone())
            });
            match result{Ok(path)=>return Ok(path),Err((error,retryable))=>{
                let _=fs::remove_file(&partial);check(state)?;
                last_error=format!("下载 {} 失败（{}）：{error}",asset.name,url.host_str().unwrap_or("下载源"));
                if !retryable{return Err(last_error);}
            }}
        }
        if round==0{for _ in 0..5{check(state)?;std::thread::sleep(Duration::from_millis(100));}}
    }
    Err(format!("{last_error}。已重试全部下载源，请检查网络后点击重试；已完成的组件包会保留。"))
}
fn network_error(error:&reqwest::Error)->String{
    if error.is_timeout(){return "连接或传输超时".into();}
    if let Some(status)=error.status(){return format!("服务器返回 HTTP {status}");}
    let mut details=vec![error.to_string()];let mut source=std::error::Error::source(error);
    for _ in 0..3{let Some(cause)=source else{break;};details.push(cause.to_string());source=cause.source();}
    details.join(" → ")
}
fn unpack(path:&Path,destination:&Path,state:&AppState)->Result<(),String>{
    fs::create_dir_all(destination).map_err(|e|e.to_string())?;
    let mut archive=zip::ZipArchive::new(fs::File::open(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let mut total=0u64;
    for index in 0..archive.len(){check(state)?;let mut entry=archive.by_index(index).map_err(|e|e.to_string())?;
        let relative=entry.enclosed_name().ok_or("组件压缩包路径无效")?;
        if entry.unix_mode().map_or(false,|mode|mode&0o170000==0o120000){return Err("组件包包含不支持的链接".into());}
        total=total.checked_add(entry.size()).ok_or("组件包过大")?;if total>1024*1024*1024{return Err("组件包过大".into());}
        let target=destination.join(relative);if entry.is_dir(){fs::create_dir_all(target).map_err(|e|e.to_string())?;}else{
            if let Some(parent)=target.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?;}
            let mut output=fs::File::create(target).map_err(|e|e.to_string())?;let mut buffer=[0u8;65536];
            loop{check(state)?;let count=entry.read(&mut buffer).map_err(|e|e.to_string())?;if count==0{break;}output.write_all(&buffer[..count]).map_err(|e|e.to_string())?;}
        }
    }Ok(())
}
fn execute(command:&mut Command,state:&AppState,log:&Path)->Result<(),String>{
    let file=fs::File::create(log).map_err(|e|e.to_string())?;
    command.stdin(Stdio::null()).stdout(file.try_clone().map_err(|e|e.to_string())?).stderr(file);
    #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
    check(state)?;let mut child=command.spawn().map_err(|e|format!("无法准备转换组件：{e}"))?;
    #[cfg(windows)] let _tree=match crate::conversion::ProcessTree::attach(&child){Ok(tree)=>tree,Err(error)=>{let _=child.kill();let _=child.wait();return Err(error);}};
    let start=Instant::now();
    loop{
        if let Err(error)=check(state){let _=child.kill();let _=child.wait();return Err(error);}
        if start.elapsed()>Duration::from_secs(600){let _=child.kill();let _=child.wait();return Err("组件准备超时，请重试".into());}
        if let Some(status)=child.try_wait().map_err(|e|e.to_string())?{if status.success(){return Ok(());}
            use std::io::{Seek,SeekFrom};let mut bytes=vec![];if let Ok(mut file)=fs::File::open(log){let length=file.metadata().map(|m|m.len()).unwrap_or(0);let _=file.seek(SeekFrom::Start(length.saturating_sub(4096)));let _=file.take(4096).read_to_end(&mut bytes);}
            return Err(format!("组件准备失败（{status}）：{}",String::from_utf8_lossy(&bytes).trim()));
        }std::thread::sleep(Duration::from_millis(150));
    }
}
fn activate(app:&AppHandle,state:&AppState,path:&Path)->Result<(),String>{
    // 与设置保存互斥，仅更新组件路径，保留其他最新设置。
    let start=Instant::now();while state.settings_busy.compare_exchange(false,true,Ordering::SeqCst,Ordering::SeqCst).is_err(){check(state)?;if start.elapsed()>Duration::from_secs(15){return Err("设置正在保存，请稍后重试".into());}std::thread::sleep(Duration::from_millis(100));}
    struct Release<'a>(&'a std::sync::atomic::AtomicBool);impl Drop for Release<'_>{fn drop(&mut self){self.0.store(false,Ordering::SeqCst);}}let _release=Release(&state.settings_busy);
    let mut settings=state.settings.lock().map_err(|e|e.to_string())?;let mut updated=settings.clone();updated.component_path=path.to_string_lossy().into();
    state.store.lock().map_err(|e|e.to_string())?.save_settings(&updated)?;*settings=updated.clone();drop(settings);let _=app.emit("settings-changed",updated);Ok(())
}
fn install(app:&AppHandle,state:&AppState)->Result<String,String>{
    let manifest=manifest()?;let root=state.root.join("conversion-components");fs::create_dir_all(&root).map_err(|e|e.to_string())?;
    let config=managed(&state.root);
    if valid_config(&config){check(state)?;activate(app,state,&config)?;return Ok(config.to_string_lossy().into());}
    let cache=root.join("downloads");fs::create_dir_all(&cache).map_err(|e|e.to_string())?;
    let client=Client::builder().https_only(true).user_agent(concat!("Shiguang/",env!("CARGO_PKG_VERSION")," component-downloader"))
        .connect_timeout(Duration::from_secs(20)).read_timeout(Duration::from_secs(10)).timeout(Duration::from_secs(1800)).build().map_err(|e|e.to_string())?;
    let assets=std::iter::once(&manifest.python).chain(std::iter::once(&manifest.office)).chain(&manifest.wheels).chain([&manifest.extractor.bootstrap,&manifest.extractor.package]);
    let mut downloaded=0;let mut paths=vec![];for asset in assets{let path=download(app,state,&client,asset,&cache,downloaded)?;downloaded+=asset.size;paths.push(path);}
    check(state)?;
    let directory=root.join(format!("{}-{}",manifest.id,uuid::Uuid::new_v4()));fs::create_dir(&directory).map_err(|e|e.to_string())?;
    let result=(||{
        update(app,state,|s|{s.status="installing".into();s.message="正在准备独立 Python 环境…".into();});
        let python_dir=directory.join("python");unpack(&paths[0],&python_dir,state)?;
        let packages=python_dir.join("Lib").join("site-packages");unpack(&paths[2],&packages,state)?;
        fs::write(python_dir.join("python313._pth"),"python313.zip\r\n.\r\nLib/site-packages\r\nimport site\r\n").map_err(|e|e.to_string())?;
        let python=python_dir.join("python.exe");let log=directory.join("prepare.log");
        let wheel_end=2+manifest.wheels.len();
        let mut pip=Command::new(&python);pip.args(["-I","-m","pip","--isolated","--disable-pip-version-check","install","--no-index","--no-deps","--no-compile","--no-cache-dir","--no-warn-script-location","--target"]).arg(&packages).args(&paths[3..wheel_end]);
        execute(&mut pip,state,&log)?;
        update(app,state,|s|s.message="正在解包 LibreOffice 便携组件…".into());
        // 仅提取已校验发行包，不运行 PortableApps 安装器或注册系统级 7-Zip。
        let extract_dir=directory.join("extractor");
        execute(Command::new(&paths[wheel_end]).arg("x").arg(&paths[wheel_end+1]).arg(format!("-o{}",extract_dir.display())).arg("-y"),state,&log)?;
        let office_dir=directory.join("LibreOfficePortable");let mut office=Command::new(extract_dir.join("7z.exe"));
        office.arg("x").arg(&paths[1]).arg(format!("-o{}",office_dir.display())).arg("-y");
        execute(&mut office,state,&log)?;
        let program=office_dir.join("App").join("libreoffice").join("program");let office=if program.join("soffice.com").is_file(){program.join("soffice.com")}else{program.join("soffice.exe")};
        if !office.is_file(){return Err("未找到解包后的 LibreOffice，请重试".into());}
        update(app,state,|s|s.message="正在检查组件并自动配置…".into());
        execute(Command::new(&python).args(["-I","-c","from pdf2docx import Converter; import cv2, numpy, lxml.etree; print('ready')"]),state,&log)?;
        let value=serde_json::json!({"python":python,"libreOffice":office});let bytes=serde_json::to_vec_pretty(&value).map_err(|e|e.to_string())?;
        fs::write(directory.join("config.json"),&bytes).map_err(|e|e.to_string())?;
        fs::write(directory.join("download-manifest.json"),include_bytes!(concat!(env!("OUT_DIR"),"/component-manifest.json"))).map_err(|e|e.to_string())?;
        fs::write(directory.join("COMPONENT-NOTICES.md"),include_bytes!("../../components/COMPONENT-NOTICES.md")).map_err(|e|e.to_string())?;
        check(state)?;activate(app,state,&directory.join("config.json"))?;
        // 根索引用于下次启动识别。设置始终指向完成目录，避免半成品被转换任务使用。
        fs::write(&config,&bytes).map_err(|e|e.to_string())?;
        Ok(directory.join("config.json").to_string_lossy().into())
    })();
    if result.is_err(){
        // 只清理本次 UUID 目录；下载缓存保留，重试时重新校验并复用。
        let active=state.settings.lock().map(|s|s.component_path==directory.join("config.json").to_string_lossy()).unwrap_or(true);
        if !active&&directory.parent()==Some(root.as_path()){let _=fs::remove_dir_all(&directory);}
    }result
}
pub fn start(app:AppHandle,state:Arc<AppState>)->Result<(),String>{
    if !cfg!(all(windows,target_arch="x86_64")){return Err("自动组件目前支持 Windows 64 位".into());}
    if state.jobs.lock().map_err(|e|e.to_string())?.iter().any(|j|j.status=="running"){return Err("请等待当前文件转换结束后再配置组件".into());}
    {let mut status=state.component_download.lock().map_err(|e|e.to_string())?;if status.busy(){return Err("组件正在下载或准备中".into());}
        state.component_cancel.store(false,Ordering::SeqCst);status.status="downloading".into();status.message="正在准备下载…".into();status.downloaded=0;status.error=None;let _=app.emit("components-changed",status.clone());}
    std::thread::spawn(move||{
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||install(&app,&state))).unwrap_or_else(|_|Err("组件准备意外中断，请重试".into()));let cancelled=state.component_cancel.load(Ordering::SeqCst)||state.stopping.load(Ordering::SeqCst);
        update(&app,&state,|s|match result{Ok(path)=>{s.status="ready".into();s.message="离线组件已就绪，已自动配置".into();s.config_path=path;s.downloaded=s.total;s.error=None;},Err(error)=>{s.status=if cancelled{"cancelled"}else{"failed"}.into();s.message=if cancelled{"已取消，可重新下载"}else{"下载或准备失败，可重试"}.into();s.error=if cancelled{None}else{Some(error)};}});
    });Ok(())
}
