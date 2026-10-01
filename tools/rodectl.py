#!/usr/bin/env python3
"""Reference implementation of the RØDE NT-USB+ HID control protocol.

Standard library only; talks to the mic through /dev/hidraw*.
See docs/PROTOCOL.md for the wire format.

    rodectl.py status                 read everything the mic reports
    rodectl.py get <name>             e.g. hpf, monitor, monitor-mix
    rodectl.py set <name> <value>     e.g. set hpf 75 / set compressor.threshold -20
    rodectl.py save                   persist current settings on the mic
    rodectl.py raw <report> <hex...>  send one raw report (4 or 8 only)
"""
from __future__ import annotations

import argparse
import bisect
import json
import math
import os
import select
import struct
import sys
import time
from pathlib import Path

VID, PID = 0x19F7, 0x0035

# Output report -> (payload length, input report carrying the reply)
REPORT_PARAM = 0x08      # 27-byte payload, reply on 0x07
REPORT_APHEX = 0x04      # 28-byte payload, reply on 0x03
REPORTS = {REPORT_PARAM: (27, 0x07), REPORT_APHEX: (28, 0x03)}
ACK = 0x41  # 'A'
STATUS = {0x45: "invalid value ('E')", 0x4E: "not supported ('N')"}

# Report 8 parameter ids
P_HPF, P_WINDSCREEN, P_MONITOR, P_MONITOR_MIX, P_BLINK, P_PING, P_SAVE = 1, 2, 3, 4, 5, 6, 10

# Report 4 effect ids and operations
FX_COMPRESSOR, FX_GATE, FX_EXCITER, FX_BIG_BOTTOM = 0, 1, 2, 3
OP_SET_ALL, OP_GET_ALL, OP_SET, OP_GET = 0, 1, 2, 3

SAMPLE_RATE = 48000.0
Q31 = 2147483648.0

TABLES = json.loads((Path(__file__).with_name("aphex_tables.json")).read_text())


class ProtocolError(RuntimeError):
    pass


def find_hidraw() -> str:
    want = f"HID_ID=0003:{VID:08X}:{PID:08X}"
    for dev in sorted(Path("/sys/class/hidraw").glob("hidraw*")):
        try:
            if want in (dev / "device" / "uevent").read_text():
                return f"/dev/{dev.name}"
        except OSError:
            continue
    raise SystemExit("RØDE NT-USB+ not found (is it plugged in?)")


class NtUsbPlus:
    def __init__(self, path: str | None = None, verbose: bool = False):
        self.path = path or find_hidraw()
        self.verbose = verbose
        try:
            self.fd = os.open(self.path, os.O_RDWR | os.O_NONBLOCK)
        except PermissionError:
            raise SystemExit(
                f"no permission for {self.path} - install udev/70-rode-nt-usb-plus.rules (see README)")

    def close(self):
        os.close(self.fd)

    def _drain(self):
        while select.select([self.fd], [], [], 0)[0]:
            try:
                os.read(self.fd, 64)
            except BlockingIOError:
                break

    def transfer(self, report: int, payload: bytes, timeout: float = 2.0) -> bytes:
        """Send one output report; return the reply payload (without report id)."""
        length, reply_id = REPORTS[report]
        if len(payload) > length:
            raise ValueError("payload too long")
        out = bytes([report]) + payload.ljust(length, b"\0")
        self._drain()
        if self.verbose:
            print(f"  > {out.hex(' ')}", file=sys.stderr)
        os.write(self.fd, out)
        deadline = time.monotonic() + timeout
        while (left := deadline - time.monotonic()) > 0:
            if not select.select([self.fd], [], [], left)[0]:
                break
            data = os.read(self.fd, 64)
            if self.verbose:
                print(f"  < {data.hex(' ')}", file=sys.stderr)
            # reply echoes the first payload byte right after the report id
            if len(data) >= 3 and data[0] == reply_id and data[1] == payload[0]:
                if data[2] != ACK:
                    why = STATUS.get(data[2], f"status {data[2]:#04x}")
                    raise ProtocolError(f"device refused {out[:5].hex(' ')}: {why}")
                return data[3:]
        raise ProtocolError(f"no reply to report {report:#04x} {payload[:4].hex(' ')}")

    # --- report 8: simple parameters -------------------------------------
    def get_param(self, pid: int) -> int:
        return self.transfer(REPORT_PARAM, bytes([pid, 1]))[0]

    def set_param(self, pid: int, value: int) -> None:
        self.transfer(REPORT_PARAM, bytes([pid, 0, value & 0xFF]))

    # --- report 4: Aphex DSP ---------------------------------------------
    def fx_get(self, fx: int, param: int) -> bytes:
        return self.transfer(REPORT_APHEX, bytes([fx, OP_GET, param]))

    def fx_set(self, fx: int, param: int, value: bytes) -> None:
        self.transfer(REPORT_APHEX, bytes([fx, OP_SET, param]) + value)


