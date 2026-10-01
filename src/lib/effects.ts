import type { EffectId } from './types';

export interface FieldMeta {
  id: string;
  label: string;
  min: number;
  max: number;
  step?: number;
  /** Logarithmic slider travel */
  log?: boolean;
  format: (value: number) => string;
  help: string;
}

export interface EffectMeta {
  id: EffectId;
  name: string;
  summary: string;
  fields: FieldMeta[];
}

const num = (value: number, digits: number) => value.toFixed(digits).replace('-', '−');
const dB = (digits = 0) => (v: number) => `${num(v, digits)} dB`;
const percent = (v: number) => `${num(v, 0)} %`;
const hertz = (v: number) => `${num(v, 0)} Hz`;
const time = (v: number) => (v >= 1000 ? `${num(v / 1000, 2)} s` : `${num(v, v < 10 ? 1 : 0)} ms`);

export const EFFECTS: EffectMeta[] = [
  {
    id: 'gate',
    name: 'Noise Gate',
    summary: 'Turns the microphone down while you are not speaking, so background noise stays out of the pauses.',
    fields: [
      { id: 'threshold', label: 'Threshold', min: -80, max: 0, format: dB(),
        help: 'Level at which the gate opens. Set it just above your background noise.' },
      { id: 'range', label: 'Range', min: -60, max: 0, format: dB(),
        help: 'How far the signal is turned down while the gate is closed.' },
      { id: 'attack', label: 'Attack', min: 1, max: 200, log: true, format: time,
        help: 'How fast the gate opens once the signal exceeds the threshold.' },
      { id: 'hold', label: 'Hold', min: 10, max: 2000, log: true, format: time,
        help: 'How long the gate stays open after the signal drops below the threshold.' },
      { id: 'release', label: 'Release', min: 10, max: 2000, log: true, format: time,
        help: 'How fast the gate closes after the hold time.' },
      { id: 'hysteresis', label: 'Hysteresis', min: 1, max: 8, step: 0.1, format: dB(1),
        help: 'How far below the threshold the signal must fall before the gate closes. More keeps quiet word endings, but lets more noise through.' },
    ],
  },
  {
    id: 'compressor',
    name: 'Compressor',
    summary: 'Evens out your level by turning down the loudest parts, then raising everything with the make-up gain.',
    fields: [
      { id: 'threshold', label: 'Threshold', min: -60, max: 0, format: dB(),
        help: 'Level above which the compressor starts to work.' },
      { id: 'ratio', label: 'Ratio', min: 1.5, max: 4.5, step: 0.1, format: (v) => `${num(v, 1)} : 1`,
        help: 'How strongly the signal above the threshold is reduced.' },
      { id: 'attack', label: 'Attack', min: 0.1, max: 10, log: true, format: time,
        help: 'How fast the compressor reacts to a loud signal.' },
      { id: 'release', label: 'Release', min: 5, max: 200, log: true, format: time,
        help: 'How long the compressor keeps working after the signal drops.' },
      { id: 'gain', label: 'Gain', min: 0, max: 9, step: 0.5, format: dB(1),
        help: 'Make-up gain applied after compression.' },
    ],
  },
  {
    id: 'exciter',
    name: 'Aural Exciter',
    summary: 'APHEX processing that adds harmonics for more detail and clarity without raising the level.',
    fields: [
      { id: 'mix', label: 'Harmonics', min: 0, max: 100, format: percent,
        help: 'Amount of added harmonics, i.e. the intensity of the effect.' },
      { id: 'tune', label: 'Tune', min: 600, max: 5000, step: 50, format: hertz,
        help: 'Frequency range of the effect. Lower adds presence in the mids, higher sounds airier.' },
    ],
  },
  {
    id: 'bigbottom',
    name: 'Big Bottom',
    summary: 'APHEX processing that adds low-end weight and sustain without the level boost of an equaliser.',
    fields: [
      { id: 'drive', label: 'Drive', min: 0, max: 100, format: percent,
        help: 'Intensity of the effect, i.e. how much bass sustain is added.' },
      { id: 'tune', label: 'Tune', min: 60, max: 312, step: 2, format: hertz,
        help: 'Upper end of the affected range. Lower keeps the effect on the deepest bass.' },
    ],
  },
];

export const HPF_MODES = ['Off', '75 Hz', '150 Hz'];
