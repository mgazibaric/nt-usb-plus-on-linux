<script lang="ts">
  import { onMount } from 'svelte';
  import * as api from './lib/api';
  import { EFFECTS, HPF_MODES } from './lib/effects';
  import Meter from './lib/Meter.svelte';
  import MicArt from './lib/MicArt.svelte';
  import { LatestQueue } from './lib/queue';
  import Slider from './lib/Slider.svelte';
  import Toggle from './lib/Toggle.svelte';
  import type { EffectId, Release, Snapshot, Status } from './lib/types';

  const METER_PREF = 'meter';
  /** Longest test recording; the backend stops keeping audio at the same point */
  const TEST_MAX_S = 30;

  let status = $state<Status['status'] | 'loading'>('loading');
  let mic = $state<Snapshot | null>(null);
  let problem = $state('');
  let selected = $state<EffectId>('gate');
  /** Settings were changed since the last "Save To Microphone" */
  let dirty = $state(false);
  let saving = $state(false);
  let reverting = $state(false);
  /** Counts changes, to notice one that arrives while saving or resetting */
  let edits = 0;
  /** What "Reset" goes back to: the settings at connect or at the last save */
  let baseline: Snapshot | null = null;
  let test = $state<'idle' | 'recording' | 'playing'>('idle');
  let testSeconds = $state(0);
  let hasTake = $state(false);
  let testTimer: ReturnType<typeof setInterval> | undefined;
  /** How the mic's firmware compares with RØDE's latest release; null while unknown */
  let release = $state<Release | null>(null);
  /** Who moved the direct-monitor mix last; the mic applies whichever came last */
  let mixSource = $state<'dial' | 'app' | null>(null);
  let meterOn = $state(readMeterPref());
  let toast = $state<{ text: string; error: boolean } | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | undefined;

  let shown = $derived(EFFECTS.find((e) => e.id === selected)!);
  let shownState = $derived(mic?.effects[selected]);
  let gainDb = $derived.by(() => {
    const g = mic?.gain;
    if (!g) return 0;
    return g.max === g.min ? g.db_min : g.db_min + ((g.value - g.min) / (g.max - g.min)) * (g.db_max - g.db_min);
  });

  const queue = new LatestQueue(fail);

  function readMeterPref(): boolean {
    try {
      return localStorage.getItem(METER_PREF) !== 'off';
    } catch {
      return true;
    }
  }

  function notify(text: string, error = false) {
    toast = { text, error };
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), error ? 6000 : 2500);
  }

  function fail(error: unknown) {
    notify(String(error), true);
    // Show what the mic actually holds now.
    void refresh();
  }

  async function refresh() {
    let next: Status;
    try {
      next = await api.getStatus();
    } catch (error) {
      next = { status: 'error', message: String(error) };
    }
    const wasConnected = !!mic;
    status = next.status;
    mic = next.status === 'connected' ? next : null;
    problem = next.status === 'error' ? next.message : next.status === 'no-permission' ? next.path : '';
    if (!mic) {
      dirty = false;
      baseline = null;
      release = null;
      mixSource = null;
      clearInterval(testTimer);
      test = 'idle';
    } else if (!wasConnected) {
      baseline = $state.snapshot(mic);
      void api.checkUpdate().then(
        (result) => (release = result),
        () => {},
      );
    }
  }

  /** Apply a change to the mic; `key` groups changes that replace each other. */
  function change(key: string, task: () => Promise<unknown>) {
    dirty = true;
    edits++;
    queue.push(key, task);
  }

  function setHpf(mode: number) {
    if (!mic) return;
    mic.hpf = mode;
    change('hpf', () => api.setHpf(mode));
  }

  function setMonitor(on: boolean) {
    if (!mic) return;
    mic.monitor = on;
    // Like the gain, direct monitoring is a live control: the mic does not save it.
    queue.push('monitor', () => api.setMonitor(on));
  }

  function setMonitorMix(value: number) {
    if (!mic) return;
    mic.monitor_mix = value;
    mixSource = 'app';
    queue.push('monitor-mix', () => api.setMonitorMix(value));
  }

  function setGain(value: number) {
    if (!mic?.gain) return;
    mic.gain.value = value;
    // The gain is a sound-card control of the computer, not a setting saved by the mic.
    queue.push('gain', () => api.setGain(value));
  }

  function setEnabled(id: EffectId, on: boolean) {
    if (!mic) return;
    mic.effects[id].enabled = on;
    change(`${id}.enabled`, async () => {
      const state = await api.setEffectEnabled(id, on);
      if (mic) mic.effects[id] = state;
    });
  }

  function setValue(id: EffectId, field: string, value: number) {
    if (!mic) return;
    mic.effects[id].values[field] = value;
    mic.effects[id].unset = false;
    change(`${id}.${field}`, () => api.setEffectValue(id, field, value));
  }

  function loadDefaults(id: EffectId) {
    change(`${id}.reset`, async () => {
      const state = await api.resetEffect(id);
      if (mic) mic.effects[id] = state;
    });
  }

  async function save() {
    saving = true;
    const at = edits;
    try {
      await queue.idle();
      const saved = $state.snapshot(mic);
      await api.save();
      baseline = saved;
      if (edits === at) dirty = false;
      notify('Saved to microphone');
    } catch (error) {
      fail(error);
    } finally {
      saving = false;
    }
  }

  /** Discard the unsaved changes. */
  async function revert() {
    if (!mic) return;
    // Show the result at once; the mic follows within a fraction of a second,
    // and the controls are inert until it has.
    reverting = true;
    const { gain, monitor, monitor_mix } = mic;
    if (baseline) mic = { ...structuredClone(baseline), gain, monitor, monitor_mix };
    dirty = false;
    try {
      await queue.idle();
      const restored = await api.revert();
      if (mic) mic = restored;
      baseline = $state.snapshot(restored);
      notify('Changes discarded');
    } catch (error) {
      dirty = true;
      fail(error);
    } finally {
      reverting = false;
    }
  }

  async function record() {
    try {
      await api.testRecord();
    } catch (error) {
      return fail(error);
    }
    test = 'recording';
    testSeconds = 0;
    const started = performance.now();
    testTimer = setInterval(() => {
      testSeconds = Math.floor((performance.now() - started) / 1000);
      if (testSeconds >= TEST_MAX_S) void stopRecording();
    }, 200);
  }

  /** End the test recording and play it back. */
  async function stopRecording() {
    clearInterval(testTimer);
    if (test !== 'recording') return;
    test = 'idle';
    try {
      hasTake = (await api.testStop()) >= 0.5;
      if (hasTake) await play();
      else notify('That was too short to play back', true);
    } catch (error) {
      fail(error);
    }
  }

  async function play() {
    try {
      await api.testPlay();
      test = 'playing';
    } catch (error) {
      fail(error);
    }
  }

  function stopPlayback() {
    test = 'idle';
    void api.testStopPlayback();
  }

  function setMeter(on: boolean) {
    meterOn = on;
    try {
      localStorage.setItem(METER_PREF, on ? 'on' : 'off');
    } catch {
      // the preference just will not be remembered
    }
    void api.setMeter(on).catch(fail);
  }

  onMount(() => {
    void api.setMeter(meterOn).then(refresh, refresh);
    const stop = [
      api.onDeviceChanged(() => void refresh()),
      api.onGain((gain) => {
        if (mic) mic.gain = gain;
      }),
      api.onMonitor((monitor) => {
        if (!mic) return;
        if (monitor.monitor_mix !== mic.monitor_mix) {
          // Nothing but the dial on the mic changes the mix behind the app's back.
          if (mixSource === 'app') notify('Mix is now set by the dial on the mic');
          mixSource = 'dial';
        }
        Object.assign(mic, monitor);
      }),
      api.onTestPlayed(() => {
        if (test === 'playing') test = 'idle';
      }),
    ];
    return () => stop.forEach((off) => off());
  });
