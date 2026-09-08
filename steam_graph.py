#!/usr/bin/env python3
"""
Steam-style real-time graph widget for TripleWrapper
GTK4 + Cairo - Shows read/write/compression speeds + ETA
"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('Gdk', '4.0')
from gi.repository import Gtk, Gdk, GLib, GObject
import cairo
import random
import time
import math
from collections import deque
from dataclasses import dataclass
from typing import Deque, List, Tuple


@dataclass
class SpeedSample:
    timestamp: float
    read_mbps: float
    write_mbps: float
    compress_mbps: float


class SteamGraphWidget(Gtk.DrawingArea):
    """Steam download graph clone: 3 lines + shaded areas + current file label"""

    MAX_SAMPLES = 300  # 5 minutes at 1 sample/sec
    UPDATE_INTERVAL_MS = 100  # 10 FPS for smooth animation

    # Steam-like colors
    COLORS = {
        'bg': (0.08, 0.09, 0.11, 1.0),           # Dark background
        'grid': (0.25, 0.28, 0.32, 0.4),         # Subtle grid
        'read': (0.2, 0.65, 0.95, 1.0),          # Blue - read speed
        'write': (0.95, 0.65, 0.2, 1.0),         # Orange - write speed
        'compress': (0.4, 0.85, 0.4, 1.0),       # Green - compression
        'read_fill': (0.2, 0.65, 0.95, 0.15),    # Blue fill
        'write_fill': (0.95, 0.65, 0.2, 0.15),   # Orange fill
        'compress_fill': (0.4, 0.85, 0.4, 0.12), # Green fill
        'text': (0.92, 0.92, 0.94, 1.0),         # Light text
        'text_dim': (0.6, 0.65, 0.7, 1.0),       # Dim text
        'current_file_bg': (0.12, 0.14, 0.18, 0.95),
    }

    def __init__(self):
        super().__init__()
        self.set_content_width(600)
        self.set_content_height(180)
        self.set_draw_func(self.on_draw)

        # Data
        self.samples: Deque[SpeedSample] = deque(maxlen=self.MAX_SAMPLES)
        self.max_speed = 100.0  # MB/s, auto-scales
        self.current_file = "Esperando operación..."
        self.eta_seconds = 0
        self.total_bytes = 0
        self.processed_bytes = 0

        # Animation
        self._tick_id = None
        self._last_frame_time = time.time()

        # Start simulation for demo
        self.start_simulation()

    def start_simulation(self):
        """Demo: simulate real compression/extraction workload"""
        self._tick_id = GLib.timeout_add(self.UPDATE_INTERVAL_MS, self._simulate_tick)

    def _simulate_tick(self) -> bool:
        now = time.time()
        dt = now - self._last_frame_time
        self._last_frame_time = now

        # Simulate varying speeds (like real compression)
        phase = (now * 0.5) % (2 * math.pi)
        base_read = 120 + 80 * math.sin(phase) + random.uniform(-15, 15)
        base_write = 90 + 60 * math.sin(phase + 1.2) + random.uniform(-10, 10)
        base_compress = 45 + 30 * math.sin(phase + 2.5) + random.uniform(-5, 5)

        # Clamp
        read = max(0, base_read)
        write = max(0, base_write)
        compress = max(0, base_compress)

        self.add_sample(read, write, compress)

        # Simulate file progress
        self.processed_bytes += (read + write) * 1_000_000 * dt * 0.5
        if self.processed_bytes >= self.total_bytes and self.total_bytes > 0:
            self._next_simulated_file()

        return True  # Continue timeout

    def _next_simulated_file(self):
        files = [
            ("pakchunk0-Windows.pak", 2.1 * 1024**3),
            ("pakchunk1-Windows.pak", 1.8 * 1024**3),
            ("pakchunk2-Windows.pak", 3.2 * 1024**3),
            ("assets/audio.bank", 450 * 1024**2),
            ("assets/textures.pak", 1.5 * 1024**3),
            ("shaders/cache.bin", 120 * 1024**2),
        ]
        name, size = random.choice(files)
        self.current_file = f"Procesando: {name}"
        self.total_bytes = size
        self.processed_bytes = 0

    def add_sample(self, read_mbps: float, write_mbps: float, compress_mbps: float):
        """Add a new speed sample (call from worker thread via GLib.idle_add)"""
        sample = SpeedSample(
            timestamp=time.time(),
            read_mbps=read_mbps,
            write_mbps=write_mbps,
            compress_mbps=compress_mbps
        )
        self.samples.append(sample)

        # Auto-scale max speed (with smoothing)
        peak = max(read_mbps, write_mbps, compress_mbps)
        if peak > self.max_speed:
            self.max_speed = peak * 1.15
        elif peak < self.max_speed * 0.4 and self.max_speed > 50:
            self.max_speed = max(50, self.max_speed * 0.98)

        # Update ETA
        if write_mbps > 0.1 and self.total_bytes > self.processed_bytes:
            remaining = self.total_bytes - self.processed_bytes
            self.eta_seconds = remaining / (write_mbps * 1_000_000)
        else:
            self.eta_seconds = 0

        self.queue_draw()

    def on_draw(self, area: Gtk.DrawingArea, cr: cairo.Context, width: int, height: int):
        # Clear background
        cr.set_source_rgba(*self.COLORS['bg'])
        cr.paint()

        # Draw grid
        self._draw_grid(cr, width, height)

        # Draw graph areas and lines
        if len(self.samples) >= 2:
            self._draw_speed_graph(cr, width, height, 'read', self.COLORS['read_fill'], self.COLORS['read'])
            self._draw_speed_graph(cr, width, height, 'write', self.COLORS['write_fill'], self.COLORS['write'])
            self._draw_speed_graph(cr, width, height, 'compress', self.COLORS['compress_fill'], self.COLORS['compress'])

        # Draw legend
        self._draw_legend(cr, width, height)

        # Draw current file + ETA overlay
        self._draw_status_overlay(cr, width, height)

    def _draw_grid(self, cr: cairo.Context, w: int, h: int):
        cr.set_source_rgba(*self.COLORS['grid'])
        cr.set_line_width(1)
        cr.set_dash([4, 4], 0)

        # Horizontal grid lines (speed markers)
        for i in range(5):
            y = self._margin_top + (h - self._margin_top - self._margin_bottom) * i / 4
            cr.move_to(self._margin_left, y)
            cr.line_to(w - self._margin_right, y)
            cr.stroke()

            # Speed labels
            speed = self.max_speed * (1 - i / 4)
            cr.set_source_rgba(*self.COLORS['text_dim'])
            cr.select_font_face("Monospace", cairo.FontSlant.NORMAL, cairo.FontWeight.NORMAL)
            cr.set_font_size(10)
            cr.move_to(4, y + 3)
            cr.show_text(f"{speed:.0f} MB/s")

        # Vertical grid lines (time markers)
        for i in range(6):
            x = self._margin_left + (w - self._margin_left - self._margin_right) * i / 5
            cr.move_to(x, self._margin_top)
            cr.line_to(x, h - self._margin_bottom)
            cr.stroke()

        cr.set_dash([], 0)

    def _draw_speed_graph(self, cr: cairo.Context, w: int, h: int,
                          attr: str, fill_color: Tuple, line_color: Tuple):
        if len(self.samples) < 2:
            return

        graph_w = w - self._margin_left - self._margin_right
        graph_h = h - self._margin_top - self._margin_bottom
        x0 = self._margin_left
        y0 = self._margin_top

        # Build path
        cr.new_path()
        first = True

        for i, sample in enumerate(self.samples):
            speed = getattr(sample, f'{attr}_mbps')
            x = x0 + graph_w * i / (self.MAX_SAMPLES - 1)
            y = y0 + graph_h * (1 - min(speed / self.max_speed, 1.0))

            if first:
                cr.move_to(x, y)
                first = False
            else:
                cr.line_to(x, y)

        # Fill area under curve
        if not first:
            cr.line_to(x, y0 + graph_h)
            cr.line_to(x0, y0 + graph_h)
            cr.close_path()
            cr.set_source_rgba(*fill_color)
            cr.fill_preserve()

            # Stroke line
            cr.set_source_rgba(*line_color)
            cr.set_line_width(2)
            cr.stroke()

    def _draw_legend(self, cr: cairo.Context, w: int, h: int):
        items = [
            ("Lectura", self.COLORS['read']),
            ("Escritura", self.COLORS['write']),
            ("Compresión", self.COLORS['compress']),
        ]
        cr.select_font_face("Sans", cairo.FontSlant.NORMAL, cairo.FontWeight.NORMAL)
        cr.set_font_size(11)

        x = w - 200
        y = self._margin_top + 10
        for label, color in items:
            # Color dot
            cr.set_source_rgba(*color)
            cr.arc(x + 6, y + 6, 4, 0, 2 * math.pi)
            cr.fill()
            # Label
            cr.set_source_rgba(*self.COLORS['text'])
            cr.move_to(x + 16, y + 11)
            cr.show_text(label)
            y += 22

    def _draw_status_overlay(self, cr: cairo.Context, w: int, h: int):
        # Current file bar at bottom
        bar_h = 36
        bar_y = h - bar_h - 4
        cr.set_source_rgba(*self.COLORS['current_file_bg'])
        cr.rectangle(4, bar_y, w - 8, bar_h)
        cr.fill()

        cr.select_font_face("Sans", cairo.FontSlant.NORMAL, cairo.FontWeight.NORMAL)
        cr.set_font_size(12)
        cr.set_source_rgba(*self.COLORS['text'])
        cr.move_to(12, bar_y + 22)
        cr.show_text(self.current_file)

        # ETA on right
        if self.eta_seconds > 0:
            eta_str = self._format_eta(self.eta_seconds)
            cr.set_font_size(11)
            cr.set_source_rgba(*self.COLORS['text_dim'])
            x = w - 140
            cr.move_to(x, bar_y + 22)
            cr.show_text(f"ETA: {eta_str}")

        # Progress % if available
        if self.total_bytes > 0:
            pct = min(100, (self.processed_bytes / self.total_bytes) * 100)
            cr.set_font_size(11)
            cr.set_source_rgba(*self.COLORS['text_dim'])
            cr.move_to(12, bar_y + 22 + 18)  # Would need taller bar
            # Mini progress bar
            pb_w = 120
            pb_x = w - pb_w - 12
            pb_y = bar_y + 8
            cr.set_source_rgba(0.15, 0.18, 0.22, 1.0)
            cr.rectangle(pb_x, pb_y, pb_w, 6)
            cr.fill()
            cr.set_source_rgba(*self.COLORS['write'])
            cr.rectangle(pb_x, pb_y, pb_w * pct / 100, 6)
            cr.fill()

    def _format_eta(self, seconds: float) -> str:
        if seconds < 60:
            return f"{int(seconds)}s"
        elif seconds < 3600:
            return f"{int(seconds/60)}m {int(seconds%60)}s"
        else:
            h = int(seconds / 3600)
            m = int((seconds % 3600) / 60)
            return f"{h}h {m}m"

    @property
    def _margin_left(self) -> int:
        return 50

    @property
    def _margin_right(self) -> int:
        return 20

    @property
    def _margin_top(self) -> int:
        return 10

    @property
    def _margin_bottom(self) -> int:
        return 50


class MainWindow(Gtk.ApplicationWindow):
    def __init__(self, app):
        super().__init__(application=app, title="TripleWrapper - Graph Prototype")
        self.set_default_size(900, 500)

        # Main layout
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.set_child(box)

        # Header bar
        header = Gtk.HeaderBar()
        header.set_title_widget(Gtk.Label(label="TripleWrapper"))
        header.set_show_title_buttons(True)
        self.set_titlebar(header)

        # Top panel (GNOME Disks style) - placeholder
        top_panel = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        top_panel.set_margin_top(24)
        top_panel.set_margin_bottom(12)
        top_panel.set_margin_start(24)
        top_panel.set_margin_end(24)

        title = Gtk.Label(label="Archivo: game_data.pak  •  4.2 GB / 8.7 GB")
        title.add_css_class("title-2")
        title.set_halign(Gtk.Align.START)
        top_panel.append(title)

        # Donut chart placeholder
        donut_box = Gtk.Box()
        donut_box.set_size_request(120, 120)
        donut_label = Gtk.Label(label="🟦 48% conservar\n🟧 32% eliminar\n🟩 20% añadir")
        donut_label.set_justify(Gtk.Justification.CENTER)
        donut_box.append(donut_label)
        top_panel.append(donut_box)

        # Action buttons
        btn_box = Gtk.Box(spacing=12)
        for label, css in [("Extraer", "suggested-action"), ("Modificar", None), ("Limpiar", "destructive-action")]:
            btn = Gtk.Button(label=label)
            if css:
                btn.add_css_class(css)
            btn.set_size_request(140, 48)
            btn_box.append(btn)
        top_panel.append(btn_box)

        box.append(top_panel)

        # Separator
        sep = Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL)
        box.append(sep)

        # Bottom panel - Steam-style graph
        graph_frame = Gtk.Frame()
        graph_frame.set_child(SteamGraphWidget())
        box.append(graph_frame)

        # Add some CSS
        self._add_css()

    def _add_css(self):
        css = """
        window { background-color: #1a1b1e; }
        frame { border: none; background: transparent; }
        button { font-size: 13px; font-weight: 600; border-radius: 8px; }
        button.suggested-action { background: #3584e4; color: white; }
        button.suggested-action:hover { background: #4a94f0; }
        button.destructive-action { background: #c0392b; color: white; }
        button.destructive-action:hover { background: #d64032; }
        label { color: #e0e0e0; }
        """
        provider = Gtk.CssProvider()
        provider.load_from_data(css.encode())
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
        )


class Application(Gtk.Application):
    def __init__(self):
        super().__init__(application_id="com.triplewrapper.graphproto")

    def do_activate(self):
        win = MainWindow(self)
        win.present()


def main():
    app = Application()
    return app.run([])


if __name__ == "__main__":
    main()