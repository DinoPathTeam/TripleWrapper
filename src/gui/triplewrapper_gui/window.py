"""
Main application window for TripleWrapper GUI
"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
gi.require_version('Gdk', '4.0')
from gi.repository import Gtk, Adw, Gdk, Gio, GLib
import os
from pathlib import Path
from typing import Optional

from .widgets import SteamGraphWidget, DonutChartWidget, DonutLegendWidget, DiskPanelWidget, QueuePanelWidget
from .core.client import get_core_client, DiskInfo, StorageVerdict
from .utils.formatting import format_bytes, format_duration, format_speed
from .utils.settings import Settings


class TripleWrapperWindow(Adw.ApplicationWindow):
    """Main application window"""
    
    def __init__(self, app: Adw.Application):
        super().__init__(application=app, title="TripleWrapper")
        self.set_default_size(1000, 700)
        
        self._app = app
        self._settings = Settings()
        self._core_client = None
        self._current_archive: Optional[str] = None
        self._current_verdict: Optional[StorageVerdict] = None
        self._operation_id = 0
        
        # Local queue for GUI (mirrors core queue)
        self._local_queue = None
        
        self._build_ui()
        self._connect_core()
    
    def _build_ui(self):
        # Root navigation view
        self._nav_view = Adw.NavigationView()
        self.set_content(self._nav_view)
        
        # Main page
        main_page = Adw.NavigationPage()
        main_page.set_title("TripleWrapper")
        self._nav_view.push_by_tag("main", main_page)
        
        # Main layout
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        main_page.set_child(main_box)
        
        # Header bar
        header = Adw.HeaderBar()
        header.set_show_title_buttons(True)
        self._setup_header_actions(header)
        main_box.append(header)
        
        # Content area with toolbar view
        toolbar_view = Adw.ToolbarView()
        main_box.append(toolbar_view)
        
        content_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=0)
        content_box.set_vexpand(True)
        toolbar_view.add_top_bar(Adw.ToolbarView.create_toolbar(None))  # Empty top toolbar
        toolbar_view.set_content(content_box)
        
        # Top panel (GNOME Disks style)
        self._build_top_panel(content_box)
        
        # Separator
        content_box.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        
        # Queue panel (v0.2 - Batch operations)
        self._build_queue_panel(content_box)
        
        # Separator
        content_box.append(Gtk.Separator(orientation=Gtk.Orientation.HORIZONTAL))
        
        # Bottom panel (Steam graph style)
        self._build_bottom_panel(content_box)
        
        # Status bar
        self._build_status_bar(content_box)
    
    def _setup_header_actions(self, header: Adw.HeaderBar):
        # Menu button
        menu_btn = Gtk.MenuButton()
        menu_btn.set_icon_name("open-menu-symbolic")
        
        menu = Gio.Menu()
        menu.append("Preferencias", "app.preferences")
        menu.append("Acerca de", "app.about")
        menu.append("Salir", "app.quit")
        
        menu_btn.set_menu_model(menu)
        header.pack_end(menu_btn)
    
    def _build_top_panel(self, parent: Gtk.Box):
        """Build GNOME Disks style top panel"""
        # Paned for sidebar + content
        paned = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL)
        paned.set_wide_handle(True)
        paned.set_shrink_start_child(False)
        paned.set_resize_start_child(False)
        parent.append(paned)
        
        # Sidebar (disk list)
        self._disk_panel = DiskPanelWidget()
        self._disk_panel.set_size_request(320, -1)
        self._disk_panel.connect_selection_changed(self._on_disk_selected)
        paned.set_start_child(self._disk_panel)
        
        # Main content
        content_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16)
        content_box.set_margin_top(24)
        content_box.set_margin_bottom(24)
        content_box.set_margin_start(24)
        content_box.set_margin_end(24)
        content_box.set_vexpand(True)
        paned.set_end_child(content_box)
        
        # Archive info
        self._archive_info_box = self._create_archive_info_box()
        content_box.append(self._archive_info_box)
        
        # Donut chart + legend
        chart_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=24)
        
        self._donut_chart = DonutChartWidget(size=160)
        chart_box.append(self._donut_chart)
        
        self._donut_legend = DonutLegendWidget(self._donut_chart)
        self._donut_legend.set_valign(Gtk.Align.CENTER)
        chart_box.append(self._donut_legend)
        
        content_box.append(chart_box)
        
        # Action buttons
        self._action_buttons_box = self._create_action_buttons()
        content_box.append(self._action_buttons_box)
        
        # Verdict display
        self._verdict_box = self._create_verdict_box()
        content_box.append(self._verdict_box)
    
    def _create_archive_info_box(self) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        
        self._archive_name_label = Gtk.Label(label="Ningún archivo seleccionado")
        self._archive_name_label.add_css_class("title-2")
        self._archive_name_label.set_halign(Gtk.Align.START)
        self._archive_name_label.set_ellipsize(3)  # PANGO_ELLIPSIZE_END
        box.append(self._archive_name_label)
        
        self._archive_size_label = Gtk.Label(label="")
        self._archive_size_label.set_halign(Gtk.Align.START)
        self._archive_size_label.add_css_class("dim-label")
        box.append(self._archive_size_label)
        
        return box
    
    def _create_action_buttons(self) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        box.set_halign(Gtk.Align.CENTER)
        box.set_margin_top(8)
        box.set_margin_bottom(8)
        
        buttons = [
            ("Abrir archivo", "document-open-symbolic", self._on_open_file, True),
            ("Analizar", "system-search-symbolic", self._on_analyze, True),
            ("Encolar", "list-add-symbolic", self._on_enqueue, True),
            ("Iniciar", "media-playback-start-symbolic", self._on_start, True),
            ("Cancelar", "process-stop-symbolic", self._on_cancel, False),
        ]
        
        self._action_buttons = {}
        for label, icon, callback, enabled in buttons:
            btn = Gtk.Button()
            btn.set_child(Gtk.Image.new_from_icon_name(icon))
            btn.set_label(label)
            btn.set_tooltip_text(label)
            btn.connect("clicked", callback)
            btn.set_sensitive(enabled)
            self._action_buttons[label] = btn
            box.append(btn)
        
        # Make "Iniciar" suggested action
        self._action_buttons["Iniciar"].add_css_class("suggested-action")
        
        return box
    
    def _create_verdict_box(self) -> Gtk.Box:
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        box.set_visible(False)
        box.set_margin_top(12)
        
        self._verdict_title = Gtk.Label()
        self._verdict_title.add_css_class("title-3")
        self._verdict_title.set_halign(Gtk.Align.START)
        box.append(self._verdict_title)
        
        self._verdict_message = Gtk.Label()
        self._verdict_message.set_halign(Gtk.Align.START)
        self._verdict_message.set_wrap(True)
        self._verdict_message.set_selectable(True)
        box.append(self._verdict_message)
        
        # Workspace info
        self._workspace_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        self._workspace_box.set_visible(False)
        box.append(self._workspace_box)
        
        self._workspace_label = Gtk.Label()
        self._workspace_label.set_halign(Gtk.Align.START)
        self._workspace_box.append(self._workspace_label)
        
        self._confirm_btn = Gtk.Button(label="Confirmar y continuar")
        self._confirm_btn.add_css_class("suggested-action")
        self._confirm_btn.set_visible(False)
        self._confirm_btn.connect("clicked", self._on_confirm_workspace)
        self._workspace_box.append(self._confirm_btn)
        
        return box
    
    def _build_bottom_panel(self, parent: Gtk.Box):
        """Build Steam-style graph panel"""
        # Frame for graph
        frame = Gtk.Frame()
        frame.add_css_class("graph-widget")
        
        self._graph = SteamGraphWidget()
        frame.set_child(self._graph)
        
        parent.append(frame)
    
    def _build_queue_panel(self, parent: Gtk.Box):
        """Build queue panel for batch operations"""
        # Frame for queue
        frame = Gtk.Frame()
        frame.add_css_class("queue-panel")
        
        self._queue_panel = QueuePanelWidget()
        self._queue_panel.connect("item-action", self._on_queue_action)
        frame.set_child(self._queue_panel)
        
        parent.append(frame)
    
    def _build_status_bar(self, parent: Gtk.Box):
        """Build status bar at bottom"""
        status_bar = Adw.StatusPage()
        status_bar.set_title("Listo")
        status_bar.set_description("Seleccione un archivo para comenzar")
        status_bar.set_icon_name("drive-harddisk-symbolic")
        parent.append(status_bar)
        self._status_page = status_bar
    
    def _connect_core(self):
        """Connect to core service asynchronously"""
        async def connect():
            try:
                self._core_client = await get_core_client()
                await self._refresh_disks()
                self._update_status("Conectado", "Listo para operar")
            except Exception as e:
                self._update_status("Desconectado", f"Error: {e}")
                # Fall back to mock
                self._core_client = await get_core_client(use_mock=True)
                await self._refresh_disks()
            
            # Initialize local queue after core connection
            await self._init_local_queue()
        
        import asyncio
        asyncio.create_task(connect())
    
    async def _init_local_queue(self):
        """Initialize local queue mirroring core queue"""
        from triplewrapper_core.queue import OperationQueue, QueueConfig
        from pathlib import Path
        import os
        
        # Get XDG data directory
        data_dir = os.environ.get('XDG_DATA_HOME', os.path.expanduser('~/.local/share'))
        persistence_path = Path(data_dir) / "triplewrapper" / "queue.json"
        
        config = QueueConfig(
            persistence_path=persistence_path,
        )
        
        self._local_queue = OperationQueue(config)
        await self._local_queue.init()
        
        # Sync queue panel with local queue
        self._sync_queue_panel()
    
    async def _refresh_disks(self):
        """Refresh disk list from core"""
        if not self._core_client:
            return
        
        try:
            disks = await self._core_client.get_disks()
            self._disk_panel.set_disks(disks)
        except Exception as e:
            print(f"Error refreshing disks: {e}")
    
    def _update_status(self, title: str, description: str):
        """Update status page"""
        self._status_page.set_title(title)
        self._status_page.set_description(description)
    
    # Event handlers
    
    def _on_open_file(self, button: Gtk.Button):
        """Open file dialog"""
        dialog = Gtk.FileDialog()
        dialog.set_title("Seleccionar archivo comprimido")
        
        # Add filters
        filters = Gio.ListStore.new(Gtk.FileFilter)
        for name, patterns in [
            ("Todos los archivos", ["*"]),
            ("7-Zip", ["*.7z"]),
            ("ZIP", ["*.zip"]),
            ("TAR", ["*.tar", "*.tar.gz", "*.tar.xz", "*.tar.zst", "*.tar.bz2"]),
            ("Pixz", ["*.tar.pixz"]),
        ]:
            filter = Gtk.FileFilter()
            filter.set_name(name)
            for pattern in patterns:
                filter.add_pattern(pattern)
            filters.append(filter)
        
        dialog.set_filters(filters)
        
        dialog.open(self, None, self._on_file_selected)
    
    def _on_file_selected(self, dialog: Gtk.FileDialog, result: Gio.AsyncResult):
        try:
            file = dialog.open_finish(result)
            if file:
                path = file.get_path()
                if path:
                    self.open_archive(path)
        except Exception as e:
            print(f"Error opening file: {e}")
    
    def open_archive(self, path: str):
        """Open and analyze an archive"""
        self._current_archive = path
        
        # Update UI
        name = os.path.basename(path)
        size = os.path.getsize(path) if os.path.exists(path) else 0
        
        self._archive_name_label.set_text(name)
        self._archive_size_label.set_text(f"Tamaño: {format_bytes(size)}")
        
        # Enable buttons
        self._action_buttons["Analizar"].set_sensitive(True)
        self._action_buttons["Encolar"].set_sensitive(True)
        self._action_buttons["Iniciar"].set_sensitive(False)
        
        # Reset verdict
        self._verdict_box.set_visible(False)
        self._current_verdict = None
        
        # Update disk panel source
        self._disk_panel.set_source_disk_from_path(path)
        
        # Update status
        self._update_status("Archivo cargado", f"{name} ({format_bytes(size)})")
    
    def _on_analyze(self, button: Gtk.Button):
        """Analyze storage requirements"""
        if not self._current_archive or not self._core_client:
            return
        
        button.set_sensitive(False)
        button.set_label("Analizando...")
        
        async def analyze():
            try:
                # For now, use defaults (0 bytes to remove/add)
                # In real implementation, would scan archive for selected files
                verdict = await self._core_client.get_verdict(
                    self._current_archive, 0, 0
                )
                
                GLib.idle_add(self._show_verdict, verdict)
            except Exception as e:
                GLib.idle_add(self._show_error, f"Error analizando: {e}")
            finally:
                GLib.idle_add(lambda: (
                    button.set_sensitive(True),
                    button.set_label("Analizar")
                ))
        
        import asyncio
        asyncio.create_task(analyze())
    
    def _show_verdict(self, verdict: StorageVerdict):
        """Display storage verdict"""
        self._current_verdict = verdict
        self._verdict_box.set_visible(True)
        
        # Title based on verdict type
        if verdict.verdict == 'internal_ok':
            self._verdict_title.set_text("✓ Espacio suficiente en disco origen")
            self._verdict_title.remove_css_class("verdict-external")
            self._verdict_title.remove_css_class("verdict-critical")
            self._verdict_title.add_css_class("verdict-internal")
        elif verdict.verdict == 'external_required':
            self._verdict_title.set_text("⚠ Se requiere unidad externa para caché")
            self._verdict_title.remove_css_class("verdict-internal")
            self._verdict_title.remove_css_class("verdict-critical")
            self._verdict_title.add_css_class("verdict-external")
        else:
            self._verdict_title.set_text("✗ Error crítico: espacio insuficiente")
            self._verdict_title.remove_css_class("verdict-internal")
            self._verdict_title.remove_css_class("verdict-external")
            self._verdict_title.add_css_class("verdict-critical")
        
        self._verdict_message.set_text(verdict.message)
        
        # Update donut chart
        if verdict.estimate:
            est = verdict.estimate
            # Calculate keep/remove/add from estimate
            keep = est.get('current_size', 0) - est.get('bytes_to_remove', 0)
            remove = est.get('bytes_to_remove', 0)
            add = est.get('bytes_to_add', 0)
            self._donut_chart.set_data(keep, remove, add)
            self._donut_legend.update(self._donut_chart._segments)
        
        # Workspace info
        if verdict.verdict == 'external_required' and verdict.workspace_disk:
            self._workspace_box.set_visible(True)
            ws = verdict.workspace_disk
            self._workspace_label.set_text(
                f"Caché en: {ws.label} ({ws.mount_point}) — "
                f"{format_bytes(ws.free_bytes)} libres — "
                f"Parámetro 7z: {verdict.sevenzip_workdir_param}"
            )
            if verdict.requires_confirmation:
                self._confirm_btn.set_visible(True)
        else:
            self._workspace_box.set_visible(False)
        
        # Enable start button if not critical error
        self._action_buttons["Iniciar"].set_sensitive(verdict.verdict != 'critical_error')
        
        # Update disk panel required space
        if verdict.estimate:
            self._disk_panel.set_required_space(verdict.estimate.get('space_needed_for_rewrite', 0))
    
    def _on_enqueue(self, button: Gtk.Button):
        """Add current operation to queue"""
        if not self._current_archive or not self._current_verdict or not self._core_client:
            return
        
        # If external required and needs confirmation, wait for confirm
        if (self._current_verdict.verdict == 'external_required' and 
            self._current_verdict.requires_confirmation):
            return  # Wait for confirm button
        
        self._add_to_queue()
    
    def _add_to_queue(self):
        """Add current operation to queue"""
        if not self._current_archive or not self._current_verdict or not self._core_client:
            return
        
        # Determine workdir
        workdir = None
        if (self._current_verdict.verdict == 'external_required' and 
            self._current_verdict.workspace_disk):
            workdir = self._current_verdict.workspace_disk.mount_point + "/.triplewrapper_cache"
        
        # Create operation request
        request = OperationRequest(
            id=OperationId(0),
            op_type=OperationType.MODIFY,
            archive_path=Path(self._current_archive),
            output_path=Path(workdir) if workdir else None,
            files_to_process=[],
            compression_level=self._settings.default_compression_level,
            compression_format=None,
            workspace_override=Path(workdir) if workdir else None,
            verify_after=self._settings.verify_checksums,
            dry_run=False,
        )
        
        # Add to local queue
        if self._local_queue:
            async def enqueue_local():
                try:
                    op_id = await self._local_queue.enqueue(request, Some(Priority.NORMAL))
                    # Update queue panel with new item
                    GLib.idle_add(self._sync_queue_panel)
                except Exception as e:
                    GLib.idle_add(self._show_error, f"Error encolando: {e}")
            
            import asyncio
            asyncio.create_task(enqueue_local())
        else:
            # Fallback to direct start if no local queue
            asyncio.create_task(self._start_operation_direct())
    
    def _on_start(self, button: Gtk.Button):
        """Start operation - add to queue"""
        if not self._current_archive or not self._current_verdict or not self._core_client:
            return
        
        # If external required and needs confirmation, wait for confirm
        if (self._current_verdict.verdict == 'external_required' and 
            self._current_verdict.requires_confirmation):
            return  # Wait for confirm button
        
        self._add_to_queue()
    
    def _on_confirm_workspace(self, button: Gtk.Button):
        """Confirm external workspace"""
        self._confirm_btn.set_visible(False)
        self._add_to_queue()
    
    def _start_operation(self):
        """Start the actual operation"""
        if not self._current_archive or not self._core_client:
            return
        
        self._action_buttons["Iniciar"].set_sensitive(False)
        self._action_buttons["Cancelar"].set_sensitive(True)
        self._graph.set_running(True)
        
        # Determine workdir
        workdir = None
        if (self._current_verdict.verdict == 'external_required' and 
            self._current_verdict.workspace_disk):
            workdir = self._current_verdict.workspace_disk.mount_point + "/.triplewrapper_cache"
        
        async def run_op():
            try:
                result = await self._core_client.start_operation(
                    archive_path=self._current_archive,
                    op_type='modify',  # Default to modify
                    workdir=workdir,
                    compression_level=self._settings.default_compression_level,
                )
                
                self._operation_id = result.get('id', 0)
                
                # Subscribe to progress
                if self._operation_id:
                    progress_rx = await self._core_client.subscribe_progress(self._operation_id)
                    if progress_rx:
                        self._monitor_progress(progress_rx)
                        
            except Exception as e:
                GLib.idle_add(self._operation_failed, str(e))
        
        import asyncio
        asyncio.create_task(run_op())
    
    def _monitor_progress(self, progress_rx):
        """Monitor progress updates"""
        async def monitor():
            while True:
                try:
                    progress = await progress_rx.recv()
                    GLib.idle_add(self._update_graph_progress, progress)
                    
                    if progress.get('status') in ('completed', 'failed', 'cancelled'):
                        break
                except Exception:
                    break
        
        import asyncio
        asyncio.create_task(monitor())
    
    def _update_graph_progress(self, progress: dict):
        """Update graph with progress data"""
        self._graph.add_sample(
            progress.get('bytes_per_second_read', 0),
            progress.get('bytes_per_second_write', 0),
            progress.get('bytes_per_second_compress', 0)
        )
        
        self._graph.set_current_file(progress.get('current_file', 'Procesando...'))
        self._graph.set_progress(
            progress.get('bytes_processed', 0),
            progress.get('bytes_total', 0),
            progress.get('eta_seconds')
        )
        
        # Check completion
        status = progress.get('status')
        if status == 'completed':
            self._operation_completed(True, "Operación completada")
        elif status == 'failed':
            self._operation_completed(False, progress.get('message', 'Error desconocido'))
        elif status == 'cancelled':
            self._operation_completed(False, "Operación cancelada")
    
    def _operation_completed(self, success: bool, message: str):
        self._graph.set_running(False)
        self._action_buttons["Iniciar"].set_sensitive(True)
        self._action_buttons["Cancelar"].set_sensitive(False)
        
        if success:
            self._update_status("Completado", message)
        else:
            self._update_status("Error", message)
    
    def _operation_failed(self, error: str):
        self._graph.set_running(False)
        self._action_buttons["Iniciar"].set_sensitive(True)
        self._action_buttons["Cancelar"].set_sensitive(False)
        self._update_status("Error", error)
    
    def _on_cancel(self, button: Gtk.Button):
        """Cancel current operation"""
        if self._operation_id and self._core_client:
            async def cancel():
                await self._core_client.cancel_operation(self._operation_id)
            
            import asyncio
            asyncio.create_task(cancel())
    
    def _on_disk_selected(self, disk: DiskInfo):
        """Handle disk selection from panel"""
        print(f"Disk selected: {disk.label} ({disk.mount_point})")
    
    def _on_queue_action(self, widget, action: str, item_id: str):
        """Handle queue panel actions"""
        if action == "pause_queue":
            pass  # Queue pause handled by panel
        elif action == "resume_queue":
            pass
        elif action in ("pause_item", "resume_item", "cancel_item", "retry_item") and item_id != "queue":
            pass  # Item actions handled by panel
        elif action == "add":
            pass  # Add new operation
    
    def _sync_queue_panel(self):
        """Sync queue panel with local queue"""
        if not self._local_queue:
            return
        
        async def sync():
            items = await self._local_queue.get_all()
            GLib.idle_add(self._queue_panel.set_items, items)
        
        import asyncio
        asyncio.create_task(sync())
    
    def _on_disk_selected(self, disk: DiskInfo):
        """Handle disk selection from panel"""
        print(f"Disk selected: {disk.label} ({disk.mount_point})")
    
    def _show_error(self, message: str):
        """Show error dialog"""
        dialog = Adw.MessageDialog(
            transient_for=self,
            heading="Error",
            body=message,
        )
        dialog.add_response("ok", "OK")
        dialog.set_default_response("ok")
        dialog.set_close_response("ok")
        dialog.present(self)