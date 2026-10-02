# rode-linux

Control a RØDE NT-USB+ on Linux without RØDE Central. Unofficial, not affiliated with RØDE.

- `src/`, `src-tauri/` — the desktop app (Tauri 2, Svelte 5, Rust)
- `docs/PROTOCOL.md` — the reverse-engineered HID protocol
- `docs/TODO.md` — what is still untested or missing
- `tools/rodectl.py` — reference command-line implementation (Python standard library only)
- `udev/70-rode-nt-usb-plus.rules` — grants the logged-in user access to the control interface

## Setup

The control interface is only accessible to root until the udev rule is installed:

```sh
sudo install -m 644 udev/70-rode-nt-usb-plus.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=hidraw --action=add
```

## App

Needs Rust, Node with pnpm, `webkit2gtk-4.1` and `alsa-lib`.

```sh
pnpm install
pnpm tauri dev                  # run with live reload
pnpm tauri build --no-bundle    # release binary: src-tauri/target/release/rode-linux
```

To add it to the application menu:

```sh
install -Dm755 src-tauri/target/release/rode-linux ~/.local/bin/rode-linux
install -Dm644 packaging/rode-linux.desktop ~/.local/share/applications/rode-linux.desktop
install -Dm644 src-tauri/icons/128x128.png ~/.local/share/icons/hicolor/128x128/apps/rode-linux.png
```

What it does:

- High-pass filter, direct monitoring and mix, noise gate, compressor, Aural Exciter and Big Bottom
  with all their parameters. Changes apply immediately; "Save To Microphone" stores them on the mic.
  While there are unsaved changes, "Reset" goes back to the settings from the last save (or from when
  the app connected, if nothing was saved since). "Defaults" loads the default values of one effect.
- "Factory Reset" writes the settings RØDE Central's factory reset writes (input level +12 dB,
  high-pass off, all effect values at their defaults, noise gate off and the other three effects on)
  and saves them to the mic. It asks first; afterwards the previous settings are gone.
- Direct monitoring is a live control like the gain: the mic does not store it. The Mix slider and
  the mix dial on the mic set the same value; the card says which of the two set it last.
- Input gain. This is the mic's ALSA capture volume, the same control your desktop's sound settings use.
- Level meter. It records from the mic through PipeWire (`pw-record`, or `arecord` without PipeWire) and
  only computes levels; no audio is kept. Its switch in the left column turns the recording off.
- Test recording. "Record" next to "Processing" keeps up to 30 seconds from the mic in memory and plays them
  back on the computer's default audio output (`pw-play`, or `aplay`), so you hear what the mic delivers
  with the current settings. Nothing is written to disk.
- Firmware notice. When the mic connects, the app reads RØDE's update list
  (`https://update.rode.com/rode-devices-manifest.json`) and shows "up to date" next to the version, or
  a notice if newer firmware has been released. It never installs firmware; that is left to RØDE
  Central on Windows or macOS. This is the app's only network access.

It never sends the firmware-update commands (HID report 2).

If the window is ever drawn as black triangles (seen once on Intel graphics under Wayland, a WebKitGTK
GPU-path problem), start the app with `WEBKIT_DISABLE_DMABUF_RENDERER=1` to use software rendering.

`RODE_LINUX_DEBUG=1` prints every command sent to the backend on stderr. `pnpm dev` alone serves the UI
with a simulated microphone at <http://localhost:1420>.

### Tests

```sh
cd src-tauri && cargo test    # encoders against vectors from tools/rodectl.py and the mic's factory values
pnpm check                    # type check of the frontend
```

After changing an encoder in `tools/rodectl.py`, regenerate the vectors:
`python3 tools/gen_golden.py > src-tauri/src/testdata/golden.json`.

## Command line

```sh
tools/rodectl.py status
tools/rodectl.py set hpf 75
tools/rodectl.py set compressor.enabled on
tools/rodectl.py set compressor.threshold -20
tools/rodectl.py save          # persist on the mic
```

Mic gain and headphone volume are ordinary ALSA controls
(`amixer -c NTUSB`), not part of the HID protocol.
