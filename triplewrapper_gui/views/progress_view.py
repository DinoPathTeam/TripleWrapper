"""Progress view with Steam-style real-time graph."""
from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Gdk, GObject, Gtk

from ..core.models import ProgressTick
from ..widgets.steam_graph import SteamGraph


class ProgressView(Gtk.Box):
    """Shows real-time throughput and overall progress."""

    __gtype_name__ = "TripleWrapperProgressView"
    __gsignals__ = {  # noqa: RUF012 - GObject signal map must be a dict
        "done": (GObject.SignalFlags.RUN_FIRST, None, ()),
        "cancel-requested": (GObject.SignalFlags.RUN_FIRST, None, ()),
    }

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=18)
        self._build_ui()

    def _build_ui(self) -> None:
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=18)
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)
        scrolled.set_child(content)
        self.append(scrolled)

        title = Gtk.Label(label="Operación en curso")
        title.add_css_class("title-2")
        title.set_halign(Gtk.Align.CENTER)
        content.append(title)

        # Graph card
        graph_card = Gtk.Frame()
        graph_card.add_css_class("card")
        graph_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        graph_box.set_margin_top(16)
        graph_box.set_margin_bottom(16)
        graph_box.set_margin_start(16)
        graph_box.set_margin_end(16)
        graph_card.set_child(graph_box)

        self._graph = SteamGraph()
        self._graph.set_size_request(0, 180)
        self._graph.set_vexpand(True)
        graph_box.append(self._graph)

        # Legend
        legend = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=18)
        legend.set_halign(Gtk.Align.CENTER)
        for label, color in [
            ("Lectura", "#3584e4"),
            ("Escritura", "#ff7800"),
            ("Compresión", "#2ec27e"),
        ]:
            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
            swatch = Gtk.DrawingArea()
            swatch.set_size_request(12, 12)
            swatch.set_draw_func(
                lambda w, ctx, *a, c=color: self._draw_swatch(ctx, c)
            )
            row.append(swatch)
            row.append(Gtk.Label(label=label))
            legend.append(row)
        graph_box.append(legend)

        content.append(graph_card)

        # Progress bar
        self._progress_bar = Gtk.ProgressBar()
        self._progress_bar.set_show_text(True)
        self._progress_bar.set_text("Preparando…")
        content.append(self._progress_bar)

        # Stats row
        stats = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=24)
        stats.set_halign(Gtk.Align.CENTER)

        self._read_lbl = self._stat_block("Lectura", "0 MB/s")
        self._write_lbl = self._stat_block("Escritura", "0 MB/s")
        self._compress_lbl = self._stat_block("Compresión", "0 MB/s")
        for lbl in (self._read_lbl, self._write_lbl, self._compress_lbl):
            stats.append(lbl)

        content.append(stats)

        # Actions
        actions = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        actions.set_halign(Gtk.Align.CENTER)

        self._cancel_btn = Gtk.Button(label="Cancelar")
        self._cancel_btn.add_css_class("destructive-action")
        self._cancel_btn.connect("clicked", lambda *_: self.emit("cancel-requested"))
        actions.append(self._cancel_btn)

        self._done_btn = Gtk.Button(label="Finalizar")
        self._done_btn.add_css_class("suggested-action")
        self._done_btn.set_sensitive(False)
        self._done_btn.connect("clicked", lambda *_: self.emit("done"))
        actions.append(self._done_btn)

        self.append(actions)

    # ----------------------------------------------------------------- API
    def reset(self) -> None:
        self._graph.reset()
        self._progress_bar.set_fraction(0.0)
        self._progress_bar.set_text("Preparando…")
        self._read_lbl.get_last_child().set_label("0 MB/s")
        self._write_lbl.get_last_child().set_label("0 MB/s")
        self._compress_lbl.get_last_child().set_label("0 MB/s")
        self._cancel_btn.set_sensitive(True)
        self._done_btn.set_sensitive(False)

    def update_tick(self, tick: ProgressTick) -> None:
        self._graph.push(tick.read_mbps, tick.write_mbps, tick.compress_mbps)
        self._progress_bar.set_fraction(tick.progress)
        self._progress_bar.set_text(tick.stage)
        self._read_lbl.get_last_child().set_label(f"{tick.read_mbps:.1f} MB/s")
        self._write_lbl.get_last_child().set_label(f"{tick.write_mbps:.1f} MB/s")
        self._compress_lbl.get_last_child().set_label(f"{tick.compress_mbps:.1f} MB/s")
        if tick.progress >= 1.0:
            self._cancel_btn.set_sensitive(False)
            self._done_btn.set_sensitive(True)

    # --------------------------------------------------------------- Helpers
    def _stat_block(self, title: str, value: str) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
        box.set_halign(Gtk.Align.CENTER)
        t = Gtk.Label(label=title)
        t.add_css_class("dim-label")
        t.add_css_class("caption")
        box.append(t)
        v = Gtk.Label(label=value)
        v.add_css_class("heading")
        box.append(v)
        return box

    @staticmethod
    def _draw_swatch(ctx, color: str) -> None:
        rgba = Gdk.RGBA.parse(color)
        ctx.set_source_rgba(rgba.red, rgba.green, rgba.blue, rgba.alpha)
        ctx.rectangle(0, 0, 12, 12)
        ctx.fill()