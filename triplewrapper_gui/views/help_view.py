"""Help and documentation view (usage, permissions, compatibility)."""
from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Gtk

from ..i18n import _


class HelpView(Gtk.Box):
    """Static help content; navigation back is owned by NavigationView."""

    __gtype_name__ = "TripleWrapperHelpView"

    def __init__(self) -> None:
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=18)
        self._build_ui()

    def _build_ui(self) -> None:
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=18)
        scrolled = Gtk.ScrolledWindow()
        scrolled.set_policy(Gtk.PolicyType.NEVER, Gtk.PolicyType.AUTOMATIC)
        scrolled.set_vexpand(True)
        scrolled.set_child(content)
        self.append(scrolled)

        title = Gtk.Label(label=_("Ayuda y documentación"))
        title.add_css_class("title-2")
        title.set_halign(Gtk.Align.CENTER)
        content.append(title)

        content.append(self._section(
            _("Uso básico"),
            _("1. Pulsa Abrir archivo o arrastra un comprimido a la ventana.\n"
              "2. Pulsa Analizar para ver espacio y workspace sugerido.\n"
              "3. Elige Iniciar para extraer ahora o Encolar para más tarde.\n"
              "4. Explorar contenido muestra y edita el interior sin salir."),
        ))
        content.append(self._section(
            _("Permisos"),
            _("TripleWrapper es local y sandboxed (Flatpak): sin red y sin "
              "acceso a tu home completo.\n"
              "· Selector de archivos: acceso puntual por portales.\n"
              "· Documentos, Descargas y medios extraíbles (/media, /mnt): "
              "lectura y escritura como destinos de trabajo.\n"
              "· UDisks2: montar USBs desde el panel de dispositivos."),
        ))
        content.append(self._section(
            _("Compatibilidad de formatos"),
            _("Lectura y escritura: 7z, ZIP, TAR y variantes (GZ, XZ, ZST, "
              "BZ2), Pixz.\n"
              "Solo lectura: RAR (formato propietario; crear o modificar "
              "archivos .rar no es posible por licencia).\n"
              "Otros formatos: plugins externos triplewrapper-*."),
        ))
        content.append(self._section(
            _("Si algo falla"),
            _("· La app muestra el motivo en un aviso: sin espacio, sin "
              "permiso o archivo dañado.\n"
              "· Una operación atascada en cola se cancela o reintenta desde "
              "Cola de operaciones.\n"
              "· Reporta errores con el archivo y el mensaje del aviso."),
        ))

    @staticmethod
    def _section(heading: str, body: str) -> Gtk.Frame:
        card = Gtk.Frame()
        card.add_css_class("card")
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8)
        box.set_margin_top(16)
        box.set_margin_bottom(16)
        box.set_margin_start(16)
        box.set_margin_end(16)
        card.set_child(box)
        title = Gtk.Label(label=heading)
        title.add_css_class("heading")
        title.set_halign(Gtk.Align.START)
        box.append(title)
        text = Gtk.Label(label=body)
        text.set_halign(Gtk.Align.START)
        text.set_wrap(True)
        text.set_selectable(True)
        box.append(text)
        return card


__all__ = ["HelpView"]
