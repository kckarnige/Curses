use std::{collections::HashMap, fs, path::Path};

use oxideav_ico::{read_ani, read_ico, select_largest};
use tauri::Manager;

#[cfg(target_os = "windows")]
use window_vibrancy::apply_mica;

#[cfg(target_os = "windows")]
#[tauri::command]
fn apply_cursor_theme(
    app: tauri::AppHandle,
    theme: String,
    cursors: HashMap<String, String>,
) -> Result<(), String> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};

    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_SETCURSORS, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    const VALID_ROLES: &[&str] = &[
        "Arrow",
        "Help",
        "AppStarting",
        "Wait",
        "Crosshair",
        "IBeam",
        "NWPen",
        "No",
        "SizeNS",
        "SizeWE",
        "SizeNWSE",
        "SizeNESW",
        "SizeAll",
        "UpArrow",
        "Hand",
        "Person",
        "Pin",
    ];

    let theme_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("cursors")
        .join(&theme)
        .canonicalize()
        .map_err(|e| e.to_string())?;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);

    let (cursor_key, _) = hkcu
        .create_subkey(r"Control Panel\Cursors")
        .map_err(|e| e.to_string())?;

    for (role, filename) in cursors {
        // Don't allow arbitrary registry values from an INF.
        if !VALID_ROLES.contains(&role.as_str()) {
            continue;
        }

        let cursor_path = theme_dir
            .join(filename)
            .canonicalize()
            .map_err(|e| e.to_string())?;

        // Protect against things like ../../some-file.
        if !cursor_path.starts_with(&theme_dir) {
            return Err("Cursor file points outside the theme directory".into());
        }

        let extension = cursor_path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        if extension != "cur" && extension != "ani" {
            return Err(format!(
                "Unsupported cursor file: {}",
                cursor_path.display()
            ));
        }

        cursor_key
            .set_value(&role, &cursor_path.to_string_lossy().to_string())
            .map_err(|e| e.to_string())?;
    }

    // Tell Windows to reload all system cursors.
    unsafe {
        SystemParametersInfoW(
            SPI_SETCURSORS,
            0,
            None,
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(())
}

// Read cursor role -> filename mappings from a cursor theme's install.inf.
#[tauri::command]
fn read_cursor_inf(path: String) -> Result<HashMap<String, String>, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;

    let mut strings = HashMap::new();
    let mut cursors = HashMap::new();

    // Pass 1: collect all variables from [Strings].
    let mut section = "";

    for raw_line in text.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len() - 1];
            continue;
        }

        if section.eq_ignore_ascii_case("Strings") {
            if let Some((key, value)) = line.split_once('=') {
                strings.insert(
                    key.trim().to_ascii_lowercase(),
                    value.trim().trim_matches('"').to_string(),
                );
            }
        }
    }

    // Pass 2: support both common cursor INF formats.
    section = "";

    for raw_line in text.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            section = &line[1..line.len() - 1];
            continue;
        }

        // Format 1:
        //
        // [Wreg]
        // HKCU,"Control Panel\Cursors",Arrow,...,"%pointer%"
        if section.eq_ignore_ascii_case("Wreg") {
            let parts: Vec<&str> = line.split(',').collect();

            if parts.len() < 5 {
                continue;
            }

            let role = parts[2].trim().trim_matches('"');

            let value = parts[4].trim().trim_matches('"');

            if role.is_empty() {
                continue;
            }

            if let Some(filename) = resolve_cursor_filename(value, &strings) {
                cursors.insert(role.to_string(), filename);
            }
        }

        // Format 2:
        //
        // [Scheme.Reg]
        // HKCU,"Control Panel\Cursors\Schemes",...
        //
        // The cursor roles are stored as one ordered comma-separated list.
        if section.eq_ignore_ascii_case("Scheme.Reg")
            && line.contains(r"Control Panel\Cursors\Schemes")
        {
            let Some((_, scheme)) = line.split_once(",,") else {
                continue;
            };

            let scheme = scheme.trim().trim_matches('"');

            const ROLES: [&str; 15] = [
                "Arrow",
                "Help",
                "AppStarting",
                "Wait",
                "Crosshair",
                "IBeam",
                "NWPen",
                "No",
                "SizeNS",
                "SizeWE",
                "SizeNWSE",
                "SizeNESW",
                "SizeAll",
                "UpArrow",
                "Hand",
            ];

            for (role, value) in ROLES.iter().zip(scheme.split(',')) {
                if let Some(filename) = resolve_cursor_filename(value, &strings) {
                    // Don't overwrite a more explicit Wreg entry.
                    cursors.entry((*role).to_string()).or_insert(filename);
                }
            }
        }
    }

    Ok(cursors)
}

