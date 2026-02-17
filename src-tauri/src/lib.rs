mod hyperx;
mod system_audio;

use hyperx::{
    connected_devices as hyperx_connected_devices,
    read_sidetone_state as hyperx_read_sidetone_state, set_sidetone as hyperx_set_sidetone,
    DeviceId, DeviceMetadata,
};
use system_audio::{
    read_virtual_surround_state as system_audio_read_virtual_surround_state,
    set_virtual_surround as system_audio_set_virtual_surround,
};

#[tauri::command]
fn list_hyperx_devices() -> Result<Vec<DeviceMetadata>, String> {
    hyperx_connected_devices().map_err(|err| err.to_string())
}

#[tauri::command]
fn set_sidetone(device_id: DeviceId, enabled: bool) -> Result<(), String> {
    hyperx_set_sidetone(device_id, enabled).map_err(|err| err.to_string())
}

#[tauri::command]
fn get_sidetone_state(device_id: DeviceId) -> Result<Option<bool>, String> {
    hyperx_read_sidetone_state(device_id).map_err(|err| err.to_string())
}

#[tauri::command]
fn set_virtual_surround(device_id: DeviceId, enabled: bool) -> Result<(), String> {
    system_audio_set_virtual_surround(device_id, enabled).map_err(|err| err.to_string())
}

#[tauri::command]
fn get_virtual_surround_state(device_id: DeviceId) -> Result<Option<bool>, String> {
    system_audio_read_virtual_surround_state(device_id).map_err(|err| err.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_hyperx_devices,
            set_sidetone,
            get_sidetone_state,
            set_virtual_surround,
            get_virtual_surround_state
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
