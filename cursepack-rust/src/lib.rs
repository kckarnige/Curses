//! Cursepack v0.3: lossless animated-WebP cursor-theme container.
//!
//! The public API is suitable for embedding in a desktop application. Animated
//! Animated WebP encoding/decoding uses the in-process `webp-animation` crate;
//! container, metadata, cursor, hotspot, and ANI handling is implemented here.

use image::{ImageBuffer, ImageFormat, Rgba, RgbaImage};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashSet, fs, path::{Path, PathBuf}};
use webp_animation::{Decoder as WebpDecoder, Encoder as WebpEncoder, EncoderOptions, EncodingConfig, EncodingType};
use tempfile::TempDir;
use thiserror::Error;

pub const FORMAT_VERSION: u32 = 3;
pub const DESCRIPTOR_FOURCC: &[u8; 4] = b"CPKD";
pub const CURSORS_FOURCC: &[u8; 4] = b"CPKC";
pub const SEPARATOR_MAGIC: &str = "CPKSEP02";
pub const ROLE_ORDER: &[&str] = &[
    "Arrow", "Help", "AppStarting", "Wait", "Crosshair", "IBeam", "NWPen", "No",
    "SizeNS", "SizeWE", "SizeNWSE", "SizeNESW", "SizeAll", "UpArrow", "Hand",
];

#[derive(Debug, Error)]
pub enum CursepackError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid Cursepack: {0}")]
    Invalid(String),
    #[error("WebP animation error: {0}")]
    Webp(String),
}

pub type Result<T> = std::result::Result<T, CursepackError>;

