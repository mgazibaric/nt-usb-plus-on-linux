//! Wire format of the NT-USB+ control channel. See docs/PROTOCOL.md.

use std::sync::LazyLock;

use serde::Deserialize;

pub const VID: u16 = 0x19F7;
pub const PID: u16 = 0x0035;

pub const ACK: u8 = 0x41;

/// Output reports. Report 2 (bootloader) is deliberately not representable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Report {
    Aphex = 0x04,
    Param = 0x08,
}

impl Report {
    /// (payload length, id of the input report carrying the reply)
    pub fn layout(self) -> (usize, u8) {
        match self {
            Report::Aphex => (28, 0x03),
            Report::Param => (27, 0x07),
        }
    }
}

// Report 8 parameter ids
pub const P_HPF: u8 = 1;
pub const P_MONITOR: u8 = 3;
pub const P_MONITOR_MIX: u8 = 4;
pub const P_SAVE: u8 = 10;

pub const HPF_MAX: u8 = 2;

// What RØDE Central's factory reset sets besides the effects
pub const FACTORY_HPF: u8 = 0;
pub const FACTORY_GAIN_DB: f32 = 12.0;
pub const MONITOR_MIX_MAX: u8 = 100;

// Report 4 operations (granular only; the NT-USB+ does not use set-all/get-all)
pub const OP_SET: u8 = 2;
pub const OP_GET: u8 = 3;

const SAMPLE_RATE: f64 = 48000.0;
const Q31: f64 = 2147483648.0;

#[derive(Deserialize)]
struct Tables {
    compressor_threshold: Vec<i32>,
    compressor_attack: Vec<i32>,
    compressor_release: Vec<i32>,
    compressor_makeup_gain: Vec<i32>,
    log: Vec<i32>,
    hf_filter_freq: Vec<i32>,
    hf_filter_gain: Vec<i32>,
}

static TABLES: LazyLock<Tables> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../tools/aphex_tables.json")).expect("aphex_tables.json is valid")
});

#[derive(Clone, Copy, Debug)]
pub enum Codec {
    CompThreshold,
    CompRatio,
    CompAttack,
    CompRelease,
    CompGain,
    /// Q31 linear gain from dB (gate threshold and range)
    GateLevel,
    /// Q31 one-pole coefficient from milliseconds
    GateAttack,
    /// Q31 per-sample step from milliseconds (gate hold and release)
    GateTime,
    /// Q31 linear gain from a positive dB offset
    GateHysteresis,
    ExciterMix,
    ExciterTune,
    BottomDrive,
    BottomTune,
}

pub struct Field {
    pub name: &'static str,
    pub param: u8,
    pub min: f64,
    pub max: f64,
    pub default: f64,
    pub codec: Codec,
}

pub struct Effect {
    pub name: &'static str,
    pub fx: u8,
    /// Whether RØDE Central's factory reset leaves the effect switched on
    pub factory_on: bool,
    /// Parameter 0 (enabled) is implicit.
    pub fields: &'static [Field],
}

const fn field(name: &'static str, param: u8, min: f64, max: f64, default: f64, codec: Codec) -> Field {
    Field { name, param, min, max, default, codec }
}

/// Defaults are the values RØDE Central's factory reset writes to an NT-USB+.
pub static EFFECTS: [Effect; 4] = [
    Effect {
        name: "compressor",
        fx: 0,
        factory_on: true,
        fields: &[
            field("threshold", 1, -60.0, 0.0, -25.0, Codec::CompThreshold),
            field("ratio", 2, 1.5, 4.5, 2.0, Codec::CompRatio),
            field("attack", 3, 0.1, 10.0, 0.7, Codec::CompAttack),
            field("release", 4, 5.0, 200.0, 21.0, Codec::CompRelease),
            field("gain", 5, 0.0, 9.0, 3.0, Codec::CompGain),
        ],
    },
    Effect {
        name: "gate",
        fx: 1,
        factory_on: false,
        fields: &[
            field("threshold", 1, -96.0, 0.0, -40.0, Codec::GateLevel),
            field("attack", 2, 0.1, 1000.0, 30.0, Codec::GateAttack),
            field("hold", 3, 1.0, 5000.0, 50.0, Codec::GateTime),
            field("release", 4, 1.0, 5000.0, 200.0, Codec::GateTime),
            field("range", 5, -96.0, 0.0, -9.0, Codec::GateLevel),
            field("hysteresis", 6, 1.0, 8.0, 2.4, Codec::GateHysteresis),
        ],
    },
    Effect {
        name: "exciter",
        fx: 2,
        factory_on: true,
        fields: &[
            field("mix", 1, 0.0, 100.0, 85.0, Codec::ExciterMix),
            field("tune", 2, 600.0, 5000.0, 3500.0, Codec::ExciterTune),
        ],
    },
    Effect {
        name: "bigbottom",
        fx: 3,
        factory_on: true,
        fields: &[
            field("drive", 1, 0.0, 100.0, 80.0, Codec::BottomDrive),
            field("tune", 2, 60.0, 312.0, 90.0, Codec::BottomTune),
        ],
    },
];

