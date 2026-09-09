"""Welcome / file selection view."""
from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import GObject, Gtk, Gio


class WelcomeView(Gtk.Box):
    """Landing view: pick an archive and analyze."""

    __gtype_name__ = "TripleWrapperWelcomeView"
    __gsignals__ = {
        "file-selected": (GObject.SignalFlags.RUN_FIRST, None, (str,)),
        "analyze-requested": (GObject.SignalFlags.RUN_FIRST, None, ()),
    }

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=24)
        self.set_valign(Gtk.Align.CENTER)
        self.set_halign(Gtk.Align.CENTER)
        self.current_path: str | None = None
        self._build_ui()

    def _build_ui(self) -> None:
        # Title block
        title_box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        title_box.set_halign(Gtk.Align.CENTER)

        title = Gtk.Label(label="TripleWrapper")
        title.add_css_class("title-1")
        title_box.append(title)

        subtitle = Gtk.Label(label="Gestor de archivos con reescritura segura en el mismo espacio")
        subtitle.add_css_class("dim-label")
        subtitle.set_wrap(True)
        subtitle.set_justify(Gtk.Justification.CENTER)
        title_box.append(subtitle)
        self.append(title_box)

        # Drop / file picker card
        card = Gtk.Frame()
        card.add_css_class("card")
        inner = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16)
        inner.set_margin_top(32)
        inner.set_margin_bottom(32)
        inner.set_margin_start(32)
        inner.set_margin_end(32)
        card.set_child(inner)

        icon = Gtk.Image.new_from_icon_name("package-x-generic-symbolic")
        icon.set_pixel_size(64)
        icon.add_css_class("dim-label")
        inner.append(icon)

        self._path_label = Gtk.Label(label="Ningún archivo seleccionado")
        self._path_label.add_css_class("heading")
        self._path_label.set_wrap(True)
        self._path_label.set_ellipsize(3)  # PANGO_ELLIPSIZE_END
        inner.append(self._path_label)

        btn_row = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=12)
        btn_row.set_halign(Gtk.Align.CENTER)

        open_btn = Gtk.Button(label="Abrir archivo")
        open_btn.add_css_class("suggested-action")
        open_btn.add_css_class("pill")
        open_btn.connect("clicked", self._on_open_clicked)
        btn_row.append(open_btn)

        self._analyze_btn = Gtk.Button(label="Analizar")
        self._analyze_btn.add_css_class("pill")
        self._analyze_btn.set_sensitive(False)
        self._analyze_btn.connect("clicked", lambda *_: self.emit("analyze-requested"))
        btn_row.append(self._analyze_btn)

        inner.append(btn_row)
        self.append(card)

        # Supported formats hint
        hint = Gtk.Label(
            label="Formatos: 7z · ZIP · TAR · TAR.GZ · TAR.XZ · TAR.ZST · TAR.BZ2 · Pixz"
        )
        hint.add_css_class("dim-label")
        hint.add_css_class("caption")
        hint.set_wrap(True)
        hint.set_justify(Gtk.Justification.CENTER)
        self.append(hint)

    # -------------------------------------------------------------- Callbacks
    def _on_open_clicked(self, btn: Gtk.Button) -> None:
        dialog = Gtk.FileDialog.new()
        dialog.set_title("Seleccionar archivo")

        # Build filter list
        filters = Gio.ListStore.new(Gtk.FileFilter)

        supported = Gtk.FileFilter()
        supported.set_name("Archivos soportados")
        for ext in (
            "7z", "zip", "tar", "gz", "xz", "zst", "bz2",
            "tar.gz", "tgz", "tar.xz", "txz", "tar.zst", "tar.bz2", "tbz2",
        ):
            supported.add_suffix(ext)
        filters.append(supported)

        all_files = Gtk.FileFilter()
        all_files.set_name("Todos los archivos")
        filters.append(all_files)

        dialog.set_filters(filters)
        dialog.open(self.get_root(), None, self._on_dialog_done)

    def _on_dialog_done(self, dialog, result) -> None:
        try:
            file = dialog.open_finish(result)
        except Exception:
            return
        if file is None:
            return
        path = file.get_path()
        if path is None:
            return
        self.current_path = path
        self._path_label.set_label(path.rsplit("/", 1)[-1])
        self._analyze_btn.set_sensitive(True)
        self.emit("file-selected", path)

    # ----------------------------------------------------------------- API
    def set_busy(self, busy: bool) -> None:
        self._analyze_btn.set_sensitive(not busy)
        if busy:
            self._analyze_btn.set_label("Analizando…")
        else:
            self._analyze_btn.set_label("Analizar")