</script>

<div class="app">
  <aside>
    <div class="brand">NT-USB+ <span>Control</span></div>

    <div class="art"><MicArt connected={!!mic} /></div>

    {#if mic}
      <div class="device">
        <div class="name">{mic.info.name || 'NT-USB+'}</div>
        <div class="state"><i class="dot"></i>Connected</div>
      </div>
      <dl>
        <dt>Firmware</dt>
        <dd>
          {mic.info.firmware}
          {#if release && !release.newer}
            <span class="current" title="RØDE's update list names {release.latest} as the latest release">· up to date</span>
          {/if}
        </dd>
        <dt>Serial</dt>
        <dd>{mic.info.serial}</dd>
      </dl>
      {#if release?.newer}
        <p class="update" role="status">
          <strong>Firmware {release.latest} is available.</strong> Install it with RØDE Central on Windows or macOS.
        </p>
      {/if}

      <section class="level">
        <div class="level-head">
          <h2>Level Meter</h2>
          <Toggle checked={meterOn} label="Level meter" onchange={setMeter} />
        </div>
        <Meter active={meterOn} />
        <p class="note">{meterOn ? 'Listening to the microphone' : 'Off, not listening'}</p>
      </section>
    {:else}
      <div class="device">
        <div class="name">NT-USB+</div>
        <div class="state off"><i class="dot"></i>{status === 'loading' ? 'Searching…' : 'Not connected'}</div>
      </div>
    {/if}

    <footer>Unofficial tool, not affiliated with RØDE.</footer>
  </aside>

  <main inert={reverting}>
    {#if mic && shownState}
      <header>
        <div>
          <h1>Settings</h1>
          <p class="sub">Changes apply immediately.</p>
        </div>
        <div class="save">
          {#if dirty}
            <span class="unsaved">Unsaved changes</span>
            <button class="ghost" type="button" disabled={saving || reverting} onclick={revert} title="Discard the unsaved changes and go back to the last saved settings">
              Reset
            </button>
          {/if}
          <button class="primary" type="button" class:attention={dirty} disabled={saving || reverting} onclick={save} title="Store the current settings on the microphone">
            {saving ? 'Saving…' : 'Save To Microphone'}
          </button>
        </div>
      </header>

      <section class="row">
        <div class="card">
          <div class="card-head"><h2>Input Level</h2></div>
          {#if mic.gain}
            <Slider
              label="Gain"
              value={mic.gain.value}
              min={mic.gain.min}
              max={mic.gain.max}
              format={() => `+${gainDb.toFixed(0)} dB`}
              onchange={setGain}
            />
          {:else}
            <p class="note">The sound card of the microphone was not found, so the gain cannot be set here.</p>
          {/if}
          <p class="note">The same control as the input volume in your system's sound settings.</p>
        </div>

        <div class="card">
          <div class="card-head">
            <h2>Direct Monitor</h2>
            <Toggle checked={mic.monitor} label="Direct monitoring" onchange={setMonitor} />
          </div>
          <Slider
            label="Mix"
            value={mic.monitor_mix}
            min={0}
            max={100}
            disabled={!mic.monitor}
            faint={mic.monitor && mixSource === 'dial'}
            format={(v) => `${v} %`}
            onchange={setMonitorMix}
          />
          <p class="note">
            Your own voice in the mic's headphone output.
            <span class="source" class:dial={mic.monitor && mixSource === 'dial'}>
              {#if !mic.monitor}
                Switched off here; the dial on the mic has no effect.
              {:else if mixSource === 'dial'}
                Mix is set by the dial on the mic.
              {:else if mixSource === 'app'}
                Mix is set here until the dial on the mic is turned.
              {:else}
                Mix follows this slider or the dial on the mic, whichever moved last.
              {/if}
            </span>
          </p>
        </div>

        <div class="card">
          <div class="card-head"><h2>High-Pass Filter</h2></div>
          <div class="segments" role="radiogroup" aria-label="High-pass filter">
            {#each HPF_MODES as name, mode}
              <button type="button" role="radio" aria-checked={mic.hpf === mode} onclick={() => setHpf(mode)}>{name}</button>
            {/each}
          </div>
          <p class="note">Removes rumble and handling noise below the chosen frequency.</p>
        </div>
      </section>

      <section class="processing">
        <div class="section-head">
          <h2>Processing</h2>
          <span class="badge">APHEX</span>
          <div class="test">
            <span class="note">Test recording</span>
            {#if test === 'recording'}
              <button class="ghost small" type="button" onclick={stopRecording} title="Stop and play it back">
                <i class="rec"></i>Stop · 0:{String(testSeconds).padStart(2, '0')}
              </button>
            {:else if test === 'playing'}
              <button class="ghost small" type="button" onclick={stopPlayback}>Stop Playback</button>
            {:else}
              {#if hasTake}
                <button class="ghost small" type="button" onclick={play}>Play Again</button>
              {/if}
              <button
                class="ghost small"
                type="button"
                onclick={record}
                title="Record yourself for up to {TEST_MAX_S} seconds and hear it played back on the computer's audio output, as the microphone delivers it with the current settings"
              >
                Record
              </button>
            {/if}
          </div>
        </div>

        <div class="tiles">
          {#each EFFECTS as effect (effect.id)}
            {@const state = mic.effects[effect.id]}
            <div class="tile" class:selected={selected === effect.id} class:on={state.enabled}>
              <button class="pick" type="button" aria-pressed={selected === effect.id} onclick={() => (selected = effect.id)}>
                <span class="tile-name">{effect.name}</span>
                <span class="tile-state">{state.enabled ? 'On' : 'Off'}</span>
              </button>
              <Toggle
                checked={state.enabled}
                label={effect.name}
                onchange={(on) => {
                  selected = effect.id;
                  setEnabled(effect.id, on);
                }}
              />
            </div>
          {/each}
        </div>

        <div class="card detail">
          <div class="card-head">
            <div>
              <h3>{shown.name}</h3>
              <p class="note">{shown.summary}</p>
            </div>
            <button class="ghost" type="button" onclick={() => loadDefaults(selected)} title="Write the default values to the microphone">Defaults</button>
          </div>
          <div class="sliders">
            {#each shown.fields as field (selected + field.id)}
              <Slider
                label={field.label}
                value={shownState.values[field.id]}
                min={field.min}
                max={field.max}
                step={field.step}
                log={field.log}
                faint={shownState.unset}
                format={field.format}
                help={field.help}
                onchange={(v) => setValue(selected, field.id, v)}
              />
            {/each}
          </div>
          {#if shownState.unset}
            <p class="note hint">Never configured on this microphone. These defaults are written when you switch the effect on or move a slider.</p>
          {/if}
        </div>
      </section>
    {:else if status === 'loading'}
      <div class="empty"><p class="muted">Looking for the microphone…</p></div>
    {:else if status === 'no-permission'}
      <div class="empty">
        <h1>No access to the microphone</h1>
        <p class="muted">
          The NT-USB+ is connected, but its control interface <code>{problem}</code> is only accessible to root. Install the udev rule from the
          project folder, then try again:
        </p>
        <pre>sudo install -m 644 udev/70-rode-nt-usb-plus.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=hidraw --action=add</pre>
        <button class="primary" type="button" onclick={refresh}>Try Again</button>
      </div>
    {:else if status === 'error'}
      <div class="empty">
        <h1>Something went wrong</h1>
        <p class="muted">{problem}</p>
        <button class="primary" type="button" onclick={refresh}>Try Again</button>
      </div>
    {:else}
      <div class="empty">
        <h1>Connect your NT-USB+</h1>
        <p class="muted">Plug the microphone into a USB port. It is detected automatically.</p>
      </div>
    {/if}
  </main>

  {#if toast}
    <div class="toast" class:error={toast.error} role="status">{toast.text}</div>
  {/if}
</div>

<style>
  .app {
    display: grid;
    grid-template-columns: 232px 1fr;
    height: 100%;
  }

  /* sidebar */
  aside {
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 22px 20px 16px;
    background: var(--panel);
    border-right: 1px solid var(--line);
    min-height: 0;
    overflow-y: auto;
  }
  /* only the picture gives way when the window is short */
  aside > * {
    flex-shrink: 0;
  }
  .brand {
    font-weight: 700;
    font-size: 15px;
    letter-spacing: 0.02em;
  }
  .brand span {
    color: var(--dim);
    font-weight: 500;
  }
  .art {
    flex: 0 1 250px;
    min-height: 110px;
    padding: 6px 0;
  }
  .device .name {
    font-size: 17px;
    font-weight: 600;
  }
  .state {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 2px;
    font-size: 12.5px;
    color: var(--muted);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  .state.off .dot {
    background: var(--dim);
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 5px 14px;
    margin: 0;
    font-size: 12.5px;
  }
  dt {
    color: var(--dim);
  }
  dd {
    margin: 0;
    text-align: right;
    color: var(--muted);
    user-select: text;
    -webkit-user-select: text;
  }
  .level {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 16px;
    border-top: 1px solid var(--line);
  }
  .level-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .current {
    color: var(--dim);
  }
  .update {
    padding: 10px 12px;
    border: 1px solid var(--accent);
    border-radius: 10px;
    font-size: 12px;
    line-height: 1.45;
    color: var(--muted);
  }
  .update strong {
    display: block;
    color: var(--text);
    font-weight: 600;
  }
  footer {
    margin-top: auto;
    font-size: 11px;
    color: var(--dim);
  }

  /* main */
  main {
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding: 22px 26px 26px;
    min-width: 0;
    overflow-y: auto;
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 16px;
  }
  h1 {
    font-size: 22px;
    font-weight: 650;
    letter-spacing: -0.01em;
  }
  h2 {
    font-size: 14px;
    font-weight: 600;
  }
  h3 {
    font-size: 16px;
    font-weight: 600;
  }
  .sub,
  .muted {
    color: var(--muted);
  }
  .sub {
    font-size: 12.5px;
  }
  .save {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .unsaved {
    font-size: 12.5px;
    color: var(--accent);
  }

  .primary,
  .ghost {
    height: 36px;
    padding: 0 18px;
    border-radius: 18px;
    font-weight: 600;
    font-size: 13px;
    white-space: nowrap;
    transition: background 0.15s, opacity 0.15s;
  }
  .save .primary {
    min-width: 172px;
  }
  .primary {
    background: var(--text);
    color: var(--accent-ink);
  }
  .primary:hover {
    background: #e4e4e7;
  }
  .primary.attention,
  .primary.attention:hover {
    background: var(--accent);
  }
  .ghost {
    border: 1px solid var(--line);
    color: var(--text);
  }
  .ghost:hover {
    background: var(--card-hi);
  }
  button:disabled {
    opacity: 0.5;
  }

  /* cards */
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1.35fr) minmax(0, 1fr) minmax(0, 1fr);
    gap: 14px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px 18px 18px;
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    min-width: 0;
  }
  .card-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
    min-height: 26px;
  }
  .note {
    font-size: 12.5px;
    color: var(--dim);
  }
  .card > .note {
    margin-top: auto;
  }
  .source {
    display: block;
    margin-top: 4px;
    color: var(--muted);
  }
  /* the dial has taken over: say so in the accent colour */
  .source.dial {
    color: var(--accent);
  }

  .segments {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    padding: 3px;
    border-radius: 12px;
    background: var(--bg);
    border: 1px solid var(--line);
  }
  .segments button {
    height: 32px;
    border-radius: 9px;
    font-size: 13px;
    font-weight: 600;
    color: var(--muted);
    transition: background 0.15s, color 0.15s;
  }
  .segments button:hover {
    color: var(--text);
  }
  .segments button[aria-checked='true'] {
    background: var(--accent);
    color: var(--accent-ink);
  }

  /* processing */
  .processing {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .section-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .section-head h2 {
    font-size: 16px;
  }
  .test {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: auto;
  }
  .test .note {
    margin-right: 4px;
  }
  .ghost.small {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 30px;
    padding: 0 14px;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
  }
  .rec {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--danger);
    animation: pulse 1s ease-in-out infinite alternate;
  }
  @keyframes pulse {
    to {
      opacity: 0.35;
    }
  }
  .badge {
    padding: 2px 7px;
    border: 1px solid var(--line);
    border-radius: 6px;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.12em;
    color: var(--muted);
  }
  .tiles {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
  }
  .tile {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 12px 0 0;
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    transition: border-color 0.15s, background 0.15s;
  }
  .tile.selected {
    border-color: var(--text);
    background: var(--card-hi);
  }
  .pick {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 13px 0 13px 14px;
    text-align: left;
    border-radius: var(--radius);
  }
  .tile-name {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tile-state {
    font-size: 12px;
    color: var(--dim);
  }
  .tile.on .tile-state {
    color: var(--accent);
  }
  .detail {
    gap: 18px;
  }
  .detail .card-head .note {
    margin-top: 3px;
    max-width: 62ch;
  }
  .sliders {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 18px 28px;
  }
  .hint {
    color: var(--muted);
  }

  /* other screens */
  .empty {
    margin: auto;
    max-width: 560px;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 14px;
  }
  pre {
    margin: 0;
    padding: 14px 16px;
    max-width: 100%;
    overflow-x: auto;
    background: var(--card);
    border: 1px solid var(--line);
    border-radius: 10px;
    font-family: ui-monospace, 'JetBrains Mono', 'DejaVu Sans Mono', monospace;
    font-size: 12px;
    line-height: 1.7;
    user-select: text;
    -webkit-user-select: text;
    cursor: text;
  }

  .toast {
    position: fixed;
    left: 50%;
    bottom: 22px;
    transform: translateX(-50%);
    max-width: 70%;
    padding: 10px 18px;
    border-radius: 20px;
    background: var(--text);
    color: var(--accent-ink);
    font-size: 13px;
    font-weight: 600;
    box-shadow: 0 8px 30px rgb(0 0 0 / 0.5);
  }
  .toast.error {
    background: var(--danger);
  }
</style>
