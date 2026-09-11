"""Analysis view showing storage report and workspace selection."""
from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Gdk, GObject, Gtk

from ..core.models import AnalysisReport, human_size
from ..widgets.queue_panel import QueueItem, QueuePanelWidget
from ..widgets.storage_donut import StorageDonut


class AnalysisView(Gtk.Box):
    """Shows analysis results and lets user pick workspace."""

    __gtype_name__ = "TripleWrapperAnalysisView"
    __gsignals__ = {  # noqa: RUF012 - GObject signal map must be a dict
        "start-requested": (GObject.SignalFlags.RUN_FIRST, None, ()),
        "enqueue-requested": (GObject.SignalFlags.RUN_FIRST, None, ()),
        "browse-requested": (GObject.SignalFlags.RUN_FIRST, None, ()),
        "back-requested": (GObject.SignalFlags.RUN_FIRST, None, ()),
        "mount-requested": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
    }

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=18)
        self._report: AnalysisReport | None = None
        self._selected_workspace: str | None = None
        self._build_ui()

    def _build_ui(self) -> None:
        # Scrollable content: lets the window shrink vertically on short
        # screens instead of locking its minimum height. Action bar stays
        # visible below.
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=18)
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)
        scrolled.set_child(content)
        self.append(scrolled)

        # Header
        header = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        header.set_halign(Gtk.Align.CENTER)

        title = Gtk.Label(label="Análisis de espacio")
        title.add_css_class("title-2")
        header.append(title)

        subtitle = Gtk.Label(label="Revisa los requerimientos antes de iniciar")
        subtitle.add_css_class("dim-label")
        header.append(subtitle)
        content.append(header)

        # Two-column responsive grid
        grid = Gtk.Grid(column_spacing=18, row_spacing=18)
        grid.set_halign(Gtk.Align.CENTER)

        # Donut card
        donut_card = Gtk.Frame()
        donut_card.add_css_class("card")
        donut_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        donut_box.set_margin_top(18)
        donut_box.set_margin_bottom(18)
        donut_box.set_margin_start(18)
        donut_box.set_margin_end(18)
        donut_card.set_child(donut_box)

        donut_title = Gtk.Label(label="Uso de almacenamiento")
        donut_title.add_css_class("heading")
        donut_title.set_halign(Gtk.Align.START)
        donut_box.append(donut_title)

        self._donut = StorageDonut()
        self._donut.set_size_request(220, 220)
        self._donut.set_halign(Gtk.Align.CENTER)
        donut_box.append(self._donut)

        self._legend = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        self._legend.set_halign(Gtk.Align.START)
        donut_box.append(self._legend)

        grid.attach(donut_card, 0, 0, 1, 1)

        # Info card
        info_card = Gtk.Frame()
        info_card.add_css_class("card")
        info_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        info_box.set_margin_top(18)
        info_box.set_margin_bottom(18)
        info_box.set_margin_start(18)
        info_box.set_margin_end(18)
        info_card.set_child(info_box)

        info_title = Gtk.Label(label="Requerimientos")
        info_title.add_css_class("heading")
        info_title.set_halign(Gtk.Align.START)
        info_box.append(info_title)

        self._info_rows = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        info_box.append(self._info_rows)

        grid.attach(info_card, 1, 0, 1, 1)

        content.append(grid)

        # Workspace suggestion
        self._workspace_card = Gtk.Frame()
        self._workspace_card.add_css_class("card")
        ws_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        ws_box.set_margin_top(16)
        ws_box.set_margin_bottom(16)
        ws_box.set_margin_start(16)
        ws_box.set_margin_end(16)
        self._workspace_card.set_child(ws_box)

        self._ws_title = Gtk.Label(label="Workspace sugerido")
        self._ws_title.add_css_class("heading")
        self._ws_title.set_halign(Gtk.Align.START)
        ws_box.append(self._ws_title)

        self._ws_label = Gtk.Label(label="—")
        self._ws_label.set_halign(Gtk.Align.START)
        self._ws_label.set_wrap(True)
        self._ws_label.set_selectable(True)
        ws_box.append(self._ws_label)

        self._ws_status = Gtk.Label()
        self._ws_status.set_halign(Gtk.Align.START)
        ws_box.append(self._ws_status)

        self._crypto_label = Gtk.Label(label="")
        self._crypto_label.set_halign(Gtk.Align.START)
        self._crypto_label.add_css_class("dim-label")
        ws_box.append(self._crypto_label)

        self._integrity_label = Gtk.Label(label="Integridad local: sin datos")
        self._integrity_label.set_halign(Gtk.Align.START)
        self._integrity_label.add_css_class("dim-label")
        self._integrity_label.set_wrap(True)
        ws_box.append(self._integrity_label)

        content.append(self._workspace_card)

        # Unmounted devices card (USB-HDD/SSD plugged but not mounted)
        devices_card = Gtk.Frame()
        devices_card.add_css_class("card")
        dev_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=10)
        dev_box.set_margin_top(16)
        dev_box.set_margin_bottom(16)
        dev_box.set_margin_start(16)
        dev_box.set_margin_end(16)
        devices_card.set_child(dev_box)

        dev_title = Gtk.Label(label="Dispositivos sin montar")
        dev_title.add_css_class("heading")
        dev_title.set_halign(Gtk.Align.START)
        dev_box.append(dev_title)

        dev_hint = Gtk.Label(label="¿Conectaste un USB y no aparece arriba? Móntalo aquí.")
        dev_hint.add_css_class("dim-label")
        dev_hint.set_halign(Gtk.Align.START)
        dev_hint.set_wrap(True)
        dev_box.append(dev_hint)

        self._devices_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        dev_box.append(self._devices_box)

        content.append(devices_card)

        # Action bar
        actions = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        actions.set_halign(Gtk.Align.CENTER)

        back_btn = Gtk.Button(label="Atrás")
        back_btn.connect("clicked", lambda *_: self.emit("back-requested"))
        actions.append(back_btn)

        self._start_btn = Gtk.Button(label="Iniciar")
        self._start_btn.add_css_class("suggested-action")
        self._start_btn.add_css_class("pill")
        self._start_btn.connect("clicked", lambda *_: self.emit("start-requested"))
        actions.append(self._start_btn)

        enqueue_btn = Gtk.Button(label="Encolar")
        enqueue_btn.add_css_class("pill")
        enqueue_btn.connect("clicked", lambda *_: self.emit("enqueue-requested"))
        actions.append(enqueue_btn)

        browse_btn = Gtk.Button(label="Explorar contenido")
        browse_btn.add_css_class("pill")
        browse_btn.connect("clicked", lambda *_: self.emit("browse-requested"))
        actions.append(browse_btn)

        self.append(actions)

        # Queue card (real backend data)
        queue_card = Gtk.Frame()
        queue_card.add_css_class("card")
        queue_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        queue_box.set_margin_top(16)
        queue_box.set_margin_bottom(16)
        queue_box.set_margin_start(16)
        queue_box.set_margin_end(16)
        queue_card.set_child(queue_box)

        queue_title = Gtk.Label(label="Cola de operaciones")
        queue_title.add_css_class("heading")
        queue_title.set_halign(Gtk.Align.START)
        queue_box.append(queue_title)

        self._queue_panel = QueuePanelWidget()
        self._queue_panel.set_size_request(0, 220)
        queue_box.append(self._queue_panel)

        content.append(queue_card)

    # ----------------------------------------------------------------- API
    def set_report(self, report: AnalysisReport) -> None:
        self._report = report
        self._selected_workspace = report.suggested_workspace

        # Donut segments
        self._donut.set_segments(
            [
                ("Usado", report.used_bytes, "#3584e4"),
                ("Archivo", report.archive_bytes, "#ff7800"),
                ("Libre", report.free_bytes, "#2ec27e"),
                ("Necesario", report.needed_bytes, "#c061cb"),
            ]
        )

        # Legend
        for child in list(self._legend):
            self._legend.remove(child)
        for label, color in self._donut.get_legend():
            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            swatch = Gtk.DrawingArea()
            swatch.set_size_request(12, 12)
            swatch.set_draw_func(lambda w, ctx, *a, c=color: self._draw_swatch(ctx, c))
            row.append(swatch)
            row.append(Gtk.Label(label=label, xalign=0))
            self._legend.append(row)

        # Info rows
        for child in list(self._info_rows):
            self._info_rows.remove(child)
        for label, value in [
            ("Tamaño del archivo", report.human_archive),
            ("Espacio necesario (in-place)", report.human_needed),
            ("Espacio libre en origen", report.human_free_source),
            ("Estado", report.status_label),
        ]:
            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            row.append(Gtk.Label(label=label, xalign=0))
            spacer = Gtk.Box()
            spacer.set_hexpand(True)
            row.append(spacer)
            v = Gtk.Label(label=value)
            v.add_css_class("dim-label")
            row.append(v)
            self._info_rows.append(row)

        # Workspace card
        self._ws_label.set_label(report.suggested_workspace)
        if report.encrypted:
            self._crypto_label.set_label("🔒 Archivo cifrado (AES) — pedirá contraseña al iniciar")
        else:
            self._crypto_label.set_label("Sin cifrado detectado")
        self._integrity_label.set_label("Integridad local: consultando…")
        self._ws_status.set_label(report.status_label)
        self._ws_status.remove_css_class("success")
        self._ws_status.remove_css_class("warning")
        self._ws_status.remove_css_class("error")
        if report.status == "ok":
            self._ws_status.add_css_class("success")
        elif report.status == "external":
            self._ws_status.add_css_class("warning")
        else:
            self._ws_status.add_css_class("error")
            self._start_btn.set_sensitive(False)

    def get_selected_workspace(self) -> str | None:
        return self._selected_workspace

    def set_integrity_text(self, text: str) -> None:
        self._integrity_label.set_label(f"Integridad local: {text}")

    def set_devices(self, devices: list[dict]) -> None:
        """Show unmounted devices, each with its Montar button."""
        for child in list(self._devices_box):
            self._devices_box.remove(child)
        if not devices:
            empty = Gtk.Label(label="Nada por montar — todo lo conectado ya está disponible.")
            empty.add_css_class("dim-label")
            empty.set_halign(Gtk.Align.START)
            self._devices_box.append(empty)
            return
        for dev in devices:
            row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
            info = f"{dev.get('dev_path', '?')} · {dev.get('fstype', '?')} · {human_size(dev.get('size_bytes', 0))}"
            if dev.get("removable"):
                info += " · extraíble"
            label = Gtk.Label(label=info)
            label.set_halign(Gtk.Align.START)
            label.set_hexpand(True)
            row.append(label)
            mount_btn = Gtk.Button(label="Montar")
            mount_btn.add_css_class("pill")
            dev_path = dev.get("dev_path", "")
            mount_btn.connect(
                "clicked",
                lambda _b, d=dev_path: self.emit("mount-requested", d),
            )
            row.append(mount_btn)
            self._devices_box.append(row)

    def set_queue_items(self, items: list[QueueItem]) -> None:
        self._queue_panel.set_items(items)

    # --------------------------------------------------------------- Helpers
    @staticmethod
    def _draw_swatch(ctx, color: str) -> None:
        rgba = Gdk.RGBA()
        rgba.parse(color)
        ctx.set_source_rgba(rgba.red, rgba.green, rgba.blue, rgba.alpha)
        ctx.rectangle(0, 0, 12, 12)
        ctx.fill()