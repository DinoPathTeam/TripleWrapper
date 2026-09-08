"""
Utilities package for TripleWrapper GUI
"""

from .formatting import (
    format_bytes,
    format_speed,
    format_duration,
    format_percentage,
    format_eta,
    truncate,
    format_operation_type,
    format_verdict_type,
    get_verdict_css_class,
    parse_size_string,
    calculate_percentage,
)
from .settings import Settings, get_settings

__all__ = [
    'format_bytes',
    'format_speed',
    'format_duration',
    'format_percentage',
    'format_eta',
    'truncate',
    'format_operation_type',
    'format_verdict_type',
    'get_verdict_css_class',
    'parse_size_string',
    'calculate_percentage',
    'Settings',
    'get_settings',
]