"""
Settings management for TripleWrapper GUI
"""

import json
import os
from pathlib import Path
from typing import Any, Dict, Optional
import gi

gi.require_version('Gio', '2.0')
from gi.repository import Gio, GLib


class Settings:
    """Wrapper around GSettings for TripleWrapper"""
    
    SCHEMA_ID = 'com.triplewrapper.TripleWrapper'
    
    def __init__(self):
        self._settings = Gio.Settings.new(self.SCHEMA_ID)
        
        # Bind to Python properties for convenience
        self._bind_properties()
    
    def _bind_properties(self):
        """Bind GSettings keys to instance attributes"""
        self._settings.bind(
            'default-compression-level',
            self,
            'default_compression_level',
            Gio.SettingsBindFlags.DEFAULT
        )
        self._settings.bind(
            'default-compression-ratio',
            self,
            'default_compression_ratio',
            Gio.SettingsBindFlags.DEFAULT
        )
        self._settings.bind(
            'safety-margin-mb',
            self,
            'safety_margin_mb',
            Gio.SettingsBindFlags.DEFAULT
        )
        self._settings.bind(
            'verify-checksums',
            self,
            'verify_checksums',
            Gio.SettingsBindFlags.DEFAULT
        )
        self._settings.bind(
            'checksum-algorithm',
            self,
            'checksum_algorithm',
            Gio.SettingsBindFlags.DEFAULT
        )
        self._settings.bind(
            'auto-select-workspace',
            self,
            'auto_select_workspace',
            Gio.SettingsBindFlags.DEFAULT
        )
        self._settings.bind(
            'keep-temp-on-failure',
            self,
            'keep_temp_on_failure',
            Gio.SettingsBindFlags.DEFAULT
        )
        self._settings.bind(
            'ui-theme',
            self,
            'ui_theme',
            Gio.SettingsBindFlags.DEFAULT
        )
        self._settings.bind(
            'preferred-cache-dirs',
            self,
            'preferred_cache_dirs',
            Gio.SettingsBindFlags.DEFAULT
        )
    
    # Properties (will be synced with GSettings)
    default_compression_level: int = 5
    default_compression_ratio: float = 0.5
    safety_margin_mb: int = 512
    verify_checksums: bool = True
    checksum_algorithm: str = 'blake3'
    auto_select_workspace: bool = True
    keep_temp_on_failure: bool = False
    ui_theme: str = 'auto'
    preferred_cache_dirs: list = None
    
    def __getattr__(self, name: str) -> Any:
        """Allow access to settings as attributes"""
        if name in {
            'default_compression_level', 'default_compression_ratio',
            'safety_margin_mb', 'verify_checksums', 'checksum_algorithm',
            'auto_select_workspace', 'keep_temp_on_failure', 'ui_theme',
            'preferred_cache_dirs'
        }:
            return self._settings.get_value(name.replace('_', '-')).unpack()
        raise AttributeError(f"'{type(self).__name__}' has no attribute '{name}'")
    
    def __setattr__(self, name: str, value: Any):
        """Allow setting settings as attributes"""
        if name.startswith('_') or name in {
            'default_compression_level', 'default_compression_ratio',
            'safety_margin_mb', 'verify_checksums', 'checksum_algorithm',
            'auto_select_workspace', 'keep_temp_on_failure', 'ui_theme',
            'preferred_cache_dirs'
        }:
            if not name.startswith('_'):
                self._settings.set_value(name.replace('_', '-'), GLib.Variant.new_from_data(
                    GLib.VariantType.new('v'), json.dumps(value).encode(), True
                ))
        super().__setattr__(name, value)
    
    def reset_to_defaults(self):
        """Reset all settings to defaults"""
        self._settings.reset(self.SCHEMA_ID)
    
    def get_cache_directories(self) -> list:
        """Get list of preferred cache directories, expanded"""
        dirs = self.preferred_cache_dirs or ['/mnt', '/media', '/run/media']
        return [os.path.expanduser(d) for d in dirs]


def get_settings() -> Settings:
    """Get global settings instance"""
    return Settings()