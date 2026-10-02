# RØDE NT-USB+ control protocol

Reverse-engineered from RØDE Central 2.0.111 for macOS (`rode_bridge.framework`,
which ships with C++ symbol names) and checked against a real NT-USB+
(USB `19f7:0035`, firmware 1.0.9). Reference implementation: `tools/rodectl.py`.

See [Verification status](#verification-status) for what has been confirmed on
hardware and what is only known from the disassembly.

## Device layout

| Interface | Class | Purpose |
|---|---|---|
| 0–2 | USB Audio 1.0 | capture (mono, 48 kHz, 16/24 bit), playback (stereo), mixer |
| 3 | HID, vendor usage page `0xFF00` | **control channel** (this document) |
| 5 | vendor specific, no endpoints | unused by RØDE Central's control path |

Firmware version is the USB `bcdDevice` field (`1.09` = v1.0.9); there is no
HID command for it.

### Things that are *not* HID

- **Input level (mic gain)** is the standard USB Audio feature unit. On Linux it
  is the ALSA control `Mic Capture Volume` (0–24 dB, 1 dB steps).
- **Headphone level** is `PCM Playback Volume` (−60–0 dB) / `PCM Playback Switch`.
- `Mic Playback Switch` is declared as the mute of the direct-monitor path, but
  the firmware acknowledges it and does nothing (see "Inside the firmware").
  Direct monitoring is switched with HID param 3.

## HID reports

Interface 3 has one interrupt IN endpoint (`0x81`) and no OUT endpoint, so
output reports are sent as `SET_REPORT` control transfers
(`bmRequestType 0x21, bRequest 0x09, wValue 0x0200 | id, wIndex 3`). Writing to
`/dev/hidraw*` does exactly that.

| Out ID | In ID | Payload | Used for |
|---|---|---|---|
| 2 | 1 | 9 bytes | **bootloader / firmware update — never send** |
| 4 | 3 | 28 bytes | Aphex DSP (compressor, gate, exciter, big bottom) |
| 6 | 5 | 14 bytes | not used for the NT-USB+ |
| 8 | 7 | 27 bytes | device parameters |

Every exchange is one request and one reply:

```
host -> mic   <out id> <cmd> <payload ...>          zero padded to full length
mic  -> host  <in id>  <cmd> 41 <reply payload ...>
```

The reply echoes the first payload byte (`cmd`) and then a status byte.
RØDE Central waits up to 2 s for a matching reply. One exchange takes about
16 ms (the interrupt endpoint is polled every 16 ms), so reading all settings
takes about a third of a second.

| Status | Meaning |
|---|---|
| `41` `'A'` | ACK |
| `45` `'E'` | value out of range (e.g. high-pass = 3, monitor mix = 101) |
| `4E` `'N'` | not supported (e.g. reading the write-only param 2) |

Only report 8 validates its input. Report 4 ACKs unknown effect, operation and
parameter numbers and returns zeros, so send only the ones listed here.

### Report 2: bootloader (do not use)

Single ASCII command bytes. Nothing in this project sends them, and none of
this has been tried on a mic. What RØDE Central's updater
(`DefaultFirmwareUpdater`) does for the NT-USB+, read from the disassembly:

1. `R` (0x52) to the running mic. It restarts into its bootloader and shows up
   as USB `19f7:0036` with the same serial number.
2. `I` (0x49): the reply carries the bootloader version and the flash page size.
3. For each 1 KiB page of the image, counting from 0: `E` (0x45) plus the 16-bit
   page number erases it, then `P` (0x50) plus the page data on report 3 writes
   it. The last page is padded with `FF`. Every reply must be `A`.
4. `R` again; the mic comes back as `19f7:0035`.

The app does not read the flash back. `W` (0x57) is for RØDE devices without a
separate bootloader id, not for this mic.

The image is embedded in RØDE Central (`NTUSBplus_v1_0_9.bin`, 44,032 bytes):
plain Cortex-M code linked at `0x08004000`, not encrypted, not signed, with a
CRC-32 of everything before it in the last four bytes. The 16 KiB below that
address hold the bootloader, which this procedure never writes; RØDE Central
replaces bootloaders only on the VideoMic NTG. Whether the bootloader checks
the CRC before starting the firmware is not known.

RØDE's update manifest (`https://update.rode.com/rode-devices-manifest.json`,
key `ntusbplus-manifest`) named 1.0.9 as the current release on 2026-09-30.