#[derive(Debug, Clone, Default)]
pub struct ThemeInfo {
    pub name: String,
    pub author: String,
    pub author_url: String,
    pub theme_url: String,
    pub description: String,
    pub license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthorMetadata {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeparatorMetadata {
    pub magic: String,
    pub frame_indices: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorMetadata {
    pub role: String,
    pub source_filename: String,
    pub source_format: String,
    pub start_frame: usize,
    pub frame_count: usize,
    pub frame_sizes: Vec<[u32; 2]>,
    pub frame_hotspots: Vec<[u16; 2]>,
    pub frame_delays_ms: Vec<u32>,
    pub marker_rgba: [u8; 4],
    pub marker_original_rgba: [u8; 4],
    #[serde(default)]
    pub ani_info: Option<Value>,
}

/// Public, flattened metadata view retained for convenient use by applications.
/// On disk this is split into the `CPKD` descriptor and `CPKC` cursor chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    // Legacy convenience fields retained for source compatibility. They are
    // synthesized from the v0.3 format definition and are not stored on disk.
    pub format: String,
    pub version: u32,
    pub container: String,
    pub encoding: String,
    pub name: String,
    pub author: AuthorMetadata,
    pub theme_url: String,
    pub description: String,
    pub license: String,
    pub canvas: [u32; 2],
    pub cursor_count: usize,
    pub frame_count_total: usize,
    pub cursors: Vec<CursorMetadata>,
    pub separator: SeparatorMetadata,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DescriptorMetadata {
    format_version: u32,
    name: String,
    author: AuthorMetadata,
    theme_url: String,
    description: String,
    license: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CursorInfoMetadata {
    canvas: [u32; 2],
    cursor_count: usize,
    frame_count_total: usize,
    cursors: Vec<CursorMetadata>,
    separator: SeparatorMetadata,
}

impl Metadata {
    fn into_parts(&self) -> (DescriptorMetadata, CursorInfoMetadata) {
        (
            DescriptorMetadata { format_version: self.version, name: self.name.clone(), author: self.author.clone(), theme_url: self.theme_url.clone(), description: self.description.clone(), license: self.license.clone() },
            CursorInfoMetadata { canvas: self.canvas, cursor_count: self.cursor_count, frame_count_total: self.frame_count_total, cursors: self.cursors.clone(), separator: self.separator.clone() },
        )
    }

    fn from_parts(d: DescriptorMetadata, c: CursorInfoMetadata) -> Self {
        Self { format: "Cursepack".into(), version: d.format_version, container: "WebP".into(), encoding: "lossless".into(), name: d.name, author: d.author, theme_url: d.theme_url, description: d.description, license: d.license, canvas: c.canvas, cursor_count: c.cursor_count, frame_count_total: c.frame_count_total, cursors: c.cursors, separator: c.separator, notes: vec!["v0.3 stores metadata in separate CPKD and CPKC RIFF chunks.".into()] }
    }
}

#[derive(Clone)]
struct CursorFrame {
    image: RgbaImage,
    hotspot: [u16; 2],
    delay_ms: u32,
}

struct SourceCursor {
    role: String,
    filename: String,
    format: String,
    frames: Vec<CursorFrame>,
    ani_info: Option<Value>,
}

/// Encode a directory containing cursor files and an `install.inf` mapping.
///
/// Uses the in-process `webp-animation`/libwebp encoder; no external process
/// or Python runtime is required.
pub fn encode_theme(input_dir: impl AsRef<Path>, output: impl AsRef<Path>, info: ThemeInfo) -> Result<Metadata> {
    let input_dir = input_dir.as_ref();
    let mapping = read_inf_mapping(&input_dir.join("install.inf"))?;
    if mapping.is_empty() {
        return Err(CursepackError::Invalid("no cursor role mappings found in install.inf".into()));
    }

    let mut cursors = Vec::new();
    for role in ROLE_ORDER {
        let Some(filename) = mapping.get(*role) else { continue };
        let path = input_dir.join(filename);
        if !path.is_file() { continue; }
        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
        let (frames, ani_info, format) = if ext == "ani" {
            let (f, a) = parse_ani(&fs::read(&path)?)?;
            (f, Some(a), "ani".to_string())
        } else if ext == "cur" {
            (vec![parse_cur(&fs::read(&path)?)?], None, "cur".to_string())
        } else { continue; };
        cursors.push(SourceCursor { role: (*role).to_string(), filename: filename.clone(), format, frames, ani_info });
    }
    if cursors.is_empty() { return Err(CursepackError::Invalid("no mapped .cur or .ani files were found".into())); }

    let canvas_w = cursors.iter().flat_map(|c| c.frames.iter()).map(|f| f.image.width()).max().unwrap_or(32).max(32);
    let canvas_h = cursors.iter().flat_map(|c| c.frames.iter()).map(|f| f.image.height()).max().unwrap_or(32).max(32);
    let canvas = [canvas_w, canvas_h];
    let mut images = Vec::<RgbaImage>::new();
    let mut delays = Vec::<u32>::new();
    let mut cursor_records = Vec::new();
    let mut separator_indices = Vec::new();

    for cursor in &cursors {
        let start_frame = images.len();
        let first = &cursor.frames[0];
        let marker_rgb = choose_marker(&first.image)?;
        let [hx, hy] = first.hotspot;
        if hx as u32 >= first.image.width() || hy as u32 >= first.image.height() {
            return Err(CursepackError::Invalid(format!("{}: hotspot outside image", cursor.role)));
        }
        let marker_rgba = [marker_rgb[0], marker_rgb[1], marker_rgb[2], 255];
        let marker_original = first.image.get_pixel(hx as u32, hy as u32).0;
        let mut sizes = Vec::new();
        let mut hotspots = Vec::new();
        let mut frame_delays = Vec::new();

        for (i, frame) in cursor.frames.iter().enumerate() {
            let mut canvas_image = ImageBuffer::from_pixel(canvas_w, canvas_h, Rgba([0, 0, 0, 0]));
            image::imageops::replace(&mut canvas_image, &frame.image, 0, 0);
            if i == 0 { canvas_image.put_pixel(hx as u32, hy as u32, Rgba(marker_rgba)); }
            images.push(canvas_image);
            delays.push(frame.delay_ms.max(10));
            sizes.push([frame.image.width(), frame.image.height()]);
            hotspots.push(frame.hotspot);
            frame_delays.push(frame.delay_ms.max(10));
        }

        cursor_records.push(CursorMetadata {
            role: cursor.role.clone(), source_filename: cursor.filename.clone(), source_format: cursor.format.clone(),
            start_frame, frame_count: cursor.frames.len(), frame_sizes: sizes, frame_hotspots: hotspots,
            frame_delays_ms: frame_delays, marker_rgba, marker_original_rgba: marker_original, ani_info: cursor.ani_info.clone(),
        });
        images.push(make_separator(canvas)?);
        delays.push(20);
        separator_indices.push(images.len() - 1);
    }

    let webp = encode_webp_animation(&images, &delays)?;
    let metadata = Metadata {
        format: "Cursepack".into(), version: FORMAT_VERSION, container: "WebP".into(), encoding: "lossless".into(),
        name: if info.name.is_empty() { input_dir.file_name().and_then(|s| s.to_str()).unwrap_or("Untitled").into() } else { info.name },
        author: AuthorMetadata { name: info.author, url: info.author_url }, theme_url: info.theme_url,
        description: info.description, license: info.license, canvas, cursor_count: cursor_records.len(),
        frame_count_total: images.len(), cursors: cursor_records,
        separator: SeparatorMetadata { magic: SEPARATOR_MAGIC.into(), frame_indices: separator_indices },
        notes: vec!["v0.3 stores metadata in separate CPKD and CPKC RIFF chunks.".into()],
    };
    let packed = append_metadata_chunks(&webp, &metadata)?;
    let output = output.as_ref();
    if let Some(parent) = output.parent().filter(|p| !p.as_os_str().is_empty()) { fs::create_dir_all(parent)?; }
    fs::write(output, packed)?;
    Ok(metadata)
}

/// Decode a `.cursepack` file into role-named `.cur` and `.ani` files.
///
/// Animated WebP frames are decoded in-process and returned as composited
/// full-canvas RGBA images.
pub fn decode_theme(input: impl AsRef<Path>, output_dir: impl AsRef<Path>) -> Result<Vec<PathBuf>> {
    let bytes = fs::read(input)?;
    let metadata = extract_metadata(&bytes)?;
    validate_metadata(&metadata)?;
    let webp = strip_metadata_chunks(&bytes)?;
    let decoder = WebpDecoder::new(&webp).map_err(|e| CursepackError::Webp(e.to_string()))?;
    let mut frames = Vec::<RgbaImage>::new();
    for frame in decoder.into_iter() {
        let (w, h) = frame.dimensions();
        let image = RgbaImage::from_raw(w, h, frame.data().to_vec())
            .ok_or_else(|| CursepackError::Invalid("WebP decoder returned an invalid RGBA frame".into()))?;
        frames.push(image);
    }
    if frames.len() != metadata.frame_count_total {
        return Err(CursepackError::Invalid(format!("frame-count mismatch: metadata says {}, WebP contains {}", metadata.frame_count_total, frames.len())));
    }
    let expected_separator = make_separator(metadata.canvas)?;
    for &idx in &metadata.separator.frame_indices {
        let actual = frames.get(idx).ok_or_else(|| CursepackError::Invalid("separator index out of range".into()))?;
        if actual != &expected_separator { return Err(CursepackError::Invalid(format!("separator signature mismatch at frame {idx}"))); }
    }

    let out = output_dir.as_ref();
    fs::create_dir_all(out)?;
    let mut written = Vec::new();
    for cursor in &metadata.cursors {
        let end = cursor.start_frame.checked_add(cursor.frame_count).ok_or_else(|| CursepackError::Invalid("frame range overflow".into()))?;
        if cursor.frame_count == 0 || end > frames.len() || cursor.frame_sizes.len() != cursor.frame_count
            || cursor.frame_hotspots.len() != cursor.frame_count || cursor.frame_delays_ms.len() != cursor.frame_count {
            return Err(CursepackError::Invalid(format!("{}: invalid frame metadata", cursor.role)));
        }
        let mut restored = Vec::new();
        for offset in 0..cursor.frame_count {
            let mut frame = frames[cursor.start_frame + offset].clone();
            if offset == 0 {
                let marker = Rgba(cursor.marker_rgba);
                let mut hits = Vec::new();
                for (x, y, p) in frame.enumerate_pixels() { if *p == marker { hits.push((x, y)); } }
                if hits.len() != 1 { return Err(CursepackError::Invalid(format!("{}: expected one hotspot marker, found {}", cursor.role, hits.len()))); }
                let (x, y) = hits[0];
                let expected = cursor.frame_hotspots[0];
                if [x as u16, y as u16] != expected { return Err(CursepackError::Invalid(format!("{}: marker disagrees with hotspot metadata", cursor.role))); }
                frame.put_pixel(x, y, Rgba(cursor.marker_original_rgba));
            }
            let [w, h] = cursor.frame_sizes[offset];
            if w == 0 || h == 0 || w > frame.width() || h > frame.height() { return Err(CursepackError::Invalid(format!("{}: invalid frame dimensions", cursor.role))); }
            let cropped = image::imageops::crop_imm(&frame, 0, 0, w, h).to_image();
            restored.push(CursorFrame { image: cropped, hotspot: cursor.frame_hotspots[offset], delay_ms: cursor.frame_delays_ms[offset] });
        }
        let extension = if cursor.source_format == "ani" { "ani" } else { "cur" };
        let path = out.join(format!("{}.{}", safe_filename(&cursor.role), extension));
        if extension == "ani" { write_ani(&path, &restored)?; } else { write_cur(&path, &restored[0].image, restored[0].hotspot)?; }
        written.push(path);
    }
    let (descriptor, cursor_info) = metadata.into_parts();
    fs::write(out.join("pack-info.json"), serde_json::to_vec_pretty(&descriptor)?)?;
    fs::write(out.join("cursors.json"), serde_json::to_vec_pretty(&cursor_info)?)?;
    Ok(written)
}

/// Read and validate the embedded metadata without decoding image frames.
pub fn inspect(input: impl AsRef<Path>) -> Result<Metadata> {
    let bytes = fs::read(input)?;
    let metadata = extract_metadata(&bytes)?;
    validate_metadata(&metadata)?;
    Ok(metadata)
}

fn safe_filename(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() || "-_".contains(c) { c } else { '_' }).collect()
}

fn validate_metadata(m: &Metadata) -> Result<()> {
    if m.format != "Cursepack" || m.version != FORMAT_VERSION || m.container != "WebP" || m.encoding != "lossless" {
        return Err(CursepackError::Invalid("unsupported Cursepack version/container/encoding".into()));
    }
    if m.cursors.len() != m.cursor_count || m.separator.magic != SEPARATOR_MAGIC {
        return Err(CursepackError::Invalid("metadata counts or separator signature are inconsistent".into()));
    }
    Ok(())
}

fn read_inf_mapping(path: &Path) -> Result<std::collections::HashMap<String, String>> {
    let text = fs::read_to_string(path)?;
    let re = Regex::new(r#"(?i)HKCU,\s*"Control Panel\\Cursors",\s*"([^"]+)",\s*,\s*"%10%\\%CUR_DIR%\\([^"]+)""#)
        .map_err(|e| CursepackError::Invalid(e.to_string()))?;
    Ok(re.captures_iter(&text).map(|c| (c[1].to_string(), c[2].to_string())).collect())
}

fn choose_marker(image: &RgbaImage) -> Result<[u8; 3]> {
    let mut used = HashSet::new();
    let mut sum = [0u64; 3];
    let mut count = 0u64;
    for p in image.pixels() {
        if p[3] == 255 { used.insert([p[0], p[1], p[2]]); for i in 0..3 { sum[i] += p[i] as u64; } count += 1; }
    }
    let avg = if count == 0 { [127, 127, 127] } else { [
        ((sum[0] as f64 / count as f64).round()) as u8,
        ((sum[1] as f64 / count as f64).round()) as u8,
        ((sum[2] as f64 / count as f64).round()) as u8,
    ]};
    for delta in 0..=255u16 {
        let candidate = [avg[0], avg[1].wrapping_add(delta as u8), avg[2]];
        if !used.contains(&candidate) { return Ok(candidate); }
    }
    for value in 0..(1u32 << 24) {
        let c = [(value >> 16) as u8, (value >> 8) as u8, value as u8];
        if !used.contains(&c) { return Ok(c); }
    }
    Err(CursepackError::Invalid("no unused RGB marker color available".into()))
}

fn make_separator(size: [u32; 2]) -> Result<RgbaImage> {
    let (w, h) = (size[0], size[1]);
    if w < 32 || h < 32 { return Err(CursepackError::Invalid("separator canvas must be at least 32x32".into())); }
    let mut image = ImageBuffer::from_pixel(w, h, Rgba([19, 23, 29, 255]));
    for (i, byte) in SEPARATOR_MAGIC.as_bytes().iter().enumerate() {
        for bit in 0..8 {
            let color = if (byte >> (7 - bit)) & 1 == 1 { [255, 0, 255, 255] } else { [0, 255, 255, 255] };
            for y in bit * 4..bit * 4 + 4 { for x in i as u32 * 4..i as u32 * 4 + 4 { image.put_pixel(x, y, Rgba(color)); } }
        }
    }
    Ok(image)
}

fn encode_webp_animation(images: &[RgbaImage], delays: &[u32]) -> Result<Vec<u8>> {
    if images.is_empty() || images.len() != delays.len() {
        return Err(CursepackError::Invalid("no frames or frame/delay count mismatch".into()));
    }
    let dimensions = images[0].dimensions();
    let mut encoder = WebpEncoder::new_with_options(dimensions, EncoderOptions {
        encoding_config: Some(EncodingConfig { encoding_type: EncodingType::Lossless, quality: 100.0, method: 6 }),
        ..Default::default()
    }).map_err(|e| CursepackError::Webp(e.to_string()))?;
    let mut timestamp = 0i32;
    for (image, delay) in images.iter().zip(delays) {
        if image.dimensions() != dimensions { return Err(CursepackError::Invalid("WebP frames must share one canvas size".into())); }
        encoder.add_frame(image.as_raw(), timestamp).map_err(|e| CursepackError::Webp(e.to_string()))?;
        let duration = i32::try_from((*delay).max(1))
            .map_err(|_| CursepackError::Invalid("frame duration exceeds WebP timestamp range".into()))?;
        timestamp = timestamp.checked_add(duration)
            .ok_or_else(|| CursepackError::Invalid("total animation duration exceeds WebP timestamp range".into()))?;
    }
    let data = encoder.finalize(timestamp).map_err(|e| CursepackError::Webp(e.to_string()))?;
    Ok(data.to_vec())
}

fn riff_chunks(data: &[u8]) -> Result<Vec<([u8; 4], std::ops::Range<usize>)>> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" { return Err(CursepackError::Invalid("not a RIFF WebP file".into())); }
    let declared = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize + 8;
    if declared > data.len() || declared < 12 { return Err(CursepackError::Invalid("invalid RIFF size".into())); }
    let mut pos = 12;
    let mut chunks = Vec::new();
    while pos + 8 <= declared {
        let tag: [u8; 4] = data[pos..pos + 4].try_into().unwrap();
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let start = pos + 8;
        let end = start.checked_add(size).ok_or_else(|| CursepackError::Invalid("RIFF chunk overflow".into()))?;
        let padded = end + (size & 1);
        if padded > declared { return Err(CursepackError::Invalid("truncated RIFF chunk".into())); }
        chunks.push((tag, pos..padded));
        pos = padded;
    }
    if pos != declared { return Err(CursepackError::Invalid("malformed RIFF trailing data".into())); }
    Ok(chunks)
}

fn append_metadata_chunks(webp: &[u8], metadata: &Metadata) -> Result<Vec<u8>> {
    let chunks = riff_chunks(webp)?;
    if chunks.iter().any(|(tag, _)| tag == DESCRIPTOR_FOURCC || tag == CURSORS_FOURCC) {
        return Err(CursepackError::Invalid("WebP already contains Cursepack metadata chunks".into()));
    }
    let (descriptor, cursor_info) = metadata.into_parts();
    let mut out = webp.to_vec();
    for (tag, payload) in [(DESCRIPTOR_FOURCC, serde_json::to_vec(&descriptor)?), (CURSORS_FOURCC, serde_json::to_vec(&cursor_info)?)] {
        out.extend_from_slice(tag);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&payload);
        if payload.len() & 1 == 1 { out.push(0); }
    }
    let riff_size = u32::try_from(out.len() - 8).map_err(|_| CursepackError::Invalid("RIFF file exceeds 4 GiB".into()))?;
    out[4..8].copy_from_slice(&riff_size.to_le_bytes());
    Ok(out)
}

fn extract_metadata(bytes: &[u8]) -> Result<Metadata> {
    let chunks = riff_chunks(bytes)?;
    let descriptor_chunks: Vec<_> = chunks.iter().filter(|(tag, _)| tag == DESCRIPTOR_FOURCC).collect();
    let cursor_chunks: Vec<_> = chunks.iter().filter(|(tag, _)| tag == CURSORS_FOURCC).collect();
    if descriptor_chunks.len() != 1 || cursor_chunks.len() != 1 {
        return Err(CursepackError::Invalid(format!("expected exactly one CPKD and one CPKC chunk; found {} and {}", descriptor_chunks.len(), cursor_chunks.len())));
    }
    let descriptor_payload = riff_chunk_payload(bytes, &descriptor_chunks[0].1)?;
    let cursor_payload = riff_chunk_payload(bytes, &cursor_chunks[0].1)?;
    let descriptor: DescriptorMetadata = serde_json::from_slice(descriptor_payload)?;
    let cursor_info: CursorInfoMetadata = serde_json::from_slice(cursor_payload)?;
    Ok(Metadata::from_parts(descriptor, cursor_info))
}

fn riff_chunk_payload<'a>(bytes: &'a [u8], range: &std::ops::Range<usize>) -> Result<&'a [u8]> {
    let size = u32::from_le_bytes(bytes[range.start + 4..range.start + 8].try_into().unwrap()) as usize;
    let start = range.start + 8;
    let end = start.checked_add(size).ok_or_else(|| CursepackError::Invalid("RIFF metadata chunk overflow".into()))?;
    bytes.get(start..end).ok_or_else(|| CursepackError::Invalid("truncated RIFF metadata payload".into()))
}