# --- value conversions -----------------------------------------------------

def _clamp(x, lo, hi):
    return max(lo, min(hi, x))


def _idx(norm: float) -> int:
    return int(_clamp(norm, 0.0, 1.0) * 255.0)


def _q31(x: float) -> int:
    # RØDE Central holds the value as a single-precision float before scaling;
    # rounding the same way reproduces the mic's factory values bit for bit.
    x = struct.unpack("<f", struct.pack("<f", x))[0]
    return int(_clamp(x * Q31, 0, 0x7FFFFFFF))


def _table_index(table: list[int], value: int) -> int:
    """Index of the table entry closest to value (tables are monotonic)."""
    if table[0] <= table[-1]:
        i = bisect.bisect_left(table, value)
    else:
        rev = table[::-1]
        i = 255 - bisect.bisect_left(rev, value)
        i = _clamp(i, 0, 255)
        cands = [j for j in (i - 1, i, i + 1) if 0 <= j <= 255]
        return min(cands, key=lambda j: abs(table[j] - value))
    cands = [j for j in (i - 1, i) if 0 <= j <= 255]
    return min(cands, key=lambda j: abs(table[j] - value))


def _log_norm(x, lo, hi):
    return math.log10(x / lo) / math.log10(hi / lo)


def _log_denorm(n, lo, hi):
    return lo * 10 ** (math.log10(hi / lo) * n)


def _pole_from_ms(ms: float) -> float:
    c = math.cos(5.0 / (ms / 1000.0) / SAMPLE_RATE)
    return c - 1.0 + math.sqrt(c * c - 4.0 * c + 3.0)


def _ms_from_pole(a: float) -> float:
    if a <= 0.0:
        return float("inf")
    y = a * a / (2.0 * (1.0 - a)) if a < 1.0 else 2.0
    w = math.acos(_clamp(1.0 - y, -1.0, 1.0))
    return 5.0 / (w * SAMPLE_RATE) * 1000.0 if w else float("inf")


def _db(v: int) -> float:
    return 20.0 * math.log10(v / Q31) if v > 0 else -math.inf


class Field:
    """One DSP parameter: how to pack a user value and unpack the reply."""

    def __init__(self, param, unit, lo, hi, enc, dec):
        self.param, self.unit, self.lo, self.hi, self.enc, self.dec = param, unit, lo, hi, enc, dec


def _i32(v):
    return struct.pack("<i", v)


def _u8(b):
    return b[0]


def _le32(b):
    return struct.unpack_from("<i", b)[0]


T = TABLES
BOOL = Field(0, "bool", 0, 1, lambda v: bytes([1 if v else 0]), lambda b: bool(b[0]))

