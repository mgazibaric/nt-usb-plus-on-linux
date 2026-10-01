<script lang="ts">
  interface Props {
    connected: boolean;
  }
  let { connected }: Props = $props();
</script>

<!-- Own drawing of the mic seen from the front; not a RØDE asset.
     No CSS filters here: WebKitGTK has shown tile artefacts with them. -->
<svg class:offline={!connected} viewBox="0 0 190 300" aria-hidden="true">
  <defs>
    <pattern id="weave" width="5" height="5" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
      <path d="M0 0H5M0 0V5" stroke="#050506" stroke-width="1.7" />
    </pattern>
    <!-- the light inside the head: a strong blue spot near the bottom of the mesh
         that fades out in a circle -->
    <radialGradient id="glow" cx="0.5" cy="0.92" r="0.62" fx="0.5" fy="0.95">
      <stop offset="0" stop-color="#b7d4ff" />
      <stop offset="0.08" stop-color="#4f86ff" />
      <stop offset="0.3" stop-color="#2554e8" stop-opacity="0.75" />
      <stop offset="0.65" stop-color="#1b3fb8" stop-opacity="0.22" />
      <stop offset="1" stop-color="#1b3fb8" stop-opacity="0" />
    </radialGradient>
    <linearGradient id="body" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#0c0c0e" />
      <stop offset="0.35" stop-color="#1d1d21" />
      <stop offset="0.7" stop-color="#131316" />
      <stop offset="1" stop-color="#0a0a0c" />
    </linearGradient>
  </defs>
  <!-- controls and headphone jack sit on the side, so only their profile shows -->
  {#each [190, 226] as y}
    <rect x="137" y={y} width="11" height="22" rx="3" fill="#1d1d21" stroke="#34343a" stroke-width="1.2" />
    <path d="M144.5 {y + 4}v14" stroke="#3f3f46" stroke-width="1.2" stroke-linecap="round" />
  {/each}
  <rect class="metal" x="138" y="255" width="5" height="9" rx="1.2" />
  <!-- head -->
  <rect x="44" y="8" width="102" height="150" rx="32" fill="#0c0c0e" stroke="#26262b" stroke-width="1.5" />
  <rect x="52" y="16" width="86" height="134" rx="26" fill="#1c1c21" />
  <rect class="glow" x="52" y="16" width="86" height="134" rx="26" fill="url(#glow)" />
  <rect x="52" y="16" width="86" height="134" rx="26" fill="url(#weave)" />
  <!-- body -->
  <path d="M51 140h88v118a10 10 0 0 1-10 10H61a10 10 0 0 1-10-10z" fill="url(#body)" />
  <circle class="metal" cx="95" cy="163" r="3.4" />
  <!-- mount -->
  <rect x="79" y="268" width="32" height="10" fill="#0c0c0e" />
  <rect x="72" y="278" width="46" height="14" rx="3" fill="#18181b" />
</svg>

<style>
  svg {
    display: block;
    height: 100%;
    margin: 0 auto;
    transition: opacity 0.3s;
  }
  .glow,
  .metal {
    transition: opacity 0.3s, fill 0.3s;
  }
  .metal {
    fill: #c9a96a;
  }
  /* no light while the mic is unplugged */
  .offline {
    opacity: 0.3;
  }
  .offline .glow {
    opacity: 0;
  }
  .offline .metal {
    fill: #45454c;
  }
</style>
