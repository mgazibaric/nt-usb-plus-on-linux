import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { EffectId, EffectState, Gain, Level, Monitor, Release, Snapshot, Status } from './types';

export interface Backend {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

const tauri: Backend = {
  invoke,
  listen: (event, handler) => listen(event, (e) => handler(e.payload as never)),
};

// `pnpm dev` in a plain browser gets a simulated microphone.
const backend: Promise<Backend> =
  import.meta.env.DEV && !('__TAURI_INTERNALS__' in window)
    ? import('./mock').then((m) => m.backend)
    : Promise.resolve(tauri);

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return (await backend).invoke<T>(command, args);
}

/** Subscribe to a backend event; returns the unsubscribe function. */
function on<T>(event: string, handler: (payload: T) => void): () => void {
  const pending = backend.then((b) => b.listen(event, handler));
  return () => void pending.then((off) => off());
}

export const getStatus = () => call<Status>('get_status');
export const setHpf = (mode: number) => call<void>('set_hpf', { mode });
export const setMonitor = (enabled: boolean) => call<void>('set_monitor', { on: enabled });
export const setMonitorMix = (value: number) => call<void>('set_monitor_mix', { value });
export const setEffectEnabled = (effect: EffectId, enabled: boolean) =>
  call<EffectState>('set_effect_enabled', { effect, on: enabled });
export const setEffectValue = (effect: EffectId, field: string, value: number) =>
  call<void>('set_effect_value', { effect, field, value });
export const resetEffect = (effect: EffectId) => call<EffectState>('reset_effect', { effect });
export const setGain = (value: number) => call<Gain>('set_gain', { value });
export const save = () => call<void>('save');
/** Go back to the settings from when the app connected or last saved. */
export const revert = () => call<Snapshot>('revert');
/** Write RØDE's factory settings to the mic and save them there. */
export const factoryReset = () => call<Snapshot>('factory_reset');
/** Record from the mic into memory, for up to 30 s; ends with `testStop`. */
export const testRecord = () => call<void>('test_record');
/** Returns the length of the recording in seconds. */
export const testStop = () => call<number>('test_stop');
export const testPlay = () => call<void>('test_play');
export const testStopPlayback = () => call<void>('test_stop_playback');
/** Null if RØDE's update list could not be read. */
export const checkUpdate = () => call<Release | null>('check_update');
export const setMeter = (enabled: boolean) => call<void>('set_meter', { on: enabled });

export const onDeviceChanged = (handler: () => void) => on<null>('device-changed', handler);
/** Direct monitoring was changed outside the app, usually with the dial on the mic. */
export const onMonitor = (handler: (monitor: Monitor) => void) => on<Monitor>('monitor', handler);
export const onGain = (handler: (gain: Gain) => void) => on<Gain>('gain', handler);
export const onTestPlayed = (handler: () => void) => on<null>('test-played', handler);
export const onMeter = (handler: (level: Level) => void) => on<Level>('meter', handler);
