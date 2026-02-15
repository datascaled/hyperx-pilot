# AGENTS.md

## Purpose
HyperX Pilot is a Tauri + Vue desktop app for controlling sidetone on HyperX headsets without NGenuity.
The current focus is one device: `Cloud III (wired)`.

## Stack
- Frontend: Vue 3 + TypeScript (`src/`)
- Backend: Tauri 2 + Rust + `hidapi` (`src-tauri/`)
- i18n: `vue-i18n` with `src/locales/de.json` and `src/locales/en.json`

## Architecture
- `src/App.vue`
  - UI state, device list, sidetone toggle, locale selection
  - Polls connected headsets periodically (currently every 3 seconds)
  - Calls Tauri commands via `invoke(...)`
- `src-tauri/src/lib.rs`
  - Tauri command bridge (`list_hyperx_devices`, `set_sidetone`, `get_sidetone_state`)
- `src-tauri/src/hyperx.rs`
  - HID logic (device detection, feature report read/write)
  - Current known report parameters for Cloud III wired:
    - VID `0x03F0`, PID `0x089D`
    - Sidetone feature report: ID `0x20`, selector `0x86`, length `62`

## Persistence (local in Browser/WebView storage)
Settings are stored in `localStorage` under `hyperx:settings`.

Schema (simplified):
```json
{
  "version": 1,
  "locale": "de|en",
  "selectedDeviceId": "cloud_iii_wired|null",
  "devices": {
    "cloud_iii_wired": { "sidetuneEnabled": true }
  }
}
```

Notes:
- `hyperx:locale` remains as a legacy key for migration/compatibility.
- Device settings are currently stored per `device.id` (not per physical serial number).

## Expected Runtime Behavior
1. App startup:
- Load persisted settings
- Detect available supported headsets from the HID device list
- Restore last selection/preferences
- Read actual sidetone state from the device

2. Device not plugged in:
- No device selection possible
- Show hint text in the UI
- Sidetone UI remains disabled

3. Device plugged in:
- Device is detected automatically
- Persisted device preferences are applied
- UI and hardware state are synchronized

## Agent Rules For This Codebase
1. Update `src-tauri/src/hyperx.rs` first when HID behavior or device support changes.
2. Update `src-tauri/src/lib.rs` only for command signatures/exposure.
3. Keep `src/App.vue` as the single source for UI state and persistence flow.
4. Every new user-facing UI message must be added to both `de.json` and `en.json`.
5. If a key is renamed, update all references in `src/App.vue` and the i18n files.
6. After changes, always run both checks:
   - `npm run build`
   - `cargo check` (in `src-tauri/`)

## Definition Of Done For Changes
- Frontend build succeeds (`npm run build`)
- Rust check succeeds (`cargo check`)
- No new unused warnings introduced by the change
- Behavior for "headset plugged in / unplugged" was considered manually

## Current Limits
- Only Cloud III wired is defined.
- Persistence is currently per device type ID, not per individual headset.
