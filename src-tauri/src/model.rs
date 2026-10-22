use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_millis() as i64
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    pub kind: String,
    pub text: String,
    pub html: Option<String>,
    pub image_path: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub favorite: bool,
    pub history_deleted: bool,
    pub source: String,
    pub sort_order: i64,
    pub merge_count: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ClipboardFormat {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub paste_formatted: bool,
    pub merge_duplicates: bool,
    pub paused: bool,
    pub always_on_top: bool,
    pub retention_days: u32,
    pub image_budget_mb: u32,
    pub ignored_apps: Vec<String>,
    pub shortcuts: BTreeMap<String, String>,
    pub component_path: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            paste_formatted: false, merge_duplicates: true, paused: false,
            always_on_top: true, retention_days: 7, image_budget_mb: 500,
            ignored_apps: vec![], component_path: String::new(),
            shortcuts: [("clipboard", "Ctrl+Alt+V"), ("capture", "Ctrl+Alt+A"),
                ("stick", "Ctrl+Alt+S"), ("ocr", "Ctrl+Alt+T")]
                .into_iter().map(|(k,v)| (k.into(),v.into())).collect(),
        }
    }
}

pub fn title(text: &str) -> String {
    text.lines().find(|line| !line.trim().is_empty()).unwrap_or("文字记录")
        .trim().chars().take(80).collect()
}