## Report 8: device parameters

```
write:  08 <param> 00 <value> 00 ...
read:   08 <param> 01 00 ...          ->  07 <param> 41 <value>
```

| Param | Name | Values | Notes |
|---|---|---|---|
| 1 | High-pass filter | 0 off, 1 = 75 Hz, 2 = 150 Hz | |
| 2 | Windscreen mode | 0 / 1 | used by RØDE's library for other mics; firmware 1.0.9 answers `'N'` to reads and writes |
| 3 | Direct monitoring | 0 off, 1 on | not saved by param 10; on after power-up |
| 4 | Direct monitor mix | 0–100 | level of the mic in the headphone output; the mix dial on the mic writes the same value |
| 5 | Blink | 0 / 1 | accepted and readable, but nothing on the NT-USB+ blinks (see below) |
| 6 | Session ping | app id 0 or 1 | keep-alive of a temporary session, see below. RØDE Central does not send it. Not needed |
| 7 | unknown | | the firmware reads and writes 24-byte records with an index 0–3 here; not used by RØDE Central for this mic, never sent |
| 10 | Save to NVM | 0 | stores the high-pass mode and the four effects in flash; the ACK takes about 1.4 s |

When direct monitoring is switched on, RØDE's library first re-sends param 4
and then param 3. RØDE Central shows no direct-monitor control for this mic.

### Inside the firmware

Read from the disassembly of firmware 1.0.9; the routines for params 3 and 4
were checked twice, the rest once. Direct monitoring, the mix dial, the
processed signal in the headphones and what a save keeps were confirmed on the
mic on 2026-10-01, by ear and by replugging.

- **Direct monitoring** works without a session. Param 3 is a flag the audio
  routine reads directly. Param 4 sets a gain of `66 * value / 100 − 60` dB for
  the mic in the headphone output, so 0 is practically silent and 100 is +6 dB.
  It is a level, not a balance with the computer audio.
- **The mix dial** on the mic calls the same routine as param 4 whenever it is
  turned, so reading param 4 shows the dial position, and a value written over
  HID holds until the dial is turned again (or a save or the end of a session
  re-applies the dial). The headphone-level dial sets a separate gain that
  cannot be read over HID.
- **Save** (param 10) writes an 80-byte block: the high-pass mode and the four
  effect blocks. Params 3 and 4 are not in it. After power-up direct monitoring
  is on and the mix is taken from the dial. The routine mutes the audio and
  waits about a second, hence the slow ACK.
- **Session** (param 6, app id 0 or 1): while pings arrive, changes are not
  copied into the block that param 10 saves; app id 1 also disables the mix
  dial. About 3.5 s after the last ping the session ends: the effects and the
  high-pass are restored from the saved block, direct monitoring is switched
  on, blinking stops and the dials are applied again. This is how RØDE Connect
  can make temporary changes. Nothing here uses it.
- **Blink** (param 5): every 500 ms the firmware toggles one output pin. With
  the command left on for over a minute the mic's blue light stayed steady and
  nothing else changed (watched on 2026-09-30), so that pin has no visible
  light on this mic. RØDE Central offers no "identify" for the NT-USB+ either.
- **Audio path**: the microcontroller does the processing itself. Per block:
  high-pass, gate, compressor, exciter / big bottom, then the result goes to
  USB capture, and the same processed buffer, scaled by the monitor gain, is
  added to the headphone output. So the headphones should carry the processed
  signal, not the raw mic.

## Report 4: Aphex DSP

```
04 <fx> <op> <param> <value ...>
```

`fx` is `(channel << 2) | effect`; the NT-USB+ has one channel, so it is just
the effect number.

| fx | Effect |
|---|---|
| 0 | Compressor |
| 1 | Noise gate |
| 2 | Aural Exciter |
| 3 | Big Bottom |

| op | Meaning |
|---|---|
| 0 | set all parameters in one packet (older firmware/mics) |
| 1 | get all parameters in one packet |
| 2 | set one parameter ("granular") |
| 3 | get one parameter |