#[tauri::command]
fn list_cursor_themes(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let cursor_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("cursors");

    let mut themes = Vec::new();

    for entry in fs::read_dir(cursor_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;

        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            themes.push(entry.file_name().to_string_lossy().into_owned());
        }
    }

    themes.sort();

    Ok(themes)
}

fn resolve_cursor_filename(value: &str, strings: &HashMap<String, String>) -> Option<String> {
    let value = value.trim().trim_matches('"');

    // First check whether the value already ends in a literal
    // .cur or .ani filename.
    if let Some(filename) = value.rsplit(['\\', '/']).next() {
        let lower = filename.to_ascii_lowercase();

        if lower.ends_with(".cur") || lower.ends_with(".ani") {
            return Some(filename.to_string());
        }
    }

    // Otherwise try resolving a %variable%, e.g. %pointer%.
    if let Some(end) = value.rfind('%') {
        if let Some(start) = value[..end].rfind('%') {
            let key = value[start + 1..end].to_ascii_lowercase();

            if let Some(filename) = strings.get(&key) {
                return Some(filename.clone());
            }
        }
    }

    None
}

// Convert CUR -> PNG or ANI -> APNG for WebView previews.
#[tauri::command]
fn cursor_preview(path: String) -> Result<Vec<u8>, String> {
    let bytes = fs::read(&path).map_err(|e| e.to_string())?;

    match Path::new(&path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "cur" => cur_to_png(&bytes),
        "ani" => ani_to_apng(&bytes),
        ext => Err(format!("Unsupported cursor format: {ext}")),
    }
}

fn cur_to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let (_, images) = read_ico(bytes).map_err(|e| e.to_string())?;

    let image = &images[select_largest(&images).ok_or("CUR contains no images")?];

    encode_png(image.width, image.height, &image.pixels)
}

fn ani_to_apng(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let ani = read_ani(bytes).map_err(|e| e.to_string())?;

    let first_step = ani.steps.first().ok_or("ANI contains no frames")?;

    let first_frame = &ani.frames[first_step.frame_index as usize];

    let first_image = &first_frame.images
        [select_largest(&first_frame.images).ok_or("ANI frame contains no images")?];

    let width = first_image.width;
    let height = first_image.height;

    let mut output = Vec::new();

    {
        let mut encoder = png::Encoder::new(&mut output, width, height);

        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        encoder
            .set_animated(ani.steps.len() as u32, 0)
            .map_err(|e| e.to_string())?;

        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;

        for step in &ani.steps {
            let frame = &ani.frames[step.frame_index as usize];

            let image = frame
                .images
                .iter()
                .find(|img| img.width == width && img.height == height)
                .ok_or("ANI frame resolution mismatch")?;

            let delay = u16::try_from(step.jiffies).map_err(|_| "ANI frame delay is too large")?;

            writer
                .set_frame_delay(delay, 60)
                .map_err(|e| e.to_string())?;

            writer
                .write_image_data(&image.pixels)
                .map_err(|e| e.to_string())?;
        }

        writer.finish().map_err(|e| e.to_string())?;
    }

    Ok(output)
}

fn encode_png(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();

    {
        let mut encoder = png::Encoder::new(&mut output, width, height);

        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);

        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;

        writer.write_image_data(pixels).map_err(|e| e.to_string())?;

        writer.finish().map_err(|e| e.to_string())?;
    }

    Ok(output)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // create_dir_all also creates the parent app-data directory.
            let cursor_dir = app.path().app_data_dir()?.join("cursors");
            fs::create_dir_all(&cursor_dir)?;

            println!("Cursor folder: {}", cursor_dir.display());

            #[cfg(target_os = "windows")]
            if let Some(window) = app.get_webview_window("main") {
                // Mica is cosmetic, so don't crash the app if it can't be applied.
                if let Err(error) = apply_mica(&window, None) {
                    eprintln!("Failed to apply Mica: {error}");
                }
            }

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            cursor_preview,
            read_cursor_inf,
            apply_cursor_theme,
            list_cursor_themes,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
