use hidapi::HidApi;
use serde::{Deserialize, Serialize};
use std::fmt;

const REPORT_LENGTH: usize = 62;

/// Identifiers for supported HyperX headsets.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceId {
    CloudIiiWired,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct DeviceMetadata {
    pub id: DeviceId,
    pub label: &'static str,
}

const DEVICE_CATALOG: &[DeviceMetadata] = &[DeviceMetadata {
    id: DeviceId::CloudIiiWired,
    label: "Cloud III (wired)",
}];

#[derive(Debug, Clone, Copy)]
struct FeatureReport {
    report_id: u8,
    selector: u8,
    length: usize,
}

#[derive(Debug, Clone, Copy)]
struct DeviceDescriptor {
    vendor_id: u16,
    product_id: u16,
    sidetone_feature: Option<FeatureReport>,
}

const CLOUD_III_WIRED: DeviceDescriptor = DeviceDescriptor {
    vendor_id: 0x03F0,
    product_id: 0x089D,
    sidetone_feature: Some(FeatureReport {
        report_id: 0x20,
        selector: 0x86,
        length: REPORT_LENGTH,
    }),
};

fn find_descriptor(device_id: DeviceId) -> DeviceDescriptor {
    match device_id {
        DeviceId::CloudIiiWired => CLOUD_III_WIRED,
    }
}

fn validate_feature(
    device_id: DeviceId,
    descriptor: DeviceDescriptor,
) -> Result<FeatureReport, ControlError> {
    descriptor
        .sidetone_feature
        .ok_or(ControlError::UnsupportedFeature { device_id })
}

fn build_feature_payload(report: FeatureReport, enabled: bool) -> Vec<u8> {
    let mut payload = vec![0u8; report.length];
    payload[0] = report.report_id;
    payload[1] = report.selector;
    let value = if enabled { 1u16 } else { 0u16 };
    payload[2] = (value & 0xFF) as u8;
    payload[3] = (value >> 8) as u8;
    payload
}

/// High-level errors returned to the frontend.
#[derive(Debug)]
pub enum ControlError {
    HidInit {
        source: hidapi::HidError,
    },
    DeviceOpen {
        vendor_id: u16,
        product_id: u16,
        source: hidapi::HidError,
    },
    ReportSend {
        report_id: u8,
        selector: u8,
        source: hidapi::HidError,
    },
    UnsupportedFeature {
        device_id: DeviceId,
    },
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ControlError::HidInit { source } => {
                write!(f, "failed to initialise HID API: {source}")
            }
            ControlError::DeviceOpen {
                vendor_id,
                product_id,
                source,
            } => write!(
                f,
                "unable to open device (VID=0x{vendor_id:04X}, PID=0x{product_id:04X}): {source}"
            ),
            ControlError::ReportSend {
                report_id,
                selector,
                source,
            } => write!(
                f,
                "failed to send feature report (id=0x{report_id:02X}, selector=0x{selector:02X}): {source}"
            ),
            ControlError::UnsupportedFeature { device_id } => {
                write!(f, "device {device_id:?} does not support this feature")
            }
        }
    }
}

impl std::error::Error for ControlError {}

/// Return a static list of known HyperX devices.
pub fn supported_devices() -> &'static [DeviceMetadata] {
    DEVICE_CATALOG
}

/// Toggle the sidetone feature for a particular device.
pub fn set_sidetone(device_id: DeviceId, enabled: bool) -> Result<(), ControlError> {
    let descriptor = find_descriptor(device_id);
    let feature = validate_feature(device_id, descriptor)?;

    let api = HidApi::new().map_err(|source| ControlError::HidInit { source })?;
    let device = api
        .open(descriptor.vendor_id, descriptor.product_id)
        .map_err(|source| ControlError::DeviceOpen {
            vendor_id: descriptor.vendor_id,
            product_id: descriptor.product_id,
            source,
        })?;

    let payload = build_feature_payload(feature, enabled);
    device
        .send_feature_report(&payload)
        .map_err(|source| ControlError::ReportSend {
            report_id: feature.report_id,
            selector: feature.selector,
            source,
        })?;

    Ok(())
}
