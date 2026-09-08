"""
DBus client for communicating with triplewrapper-core service
"""

import asyncio
import logging
from typing import Optional, List, Dict, Any
from dataclasses import dataclass
from pathlib import Path

import gi
gi.require_version('GLib', '2.0')
gi.require_version('Gio', '2.0')
from gi.repository import GLib, Gio

from ...utils.formatting import format_bytes

logger = logging.getLogger(__name__)

# DBus constants
SERVICE_NAME = 'com.triplewrapper.Core'
OBJECT_PATH = '/com/triplewrapper/Core'
INTERFACE_NAME = 'com.triplewrapper.Core'


@dataclass
class DiskInfo:
    mount_point: str
    label: str
    filesystem: str
    total_bytes: int
    free_bytes: int
    used_bytes: int
    is_removable: bool
    is_system: bool
    device_path: Optional[str] = None
    
    @property
    def free_gb(self) -> float:
        return self.free_bytes / (1024 ** 3)
    
    @property
    def total_gb(self) -> float:
        return self.total_bytes / (1024 ** 3)
    
    @property
    def usage_percent(self) -> float:
        if self.total_bytes == 0:
            return 0.0
        return (self.used_bytes / self.total_bytes) * 100


@dataclass
class StorageVerdict:
    verdict: str  # 'internal_ok', 'external_required', 'critical_error'
    source_disk: DiskInfo
    workspace_disk: Optional[DiskInfo] = None
    estimate: Optional[Dict[str, Any]] = None
    workdir: Optional[str] = None
    sevenzip_workdir_param: Optional[str] = None
    message: str = ""
    requires_confirmation: bool = False


class CoreClient:
    """Async DBus client for triplewrapper-core"""
    
    def __init__(self):
        self._proxy: Optional[Gio.DBusProxy] = None
        self._connection: Optional[Gio.DBusConnection] = None
        self._ready = asyncio.Event()
    
    async def connect(self) -> bool:
        """Connect to the core service"""
        try:
            # Get session bus
            self._connection = await self._get_session_bus()
            
            # Create proxy
            self._proxy = Gio.DBusProxy.new_sync(
                self._connection,
                Gio.DBusProxyFlags.NONE,
                None,
                SERVICE_NAME,
                OBJECT_PATH,
                INTERFACE_NAME,
                None
            )
            
            # Test connection
            await self.ping()
            self._ready.set()
            logger.info("Connected to triplewrapper-core service")
            return True
            
        except Exception as e:
            logger.error(f"Failed to connect to core service: {e}")
            self._ready.clear()
            return False
    
    async def _get_session_bus(self) -> Gio.DBusConnection:
        """Get session bus connection"""
        loop = asyncio.get_event_loop()
        return await loop.run_in_executor(None, Gio.bus_get_sync, Gio.BusType.SESSION, None)
    
    async def ping(self) -> str:
        """Ping the service"""
        if not self._proxy:
            raise RuntimeError("Not connected")
        
        result = await self._call_method('Ping')
        return result.unpack()
    
    async def get_disks(self) -> List[DiskInfo]:
        """Get list of disks"""
        if not self._proxy:
            raise RuntimeError("Not connected")
        
        result = await self._call_method('GetDisks')
        disks_data = result.unpack()
        
        disks = []
        for d in disks_data:
            disks.append(DiskInfo(
                mount_point=d.get('mount_point', ''),
                label=d.get('label', ''),
                filesystem=d.get('filesystem', ''),
                total_bytes=d.get('total_bytes', 0),
                free_bytes=d.get('free_bytes', 0),
                used_bytes=d.get('used_bytes', 0),
                is_removable=d.get('is_removable', False),
                is_system=d.get('is_system', False),
                device_path=d.get('device_path'),
            ))
        return disks
    
    async def get_verdict(
        self,
        archive_path: str,
        bytes_to_remove: int = 0,
        bytes_to_add: int = 0
    ) -> StorageVerdict:
        """Get storage verdict for an operation"""
        if not self._proxy:
            raise RuntimeError("Not connected")
        
        result = await self._call_method('GetVerdict', archive_path, bytes_to_remove, bytes_to_add)
        verdict_data = result.unpack()
        
        return self._parse_verdict(verdict_data)
    
    async def start_operation(
        self,
        archive_path: str,
        op_type: str,
        output_path: Optional[str] = None,
        files_to_process: Optional[List[str]] = None,
        compression_level: int = 5,
        workspace_override: Optional[str] = None,
        verify_after: bool = True,
        dry_run: bool = False
    ) -> Dict[str, Any]:
        """Start an archive operation"""
        if not self._proxy:
            raise RuntimeError("Not connected")
        
        request = {
            'id': 0,  # Will be generated by service
            'op_type': op_type,
            'archive_path': archive_path,
            'output_path': output_path,
            'files_to_process': files_to_process or [],
            'compression_level': compression_level,
            'compression_format': None,
            'workspace_override': workspace_override,
            'verify_after': verify_after,
            'dry_run': dry_run,
        }
        
        result = await self._call_method('StartOperation', request)
        return result.unpack()
    
    async def cancel_operation(self, operation_id: int) -> bool:
        """Cancel a running operation"""
        if not self._proxy:
            raise RuntimeError("Not connected")
        
        result = await self._call_method('CancelOperation', operation_id)
        return result.unpack()
    
    async def get_progress(self, operation_id: int) -> Optional[Dict[str, Any]]:
        """Get progress for an operation"""
        if not self._proxy:
            raise RuntimeError("Not connected")
        
        result = await self._call_method('GetProgress', operation_id)
        progress = result.unpack()
        return progress if progress else None
    
    def _parse_verdict(self, data: Dict[str, Any]) -> StorageVerdict:
        """Parse verdict from DBus response"""
        source_disk = self._parse_disk(data.get('source_disk', {}))
        workspace_disk = None
        if data.get('workspace_disk'):
            workspace_disk = self._parse_disk(data['workspace_disk'])
        
        return StorageVerdict(
            verdict=data.get('verdict', 'critical_error'),
            source_disk=source_disk,
            workspace_disk=workspace_disk,
            estimate=data.get('estimate'),
            workdir=data.get('workdir'),
            sevenzip_workdir_param=data.get('sevenzip_workdir_param'),
            message=data.get('message', ''),
            requires_confirmation=data.get('requires_confirmation', False),
        )
    
    def _parse_disk(self, data: Dict[str, Any]) -> DiskInfo:
        return DiskInfo(
            mount_point=data.get('mount_point', ''),
            label=data.get('label', ''),
            filesystem=data.get('filesystem', ''),
            total_bytes=data.get('total_bytes', 0),
            free_bytes=data.get('free_bytes', 0),
            used_bytes=data.get('used_bytes', 0),
            is_removable=data.get('is_removable', False),
            is_system=data.get('is_system', False),
            device_path=data.get('device_path'),
        )
    
    async def _call_method(self, method: str, *args) -> GLib.Variant:
        """Call a DBus method asynchronously"""
        loop = asyncio.get_event_loop()
        
        def _sync_call():
            return self._proxy.call_sync(
                method,
                GLib.Variant('(*)', args) if args else None,
                Gio.DBusCallFlags.NONE,
                -1,  # Default timeout
                None
            )
        
        try:
            result = await loop.run_in_executor(None, _sync_call)
            return result
        except GLib.Error as e:
            logger.error(f"DBus call {method} failed: {e}")
            raise RuntimeError(f"DBus call failed: {e}")


