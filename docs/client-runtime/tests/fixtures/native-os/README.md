# Native X11 and accessibility acceptance

Install only fixture-local Ubuntu Noble packages, then run the real daemon in an isolated graphical session:

```sh
python3 tests/fixtures/native-os/install.py --receipt /absolute/external/path/packages.json
python3 tests/fixtures/native-os/acceptance.py --artifacts /absolute/external/path/fresh-run-directory
```

The installer uses `apt download` and `dpkg-deb -x` exclusively. It extracts the optional window manager, wmctrl, ImageMagick and pyatspi dependencies into ignored `vendor/`, with local symlinks for executable discovery. No package is installed into the operating system. Versions and archive hashes are recorded in the external receipt. Existing Xvfb, D-Bus, Gtk 3 GIR, AT-SPI services and `/usr/bin/python3` with gi remain host prerequisites. The current executable must exist at `target/debug/swarmie`.

The runner creates its own D-Bus session, Xvfb display, Openbox window manager, HOME/XDG directories and daemon on a newly assigned loopback port. It never reads a user display or existing daemon. Two separate GTK processes expose controlled fixture content. Assertions check native selected-window PID/bounds, actual PNG pixels, real AT-SPI names/roles/text, exclusion of the decoy process, and rejection of malformed window identifiers. Images, traces, logs and isolated state are saved only in the external artifact directory.

Linux context currently selects the application PID associated with the X11 window and reports named accessible nodes. This acceptance proves those actual controls; it does not claim extraction of every unnamed node, text-field value or exact-window filtering among several windows owned by one application. Wayland-only sessions, macOS permission flows and Windows UI Automation are separate platform concerns.

Owned daemon, GTK, window-manager and Xvfb processes are closed, then the private D-Bus process group is terminated and checked for remaining live services. A fresh artifact directory is required for each run.
