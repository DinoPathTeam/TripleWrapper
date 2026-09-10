"""Gtk.Application subclass with Libadwaita styling."""
from __future__ import annotations

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")

from gi.repository import Adw, Gio, Gtk

from .main_window import TripleWrapperWindow


class TripleWrapperApplication(Adw.Application):
    """Main application class."""

    def __init__(self, application_id: str, resourcebasepath: str) -> None:
        super().__init__(
            application_id=application_id,
            flags=Gio.ApplicationFlags.FLAGS_NONE,
        )
        self.resourcebasepath = resourcebasepath
        self._window: TripleWrapperWindow | None = None

    def do_activate(self) -> None:
        if self._window is None:
            self._window = TripleWrapperWindow(application=self)
        self._window.present()

    def do_startup(self) -> None:
        Adw.Application.do_startup(self)
        self._fix_legacy_dark_theme_key()
        self.setup_actions()

    @staticmethod
    def _fix_legacy_dark_theme_key() -> None:
        """Ignore GtkSettings:gtk-application-prefer-dark-theme for this process.

        Some desktops (e.g. KDE Breeze-dark) set that legacy key globally,
        which libadwaita explicitly does not support (it warns at startup).
        Resetting it process-locally lets Adw.StyleManager own the color
        scheme; the user's config file is left untouched.
        """
        settings = Gtk.Settings.get_default()
        if settings is not None:
            settings.reset_property("gtk-application-prefer-dark-theme")

    def setup_actions(self) -> None:
        # Light/Dark/Default style actions
        style_manager = Adw.StyleManager.get_default()
        for variant, value in [
            ("app.style-light", Adw.ColorScheme.FORCE_LIGHT),
            ("app.style-dark", Adw.ColorScheme.FORCE_DARK),
            ("app.style-default", Adw.ColorScheme.DEFAULT),
        ]:
            action = Gio.SimpleAction.new(variant.split(".")[-1], None)
            action.connect(
                "activate",
                lambda a, p, sm=style_manager, v=value: sm.set_color_scheme(v),
            )
            self.add_action(action)

        # Quit action
        quit_action = Gio.SimpleAction.new("quit", None)
        quit_action.connect("activate", lambda *_: self.quit())
        self.add_action(quit_action)
        self.set_accels_for_action("app.quit", ["Q"])