fn strip_metadata_chunks(bytes: &[u8]) -> Result<Vec<u8>> {
    let chunks = riff_chunks(bytes)?;
    let mut out = bytes[..12].to_vec();
    let mut descriptor_count = 0;
    let mut cursor_count = 0;
    for (tag, range) in chunks {
        if &tag == DESCRIPTOR_FOURCC { descriptor_count += 1; }
        else if &tag == CURSORS_FOURCC { cursor_count += 1; }
        else { out.extend_from_slice(&bytes[range]); }
    }
    if descriptor_count != 1 || cursor_count != 1 {
        return Err(CursepackError::Invalid(format!("expected one CPKD and one CPKC chunk, found {descriptor_count} and {cursor_count}")));
    }
    let riff_size = u32::try_from(out.len() - 8).map_err(|_| CursepackError::Invalid("RIFF file exceeds 4 GiB".into()))?;
    out[4..8].copy_from_slice(&riff_size.to_le_bytes());
    Ok(out)
}

fn read_riff_chunks(data: &[u8], start: usize, limit: usize) -> Result<Vec<([u8; 4], Vec<u8>)>> {
    let mut pos = start;
    let mut chunks = Vec::new();
    while pos + 8 <= limit {
        let tag = data[pos..pos + 4].try_into().unwrap();
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let end = pos + 8 + size;
        let padded = end + (size & 1);
        if padded > limit { return Err(CursepackError::Invalid("malformed nested RIFF chunk".into())); }
        chunks.push((tag, data[pos + 8..end].to_vec()));
        pos = padded;
    }
    if pos != limit { return Err(CursepackError::Invalid("malformed RIFF chunk padding".into())); }
    Ok(chunks)
}

