#!/bin/sh
# Apply the udev rule to a mic that is already plugged in.
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload || true
    udevadm trigger --subsystem-match=hidraw --action=add || true
fi
