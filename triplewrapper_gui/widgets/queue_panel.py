"""
Queue panel widget for batch operations visualization
"""

import gi
gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
from gi.repository import Gtk, Adw, GLib, GObject
from typing import Optional, List
from dataclasses import dataclass
from enum import Enum


class QueueItemStatus(Enum):
    PENDING = "pending"
    RUNNING = "running"
    PAUSED = "paused"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELLED = "cancelled"


class Priority(Enum):
    LOW = 0
    NORMAL = 1
    HIGH = 2
    CRITICAL = 3


@dataclass
class QueueItem:
    id: int
    uuid: str
    operation: str
    archive: str
    output: Optional[str]
    priority: Priority
    status: QueueItemStatus
    progress: float = 0.0
    current_file: str = ""
    error_message: Optional[str] = None
    retry_count: int = 0
    max_retries: int = 3


class QueuePanelWidget(Adw.Bin):
    """
    Panel showing queued operations with management controls.
    """
    
    # Signal emitted when user requests an action
    __gsignals__ = {
        'item-action': (GObject.SignalFlags.RUN_FIRST, None,
                        (str, str)),  # action, item_id
    }
    
    def __init__(self, **kwargs):
        super().__init__(**kwargs)
        
        self._items: List[QueueItem] = []
        self._selected_id: Optional[int] = None
        
        self._build_ui()
    
    def _build_ui(self):
        main_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6)
        main_box.set_margin_top(6)
        main_box.set_margin_bottom(6)
        main_box.set_margin_start(6)
        main_box.set_margin_end(6)
        self.set_child(main_box)
        
        # Header
        header_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        
        title = Gtk.Label(label="Cola de operaciones")
        title.add_css_class("title-3")
        title.set_halign(Gtk.Align.START)
        header_box.append(title)
        
        # Queue controls
        controls_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        controls_box.set_halign(Gtk.Align.END)
        
        self._pause_queue_btn = Gtk.Button(icon_name="media-playback-pause-symbolic")
        self._pause_queue_btn.set_tooltip_text("Pausar cola")
        self._pause_queue_btn.connect("clicked", self._on_pause_queue)
        controls_box.append(self._pause_queue_btn)
        
        self._resume_queue_btn = Gtk.Button(icon_name="media-playback-start-symbolic")
        self._resume_queue_btn.set_tooltip_text("Reanudar cola")
        self._resume_queue_btn.connect("clicked", self._on_resume_queue)
        self._resume_queue_btn.set_sensitive(False)
        controls_box.append(self._resume_queue_btn)
        
        add_btn = Gtk.Button(icon_name="list-add-symbolic")
        add_btn.set_tooltip_text("Añadir operación")
        add_btn.connect("clicked", lambda b: self.emit("item-action", "add", "new"))
        controls_box.append(add_btn)
        
        header_box.append(controls_box)
        main_box.append(header_box)
        
        # Status bar
        self._status_bar = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        self._status_bar.add_css_class("linked")
        
        self._pending_label = Gtk.Label(label="Pendientes: 0")
        self._pending_label.add_css_class("caption")
        self._status_bar.append(self._pending_label)
        
        self._running_label = Gtk.Label(label="En ejecución: 0")
        self._running_label.add_css_class("caption")
        self._status_bar.append(self._running_label)
        
        self._paused_label = Gtk.Label(label="Pausados: 0")
        self._paused_label.add_css_class("caption")
        self._status_bar.append(self._paused_label)
        
        main_box.append(self._status_bar)
        
        # Item list
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.AUTOMATIC, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)
        scrolled.set_min_content_height(300)
        
        self._list_box = Gtk.ListBox()
        self._list_box.set_selection_mode(Gtk.SelectionMode.SINGLE)
        self._list_box.connect("row-selected", self._on_row_selected)
        scrolled.set_child(self._list_box)
        
        main_box.append(scrolled)
        
        # Item actions bar (shown when item selected)
        self._item_actions_box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=6)
        self._item_actions_box.set_visible(False)
        self._item_actions_box.set_margin_top(6)
        main_box.append(self._item_actions_box)
        
        self._pause_item_btn = Gtk.Button(label="Pausar")
        self._pause_item_btn.add_css_class("pill")
        self._pause_item_btn.connect("clicked", self._on_pause_item)
        self._item_actions_box.append(self._pause_item_btn)
        
        self._resume_item_btn = Gtk.Button(label="Reanudar")
        self._resume_item_btn.add_css_class("pill")
        self._resume_item_btn.add_css_class("suggested-action")
        self._resume_item_btn.connect("clicked", self._on_resume_item)
        self._item_actions_box.append(self._resume_item_btn)
        
        self._retry_btn = Gtk.Button(label="Reintentar")
        self._retry_btn.add_css_class("pill")
        self._retry_btn.connect("clicked", self._on_retry_item)
        self._item_actions_box.append(self._retry_btn)
        
        self._cancel_btn = Gtk.Button(label="Cancelar")
        self._cancel_btn.add_css_class("pill")
        self._cancel_btn.add_css_class("destructive-action")
        self._cancel_btn.connect("clicked", self._on_cancel_item)
        self._item_actions_box.append(self._cancel_btn)
    
    def set_items(self, items: List[QueueItem]):
        """Update the queue items"""
        self._items = items
        self._refresh_list()
        self._update_status_bar()
    
    def add_item(self, item: QueueItem):
        """Add a single item"""
        self._items.append(item)
        self._refresh_list()
        self._update_status_bar()
    
    def update_item(self, item_id: int, **kwargs):
        """Update item properties"""
        for item in self._items:
            if item.id == item_id:
                for key, value in kwargs.items():
                    if hasattr(item, key):
                        setattr(item, key, value)
                break
        self._refresh_list()
        self._update_status_bar()
    
    def remove_item(self, item_id: int):
        """Remove an item"""
        self._items = [i for i in self._items if i.id != item_id]
        if self._selected_id == item_id:
            self._selected_id = None
            self._item_actions_box.set_visible(False)
        self._refresh_list()
        self._update_status_bar()
    
    def _refresh_list(self):
        # Clear existing
        child = self._list_box.get_first_child()
        while child:
            self._list_box.remove(child)
            child = self._list_box.get_first_child()
        
        # Sort: running first, then by priority
        sorted_items = sorted(
            self._items,
            key=lambda x: (
                0 if x.status == QueueItemStatus.RUNNING else
                1 if x.status == QueueItemStatus.PENDING else
                2 if x.status == QueueItemStatus.PAUSED else
                3,
                -x.priority.value
            )
        )
        
        for item in sorted_items:
            row = self._create_item_row(item)
            self._list_box.append(row)
    
    def _create_item_row(self, item: QueueItem) -> Adw.ActionRow:
        row = Adw.ActionRow()
        row.set_activatable(True)
        
        # Title: operation + archive
        title = f"{item.operation}: {item.archive}"
        row.set_title(title)
        
        # Subtitle: status + priority + progress
        status_text = f"Estado: {item.status.value} | Prioridad: {item.priority.name}"
        if item.status == QueueItemStatus.RUNNING:
            status_text += f" | Progreso: {item.progress:.0f}%"
            if item.current_file:
                status_text += f" ({item.current_file})"
        elif item.error_message:
            status_text += f" | Error: {item.error_message[:50]}"
        
        row.set_subtitle(status_text)
        
        # Status badge
        badge = Gtk.Label()
        badge.add_css_class("caption")
        status_css = {
            QueueItemStatus.PENDING: "info",
            QueueItemStatus.RUNNING: "success",
            QueueItemStatus.PAUSED: "warning",
            QueueItemStatus.COMPLETED: "success",
            QueueItemStatus.FAILED: "error",
            QueueItemStatus.CANCELLED: "dim-label",
        }
        badge.add_css_class(status_css.get(item.status, "dim-label"))
        badge.set_text(item.status.value.upper())
        row.add_suffix(badge)
        
        # Priority indicator
        if item.priority in (Priority.HIGH, Priority.CRITICAL):
            priority_badge = Gtk.Label(label="⚡")
            priority_badge.add_css_class("caption")
            row.add_suffix(priority_badge)
        
        row.item_id = item.id
        return row
    
    def _on_row_selected(self, list_box: Gtk.ListBox, row: Optional[Adw.ActionRow]):
        if row and hasattr(row, 'item_id'):
            self._selected_id = row.item_id
            item = next((i for i in self._items if i.id == self._selected_id), None)
            self._item_actions_box.set_visible(True)
            
            if item:
                # Show/hide buttons based on status
                self._pause_item_btn.set_visible(item.status in (QueueItemStatus.PENDING, QueueItemStatus.RUNNING))
                self._resume_item_btn.set_visible(item.status == QueueItemStatus.PAUSED)
                self._retry_btn.set_visible(item.status == QueueItemStatus.FAILED)
                self._cancel_btn.set_visible(item.status in (QueueItemStatus.PENDING, QueueItemStatus.RUNNING, QueueItemStatus.PAUSED))
        else:
            self._selected_id = None
            self._item_actions_box.set_visible(False)
    
    def _update_status_bar(self):
        pending = sum(1 for i in self._items if i.status == QueueItemStatus.PENDING)
        running = sum(1 for i in self._items if i.status == QueueItemStatus.RUNNING)
        paused = sum(1 for i in self._items if i.status == QueueItemStatus.PAUSED)
        
        self._pending_label.set_text(f"Pendientes: {pending}")
        self._running_label.set_text(f"En ejecución: {running}")
        self._paused_label.set_text(f"Pausados: {paused}")
    
    # Event handlers
    def _on_pause_queue(self, button: Gtk.Button):
        self.emit("item-action", "pause_queue", "queue")
    
    def _on_resume_queue(self, button: Gtk.Button):
        self.emit("item-action", "resume_queue", "queue")
    
    def _on_pause_item(self, button: Gtk.Button):
        if self._selected_id is not None:
            self.emit("item-action", "pause_item", str(self._selected_id))
    
    def _on_resume_item(self, button: Gtk.Button):
        if self._selected_id is not None:
            self.emit("item-action", "resume_item", str(self._selected_id))
    
    def _on_retry_item(self, button: Gtk.Button):
        if self._selected_id is not None:
            self.emit("item-action", "retry_item", str(self._selected_id))
    
    def _on_cancel_item(self, button: Gtk.Button):
        if self._selected_id is not None:
            self.emit("item-action", "cancel_item", str(self._selected_id))
    
    # Public API for queue state
    def set_queue_paused(self, paused: bool):
        self._pause_queue_btn.set_sensitive(not paused)
        self._resume_queue_btn.set_sensitive(paused)


# Convenience function for creating queue item from CLI output
def parse_queue_list_output(output: str) -> List[QueueItem]:
    """Parse queue list output from CLI"""
    items = []
    lines = output.strip().split('\n')
    
    for line in lines[2:]:  # Skip header lines
        if not line.strip() or line.startswith('-'):
            continue
        parts = line.split()
        if len(parts) >= 5:
            try:
                item_id = int(parts[0])
                status = QueueItemStatus(parts[1].lower())
                priority = Priority[parts[2].upper()]
                operation = parts[3]
                archive = parts[4]
                
                items.append(QueueItem(
                    id=item_id,
                    uuid="",
                    operation=operation,
                    archive=archive,
                    output=None,
                    priority=priority,
                    status=status
                ))
            except (ValueError, KeyError, IndexError):
                continue
    
    return items