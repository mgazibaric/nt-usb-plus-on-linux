mod device;
mod error;
mod meter;
mod mixer;
mod player;
mod protocol;
mod update;

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, RunEvent};

use device::{Device, Info, PARAM};
use error::{Error, Result};
use meter::Meter;
use mixer::Gain;
use player::Player;
use protocol::{Effect, EFFECTS, FACTORY_GAIN_DB, FACTORY_HPF, HPF_MAX, MONITOR_MIX_MAX, P_HPF, P_MONITOR, P_MONITOR_MIX};

const WATCH_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Inner {
    dev: Option<Device>,
    meter: Option<Meter>,
    meter_wanted: bool,
    /// Last gain the frontend knows about, to report changes made elsewhere
    gain_seen: Option<i64>,
    /// What "reset" goes back to: the settings at connect or at the last save
    baseline: Option<Raw>,
    /// A test recording is running
    recording: bool,
    /// The last test recording (mono S16LE); only ever held in memory
    take: Option<Arc<Vec<u8>>>,
    player: Option<Player>,
}

#[derive(Default)]
struct Shared(Mutex<Inner>);

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[derive(Serialize)]
struct EffectState {
    enabled: bool,
    /// The mic has never been given parameters for this effect; `values` are defaults.
    unset: bool,
    values: BTreeMap<&'static str, f64>,
}

#[derive(Serialize)]
struct Snapshot {
    info: Info,
    hpf: u8,
    monitor: bool,
    monitor_mix: u8,
    effects: BTreeMap<&'static str, EffectState>,
    gain: Option<Gain>,
}

