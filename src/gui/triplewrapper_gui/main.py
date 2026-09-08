#!/usr/bin/env python3
"""
TripleWrapper GUI - Main entry point
"""

import sys
import gi

gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
gi.require_version('Gdk', '4.0')

from gi.repository import Gtk, Adw, Gio, GLib
import signal

from .window import TripleWrapperWindow
from .utils.settings import Settings


class TripleWrapperApplication(Adw.Application):
    """Main application class"""
    
    def __init__(self):
        super().__init__(
            application_id='com.triplewrapper.TripleWrapper',
            flags=Gio.ApplicationFlags.DEFAULT_FLAGS | Gio.ApplicationFlags.HANDLES_OPEN
        )
        self.settings = Settings()
        self.window = None

    def do_startup(self):
        Adw.Application.do_startup(self)
        
        # Set up actions
        self._setup_actions()
        
        # Load CSS
        self._load_css()

    def do_activate(self):
        if not self.window:
            self.window = TripleWrapperWindow(self)
        self.window.present()

    def do_open(self, files, n_files, hint):
        """Handle file opening from command line or file manager"""
        if self.window and n_files > 0:
            file_path = files[0].get_path()
            if file_path:
                self.window.open_archive(file_path)

    def _setup_actions(self):
        # Quit action
        quit_action = Gio.SimpleAction.new('quit', None)
        quit_action.connect('activate', lambda *_: self.quit())
        self.add_action(quit_action)
        self.set_accels_for_action('app.quit', ['<Ctrl>Q'])

        # About action
        about_action = Gio.SimpleAction.new('about', None)
        about_action.connect('activate', self._show_about)
        self.add_action(about_action)

        # Preferences action
        prefs_action = Gio.SimpleAction.new('preferences', None)
        prefs_action.connect('activate', self._show_preferences)
        self.add_action(prefs_action)

        # Keyboard shortcuts
        self.set_accels_for_action('app.preferences', ['<Ctrl>comma'])

    def _show_about(self, action, param):
        dialog = Adw.AboutDialog(
            application_name='TripleWrapper',
            application_icon='com.triplewrapper.TripleWrapper',
            version='0.1.0',
            developers=['TripleWrapper Team'],
            license_type=Adw.LicenseType.GPL_3_0,
            website='https://github.com/triplewrapper/triplewrapper',
            issue_url='https://github.com/triplewrapper/triplewrapper/issues',
            comments=_('Smart archive manager with intelligent storage decisions'),
            translator_credits=_('translator-credits'),
        )
        dialog.present(self.window)

    def _show_preferences(self, action, param):
        # TODO: Implement preferences dialog
        pass

    def _load_css(self):
        """Load custom CSS"""
        css_provider = Gtk.CssProvider()
        css = b"""
        /* Custom styles for TripleWrapper */
        .graph-widget {
            background-color: @theme_bg_color;
        }
        
        .donut-chart {
            background-color: transparent;
        }
        
        .disk-panel {
            background-color: @theme_bg_color;
        }
        
        .action-button {
            min-width: 140px;
            min-height: 44px;
            font-weight: 600;
        }
        
        .verdict-internal { color: @success_color; }
        .verdict-external { color: @warning_color; }
        .verdict-critical { color: @error_color; }
        """
        css_provider.load_from_data(css)
        
        display = Gdk.Display.get_default()
        if display:
            Gtk.StyleContext.add_provider_for_display(
                display, css_provider, Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
            )


def main():
    # Handle SIGINT gracefully
    signal.signal(signal.SIGINT, signal.SIG_DFL)
    
    app = TripleWrapperApplication()
    return app.run(sys.argv)


if __name__ == '__main__':
    sys.exit(main())