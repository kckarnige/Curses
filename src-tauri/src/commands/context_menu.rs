
#![cfg(target_os = "windows")]

use serde::Deserialize;

use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{
            AppendMenuW,
            CreatePopupMenu,
            DestroyMenu,
            TrackPopupMenu,
            MF_GRAYED,
            MF_SEPARATOR,
            MF_STRING,
            TPM_LEFTALIGN,
            TPM_NONOTIFY,
            TPM_RETURNCMD,
            TPM_RIGHTBUTTON,
        },
    },
};

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MenuEntry {
    Item {
        id: String,
        label: String,

        #[serde(default = "default_enabled")]
        enabled: bool,
    },
    Separator,
}

fn default_enabled() -> bool {
    true
}

pub unsafe fn show_native_context_menu(
    hwnd: HWND,
    x: i32,
    y: i32,
    items: Vec<MenuEntry>,
) -> windows::core::Result<Option<String>> {
    unsafe {
        let menu = CreatePopupMenu()?;

        let result = (|| -> windows::core::Result<Option<String>> {
            let mut command_map: Vec<(u32, String)> = Vec::new();
            let mut next_id = 1u32;

            for item in &items {
                match item {
                    MenuEntry::Separator => {
                        AppendMenuW(
                            menu,
                            MF_SEPARATOR,
                            0,
                            PCWSTR::null(),
                        )?;
                    }

                    MenuEntry::Item { id, label, enabled } => {
                        let wide: Vec<u16> = label
                            .encode_utf16()
                            .chain(std::iter::once(0))
                            .collect();

                        let flags = if *enabled {
                            MF_STRING
                        } else {
                            MF_STRING | MF_GRAYED
                        };

                        AppendMenuW(
                            menu,
                            flags,
                            next_id as usize,
                            PCWSTR(wide.as_ptr()),
                        )?;

                        command_map.push((next_id, id.clone()));
                        next_id += 1;
                    }
                }
            }

            let selected = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN
                    | TPM_RIGHTBUTTON
                    | TPM_RETURNCMD
                    | TPM_NONOTIFY,
                x,
                y,
                None,
                hwnd,
                None,
            );

            let selected_id = selected.0 as u32;

            Ok(command_map
                .into_iter()
                .find(|(command_id, _)| *command_id == selected_id)
                .map(|(_, id)| id))
        })();

        // Release the native menu, even if building or displaying it failed.
        let _ = DestroyMenu(menu);

        result
    }
}

#[tauri::command]
pub fn show_context_menu(
    window: tauri::WebviewWindow,
    x: i32,
    y: i32,
    items: Vec<MenuEntry>,
) -> Result<Option<String>, String> {
    let native_hwnd = window.hwnd().map_err(|e| e.to_string())?;

    let hwnd = HWND(native_hwnd.0 as _);

    unsafe {
        show_native_context_menu(hwnd, x, y, items)
            .map_err(|e| e.to_string())
    }
}
