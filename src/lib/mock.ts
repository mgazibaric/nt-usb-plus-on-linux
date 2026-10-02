// Simulated microphone for working on the UI in a plain browser (`pnpm dev`).
// Never part of a production build, see api.ts.
import type { Backend } from './api';
import type { EffectId, EffectState, Gain, Level, Snapshot, Status } from './types';

const DEFAULTS: Record<EffectId, Record<string, number>> = {
  gate: { threshold: -40, attack: 30, hold: 50, release: 200, range: -9, hysteresis: 2.4 },
  compressor: { threshold: -25, ratio: 2, attack: 0.7, release: 21, gain: 3 },
  exciter: { mix: 85, tune: 3500 },
  bigbottom: { drive: 80, tune: 90 },
};

const effect = (id: EffectId, unset: boolean): EffectState => ({ enabled: false, unset, values: { ...DEFAULTS[id] } });

const mic: Snapshot = {
  info: { name: 'RØDE NT-USB+', serial: 'SIMULATED', firmware: '1.0.9', hidraw: '/dev/hidraw0', card: 1 },
  hpf: 0,
  monitor: true,
  monitor_mix: 0,
  effects: {
    gate: effect('gate', false),
    compressor: effect('compressor', true),
    exciter: effect('exciter', true),
    bigbottom: effect('bigbottom', true),
  },
  gain: { value: 24, min: 0, max: 24, db_min: 0, db_max: 24 },
};

/** What `revert` goes back to */
let saved = structuredClone(mic);

// ?mock=disconnected | no-permission | error shows the other screens, ?mock=update a firmware notice
const scenario = new URLSearchParams(location.search).get('mock');

const commands: Record<string, (args: any) => unknown> = {
  get_status: (): Status => {
    if (scenario === 'disconnected') return { status: 'disconnected' };
    if (scenario === 'no-permission') return { status: 'no-permission', path: '/dev/hidraw10' };
    if (scenario === 'error') return { status: 'error', message: 'microphone did not answer' };
    return { status: 'connected', ...structuredClone(mic) };
  },
  set_hpf: ({ mode }) => void (mic.hpf = mode),
  set_monitor: ({ on }) => void (mic.monitor = on),
  set_monitor_mix: ({ value }) => void (mic.monitor_mix = value),
  set_effect_enabled: ({ effect: id, on }: { effect: EffectId; on: boolean }) => {
    Object.assign(mic.effects[id], { enabled: on, unset: on ? false : mic.effects[id].unset });
    return structuredClone(mic.effects[id]);
  },
  set_effect_value: ({ effect: id, field, value }: { effect: EffectId; field: string; value: number }) => {
    mic.effects[id].unset = false;
    mic.effects[id].values[field] = value;
  },
  reset_effect: ({ effect: id }: { effect: EffectId }) => {
    Object.assign(mic.effects[id], { unset: false, values: { ...DEFAULTS[id] } });
    return structuredClone(mic.effects[id]);
  },
  set_gain: ({ value }): Gain => ({ ...Object.assign(mic.gain!, { value }) }),
  save: () => void (saved = structuredClone(mic)),
  revert: (): Snapshot => {
    const { hpf, monitor, monitor_mix, effects } = structuredClone(saved);
    Object.assign(mic, { hpf, monitor, monitor_mix, effects });
    return structuredClone(mic);
  },
  factory_reset: (): Snapshot => {
    mic.hpf = 0;
    Object.assign(mic.gain!, { value: 12 });
    for (const id of Object.keys(DEFAULTS) as EffectId[]) {
      mic.effects[id] = { enabled: id !== 'gate', unset: false, values: { ...DEFAULTS[id] } };
    }
    saved = structuredClone(mic);
    return structuredClone(mic);
  },
  set_meter: ({ on }) => void (metering = on),
  test_record: () => void (recordingSince = Date.now()),
  test_stop: () => (taken = (Date.now() - recordingSince) / 1000),
  test_play: () => void (playing = setTimeout(() => playedListeners.forEach((fn) => fn()), taken * 1000)),
  test_stop_playback: () => clearTimeout(playing),
  check_update: () => (scenario === 'update' ? { latest: '1.1.0', newer: true } : { latest: '1.0.9', newer: false }),
};

let recordingSince = 0;
let taken = 0;
let playing: ReturnType<typeof setTimeout> | undefined;
const playedListeners = new Set<() => void>();

let metering = false;
const meterListeners = new Set<(level: Level) => void>();
let phase = 0;
setInterval(() => {
  if (!metering) return;
  phase += 0.05;
  // ?level=-12 pins the meter, for screenshots
  const pinned = Number(new URLSearchParams(location.search).get('level') ?? NaN);
  const rms = Number.isNaN(pinned) ? -34 + 14 * Math.sin(phase * 2.3) * Math.sin(phase * 0.7) + 3 * Math.random() : pinned;
  meterListeners.forEach((fn) => fn({ rms, peak: Math.min(0, rms + 9) }));
}, 50);

export const backend: Backend = {
  async invoke<T>(command: string, args?: Record<string, unknown>) {
    await new Promise((resolve) => setTimeout(resolve, 15));
    return commands[command](args) as T;
  },
  async listen<T>(event: string, handler: (payload: T) => void) {
    const listeners = event === 'meter' ? meterListeners : event === 'test-played' ? playedListeners : null;
    listeners?.add(handler as never);
    return () => void listeners?.delete(handler as never);
  },
};
