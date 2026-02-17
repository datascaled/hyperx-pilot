use crate::hyperx::DeviceId;
use std::fmt;

#[derive(Debug)]
pub enum SystemAudioError {
    #[cfg(not(target_os = "macos"))]
    UnsupportedPlatform,
    RuntimePoisoned,
    MissingHomeDir,
    Io {
        context: &'static str,
        source: std::io::Error,
    },
    CommandFailed {
        context: &'static str,
        details: String,
    },
    DownloadIntegrityMismatch {
        expected_sha256: &'static str,
        actual_sha256: String,
    },
    CoreAudio {
        context: &'static str,
        status: i32,
    },
    DeviceNotFound {
        kind: &'static str,
        query: String,
    },
    Stream {
        context: &'static str,
        details: String,
    },
}

impl fmt::Display for SystemAudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(not(target_os = "macos"))]
            SystemAudioError::UnsupportedPlatform => {
                write!(f, "virtual surround is only available on macOS")
            }
            SystemAudioError::RuntimePoisoned => {
                write!(f, "virtual surround runtime lock was poisoned")
            }
            SystemAudioError::MissingHomeDir => {
                write!(f, "unable to resolve home directory")
            }
            SystemAudioError::Io { context, source } => {
                write!(f, "{context}: {source}")
            }
            SystemAudioError::CommandFailed { context, details } => {
                write!(f, "{context}: {details}")
            }
            SystemAudioError::DownloadIntegrityMismatch {
                expected_sha256,
                actual_sha256,
            } => write!(
                f,
                "downloaded BlackHole package checksum mismatch (expected {expected_sha256}, got {actual_sha256})"
            ),
            SystemAudioError::CoreAudio { context, status } => {
                write!(f, "{context} failed with OSStatus {status}")
            }
            SystemAudioError::DeviceNotFound { kind, query } => {
                write!(f, "unable to find {kind} device matching \"{query}\"")
            }
            SystemAudioError::Stream { context, details } => {
                write!(f, "{context}: {details}")
            }
        }
    }
}

impl std::error::Error for SystemAudioError {}

#[cfg(not(target_os = "macos"))]
pub fn set_virtual_surround(_device_id: DeviceId, _enabled: bool) -> Result<(), SystemAudioError> {
    Err(SystemAudioError::UnsupportedPlatform)
}

#[cfg(not(target_os = "macos"))]
pub fn read_virtual_surround_state(_device_id: DeviceId) -> Result<Option<bool>, SystemAudioError> {
    Err(SystemAudioError::UnsupportedPlatform)
}

#[cfg(target_os = "macos")]
pub fn set_virtual_surround(device_id: DeviceId, enabled: bool) -> Result<(), SystemAudioError> {
    macos::set_virtual_surround(device_id, enabled)
}

#[cfg(target_os = "macos")]
pub fn read_virtual_surround_state(device_id: DeviceId) -> Result<Option<bool>, SystemAudioError> {
    macos::read_virtual_surround_state(device_id)
}

#[cfg(target_os = "macos")]
mod macos {
    use super::SystemAudioError;
    use crate::hyperx::DeviceId;
    use coreaudio_sys::{
        kAudioDevicePropertyDeviceUID, kAudioDevicePropertyScopeOutput,
        kAudioDevicePropertyStreams, kAudioHardwarePropertyDefaultOutputDevice,
        kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
        kAudioObjectPropertyScopeGlobal, kAudioObjectSystemObject, kCFStringEncodingUTF8,
        AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
        AudioObjectPropertyAddress, AudioObjectSetPropertyData, CFRelease, CFStringGetCString,
        CFStringGetLength, CFStringGetMaximumSizeForEncoding, CFStringRef,
    };
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{BufferSize, SampleFormat, SampleRate, Stream, StreamConfig};
    use ringbuf::traits::{Consumer, Producer, Split};
    use ringbuf::HeapRb;
    use std::ffi::c_void;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const BLACKHOLE_DEVICE_NAME: &str = "BlackHole 2ch";
    const BLACKHOLE_VERSION: &str = "0.6.1";
    const BLACKHOLE_URL: &str = "https://existential.audio/downloads/BlackHole2ch-0.6.1.pkg";
    const BLACKHOLE_SHA256: &str =
        "c829afa041a9f6e1b369c01953c8f079740dd1f02421109855829edc0d3c1988";
    const DEBUG_LOG_FILE: &str = "spatial-debug.log";

    struct AudioBridge {
        _input: Stream,
        _output: Stream,
    }

    struct BridgeHandle {
        stop_tx: Sender<()>,
        join_handle: JoinHandle<()>,
    }