pub fn effect(name: &str) -> Option<&'static Effect> {
    EFFECTS.iter().find(|e| e.name == name)
}

impl Effect {
    pub fn field(&self, name: &str) -> Option<&'static Field> {
        self.fields.iter().find(|f| f.name == name)
    }
}

fn idx(norm: f64) -> usize {
    (norm.clamp(0.0, 1.0) * 255.0) as usize
}

/// RØDE Central holds these values as single-precision floats before scaling;
/// rounding the same way reproduces the mic's factory values bit for bit.
fn q31(x: f64) -> i32 {
    (x as f32 as f64 * Q31).clamp(0.0, i32::MAX as f64) as i32
}

fn le32(bytes: &[u8]) -> i32 {
    let mut raw = [0u8; 4];
    let n = bytes.len().min(4);
    raw[..n].copy_from_slice(&bytes[..n]);
    i32::from_le_bytes(raw)
}

fn byte(bytes: &[u8], at: usize) -> f64 {
    bytes.get(at).copied().unwrap_or(0) as f64
}

/// Index of the table entry closest to `value`.
fn table_index(table: &[i32], value: i32) -> f64 {
    let (i, _) = table
        .iter()
        .enumerate()
        .min_by_key(|(_, &t)| (t as i64 - value as i64).abs())
        .expect("tables are not empty");
    i as f64
}

fn log_norm(x: f64, lo: f64, hi: f64) -> f64 {
    (x / lo).log10() / (hi / lo).log10()
}

fn log_denorm(n: f64, lo: f64, hi: f64) -> f64 {
    lo * 10f64.powf((hi / lo).log10() * n)
}

fn pole_from_ms(ms: f64) -> f64 {
    let c = (5.0 / (ms / 1000.0) / SAMPLE_RATE).cos();
    c - 1.0 + (c * c - 4.0 * c + 3.0).sqrt()
}

fn ms_from_pole(a: f64) -> f64 {
    if a <= 0.0 {
        return f64::INFINITY;
    }
    let y = if a < 1.0 { a * a / (2.0 * (1.0 - a)) } else { 2.0 };
    let w = (1.0 - y).clamp(-1.0, 1.0).acos();
    if w > 0.0 {
        5.0 / (w * SAMPLE_RATE) * 1000.0
    } else {
        f64::INFINITY
    }
}

fn db(v: i32) -> f64 {
    if v > 0 {
        20.0 * (v as f64 / Q31).log10()
    } else {
        f64::NEG_INFINITY
    }
}

impl Field {
    /// Wire bytes for a value in user units. The value is clamped to the field's range.
    pub fn encode(&self, value: f64) -> Vec<u8> {
        let t = &*TABLES;
        let v = value.clamp(self.min, self.max);
        let int = |x: i32| x.to_le_bytes().to_vec();
        match self.codec {
            Codec::CompThreshold => int(t.compressor_threshold[idx(-v / 60.0)]),
            Codec::CompRatio => vec![idx((v - 1.5) / 3.0) as u8],
            Codec::CompAttack => int(t.compressor_attack[idx(log_norm(v, 0.1, 10.0))]),
            Codec::CompRelease => int(t.compressor_release[idx(log_norm(v, 5.0, 200.0))]),
            Codec::CompGain => int(t.compressor_makeup_gain[idx(v / 9.0)]),
            Codec::GateLevel => int(q31(10f64.powf(v / 20.0))),
            Codec::GateAttack => int(q31(pole_from_ms(v))),
            Codec::GateTime => int(q31(1.0 / (v / 1000.0 * SAMPLE_RATE))),
            Codec::GateHysteresis => int(q31(10f64.powf(-v / 20.0))),
            Codec::ExciterMix | Codec::BottomDrive => {
                let i = idx(v / 100.0);
                let mut out = int(t.log[i]);
                out.push(i as u8);
                out
            }
            Codec::ExciterTune => {
                let i = idx((v - 600.0) / 4400.0);
                let mut out = int(t.hf_filter_freq[i]);
                out.extend(int(t.hf_filter_gain[i]));
                out.push(i as u8);
                out
            }
            Codec::BottomTune => vec![idx((v - 60.0) / 252.0) as u8],
        }
    }