/// Payload of the `monitor` event
#[derive(Clone, Serialize)]
struct Monitor {
    monitor: bool,
    monitor_mix: u8,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
enum Status {
    Disconnected,
    NoPermission { path: String },
    Error { message: String },
    Connected(Snapshot),
}

/// The mic's settings exactly as it reports them, so that they can be put back
/// bit for bit (including the all-zero parameters of a never-configured effect).
#[derive(Clone)]
struct Raw {
    hpf: u8,
    monitor: u8,
    monitor_mix: u8,
    /// In the order of `EFFECTS`
    effects: Vec<RawEffect>,
}

#[derive(Clone)]
struct RawEffect {
    enabled: u8,
    /// In the order of the effect's fields
    params: Vec<Vec<u8>>,
}

/// Set NT_USB_PLUS_DEBUG=1 to trace commands on stderr.
fn trace(what: std::fmt::Arguments<'_>) {
    static ON: OnceLock<bool> = OnceLock::new();
    if *ON.get_or_init(|| std::env::var_os("NT_USB_PLUS_DEBUG").is_some()) {
        eprintln!("[nt-usb-plus-on-linux] {what}");
    }
}

fn read_raw_effect(dev: &mut Device, effect: &Effect) -> Result<RawEffect> {
    let enabled = dev.fx_get(effect.fx, 0)?.first().copied().unwrap_or(0);
    let mut params = Vec::with_capacity(effect.fields.len());
    for field in effect.fields {
        let mut value = dev.fx_get(effect.fx, field.param)?;
        value.resize(field.width(), 0);
        params.push(value);
    }
    Ok(RawEffect { enabled, params })
}

fn effect_state(effect: &Effect, raw: &RawEffect) -> EffectState {
    // A mic that was never configured answers all zeros, which is not a usable
    // setting (and not what the DSP runs with), so show defaults instead. The
    // one real setting that also reads as zeros is big bottom at 0 % / 60 Hz.
    let unset = raw.params.iter().flatten().all(|&b| b == 0);
    let values = effect
        .fields
        .iter()
        .zip(&raw.params)
        .map(|(f, r)| (f.name, if unset { f.default } else { f.decode(r) }))
        .collect();
    EffectState { enabled: raw.enabled != 0, unset, values }
}

fn read_effect(dev: &mut Device, effect: &Effect) -> Result<EffectState> {
    let state = effect_state(effect, &read_raw_effect(dev, effect)?);
    dev.configured[effect.fx as usize] = !state.unset;
    Ok(state)
}

fn write_defaults(dev: &mut Device, effect: &Effect) -> Result<()> {
    for field in effect.fields {
        dev.fx_set(effect.fx, field.param, &field.encode(field.default))?;
    }
    dev.configured[effect.fx as usize] = true;
    Ok(())
}

/// What the frontend shows for the settings in `raw`.
fn snapshot(dev: &mut Device, raw: &Raw) -> Snapshot {
    let mut effects = BTreeMap::new();
    for (effect, stored) in EFFECTS.iter().zip(&raw.effects) {
        let state = effect_state(effect, stored);
        dev.configured[effect.fx as usize] = !state.unset;
        effects.insert(effect.name, state);
    }
    let gain = dev.info.card.and_then(|card| mixer::gain(card).ok());
    Snapshot { info: dev.info.clone(), hpf: raw.hpf, monitor: raw.monitor != 0, monitor_mix: raw.monitor_mix, effects, gain }
}

fn read_raw(dev: &mut Device) -> Result<Raw> {
    let hpf = dev.get_param(P_HPF)?;
    let monitor = dev.get_param(P_MONITOR)?;
    let monitor_mix = dev.get_param(P_MONITOR_MIX)?;
    let mut effects = Vec::with_capacity(EFFECTS.len());
    for effect in &EFFECTS {
        effects.push(read_raw_effect(dev, effect)?);
    }
    Ok(Raw { hpf, monitor, monitor_mix, effects })
}

/// Put back the settings in `raw` that "Save To Microphone" stores. Direct
/// monitoring is not among them: the mic does not save it and its mix follows
/// the dial. Every exchange with the mic takes about 16 ms, so only what this
/// app has written since is sent.
fn restore(dev: &mut Device, raw: &Raw) -> Result<()> {
    let changed = dev.written.clone();
    for (effect, stored) in EFFECTS.iter().zip(&raw.effects) {
        let switch = changed.contains(&(effect.fx, 0));
        // An effect must not run on half-restored parameters.
        if switch && stored.enabled == 0 {
            dev.fx_set(effect.fx, 0, &[0])?;
        }
        for (field, value) in effect.fields.iter().zip(&stored.params) {
            if changed.contains(&(effect.fx, field.param)) {
                dev.fx_set(effect.fx, field.param, value)?;
            }
        }
        if switch && stored.enabled != 0 {
            dev.fx_set(effect.fx, 0, &[stored.enabled])?;
        }
    }
    if changed.contains(&(PARAM, P_HPF)) {
        dev.set_param(P_HPF, raw.hpf)?;
    }
    dev.written.clear();
    Ok(())
}

fn disconnect(inner: &mut Inner) {
    inner.dev = None;
    inner.baseline = None;
    inner.recording = false;
    inner.player = None;
    inner.meter = None;
    inner.gain_seen = None;
}

fn sync_meter(app: &AppHandle, inner: &mut Inner) {
    let card = inner.dev.as_ref().and_then(|d| d.info.card);
    match (inner.meter_wanted || inner.recording, card) {
        (true, Some(card)) => {
            if inner.meter.is_none() {
                let app = app.clone();
                let started = Meter::start(card, move |level| {
                    let _ = app.emit("meter", level);
                });
                match started {
                    Ok(meter) => inner.meter = Some(meter),
                    Err(e) => trace(format_args!("level meter unavailable: {e}")),
                }
            }
        }
        _ => inner.meter = None,
    }
}

type CommandResult<T> = std::result::Result<T, String>;

/// Run `f` with the connected mic. A lost connection is reported to the frontend.
fn with_device<T>(app: &AppHandle, f: impl FnOnce(&mut Device) -> Result<T>) -> CommandResult<T> {
    let shared = app.state::<Shared>();
    let mut inner = shared.lock();
    let dev = inner.dev.as_mut().ok_or("the microphone is not connected")?;
    f(dev).map_err(|e| {
        trace(format_args!("error: {e}"));
        if matches!(e, Error::Io(_)) {
            disconnect(&mut inner);
            let _ = app.emit("device-changed", ());
        }
        e.to_string()
    })
}

fn find_effect(name: &str) -> Result<&'static Effect> {
    protocol::effect(name).ok_or_else(|| Error::Invalid(format!("unknown effect {name:?}")))
}

