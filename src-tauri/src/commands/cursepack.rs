use cursepack::{decode_theme, encode_theme, inspect, ThemeInfo};
use std::path::PathBuf;

#[tauri::command]
pub fn inspect_cursepack(path: String) -> Result<cursepack::Metadata, String> {
    inspect(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn decode_cursepack(
    path: String,
    output_dir: String,
) -> Result<Vec<String>, String> {
    decode_theme(path, output_dir)
        .map(|files| {
            files.into_iter()
                .map(|file| file.to_string_lossy().into_owned())
                .collect()
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn encode_cursepack(
    input_dir: String,
    output_file: String,
    name: String,
    author: String,
    author_url: String,
    theme_url: String,
    description: String,
    license: String,
) -> Result<cursepack::Metadata, String> {
    let info = ThemeInfo {
        name,
        author,
        author_url,
        theme_url,
        description,
        license,
    };

    encode_theme(
        PathBuf::from(input_dir),
        PathBuf::from(output_file),
        info,
    )
    .map_err(|e| e.to_string())
}