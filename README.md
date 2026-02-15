# HyperX Pilot

HyperX Pilot is a lightweight Tauri + Vue desktop companion for HyperX headsets. The initial release targets the HyperX Cloud III (wired) headset and focuses on a single task: toggling sidetone on and off without having to install HyperX's NGenuity suite. Support for additional models will be added as their HID behaviour is documented.

## Installation

Navigate to releases and download the latest one.

## What It Does
- lists compatible HyperX headsets and lets you pick the active device
- reads the current sidetone setting via HID feature reports
- flips the sidetone state with a single switch in the UI
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

## Limitations & Roadmap
- Currently tested with the HyperX Cloud III (wired); other models will be added once their HID traffic is captured and validated.
- No persistence of custom levels—just on/off control as exposed through the HID report.
- Contributions for additional devices are welcome as long as they rely on the documented HID interface.