#[tauri::command(async)]
fn get_status(app: AppHandle) -> Status {
    let shared = app.state::<Shared>();
    let mut inner = shared.lock();
    if inner.dev.is_none() {
        let Some(location) = device::locate() else {
            trace(format_args!("get_status: no microphone"));
            return Status::Disconnected;
        };
        match Device::open(&location) {
            Ok(dev) => inner.dev = Some(dev),
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                return Status::NoPermission { path: location.hidraw.display().to_string() };
            }
            Err(e) => return Status::Error { message: format!("cannot open {}: {e}", location.hidraw.display()) },
        }
    }
    let fresh = inner.baseline.is_none();
    let dev = inner.dev.as_mut().expect("connected above");
    match read_raw(dev) {
        Ok(raw) => {
            let snapshot = snapshot(dev, &raw);
            dev.monitor_seen = Some((raw.monitor, raw.monitor_mix));
            trace(format_args!("get_status: {} on {}", snapshot.info.name, snapshot.info.hidraw));
            if fresh {
                dev.written.clear();
                inner.baseline = Some(raw);
            }
            inner.gain_seen = snapshot.gain.map(|g| g.value);
            sync_meter(&app, &mut inner);
            Status::Connected(snapshot)
        }
        Err(e) => {
            trace(format_args!("get_status: {e}"));
            disconnect(&mut inner);
            Status::Error { message: e.to_string() }
        }
    }
}

#[tauri::command(async)]
fn set_hpf(app: AppHandle, mode: u8) -> CommandResult<()> {
    trace(format_args!("set_hpf {mode}"));
    with_device(&app, |dev| {
        if mode > HPF_MAX {
            return Err(Error::Invalid(format!("high-pass mode {mode} does not exist")));
        }
        dev.set_param(P_HPF, mode)
    })
}

#[tauri::command(async)]
fn set_monitor(app: AppHandle, on: bool) -> CommandResult<()> {
    trace(format_args!("set_monitor {on}"));
    with_device(&app, |dev| {
        if on {
            // RØDE Central re-sends the mix before switching monitoring on.
            let mix = dev.get_param(P_MONITOR_MIX)?;
            dev.set_param(P_MONITOR_MIX, mix)?;
        }
        dev.set_param(P_MONITOR, on as u8)?;
        let mix = dev.get_param(P_MONITOR_MIX)?;
        dev.monitor_seen = Some((on as u8, mix));
        Ok(())
    })
}

#[tauri::command(async)]
fn set_monitor_mix(app: AppHandle, value: u8) -> CommandResult<()> {
    trace(format_args!("set_monitor_mix {value}"));
    with_device(&app, |dev| {
        let value = value.min(MONITOR_MIX_MAX);
        dev.set_param(P_MONITOR_MIX, value)?;
        if let Some(seen) = dev.monitor_seen.as_mut() {
            seen.1 = value;
        }
        Ok(())
    })
}

#[tauri::command(async)]
fn set_effect_enabled(app: AppHandle, effect: String, on: bool) -> CommandResult<EffectState> {
    trace(format_args!("set_effect_enabled {effect} {on}"));
    with_device(&app, |dev| {
        let effect = find_effect(&effect)?;
        if on && !dev.configured[effect.fx as usize] && read_effect(dev, effect)?.unset {
            // Make the mic run with the values the UI shows.
            write_defaults(dev, effect)?;
        }
        dev.fx_set(effect.fx, 0, &[on as u8])?;
        read_effect(dev, effect)
    })
}

#[tauri::command(async)]
fn set_effect_value(app: AppHandle, effect: String, field: String, value: f64) -> CommandResult<()> {
    trace(format_args!("set_effect_value {effect}.{field} {value}"));
    with_device(&app, |dev| {
        let effect = find_effect(&effect)?;
        let target = effect.field(&field).ok_or_else(|| Error::Invalid(format!("unknown parameter {field:?}")))?;
        if !value.is_finite() {
            return Err(Error::Invalid(format!("{field} must be a number")));
        }
        if !dev.configured[effect.fx as usize] && read_effect(dev, effect)?.unset {
            // Give the other parameters of a never-configured effect a defined value too.
            write_defaults(dev, effect)?;
        }
        dev.fx_set(effect.fx, target.param, &target.encode(value))
    })
}

