use std::path::{Path, PathBuf};
use std::fs;
use std::io::Read;
use base64::{Engine as _, engine::general_purpose};
use encoding_rs::{GBK, UTF_8};
use lofty::{read_from_path, Accessor, TaggedFileExt, AudioFile};
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct TrackMetadata {
    pub path: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub cover: String,
    pub duration: f64,
}

// =================================================================
// 🛡️ 音频文件合法性预检验器 (Thread-Safe, O(1), Zero-Panic)
// =================================================================
// 设计原则：
//   1. 仅读取文件前12字节，亚毫秒级完成，不影响运行效率
//   2. 纯字节比较，无任何可能panic的操作
//   3. 无状态纯函数，天然线程安全
//   4. 检测格式伪装（扩展名与实际内容不匹配），彻底杜绝mp4改mp3崩溃
pub fn validate_audio_file(path: &str) -> Result<(), String> {
    let mut file = std::fs::File::open(path)
        .map_err(|e| format!("Play failed: cannot open file: {}", e))?;
    
    let file_size = file.metadata()
        .map_err(|e| format!("Play failed: cannot read file metadata: {}", e))?
        .len();
    
    // 过小的文件不可能包含有效音频帧
    if file_size < 128 {
        return Err("Play failed: file too small to contain valid audio data".to_string());
    }

    let mut header = [0u8; 12];
    let bytes_read = file.read(&mut header)
        .map_err(|e| format!("Play failed: cannot read file header: {}", e))?;
    
    if bytes_read < 4 {
        return Err("Play failed: file header too short".to_string());
    }

    // Layer 1: 识别文件真实格式签名
    let is_id3v2 = bytes_read >= 3 && &header[0..3] == b"ID3";
    let is_mp3_sync = header[0] == 0xFF && (header[1] & 0xE0) == 0xE0;
    let is_flac = bytes_read >= 4 && &header[0..4] == b"fLaC";
    let is_riff = bytes_read >= 4 && &header[0..4] == b"RIFF";
    let is_ogg = bytes_read >= 4 && &header[0..4] == b"OggS";
    let is_mp4_ftyp = bytes_read >= 8 && &header[4..8] == b"ftyp";
    let is_aac_adts = header[0] == 0xFF && (header[1] & 0xF6) == 0xF0;
    let is_wma_asf = bytes_read >= 4 && header[0] == 0x30 && header[1] == 0x26 
                     && header[2] == 0xB2 && header[3] == 0x75;

    let has_known_signature = is_id3v2 || is_mp3_sync || is_flac || is_riff 
                             || is_ogg || is_mp4_ftyp || is_aac_adts || is_wma_asf;

    if !has_known_signature {
        return Err(format!(
            "Play failed: unrecognized audio format (header bytes: {:02X} {:02X} {:02X} {:02X})",
            header[0], header[1], header[2], header[3]
        ));
    }

    // Layer 2: 格式伪装检测 —— 扩展名与实际内容交叉验证
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // 核心防护：MP4/视频容器伪装为纯音频格式（如mp4直接改后缀为mp3）
    if is_mp4_ftyp {
        match ext.as_str() {
            "m4a" | "mp4" | "aac" | "alac" | "m4b" | "m4p" | "m4r" => {
                // 合法的 MP4 容器音频扩展名，放行
            }
            _ => {
                return Err(format!(
                    "Play failed: file is an MP4/M4A container but has .{} extension (possible renamed video file)",
                    ext
                ));
            }
        }
    }

    // WAV 容器内容验证
    if is_riff && bytes_read >= 12 && ext == "wav" {
        if &header[8..12] != b"WAVE" {
            return Err("Play failed: RIFF file does not contain WAVE data".to_string());
        }
    }

    Ok(())
}

pub fn repair_mojibake(input: &str) -> String {
    if input.chars().any(|c| c as u32 > 0xFF) { return input.to_string(); }
    let bytes: Vec<u8> = input.chars().map(|c| c as u8).collect();
    let (decoded, _, had_errors) = GBK.decode(&bytes);
    if !had_errors { return decoded.into_owned(); }
    input.to_string()
}

fn find_cover_image(file_path: &Path, tag: &lofty::Tag) -> String {
    if let Some(picture) = tag.pictures().first() {
        let base64_str = general_purpose::STANDARD.encode(picture.data());
        let mime = picture.mime_type().as_str(); 
        return format!("data:{};base64,{}", mime, base64_str);
    }
    if let Some(parent) = file_path.parent() {
        let stem = file_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let exact_matches = vec![
            format!("{}.jpg", stem), format!("{}.png", stem), format!("{}.jpeg", stem)
        ];
        for name in &exact_matches {
            let img_path = parent.join(name);
            if img_path.exists() {
                if let Ok(bytes) = fs::read(img_path) {
                    let base64_str = general_purpose::STANDARD.encode(&bytes);
                    return format!("data:image/jpeg;base64,{}", base64_str);
                }
            }
        }
    }
    "DEFAULT_COVER".to_string()
}

pub fn extract_metadata(path: &PathBuf) -> TrackMetadata {
    let filename = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let mut meta = TrackMetadata {
        path: path.to_string_lossy().to_string(),
        title: filename.clone(), artist: "Unknown Artist".to_string(), album: "Unknown Album".to_string(), cover: "DEFAULT_COVER".to_string(), duration: 0.0,
    };
    if let Ok(tagged_file) = read_from_path(path) {
        let tag = tagged_file.primary_tag().or_else(|| tagged_file.first_tag());
        let properties = tagged_file.properties();
        if let Some(t) = tag {
            if let Some(title) = t.title() { let trimmed = title.trim(); if !trimmed.is_empty() { meta.title = repair_mojibake(trimmed); } }
            if let Some(artist) = t.artist() { let trimmed = artist.trim(); if !trimmed.is_empty() { meta.artist = repair_mojibake(trimmed); } }
            if let Some(album) = t.album() { let trimmed = album.trim(); if !trimmed.is_empty() { meta.album = repair_mojibake(trimmed); } }
            let empty_tag = lofty::Tag::new(lofty::TagType::Id3v2);
            meta.cover = find_cover_image(path, tag.unwrap_or(&empty_tag));
        }
        meta.duration = properties.duration().as_secs_f64();
    }
    meta
}

pub fn parse_lyrics_file(path: String) -> Result<String, String> {
    let audio_path = Path::new(&path);
    let lrc_path = audio_path.with_extension("lrc");

    if lrc_path.exists() {
        let mut file = fs::File::open(lrc_path).map_err(|e| e.to_string())?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;

        let (decoded, _, had_errors) = UTF_8.decode(&buffer);
        if !had_errors {
            return Ok(decoded.into_owned());
        }
        let (decoded_gbk, _, _) = GBK.decode(&buffer);
        return Ok(decoded_gbk.into_owned());
    }
    Ok("".to_string())
}