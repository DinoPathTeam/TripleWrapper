"""Main application window with view stack."""
from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gio, Gtk

from .core.bridge import CoreBridge
from .views.analysis_view import AnalysisView
from .views.progress_view import ProgressView
from .views.welcome_view import WelcomeView


class TripleWrapperWindow(Adw.ApplicationWindow):
    """Primary window holding the navigation stack."""

    def __init__(self, **kwargs) -> None:
        super().__init__(
            title="TripleWrapper",
            default_width=900,
            default_height=680,
            **kwargs,
        )

        self._bridge = CoreBridge()
        self.build_ui()
        self.bind_signals()

    # ------------------------------------------------------------------ UI
    def build_ui(self) -> None:
        # Top-level layout
        self.main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.set_content(self.main_box)

        # Header bar
        self._header = Adw.HeaderBar()
        self._header.set_show_end_title_buttons(True)

        # Style menu
        style_menu = Gio.Menu()
        style_section = Gio.Menu()
        style_section.append("Claro", "app.style-light")
        style_section.append("Oscuro", "app.style-dark")
        style_section.append("Sistema", "app.style-default")
        style_menu.append_section("Apariencia", style_section)

        menu_btn = Gtk.MenuButton(menu_model=style_menu)
        menu_btn.set_icon_name("open-menu-symbolic")
        menu_btn.set_tooltip_text("Preferencias")
        self._header.pack_end(menu_btn)

        self.main_box.append(self._header)

        # Navigation view (responsive: collapses on narrow widths)
        self._nav = Adw.NavigationView()
        self.main_box.append(self._nav)

        # View stack
        self._stack = Gtk.Stack()
        self._stack.set_transition_type(Gtk.StackTransitionType.CROSSFADE)
        self._stack.set_transition_duration(220)
        self._stack.set_vexpand(True)
        self._stack.set_hexpand(True)

        # Wrap stack in a clamp for responsive padding on large screens
        clamp = Adw.Clamp()
        clamp.set_maximum_size(1100)
        clamp.set_tightening_threshold(720)
        clamp.set_child(self._stack)
        clamp.set_margin_top(18)
        clamp.set_margin_bottom(18)
        clamp.set_margin_start(18)
        clamp.set_margin_end(18)

        self._toast_overlay = Adw.ToastOverlay()
        self._toast_overlay.set_child(clamp)

        page = Adw.NavigationPage(title="TripleWrapper")
        page.set_child(self._toast_overlay)
        self._nav.push(page)

        # Views
        self._welcome = WelcomeView()
        self._analysis = AnalysisView()
        self._progress = ProgressView()

        self._stack.add_named(self._welcome, "welcome")
        self._stack.add_named(self._analysis, "analysis")
        self._stack.add_named(self._progress, "progress")

        self._stack.set_visible_child_name("welcome")

    # --------------------------------------------------------------- Events
    def bind_signals(self) -> None:
        self._welcome.connect("file-selected", self.on_file_selected)
        self._welcome.connect("analyze-requested", self.on_analyze_requested)

        self._analysis.connect("start-requested", self.on_start_requested)
        self._analysis.connect("enqueue-requested", self.on_enqueue_requested)
        self._analysis.connect("back-requested", lambda *_: self.go_welcome())

        self._progress.connect("done", self.on_done)
        self._progress.connect("cancel-requested", self.on_cancel)

        self._bridge.connect("analysis-ready", self.on_analysis_ready)
        self._bridge.connect("analysis-failed", self.on_analysis_failed)
        self._bridge.connect("progress-tick", self.on_progress_tick)
        self._bridge.connect("operation-failed", self.on_operation_failed)

    # ------------------------------------------------------------- Navigation
    def go_welcome(self) -> None:
        self._stack.set_visible_child_name("welcome")

    def go_analysis(self) -> None:
        self._stack.set_visible_child_name("analysis")
        self.refresh_queue()

    def go_progress(self) -> None:
        self._stack.set_visible_child_name("progress")

    # -------------------------------------------------------------- Handlers
    def on_file_selected(self, view, path: str) -> None:
        self._bridge.set_archive_path(path)

    def on_analyze_requested(self, view) -> None:
        self._welcome.set_busy(True)
        self._bridge.analyze()

    def on_analysis_ready(self, bridge, report) -> None:
        self._welcome.set_busy(False)
        self._report = report
        self._analysis.set_report(report)
        self.go_analysis()
        self.refresh_integrity_label()

    def on_analysis_failed(self, bridge, message: str) -> None:
        self._welcome.set_busy(False)
        self.show_toast(message, priority=Adw.ToastPriority.HIGH)

    def on_start_requested(self, view) -> None:
        workspace = self._analysis.get_selected_workspace()
        password = self.ensure_password()
        if password is None and self._is_encrypted():
            return  # user cancelled the password prompt
        self._progress.reset()
        self.go_progress()
        self._bridge.start_operation(workspace, password=password)

    def _is_encrypted(self) -> bool:
        report = getattr(self, "_report", None)
        return bool(report and report.encrypted)

    def ensure_password(self) -> str | None:
        """Ask for the archive password if the report says encrypted.

        Returns the password, None when not needed, and None + cancelled
        flag via dialog response when the user backs out.
        """
        if not self._is_encrypted():
            return None
        dialog = Adw.MessageDialog(
            transient_for=self,
            heading="Archivo cifrado",
            body="Este archivo requiere contraseña (AES). No se guarda en ningún sitio.",
        )
        entry = Gtk.PasswordEntry(show_peek_icon=True)
        entry.set_placeholder_text("Contraseña del archivo")
        dialog.set_extra_child(entry)
        dialog.add_response("cancel", "Cancelar")
        dialog.add_response("ok", "Continuar")
        dialog.set_response_appearance("ok", Adw.ResponseAppearance.SUGGESTED)
        dialog.set_default_response("ok")
        dialog.set_close_response("cancel")
        chosen: list = []
        dialog.connect("response", lambda d, r: chosen.append(r))
        dialog.present()
        # Poke the main loop until the user answers.
        while not chosen:
            import time

            time.sleep(0.05)
            while Gtk.events_pending():
                Gtk.main_iteration()
        text = str(entry.get_text())
        if chosen[0] != "ok" or not text:
            return None
        return text

    def on_enqueue_requested(self, view) -> None:
        import threading

        archive = self._bridge._archive_path
        workspace = self._analysis.get_selected_workspace()
        if not archive:
            self.show_toast("No hay archivo para encolar", priority=Adw.ToastPriority.HIGH)
            return
        password = self.ensure_password()
        if password is None and self._is_encrypted():
            return
        threading.Thread(
            target=self._do_enqueue, args=(archive, workspace, password), daemon=True
        ).start()

    def _do_enqueue(self, archive: str, workspace: str | None, password: str | None = None) -> None:
        from gi.repository import GLib

        try:
            op_id = self._bridge.queue_add(archive, "extract", "normal", workspace, password)
            GLib.idle_add(
                self.show_toast, f"Encolado con ID {op_id}",
            )
            GLib.idle_add(self.refresh_queue)
        except Exception as exc:  # noqa: BLE001
            GLib.idle_add(self.show_toast, f"No se pudo encolar: {exc}", Adw.ToastPriority.HIGH)

    def refresh_queue(self) -> None:
        import threading

        threading.Thread(target=self._do_refresh_queue, daemon=True).start()

    def refresh_integrity_label(self) -> None:
        import threading

        threading.Thread(target=self._do_refresh_integrity, daemon=True).start()

    def _do_refresh_integrity(self) -> None:
        from gi.repository import GLib

        try:
            archive = self._bridge._archive_path
            if not archive:
                return
            record = self._bridge.integrity_last(archive)
            if record:
                text = f"Última verificación: BLAKE3 {record['blake3'][:16]}… ({record['size_bytes']} B)"
            else:
                text = "Sin historial de integridad local"
            GLib.idle_add(self._analysis.set_integrity_text, text)
        except Exception:  # noqa: BLE001 - best-effort background refresh
            return

    def _do_refresh_queue(self) -> None:
        from gi.repository import GLib

        from .core.protocol import queue_items_from_raw
        from .widgets.queue_panel import Priority, QueueItem, QueueItemStatus

        try:
            raw = self._bridge.queue_list()
            items = []
            for entry in queue_items_from_raw(raw):
                try:
                    prio = Priority[entry["priority"].upper()]
                except KeyError:
                    prio = Priority.NORMAL
                try:
                    st = QueueItemStatus(entry["status"])
                except ValueError:
                    st = QueueItemStatus.PENDING
                items.append(QueueItem(
                    id=entry["id"], uuid="", operation=entry["operation"],
                    archive=entry["archive"], output=None, priority=prio,
                    status=st, progress=entry["progress"],
                    current_file=entry["current_file"],
                    error_message=entry["error_message"],
                ))
            GLib.idle_add(self._analysis.set_queue_items, items)
        except Exception:  # noqa: BLE001 - best-effort background refresh
            return

    def on_progress_tick(self, bridge, tick) -> None:
        self._progress.update_tick(tick)

    def on_done(self, _view) -> None:
        self.go_welcome()
        self._bridge.reset()

    def on_cancel(self, _view) -> None:
        self._bridge.cancel()
        self.go_welcome()

    def on_operation_failed(self, bridge, message: str) -> None:
        self.show_toast(message, priority=Adw.ToastPriority.HIGH)
        self.go_welcome()

    # --------------------------------------------------------------- Helpers
    def show_toast(self, text: str, priority: Adw.ToastPriority = Adw.ToastPriority.NORMAL) -> None:
        toast = Adw.Toast(title=text, priority=priority, timeout=5)
        self._toast_overlay.add_toast(toast)