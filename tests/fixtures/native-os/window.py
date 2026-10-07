#!/usr/bin/env python3
"""Display only controlled GTK controls on the explicit private fixture display."""
import os
import sys

if os.environ.get('DISPLAY') != os.environ.get('MPAI_OS_PRIVATE_DISPLAY') or os.environ.get('DISPLAY') in ['', ':0', ':0.0', None]:
    raise SystemExit('A fixture-owned private X display is required')

import gi
gi.require_version('Gtk', '3.0')
from gi.repository import Gtk, Gdk

selected = sys.argv[1] == 'selected'
title = 'MPAI controlled native OS fixture' if selected else 'MPAI separate decoy fixture'
window = Gtk.Window(title=title)
window.set_default_size(640, 360)
window.set_resizable(False)
window.move(40, 40) if selected else window.move(650, 500)
window.connect('destroy', Gtk.main_quit)
box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=20)
box.set_border_width(24)
window.add(box)
label = Gtk.Label(label='CONTROLLED_VISIBLE_TEXT_20261003' if selected else 'DECOY_TEXT_MUST_NOT_APPEAR')
label.set_name('fixture-label')
box.pack_start(label, False, False, 0)
button = Gtk.Button(label='Controlled action button' if selected else 'Decoy action')
box.pack_start(button, False, False, 0)
entry = Gtk.Entry()
entry.set_text('Controlled entry value' if selected else 'Decoy entry value')
entry.get_accessible().set_name('Controlled entry field' if selected else 'Decoy field')
box.pack_start(entry, False, False, 0)
provider = Gtk.CssProvider()
provider.load_from_data(b'window { background: #1144aa; } #fixture-label { color: white; font-size: 20px; }')
Gtk.StyleContext.add_provider_for_screen(Gdk.Screen.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
window.show_all()
Gtk.main()