#[tauri::command(async)]
fn reset_effect(app: AppHandle, effect: String) -> CommandResult<EffectState> {
    trace(format_args!("reset_effect {effect}"));
    with_device(&app, |dev| {
        let effect = find_effect(&effect)?;
        write_defaults(dev, effect)?;
        read_effect(dev, effect)
    })
}

#[tauri::command(async)]
fn set_gain(app: AppHandle, value: i64) -> CommandResult<Gain> {
    trace(format_args!("set_gain {value}"));
    let shared = app.state::<Shared>();
    let mut inner = shared.lock();
    let card = inner.dev.as_ref().and_then(|d| d.info.card).ok_or("the microphone has no audio device")?;
    let gain = mixer::set_gain(card, value).map_err(|e| e.to_string())?;
    inner.gain_seen = Some(gain.value);
    Ok(gain)
}

#[tauri::command(async)]
fn save(app: AppHandle) -> CommandResult<()> {
    trace(format_args!("save"));
    let saved = with_device(&app, |dev| {
        let raw = read_raw(dev)?;
        dev.save()?;
        dev.written.clear();
        Ok(raw)
    })?;
    app.state::<Shared>().lock().baseline = Some(saved);
    Ok(())
}

/// Undo everything since the app connected or last saved.
#[tauri::command(async)]
fn revert(app: AppHandle) -> CommandResult<Snapshot> {
    trace(format_args!("revert"));
    let baseline = app.state::<Shared>().lock().baseline.clone().ok_or("there are no earlier settings to go back to")?;
    with_device(&app, |dev| {
        restore(dev, &baseline)?;
        let (monitor, monitor_mix) = (dev.get_param(P_MONITOR)?, dev.get_param(P_MONITOR_MIX)?);
        dev.monitor_seen = Some((monitor, monitor_mix));
        Ok(snapshot(dev, &Raw { monitor, monitor_mix, ..baseline }))
    })
}

/// Write what RØDE Central's factory reset gives an NT-USB+ and save it. The
/// mic has no reset command of its own; these are ordinary writes.
#[tauri::command(async)]
fn factory_reset(app: AppHandle) -> CommandResult<Snapshot> {
    trace(format_args!("factory_reset"));
    let (snapshot, raw) = with_device(&app, |dev| {
        if let Some(card) = dev.info.card {
            mixer::set_gain_db(card, FACTORY_GAIN_DB)?;
        }
        dev.set_param(P_HPF, FACTORY_HPF)?;
        let mut effects = Vec::with_capacity(EFFECTS.len());
        for effect in &EFFECTS {
            write_defaults(dev, effect)?;
            let params = effect.fields.iter().map(|f| f.encode(f.default)).collect();
            effects.push(RawEffect { enabled: effect.factory_on as u8, params });
        }
        for effect in &EFFECTS {
            dev.fx_set(effect.fx, 0, &[effect.factory_on as u8])?;
        }
        dev.save()?;
        dev.written.clear();
        let (monitor, monitor_mix) = (dev.get_param(P_MONITOR)?, dev.get_param(P_MONITOR_MIX)?);
        dev.monitor_seen = Some((monitor, monitor_mix));
        // Every write was acknowledged, so this is what the mic holds now.
        let raw = Raw { hpf: FACTORY_HPF, monitor, monitor_mix, effects };
        Ok((snapshot(dev, &raw), raw))
    })?;
    let shared = app.state::<Shared>();
    let mut inner = shared.lock();
    inner.gain_seen = snapshot.gain.map(|g| g.value);
    inner.baseline = Some(raw);
    Ok(snapshot)
}

/// Start a test recording. It ends with `test_stop`, or stops growing after
/// `meter::MAX_SECONDS`.
#[tauri::command(async)]
fn test_record(app: AppHandle) -> CommandResult<()> {
    trace(format_args!("test_record"));
    let shared = app.state::<Shared>();
    let mut inner = shared.lock();
    inner.player = None;
    inner.recording = true;
    sync_meter(&app, &mut inner);
    match &inner.meter {
        Some(meter) => {
            meter.record();
            Ok(())
        }
        None => {
            inner.recording = false;
            Err("cannot record from the microphone".into())
        }
    }
}

