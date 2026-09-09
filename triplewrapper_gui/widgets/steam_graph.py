"""Steam-style real-time throughput graph (Cairo)."""
from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")

from gi.repository import Gtk


class SteamGraph(Gtk.DrawingArea):
    """Scrolling line chart with three series (read/write/compress)."""

    MAX_POINTS = 120  # ~2 min at 1 sample/s

    def __init__(self) -> None:
        super().__init__()
        self._read: list[float] = []
        self._write: list[float] = []
        self._compress: list[float] = []
        self.set_draw_func(self._draw)

    # ----------------------------------------------------------------- API
    def reset(self) -> None:
        self._read.clear()
        self._write.clear()
        self._compress.clear()
        self.queue_draw()

    def push(self, read: float, write: float, compress: float) -> None:
        self._read.append(read)
        self._write.append(write)
        self._compress.append(compress)
        if len(self._read) > self.MAX_POINTS:
            self._read.pop(0)
            self._write.pop(0)
            self._compress.pop(0)
        self.queue_draw()

    # --------------------------------------------------------------- Drawing
    def _draw(self, area: Gtk.DrawingArea, ctx, width: int, height: int) -> None:
        import cairo  # noqa: WPS433

        # Background
        ctx.set_source_rgba(0, 0, 0, 0)
        ctx.paint()

        # Grid
        ctx.set_line_width(1)
        ctx.set_source_rgba(0.5, 0.5, 0.5, 0.15)
        for i in range(1, 4):
            y = height * i / 4
            ctx.move_to(0, y)
            ctx.line_to(width, y)
            ctx.stroke()

        if not self._read:
            return

        # Compute Y scale from max of all series
        max_val = max(
            max(self._read) if self._read else 0,
            max(self._write) if self._write else 0,
            max(self._compress) if self._compress else 0,
            1.0,
        )
        max_val *= 1.15  # headroom

        step = width / max(self.MAX_POINTS - 1, 1)

        def draw_series(series: list[float], color: tuple[float, float, float, float]) -> None:
            if not series:
                return
            # Fill
            ctx.move_to(0, height)
            for i, v in enumerate(series):
                x = i * step
                y = height - (v / max_val) * height
                ctx.line_to(x, y)
            ctx.line_to((len(series) - 1) * step, height)
            ctx.close_path()
            ctx.set_source_rgba(*color)
            ctx.fill_preserve()
            # Stroke
            ctx.set_source_rgba(color[0], color[1], color[2], 1.0)
            ctx.set_line_width(1.5)
            ctx.stroke()

        draw_series(self._read, (0.208, 0.518, 0.894, 0.25))     # #3584e4
        draw_series(self._write, (1.0, 0.471, 0.0, 0.25))        # #ff7800
        draw_series(self._compress, (0.180, 0.761, 0.494, 0.25)) # #2ec27e