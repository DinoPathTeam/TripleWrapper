"""
Donut chart widget for storage visualization
Shows: keep / remove / add breakdown
"""

import gi
gi.require_version('Gtk', '4.0')
from gi.repository import Gtk, Cairo
import math
from typing import Tuple
from dataclasses import dataclass


@dataclass
class DonutSegment:
    """Single segment of the donut chart"""
    value: float      # Value in bytes
    color: Tuple[float, float, float, float]
    label: str
    percentage: float = 0.0


class DonutChartWidget(Gtk.DrawingArea):
    """
    Animated donut chart for storage breakdown.
    Shows: Keep (blue), Remove (orange), Add (green)
    """
    
    COLORS = {
        'keep': (0.20, 0.55, 0.95, 1.0),      # Blue - files to keep
        'remove': (0.95, 0.55, 0.20, 1.0),    # Orange - files to remove
        'add': (0.35, 0.80, 0.35, 1.0),       # Green - files to add
        'bg': (0.12, 0.14, 0.18, 1.0),        # Dark background
        'text': (0.92, 0.92, 0.94, 1.0),
        'text_dim': (0.60, 0.65, 0.70, 1.0),
    }
    
    def __init__(self, size: int = 140, **kwargs):
        super().__init__(**kwargs)
        self._size = size
        self.set_content_width(size)
        self.set_content_height(size)
        self.set_draw_func(self._on_draw)
        
        # Data
        self._segments: list[DonutSegment] = []
        self._total = 0
        self._animated = False
        self._animation_progress = 0.0
    
    def set_data(self, keep_bytes: int, remove_bytes: int, add_bytes: int, animate: bool = True):
        """Update chart data"""
        total = keep_bytes + remove_bytes + add_bytes
        
        if total == 0:
            self._segments = []
            self._total = 0
            self.queue_draw()
            return
        
        self._segments = [
            DonutSegment(
                value=keep_bytes,
                color=self.COLORS['keep'],
                label="Conservar",
                percentage=keep_bytes / total * 100 if total > 0 else 0
            ),
            DonutSegment(
                value=remove_bytes,
                color=self.COLORS['remove'],
                label="Eliminar",
                percentage=remove_bytes / total * 100 if total > 0 else 0
            ),
            DonutSegment(
                value=add_bytes,
                color=self.COLORS['add'],
                label="Añadir",
                percentage=add_bytes / total * 100 if total > 0 else 0
            ),
        ]
        
        # Filter out zero segments
        self._segments = [s for s in self._segments if s.value > 0]
        self._total = total
        
        if animate:
            self._start_animation()
        else:
            self._animation_progress = 1.0
        
        self.queue_draw()
    
    def _start_animation(self):
        """Start entrance animation"""
        self._animated = True
        self._animation_progress = 0.0
        
        def animate():
            if not self._animated:
                return False
            self._animation_progress = min(1.0, self._animation_progress + 0.05)
            self.queue_draw()
            return self._animation_progress < 1.0
        
        import gi
        from gi.repository import GLib
        GLib.timeout_add(16, animate)  # ~60 FPS
    
    def _on_draw(self, area: Gtk.DrawingArea, cr: Cairo.Context, width: int, height: int):
        # Background
        cr.set_source_rgba(*self.COLORS['bg'])
        cr.paint()
        
        if not self._segments:
            self._draw_empty(cr, width, height)
            return
        
        # Center and radius
        cx, cy = width / 2, height / 2
        outer_radius = min(width, height) / 2 - 8
        inner_radius = outer_radius * 0.55
        
        # Draw segments
        start_angle = -math.pi / 2  # Start at top
        
        for segment in self._segments:
            sweep = (segment.percentage / 100) * 2 * math.pi * self._animation_progress
            
            if sweep <= 0:
                continue
            
            # Draw segment path
            cr.new_path()
            
            # Outer arc
            cr.arc(cx, cy, outer_radius, start_angle, start_angle + sweep)
            
            # Inner arc (reverse)
            cr.arc(cx, cy, inner_radius, start_angle + sweep, start_angle)
            cr.close_path()
            
            # Fill
            cr.set_source_rgba(*segment.color)
            cr.fill_preserve()
            
            # Subtle border
            cr.set_source_rgba(1, 1, 1, 0.1)
            cr.set_line_width(1)
            cr.stroke()
            
            start_angle += sweep
        
        # Center text
        self._draw_center_text(cr, cx, cy, inner_radius)
    
    def _draw_empty(self, cr: Cairo.Context, w: int, h: int):
        cr.set_source_rgba(*self.COLORS['text_dim'])
        cr.select_font_face("Sans", Cairo.FontSlant.NORMAL, Cairo.FontWeight.NORMAL)
        cr.set_font_size(12)
        text = "Sin datos"
        extents = cr.text_extents(text)
        cr.move_to((w - extents.width) / 2, (h + extents.height) / 2)
        cr.show_text(text)
    
    def _draw_center_text(self, cr: Cairo.Context, cx: float, cy: float, inner_radius: float):
        if not self._segments:
            return
        
        # Main total
        cr.select_font_face("Sans", Cairo.FontSlant.NORMAL, Cairo.FontWeight.BOLD)
        cr.set_font_size(18)
        cr.set_source_rgba(*self.COLORS['text'])
        
        total_str = self._format_bytes(self._total)
        extents = cr.text_extents(total_str)
        cr.move_to(cx - extents.width / 2, cy + extents.height / 2 - 4)
        cr.show_text(total_str)
        
        # Subtitle
        cr.set_font_size(10)
        cr.set_source_rgba(*self.COLORS['text_dim'])
        sub = "Total estimado"
        extents = cr.text_extents(sub)
        cr.move_to(cx - extents.width / 2, cy + 22)
        cr.show_text(sub)
    
    def _format_bytes(self, bytes_val: int) -> str:
        units = ['B', 'KB', 'MB', 'GB', 'TB']
        size = float(bytes_val)
        unit = 0
        while size >= 1024 and unit < len(units) - 1:
            size /= 1024
            unit += 1
        return f"{size:.1f} {units[unit]}"


