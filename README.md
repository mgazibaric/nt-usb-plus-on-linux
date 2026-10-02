# NT-USB+ on Linux

Control a RØDE NT-USB+ on Linux without RØDE Central. Unofficial, not affiliated with RØDE.

- `src/`, `src-tauri/` — the desktop app (Tauri 2, Svelte 5, Rust)
- `docs/PROTOCOL.md` — the reverse-engineered HID protocol
- `tools/rodectl.py` — reference command-line implementation (Python standard library only)
- `tools/aphex_tables.json` — lookup tables for the effects, copied from RØDE Central (see
  [Where the protocol comes from](#where-the-protocol-comes-from))
- `udev/70-rode-nt-usb-plus.rules` — grants the logged-in user access to the control interface
- `packaging/` — menu entry, Arch `PKGBUILD`, and what the .deb/.rpm bundles add

## Install

The packages contain the app, its menu entry and icon, and the udev rule that gives the logged-in
user access to the mic's control interface.

Arch, Manjaro (builds from this checkout; needs `cargo`, `nodejs` and `pnpm`):

```sh
cd packaging/arch
makepkg -sic
```

Debian, Ubuntu, Fedora, openSUSE (needs Rust, Node with pnpm and the development packages of
`webkit2gtk-4.1` and `alsa-lib`):

```sh
pnpm install
pnpm tauri build    # src-tauri/target/release/bundle/deb/*.deb and rpm/*.rpm
```

## Without a package

The control interface is only accessible to root until the udev rule is installed:

```sh
sudo install -m 644 udev/70-rode-nt-usb-plus.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger --subsystem-match=hidraw --action=add
```

Build the app (needs Rust, Node with pnpm, `webkit2gtk-4.1` and `alsa-lib`):

```sh
pnpm install
pnpm tauri dev                  # run with live reload
pnpm tauri build --no-bundle    # release binary: src-tauri/target/release/nt-usb-plus-on-linux
```

To add it to the application menu:

```sh
install -Dm755 src-tauri/target/release/nt-usb-plus-on-linux ~/.local/bin/nt-usb-plus-on-linux
install -Dm644 packaging/nt-usb-plus-on-linux.desktop ~/.local/share/applications/nt-usb-plus-on-linux.desktop
install -Dm644 src-tauri/icons/128x128.png ~/.local/share/icons/hicolor/128x128/apps/nt-usb-plus-on-linux.png
```

## App

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

`NT_USB_PLUS_DEBUG=1` prints every command sent to the backend on stderr. `pnpm dev` alone serves the UI
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

## Where the protocol comes from

RØDE publishes no documentation for the mic's control interface. The protocol was worked out for
interoperability, by watching the mic's answers and by reading RØDE Central, RØDE's own app;
`docs/PROTOCOL.md` is this project's description of it.

One file is not this project's own work: `tools/aphex_tables.json` holds seven tables of 256 numbers
each, copied unchanged from RØDE Central. The mic's effects take raw DSP values, and the tables
translate a setting such as a threshold in dB into the value the mic expects. They remain RØDE's
data.

RØDE, NT-USB+, Aphex, Aural Exciter and Big Bottom are trademarks of their respective owners. This
project is not affiliated with or endorsed by them.

## Licence

MIT, see `LICENSE`. It covers this project's code and documentation, not
`tools/aphex_tables.json`.
