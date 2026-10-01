//! hidraw transport and device discovery.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::protocol::{Report, ACK, OP_GET, OP_SET, PID, P_SAVE, VID};

/// RØDE Central waits this long for a reply.
const TIMEOUT: Duration = Duration::from_secs(2);
/// Writing the settings to flash takes about 1.4 s, too close to the above.
const SAVE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Location {
    pub hidraw: PathBuf,
    /// sysfs directory of the USB device
    usb: PathBuf,
}

/// Find the control interface of a connected NT-USB+.
pub fn locate() -> Option<Location> {
    let want = format!("HID_ID=0003:{VID:08X}:{PID:08X}");
    let mut nodes: Vec<_> = fs::read_dir("/sys/class/hidraw").ok()?.flatten().collect();
    nodes.sort_by_key(|n| n.file_name());
    for node in nodes {
        let hid = node.path().join("device");
        let Ok(uevent) = fs::read_to_string(hid.join("uevent")) else { continue };
        if !uevent.contains(&want) {
            continue;
        }
        // hid device -> usb interface -> usb device
        let Ok(hid) = fs::canonicalize(&hid) else { continue };
        let Some(usb) = hid.parent().and_then(Path::parent) else { continue };
        return Some(Location { hidraw: Path::new("/dev").join(node.file_name()), usb: usb.to_path_buf() });
    }
    None
}

#[derive(Clone, Debug, Serialize)]
pub struct Info {
    pub name: String,
    pub serial: String,
    pub firmware: String,
    pub hidraw: String,
    /// ALSA card number of the mic's audio interface
    pub card: Option<u32>,
}

impl Location {
    fn attr(&self, name: &str) -> String {
        fs::read_to_string(self.usb.join(name)).map(|s| s.trim().to_string()).unwrap_or_default()
    }

    fn card(&self) -> Option<u32> {
        for iface in fs::read_dir(&self.usb).ok()?.flatten() {
            let Ok(sound) = fs::read_dir(iface.path().join("sound")) else { continue };
            for entry in sound.flatten() {
                if let Some(n) = entry.file_name().to_str().and_then(|s| s.strip_prefix("card")) {
                    if let Ok(n) = n.parse() {
                        return Some(n);
                    }
                }
            }
        }
        None
    }

    pub fn info(&self) -> Info {
        Info {
            name: self.attr("product"),
            serial: self.attr("serial"),
            firmware: firmware_version(&self.attr("bcdDevice")),
            hidraw: self.hidraw.display().to_string(),
            card: self.card(),
        }
    }
}

/// bcdDevice "0109" is firmware 1.0.9.
fn firmware_version(bcd: &str) -> String {
    if bcd.len() == 4 && bcd.bytes().all(|b| b.is_ascii_digit()) {
        let major: u32 = bcd[..2].parse().unwrap_or(0);
        format!("{major}.{}.{}", &bcd[2..3], &bcd[3..4])
    } else {
        bcd.to_string()
    }
}

pub struct Device {
    file: File,
    pub info: Info,
    /// Per effect (indexed by fx): known to hold parameters, see `read_effect`
    pub configured: [bool; 4],
    /// What was written since the app connected or saved, as (fx, param) or
    /// (`PARAM`, param) for report 8. Only these need putting back on a reset.
    pub written: HashSet<(u8, u8)>,
    /// Direct-monitor switch and mix the frontend knows about, to report when
    /// the dial on the mic changes them
    pub monitor_seen: Option<(u8, u8)>,
}

/// Stands for report 8 in `Device::written`
pub const PARAM: u8 = 0xFF;

impl Device {
    pub fn open(location: &Location) -> io::Result<Device> {
        let file = OpenOptions::new().read(true).write(true).custom_flags(libc::O_NONBLOCK).open(&location.hidraw)?;
        Ok(Device { file, info: location.info(), configured: [false; 4], written: HashSet::new(), monitor_seen: None })
    }

    fn drain(&mut self) {
        let mut buf = [0u8; 64];
        while matches!(self.file.read(&mut buf), Ok(n) if n > 0) {}
    }

    /// Send one output report and return the reply payload after the status byte.
    fn transfer(&mut self, report: Report, payload: &[u8]) -> Result<Vec<u8>> {
        self.transfer_within(TIMEOUT, report, payload)
    }

    fn transfer_within(&mut self, timeout: Duration, report: Report, payload: &[u8]) -> Result<Vec<u8>> {
        let (len, reply_id) = report.layout();
        assert!(!payload.is_empty() && payload.len() <= len);
        let mut out = vec![0u8; len + 1];
        out[0] = report as u8;
        out[1..=payload.len()].copy_from_slice(payload);

        self.drain();
        self.file.write_all(&out)?;

        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 64];
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout);
            }
            let mut pfd = libc::pollfd { fd: self.file.as_raw_fd(), events: libc::POLLIN, revents: 0 };
            // SAFETY: pfd is a valid pollfd for the duration of the call.
            let ready = unsafe { libc::poll(&mut pfd, 1, left.as_millis() as libc::c_int) };
            if ready < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Err(e.into());
            }
            if ready == 0 {
                return Err(Error::Timeout);
            }
            if pfd.revents & libc::POLLIN == 0 {
                // POLLERR / POLLHUP: the mic was unplugged
                return Err(io::Error::from_raw_os_error(libc::ENODEV).into());
            }
            let n = match self.file.read(&mut buf) {
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => return Err(e.into()),
            };
            let data = &buf[..n];
            // The reply echoes the first payload byte right after the report id.
            if n >= 3 && data[0] == reply_id && data[1] == payload[0] {
                if data[2] != ACK {
                    let request = out[..5].iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
                    return Err(Error::Refused { request, status: data[2] });
                }
                return Ok(data[3..].to_vec());
            }
        }
    }

    pub fn get_param(&mut self, param: u8) -> Result<u8> {
        let reply = self.transfer(Report::Param, &[param, 1])?;
        reply.first().copied().ok_or_else(|| Error::Invalid("short reply from the microphone".into()))
    }

    pub fn set_param(&mut self, param: u8, value: u8) -> Result<()> {
        self.written.insert((PARAM, param));
        self.transfer(Report::Param, &[param, 0, value]).map(drop)
    }

    /// Persist the current settings on the mic.
    pub fn save(&mut self) -> Result<()> {
        self.transfer_within(SAVE_TIMEOUT, Report::Param, &[P_SAVE, 0, 0]).map(drop)
    }

    pub fn fx_get(&mut self, fx: u8, param: u8) -> Result<Vec<u8>> {
        self.transfer(Report::Aphex, &[fx, OP_GET, param])
    }

    pub fn fx_set(&mut self, fx: u8, param: u8, value: &[u8]) -> Result<()> {
        self.written.insert((fx, param));
        let mut payload = vec![fx, OP_SET, param];
        payload.extend_from_slice(value);
        self.transfer(Report::Aphex, &payload).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::firmware_version;

    #[test]
    fn firmware_from_bcd() {
        assert_eq!(firmware_version("0109"), "1.0.9");
        assert_eq!(firmware_version("1234"), "12.3.4");
        assert_eq!(firmware_version("0010"), "0.1.0");
        assert_eq!(firmware_version(""), "");
    }
}