class DonutLegendWidget(Gtk.Box):
    """Legend for donut chart"""
    
    def __init__(self, chart: DonutChartWidget, **kwargs):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8, **kwargs)
        self._chart = chart
        self._rows = []
        self._build()
    
    def _build(self):
        for i in range(3):
            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            
            # Color dot
            dot = Gtk.DrawingArea()
            dot.set_content_width(12)
            dot.set_content_height(12)
            dot.set_draw_func(lambda a, cr, w, h, idx=i: self._draw_dot(cr, w, h, idx))
            
            # Labels
            label_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
            name_label = Gtk.Label(label="—")
            name_label.set_halign(Gtk.Align.START)
            name_label.add_css_class("caption")
            
            value_label = Gtk.Label(label="—")
            value_label.set_halign(Gtk.Align.START)
            value_label.add_css_class("dim-label")
            
            label_box.append(name_label)
            label_box.append(value_label)
            
            row.append(dot)
            row.append(label_box)
            self.append(row)
            self._rows.append((dot, name_label, value_label))
    
    def _draw_dot(self, cr: Cairo.Context, w: int, h: int, idx: int):
        colors = [
            (0.20, 0.55, 0.95, 1.0),
            (0.95, 0.55, 0.20, 1.0),
            (0.35, 0.80, 0.35, 1.0),
        ]
        cr.set_source_rgba(*colors[idx])
        cr.arc(w/2, h/2, 5, 0, 2 * math.pi)
        cr.fill()
    
    def update(self, segments: list):
        """Update legend from chart segments"""
        labels = ["Conservar", "Eliminar", "Añadir"]
        for i, (dot, name_label, value_label) in enumerate(self._rows):
            if i < len(segments):
                seg = segments[i]
                name_label.set_text(f"{seg.label}: {seg.percentage:.0f}%")
                value_label.set_text(self._format_bytes(int(seg.value)))
            else:
                name_label.set_text("—")
                value_label.set_text("—")
    
    def _format_bytes(self, bytes_val: int) -> str:
        units = ['B', 'KB', 'MB', 'GB', 'TB']
        size = float(bytes_val)
        unit = 0
        while size >= 1024 and unit < len(units) - 1:
            size /= 1024
            unit += 1
        return f"{size:.1f} {units[unit]}"