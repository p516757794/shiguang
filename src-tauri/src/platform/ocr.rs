use image::{imageops, Rgba, RgbaImage};
use windows::{
    Foundation::Rect,
    Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::OcrEngine,
    Storage::Streams::DataWriter,
    Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
};

struct Apartment;
impl Drop for Apartment { fn drop(&mut self) { unsafe { RoUninitialize(); } } }
#[derive(Clone)]
struct Word { text: String, rect: Rect }
#[derive(Clone)]
struct Line { text: String, rect: Rect, words: Vec<Word> }
fn cjk(text: &str) -> bool {
    text.chars().any(|c| matches!(c as u32, 0x3400..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x323af))
}
fn overlap(a: Rect, b: Rect) -> f32 {
    let width = (a.X + a.Width).min(b.X + b.Width) - a.X.max(b.X);
    let height = (a.Y + a.Height).min(b.Y + b.Height) - a.Y.max(b.Y);
    (width.max(0.0) * height.max(0.0)) / (a.Width * a.Height).min(b.Width * b.Height).max(1.0)
}
fn union(a: Rect, b: Rect) -> Rect {
    let x = a.X.min(b.X); let y = a.Y.min(b.Y);
    Rect { X: x, Y: y, Width: (a.X + a.Width).max(b.X + b.Width) - x, Height: (a.Y + a.Height).max(b.Y + b.Height) - y }
}
fn prepare(source: RgbaImage, limit: u32) -> Result<SoftwareBitmap, String> {
    const PADDING: u32 = 12;
    let (width, height) = source.dimensions();
    if width == 0 || height == 0 || limit <= PADDING * 2 { return Err("识字图片尺寸无效".into()); }
    let available = limit - PADDING * 2;
    // 不缩小原图去适应 OCR 上限，否则小字会进一步丢失。
    if width > available || height > available { return Err("图片过大，请缩小识字框选范围".into()); }
    let scale = 3.0_f64.min(available as f64 / width.max(height) as f64)
        .min((16_000_000.0 / (width as f64 * height as f64)).sqrt()).max(1.0);
    let mut flat = source;
    // 透明背景先合成白色，避免透明像素变黑干扰文字轮廓。
    for pixel in flat.pixels_mut() {
        let alpha = pixel[3] as u32;
        for channel in 0..3 { pixel[channel] = ((pixel[channel] as u32 * alpha + 255 * (255 - alpha) + 127) / 255) as u8; }
        // 灰度消除屏幕字体的彩色子像素边缘，保留抗锯齿，避免硬二值化损坏细笔画。
        let gray=((pixel[0] as u32 * 299 + pixel[1] as u32 * 587 + pixel[2] as u32 * 114 + 500) / 1000) as u8;
        *pixel=Rgba([gray,gray,gray,255]);
    }
    let resized = if scale > 1.01 {
        let enlarged=imageops::resize(&flat, (width as f64 * scale).round() as u32, (height as f64 * scale).round() as u32, imageops::FilterType::CatmullRom);
        imageops::unsharpen(&enlarged,0.7,5)
    } else { flat };
    let mut canvas = RgbaImage::from_pixel(resized.width() + PADDING * 2, resized.height() + PADDING * 2, Rgba([255, 255, 255, 255]));
    imageops::replace(&mut canvas, &resized, PADDING as i64, PADDING as i64);
    let (width, height) = canvas.dimensions();
    let mut bytes = canvas.into_raw();
    for pixel in bytes.chunks_exact_mut(4) { pixel.swap(0, 2); }
    let create = || -> windows::core::Result<SoftwareBitmap> {
        let writer = DataWriter::new()?; writer.WriteBytes(&bytes)?;
        SoftwareBitmap::CreateCopyWithAlphaFromBuffer(&writer.DetachBuffer()?, BitmapPixelFormat::Bgra8, width as i32, height as i32, BitmapAlphaMode::Ignore)
    };
    create().map_err(|e| format!("无法准备识字画面：{e}"))
}
fn render_words(words: &[Word]) -> String {
    let mut text=String::new();let mut previous:Option<&Word>=None;
    for word in words {
        if let Some(last)=previous {
            let left=last.text.chars().last().unwrap_or(' ');let right=word.text.chars().next().unwrap_or(' ');
            let close="，。！？；：、）】》〉」』〕］｝,.!?;:%)]}".contains(right);
            let open="（【《〈「『〔［｛([{".contains(left);
            let char_width=(last.rect.Width/last.text.chars().count().max(1) as f32+word.rect.Width/word.text.chars().count().max(1) as f32)*0.5;
            let gap=word.rect.X-(last.rect.X+last.rect.Width);
            // Windows 中文引擎会给每个汉字之间添加空格，按词框重新拼接；英文按实际词间距保留空格。
            if !cjk(&left.to_string())&&!cjk(&right.to_string())&&!close&&!open&&gap>char_width*0.25{text.push(' ');}
        }
        text.push_str(&word.text);previous=Some(word);
    }text
}
fn lines(engine: &OcrEngine, bitmap: &SoftwareBitmap) -> windows::core::Result<Vec<Line>> {
    let result = engine.RecognizeAsync(bitmap)?.get()?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        let mut words = Vec::new(); let mut rect = None;
        for word in line.Words()? {
            let bounds = word.BoundingRect()?;
            rect = Some(rect.map_or(bounds, |old| union(old, bounds)));
            words.push(Word { text: word.Text()?.to_string(), rect: bounds });
        }
        words.sort_by(|a,b|a.rect.X.total_cmp(&b.rect.X));
        let text=render_words(&words);
        if let Some(rect) = rect { if !text.trim().is_empty() { lines.push(Line { text, rect, words }); } }
    }
    Ok(lines)
}
fn improve_latin(line: &mut Line, english: &[Line]) {
    // 混排只替换原中文结果中已经识别出的拉丁词，禁止英文引擎覆盖汉字。
    let mut replacements = Vec::new();
    for (index,word) in line.words.iter().enumerate() {
        if cjk(&word.text) || !word.text.chars().any(|c| c.is_ascii_alphabetic()) { continue; }
        let candidate = english.iter().flat_map(|line| &line.words)
            .filter(|other| other.text.chars().any(|c| c.is_ascii_alphabetic()) && overlap(word.rect, other.rect) > 0.6 && other.rect.Width < word.rect.Width * 1.8)
            .filter(|other| !line.words.iter().any(|chinese| cjk(&chinese.text) && overlap(chinese.rect, other.rect) > 0.25))
            .max_by(|a, b| overlap(word.rect, a.rect).total_cmp(&overlap(word.rect, b.rect)));
        if let Some(other) = candidate { replacements.push((index, other.text.clone())); }
    }
    for (index,text) in replacements {line.words[index].text=text;}
    line.text=render_words(&line.words);
}
fn merge(mut chinese: Vec<Line>, english: Vec<Line>) -> String {
    for line in chinese.iter_mut().filter(|line| cjk(&line.text)) { improve_latin(line, &english); }
    let mut removed = vec![false; chinese.len()]; let mut output = Vec::new();
    for line in english {
        if !line.text.chars().any(|c| c.is_ascii_alphanumeric()) { continue; }
        let matches: Vec<usize> = chinese.iter().enumerate().filter(|(_, other)| overlap(line.rect, other.rect) > 0.35).map(|(index, _)| index).collect();
        if matches.iter().any(|index| cjk(&chinese[*index].text)) { continue; }
        for index in matches { removed[index] = true; }
        output.push(line);
    }
    output.extend(chinese.into_iter().enumerate().filter(|(index, _)| !removed[*index]).map(|(_, line)| line));
    output.sort_by(|a, b| a.rect.Y.total_cmp(&b.rect.Y).then(a.rect.X.total_cmp(&b.rect.X)));
    // 同一行的分段按横向位置排列，避免两种引擎的轻微基线偏差打乱顺序。
    let mut start = 0; let mut rows = Vec::new();
    while start < output.len() {
        let top = output[start].rect.Y; let height = output[start].rect.Height;
        let mut end = start + 1;
        while end < output.len() && output[end].rect.Y - top < height.min(output[end].rect.Height) * 0.45 { end += 1; }
        output[start..end].sort_by(|a, b| a.rect.X.total_cmp(&b.rect.X));
        let mut words=output[start..end].iter().flat_map(|line|line.words.iter().cloned()).collect::<Vec<_>>();
        words.sort_by(|a,b|a.rect.X.total_cmp(&b.rect.X));rows.push(render_words(&words));
        start = end;
    }
    rows.join("\n")
}
pub fn recognize(path: &str) -> Result<String, String> {
    let image=image::open(path).map_err(|e|format!("无法读取识字图片：{e}"))?.into_rgba8();
    recognize_image(image)
}
pub fn recognize_image(image:RgbaImage) -> Result<String,String>{
    unsafe { RoInitialize(RO_INIT_MULTITHREADED).map_err(|e| format!("无法初始化识字组件：{e}"))?; }
    let _apartment = Apartment;
    let operation = || -> Result<String, String> {
        let languages = OcrEngine::AvailableRecognizerLanguages().map_err(|e| e.to_string())?;
        let mut chinese = None; let mut english = None;
        for language in languages {
            let tag = language.LanguageTag().map_err(|e| e.to_string())?.to_string().to_ascii_lowercase();
            if (tag.starts_with("zh") && chinese.is_none()) || (tag.starts_with("en") && english.is_none()) {
                let engine = OcrEngine::TryCreateFromLanguage(&language).map_err(|e| e.to_string())?;
                if tag.starts_with("zh") { chinese = Some(engine); } else { english = Some(engine); }
            }
        }
        if chinese.is_none() && english.is_none() { return Err("未安装中文或英文 OCR，请先安装 Windows 对应语言的文字识别组件".into()); }
        let bitmap = prepare(image, OcrEngine::MaxImageDimension().map_err(|e| e.to_string())?)?;
        let mut errors = Vec::new(); let mut succeeded = false;
        let mut read = |engine: Option<OcrEngine>| -> Vec<Line> {
            match engine { Some(engine) => match lines(&engine, &bitmap) { Ok(lines) => { succeeded = true; lines }, Err(error) => { errors.push(error.to_string()); vec![] } }, None => vec![] }
        };
        let chinese = read(chinese); let english = read(english);
        if !succeeded { return Err(errors.join("；")); }
        Ok(merge(chinese, english))
    };
    operation().map_err(|e| format!("识字失败：{e}"))
}
