"""
Steam-style real-time graph widget for TripleWrapper
Ported from prototype to production GTK4/Libadwaita
"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('Graphene', '1.0')
from gi.repository import Gtk, Gdk, GLib, Graphene, Cairo
import math
from collections import deque
from typing import Deque, Tuple, Optional
from dataclasses import dataclass


@dataclass
class SpeedSample:
    """Single speed measurement"""
    read_mbps: float
    write_mbps: float
    compress_mbps: float


class SteamGraphWidget(Gtk.DrawingArea):
    """
    Steam download graph clone for GTK4.
    Shows read/write/compression speeds with shaded areas.
    """
    
    # Configuration
    MAX_SAMPLES = 300  # 5 minutes at 1 Hz
    DEFAULT_MAX_SPEED = 100.0  # MB/s
    
    # Steam-inspired color palette
    COLORS = {
        'bg': (0.08, 0.09, 0.11, 1.0),
        'grid': (0.25, 0.28, 0.32, 0.4),
        'read': (0.20, 0.65, 0.95, 1.0),
        'write': (0.95, 0.65, 0.20, 1.0),
        'compress': (0.40, 0.85, 0.40, 1.0),
        'read_fill': (0.20, 0.65, 0.95, 0.15),
        'write_fill': (0.95, 0.65, 0.20, 0.15),
        'compress_fill': (0.40, 0.85, 0.40, 0.12),
        'text': (0.92, 0.92, 0.94, 1.0),
        'text_dim': (0.60, 0.65, 0.70, 1.0),
        'accent': (0.35, 0.52, 0.89, 1.0),  # Adwaita blue
        'status_bg': (0.12, 0.14, 0.18, 0.95),
    }
    
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        
        # Size
        self.set_content_width(600)
        self.set_content_height(180)
        self.set_draw_func(self._on_draw)
        
        # Data
        self._samples: Deque[SpeedSample] = deque(maxlen=self.MAX_SAMPLES)
        self._max_speed = self.DEFAULT_MAX_SPEED
        self._current_file = "Esperando operación..."
        self._eta_seconds: Optional[int] = None
        self._total_bytes: int = 0
        self._processed_bytes: int = 0
        self._running = False
        
        # Animation
        self._frame_time = 0.0
    
    # Public API for telemetry updates
    
    def add_sample(self, read_mbps: float, write_mbps: float, compress_mbps: float):
        """Add a speed sample (thread-safe via GLib.idle_add)"""
        sample = SpeedSample(
            read_mbps=max(0, read_mbps),
            write_mbps=max(0, write_mbps),
            compress_mbps=max(0, compress_mbps),
        )
        self._samples.append(sample)
        self._update_max_speed(sample)
        self.queue_draw()
    
    def set_current_file(self, filename: str):
        """Set current file being processed"""
        self._current_file = filename
        self.queue_draw()
    
    def set_progress(self, processed: int, total: int, eta_seconds: Optional[int] = None):
        """Update progress and ETA"""
        self._processed_bytes = processed
        self._total_bytes = total
        self._eta_seconds = eta_seconds
        self.queue_draw()
    
    def set_running(self, running: bool):
        """Set operation running state"""
        self._running = running
        if not running:
            self._eta_seconds = None
        self.queue_draw()
    
    def reset(self):
        """Reset graph to initial state"""
        self._samples.clear()
        self._max_speed = self.DEFAULT_MAX_SPEED
        self._current_file = "Esperando operación..."
        self._eta_seconds = None
        self._total_bytes = 0
        self._processed_bytes = 0
        self._running = False
        self.queue_draw()
    
    # Internal methods
    
    def _update_max_speed(self, sample: SpeedSample):
        """Auto-scale Y axis based on peak speed"""
        peak = max(sample.read_mbps, sample.write_mbps, sample.compress_mbps)
        if peak > self._max_speed:
            self._max_speed = peak * 1.15
        elif peak < self._max_speed * 0.3 and self._max_speed > 20:
            self._max_speed = max(20, self._max_speed * 0.99)
    
    # Drawing
    
    def _on_draw(self, area: Gtk.DrawingArea, cr: Cairo.Context, width: int, height: int):
        # Background
        cr.set_source_rgba(*self.COLORS['bg'])
        cr.paint()
        
        if len(self._samples) < 2:
            self._draw_empty_state(cr, width, height)
            return
        
        # Margins
        m_l, m_r, m_t, m_b = 50, 20, 10, 45
        graph_w = width - m_l - m_r
        graph_h = height - m_t - m_b
        
        # Grid
        self._draw_grid(cr, width, height, m_l, m_r, m_t, m_b, graph_w, graph_h)
        
        # Graph lines (bottom to top for layering)
        self._draw_metric(cr, m_l, m_t, graph_w, graph_h, 2,  # compress (bottom)
                         self.COLORS['compress_fill'], self.COLORS['compress'])
        self._draw_metric(cr, m_l, m_t, graph_w, graph_h, 1,  # write (middle)
                         self.COLORS['write_fill'], self.COLORS['write'])
        self._draw_metric(cr, m_l, m_t, graph_w, graph_h, 0,  # read (top)
                         self.COLORS['read_fill'], self.COLORS['read'])
        
        # Legend
        self._draw_legend(cr, width, height, m_t)
        
        # Status overlay
        self._draw_status_overlay(cr, width, height, m_l, m_r, m_b)
    
    def _draw_empty_state(self, cr: Cairo.Context, w: int, h: int):
        cr.set_source_rgba(*self.COLORS['text_dim'])
        cr.select_font_face("Sans", Cairo.FontSlant.NORMAL, Cairo.FontWeight.NORMAL)
        cr.set_font_size(14)
        text = "Sin datos de telemetría"
        extents = cr.text_extents(text)
        cr.move_to((w - extents.width) / 2, (h - extents.height) / 2)
        cr.show_text(text)
    
    def _draw_grid(self, cr: Cairo.Context, w: int, h: int, 
                   m_l: int, m_r: int, m_t: int, m_b: int, gw: int, gh: int):
        cr.set_source_rgba(*self.COLORS['grid'])
        cr.set_line_width(1)
        cr.set_dash([4, 4], 0)
        
        # Horizontal lines (speed markers)
        for i in range(5):
            y = m_t + gh * i / 4
            cr.move_to(m_l, y)
            cr.line_to(w - m_r, y)
            cr.stroke()
            
            # Speed labels
            speed = self._max_speed * (1 - i / 4)
            cr.set_source_rgba(*self.COLORS['text_dim'])
            cr.set_font_size(10)
            cr.move_to(4, y + 3)
            cr.show_text(f"{speed:.0f} MB/s")
        
        # Vertical lines (time markers)
        for i in range(6):
            x = m_l + gw * i / 5
            cr.move_to(x, m_t)
            cr.line_to(x, h - m_b)
            cr.stroke()
        
        cr.set_dash([], 0)
    
    def _draw_metric(self, cr: Cairo.Context, m_l: int, m_t: int, 
                     gw: int, gh: int, attr_idx: int,
                     fill_color: Tuple, line_color: Tuple):
        """Draw one metric (read/write/compress)"""
        cr.new_path()
        first = True
        n = len(self._samples)
        
        for i, sample in enumerate(self._samples):
            speed = [sample.read_mbps, sample.write_mbps, sample.compress_mbps][attr_idx]
            x = m_l + gw * i / (self.MAX_SAMPLES - 1)
            y = m_t + gh * (1 - min(speed / self._max_speed, 1.0))
            
            if first:
                cr.move_to(x, y)
                first = False
            else:
                cr.line_to(x, y)
        
        if not first:
            # Close path for fill
            cr.line_to(x, m_t + gh)
            cr.line_to(m_l, m_t + gh)
            cr.close_path()
            
            # Fill
            cr.set_source_rgba(*fill_color)
            cr.fill_preserve()
            
            # Stroke
            cr.set_source_rgba(*line_color)
            cr.set_line_width(2)
            cr.stroke()
    
    def _draw_legend(self, cr: Cairo.Context, w: int, h: int, m_t: int):
        items = [
            ("Lectura", self.COLORS['read']),
            ("Escritura", self.COLORS['write']),
            ("Compresión", self.COLORS['compress']),
        ]
        cr.select_font_face("Sans", Cairo.FontSlant.NORMAL, Cairo.FontWeight.NORMAL)
        cr.set_font_size(11)
        
        x = w - 180
        y = m_t + 10
        for label, color in items:
            # Color indicator
            cr.set_source_rgba(*color)
            cr.arc(x + 6, y + 6, 4, 0, 2 * math.pi)
            cr.fill()
            # Label
            cr.set_source_rgba(*self.COLORS['text'])
            cr.move_to(x + 16, y + 11)
            cr.show_text(label)
            y += 22
    
    def _draw_status_overlay(self, cr: Cairo.Context, w: int, h: int, 
                             m_l: int, m_r: int, m_b: int):
        # Status bar at bottom
        bar_h = 36
        bar_y = h - bar_h - 4
        
        cr.set_source_rgba(*self.COLORS['status_bg'])
        cr.rectangle(4, bar_y, w - 8, bar_h)
        cr.fill()
        
        # Current file
        cr.select_font_face("Sans", Cairo.FontSlant.NORMAL, Cairo.FontWeight.NORMAL)
        cr.set_font_size(12)
        cr.set_source_rgba(*self.COLORS['text'])
        cr.move_to(12, bar_y + 22)
        cr.show_text(self._current_file)
        
        # ETA (right side)
        if self._eta_seconds and self._eta_seconds > 0:
            eta_str = self._format_eta(self._eta_seconds)
            cr.set_font_size(11)
            cr.set_source_rgba(*self.COLORS['text_dim'])
            cr.move_to(w - 140, bar_y + 22)
            cr.show_text(f"ETA: {eta_str}")
        
        # Progress bar (if we have total)
        if self._total_bytes > 0:
            pct = min(100, (self._processed_bytes / self._total_bytes) * 100)
            pb_w = 120
            pb_x = w - pb_w - 12
            pb_y = bar_y + 8
            
            # Background
            cr.set_source_rgba(0.15, 0.18, 0.22, 1.0)
            cr.rectangle(pb_x, pb_y, pb_w, 6)
            cr.fill()
            
            # Progress
            cr.set_source_rgba(*self.COLORS['write'])
            cr.rectangle(pb_x, pb_y, pb_w * pct / 100, 6)
            cr.fill()
            
            # Percentage text
            cr.set_font_size(10)
            cr.set_source_rgba(*self.COLORS['text_dim'])
            cr.move_to(pb_x - 45, pb_y + 10)
            cr.show_text(f"{pct:.0f}%")
    
    def _format_eta(self, seconds: int) -> str:
        if seconds < 60:
            return f"{seconds}s"
        elif seconds < 3600:
            return f"{seconds // 60}m {seconds % 60}s"
        else:
            return f"{seconds // 3600}h {(seconds % 3600) // 60}m"


# Convenience function for GLib.idle_add
def emit_speeds(widget: SteamGraphWidget, read: float, write: float, compress: float, file: str = ""):
    """Thread-safe wrapper for adding samples"""
    widget.add_sample(read, write, compress)
    if file:
        widget.set_current_file(file)