EFFECTS: dict[str, tuple[int, dict[str, Field]]] = {
    "compressor": (FX_COMPRESSOR, {
        "enabled": BOOL,
        "threshold": Field(1, "dB", -60, 0,
                           lambda v: _i32(T["compressor_threshold"][_idx(-v / 60.0)]),
                           lambda b: -60.0 * _table_index(T["compressor_threshold"], _le32(b)) / 255.0),
        "ratio": Field(2, ":1", 1.5, 4.5,
                       lambda v: bytes([_idx((v - 1.5) / 3.0)]),
                       lambda b: 1.5 + b[0] / 255.0 * 3.0),
        "attack": Field(3, "ms", 0.1, 10,
                        lambda v: _i32(T["compressor_attack"][_idx(_log_norm(v, 0.1, 10.0))]),
                        lambda b: _log_denorm(_table_index(T["compressor_attack"], _le32(b)) / 255.0, 0.1, 10.0)),
        "release": Field(4, "ms", 5, 200,
                         lambda v: _i32(T["compressor_release"][_idx(_log_norm(v, 5.0, 200.0))]),
                         lambda b: _log_denorm(_table_index(T["compressor_release"], _le32(b)) / 255.0, 5.0, 200.0)),
        "gain": Field(5, "dB", 0, 9,
                      lambda v: _i32(T["compressor_makeup_gain"][_idx(v / 9.0)]),
                      lambda b: 9.0 * _table_index(T["compressor_makeup_gain"], _le32(b)) / 255.0),
    }),
    "gate": (FX_GATE, {
        "enabled": BOOL,
        "threshold": Field(1, "dB", -100, 0, lambda v: _i32(_q31(10 ** (v / 20.0))), lambda b: _db(_le32(b))),
        "attack": Field(2, "ms", 0.1, 1000, lambda v: _i32(_q31(_pole_from_ms(v))),
                        lambda b: _ms_from_pole(_le32(b) / Q31)),
        "hold": Field(3, "s", 0.0001, 10, lambda v: _i32(_q31(1.0 / (v * SAMPLE_RATE))),
                      lambda b: 1.0 / (_le32(b) / Q31 * SAMPLE_RATE) if _le32(b) > 0 else 0.0),
        "release": Field(4, "s", 0.0001, 10, lambda v: _i32(_q31(1.0 / (v * SAMPLE_RATE))),
                         lambda b: 1.0 / (_le32(b) / Q31 * SAMPLE_RATE) if _le32(b) > 0 else 0.0),
        "range": Field(5, "dB", -100, 0, lambda v: _i32(_q31(10 ** (v / 20.0))), lambda b: _db(_le32(b))),
        "hysteresis": Field(6, "dB", -8, -1, lambda v: _i32(_q31(10 ** (v / 20.0))), lambda b: _db(_le32(b))),
    }),
    "exciter": (FX_EXCITER, {
        "enabled": BOOL,
        "mix": Field(1, "%", 0, 100,
                     lambda v: _i32(T["log"][_idx(v / 100.0)]) + bytes([_idx(v / 100.0)]),
                     lambda b: b[4] / 255.0 * 100.0),
        "tune": Field(2, "Hz", 600, 5000,
                      lambda v: (_i32(T["hf_filter_freq"][_idx((v - 600.0) / 4400.0)])
                                 + _i32(T["hf_filter_gain"][_idx((v - 600.0) / 4400.0)])
                                 + bytes([_idx((v - 600.0) / 4400.0)])),
                      lambda b: 600.0 + b[8] / 255.0 * 4400.0),
    }),
    "bigbottom": (FX_BIG_BOTTOM, {
        "enabled": BOOL,
        "drive": Field(1, "%", 0, 100,
                       lambda v: _i32(T["log"][_idx(v / 100.0)]) + bytes([_idx(v / 100.0)]),
                       lambda b: b[4] / 255.0 * 100.0),
        "tune": Field(2, "Hz", 60, 312,
                      lambda v: bytes([_idx((v - 60.0) / 252.0)]),
                      lambda b: 60.0 + b[0] / 255.0 * 252.0),
    }),
}

HPF_NAMES = {0: "off", 1: "75", 2: "150"}
SIMPLE = {"hpf": P_HPF, "monitor": P_MONITOR, "monitor-mix": P_MONITOR_MIX}


