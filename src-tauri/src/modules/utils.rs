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