class MockCoreClient(CoreClient):
    """Mock client for testing without DBus service"""
    
    def __init__(self):
        super().__init__()
        self._ready.set()
    
    async def connect(self) -> bool:
        return True
    
    async def ping(self) -> str:
        return "pong (mock)"
    
    async def get_disks(self) -> List[DiskInfo]:
        # Return mock disks
        return [
            DiskInfo(
                mount_point="/",
                label="Disco raíz",
                filesystem="btrfs",
                total_bytes=500 * 1024**3,
                free_bytes=100 * 1024**3,
                used_bytes=400 * 1024**3,
                is_removable=False,
                is_system=True,
                device_path="/dev/nvme0n1p2",
            ),
            DiskInfo(
                mount_point="/mnt/hdd_seagate",
                label="Seagate HDD",
                filesystem="ext4",
                total_bytes=2 * 1024**4,
                free_bytes=500 * 1024**3,
                used_bytes=1500 * 1024**3,
                is_removable=True,
                is_system=False,
                device_path="/dev/sdb1",
            ),
        ]
    
    async def get_verdict(self, archive_path: str, bytes_to_remove: int, bytes_to_add: int) -> StorageVerdict:
        import os
        path = Path(archive_path)
        current_size = path.stat().st_size if path.exists() else 2 * 1024**3
        
        # Simple logic: if > 100GB, need external
        space_needed = current_size + max(0, current_size - bytes_to_remove + bytes_to_add)
        
        disks = await self.get_disks()
        source_disk = next((d for d in disks if str(path).startswith(d.mount_point)), disks[0])
        
        if source_disk.free_bytes >= space_needed:
            return StorageVerdict(
                verdict='internal_ok',
                source_disk=source_disk,
                estimate={'current_size': current_size, 'estimated_final_size': space_needed},
                message=f"Espacio suficiente en {source_disk.label}",
            )
        else:
            external = next((d for d in disks if d != source_disk), None)
            if external:
                return StorageVerdict(
                    verdict='external_required',
                    source_disk=source_disk,
                    workspace_disk=external,
                    estimate={'current_size': current_size, 'estimated_final_size': space_needed},
                    workdir=f"{external.mount_point}/.triplewrapper_cache",
                    sevenzip_workdir_param=f"-w{external.mount_point}/.triplewrapper_cache",
                    message=f"Usando caché en {external.label}",
                    requires_confirmation=True,
                )
        
        return StorageVerdict(
            verdict='critical_error',
            source_disk=source_disk,
            estimate={'current_size': current_size, 'estimated_final_size': space_needed},
            message="Sin espacio suficiente",
        )


# Global client instance
_client: Optional[CoreClient] = None


async def get_core_client(use_mock: bool = False) -> CoreClient:
    """Get global core client instance"""
    global _client
    
    if _client is None:
        if use_mock:
            _client = MockCoreClient()
        else:
            _client = CoreClient()
            await _client.connect()
    
    return _client