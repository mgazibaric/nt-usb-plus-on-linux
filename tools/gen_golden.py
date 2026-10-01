#!/usr/bin/env python3
"""Write test vectors for the Rust encoders from the reference implementation.

    tools/gen_golden.py > src-tauri/src/testdata/golden.json

The Rust side uses milliseconds for gate hold/release and a positive dB offset
for hysteresis; rodectl.py uses seconds and a negative dB value.
"""
import json

import rodectl as r

# (rust effect, rust field) -> (python field, rust value -> python value, lo, hi) in rust units
FIELDS = {
    ("compressor", "threshold"): ("threshold", lambda v: v, -60, 0),
    ("compressor", "ratio"): ("ratio", lambda v: v, 1.5, 4.5),
    ("compressor", "attack"): ("attack", lambda v: v, 0.1, 10),
    ("compressor", "release"): ("release", lambda v: v, 5, 200),
    ("compressor", "gain"): ("gain", lambda v: v, 0, 9),
    ("gate", "threshold"): ("threshold", lambda v: v, -96, 0),
    ("gate", "attack"): ("attack", lambda v: v, 0.1, 1000),
    ("gate", "hold"): ("hold", lambda v: v / 1000.0, 1, 5000),
    ("gate", "release"): ("release", lambda v: v / 1000.0, 1, 5000),
    ("gate", "range"): ("range", lambda v: v, -96, 0),
    ("gate", "hysteresis"): ("hysteresis", lambda v: -v, 1, 8),
    ("exciter", "mix"): ("mix", lambda v: v, 0, 100),
    ("exciter", "tune"): ("tune", lambda v: v, 600, 5000),
    ("bigbottom", "drive"): ("drive", lambda v: v, 0, 100),
    ("bigbottom", "tune"): ("tune", lambda v: v, 60, 312),
}
STEPS = 16

out = []
for (effect, field), (pyfield, conv, lo, hi) in FIELDS.items():
    f = r.EFFECTS[effect][1][pyfield]
    for i in range(STEPS + 1):
        value = round(lo + (hi - lo) * i / STEPS, 6)
        out.append({"effect": effect, "field": field, "value": value, "hex": f.enc(conv(value)).hex()})
print(json.dumps(out, indent=0))