fn parse_cur(data: &[u8]) -> Result<CursorFrame> {
    if data.len() < 22 || data[0..4] != [0, 0, 2, 0] { return Err(CursepackError::Invalid("not a CUR file".into())); }
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    if count == 0 { return Err(CursepackError::Invalid("CUR contains no images".into())); }
    let hx = u16::from_le_bytes([data[10], data[11]]);
    let hy = u16::from_le_bytes([data[12], data[13]]);
    let size = u32::from_le_bytes(data[14..18].try_into().unwrap()) as usize;
    let offset = u32::from_le_bytes(data[18..22].try_into().unwrap()) as usize;
    let end = offset.checked_add(size).ok_or_else(|| CursepackError::Invalid("CUR image offset overflow".into()))?;
    let blob = data.get(offset..end).ok_or_else(|| CursepackError::Invalid("CUR image data out of bounds".into()))?;
    let image = if blob.starts_with(b"\x89PNG\r\n\x1a\n") {
        image::load_from_memory_with_format(blob, ImageFormat::Png)?.into_rgba8()
    } else { decode_dib(blob)? };
    Ok(CursorFrame { image, hotspot: [hx, hy], delay_ms: 100 })
}

fn decode_dib(data: &[u8]) -> Result<RgbaImage> {
    if data.len() < 40 { return Err(CursepackError::Invalid("CUR DIB header is truncated".into())); }
    let header = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    if header < 40 || data.len() < header { return Err(CursepackError::Invalid("unsupported CUR DIB header".into())); }
    let width = i32::from_le_bytes(data[4..8].try_into().unwrap()).unsigned_abs();
    let doubled_h = i32::from_le_bytes(data[8..12].try_into().unwrap()).unsigned_abs();
    let height = doubled_h / 2;
    let bpp = u16::from_le_bytes(data[14..16].try_into().unwrap());
    let compression = u32::from_le_bytes(data[16..20].try_into().unwrap());
    if width == 0 || height == 0 || ![24, 32].contains(&bpp) || compression != 0 {
        return Err(CursepackError::Invalid(format!("unsupported CUR DIB: {width}x{height}, {bpp}bpp, compression {compression}")));
    }
    let stride = ((width as usize * bpp as usize + 31) / 32) * 4;
    let pixel_bytes = stride.checked_mul(height as usize).ok_or_else(|| CursepackError::Invalid("DIB dimensions overflow".into()))?;
    if header + pixel_bytes > data.len() { return Err(CursepackError::Invalid("CUR DIB pixels are truncated".into())); }
    let top_down = i32::from_le_bytes(data[8..12].try_into().unwrap()) < 0;
    let mut image = RgbaImage::new(width, height);
    for y in 0..height as usize {
        let src_y = if top_down { y } else { height as usize - 1 - y };
        for x in 0..width as usize {
            let p = header + src_y * stride + x * (bpp as usize / 8);
            let b = data[p]; let g = data[p + 1]; let r = data[p + 2];
            let a = if bpp == 32 { data[p + 3] } else { 255 };
            image.put_pixel(x as u32, y as u32, Rgba([r, g, b, a]));
        }
    }
    // CUR DIB alpha is occasionally all zero despite a meaningful AND mask.
    if bpp == 32 && image.pixels().all(|p| p[3] == 0) {
        let mask_offset = header + pixel_bytes;
        let mask_stride = ((width as usize + 31) / 32) * 4;
        if mask_offset + mask_stride * height as usize <= data.len() {
            for y in 0..height as usize { for x in 0..width as usize {
                let sy = height as usize - 1 - y;
                let bit = (data[mask_offset + sy * mask_stride + x / 8] >> (7 - (x % 8))) & 1;
                image.get_pixel_mut(x as u32, y as u32)[3] = if bit == 1 { 0 } else { 255 };
            }}
        }
    }
    Ok(image)
}

