"""
Disk panel widget for storage device selection and display
"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
from gi.repository import Gtk, Adw, Gdk, GLib, Gio
from typing import Optional, List, Callable
from dataclasses import dataclass


@dataclass
class DiskInfo:
    mount_point: str
    label: str
    filesystem: str
    total_bytes: int
    free_bytes: int
    used_bytes: int
    is_removable: bool
    is_system: bool
    device_path: Optional[str] = None
    
    @property
    def free_gb(self) -> float:
        return self.free_bytes / (1024 ** 3)
    
    @property
    def total_gb(self) -> float:
        return self.total_bytes / (1024 ** 3)
    
    @property
    def usage_percent(self) -> float:
        if self.total_bytes == 0:
            return 0.0
        return (self.used_bytes / self.total_bytes) * 100


class DiskPanelWidget(Adw.Bin):
    """
    Panel showing available disks with space info.
    Allows selection of cache workspace.
    """
    
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        
        self._disks: List[DiskInfo] = []
        self._selected_disk: Optional[DiskInfo] = None
        self._on_selection_changed: Optional[Callable[[DiskInfo], None]] = None
        
        self._build_ui()
    
    def _build_ui(self):
        # Main container
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=12)
        main_box.set_margin_top(12)
        main_box.set_margin_bottom(12)
        main_box.set_margin_start(12)
        main_box.set_margin_end(12)
        self.set_child(main_box)
        
        # Header
        header_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        
        title = Gtk.Label(label="Unidades de almacenamiento")
        title.add_css_class("title-3")
        title.set_halign(Gtk.Align.START)
        header_box.append(title)
        
        # Refresh button
        refresh_btn = Gtk.Button(icon_name="view-refresh-symbolic")
        refresh_btn.set_tooltip_text("Actualizar")
        refresh_btn.connect("clicked", self._on_refresh)
        header_box.append(refresh_btn)
        
        main_box.append(header_box)
        
        # Disk list
        self._list_box = Gtk.ListBox()
        self._list_box.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self._list_box.connect("row-selected", self._on_row_selected)
        main_box.append(self._list_box)
        
        # Selected disk details
        self._details_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        self._details_box.set_visible(False)
        self._details_box.set_margin_top(12)
        main_box.append(self._details_box)
        
        self._detail_label = Gtk.Label()
        self._detail_label.set_halign(Gtk.Align.START)
        self._detail_label.set_wrap(True)
        self._details_box.append(self._detail_label)
        
        # Use as cache button
        self._use_cache_btn = Gtk.Button(label="Usar como caché")
        self._use_cache_btn.add_css_class("suggested-action")
        self._use_cache_btn.set_visible(False)
        self._use_cache_btn.connect("clicked", self._on_use_cache)
        self._details_box.append(self._use_cache_btn)
    
    def set_disks(self, disks: List[DiskInfo]):
        """Update disk list"""
        self._disks = disks
        self._refresh_list()
    
    def set_source_disk(self, disk: DiskInfo):
        """Mark source disk (where archive is located)"""
        self._source_disk = disk
        self._refresh_list()
    
    def set_required_space(self, bytes_needed: int):
        """Set required space for operation"""
        self._required_space = bytes_needed
        self._refresh_list()
    
    def _refresh_list(self):
        """Rebuild list rows"""
        # Clear existing
        child = self._list_box.get_first_child()
        while child:
            self._list_box.remove(child)
            child = self._list_box.get_first_child()
        
        # Add rows
        for disk in self._disks:
            row = self._create_disk_row(disk)
            self._list_box.append(row)
        
        # Select source disk if set
        if hasattr(self, '_source_disk') and self._source_disk:
            self._select_disk(self._source_disk)
    
    def _create_disk_row(self, disk: DiskInfo) -> Adw.ActionRow:
        """Create a list row for a disk"""
        row = Adw.ActionRow()
        row.set_activatable(True)
        
        # Title: label + mount point
        title = f"{disk.label} ({disk.mount_point})"
        row.set_title(title)
        
        # Subtitle: free space / total
        free_str = self._format_bytes(disk.free_bytes)
        total_str = self._format_bytes(disk.total_bytes)
        subtitle = f"Libre: {free_str} / {total_str} ({disk.usage_percent:.0f}% usado)"
        row.set_subtitle(subtitle)
        
        # Icon based on type
        if disk.is_system:
            icon_name = "drive-harddisk-system-symbolic"
        elif disk.is_removable:
            icon_name = "drive-removable-media-symbolic"
        else:
            icon_name = "drive-harddisk-symbolic"
        
        row.set_icon_name(icon_name)
        
        # Add tags
        if disk.is_system:
            row.add_suffix(self._create_tag("Sistema", "system"))
        
        # Check if has enough space
        if hasattr(self, '_required_space') and disk.free_bytes >= self._required_space:
            row.add_suffix(self._create_tag("✓ Espacio OK", "success"))
        elif hasattr(self, '_required_space'):
            row.add_suffix(self._create_tag("⚠ Insuficiente", "warning"))
        
        # Store disk reference
        row.disk = disk
        
        return row
    
    def _create_tag(self, text: str, variant: str) -> Gtk.Label:
        """Create a styled tag label"""
        label = Gtk.Label(label=text)
        label.add_css_class("dim-label")
        label.set_margin_end(6)
        label.set_margin_start(6)
        label.set_margin_top(2)
        label.set_margin_bottom(2)
        
        if variant == "success":
            label.add_css_class("success")
        elif variant == "warning":
            label.add_css_class("warning")
        elif variant == "system":
            label.add_css_class("accent")
        
        return label
    
    def _on_row_selected(self, list_box: Gtk.ListBox, row: Optional[Adw.ActionRow]):
        if row and hasattr(row, 'disk'):
            self._selected_disk = row.disk
            self._show_details(row.disk)
            if self._on_selection_changed:
                self._on_selection_changed(row.disk)
    
    def _select_disk(self, disk: DiskInfo):
        """Programmatically select a disk"""
        child = self._list_box.get_first_child()
        while child:
            if hasattr(child, 'disk') and child.disk.mount_point == disk.mount_point:
                self._list_box.select_row(child)
                break
            child = child.get_next_sibling()
    
    def _show_details(self, disk: DiskInfo):
        """Show disk details"""
        self._details_box.set_visible(True)
        
        details = [
            f"Dispositivo: {disk.device_path or 'Desconocido'}",
            f"Sistema de archivos: {disk.filesystem}",
            f"Punto de montaje: {disk.mount_point}",
            f"Total: {self._format_bytes(disk.total_bytes)}",
            f"Libre: {self._format_bytes(disk.free_bytes)} ({disk.usage_percent:.1f}% usado)",
            f"Extraíble: {'Sí' if disk.is_removable else 'No'}",
            f"Sistema: {'Sí' if disk.is_system else 'No'}",
        ]
        
        self._detail_label.set_text("\n".join(details))
        
        # Show use-as-cache button for non-system disks with space
        if not disk.is_system and hasattr(self, '_required_space'):
            if disk.free_bytes >= self._required_space:
                self._use_cache_btn.set_visible(True)
            else:
                self._use_cache_btn.set_visible(False)
        else:
            self._use_cache_btn.set_visible(False)
    
    def _on_refresh(self, button: Gtk.Button):
        """Refresh button clicked - emit signal"""
        # In real implementation, would trigger async disk scan
        pass
    
    def _on_use_cache(self, button: Gtk.Button):
        """Use selected disk as cache workspace"""
        if self._selected_disk and self._on_selection_changed:
            self._on_selection_changed(self._selected_disk)
    
    def connect_selection_changed(self, callback: Callable[[DiskInfo], None]):
        """Connect callback for disk selection"""
        self._on_selection_changed = callback
    
    def _format_bytes(self, bytes_val: int) -> str:
        units = ['B', 'KB', 'MB', 'GB', 'TB']
        size = float(bytes_val)
        unit = 0
        while size >= 1024 and unit < len(units) - 1:
            size /= 1024
            unit += 1
        return f"{size:.1f} {units[unit]}"