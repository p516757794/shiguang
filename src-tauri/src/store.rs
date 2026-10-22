use crate::model::{now, ClipboardFormat, Record, Settings};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub struct Store { pub connection: Connection, pub root: PathBuf, pub protected: std::collections::HashSet<String> }

impl Store {
    pub fn open(root: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(root.join("images")).map_err(|e| e.to_string())?;
        let connection = Connection::open(root.join("shiguang.sqlite3")).map_err(|e| e.to_string())?;
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
            CREATE TABLE IF NOT EXISTS records(id TEXT PRIMARY KEY, data TEXT NOT NULL,
                fingerprint TEXT NOT NULL, touched_at INTEGER NOT NULL, sort_order INTEGER NOT NULL);
            CREATE INDEX IF NOT EXISTS records_fingerprint ON records(fingerprint);
            CREATE TABLE IF NOT EXISTS formats(record_id TEXT NOT NULL REFERENCES records(id) ON DELETE CASCADE,
                name TEXT NOT NULL, bytes BLOB NOT NULL, PRIMARY KEY(record_id,name));
            CREATE TABLE IF NOT EXISTS settings(id INTEGER PRIMARY KEY CHECK(id=1), data TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS conversions(id TEXT PRIMARY KEY, data TEXT NOT NULL);")
            .map_err(|e| e.to_string())?;
        Ok(Self { connection, root, protected:std::collections::HashSet::new() })
    }

    pub fn settings(&self) -> Result<Settings, String> {
        let value: Option<String> = self.connection.query_row("SELECT data FROM settings WHERE id=1", [], |r| r.get(0))
            .optional().map_err(|e| e.to_string())?;
        value.map(|v| serde_json::from_str(&v).map_err(|e| e.to_string())).unwrap_or_else(|| Ok(Settings::default()))
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<(), String> {
        self.connection.execute("INSERT OR REPLACE INTO settings VALUES(1,?1)",
            [serde_json::to_string(settings).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<Record>, String> {
        let mut stmt = self.connection.prepare("SELECT data,sort_order FROM records ORDER BY sort_order DESC,touched_at DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(|e| e.to_string())?;
        let mut records = vec![];
        for row in rows {
            let (data, order) = row.map_err(|e| e.to_string())?;
            let mut record: Record = serde_json::from_str(&data).map_err(|e| e.to_string())?;
            record.sort_order = order;
            records.push(record);
        }
        Ok(records)
    }

    pub fn get(&self, id: &str) -> Result<Record, String> {
        let data: String = self.connection.query_row("SELECT data FROM records WHERE id=?1", [id], |r| r.get(0))
            .map_err(|_| "记录已不存在".to_string())?;
        serde_json::from_str(&data).map_err(|e| e.to_string())
    }

    pub fn formats(&self, id: &str) -> Result<Vec<ClipboardFormat>, String> {
        let mut stmt = self.connection.prepare("SELECT name,bytes FROM formats WHERE record_id=?1 ORDER BY name").map_err(|e|e.to_string())?;
        let rows = stmt.query_map([id], |r| Ok(ClipboardFormat { name:r.get(0)?, bytes:r.get(1)? })).map_err(|e|e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e|e.to_string())
    }

    pub fn put(&self, record: &Record) -> Result<(), String> {
        let data = serde_json::to_string(record).map_err(|e|e.to_string())?;
        self.connection.execute("UPDATE records SET data=?1,touched_at=?2 WHERE id=?3", params![data, record.updated_at, record.id])
            .map_err(|e|e.to_string())?;
        Ok(())
    }

    pub fn insert(&mut self, mut record: Record, formats: Vec<ClipboardFormat>, merge: bool) -> Result<Record, String> {
        let mut hash = Sha256::new();
        hash.update(record.kind.as_bytes()); hash.update(record.text.as_bytes());
        hash.update(record.html.as_deref().unwrap_or("").as_bytes());
        if record.kind!="image" {for format in &formats { hash.update(format.name.as_bytes()); hash.update(&format.bytes); }}
        if let Some(path) = &record.image_path { hash.update(std::fs::read(path).map_err(|e|e.to_string())?); }
        let fingerprint = format!("{:x}", hash.finalize());
        let old: Option<String> = if merge {
            self.connection.query_row("SELECT id FROM records WHERE fingerprint=?1 ORDER BY touched_at DESC LIMIT 1", [&fingerprint], |r|r.get(0))
                .optional().map_err(|e|e.to_string())?
        } else { None };
        let orphan_image = if let Some(id) = old {
            let previous = self.get(&id)?;
            record.id = id; record.favorite = previous.favorite;
            record.merge_count = previous.merge_count.saturating_add(1);
            // 相同图片复用原文件，活动贴图仍可能引用这个路径。
            if previous.image_path.as_ref().map_or(false,|p|Path::new(p).is_file()) {
                let unused=record.image_path.take();record.image_path=previous.image_path;unused
            }else{previous.image_path}
        } else { None };
        let largest: i64 = self.connection.query_row("SELECT COALESCE(MAX(sort_order),0) FROM records", [], |r|r.get(0)).map_err(|e|e.to_string())?;
        record.sort_order = now().max(largest.saturating_add(1)); record.updated_at = now(); record.history_deleted = false;
        let tx = self.connection.transaction().map_err(|e|e.to_string())?;
        tx.execute("INSERT INTO records VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET data=excluded.data,
            fingerprint=excluded.fingerprint,touched_at=excluded.touched_at,sort_order=excluded.sort_order",
            params![record.id,serde_json::to_string(&record).map_err(|e|e.to_string())?,fingerprint,record.updated_at,record.sort_order]).map_err(|e|e.to_string())?;
        tx.execute("DELETE FROM formats WHERE record_id=?1", [&record.id]).map_err(|e|e.to_string())?;
        for f in formats { tx.execute("INSERT INTO formats VALUES(?1,?2,?3)", params![record.id,f.name,f.bytes]).map_err(|e|e.to_string())?; }
        tx.commit().map_err(|e|e.to_string())?;
        if let Some(path) = orphan_image { if Some(&path) != record.image_path.as_ref() { self.remove_image(&path); } }
        Ok(record)
    }

    pub fn replace_formats(&mut self, record: &Record, formats: Vec<ClipboardFormat>) -> Result<(), String> {
        let id=&record.id;
        let tx = self.connection.transaction().map_err(|e|e.to_string())?;
        tx.execute("UPDATE records SET data=?1,touched_at=?2 WHERE id=?3",params![serde_json::to_string(record).map_err(|e|e.to_string())?,record.updated_at,id]).map_err(|e|e.to_string())?;
        tx.execute("DELETE FROM formats WHERE record_id=?1",[id]).map_err(|e|e.to_string())?;
        // 编辑后的内容不再匹配旧的原始格式指纹。
        tx.execute("UPDATE records SET fingerprint=?1 WHERE id=?2",params![uuid::Uuid::new_v4().to_string(),id]).map_err(|e|e.to_string())?;
        for f in formats { tx.execute("INSERT INTO formats VALUES(?1,?2,?3)",params![id,f.name,f.bytes]).map_err(|e|e.to_string())?; }
        tx.commit().map_err(|e|e.to_string())
    }

    pub fn delete(&self, ids: &[String], view: &str) -> Result<(), String> {
        for id in ids {
            let mut record = self.get(id)?;
            if view == "favorites" { record.favorite = false; }
            else { record.history_deleted = true; }
            if !record.favorite && record.history_deleted && !self.protected.contains(id) {
                self.connection.execute("DELETE FROM records WHERE id=?1",[id]).map_err(|e|e.to_string())?;
                if let Some(path) = record.image_path { self.remove_image(&path); }
            } else { self.put(&record)?; }
        }
        Ok(())
    }

    fn remove_image(&self, path: &str) {
        let image = Path::new(path);
        if image.parent() == Some(self.root.join("images").as_path()) { let _ = std::fs::remove_file(image); }
    }

    pub fn cleanup(&self, settings: &Settings) -> Result<(), String> {
        let records = self.list()?;
        // 按本机日期保留今天及前六天，与日期栏的七个入口一致。
        let offset=format!("-{} days",settings.retention_days.saturating_sub(1));
        let cutoff:i64=self.connection.query_row("SELECT CAST(strftime('%s','now','localtime','start of day',?1,'utc') AS INTEGER)*1000",[offset],|r|r.get(0)).map_err(|e|e.to_string())?;
        let ids = records.iter().filter(|r| (!r.favorite && r.history_deleted) || (r.kind!="phrase" && !r.history_deleted && r.created_at<cutoff)).map(|r|r.id.clone()).collect::<Vec<_>>();
        self.delete(&ids,"clipboard")?;
        let records = self.list()?;
        let mut total = self.storage_bytes()?;
        for record in records.iter().rev().filter(|r|r.kind=="image" && !r.favorite && !r.history_deleted && !self.protected.contains(&r.id)) {
            if total <= u64::from(settings.image_budget_mb) * 1024 * 1024 { break; }
            let size = self.record_image_bytes(record)?;
            self.delete(&[record.id.clone()],"clipboard")?; total = total.saturating_sub(size);
        }
        Ok(())
    }

    fn record_image_bytes(&self, record: &Record) -> Result<u64, String> {
        let disk = record.image_path.as_ref().and_then(|p|std::fs::metadata(p).ok()).map(|m|m.len()).unwrap_or(0);
        let blobs: i64 = self.connection.query_row("SELECT COALESCE(SUM(length(bytes)),0) FROM formats WHERE record_id=?1",[&record.id],|r|r.get(0)).map_err(|e|e.to_string())?;
        Ok(disk+blobs as u64)
    }
    pub fn storage_bytes(&self) -> Result<u64, String> {
        self.list()?.iter().filter(|r|r.kind=="image").try_fold(0,|sum,r|Ok(sum+self.record_image_bytes(r)?))
    }
    pub fn reorder(&mut self, ids: &[String]) -> Result<(), String> {
        let existing = self.list()?;
        let requested: std::collections::HashSet<_> = ids.iter().collect();
        if requested.len()!=ids.len() || ids.iter().any(|id|!existing.iter().any(|r|&r.id==id)) { return Err("排序记录无效".into()); }
        // 只重新分配当前可见记录原有的位置，不挪动其他日期或筛选隐藏的记录。
        let mut slots = existing.iter().filter(|r|requested.contains(&r.id)).map(|r|r.sort_order).collect::<Vec<_>>();
        slots.sort_by(|a,b|b.cmp(a));
        let tx=self.connection.transaction().map_err(|e|e.to_string())?;
        for (id,slot) in ids.iter().zip(slots) { tx.execute("UPDATE records SET sort_order=?1 WHERE id=?2",params![slot,id]).map_err(|e|e.to_string())?; }
        tx.commit().map_err(|e|e.to_string())
    }
}