fn parse_ani(data: &[u8]) -> Result<(Vec<CursorFrame>, Value)> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"ACON" { return Err(CursepackError::Invalid("not a RIFF ANI file".into())); }
    let limit = (u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize + 8).min(data.len());
    let top = read_riff_chunks(data, 12, limit)?;
    let anih = top.iter().find(|(t, _)| t == b"anih").map(|(_, p)| p).ok_or_else(|| CursepackError::Invalid("ANI missing anih".into()))?;
    if anih.len() < 36 { return Err(CursepackError::Invalid("ANI header too short".into())); }
    let words: Vec<u32> = anih[..36].chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    let c_frames = words[1] as usize; let c_steps = words[2] as usize;
    let jif = words[7].max(1); let flags = words[8];
    let fram_payload = top.iter().find(|(t, p)| t == b"LIST" && p.get(..4) == Some(b"fram")).map(|(_, p)| p)
        .ok_or_else(|| CursepackError::Invalid("ANI missing LIST fram".into()))?;
    let icon_chunks = read_riff_chunks(fram_payload, 4, fram_payload.len())?;
    let icons: Vec<Vec<u8>> = icon_chunks.into_iter().filter(|(t, _)| t == b"icon").map(|(_, p)| p).collect();
    if icons.is_empty() { return Err(CursepackError::Invalid("ANI has no icon frames".into())); }
    let rates: Vec<u32> = top.iter().find(|(t, _)| t == b"rate").map(|(_, p)| p.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()).unwrap_or_else(|| vec![jif; c_steps.max(1)]);
    let sequence: Vec<u32> = top.iter().find(|(t, _)| t == b"seq ").map(|(_, p)| p.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()).unwrap_or_else(|| (0..c_steps.max(c_frames) as u32).collect());
    let mut frames = Vec::new();
    for (step, index) in sequence.iter().enumerate() {
        let idx = (*index as usize).min(icons.len() - 1);
        let mut frame = parse_cur(&icons[idx])?;
        let ticks = rates.get(step).copied().unwrap_or(jif).max(1);
        frame.delay_ms = ((ticks as f64 * 1000.0 / 60.0).round() as u32).max(10);
        frames.push(frame);
    }
    let info = serde_json::json!({ "jif_rate": jif, "flags": flags, "cx": words[3], "cy": words[4] });
    Ok((frames, info))
}

