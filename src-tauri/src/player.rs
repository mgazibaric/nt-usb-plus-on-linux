//! Plays a test recording on the computer's default audio output.

use std::io::{self, Write};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crate::meter::RATE;

pub struct Player {
    child: Arc<Mutex<Child>>,
    stopped: Arc<AtomicBool>,
}

/// Players that read mono S16LE from stdin, in order of preference.
fn players() -> [Command; 2] {
    let mut pipewire = Command::new("pw-play");
    pipewire
        .args(["--raw", "--format", "s16", "--channels", "1", "--rate", &RATE.to_string()])
        .args(["-P", "{ application.name=\"NT-USB+ Control\" media.name=\"Test recording\" }"])
        .arg("-");
    let mut alsa = Command::new("aplay");
    alsa.args(["-q", "-t", "raw", "-f", "S16_LE", "-c", "1", "-r", &RATE.to_string()]);
    [pipewire, alsa]
}

impl Player {
    /// Start playing; `on_done` is called from a background thread when the
    /// recording has played to the end, but not when the player is dropped first.
    pub fn start(audio: Arc<Vec<u8>>, on_done: impl FnOnce() + Send + 'static) -> io::Result<Player> {
        let mut spawned = Err(io::Error::from(io::ErrorKind::NotFound));
        for mut player in players() {
            spawned = player.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
            if spawned.is_ok() {
                break;
            }
        }
        let mut child = spawned?;
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let child = Arc::new(Mutex::new(child));
        let stopped = Arc::new(AtomicBool::new(false));
        thread::spawn({
            let (child, stopped) = (child.clone(), stopped.clone());
            move || {
                // Blocks at the speed of playback; fails once the player is killed.
                let _ = stdin.write_all(&audio);
                drop(stdin);
                while !stopped.load(Ordering::Relaxed) {
                    let exited = child.lock().map(|mut c| !matches!(c.try_wait(), Ok(None))).unwrap_or(true);
                    if exited {
                        on_done();
                        return;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            }
        });
        Ok(Player { child, stopped })
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
