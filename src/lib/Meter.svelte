<script lang="ts">
  import { onMeter } from './api';

  interface Props {
    active: boolean;
  }
  let { active }: Props = $props();

  const FLOOR = -60;
  const HOLD_MS = 1200;
  const TICKS = [-48, -36, -24, -12, -6, 0];

  let rms = $state(FLOOR);
  let peak = $state(FLOOR);
  let held = $state(FLOOR);
  let heldAt = 0;

  // dBFS -> fraction of the bar
  const at = (db: number) => Math.min(1, Math.max(0, 1 - db / FLOOR));

  $effect(() => {
    if (!active) {
      rms = peak = held = FLOOR;
      return;
    }
    return onMeter((level) => {
      rms = level.rms;
      peak = level.peak;
      const now = performance.now();
      if (level.peak >= held || now - heldAt > HOLD_MS) {
        held = level.peak;
        heldAt = now;
      }
    });
  });
</script>

<div class="meter" class:off={!active} role="meter" aria-label="Input level" aria-valuemin={FLOOR} aria-valuemax={0} aria-valuenow={Math.round(peak)}>
  <div class="bar">
    <div class="fill peak" style:clip-path="inset(0 {(1 - at(peak)) * 100}% 0 0)"></div>
    <div class="fill" style:clip-path="inset(0 {(1 - at(rms)) * 100}% 0 0)"></div>
    {#if active && held > FLOOR}
      <div class="held" class:clip={held > -1} style:left="{at(held) * 100}%"></div>
    {/if}
  </div>
  <div class="scale" aria-hidden="true">
    {#each TICKS as tick}
      <span style:left="{at(tick) * 100}%">{tick}</span>
    {/each}
  </div>
</div>

<style>
  .meter {
    padding-bottom: 16px;
    position: relative;
  }
  .bar {
    position: relative;
    height: 8px;
    border-radius: 4px;
    background: var(--track);
    overflow: hidden;
  }
  .fill {
    position: absolute;
    inset: 0;
    /* -12 dBFS is at 80 %, -6 at 90 % */
    background: linear-gradient(to right, var(--ok) 0 78%, var(--warn) 82% 90%, var(--danger) 94%);
    transition: clip-path 0.06s linear;
  }
  .fill.peak {
    opacity: 0.35;
  }
  .held {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -2px;
    background: var(--text);
  }
  .held.clip {
    background: var(--danger);
  }
  .scale span {
    position: absolute;
    bottom: 0;
    transform: translateX(-50%);
    font-size: 10px;
    color: var(--dim);
  }
  .scale span:last-child {
    transform: translateX(-100%);
  }
  .off {
    opacity: 0.4;
  }
</style>
