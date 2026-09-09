"""Storage donut chart (Cairo)."""
from __future__ import annotations

import math

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Gdk", "4.0")

from gi.repository import Gdk, Gtk


class StorageDonut(Gtk.DrawingArea):
    """Donut chart with labeled segments."""

    def __init__(self) -> None:
        super().__init__()
        self._segments: list[tuple[str, int, str]] = []
        self.set_draw_func(self._draw)

    # ----------------------------------------------------------------- API
    def set_segments(self, segments: list[tuple[str, int, str]]) -> None:
        self._segments = segments
        self.queue_draw()

    def get_legend(self) -> list[tuple[str, str]]:
        return [(label, color) for label, _, color in self._segments]

    # --------------------------------------------------------------- Drawing
    def _draw(self, area: Gtk.DrawingArea, ctx, width: int, height: int) -> None:

        cx, cy = width / 2, height / 2
        radius = min(width, height) / 2 - 6
        inner = radius * 0.62

        total = sum(v for _, v, _ in self._segments) or 1
        start = -math.pi / 2

        for label, value, color in self._segments:
            sweep = (value / total) * math.tau
            ctx.set_source_rgba(*self._parse_color(color))
            ctx.arc(cx, cy, radius, start, start + sweep)
            ctx.arc_negative(cx, cy, inner, start + sweep, start)
            ctx.close_path()
            ctx.fill()
            start += sweep

        # Center text
        ctx.set_source_rgba(1, 1, 1, 0.9)
        ctx.select_font_face("Sans")
        ctx.set_font_size(radius * 0.22)
        text = f"{len(self._segments)}"
        extents = ctx.text_extents(text)
        ctx.move_to(cx - extents.width / 2, cy + extents.height / 2)
        ctx.show_text(text)

    @staticmethod
    def _parse_color(hex_color: str) -> tuple[float, float, float, float]:
        rgba = Gdk.RGBA.parse(hex_color)
        return (rgba.red, rgba.green, rgba.blue, rgba.alpha)