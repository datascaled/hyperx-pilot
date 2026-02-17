# HyperX Pilot

HyperX Pilot is a lightweight Tauri + Vue desktop companion for HyperX headsets. The initial release targets the HyperX Cloud III (wired) and focuses on sidetone control plus a macOS-only virtual surround output mode without requiring HyperX NGenuity.

## Installation

Navigate to releases and download the latest one.

## What It Does
- lists compatible HyperX headsets and lets you pick the active device
- reads the current sidetone setting via HID feature reports
- flips the sidetone state with a single switch in the UI
- can toggle a macOS virtual surround output mode from inside the app
- runs fully offline with a modern, minimal interface

## HID-First Approach
The app talks exclusively to the publicly exposed USB HID interface of the headset using `hidapi`. The report IDs, selectors, and payload format were learned by passively capturing the HID traffic that NGenuity sends when the sidetone toggle is used. We did **not** perform any firmware or binary reverse engineering—only traffic observation of already available HID messages.

## Getting Started
1. Install dependencies
   ```bash
   npm install
   ```
2. Start the app in development mode
   ```bash
   npm run tauri dev
   ```
3. Build a release bundle
   ```bash
   npm run tauri build
   ```

You need a USB-connected HyperX Cloud III (wired) headset for the HID interaction to succeed. On Linux you may have to grant your user permission to access HID devices (e.g. via `udev` rules).

## Project Structure
- `src/`: Vue 3 front-end with i18n support for English and German.
- `src-tauri/`: Rust backend that exposes Tauri commands and issues HID feature reports.
- `src-tauri/src/hyperx.rs`: central logic for locating devices and reading/writing sidetone state.
- `src-tauri/src/system_audio.rs`: macOS virtual-surround runtime (driver provisioning, routing, realtime processing).

## Virtual Surround Setup (System Audio)
The virtual surround toggle does **not** use headset HID. It currently targets **macOS only**.

On first activation, the app:
- downloads `BlackHole2ch-0.6.1.pkg` from the official source
- verifies the package checksum
- requests admin privileges to install the driver
- restarts CoreAudio and enables the realtime surround bridge

When enabled:
- default system output is switched to `BlackHole 2ch`
- audio is processed in realtime and played back on the selected headset output

When disabled:
- the previous default output device is restored

If your system does not expose BlackHole immediately after installation, a reboot may still be required once.

## Limitations & Roadmap
- Currently tested with the HyperX Cloud III (wired); other models will be added once their HID traffic is captured and validated.
- User preferences are persisted locally (language, selected headset, sidetone preference per supported device id).
- Contributions for additional devices are welcome as long as they rely on the documented HID interface.
