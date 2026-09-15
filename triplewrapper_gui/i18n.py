"""Gettext setup: Spanish source strings, English catalog for other locales.

Locale files live in ``triplewrapper_gui/locale/<lang>/LC_MESSAGES/`` and
are built from ``po/*.po`` by Meson. Missing catalogs fall back to the
Spanish source strings, so development works without building.
"""
from __future__ import annotations

import gettext
import locale
from pathlib import Path

_DOMAIN = "triplewrapper"
_LOCALEDIR = Path(__file__).resolve().parent / "locale"

try:
    locale.setlocale(locale.LC_ALL, "")
except locale.Error:
    pass

_translation = gettext.translation(_DOMAIN, localedir=str(_LOCALEDIR), fallback=True)


def _(message: str) -> str:
    """Translate a Spanish source string to the active locale."""
    return _translation.gettext(message)