fn write_cur(path: &Path, image: &RgbaImage, hotspot: [u16; 2]) -> Result<()> {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 || w > 256 || h > 256 || hotspot[0] as u32 >= w || hotspot[1] as u32 >= h {
        return Err(CursepackError::Invalid("CUR dimensions/hotspot out of range".into()));
    }
    let mut xor = Vec::with_capacity((w * h * 4) as usize);
    for y in (0..h).rev() { for x in 0..w { let p = image.get_pixel(x, y); xor.extend_from_slice(&[p[2], p[1], p[0], p[3]]); } }
    let mask_stride = ((w as usize + 31) / 32) * 4;
    let mut and_mask = vec![0u8; mask_stride * h as usize];
    for y in 0..h as usize { for x in 0..w as usize {
        if image.get_pixel(x as u32, y as u32)[3] < 128 { and_mask[(h as usize - 1 - y) * mask_stride + x / 8] |= 1 << (7 - x % 8); }
    }}
    let mut dib = Vec::new();
    dib.extend_from_slice(&40u32.to_le_bytes()); dib.extend_from_slice(&(w as i32).to_le_bytes()); dib.extend_from_slice(&((h * 2) as i32).to_le_bytes());
    dib.extend_from_slice(&1u16.to_le_bytes()); dib.extend_from_slice(&32u16.to_le_bytes()); dib.extend_from_slice(&0u32.to_le_bytes());
    dib.extend_from_slice(&((xor.len() + and_mask.len()) as u32).to_le_bytes()); dib.extend_from_slice(&[0u8; 16]);
    dib.extend_from_slice(&xor); dib.extend_from_slice(&and_mask);
    let mut out = Vec::new(); out.extend_from_slice(&[0, 0, 2, 0]); out.extend_from_slice(&1u16.to_le_bytes());
    out.push(if w == 256 { 0 } else { w as u8 }); out.push(if h == 256 { 0 } else { h as u8 }); out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(&hotspot[0].to_le_bytes()); out.extend_from_slice(&hotspot[1].to_le_bytes());
    out.extend_from_slice(&(dib.len() as u32).to_le_bytes()); out.extend_from_slice(&22u32.to_le_bytes()); out.extend_from_slice(&dib);
    fs::write(path, out)?; Ok(())
}