/// End the test recording; returns its length in seconds.
#[tauri::command(async)]
fn test_stop(app: AppHandle) -> f64 {
    let shared = app.state::<Shared>();
    let mut inner = shared.lock();
    inner.recording = false;
    let take = inner.meter.as_ref().map(Meter::take).unwrap_or_default();
    sync_meter(&app, &mut inner);
    let seconds = take.len() as f64 / (2.0 * meter::RATE as f64);
    trace(format_args!("test_stop: {seconds:.1} s"));
    inner.take = (!take.is_empty()).then(|| Arc::new(take));
    seconds
}

/// Play the test recording on the default output; `test-played` is emitted at its end.
#[tauri::command(async)]
fn test_play(app: AppHandle) -> CommandResult<()> {
    trace(format_args!("test_play"));
    let shared = app.state::<Shared>();
    let mut inner = shared.lock();
    let take = inner.take.clone().ok_or("nothing has been recorded yet")?;
    inner.player = None;
    let handle = app.clone();
    let player = Player::start(take, move || {
        let _ = handle.emit("test-played", ());
    });
    inner.player = Some(player.map_err(|e| format!("cannot play the recording: {e}"))?);
    Ok(())
}

#[tauri::command(async)]
fn test_stop_playback(app: AppHandle) {
    app.state::<Shared>().lock().player = None;
}

#[derive(Serialize)]
struct Release {
    /// Newest firmware RØDE has released
    latest: String,
    /// It is newer than what the mic runs
    newer: bool,
}

/// Compare the mic's firmware with RØDE's update list; `None` if the list could not be read.
#[tauri::command(async)]
fn check_update(app: AppHandle) -> Option<Release> {
    let installed = app.state::<Shared>().lock().dev.as_ref()?.info.firmware.clone();
    match update::latest() {
        Ok(latest) => {
            trace(format_args!("check_update: mic has {installed}, released is {latest}"));
            Some(Release { newer: update::is_newer(&latest, &installed), latest })
        }
        Err(e) => {
            trace(format_args!("check_update: {e}"));
            None
        }
    }
}

#[tauri::command(async)]
fn set_meter(app: AppHandle, on: bool) {
    trace(format_args!("set_meter {on}"));
    let shared = app.state::<Shared>();
    let mut inner = shared.lock();
    inner.meter_wanted = on;
    sync_meter(&app, &mut inner);
}

/// Report plugging and unplugging, gain changes made by other programs, and
/// direct-monitor changes made with the dial on the mic.
fn watch(app: AppHandle) {
    let mut present = device::locate().is_some();
    loop {
        thread::sleep(WATCH_INTERVAL);
        let now = device::locate().is_some();
        let shared = app.state::<Shared>();
        if now != present {
            present = now;
            if !now {
                disconnect(&mut shared.lock());
            }
            trace(format_args!("microphone {}", if now { "plugged in" } else { "unplugged" }));
            let _ = app.emit("device-changed", ());
            continue;
        }
        let mut inner = shared.lock();
        if let Some(dev) = inner.dev.as_mut() {
            // A failure here is either passing or shows up as an unplug above.
            if let (Ok(monitor), Ok(monitor_mix)) = (dev.get_param(P_MONITOR), dev.get_param(P_MONITOR_MIX)) {
                if dev.monitor_seen.replace((monitor, monitor_mix)) != Some((monitor, monitor_mix)) {
                    let _ = app.emit("monitor", Monitor { monitor: monitor != 0, monitor_mix });
                }
            }
        }
        let Some(card) = inner.dev.as_ref().and_then(|d| d.info.card) else { continue };
        if let Ok(gain) = mixer::gain(card) {
            if inner.gain_seen.replace(gain.value) != Some(gain.value) {
                let _ = app.emit("gain", gain);
            }
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .manage(Shared::default())
        .setup(|app| {
            let handle = app.handle().clone();
            thread::spawn(move || watch(handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            set_hpf,
            set_monitor,
            set_monitor_mix,
            set_effect_enabled,
            set_effect_value,
            reset_effect,
            set_gain,
            save,
            revert,
            factory_reset,
            set_meter,
            test_record,
            test_stop,
            test_play,
            test_stop_playback,
            check_update,
        ])
        .build(tauri::generate_context!())
        .expect("failed to start the application")
        .run(|app, event| {
            if let RunEvent::Exit = event {
                // Do not leave the recorder or the player running.
                disconnect(&mut app.state::<Shared>().lock());
            }
        });
}
