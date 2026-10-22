use crate::{model::now, AppState};
use serde::{Deserialize, Serialize};
use std::{path::{Path,PathBuf}, process::{Command,Stdio}, sync::{Arc,atomic::Ordering}, time::Duration};
use tauri::{AppHandle,Emitter};

#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
pub struct Job { pub id:String,pub input:String,pub output:Option<String>,pub status:String,
    pub progress:Option<u32>,pub error:Option<String>,pub created_at:i64 }
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Components { python:String,libre_office:String }

pub fn load(store:&crate::store::Store)->Result<Vec<Job>,String>{
    let mut stmt=store.connection.prepare("SELECT data FROM conversions ORDER BY rowid").map_err(|e|e.to_string())?;
    let rows=stmt.query_map([],|r|r.get::<_,String>(0)).map_err(|e|e.to_string())?;
    let mut jobs=vec![];
    for row in rows { let mut job:Job=serde_json::from_str(&row.map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        if job.status=="running" {job.status="failed".into();job.error=Some("上次退出中断，请重试".into());job.progress=None;} jobs.push(job);
    } Ok(jobs)
}
pub fn persist(state:&AppState,jobs:&[Job])->Result<(),String>{
    let mut store=state.store.lock().map_err(|e|e.to_string())?;
    let tx=store.connection.transaction().map_err(|e|e.to_string())?;
    tx.execute("DELETE FROM conversions",[]).map_err(|e|e.to_string())?;
    for job in jobs {tx.execute("INSERT INTO conversions VALUES(?1,?2)",rusqlite::params![job.id,serde_json::to_string(job).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;}
    tx.commit().map_err(|e|e.to_string())
}
fn notify(app:&AppHandle,state:&AppState,jobs:&[Job]) {if let Err(e)=persist(state,jobs){let _=app.emit("app-error",e);}let _=app.emit("jobs-changed",jobs);}
pub fn enqueue(app:&AppHandle,state:&AppState,paths:Vec<String>)->Result<(),String>{
    let mut accepted=vec![];
    for path in paths {
        let file=std::fs::canonicalize(path).map_err(|e|format!("无法读取文件：{e}"))?;
        let ext=file.extension().and_then(|e|e.to_str()).unwrap_or("").to_lowercase();
        if !file.is_file() || !matches!(ext.as_str(),"doc"|"docx"|"pdf") {return Err("仅支持 Word（.doc / .docx）与 PDF 文件".into());}
        let raw=file.to_string_lossy().to_string();
        #[cfg(windows)] let raw=if let Some(unc)=raw.strip_prefix(r"\\?\UNC\"){format!(r"\\{unc}")}else{raw.strip_prefix(r"\\?\").unwrap_or(&raw).to_string()};
        accepted.push(raw);
    }
    let mut jobs=state.jobs.lock().map_err(|e|e.to_string())?;
    for input in accepted {if jobs.iter().any(|j|j.input==input&&matches!(j.status.as_str(),"queued"|"running"|"paused")){continue;}
        jobs.push(Job{id:uuid::Uuid::new_v4().to_string(),input,output:None,status:"queued".into(),progress:None,error:None,created_at:now()});}
    notify(app,state,&jobs);Ok(())
}
pub fn control(app:&AppHandle,state:&AppState,id:&str,action:&str)->Result<(),String>{
    let mut jobs=state.jobs.lock().map_err(|e|e.to_string())?;
    let job=jobs.iter_mut().find(|j|j.id==id).ok_or("转换记录不存在")?;
    match (action,job.status.as_str()) {
        ("pause","queued"|"running")=>job.status="paused".into(),
        ("resume","paused")|("retry","failed"|"cancelled")=>{job.status="queued".into();job.error=None;job.progress=None;},
        ("cancel","queued"|"running"|"paused")=>{job.status="cancelled".into();job.progress=None;},
        _=>return Err("当前状态不能执行此操作".into())
    }
    notify(app,state,&jobs);Ok(())
}
pub fn remove(app:&AppHandle,state:&AppState,ids:&[String])->Result<(),String>{
    let mut jobs=state.jobs.lock().map_err(|e|e.to_string())?;
    if jobs.iter().any(|j|ids.contains(&j.id)&&matches!(j.status.as_str(),"running"|"queued"|"paused")){return Err("请先取消正在处理的任务".into());}
    jobs.retain(|j|!ids.contains(&j.id));notify(app,state,&jobs);Ok(())
}
fn create_output(source:&Path,temporary:&Path,is_active:impl Fn()->bool)->Result<PathBuf,String>{
    use std::io::{Read,Write};
    let parent=source.parent().ok_or("来源文件夹不存在")?;
    let stem=source.file_stem().ok_or("文件名无效")?.to_string_lossy();
    let extension=temporary.extension().and_then(|s|s.to_str()).ok_or("输出格式无效")?;
    for index in 0..10000 {
        let filename=if index==0{format!("{stem}.{extension}")}else{format!("{stem} ({index}).{extension}")};
        let output=parent.join(filename);
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&output) {
            Ok(mut dest)=>{
                let result=(||{
                    let mut source=std::fs::File::open(temporary)?;let mut buffer=[0u8;65536];
                    loop {
                        if !is_active(){return Err(std::io::Error::new(std::io::ErrorKind::Interrupted,"任务已停止"));}
                        let count=source.read(&mut buffer)?;if count==0{break;}dest.write_all(&buffer[..count])?;
                    }
                    dest.flush()?;dest.sync_all()
                })();
                if let Err(e)=result {drop(dest);let _=std::fs::remove_file(&output);return Err(format!("保存输出失败：{e}"));} return Ok(output);
            },
            Err(e) if e.kind()==std::io::ErrorKind::AlreadyExists=>continue,
            Err(e)=>return Err(format!("无法写入来源文件夹：{e}"))
        }
    } Err("同名文件过多，无法生成输出名称".into())
}
#[cfg(windows)]
pub(crate) struct ProcessTree(windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl ProcessTree {
    pub(crate) fn attach(child:&std::process::Child)->Result<Self,String>{
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        unsafe {
            let handle=CreateJobObjectW(std::ptr::null(),std::ptr::null());if handle.is_null(){return Err("无法创建转换进程组".into());}
            let tree=Self(handle);let mut info:JOBOBJECT_EXTENDED_LIMIT_INFORMATION=std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(handle,JobObjectExtendedLimitInformation,&info as *const _ as *const _,std::mem::size_of_val(&info) as u32)==0
                || AssignProcessToJobObject(handle,child.as_raw_handle())==0 {return Err("无法隔离转换进程，请检查系统策略".into());} Ok(tree)
        }
    }
}
#[cfg(windows)]
impl Drop for ProcessTree {fn drop(&mut self){unsafe{windows_sys::Win32::Foundation::CloseHandle(self.0);}}}
fn convert(app:&AppHandle,state:&AppState,job:&Job)->Result<(),String>{
    let config_path=state.settings.lock().map_err(|e|e.to_string())?.component_path.clone();
    if config_path.is_empty(){return Err("尚未配置转换组件，请在设置中点击自动下载组件".into());}
    let config:Components=serde_json::from_slice(&std::fs::read(config_path).map_err(|e|format!("读取组件配置失败：{e}"))?).map_err(|e|format!("组件配置格式错误：{e}"))?;
    if !Path::new(&config.python).is_absolute() || !Path::new(&config.python).is_file(){return Err("配置的 Python 不存在，请使用绝对路径".into());}
    let source=Path::new(&job.input);
    let extension=if source.extension().and_then(|s|s.to_str()).unwrap_or("").eq_ignore_ascii_case("pdf"){"docx"}else{"pdf"};
    let workspace=state.root.join("conversion-temp").join(&job.id);
    std::fs::create_dir_all(&workspace).map_err(|e|e.to_string())?;
    // 上次异常退出可能留下握手文件；不能让新进程跳过隔离握手。
    let _=std::fs::remove_file(workspace.join("ready"));
    let result=(||{
        let output=workspace.join(format!("result.{extension}"));let log_path=workspace.join("worker.log");
        let log=std::fs::File::create(&log_path).map_err(|e|e.to_string())?;
        let mut command=Command::new(&config.python);
        command.args(["-I","-u"]).arg(state.root.join("conversion-worker.py")).arg(source).arg(&output).arg(&config.libre_office).arg(&workspace)
            .stdin(Stdio::null()).stdout(log.try_clone().map_err(|e|e.to_string())?).stderr(log);
        #[cfg(windows)] {use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
        let mut child=command.spawn().map_err(|e|format!("无法启动转换组件：{e}"))?;
        #[cfg(windows)] let _tree=match ProcessTree::attach(&child){Ok(t)=>t,Err(e)=>{let _=child.kill();let _=child.wait();return Err(e);}};
        // worker 等待此文件，确保子进程属于本工具的进程组后才开始转换。
        if let Err(e)=std::fs::write(workspace.join("ready"),b"ready"){let _=child.kill();let _=child.wait();return Err(e.to_string());}
        loop {
            let active=state.jobs.lock().map_err(|e|e.to_string())?.iter().any(|j|j.id==job.id&&j.status=="running");
            if state.stopping.load(Ordering::SeqCst)||!active {let _=child.kill();let _=child.wait();return Ok(());}
            if let Some(status)=child.try_wait().map_err(|e|e.to_string())? {
                if !status.success(){
                    use std::io::{Read,Seek,SeekFrom};let mut bytes=vec![];
                    if let Ok(mut file)=std::fs::File::open(&log_path){let length=file.metadata().map(|m|m.len()).unwrap_or(0);let _=file.seek(SeekFrom::Start(length.saturating_sub(8192)));let _=file.take(8192).read_to_end(&mut bytes);}
                    return Err(format!("转换失败：{}",String::from_utf8_lossy(&bytes).trim()));
                } break;
            } std::thread::sleep(Duration::from_millis(120));
        }
        if std::fs::metadata(&output).map(|m|m.len()).unwrap_or(0)==0{return Err("转换组件未生成有效文件".into());}
        // 输出复制期间不持有队列锁，暂停、取消和托盘操作仍可及时响应。
        let path=create_output(source,&output,||!state.stopping.load(Ordering::SeqCst)&&state.jobs.lock().map(|jobs|jobs.iter().any(|j|j.id==job.id&&j.status=="running")).unwrap_or(false))?;
        let mut jobs=state.jobs.lock().map_err(|e|e.to_string())?;
        if !jobs.iter().any(|j|j.id==job.id&&j.status=="running") {let _=std::fs::remove_file(path);return Ok(());}
        if let Some(current)=jobs.iter_mut().find(|j|j.id==job.id){current.status="completed".into();current.progress=Some(100);current.output=Some(path.to_string_lossy().into());current.error=None;}
        notify(app,state,&jobs);Ok(())
    })();
    if workspace.parent()==Some(state.root.join("conversion-temp").as_path()){let _=std::fs::remove_dir_all(workspace);} result
}
pub fn start(app:AppHandle,state:Arc<AppState>){std::thread::spawn(move||{
    // 即使线程异常退出，也不能使退出协调线程永远等待。
    struct Finished<'a>(&'a std::sync::atomic::AtomicBool);
    impl Drop for Finished<'_>{fn drop(&mut self){self.0.store(true,Ordering::SeqCst);}}
    let _finished=Finished(&state.worker_finished);
    while !state.stopping.load(Ordering::SeqCst){
        let next=state.jobs.lock().ok().and_then(|mut jobs|{
            let job=jobs.iter_mut().find(|j|j.status=="queued")?;job.status="running".into();job.progress=None;let selected=job.clone();notify(&app,&state,&jobs);Some(selected)
        });
        if let Some(job)=next {if let Err(error)=convert(&app,&state,&job){if let Ok(mut jobs)=state.jobs.lock(){
            if let Some(current)=jobs.iter_mut().find(|j|j.id==job.id&&j.status=="running"){current.status="failed".into();current.error=Some(error);current.progress=None;}
            notify(&app,&state,&jobs);
        }}}else{std::thread::sleep(Duration::from_millis(200));}
    }
});}