fn riff_chunk(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len() + (payload.len() & 1));
    out.extend_from_slice(tag); out.extend_from_slice(&(payload.len() as u32).to_le_bytes()); out.extend_from_slice(payload);
    if payload.len() & 1 == 1 { out.push(0); } out
}

fn write_ani(path: &Path, frames: &[CursorFrame]) -> Result<()> {
    if frames.is_empty() { return Err(CursepackError::Invalid("cannot write empty ANI".into())); }
    let temp = TempDir::new()?;
    let mut icons = Vec::new();
    for (i, frame) in frames.iter().enumerate() {
        let cur = temp.path().join(format!("{i}.cur")); write_cur(&cur, &frame.image, frame.hotspot)?; icons.push(fs::read(cur)?);
    }
    let rates: Vec<u32> = frames.iter().map(|f| ((f.delay_ms.max(10) as f64 * 60.0 / 1000.0).round() as u32).max(1)).collect();
    let jif = rates[0]; let (w, h) = frames[0].image.dimensions();
    let mut anih = Vec::new(); for n in [36u32, frames.len() as u32, frames.len() as u32, w, h, 32, 1, jif, 1] { anih.extend_from_slice(&n.to_le_bytes()); }
    let mut rate = Vec::new(); for r in rates { rate.extend_from_slice(&r.to_le_bytes()); }
    let mut fram = b"fram".to_vec(); for icon in icons { fram.extend(riff_chunk(b"icon", &icon)); }
    let mut body = riff_chunk(b"anih", &anih); body.extend(riff_chunk(b"rate", &rate)); body.extend(riff_chunk(b"LIST", &fram));
    let mut file = b"RIFF".to_vec(); file.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes()); file.extend_from_slice(b"ACON"); file.extend(body);
    fs::write(path, file)?; Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trip_uses_two_v03_chunks() {
        let m = Metadata { format: "Cursepack".into(), version: 3, container: "WebP".into(), encoding: "lossless".into(), name: "test".into(),
            author: AuthorMetadata { name: "a".into(), url: "".into() }, theme_url: "".into(), description: "".into(), license: "".into(),
            canvas: [32, 32], cursor_count: 0, frame_count_total: 0, cursors: vec![], separator: SeparatorMetadata { magic: SEPARATOR_MAGIC.into(), frame_indices: vec![] }, notes: vec![] };
        let webp = b"RIFF\x04\0\0\0WEBP".to_vec();
        let packed = append_metadata_chunks(&webp, &m).unwrap();
        let got = extract_metadata(&packed).unwrap();
        assert_eq!(got.name, "test");
        let tags: Vec<_> = riff_chunks(&packed).unwrap().into_iter().map(|v| v.0).collect();
        assert!(tags.contains(DESCRIPTOR_FOURCC));
        assert!(tags.contains(CURSORS_FOURCC));
        assert!(!tags.contains(b"CPKM"));
    }

    #[test]
    fn separator_signature_is_stable() { let image = make_separator([32, 32]).unwrap(); assert_eq!(image.dimensions(), (32, 32)); }

    #[test]
    fn rust_encoder_decoder_round_trip() {
        let root = TempDir::new().unwrap();
        let input = root.path().join("theme");
        let output = root.path().join("theme.cursepack");
        let decoded = root.path().join("decoded");
        fs::create_dir_all(&input).unwrap();
        let mut cursor = RgbaImage::new(32, 32);
        for (x, y, pixel) in cursor.enumerate_pixels_mut() { *pixel = if x == y { Rgba([220, 80, 30, 255]) } else { Rgba([12, 34, 56, 0]) }; }
        write_cur(&input.join("arrow.cur"), &cursor, [3, 4]).unwrap();
        fs::write(input.join("install.inf"), r#"[Scheme.Reg]
HKCU,"Control Panel\Cursors","Arrow",,"%10%\%CUR_DIR%\arrow.cur"
"#).unwrap();
        let meta = encode_theme(&input, &output, ThemeInfo::default()).unwrap();
        assert_eq!(meta.cursor_count, 1);
        let files = decode_theme(&output, &decoded).unwrap();
        assert_eq!(files.len(), 1);
        assert!(decoded.join("pack-info.json").exists());
        assert!(decoded.join("cursors.json").exists());
        let decoded_frame = parse_cur(&fs::read(&files[0]).unwrap()).unwrap();
        assert_eq!(decoded_frame.hotspot, [3, 4]);
        assert_eq!(decoded_frame.image, cursor);
    }
}
