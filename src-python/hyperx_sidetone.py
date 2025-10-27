#!/usr/bin/env python3
"""
HyperX Cloud III (USB) — Sidetone on/off (macOS)
------------------------------------------------

This script replicates the USB HID control transfer observed in your PCAP:
- Host-to-device, HID SET_REPORT:
  bmRequestType = 0x21
  bRequest      = 0x09 (SET_REPORT)
  wValue        = 0x0320  (Feature report, ID = 0x20)
  wIndex        = 0x0003  (Interface 3)
  wLength       = 62
  Data (62 bytes): ReportID(0x20), then parameter 0x86 with value 0x0001 (enable) or 0x0000 (disable), rest zeros.
    Example (enable): 20 86 01 00 00 00 00 00 00 ... (total 62 bytes)
    Example (disable): 20 86 00 00 00 00 00 00 00 ... (total 62 bytes)

Device identifiers from your macOS ioreg output:
- Vendor ID (HP, Inc.): 0x03F0
- Product ID (HyperX Cloud III): 0x089D (decimal 2205)
- HID feature interface: #3

USAGE:
  python3 hyperx_sidetone.py on
  python3 hyperx_sidetone.py off

Dependencies (one of the following):
  1) hidapi (preferred):   pip install hidapi
     - On macOS you may need: brew install hidapi
  2) PyUSB (fallback):     pip install pyusb
     - And: brew install libusb

Notes:
- hidapi does not require detaching kernel drivers on macOS and is usually simpler for HID feature reports.
- PyUSB fallback uses a control transfer (0x21, 0x09, wValue=0x0320, wIndex=3, length=62). It may require elevated privileges.
- Tested by reproducing the report seen in the provided PCAP.
"""
import sys
import time

VID = 0x03F0  # HP, Inc.
PID = 0x089D  # HyperX Cloud III
HID_REPORT_ID = 0x20
HID_REPORT_LEN = 62
HID_IFACE = 3  # from the capture (wIndex)

def build_feature_payload(enabled: bool) -> bytes:
    """
    Build a 62-byte feature report payload for Report ID 0x20.
    Byte layout (as derived from the capture):
      [0]   = 0x20  (Report ID)
      [1]   = 0x86  (parameter selector for sidetone)
      [2:4] = value (uint16 little-endian): 0x0001 = enable, 0x0000 = disable
      [4:]  = zero padding to total length 62
    """
    val = 1 if enabled else 0
    buf = bytearray(HID_REPORT_LEN)
    buf[0] = HID_REPORT_ID
    buf[1] = 0x86
    buf[2] = val & 0xFF
    buf[3] = (val >> 8) & 0xFF
    # remainder already zero
    return bytes(buf)

def _set_with_hidapi(enabled: bool) -> None:
    import hid  # pip install hidapi
    payload = build_feature_payload(enabled)
    d = hid.device()
    d.open(VID, PID)
    try:
        # Send Feature Report (includes Report ID as first byte)
        sent = d.send_feature_report(payload)
        if sent != len(payload):
            raise RuntimeError(f"hidapi: sent {sent} of {len(payload)} bytes")
        # Optional: read back the report to verify (some devices support it)
        try:
            ret = d.get_feature_report(HID_REPORT_ID, HID_REPORT_LEN)
            # ret[0] should be report ID (0x20)
            if len(ret) >= 4 and ret[0] == HID_REPORT_ID and ret[1] == 0x86:
                state = ret[2] | (ret[3] << 8)
                print(f"[hidapi] Read-back reports sidetone={'ON' if state else 'OFF'}")
            else:
                print("[hidapi] Feature read-back not conclusive (device may not echo 0x86 selector).")
        except Exception as e:
            print(f"[hidapi] Feature read-back failed (non-fatal): {e}")
    finally:
        d.close()

def _set_with_pyusb(enabled: bool) -> None:
    import usb.core  # pip install pyusb
    import usb.util

    dev = usb.core.find(idVendor=VID, idProduct=PID)
    if dev is None:
        raise RuntimeError("PyUSB: device not found")
    # On macOS, claiming HID interface can fail if the OS driver has it.
    # Many devices accept control transfers without detaching; if this fails,
    # try running with sudo, or use hidapi instead.
    try:
        if dev.is_kernel_driver_active(HID_IFACE):
            try:
                dev.detach_kernel_driver(HID_IFACE)
            except Exception:
                pass
    except Exception:
        # Not all backends support is_kernel_driver_active on macOS.
        pass

    payload = build_feature_payload(enabled)
    # bmRequestType=0x21 (HostToDevice|Class|Interface), bRequest=0x09 (SET_REPORT)
    # wValue=0x0320 (Feature, ReportID=0x20), wIndex=interface (3), data=62 bytes
    bmRequestType = 0x21
    bRequest = 0x09
    wValue = (0x03 << 8) | HID_REPORT_ID  # feature report type (3) << 8 | report id
    wIndex = HID_IFACE

    sent = dev.ctrl_transfer(bmRequestType, bRequest, wValue, wIndex, payload, timeout=3000)
    if sent != len(payload):
        raise RuntimeError(f"PyUSB: sent {sent} of {len(payload)} bytes")

def set_sidetone(enabled: bool) -> None:
    """
    Try hidapi first (cleaner for HID on macOS), then fall back to PyUSB.
    """
    try:
        import hid  # noqa: F401
        print("[*] Using hidapi backend")
        _set_with_hidapi(enabled)
        return
    except Exception as e:
        print(f"[!] hidapi not available or failed: {e}")
        print("[*] Falling back to PyUSB backend")
        _set_with_pyusb(enabled)

def main(argv):
    if len(argv) != 2 or argv[1].lower() not in ("on", "off"):
        print("Usage: python3 hyperx_sidetone.py [on|off]")
        return 1
    enabled = (argv[1].lower() == "on")
    print(f"Setting sidetone {'ON' if enabled else 'OFF'} for HyperX Cloud III (VID=0x{VID:04X}, PID=0x{PID:04X})")
    set_sidetone(enabled)
    print("Done.")
    return 0

if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