The NT-USB+ uses the granular operations. Reply to op 3 is
`03 <fx> 41 <value ...>` with the value in the same encoding as the write.
Multi-byte values are little-endian; "Q31" is a signed 32-bit fixed-point
fraction (`value * 2^31`, saturated to `0x7FFFFFFF`). The app rounds the value
to a single-precision float before scaling, so only the top 24 bits are
significant; doing the same reproduces the factory gate values bit for bit.

Several parameters are 8-bit indices into 256-entry lookup tables
(`tools/aphex_tables.json`, taken from the app). `idx(x) = trunc(x * 255)` with
`x` clamped to 0..1.

### Compressor (fx 0)

| Param | Name | Range | Default | Encoding |
|---|---|---|---|---|
| 0 | Enabled | 0 / 1 | | 1 byte |
| 1 | Threshold | −60…0 dB | −25 | int32 `compressor_threshold[idx(-dB / 60)]` |
| 2 | Ratio ("shape") | 1.5…4.5 | 2.0 | 1 byte `idx((ratio − 1.5) / 3)` |
| 3 | Attack | 0.1…10 ms | 0.7 | int32 `compressor_attack[idx(log10(ms / 0.1) / 2)]` |
| 4 | Release | 5…200 ms | 21 | int32 `compressor_release[idx(log10(ms / 5) / log10(40))]` |
| 5 | Make-up gain | 0…9 dB | 3 | int32 `compressor_makeup_gain[idx(dB / 9)]` |

Reading back: look the int32 up in the table to recover the index, then invert
the formula.

A mic that has never had these written returns all zeros for compressor,
exciter and big bottom parameters. Zero is not a valid attack or release table
entry, so treat an all-zero effect as "unset" and write the defaults before
enabling it. The gate comes with its defaults already stored.

#### Compressor, measured

Measured on 2026-10-01, firmware 1.0.9. A 1 kHz tone in 5 dB steps was recorded through the mic with
the compressor off and with several settings (attack 0.7 ms, release 21 ms, make-up 0 dB). The table
gives the change against "off" in dB; the top row is the level in dBFS with the compressor off.

| Setting | −11 | −16 | −21 | −26 | −31 | −36 | −41 | −46 | −51 | −56 |
|---|---|---|---|---|---|---|---|---|---|---|
| Threshold −60 dB, 4.5:1 | −12.5 | −12.4 | −12.1 | −10.9 | −8.5 | −4.7 | −2.8 | −2.0 | −2.2 | −1.7 |
| Threshold −60 dB, 2:1 | −12.5 | −12.1 | −10.2 | −7.4 | −5.7 | −4.6 | −4.5 | −3.2 | −3.1 | −2.1 |
| Threshold −40 dB, 4.5:1 | −11.6 | −8.8 | −5.4 | −2.9 | −1.6 | −0.6 | −0.4 | −0.5 | −0.2 | −0.6 |
| Threshold −25 dB, 4.5:1 | −6.2 | −3.4 | −1.7 | −0.7 | −0.4 | −0.2 | −0.1 | −0.3 | −0.5 | +0.2 |
| Threshold −25 dB, 2:1 (default) | −4.5 | −2.7 | −1.4 | −0.9 | −0.9 | −0.6 | −0.4 | −0.4 | −0.4 | +0.1 |
| Threshold 0 dB, 4.5:1 | −0.2 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | −0.1 | −0.1 | 0.0 |

- The gain reduction stops at about 12.5 dB, whatever the ratio.
- Compression starts well above the labelled threshold: a hard knee fitted to the 4.5:1 rows sits at
  about −42 dBFS for the −60 dB setting, −28 dBFS for −40 dB and −19 dBFS for −25 dB. The knee is soft.
- Below the limit the slopes are close to the labelled ratios (about 3:1 for 4.5:1 just above the
  knee, 1.8:1 for 2:1).
- Make-up gain +3 dB gave +3.0 dB (earlier run on the same day).

