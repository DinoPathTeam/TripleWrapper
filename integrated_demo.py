#!/usr/bin/env python3
"""
Integración: StorageDecisionEngine + SteamGraphWidget
Demuestra cómo conectar las señales de telemetría del motor al widget Cairo/GTK4.
"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('Gdk', '4.0')
from gi.repository import Gtk, Gdk, GLib, GObject
import cairo
import math
import random
import time
from collections import deque
from dataclasses import dataclass, field
from typing import Deque, List, Tuple, Optional
from enum import Enum

# Importar el motor de almacenamiento
import sys
sys.path.insert(0, "/home/adrexcou/Escritorio/TripleWrapper")
from storage_engine import (
    StorageDecisionEngine, TelemetrySignal, StorageVerdict, 
    VerdictType, DiskInfo, CompressionEstimate
)


# ============================================================
# STEAM GRAPH WIDGET (versión simplificada para integración)
# ============================================================

class SteamGraphWidget(Gtk.DrawingArea):
    """Widget de gráficas estilo Steam - recibe telemetría vía GObject signals."""

    MAX_SAMPLES = 300
    UPDATE_INTERVAL_MS = 50  # 20 FPS suave

    COLORS = {
        'bg': (0.08, 0.09, 0.11, 1.0),
        'grid': (0.25, 0.28, 0.32, 0.4),
        'read': (0.2, 0.65, 0.95, 1.0),
        'write': (0.95, 0.65, 0.2, 1.0),
        'compress': (0.4, 0.85, 0.4, 1.0),
        'read_fill': (0.2, 0.65, 0.95, 0.15),
        'write_fill': (0.95, 0.65, 0.2, 0.15),
        'compress_fill': (0.4, 0.85, 0.4, 0.12),
        'text': (0.92, 0.92, 0.94, 1.0),
        'text_dim': (0.6, 0.65, 0.7, 1.0),
        'current_file_bg': (0.12, 0.14, 0.18, 0.95),
    }

    def __init__(self):
        super().__init__()
        self.set_content_width(600)
        self.set_content_height(160)
        self.set_draw_func(self.on_draw)

        self.samples: Deque[Tuple[float, float, float]] = deque(maxlen=self.MAX_SAMPLES)
        self.max_speed = 100.0
        self.current_file = "Esperando operación..."
        self.eta_seconds = 0
        self.total_bytes = 0
        self.processed_bytes = 0
        self._running = False

    def connect_telemetry(self, telemetry: TelemetrySignal):
        """Conecta señales del StorageDecisionEngine.TelemetrySignal."""
        telemetry.connect('speed-update', self.on_speed_update)
        telemetry.connect('progress-update', self.on_progress_update)
        telemetry.connect('operation-complete', self.on_operation_complete)
        telemetry.connect('operation-error', self.on_operation_error)

    def on_speed_update(self, _source, read_mbps: float, write_mbps: float, 
                        compress_mbps: float, current_file: str):
        """Callback para señal speed-update."""
        self.samples.append((read_mbps, write_mbps, compress_mbps))
        self.current_file = current_file or self.current_file
        
        # Auto-escalar max_speed
        peak = max(read_mbps, write_mbps, compress_mbps)
        if peak > self.max_speed:
            self.max_speed = peak * 1.15
        elif peak < self.max_speed * 0.3 and self.max_speed > 50:
            self.max_speed = max(50, self.max_speed * 0.99)
        
        self.queue_draw()

    def on_progress_update(self, _source, processed: float, total: float, eta: int):
        self.processed_bytes = int(processed)
        self.total_bytes = int(total)
        self.eta_seconds = eta
        self.queue_draw()

    def on_operation_complete(self, _source, success: bool, msg: str):
        self.current_file = f"✓ {msg}" if success else f"✗ {msg}"
        self._running = False
        self.queue_draw()

    def on_operation_error(self, _source, msg: str):
        self.current_file = f"✗ Error: {msg}"
        self._running = False
        self.queue_draw()

    def on_draw(self, area: Gtk.DrawingArea, cr: cairo.Context, width: int, height: int):
        # Background
        cr.set_source_rgba(*self.COLORS['bg'])
        cr.paint()

        if len(self.samples) < 2:
            self._draw_empty_state(cr, width, height)
            return

        # Grid
        self._draw_grid(cr, width, height)
        
        # Graph lines (read, write, compress)
        for i, (attr_idx, fill_color, line_color) in enumerate([
            (0, self.COLORS['read_fill'], self.COLORS['read']),
            (1, self.COLORS['write_fill'], self.COLORS['write']),
            (2, self.COLORS['compress_fill'], self.COLORS['compress']),
        ]):
            self._draw_line(cr, width, height, attr_idx, fill_color, line_color)

        # Legend
        self._draw_legend(cr, width, height)
        
        # Status overlay
        self._draw_status(cr, width, height)

    def _draw_empty_state(self, cr, w, h):
        cr.set_source_rgba(*self.COLORS['text_dim'])
        cr.select_font_face("Sans", cairo.FontSlant.NORMAL, cairo.FontWeight.NORMAL)
        cr.set_font_size(14)
        cr.move_to(w/2 - 80, h/2)
        cr.show_text("Sin datos de telemetría")

    def _draw_grid(self, cr, w, h):
        cr.set_source_rgba(*self.COLORS['grid'])
        cr.set_line_width(1)
        cr.set_dash([4, 4], 0)
        
        m_l, m_r, m_t, m_b = 50, 20, 10, 45
        gw, gh = w - m_l - m_r, h - m_t - m_b
        
        for i in range(5):
            y = m_t + gh * i / 4
            cr.move_to(m_l, y)
            cr.line_to(w - m_r, y)
            cr.stroke()
            speed = self.max_speed * (1 - i / 4)
            cr.set_source_rgba(*self.COLORS['text_dim'])
            cr.set_font_size(10)
            cr.move_to(4, y + 3)
            cr.show_text(f"{speed:.0f}")
        
        for i in range(6):
            x = m_l + gw * i / 5
            cr.move_to(x, m_t)
            cr.line_to(x, h - m_b)
            cr.stroke()
        
        cr.set_dash([], 0)

    def _draw_line(self, cr, w, h, attr_idx, fill_color, line_color):
        m_l, m_r, m_t, m_b = 50, 20, 10, 45
        gw, gh = w - m_l - m_r, h - m_t - m_b
        x0, y0 = m_l, m_t
        
        cr.new_path()
        first = True
        n = len(self.samples)
        
        for i, sample in enumerate(self.samples):
            speed = sample[attr_idx]
            x = x0 + gw * i / (self.MAX_SAMPLES - 1)
            y = y0 + gh * (1 - min(speed / self.max_speed, 1.0))
            if first:
                cr.move_to(x, y)
                first = False
            else:
                cr.line_to(x, y)
        
        if not first:
            cr.line_to(x, y0 + gh)
            cr.line_to(x0, y0 + gh)
            cr.close_path()
            cr.set_source_rgba(*fill_color)
            cr.fill_preserve()
            cr.set_source_rgba(*line_color)
            cr.set_line_width(2)
            cr.stroke()

    def _draw_legend(self, cr, w, h):
        items = [("Lectura", self.COLORS['read']), 
                 ("Escritura", self.COLORS['write']), 
                 ("Compresión", self.COLORS['compress'])]
        cr.select_font_face("Sans", cairo.FontSlant.NORMAL, cairo.FontWeight.NORMAL)
        cr.set_font_size(11)
        x, y = w - 180, m_t + 10
        for label, color in items:
            cr.set_source_rgba(*color)
            cr.arc(x + 6, y + 6, 4, 0, 2 * math.pi)
            cr.fill()
            cr.set_source_rgba(*self.COLORS['text'])
            cr.move_to(x + 16, y + 11)
            cr.show_text(label)
            y += 22

    def _draw_status(self, cr, w, h):
        bar_h = 32
        bar_y = h - bar_h - 4
        cr.set_source_rgba(*self.COLORS['current_file_bg'])
        cr.rectangle(4, bar_y, w - 8, bar_h)
        cr.fill()
        
        cr.select_font_face("Sans", cairo.FontSlant.NORMAL, cairo.FontWeight.NORMAL)
        cr.set_font_size(12)
        cr.set_source_rgba(*self.COLORS['text'])
        cr.move_to(12, bar_y + 20)
        cr.show_text(self.current_file)
        
        if self.eta_seconds > 0:
            eta_str = self._fmt_eta(self.eta_seconds)
            cr.set_font_size(11)
            cr.set_source_rgba(*self.COLORS['text_dim'])
            cr.move_to(w - 120, bar_y + 20)
            cr.show_text(f"ETA: {eta_str}")
        
        if self.total_bytes > 0:
            pct = min(100, (self.processed_bytes / self.total_bytes) * 100)
            pb_w = 100
            pb_x = w - pb_w - 12
            pb_y = bar_y + 10
            cr.set_source_rgba(0.15, 0.18, 0.22, 1.0)
            cr.rectangle(pb_x, pb_y, pb_w, 5)
            cr.fill()
            cr.set_source_rgba(*self.COLORS['write'])
            cr.rectangle(pb_x, pb_y, pb_w * pct / 100, 5)
            cr.fill()

    def _fmt_eta(self, s: float) -> str:
        if s < 60: return f"{int(s)}s"
        if s < 3600: return f"{int(s/60)}m {int(s%60)}s"
        return f"{int(s/3600)}h {int((s%3600)/60)}m"


# ============================================================
# VENTANA PRINCIPAL CON INTEGRACIÓN COMPLETA
# ============================================================

class TripleWrapperWindow(Gtk.ApplicationWindow):
    def __init__(self, app):
        super().__init__(application=app, title="TripleWrapper - Storage Engine + Graph")
        self.set_default_size(1000, 650)
        
        # Motor de almacenamiento
        self.engine = StorageDecisionEngine()
        self.verdict: Optional[StorageVerdict] = None
        
        # UI
        self._build_ui()
        self._apply_css()
        
        # Conectar telemetría al gráfico
        self.graph.connect_telemetry(self.engine.telemetry)

    def _build_ui(self):
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        self.set_child(main_box)
        
        # Header
        header = Gtk.HeaderBar()
        header.set_title_widget(Gtk.Label(label="TripleWrapper"))
        self.set_titlebar(header)
        
        # PANEL SUPERIOR - Estilo GNOME Disks
        top_panel = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        top_panel.set_margin_top(24)
        top_panel.set_margin_bottom(12)
        top_panel.set_margin_start(24)
        top_panel.set_margin_end(24)
        
        # Título archivo
        self.file_label = Gtk.Label(label="Archivo: —  |  Tamaño: —")
        self.file_label.add_css_class("title-2")
        self.file_label.set_halign(Gtk.Align.START)
        top_panel.append(self.file_label)
        
        # Info discos + donut chart placeholder
        info_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=24)
        
        # Disco origen
        self.src_disk_label = Gtk.Label(label="Disco origen: —")
        self.src_disk_label.set_halign(Gtk.Align.START)
        info_box.append(self.src_disk_label)
        
        # Disco caché
        self.cache_disk_label = Gtk.Label(label="Caché: —")
        self.cache_disk_label.set_halign(Gtk.Align.START)
        info_box.append(self.cache_disk_label)
        
        top_panel.append(info_box)
        
        # Botones acción
        btn_box = Gtk.Box(spacing=12)
        for label, css, callback in [
            ("Analizar", "suggested-action", self.on_analyze),
            ("Simular Operación", "", self.on_simulate),
            ("Limpiar", "destructive-action", self.on_clear),
        ]:
            btn = Gtk.Button(label=label)
            if css:
                btn.add_css_class(css)
            btn.set_size_request(160, 44)
            btn.connect("clicked", callback)
            btn_box.append(btn)
        top_panel.append(btn_box)
        
        main_box.append(top_panel)
        
        # Separador
        main_box.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        
        # PANEL INFERIOR - Gráfica Steam
        graph_frame = Gtk.Frame()
        self.graph = SteamGraphWidget()
        graph_frame.set_child(self.graph)
        main_box.append(graph_frame)

    def _apply_css(self):
        css = b"""
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
        provider.load_from_data(css)
        Gtk.StyleContext.add_provider_for_display(
            Gdk.Display.get_default(), provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
        )

    def on_analyze(self, _btn):
        """Analiza el archivo y muestra veredicto."""
        # Usar archivo de prueba real
        test_file = "/tmp/test_archive.zip"
        if not os.path.exists(test_file):
            with open(test_file, "wb") as f:
                f.write(b"x" * (2 * 1024**3))
        
        # Simular: archivo 2GB, quitar 500MB, añadir 1.5GB
        self.verdict = self.engine.decide_workspace(
            test_file,
            self.engine.calculate_estimate(
                current_zip_size=2 * 1024**3,
                bytes_to_remove=500 * 1024**2,
                bytes_to_add=1500 * 1024**2,
                compression_ratio=0.45
            )
        )
        self._update_verdict_ui()

    def _update_verdict_ui(self):
        if not self.verdict:
            return
            
        v = self.verdict
        self.file_label.set_text(
            f"Archivo: test_archive.zip  |  "
            f"Actual: {v.estimate.current_zip_size/(1024**3):.1f} GB  →  "
            f"Estimado: {v.estimate.estimated_final_size/(1024**3):.1f} GB"
        )
        
        src = v.source_disk
        self.src_disk_label.set_text(
            f"📀 Origen: {src.label} ({src.mount_point})  "
            f"| Libre: {src.free_gb:.1f} GB / {src.total_gb:.1f} GB"
        )
        
        if v.workspace_disk:
            ws = v.workspace_disk
            self.cache_disk_label.set_text(
                f"💾 Caché: {ws.label} ({ws.mount_point})  "
                f"| Libre: {ws.free_gb:.1f} GB  |  7z param: {v.sevenzip_workdir_param}"
            )
        else:
            self.cache_disk_label.set_text("💾 Caché: Interna (temp por defecto)")
        
        # Mostrar diálogo si requiere confirmación
        if v.requires_user_confirmation:
            dialog = Gtk.MessageDialog(
                transient_for=self,
                modal=True,
                message_type=Gtk.MessageType.WARNING,
                buttons=Gtk.ButtonsType.OK_CANCEL,
                text="Espacio insuficiente en disco origen",
            )
            dialog.format_secondary_text(v.message)
            dialog.connect("response", self._on_confirm_response)
            dialog.present()

    def _on_confirm_response(self, dialog, response):
        if response == Gtk.ResponseType.OK:
            self.on_simulate(None)
        dialog.destroy()

    def on_simulate(self, _btn):
        """Inicia simulación de operación con telemetría."""
        if not self.verdict:
            return
        self.graph.current_file = "Iniciando operación..."
        self.graph._running = True
        # Ejecutar simulación de telemetría en hilo
        import threading
        threading.Thread(target=self._run_simulation, daemon=True).start()

    def _run_simulation(self):
        """Simula telemetría realista (en producción: parsear 7z + /proc/pid/io)."""
        files = ["pakchunk0-Windows.pak", "pakchunk1-Windows.pak", 
                 "assets/audio.bank", "assets/textures.pak", "shaders/cache.bin"]
        total = sum(random.randint(500_000_000, 2_000_000_000) for _ in files)
        processed = 0
        start = time.time()
        
        for fname in files:
            if not self.graph._running:
                break
            fsize = random.randint(500_000_000, 2_000_000_000)
            self.engine.telemetry.set_current_file(fname)
            
            for _ in range(max(1, fsize // (32 * 1024**2))):
                if not self.graph._running:
                    break
                elapsed = time.time() - start
                phase = elapsed * 0.6
                
                r = max(0, 100 + 60 * math.sin(phase) + random.uniform(-15, 15))
                w = max(0, 80 + 50 * math.sin(phase + 1.2) + random.uniform(-10, 10))
                c = max(0, 40 + 30 * math.sin(phase + 2.5) + random.uniform(-5, 5))
                
                self.engine.telemetry.emit_speeds(r, w, c)
                
                chunk = fsize // max(1, fsize // (32 * 1024**2))
                processed += chunk
                eta = int((total - processed) / max(w * 1_000_000, 0.1))
                self.engine.telemetry.emit_progress(processed, total, eta)
                
                time.sleep(0.04)
        
        if self.graph._running:
            self.engine.telemetry.emit_complete(True, "Operación completada")

    def on_clear(self, _btn):
        self.graph.samples.clear()
        self.graph.current_file = "Esperando operación..."
        self.graph.queue_draw()
        self.file_label.set_text("Archivo: —  |  Tamaño: —")
        self.src_disk_label.set_text("Disco origen: —")
        self.cache_disk_label.set_text("Caché: —")


class Application(Gtk.Application):
    def __init__(self):
        super().__init__(application_id="com.triplewrapper.integrated")

    def do_activate(self):
        win = TripleWrapperWindow(self)
        win.present()


if __name__ == "__main__":
    import os
    app = Application()
    app.run([])