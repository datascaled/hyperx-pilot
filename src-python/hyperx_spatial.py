#!/usr/bin/env python3
"""
HyperX Cloud III (USB) — Spatial Sound (macOS)
----------------------------------------------

Zweck
-----
Schaltet „Spatial Sound“ EIN/AUS, indem der HID **Feature-Report 0x52** (162 Bytes) an das Headset gesendet wird.
Die ON/OFF-Payloads stammen direkt aus deinem PCAP (Windows; zuerst EIN, danach AUS).

Technische Details
------------------
- **Report-ID**: 0x52  (Feature Report)
- **Länge**: 162 Bytes
- **wIndex (Interface)**: 0
- **bmRequestType/bRequest** (falls über libusb): 0x21 / 0x09 (SET_REPORT)
- Daten beginnen mit der Report-ID (0x52), danach folgt eine strukturierte Konfiguration.
  In deinem Mitschnitt wechseln an mehreren Offsets 16‑Bit‑Werte zwischen `00 ff` und `ff 00`.
  Ein besonders „früher/später“ schaltender Indikator ist das Paar an Offsets **3/4**.

Verwendung
----------
  python3 hyperx_spatial.py on
  python3 hyperx_spatial.py off
  # Optional: nur das Flag patchen (statt komplette 162-Byte-Snapshot zu schicken):
  python3 hyperx_spatial.py on  --patch
  python3 hyperx_spatial.py off --patch

Abhängigkeiten
--------------
Variante A (empfohlen): hidapi
    brew install hidapi
    pip3 install hidapi
Variante B (Fallback): PyUSB/libusb
    brew install libusb
    pip3 install pyusb

Hinweise
--------
- Standardmäßig sendet das Skript die **vollständigen** 162‑Byte‑Payloads für ON/OFF (snapshot aus PCAP).
  Mit `--patch` versucht es stattdessen, den aktuellen 0x52‑Report zu lesen und **nur** die Bytes an
  **Offset 3/4** umzuschalten (`00 ff`=ON, `ff 00`=OFF). Falls das Lesen fehlschlägt, fällt es auf
  die Snapshot-Methode zurück.
- VID/PID (aus ioreg): VID=0x03F0 (HP, Inc.), PID=0x089D (HyperX Cloud III).
- Das Senden von Feature-Reports über hidapi benötigt keine root-Rechte; PyUSB/libusb ggf. schon.
"""
import sys

VID = 0x03F0  # HP, Inc.
PID = 0x089D  # HyperX Cloud III
HID_REPORT_ID = 0x52
HID_REPORT_LEN = 162
HID_IFACE = 0  # wIndex für 0x52 laut PCAP

# PCAP-extrahierte Snapshots:
ON_SNAPSHOT  = bytes(""" + str(on_payload_hex) + r""")
OFF_SNAPSHOT = bytes(""" + str(off_payload_hex) + r""")

def _hidapi_send_feature(report: bytes) -> None:
    import hid
    d = hid.device()
    d.open(VID, PID)
    try:
        sent = d.send_feature_report(report)
        if sent != len(report):
            raise RuntimeError(f"hidapi: sent {sent}/{len(report)} bytes")
    finally:
        d.close()

def _hidapi_get_feature(report_id: int, length: int) -> bytes:
    import hid
    d = hid.device()
    d.open(VID, PID)
    try:
        return bytes(d.get_feature_report(report_id, length))
    finally:
        d.close()

def _pyusb_send_feature(report: bytes) -> None:
    import usb.core, usb.util
    dev = usb.core.find(idVendor=VID, idProduct=PID)
    if dev is None:
        raise RuntimeError("PyUSB: device not found")
    # Manche Backends benötigen kein Detach; wenn es fehlschlägt, weiter versuchen
    try:
        if dev.is_kernel_driver_active(HID_IFACE):
            try:
                dev.detach_kernel_driver(HID_IFACE)
            except Exception:
                pass
    except Exception:
        pass
    bmRequestType = 0x21  # HostToDevice | Class | Interface
    bRequest = 0x09       # SET_REPORT
    wValue = (0x03 << 8) | HID_REPORT_ID  # Feature(3) << 8 | ReportID
    wIndex = HID_IFACE
    sent = dev.ctrl_transfer(bmRequestType, bRequest, wValue, wIndex, report, timeout=3000)
    if sent != len(report):
        raise RuntimeError(f"PyUSB: sent {sent}/{len(report)} bytes")

def send_feature(report: bytes) -> None:
    # Bevorzugt hidapi, fallback PyUSB
    try:
        import hid  # noqa: F401
        _hidapi_send_feature(report)
        return
    except Exception as e:
        print(f"[!] hidapi send failed ({e}); falling back to PyUSB")
        _pyusb_send_feature(report)

def read_current_52() -> bytes:
    try:
        import hid  # noqa: F401
        cur = _hidapi_get_feature(HID_REPORT_ID, HID_REPORT_LEN)
        if len(cur) >= HID_REPORT_LEN and cur[0] == HID_REPORT_ID:
            return cur[:HID_REPORT_LEN]
    except Exception as e:
        print(f"[!] hidapi get_feature failed ({e})")
    return b""

def set_spatial(state: str, patch_only: bool) -> None:
    state = state.lower()
    if state not in ("on", "off"):
        raise ValueError("state must be 'on' or 'off'")
    if patch_only:
        cur = read_current_52()
        if len(cur) != HID_REPORT_LEN or cur[0] != HID_REPORT_ID:
            print("[*] Could not read current report or invalid length; using snapshot method instead.")
            patch_only = False
        else:
            # Flip nur an Offsets 3/4: 00 ff -> ON, ff 00 -> OFF
            buf = bytearray(cur)
            if state == "on":
                buf[3], buf[4] = 0x00, 0xff
            else:
                buf[3], buf[4] = 0xff, 0x00
            print(f"[*] Patching offsets 3/4 to {'00 ff' if state=='on' else 'ff 00'} and sending…")
            send_feature(bytes(buf))
            return
    # Snapshot-Fallback
    report = ON_SNAPSHOT if state == "on" else OFF_SNAPSHOT
    if report[0] != HID_REPORT_ID or len(report) != HID_REPORT_LEN:
        raise RuntimeError("Internal snapshot length/ID mismatch")
    print(f"[*] Sending full 0x52 snapshot for spatial {state.upper()} ({len(report)} bytes)…")
    send_feature(report)

def main(argv):
    if len(argv) < 2 or argv[1].lower() not in ("on","off"):
        print("Usage: python3 hyperx_spatial.py [on|off] [--patch]")
        return 1
    patch_only = ("--patch" in argv[2:])
    set_spatial(argv[1], patch_only)
    print("Done.")
    return 0

if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
