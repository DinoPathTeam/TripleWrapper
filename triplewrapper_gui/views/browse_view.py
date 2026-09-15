"""Archive content browser: flat entry list with delete/add actions."""
from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, GObject, Gtk

from ..i18n import _


def human_size(n: int) -> str:
    units = ["B", "KB", "MB", "GB", "TB"]
    value = float(n)
    for unit in units:
        if value < 1024:
            return f"{value:.1f} {unit}"
        value /= 1024
    return f"{value:.1f} PB"


class BrowseView(Gtk.Box):
    """Flat list of archive entries. Tick to select, delete or add files."""

    __gtype_name__ = "TripleWrapperBrowseView"
    __gsignals__ = {  # noqa: RUF012 - GObject signal map must be a dict
        "delete-requested": (GObject.SignalFlags.RUN_FIRST, None, (object,)),
        "add-requested": (GObject.SignalFlags.RUN_FIRST, None, (object,)),
        "back-requested": (GObject.SignalFlags.RUN_FIRST, None, ()),
    }

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=18)
        self._checks: dict[str, Gtk.CheckButton] = {}
        self._build_ui()

    def _build_ui(self) -> None:
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=18)
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)
        scrolled.set_child(content)
        self.append(scrolled)

        title = Gtk.Label(label=_("Contenido del archivo"))
        title.add_css_class("title-2")
        title.set_halign(Gtk.Align.CENTER)
        content.append(title)

        # Entry list card
        card = Gtk.Frame()
        card.add_css_class("card")
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        box.set_margin_top(16)
        box.set_margin_bottom(16)
        box.set_margin_start(16)
        box.set_margin_end(16)
        card.set_child(box)

        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_min_content_height(280)
        scrolled.set_vexpand(True)

        self._list_box = Gtk.ListBox()
        self._list_box.set_selection_mode(Gtk.SelectionMode.NONE)
        scrolled.set_child(self._list_box)
        box.append(scrolled)
        content.append(card)

        # Action bar
        actions = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        actions.set_halign(Gtk.Align.CENTER)

        back_btn = Gtk.Button(label=_("Atrás"))
        back_btn.connect("clicked", lambda *_: self.emit("back-requested"))
        actions.append(back_btn)

        self._delete_btn = Gtk.Button(label=_("Eliminar seleccionados"))
        self._delete_btn.add_css_class("destructive-action")
        self._delete_btn.add_css_class("pill")
        self._delete_btn.connect("clicked", self._on_delete_clicked)
        actions.append(self._delete_btn)

        add_btn = Gtk.Button(label=_("Añadir archivos…"))
        add_btn.add_css_class("suggested-action")
        add_btn.add_css_class("pill")
        add_btn.connect("clicked", self._on_add_clicked)
        actions.append(add_btn)

        self.append(actions)

    # ----------------------------------------------------------------- API
    def set_entries(self, entries: list[dict]) -> None:
        """Replace the list. Each entry: path, size, is_directory."""
        self._checks.clear()
        child = self._list_box.get_first_child()
        while child:
            self._list_box.remove(child)
            child = self._list_box.get_first_child()

        if not entries:
            row = Adw.ActionRow(
                title=_("Archivo vacío o sin entradas legibles"))
            row.set_sensitive(False)
            self._list_box.append(row)
            return

        for entry in sorted(entries, key=lambda e: str(e.get("path", ""))):
            path = str(entry.get("path", "?"))
            size = human_size(int(entry.get("size", 0) or 0))
            suffix = "/" if entry.get("is_directory") else ""
            row = Adw.ActionRow(title=f"{path}{suffix}", subtitle=size)
            row.set_activatable(True)
            check = Gtk.CheckButton()
            check.set_valign(Gtk.Align.CENTER)
            row.add_prefix(check)
            row.connect("activated", lambda r, c=check: c.set_active(not c.get_active()))
            self._checks[path] = check
            self._list_box.append(row)

    def selected_paths(self) -> list[str]:
        return [p for p, c in self._checks.items() if c.get_active()]

    # -------------------------------------------------------------- Events
    def _on_delete_clicked(self, _btn: Gtk.Button) -> None:
        self.emit("delete-requested", self.selected_paths())

    def _on_add_clicked(self, _btn: Gtk.Button) -> None:
        # Same reason as welcome_view: FileDialog wrappers never present
        # here, so use the classic dialog with multi-select instead.
        dialog = Gtk.FileChooserDialog(
            title=_("Añadir archivos al archivo"),
            action=Gtk.FileChooserAction.OPEN,
        )
        dialog.set_select_multiple(True)
        root = self.get_root()
        if isinstance(root, Gtk.Window):
            dialog.set_transient_for(root)
        dialog.add_button("_" + _("Cancelar"), Gtk.ResponseType.CANCEL)
        dialog.add_button("_" + _("Añadir"), Gtk.ResponseType.ACCEPT)
        dialog.set_default_response(Gtk.ResponseType.ACCEPT)
        dialog.connect("response", self._on_add_response)
        dialog.present()

    def _on_add_response(self, dialog: Gtk.FileChooserDialog, response: int) -> None:
        try:
            if response != Gtk.ResponseType.ACCEPT:
                return
            model = dialog.get_files()
            paths = []
            for i in range(model.get_n_items()):
                f = model.get_item(i)
                path = f.get_path() if f else None
                if path:
                    paths.append(path)
            if paths:
                self.emit("add-requested", paths)
        finally:
            dialog.destroy()
