//! Input level meter. Audio is reduced to peak and RMS in memory. It is kept
//! (in memory only) while a test recording is running, and at no other time.

use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use serde::Serialize;
use serde_json::Value;

pub const RATE: u32 = 48000;
/// Longest test recording
pub const MAX_SECONDS: u32 = 30;
/// The same in bytes of mono S16
const TAPE_MAX: usize = MAX_SECONDS as usize * RATE as usize * 2;
/// Samples per level update (20 updates per second)
const BLOCK: usize = RATE as usize / 20;
const FLOOR_DB: f32 = -90.0;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Level {
    /// dBFS
    pub peak: f32,
    /// dBFS
    pub rms: f32,
}

pub struct Meter {
    child: Child,
    reader: Option<JoinHandle<()>>,
    /// The test recording in progress, if any
    tape: Arc<Mutex<Option<Vec<u8>>>>,
}

/// `object.serial` of the PipeWire source that belongs to the ALSA card.
fn pipewire_source(card: u32) -> Option<u64> {
    let out = Command::new("pw-dump").stderr(Stdio::null()).output().ok()?;
    let objects: Value = serde_json::from_slice(&out.stdout).ok()?;
    let is_card = |v: &Value| v.as_u64() == Some(card as u64) || v.as_str() == Some(&card.to_string());
    objects.as_array()?.iter().find_map(|o| {
        let props = o.get("info")?.get("props")?;
        if props.get("media.class")?.as_str()? != "Audio/Source" || !is_card(props.get("alsa.card")?) {
            return None;
        }
        props.get("object.serial")?.as_u64()
    })
}

/// A recorder that writes mono S16LE at 48 kHz to stdout. PipeWire is preferred
/// because it can share the mic with other applications.
fn recorder(card: u32) -> Command {
    if let Some(serial) = pipewire_source(card) {
        let mut cmd = Command::new("pw-record");
        cmd.args(["--raw", "--format", "s16", "--channels", "1", "--latency", "50ms"])
            .args(["--rate", &RATE.to_string(), "--target", &serial.to_string()])
            // Never fall back to another microphone when this one goes away.
            .args(["-P", "{ node.name=nt-usb-plus-on-linux-meter application.name=\"NT-USB+ Control\" media.name=\"Level meter\" node.dont-reconnect=true }"])
            .arg("-");
        cmd
    } else {
        let mut cmd = Command::new("arecord");
        cmd.args(["-q", "-t", "raw", "-f", "S16_LE", "-c", "1"])
            .args(["-r", &RATE.to_string(), "-D", &format!("plughw:{card},0")]);
        cmd
    }
}

fn level(block: &[u8]) -> Level {
    let mut peak = 0i32;
    let mut sum = 0f64;
    for s in block.chunks_exact(2) {
        let v = i16::from_le_bytes([s[0], s[1]]) as i32;
        peak = peak.max(v.abs());
        sum += (v * v) as f64;
    }
    let db = |x: f64| if x > 0.0 { (20.0 * (x / 32768.0).log10() as f32).max(FLOOR_DB) } else { FLOOR_DB };
    Level { peak: db(peak as f64), rms: db((sum / (block.len() / 2) as f64).sqrt()) }
}

impl Meter {
    /// Start metering; `on_level` is called from a background thread.
    pub fn start(card: u32, on_level: impl Fn(Level) + Send + 'static) -> io::Result<Meter> {
        let mut child = recorder(card).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
        let mut audio = child.stdout.take().expect("stdout is piped");
        let tape = Arc::new(Mutex::new(None::<Vec<u8>>));
        let reader = thread::spawn({
            let tape = tape.clone();
            move || {
                let mut block = vec![0u8; BLOCK * 2];
                while audio.read_exact(&mut block).is_ok() {
                    on_level(level(&block));
                    if let Some(tape) = tape.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                        if tape.len() < TAPE_MAX {
                            tape.extend_from_slice(&block);
                        }
                    }
                }
            }
        });
        Ok(Meter { child, reader: Some(reader), tape })
    }

    /// Start keeping the audio, up to `MAX_SECONDS`.
    pub fn record(&self) {
        *self.tape.lock().unwrap_or_else(|p| p.into_inner()) = Some(Vec::new());
    }

    /// Stop keeping the audio and hand over what was recorded (mono S16LE at `RATE`).
    pub fn take(&self) -> Vec<u8> {
        self.tape.lock().unwrap_or_else(|p| p.into_inner()).take().unwrap_or_default()
    }
}

impl Drop for Meter {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        let silence = level(&[0u8; 64]);
        assert_eq!((silence.peak, silence.rms), (FLOOR_DB, FLOOR_DB));
        let full: Vec<u8> = std::iter::repeat(i16::MIN.to_le_bytes()).take(32).flatten().collect();
        let l = level(&full);
        assert!(l.peak.abs() < 0.01 && l.rms.abs() < 0.01);
    }
}
