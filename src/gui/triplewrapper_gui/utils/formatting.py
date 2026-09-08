"""
Formatting utilities for TripleWrapper GUI
"""

import math
from typing import Optional


def format_bytes(bytes_val: int, precision: int = 1) -> str:
    """Format bytes as human-readable string"""
    if bytes_val == 0:
        return "0 B"
    
    units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
    unit_idx = 0
    size = float(bytes_val)
    
    while size >= 1024.0 and unit_idx < len(units) - 1:
        size /= 1024.0
        unit_idx += 1
    
    if unit_idx == 0:
        return f"{int(size)} {units[unit_idx]}"
    else:
        return f"{size:.{precision}f} {units[unit_idx]}"


def format_speed(bytes_per_sec: float) -> str:
    """Format speed as MB/s or KB/s"""
    if bytes_per_sec >= 1_048_576:
        return f"{bytes_per_sec / 1_048_576:.1f} MB/s"
    elif bytes_per_sec >= 1024:
        return f"{bytes_per_sec / 1024:.1f} KB/s"
    else:
        return f"{bytes_per_sec:.0f} B/s"


def format_duration(seconds: float) -> str:
    """Format duration as human-readable string"""
    if seconds < 60:
        return f"{int(seconds)}s"
    elif seconds < 3600:
        mins = int(seconds // 60)
        secs = int(seconds % 60)
        return f"{mins}m {secs}s"
    elif seconds < 86400:
        hours = int(seconds // 3600)
        mins = int((seconds % 3600) // 60)
        return f"{hours}h {mins}m"
    else:
        days = int(seconds // 86400)
        hours = int((seconds % 86400) // 3600)
        return f"{days}d {hours}h"


def format_percentage(value: float, precision: int = 1) -> str:
    """Format percentage"""
    return f"{value:.{precision}f}%"


def format_eta(seconds: Optional[float]) -> str:
    """Format ETA"""
    if seconds is None or seconds <= 0:
        return "—"
    return format_duration(seconds)


def truncate(text: str, max_len: int, suffix: str = "…") -> str:
    """Truncate text to max length"""
    if len(text) <= max_len:
        return text
    return text[:max_len - len(suffix)] + suffix


def format_operation_type(op_type: str) -> str:
    """Format operation type for display"""
    mapping = {
        'extract': 'Extraer',
        'modify': 'Modificar',
        'clean': 'Limpiar',
        'test': 'Verificar',
        'list': 'Listar',
    }
    return mapping.get(op_type, op_type.capitalize())


def format_verdict_type(verdict: str) -> str:
    """Format verdict type for display"""
    mapping = {
        'internal_ok': 'Interno',
        'external_required': 'Externo',
        'critical_error': 'Crítico',
    }
    return mapping.get(verdict, verdict)


def get_verdict_css_class(verdict: str) -> str:
    """Get CSS class for verdict"""
    mapping = {
        'internal_ok': 'verdict-internal',
        'external_required': 'verdict-external',
        'critical_error': 'verdict-critical',
    }
    return mapping.get(verdict, '')


def parse_size_string(size_str: str) -> int:
    """Parse human-readable size string to bytes"""
    size_str = size_str.strip().upper()
    units = {
        'B': 1,
        'KB': 1024,
        'MB': 1024**2,
        'GB': 1024**3,
        'TB': 1024**4,
        'PB': 1024**5,
    }
    
    for unit, mult in units.items():
        if size_str.endswith(unit):
            try:
                return int(float(size_str[:-len(unit)]) * mult)
            except ValueError:
                pass
    return 0


def calculate_percentage(part: int, total: int) -> float:
    """Calculate percentage safely"""
    if total == 0:
        return 0.0
    return min(100.0, (part / total) * 100.0)