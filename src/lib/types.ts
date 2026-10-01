export type EffectId = 'gate' | 'compressor' | 'exciter' | 'bigbottom';

export interface EffectState {
  enabled: boolean;
  /** The mic has never been given parameters for this effect; values are defaults. */
  unset: boolean;
  values: Record<string, number>;
}

export interface Gain {
  value: number;
  min: number;
  max: number;
  db_min: number;
  db_max: number;
}

export interface Snapshot {
  info: { name: string; serial: string; firmware: string; hidraw: string; card: number | null };
  /** 0 off, 1 = 75 Hz, 2 = 150 Hz */
  hpf: number;
  monitor: boolean;
  monitor_mix: number;
  effects: Record<EffectId, EffectState>;
  gain: Gain | null;
}

export type Status =
  | { status: 'disconnected' }
  | { status: 'no-permission'; path: string }
  | { status: 'error'; message: string }
  | ({ status: 'connected' } & Snapshot);

/** Result of comparing the mic's firmware with RØDE's update list */
export interface Release {
  latest: string;
  newer: boolean;
}

export interface Monitor {
  monitor: boolean;
  monitor_mix: number;
}

/** dBFS */
export interface Level {
  peak: number;
  rms: number;
}