    struct RuntimeState {
        previous_default_uid: String,
        bridge: BridgeHandle,
    }

    #[derive(Clone, Debug)]
    struct OutputDevice {
        id: AudioObjectID,
        name: String,
        uid: String,
    }

    static RUNTIME: OnceLock<Mutex<Option<RuntimeState>>> = OnceLock::new();

    fn runtime() -> &'static Mutex<Option<RuntimeState>> {
        RUNTIME.get_or_init(|| Mutex::new(None))
    }

    pub fn set_virtual_surround(
        device_id: DeviceId,
        enabled: bool,
    ) -> Result<(), SystemAudioError> {
        if enabled {
            enable_virtual_surround(device_id)
        } else {
            disable_virtual_surround()
        }
    }

    pub fn read_virtual_surround_state(
        _device_id: DeviceId,
    ) -> Result<Option<bool>, SystemAudioError> {
        let runtime = runtime()
            .lock()
            .map_err(|_| SystemAudioError::RuntimePoisoned)?;
        Ok(Some(runtime.is_some()))
    }

    fn enable_virtual_surround(device_id: DeviceId) -> Result<(), SystemAudioError> {
        {
            let runtime = runtime()
                .lock()
                .map_err(|_| SystemAudioError::RuntimePoisoned)?;
            if runtime.is_some() {
                return Ok(());
            }
        }

        append_debug_log("enable_virtual_surround start");

        ensure_blackhole_installed()?;

        let output_devices = list_output_devices()?;
        let blackhole = output_devices
            .iter()
            .find(|device| equals_ignore_ascii_case(&device.name, BLACKHOLE_DEVICE_NAME))
            .cloned()
            .ok_or_else(|| SystemAudioError::DeviceNotFound {
                kind: "output",
                query: BLACKHOLE_DEVICE_NAME.to_string(),
            })?;

        let previous_default_id = get_default_output_device_id()?;
        let previous_default = output_devices
            .iter()
            .find(|device| device.id == previous_default_id)
            .cloned()
            .ok_or_else(|| SystemAudioError::DeviceNotFound {
                kind: "default output",
                query: previous_default_id.to_string(),
            })?;

        let target_output = choose_target_output(
            &output_devices,
            device_id,
            blackhole.id,
            previous_default.id,
        )?;

        append_debug_log(&format!(
            "enable_virtual_surround target=\"{}\" previous_default=\"{}\" blackhole=\"{}\"",
            target_output.name, previous_default.name, blackhole.name
        ));

        let bridge = start_audio_bridge(&target_output, &blackhole)?;

        if let Err(error) = set_default_output_device(blackhole.id) {
            append_debug_log(
                "enable_virtual_surround failed to set default output, dropping bridge",
            );
            stop_bridge(bridge);
            return Err(error);
        }

        let mut runtime = match runtime().lock() {
            Ok(runtime) => runtime,
            Err(_) => {
                stop_bridge(bridge);
                return Err(SystemAudioError::RuntimePoisoned);
            }
        };
        *runtime = Some(RuntimeState {
            previous_default_uid: previous_default.uid,
            bridge,
        });

        append_debug_log("enable_virtual_surround complete");
        Ok(())
    }

    fn disable_virtual_surround() -> Result<(), SystemAudioError> {
        append_debug_log("disable_virtual_surround start");

        let (previous_uid, bridge) = {
            let mut runtime = runtime()
                .lock()
                .map_err(|_| SystemAudioError::RuntimePoisoned)?;
            let Some(state) = runtime.take() else {
                append_debug_log("disable_virtual_surround noop (already disabled)");
                return Ok(());
            };
            (state.previous_default_uid, state.bridge)
        };

        if let Some(previous_output) = find_output_device_by_uid(&previous_uid)? {
            set_default_output_device(previous_output.id)?;
            append_debug_log(&format!(
                "disable_virtual_surround restored default output to \"{}\"",
                previous_output.name
            ));
        } else {
            append_debug_log(
                "disable_virtual_surround previous default output no longer available",
            );
        }

        stop_bridge(bridge);

        append_debug_log("disable_virtual_surround complete");
        Ok(())
    }

    fn ensure_blackhole_installed() -> Result<(), SystemAudioError> {
        if has_blackhole_device()? {
            return Ok(());
        }

        append_debug_log("BlackHole missing, starting automated installation");

        let downloads_dir = app_support_dir()?.join("downloads");
        fs::create_dir_all(&downloads_dir).map_err(|source| SystemAudioError::Io {
            context: "failed to create downloads directory",
            source,
        })?;

        let pkg_name = format!("BlackHole2ch-{BLACKHOLE_VERSION}.pkg");
        let pkg_path = downloads_dir.join(pkg_name);

        if !pkg_path.is_file() || !verify_sha256(&pkg_path, BLACKHOLE_SHA256)? {
            download_blackhole_pkg(&pkg_path)?;
            let digest = sha256(&pkg_path)?;
            if digest != BLACKHOLE_SHA256 {
                return Err(SystemAudioError::DownloadIntegrityMismatch {
                    expected_sha256: BLACKHOLE_SHA256,
                    actual_sha256: digest,
                });
            }
        }

        run_privileged_shell_command(&format!(
            "installer -pkg {} -target /",
            shell_quote(pkg_path.as_os_str().to_string_lossy().as_ref())
        ))?;

        // The upstream installer requests a reboot, but reloading coreaudiod is enough for most systems.
        let _ = run_privileged_shell_command("killall -9 coreaudiod || true");

        for _ in 0..30 {
            if has_blackhole_device()? {
                append_debug_log("BlackHole installation detected");
                return Ok(());
            }
            thread::sleep(Duration::from_secs(1));
        }

        Err(SystemAudioError::DeviceNotFound {
            kind: "output",
            query: BLACKHOLE_DEVICE_NAME.to_string(),
        })
    }

    fn choose_target_output(
        output_devices: &[OutputDevice],
        device_id: DeviceId,
        blackhole_id: AudioObjectID,
        previous_default_id: AudioObjectID,
    ) -> Result<OutputDevice, SystemAudioError> {
        let preferred_fragment = match device_id {
            DeviceId::CloudIiiWired => "hyperx cloud iii",
        };

        if let Some(device) = output_devices
            .iter()
            .find(|device| {
                device.id != blackhole_id
                    && device
                        .name
                        .to_ascii_lowercase()
                        .contains(preferred_fragment)
            })
            .cloned()
        {
            return Ok(device);
        }

        if let Some(device) = output_devices
            .iter()
            .find(|device| device.id == previous_default_id && device.id != blackhole_id)
            .cloned()
        {
            return Ok(device);
        }

        output_devices
            .iter()
            .find(|device| device.id != blackhole_id)
            .cloned()
            .ok_or_else(|| SystemAudioError::DeviceNotFound {
                kind: "output",
                query: "non-BlackHole output".to_string(),
            })
    }

    fn start_audio_bridge(
        target_output: &OutputDevice,
        blackhole: &OutputDevice,
    ) -> Result<BridgeHandle, SystemAudioError> {
        let target_output_name = target_output.name.clone();
        let blackhole_name = blackhole.name.clone();

        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        let join_handle = thread::spawn(move || {
            match create_audio_bridge(&target_output_name, &blackhole_name) {
                Ok(_bridge) => {
                    let _ = ready_tx.send(Ok(()));
                    while stop_rx.recv_timeout(Duration::from_millis(250)).is_err() {}
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                }
            }
        });

        match ready_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(BridgeHandle {
                stop_tx,
                join_handle,
            }),
            Ok(Err(details)) => {
                let _ = stop_tx.send(());
                let _ = join_handle.join();
                Err(SystemAudioError::Stream {
                    context: "failed to initialize realtime surround bridge",
                    details,
                })
            }
            Err(_) => {
                let _ = stop_tx.send(());
                let _ = join_handle.join();
                Err(SystemAudioError::Stream {
                    context: "timed out while starting realtime surround bridge",
                    details: "bridge initialization did not signal readiness".to_string(),
                })
            }
        }
    }

    fn stop_bridge(bridge: BridgeHandle) {
        let BridgeHandle {
            stop_tx,
            join_handle,
        } = bridge;
        let _ = stop_tx.send(());
        let _ = join_handle.join();
    }

    fn create_audio_bridge(
        target_output_name: &str,
        blackhole_name: &str,
    ) -> Result<AudioBridge, SystemAudioError> {
        let host = cpal::default_host();

        let blackhole_input = host
            .input_devices()
            .map_err(|error| SystemAudioError::Stream {
                context: "failed to list input devices",
                details: error.to_string(),
            })?
            .find(|device| {
                device
                    .name()
                    .ok()
                    .map(|name| equals_ignore_ascii_case(&name, blackhole_name))
                    .unwrap_or(false)
            })
            .ok_or_else(|| SystemAudioError::DeviceNotFound {
                kind: "input",
                query: blackhole_name.to_string(),
            })?;

        let target_output_device = host
            .output_devices()
            .map_err(|error| SystemAudioError::Stream {
                context: "failed to list output devices",
                details: error.to_string(),
            })?
            .find(|device| {
                device
                    .name()
                    .ok()
                    .map(|name| equals_ignore_ascii_case(&name, target_output_name))
                    .unwrap_or(false)
            })
            .ok_or_else(|| SystemAudioError::DeviceNotFound {
                kind: "output",
                query: target_output_name.to_string(),
            })?;

        let default_input =
            blackhole_input
                .default_input_config()
                .map_err(|error| SystemAudioError::Stream {
                    context: "failed to read BlackHole input config",
                    details: error.to_string(),
                })?;
        let default_output = target_output_device
            .default_output_config()
            .map_err(|error| SystemAudioError::Stream {
                context: "failed to read output device config",
                details: error.to_string(),
            })?;

        let (sample_rate, input_format, input_config, output_format, output_config) =
            pick_stream_configuration(
                &blackhole_input,
                &target_output_device,
                default_input.sample_rate().0,
                default_output.sample_rate().0,
            )?;

        append_debug_log(&format!(
            "start_audio_bridge sample_rate={} input_format={:?} output_format={:?}",
            sample_rate, input_format, output_format
        ));

        let rb = HeapRb::<f32>::new((sample_rate as usize) * 8);
        let (producer, consumer) = rb.split();
        let producer = Arc::new(Mutex::new(producer));
        let consumer = Arc::new(Mutex::new(consumer));

        let input_channels = input_config.channels as usize;
        let output_channels = output_config.channels as usize;

        let input_consumer = producer.clone();
        let input_stream = blackhole_input
            .build_input_stream_raw(
                &input_config,
                input_format,
                move |data, _| {
                    if let Ok(mut producer) = input_consumer.lock() {
                        match data.sample_format() {
                            SampleFormat::F32 => {
                                if let Some(samples) = data.as_slice::<f32>() {
                                    for frame in samples.chunks(input_channels) {
                                        let left = frame[0];
                                        let right = *frame.get(1).unwrap_or(&left);
                                        let _ = producer.try_push(left);
                                        let _ = producer.try_push(right);
                                    }
                                }
                            }
                            SampleFormat::I16 => {
                                if let Some(samples) = data.as_slice::<i16>() {
                                    for frame in samples.chunks(input_channels) {
                                        let left = frame[0] as f32 / i16::MAX as f32;
                                        let right = frame.get(1).copied().unwrap_or(frame[0])
                                            as f32
                                            / i16::MAX as f32;
                                        let _ = producer.try_push(left);
                                        let _ = producer.try_push(right);
                                    }
                                }
                            }
                            SampleFormat::U16 => {
                                if let Some(samples) = data.as_slice::<u16>() {
                                    for frame in samples.chunks(input_channels) {
                                        let left = u16_to_f32(frame[0]);
                                        let right =
                                            u16_to_f32(frame.get(1).copied().unwrap_or(frame[0]));
                                        let _ = producer.try_push(left);
                                        let _ = producer.try_push(right);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                },
                move |error| {
                    append_debug_log(&format!("input stream error: {error}"));
                },
                None,
            )
            .map_err(|error| SystemAudioError::Stream {
                context: "failed to build input stream",
                details: error.to_string(),
            })?;

        let output_consumer = consumer.clone();
        let mut processor = SurroundProcessor::new(sample_rate);
        let output_stream = target_output_device
            .build_output_stream_raw(
                &output_config,
                output_format,
                move |data, _| {
                    if let Ok(mut consumer) = output_consumer.lock() {
                        match data.sample_format() {
                            SampleFormat::F32 => {
                                if let Some(samples) = data.as_slice_mut::<f32>() {
                                    for frame in samples.chunks_mut(output_channels) {
                                        let (left, right) = next_stereo_sample(&mut *consumer);
                                        let (left, right) = processor.process(left, right);
                                        frame[0] = left;
                                        if output_channels > 1 {
                                            frame[1] = right;
                                        }
                                        for channel in frame.iter_mut().skip(2) {
                                            *channel = (left + right) * 0.5;
                                        }
                                    }
                                }
                            }
                            SampleFormat::I16 => {
                                if let Some(samples) = data.as_slice_mut::<i16>() {
                                    for frame in samples.chunks_mut(output_channels) {
                                        let (left, right) = next_stereo_sample(&mut *consumer);
                                        let (left, right) = processor.process(left, right);
                                        frame[0] = f32_to_i16(left);
                                        if output_channels > 1 {
                                            frame[1] = f32_to_i16(right);
                                        }
                                        for channel in frame.iter_mut().skip(2) {
                                            *channel = f32_to_i16((left + right) * 0.5);
                                        }
                                    }
                                }
                            }
                            SampleFormat::U16 => {
                                if let Some(samples) = data.as_slice_mut::<u16>() {
                                    for frame in samples.chunks_mut(output_channels) {
                                        let (left, right) = next_stereo_sample(&mut *consumer);
                                        let (left, right) = processor.process(left, right);
                                        frame[0] = f32_to_u16(left);
                                        if output_channels > 1 {
                                            frame[1] = f32_to_u16(right);
                                        }
                                        for channel in frame.iter_mut().skip(2) {
                                            *channel = f32_to_u16((left + right) * 0.5);
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                },
                move |error| {
                    append_debug_log(&format!("output stream error: {error}"));
                },
                None,
            )
            .map_err(|error| SystemAudioError::Stream {
                context: "failed to build output stream",
                details: error.to_string(),
            })?;

        input_stream
            .play()
            .map_err(|error| SystemAudioError::Stream {
                context: "failed to start input stream",
                details: error.to_string(),
            })?;
        output_stream
            .play()
            .map_err(|error| SystemAudioError::Stream {
                context: "failed to start output stream",
                details: error.to_string(),
            })?;

        Ok(AudioBridge {
            _input: input_stream,
            _output: output_stream,
        })
    }

    fn pick_stream_configuration(
        input_device: &cpal::Device,
        output_device: &cpal::Device,
        input_default_rate: u32,
        output_default_rate: u32,
    ) -> Result<(u32, SampleFormat, StreamConfig, SampleFormat, StreamConfig), SystemAudioError>
    {
        let mut candidates = vec![
            48_000u32,
            44_100u32,
            output_default_rate,
            input_default_rate,
        ];
        candidates.sort_unstable();
        candidates.dedup();
        candidates.reverse();

        for sample_rate in candidates {
            let input = find_input_stream_config(input_device, sample_rate)?;
            let output = find_output_stream_config(output_device, sample_rate)?;
            if let (Some((input_format, input_config)), Some((output_format, output_config))) =
                (input, output)
            {
                return Ok((
                    sample_rate,
                    input_format,
                    input_config,
                    output_format,
                    output_config,
                ));
            }
        }

        Err(SystemAudioError::Stream {
            context:
                "no common sample-rate/channel format between BlackHole input and output device",
            details: "unable to build realtime bridge".to_string(),
        })
    }

    fn find_input_stream_config(
        device: &cpal::Device,
        sample_rate: u32,
    ) -> Result<Option<(SampleFormat, StreamConfig)>, SystemAudioError> {
        let ranges =
            device
                .supported_input_configs()
                .map_err(|error| SystemAudioError::Stream {
                    context: "failed to query supported input configs",
                    details: error.to_string(),
                })?;

        for range in ranges {
            if range.channels() < 2 {
                continue;
            }
            if sample_rate < range.min_sample_rate().0 || sample_rate > range.max_sample_rate().0 {
                continue;
            }
            let format = range.sample_format();
            let config = StreamConfig {
                channels: 2,
                sample_rate: SampleRate(sample_rate),
                buffer_size: BufferSize::Default,
            };
            return Ok(Some((format, config)));
        }
        Ok(None)
    }

    fn find_output_stream_config(
        device: &cpal::Device,
        sample_rate: u32,
    ) -> Result<Option<(SampleFormat, StreamConfig)>, SystemAudioError> {
        let ranges =
            device
                .supported_output_configs()
                .map_err(|error| SystemAudioError::Stream {
                    context: "failed to query supported output configs",
                    details: error.to_string(),
                })?;

        for range in ranges {
            if range.channels() < 2 {
                continue;
            }
            if sample_rate < range.min_sample_rate().0 || sample_rate > range.max_sample_rate().0 {
                continue;
            }
            let format = range.sample_format();
            let config = StreamConfig {
                channels: 2,
                sample_rate: SampleRate(sample_rate),
                buffer_size: BufferSize::Default,
            };
            return Ok(Some((format, config)));
        }
        Ok(None)
    }

    fn next_stereo_sample<C: Consumer<Item = f32>>(consumer: &mut C) -> (f32, f32) {
        let left = consumer.try_pop().unwrap_or(0.0);
        let right = consumer.try_pop().unwrap_or(0.0);
        (left, right)
    }

    fn has_blackhole_device() -> Result<bool, SystemAudioError> {
        Ok(list_output_devices()?
            .iter()
            .any(|device| equals_ignore_ascii_case(&device.name, BLACKHOLE_DEVICE_NAME)))
    }

    fn find_output_device_by_uid(uid: &str) -> Result<Option<OutputDevice>, SystemAudioError> {
        Ok(list_output_devices()?
            .into_iter()
            .find(|device| device.uid == uid))
    }

    fn get_default_output_device_id() -> Result<AudioObjectID, SystemAudioError> {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let mut device_id: AudioObjectID = 0;
        let mut data_size = std::mem::size_of::<AudioObjectID>() as u32;

        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                std::ptr::null(),
                &mut data_size,
                (&mut device_id as *mut AudioObjectID).cast::<c_void>(),
            )
        };

        if status != 0 {
            return Err(SystemAudioError::CoreAudio {
                context: "reading default output device",
                status,
            });
        }

        Ok(device_id)
    }

    fn set_default_output_device(device_id: AudioObjectID) -> Result<(), SystemAudioError> {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDefaultOutputDevice,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let status = unsafe {
            AudioObjectSetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                std::ptr::null(),
                std::mem::size_of::<AudioObjectID>() as u32,
                (&device_id as *const AudioObjectID).cast::<c_void>(),
            )
        };

        if status != 0 {
            return Err(SystemAudioError::CoreAudio {
                context: "setting default output device",
                status,
            });
        }

        Ok(())
    }

    fn list_output_devices() -> Result<Vec<OutputDevice>, SystemAudioError> {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioHardwarePropertyDevices,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        };

        let mut data_size: u32 = 0;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(
                kAudioObjectSystemObject,
                &address,
                0,
                std::ptr::null(),
                &mut data_size,
            )
        };
        if status != 0 {
            return Err(SystemAudioError::CoreAudio {
                context: "querying output device list size",
                status,
            });
        }

        let count = data_size as usize / std::mem::size_of::<AudioObjectID>();
        let mut device_ids = vec![0u32; count];
        let status = unsafe {
            AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &address,
                0,
                std::ptr::null(),
                &mut data_size,
                device_ids.as_mut_ptr().cast::<c_void>(),
            )
        };
        if status != 0 {
            return Err(SystemAudioError::CoreAudio {
                context: "querying output device list",
                status,
            });
        }

        let mut devices = Vec::new();
        for device_id in device_ids {
            if !device_has_output_streams(device_id)? {
                continue;
            }
            let name = get_device_string_property(
                device_id,
                kAudioObjectPropertyName,
                kAudioObjectPropertyScopeGlobal,
            )?;
            let uid = get_device_string_property(
                device_id,
                kAudioDevicePropertyDeviceUID,
                kAudioObjectPropertyScopeGlobal,
            )?;
            devices.push(OutputDevice {
                id: device_id,
                name,
                uid,
            });
        }

        Ok(devices)
    }

    fn device_has_output_streams(device_id: AudioObjectID) -> Result<bool, SystemAudioError> {
        let address = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreams,
            mScope: kAudioDevicePropertyScopeOutput,
            mElement: kAudioObjectPropertyElementMain,
        };

        let mut data_size: u32 = 0;
        let status = unsafe {
            AudioObjectGetPropertyDataSize(device_id, &address, 0, std::ptr::null(), &mut data_size)
        };
        if status != 0 {
            return Err(SystemAudioError::CoreAudio {
                context: "querying output stream list size",
                status,
            });
        }

        Ok(data_size > 0)
    }

    fn get_device_string_property(
        device_id: AudioObjectID,
        selector: u32,
        scope: u32,
    ) -> Result<String, SystemAudioError> {
        let address = AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
        };

        let mut value: CFStringRef = std::ptr::null();
        let mut data_size = std::mem::size_of::<CFStringRef>() as u32;
        let status = unsafe {
            AudioObjectGetPropertyData(
                device_id,
                &address,
                0,
                std::ptr::null(),
                &mut data_size,
                (&mut value as *mut CFStringRef).cast::<c_void>(),
            )
        };
        if status != 0 {
            return Err(SystemAudioError::CoreAudio {
                context: "reading CoreAudio string property",
                status,
            });
        }
        if value.is_null() {
            return Ok(String::new());
        }

        let string = cf_string_to_string(value)?;
        unsafe {
            CFRelease(value.cast::<c_void>());
        }
        Ok(string)
    }

    fn cf_string_to_string(value: CFStringRef) -> Result<String, SystemAudioError> {
        let length = unsafe { CFStringGetLength(value) };
        let max_size = unsafe { CFStringGetMaximumSizeForEncoding(length, kCFStringEncodingUTF8) };
        if max_size <= 0 {
            return Ok(String::new());
        }

        let mut buffer = vec![0i8; max_size as usize + 1];
        let success = unsafe {
            CFStringGetCString(
                value,
                buffer.as_mut_ptr(),
                buffer.len() as i64,
                kCFStringEncodingUTF8,
            )
        };
        if success == 0 {
            return Err(SystemAudioError::CommandFailed {
                context: "converting CoreAudio device name",
                details: "CFStringGetCString returned false".to_string(),
            });
        }

        let nul = buffer
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(buffer.len());
        let bytes: Vec<u8> = buffer[..nul].iter().map(|byte| *byte as u8).collect();
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn download_blackhole_pkg(path: &Path) -> Result<(), SystemAudioError> {
        append_debug_log(&format!(
            "downloading BlackHole package from {BLACKHOLE_URL}"
        ));

        let output = Command::new("/usr/bin/curl")
            .args([
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--output",
            ])
            .arg(path)
            .arg(BLACKHOLE_URL)
            .output()
            .map_err(|source| SystemAudioError::Io {
                context: "failed to execute curl",
                source,
            })?;

        if !output.status.success() {
            return Err(SystemAudioError::CommandFailed {
                context: "failed to download BlackHole package",
                details: command_error_details(&output),
            });
        }

        Ok(())
    }

    fn verify_sha256(path: &Path, expected_sha256: &'static str) -> Result<bool, SystemAudioError> {
        let digest = sha256(path)?;
        Ok(digest == expected_sha256)
    }

    fn sha256(path: &Path) -> Result<String, SystemAudioError> {
        let output = Command::new("/usr/bin/shasum")
            .args(["-a", "256"])
            .arg(path)
            .output()
            .map_err(|source| SystemAudioError::Io {
                context: "failed to execute shasum",
                source,
            })?;

        if !output.status.success() {
            return Err(SystemAudioError::CommandFailed {
                context: "failed to compute SHA-256 digest",
                details: command_error_details(&output),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let digest = stdout
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        Ok(digest)
    }

    fn run_privileged_shell_command(command: &str) -> Result<(), SystemAudioError> {
        append_debug_log(&format!("running privileged shell command: {command}"));
        let output = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg("on run argv")
            .arg("-e")
            .arg("do shell script (item 1 of argv) with administrator privileges")
            .arg("-e")
            .arg("end run")
            .arg(command)
            .output()
            .map_err(|source| SystemAudioError::Io {
                context: "failed to execute osascript",
                source,
            })?;

        if !output.status.success() {
            return Err(SystemAudioError::CommandFailed {
                context: "privileged command failed",
                details: command_error_details(&output),
            });
        }
        Ok(())
    }

    fn command_error_details(output: &std::process::Output) -> String {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !stderr.is_empty() {
            format!("exit status {}; stderr: {stderr}", output.status)
        } else if !stdout.is_empty() {
            format!("exit status {}; stdout: {stdout}", output.status)
        } else {
            format!("exit status {}", output.status)
        }
    }

    fn app_support_dir() -> Result<PathBuf, SystemAudioError> {
        let home = std::env::var_os("HOME").ok_or(SystemAudioError::MissingHomeDir)?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("hyperx-pilot"))
    }

    fn debug_log_path() -> Result<PathBuf, SystemAudioError> {
        let home = std::env::var_os("HOME").ok_or(SystemAudioError::MissingHomeDir)?;
        Ok(PathBuf::from(home)
            .join("Library")
            .join("Logs")
            .join("hyperx-pilot")
            .join(DEBUG_LOG_FILE))
    }

    fn append_debug_log(message: &str) {
        let _ = append_debug_log_inner(message);
    }

    fn append_debug_log_inner(message: &str) -> Result<(), SystemAudioError> {
        let path = debug_log_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| SystemAudioError::Io {
                context: "failed to create debug log directory",
                source,
            })?;
        }

        let mut file = File::options()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| SystemAudioError::Io {
                context: "failed to open debug log file",
                source,
            })?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::from_millis(0))
            .as_millis();
        writeln!(file, "[{timestamp}] {message}").map_err(|source| SystemAudioError::Io {
            context: "failed to write debug log",
            source,
        })?;
        Ok(())
    }

    fn equals_ignore_ascii_case(left: &str, right: &str) -> bool {
        left.trim().eq_ignore_ascii_case(right.trim())
    }

    fn shell_quote(value: &str) -> String {
        let escaped = value.replace('\'', "'\"'\"'");
        format!("'{escaped}'")
    }

    fn clamp_unit(sample: f32) -> f32 {
        sample.clamp(-1.0, 1.0)
    }

    fn f32_to_i16(sample: f32) -> i16 {
        (clamp_unit(sample) * i16::MAX as f32).round() as i16
    }

    fn f32_to_u16(sample: f32) -> u16 {
        (((clamp_unit(sample) + 1.0) * 0.5) * u16::MAX as f32).round() as u16
    }

    fn u16_to_f32(sample: u16) -> f32 {
        (sample as f32 / u16::MAX as f32) * 2.0 - 1.0
    }

    struct SurroundProcessor {
        rear_delay_left: Vec<f32>,
        rear_delay_right: Vec<f32>,
        side_delay_left: Vec<f32>,
        side_delay_right: Vec<f32>,
        cross_delay_left: Vec<f32>,
        cross_delay_right: Vec<f32>,
        rear_index: usize,
        side_index: usize,
        cross_index: usize,
        cross_lp_left: f32,
        cross_lp_right: f32,
        cross_lp_alpha: f32,
        wet: f32,
        output_gain: f32,
    }

    impl SurroundProcessor {
        fn new(sample_rate: u32) -> Self {
            let rear_samples = ((sample_rate as f32) * 0.008).round().max(1.0) as usize;
            let side_samples = ((sample_rate as f32) * 0.0032).round().max(1.0) as usize;
            let cross_samples = ((sample_rate as f32) * 0.00045).round().max(1.0) as usize;

            // Low-pass around ~1.2kHz for opposite-ear crossfeed so it resembles head shadowing.
            let cutoff_hz = 1_200.0f32;
            let dt = 1.0 / sample_rate as f32;
            let rc = 1.0 / (2.0 * std::f32::consts::PI * cutoff_hz);
            let cross_lp_alpha = (dt / (rc + dt)).clamp(0.01, 0.9);

            Self {
                rear_delay_left: vec![0.0; rear_samples],
                rear_delay_right: vec![0.0; rear_samples],
                side_delay_left: vec![0.0; side_samples],
                side_delay_right: vec![0.0; side_samples],
                cross_delay_left: vec![0.0; cross_samples],
                cross_delay_right: vec![0.0; cross_samples],
                rear_index: 0,
                side_index: 0,
                cross_index: 0,
                cross_lp_left: 0.0,
                cross_lp_right: 0.0,
                cross_lp_alpha,
                wet: 0.96,
                output_gain: 0.86,
            }
        }

        fn process(&mut self, input_left: f32, input_right: f32) -> (f32, f32) {
            let mid = (input_left + input_right) * 0.5;
            let side = (input_left - input_right) * 0.5;

            // Build virtual speaker feeds from stereo source.
            let center = mid;
            let side_src_left = input_left * 0.62 + side * 0.48;
            let side_src_right = input_right * 0.62 - side * 0.48;
            let rear_src_left = side * 1.1 + input_left * 0.18 - input_right * 0.08;
            let rear_src_right = -side * 1.1 + input_right * 0.18 - input_left * 0.08;

            let rear_left = self.rear_delay_left[self.rear_index];
            let rear_right = self.rear_delay_right[self.rear_index];
            let side_left = self.side_delay_left[self.side_index];
            let side_right = self.side_delay_right[self.side_index];
            let cross_left = self.cross_delay_left[self.cross_index];
            let cross_right = self.cross_delay_right[self.cross_index];

            self.rear_delay_left[self.rear_index] = rear_src_left;
            self.rear_delay_right[self.rear_index] = rear_src_right;
            self.side_delay_left[self.side_index] = side_src_left;
            self.side_delay_right[self.side_index] = side_src_right;
            self.cross_delay_left[self.cross_index] = input_left;
            self.cross_delay_right[self.cross_index] = input_right;

            self.rear_index = (self.rear_index + 1) % self.rear_delay_left.len();
            self.side_index = (self.side_index + 1) % self.side_delay_left.len();
            self.cross_index = (self.cross_index + 1) % self.cross_delay_left.len();

            // Frequency-shaped opposite-ear feed for stronger externalization.
            self.cross_lp_left += self.cross_lp_alpha * (cross_right - self.cross_lp_left);
            self.cross_lp_right += self.cross_lp_alpha * (cross_left - self.cross_lp_right);

            let virtual_left = input_left * 0.92
                + center * 0.34
                + side_left * 0.78
                + rear_left * 0.95
                + self.cross_lp_left * 0.36
                - rear_right * 0.16;
            let virtual_right = input_right * 0.92
                + center * 0.34
                + side_right * 0.78
                + rear_right * 0.95
                + self.cross_lp_right * 0.36
                - rear_left * 0.16;

            let mut out_left = input_left * (1.0 - self.wet) + virtual_left * self.wet;
            let mut out_right = input_right * (1.0 - self.wet) + virtual_right * self.wet;

            // Simple peak guard so the stronger matrix does not clip hard.
            let peak = out_left.abs().max(out_right.abs());
            if peak > 1.0 {
                let scale = 1.0 / peak;
                out_left *= scale;
                out_right *= scale;
            }

            (
                clamp_unit(out_left * self.output_gain),
                clamp_unit(out_right * self.output_gain),
            )
        }
    }
}