    /// User-unit value of a reply payload, clamped to the field's range.
    pub fn decode(&self, reply: &[u8]) -> f64 {
        // The lookup tables are coarse (3500 Hz is stored as the entry for
        // 3499 Hz); what was written as the default reads as the default.
        if reply.get(..self.width()) == Some(&self.encode(self.default)[..]) {
            return self.default;
        }
        let t = &*TABLES;
        let raw = le32(reply);
        let v = match self.codec {
            Codec::CompThreshold => -60.0 * table_index(&t.compressor_threshold, raw) / 255.0,
            Codec::CompRatio => 1.5 + byte(reply, 0) / 255.0 * 3.0,
            Codec::CompAttack => log_denorm(table_index(&t.compressor_attack, raw) / 255.0, 0.1, 10.0),
            Codec::CompRelease => log_denorm(table_index(&t.compressor_release, raw) / 255.0, 5.0, 200.0),
            Codec::CompGain => 9.0 * table_index(&t.compressor_makeup_gain, raw) / 255.0,
            Codec::GateLevel => db(raw),
            Codec::GateAttack => ms_from_pole(raw as f64 / Q31),
            Codec::GateTime if raw > 0 => 1000.0 / (raw as f64 / Q31 * SAMPLE_RATE),
            Codec::GateTime => f64::INFINITY,
            Codec::GateHysteresis => -db(raw),
            Codec::ExciterMix | Codec::BottomDrive => byte(reply, 4) / 255.0 * 100.0,
            Codec::ExciterTune => 600.0 + byte(reply, 8) / 255.0 * 4400.0,
            Codec::BottomTune => 60.0 + byte(reply, 0) / 255.0 * 252.0,
        };
        if v.is_nan() {
            self.default
        } else {
            v.clamp(self.min, self.max)
        }
    }

    /// Number of reply bytes that carry this field's value.
    pub fn width(&self) -> usize {
        match self.codec {
            Codec::CompRatio | Codec::BottomTune => 1,
            Codec::ExciterMix | Codec::BottomDrive => 5,
            Codec::ExciterTune => 9,
            _ => 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Golden {
        effect: String,
        field: String,
        value: f64,
        hex: String,
    }

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// Vectors come from tools/rodectl.py (see tools/gen_golden.py), which was
    /// checked byte for byte against the microphone.
    #[test]
    fn encodings_match_reference_implementation() {
        let golden: Vec<Golden> = serde_json::from_str(include_str!("testdata/golden.json")).unwrap();
        assert!(golden.len() > 100);
        for g in golden {
            let f = effect(&g.effect).unwrap().field(&g.field).unwrap();
            assert_eq!(f.encode(g.value), unhex(&g.hex), "{}.{} = {}", g.effect, g.field, g.value);
            assert_eq!(f.encode(g.value).len(), f.width(), "{}.{}", g.effect, g.field);
        }
    }

    #[test]
    fn decode_inverts_encode() {
        for e in &EFFECTS {
            for f in e.fields {
                for step in 0..=40 {
                    let v = f.min + (f.max - f.min) * step as f64 / 40.0;
                    let back = f.decode(&f.encode(v));
                    let again = f.decode(&f.encode(back));
                    // Within about one table step: 1 % of the range, or 3 % of the
                    // value on the logarithmic time scales.
                    let near = |a: f64, b: f64| (a - b).abs() <= (f.max - f.min) / 100.0 || (a / b - 1.0).abs() <= 0.03;
                    assert!(near(back, v), "{}.{}: {v} -> {back}", e.name, f.name);
                    assert!(near(again, back), "{}.{}: {back} -> {again}", e.name, f.name);
                }
            }
        }
    }

    /// Restoring earlier settings writes back `width` reply bytes unchanged.
    #[test]
    fn a_reply_is_as_wide_as_the_value_written() {
        for e in &EFFECTS {
            for f in e.fields {
                assert_eq!(f.encode(f.default).len(), f.width(), "{}.{}", e.name, f.name);
            }
        }
    }

    #[test]
    fn the_default_reads_back_exactly() {
        for e in &EFFECTS {
            for f in e.fields {
                assert_eq!(f.decode(&f.encode(f.default)), f.default, "{}.{}", e.name, f.name);
            }
        }
    }

    #[test]
    fn decode_survives_garbage() {
        for e in &EFFECTS {
            for f in e.fields {
                for reply in [&[][..], &[0u8; 25], &[0xFFu8; 25]] {
                    let v = f.decode(reply);
                    assert!(v.is_finite() && v >= f.min && v <= f.max, "{}.{}: {v}", e.name, f.name);
                }
            }
        }
    }

    #[test]
    fn gate_defaults_are_the_factory_bytes() {
        // Replies read from an NT-USB+ (firmware 1.0.9) whose gate was never changed.
        let gate = effect("gate").unwrap();
        let factory: [(&str, [u8; 4]); 6] = [
            ("threshold", [0x14, 0xae, 0x47, 0x01]),
            ("attack", [0x92, 0x94, 0x71, 0x00]),
            ("hold", [0x40, 0xa7, 0x0d, 0x00]),
            ("release", [0xd0, 0x69, 0x03, 0x00]),
            ("range", [0x80, 0x86, 0x6a, 0x2d]),
            ("hysteresis", [0x00, 0x12, 0x19, 0x61]),
        ];
        for (name, bytes) in factory {
            let f = gate.field(name).unwrap();
            let got = f.decode(&bytes);
            assert!((got - f.default).abs() < f.default.abs() * 1e-3, "{name}: {got}");
            assert_eq!(f.encode(f.default), bytes, "{name}");
        }
    }
}
