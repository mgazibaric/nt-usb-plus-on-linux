<script lang="ts">
  interface Props {
    label: string;
    value: number;
    min: number;
    max: number;
    step?: number;
    /** Logarithmic travel; min must be above zero */
    log?: boolean;
    disabled?: boolean;
    /** Shown in a muted style, e.g. for a value the mic does not hold yet */
    faint?: boolean;
    format?: (value: number) => string;
    help?: string;
    onchange: (value: number) => void;
  }
  let {
    label,
    value,
    min,
    max,
    step = 1,
    log = false,
    disabled = false,
    faint = false,
    format = String,
    help,
    onchange,
  }: Props = $props();

  const TRAVEL = 1000;
  const clamp = (v: number) => Math.min(max, Math.max(min, v));
  const round3 = (v: number) => Number(v.toPrecision(3));

  let position = $derived(log ? (Math.log(clamp(value) / min) / Math.log(max / min)) * TRAVEL : clamp(value));
  let fill = $derived(log ? position / TRAVEL : (position - min) / (max - min));

  function input(event: Event & { currentTarget: HTMLInputElement }) {
    const p = event.currentTarget.valueAsNumber;
    onchange(log ? clamp(round3(min * Math.pow(max / min, p / TRAVEL))) : p);
  }
</script>

<label class="slider" class:disabled class:faint title={help}>
  <span class="head">
    <span class="name">{label}</span>
    <output>{format(value)}</output>
  </span>
  <input
    type="range"
    min={log ? 0 : min}
    max={log ? TRAVEL : max}
    step={log ? 1 : step}
    value={position}
    {disabled}
    style:--fill="{fill * 100}%"
    oninput={input}
  />
</label>

<style>
  .slider {
    display: block;
    min-width: 0;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    margin-bottom: 6px;
  }
  .name {
    color: var(--muted);
    font-size: 13px;
  }
  output {
    font-weight: 600;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }
  .faint output {
    color: var(--dim);
  }
  .disabled {
    opacity: 0.45;
  }

  input {
    -webkit-appearance: none;
    appearance: none;
    display: block;
    width: 100%;
    height: 20px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }
  input:disabled {
    cursor: default;
  }
  input::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(to right, var(--accent) var(--fill), var(--track) var(--fill));
  }
  input::-webkit-slider-thumb {
    -webkit-appearance: none;
    appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border-radius: 50%;
    background: var(--text);
    box-shadow: 0 1px 4px rgb(0 0 0 / 0.5);
    transition: transform 0.1s;
  }
  input:not(:disabled):active::-webkit-slider-thumb {
    transform: scale(1.2);
  }
  .faint input::-webkit-slider-runnable-track {
    background: linear-gradient(to right, var(--dim) var(--fill), var(--track) var(--fill));
  }
</style>