Attack and release, from a 3 kHz tone stepping up by 20 dB (−50 to −30 dBFS at the mic) for one
second and back, threshold −60 dB, 4.5:1. Gain over time is the recording with the compressor divided
by one without; "63 %" is the time until 63 % of the final gain change has happened. Time resolution
about 1 ms. The room was noisy (office), which matters for the last column.

| Label | Table entry as `1 / (coefficient · 48000)` | Measured, 63 % / 90 % |
|---|---|---|
| Attack 0.1 ms | 0.2 ms | about 1 ms / 2 ms (at the resolution limit) |
| Attack 0.3 ms | 1.9 ms | 5 ms / 13 ms |
| Attack 1 ms | 14 ms | 31 ms / 104 ms |
| Attack 3 ms | 85 ms | 87 ms / 169 ms |
| Attack 10 ms | 610 ms | first run only: 2 dB of about 10 dB after 400 ms |
| Release 5 ms | 23 ms | about 25 ms / 110 ms |
| Release 21 ms | 150 ms | 56–69 ms / about 125 ms |
| Release 50 ms | 450 ms | not reliable: 170–300 ms |
| Release 100 ms, 200 ms | 1 s, 2.2 s | not reliable: several hundred ms |

So the int32 values in `compressor_attack` and `compressor_release` behave like one-pole
coefficients in Q31 at 48 kHz (attack 0.2…610 ms, release 23 ms…2.2 s), not like the 0.1…10 ms and
5…200 ms that the control ranges name: attack takes 10 to 30 times the label, release 3 to 5 times.
The ranges are RØDE's own: `AphexFxParamsUtils::ToAphexCompressorAttackHidParam` and
`…ReleaseHidParam` in `rode_bridge` assert 0.1…10 and 5…200, normalise logarithmically, multiply by
255 and look the result up in `ATTACK_TABLE` / `RELEASE_TABLE`. The tables themselves are evenly
spaced on a log scale over 0.4…610 ms and 23 ms…2.2 s (entry 0 of the attack table is a step apart
at 0.2 ms), so no single formula turns the labelled ranges into them.

Two more things seen in these runs:

- The steady reduction depends on attack and release together. For the same tone it was 7.5 dB with
  attack 0.3 ms and 4.7 dB with attack 3 ms (release 21 ms both), and in the first run about 2 dB
  instead of 10 dB with attack 10 ms and release 5 ms.
- With attack 0.1 ms and a release label of 21 ms or more, the level of the quiet tone wandered by up
  to 10 dB: the compressor was following the room noise. This is why the long release values could
  not be measured in that room.

### Noise gate (fx 1)

All values except `Enabled` are Q31.

| Param | Name | Default | Encoding |
|---|---|---|---|
| 0 | Enabled | | 1 byte |
| 1 | Threshold | −40 dB | `10^(dB / 20)` |
| 2 | Attack | 30 ms | one-pole coefficient, see below |
| 3 | Hold | 0.05 s | `1 / (seconds * 48000)` |
| 4 | Release | 0.2 s | `1 / (seconds * 48000)` |
| 5 | Range | −9 dB | `10^(dB / 20)` |
| 6 | Hysteresis | 0.2 | `10^((−1 − 7·h) / 20)`, h = 0…1, i.e. −1…−8 dB |

Attack coefficient for a time `t` in seconds:

```
c = cos(5 / (t * 48000))
a = c − 1 + sqrt(c² − 4c + 3)
```

### Aural Exciter (fx 2)

| Param | Name | Range | Default | Encoding |
|---|---|---|---|---|
| 0 | Enabled | 0 / 1 | | 1 byte |
| 1 | Mix ("harmonics") | 0…100 % | 85 | `i = idx(mix / 100)`: int32 `log[i]`, then byte `i` |
| 2 | Tune | 600…5000 Hz | 3500 | `i = idx((Hz − 600) / 4400)`: int32 `hf_filter_freq[i]`, int32 `hf_filter_gain[i]`, then byte `i` |

### Big Bottom (fx 3)

| Param | Name | Range | Default | Encoding |
|---|---|---|---|---|
| 0 | Enabled | 0 / 1 | | 1 byte |
| 1 | Drive | 0…100 % | 80 | `i = idx(drive / 100)`: int32 `log[i]`, then byte `i` |
| 2 | Tune | 60…312 Hz | 90 | 1 byte `idx((Hz − 60) / 252)` |

