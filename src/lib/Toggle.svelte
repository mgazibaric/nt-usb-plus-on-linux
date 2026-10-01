<script lang="ts">
  interface Props {
    checked: boolean;
    label: string;
    disabled?: boolean;
    onchange: (checked: boolean) => void;
  }
  let { checked, label, disabled = false, onchange }: Props = $props();
</script>

<button
  type="button"
  class="toggle"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onchange(!checked)}
>
  <span class="knob"></span>
</button>

<style>
  .toggle {
    flex: none;
    width: 44px;
    height: 26px;
    padding: 3px;
    border-radius: 13px;
    background: var(--track);
    transition: background 0.15s;
  }
  .knob {
    display: block;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--text);
    transition: transform 0.15s, background 0.15s;
  }
  .toggle[aria-checked='true'] {
    background: var(--accent);
  }
  .toggle[aria-checked='true'] .knob {
    transform: translateX(18px);
    background: var(--accent-ink);
  }
  .toggle:disabled {
    opacity: 0.4;
  }
</style>