def cmd_status(mic: NtUsbPlus, raw: bool):
    hpf = mic.get_param(P_HPF)
    print(f"hpf           {HPF_NAMES.get(hpf, hpf)}")
    print(f"monitor       {'on' if mic.get_param(P_MONITOR) == 1 else 'off'}")
    print(f"monitor-mix   {mic.get_param(P_MONITOR_MIX)}")
    for name, (fx, fields) in EFFECTS.items():
        print(f"{name}:")
        for fname, f in fields.items():
            data = mic.fx_get(fx, f.param)
            value = f.dec(data)
            if f.unit == "bool":
                shown = "on" if value else "off"
            elif fx != FX_GATE and not any(data[:9]):
                shown = "(never set)"  # factory state reads back as all zeros
            else:
                shown = f"{value:.3f} {f.unit}"
            extra = f"   [{data[:9].hex(' ')}]" if raw else ""
            print(f"  {fname:11s} {shown}{extra}")


def parse_bool(s: str) -> bool:
    if s.lower() in ("1", "on", "true", "yes"):
        return True
    if s.lower() in ("0", "off", "false", "no"):
        return False
    raise SystemExit(f"expected on/off, got {s!r}")


def cmd_get(mic: NtUsbPlus, name: str):
    if name in SIMPLE:
        print(mic.get_param(SIMPLE[name]))
        return
    fxname, _, fname = name.partition(".")
    fx, fields = EFFECTS[fxname]
    print(fields[fname].dec(mic.fx_get(fx, fields[fname].param)))


def cmd_set(mic: NtUsbPlus, name: str, value: str):
    if name == "hpf":
        rev = {v: k for k, v in HPF_NAMES.items()}
        if value not in rev:
            raise SystemExit("hpf must be off, 75 or 150")
        mic.set_param(P_HPF, rev[value])
    elif name == "monitor":
        mic.set_param(P_MONITOR, int(parse_bool(value)))
    elif name == "monitor-mix":
        if not 0 <= int(value) <= 100:
            raise SystemExit("monitor-mix must be within 0..100")
        mic.set_param(P_MONITOR_MIX, int(value))
    elif name == "blink":
        mic.set_param(P_BLINK, int(parse_bool(value)))
    else:
        fxname, _, fname = name.partition(".")
        if fxname not in EFFECTS or fname not in EFFECTS[fxname][1]:
            raise SystemExit(f"unknown setting {name!r}")
        fx, fields = EFFECTS[fxname]
        f = fields[fname]
        if f.unit == "bool":
            v = parse_bool(value)
        else:
            v = float(value)
            if not f.lo <= v <= f.hi:
                raise SystemExit(f"{name} must be within {f.lo}..{f.hi} {f.unit}")
        mic.fx_set(fx, f.param, f.enc(v))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("-d", "--device", help="hidraw path (auto-detected by default)")
    ap.add_argument("-v", "--verbose", action="store_true", help="print raw reports")
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("status")
    s.add_argument("--raw", action="store_true", help="also show reply bytes")
    g = sub.add_parser("get")
    g.add_argument("name")
    st = sub.add_parser("set")
    st.add_argument("name")
    st.add_argument("value")
    sub.add_parser("save")
    r = sub.add_parser("raw")
    r.add_argument("report", type=lambda x: int(x, 0), choices=sorted(REPORTS))
    r.add_argument("hex", nargs="+")
    args = ap.parse_args()

    mic = NtUsbPlus(args.device, args.verbose)
    try:
        if args.cmd == "status":
            cmd_status(mic, args.raw)
        elif args.cmd == "get":
            cmd_get(mic, args.name)
        elif args.cmd == "set":
            cmd_set(mic, args.name, args.value)
        elif args.cmd == "save":
            # the flash write takes about 1.4 s
            mic.transfer(REPORT_PARAM, bytes([P_SAVE, 0, 0]), timeout=5.0)
        elif args.cmd == "raw":
            print(mic.transfer(args.report, bytes.fromhex("".join(args.hex))).hex(" "))
    except ProtocolError as e:
        raise SystemExit(str(e))
    finally:
        mic.close()


if __name__ == "__main__":
    main()