"Default" in these tables is what RØDE Central's factory reset writes for the
NT-USB+ (see below).

## What RØDE Central does

Its NT-USB+ page offers input level, the high-pass filter, one on/off switch
each for noise gate, compressor, Aural Exciter and Big Bottom, and "Save To
Microphone" (feature list in the app's `assets/json/devices.json`). It has no
parameter sliders and no direct-monitor control for this mic; the library code
for those is shared with other mics and other RØDE apps.

1. On connect, read report-8 params 1, 3, 4 and every granular parameter of the
   four effects.
2. On a change, write only the parameters that differ from what it last read or
   wrote. An effect switch therefore sends just `Enabled`; effect values are
   never written in normal use.
3. "Save to microphone" writes param 10.
4. Factory reset is done by the app with ordinary writes: input level 12 dB,
   high-pass off, the defaults from the effect tables above, then gate off and
   compressor, Big Bottom and Aural Exciter on. The routine sends no save
   and nothing for direct monitoring; the mic has no reset command of its
   own. (Read from `factoryResetDeviceWithSessionInfo`; not tested.) The
   mic this was tested on had all four effects off when first read, with
   only the gate values stored, so the three switches are RØDE Central's
   choice, not the state the mic ships in.

Settings are active as soon as they are written. Only a save (step 3) makes
them survive a power cycle (seen on 2026-10-01: unsaved effect settings were
gone after a replug, saved ones were back).

## Verification status

Tested on an NT-USB+ with firmware 1.0.9. "Measured" means the effect was
observed in the mic's own audio signal, using room noise as the test signal
unless a test tone is named.

| Item | Status |
|---|---|
| Report framing, status bytes, report ids | confirmed |
| Params 1, 3, 4, 5 read and write; param 10 accepted | confirmed |
| High-pass values | measured: 2 removes clearly more low end than 1 (about −19 dB vs −8 dB below 40 Hz) |
| Monitor mix range 0–100 | confirmed (101 is refused) |
| Aphex op 2 / op 3 and all reply layouts | confirmed, values read back byte-identical |
| Gate encodings | confirmed: factory values decode to exactly the app's defaults |
| Gate enable, threshold, range | measured: gate closes to about −57 dB with range −60 dB |
| Gate param 3 = hold, param 4 = release | measured: 2 s hold keeps the level flat for 2 s; 2 s release ramps it down over 2 s |
| Compressor gain, threshold, ratio | measured on 2026-10-01 with a 1 kHz tone played through the mic's headphone output into the capsule, see "Compressor, measured" below. It compresses, make-up gain is exact, and the gain reduction never exceeds about 12.5 dB |
| Compressor attack and release tables | measured on 2026-10-01 with a stepped 3 kHz tone, see "Compressor, measured": both act in the right direction, but attack takes 10 to 30 times and release 3 to 5 times the labelled time. Release labels above 21 ms could not be measured reliably (noisy room) |
| Exciter and big bottom audible effect | heard by the user on 2026-10-01 (test recording in the app); not measured |
| Windscreen mode (param 2) | not supported by this firmware (from the disassembly; reads return `'N'`) |
| Blink (param 5) | accepted and reads back; nothing visible happens on the mic |
| Mix dial writes param 4 | confirmed: the value read over HID follows the dial |
| Persistence of param 10 across a replug | confirmed by the user on 2026-10-01: saved settings are back after a replug |
| Direct monitoring | heard on 2026-10-01: the dial changes the level of the own voice. Measured: computer audio keeps the same level in the headphone output with param 3 on or off and with param 4 at 0. After a power cycle param 3 read 1 and param 4 the dial position. The user's checks on 2026-10-01: with param 3 off the own voice is gone from the headphones; the voice is nearly silent at a low mix and loud at 100; the gate (threshold 0 dB, range −60 dB) also silences the voice in the headphones, so they carry the processed signal. Still from the disassembly only: session behaviour |
| Settings across a power cycle | seen on 2026-10-01: effect settings that had not been saved were gone after unplugging the mic's cable; the mic came back with the block from the last save |
| Firmware update procedure (report 2) | from disassembly only, never sent |
