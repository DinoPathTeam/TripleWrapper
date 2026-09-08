#!/usr/bin/env python3
"""
TripleWrapper - Storage Decision Engine
Motor de decisiones de almacenamiento inteligente para gestión de archivos comprimidos.

Integra con GTK4/GObject para emitir señales de telemetría hacia el widget de gráficas Cairo.
"""

import os
import json
import shutil
import subprocess
from dataclasses import dataclass, field, asdict
from enum import Enum
from pathlib import Path
from typing import Optional, List, Callable
from gi.repository import GObject, GLib


class VerdictType(Enum):
    """Tipos de veredicto del motor de decisiones."""
    INTERNAL_OK = "internal_ok"           # Espacio suficiente en disco origen
    EXTERNAL_REQUIRED = "external_required"  # Requiere disco externo, hay uno disponible
    CRITICAL_ERROR = "critical_error"     # Sin espacio en ningún disco


@dataclass
class DiskInfo:
    """Información de una unidad de almacenamiento."""
    mount_point: str
    label: str
    total_bytes: int
    free_bytes: int
    used_bytes: int
    is_removable: bool = False
    is_system: bool = False
    filesystem: str = ""

    @property
    def free_gb(self) -> float:
        return self.free_bytes / (1024 ** 3)

    @property
    def total_gb(self) -> float:
        return self.total_bytes / (1024 ** 3)

    @property
    def usage_percent(self) -> float:
        return (self.used_bytes / self.total_bytes * 100) if self.total_bytes > 0 else 0


@dataclass
class CompressionEstimate:
    """Estimación de tamaño tras operación de compresión/modificación."""
    current_zip_size: int
    bytes_to_remove: int
    bytes_to_add: int
    compression_ratio: float = 0.5  # Ratio conservador por defecto (50%)
    estimated_final_size: int = field(init=False, default=0)

    def __post_init__(self):
        # Estimación: tamaño actual - eliminados + añadidos * ratio_compresión
        # Asumimos que lo añadido se comprime, lo eliminado libera espacio raw
        raw_added = self.bytes_to_add * self.compression_ratio
        self.estimated_final_size = max(
            0,
            self.current_zip_size - self.bytes_to_remove + int(raw_added)
        )

    @property
    def space_needed_for_rewrite(self) -> int:
        """
        Espacio necesario durante la reescritura (In-Place Seguro):
        Necesitamos el archivo original + el archivo temporal nuevo simultáneamente.
        """
        return self.current_zip_size + self.estimated_final_size


@dataclass
class StorageVerdict:
    """Resultado de la decisión de almacenamiento."""
    verdict: VerdictType
    source_disk: DiskInfo
    workspace_disk: Optional[DiskInfo] = None
    estimate: Optional[CompressionEstimate] = None
    message: str = ""
    requires_user_confirmation: bool = False
    sevenzip_workdir_param: Optional[str] = None  # Parámetro -w para 7z

    def to_json(self) -> str:
        """Serializa a JSON para la GUI."""
        data = {
            "verdict": self.verdict.value,
            "source_disk": asdict(self.source_disk),
            "estimate": asdict(self.estimate) if self.estimate else None,
            "message": self.message,
            "requires_user_confirmation": self.requires_user_confirmation,
            "sevenzip_workdir_param": self.sevenzip_workdir_param,
        }
        if self.workspace_disk:
            data["workspace_disk"] = asdict(self.workspace_disk)
        return json.dumps(data, indent=2)


class TelemetrySignal(GObject.Object):
    """
    Señales de telemetría para el widget de gráficas (SteamGraphWidget).
    Se emiten desde el hilo worker y se reciben en el hilo principal GTK.
    """
    __gsignals__ = {
        'speed-update': (GObject.SignalFlags.RUN_FIRST, None,
                         (float, float, float, str)),  # read_mbps, write_mbps, compress_mbps, current_file
        'progress-update': (GObject.SignalFlags.RUN_FIRST, None,
                            (float, float, int)),  # processed_bytes, total_bytes, eta_seconds
        'operation-complete': (GObject.SignalFlags.RUN_FIRST, None,
                               (bool, str)),  # success, message
        'operation-error': (GObject.SignalFlags.RUN_FIRST, None,
                            (str,)),  # error_message
    }

    def __init__(self):
        super().__init__()
        self._current_file = ""

    def emit_speeds(self, read_mbps: float, write_mbps: float, compress_mbps: float):
        """Emitir velocidades actuales (llamar desde hilo worker via GLib.idle_add)."""
        GLib.idle_add(self.emit, 'speed-update', read_mbps, write_mbps, compress_mbps, self._current_file)

    def emit_progress(self, processed: int, total: int, eta: int):
        """Emitir progreso y ETA."""
        GLib.idle_add(self.emit, 'progress-update', float(processed), float(total), eta)

    def emit_complete(self, success: bool, msg: str):
        GLib.idle_add(self.emit, 'operation-complete', success, msg)

    def emit_error(self, msg: str):
        GLib.idle_add(self.emit, 'operation-error', msg)

    def set_current_file(self, filename: str):
        self._current_file = filename


class StorageDecisionEngine(GObject.Object):
    """
    Motor principal de decisiones de almacenamiento.
    
    Flujo:
    1. scan_disks() -> detecta discos y espacio libre
    2. evaluate_operation(zip_path, bytes_to_remove, bytes_to_add) -> calcula viabilidad
    3. decide_workspace() -> determina dónde hacer la caché temporal
    4. Retorna StorageVerdict para la GUI
    
    Durante la operación real (7z/tar/pixz), emite telemetría vía TelemetrySignal.
    """

    # Rutas típicas de montaje externo en Linux
    EXTERNAL_MOUNT_PATHS = [
        "/mnt",
        "/media",
        "/run/media",
        "/mnt/external",
    ]

    def __init__(self):
        super().__init__()
        self.telemetry = TelemetrySignal()
        self._disks_cache: List[DiskInfo] = []
        self._scan_callbacks: List[Callable] = []

    # ============================================================
    # 1. MONITOREO DE DISCOS
    # ============================================================
    
    def scan_disks(self, force_refresh: bool = False) -> List[DiskInfo]:
        """
        Escanea discos montados y detecta unidades externas.
        Usa `findmnt` y `/proc/mounts` para info precisa en Linux.
        """
        if self._disks_cache and not force_refresh:
            return self._disks_cache

        disks = []
        seen_mounts = set()

        # 1. Leer /proc/mounts para todos los mounts
        try:
            with open("/proc/mounts", "r") as f:
                for line in f:
                    parts = line.strip().split()
                    if len(parts) < 3:
                        continue
                    device, mount_point, fstype = parts[0], parts[1], parts[2]
                    
                    # Filtrar: solo filesystems reales, sin virtuales
                    if fstype in ("proc", "sysfs", "devtmpfs", "devpts", "tmpfs", 
                                  "cgroup", "cgroup2", "pstore", "bpf", "tracefs",
                                  "securityfs", "configfs", "debugfs", "hugetlbfs",
                                  "mqueue", "nsfs", "rpc_pipefs", "autofs"):
                        continue
                    
                    # Ignorar mounts duplicados (bind mounts)
                    if mount_point in seen_mounts:
                        continue
                    seen_mounts.add(mount_point)

                    # Obtener espacio libre
                    try:
                        usage = shutil.disk_usage(mount_point)
                        disk = DiskInfo(
                            mount_point=mount_point,
                            label=self._get_disk_label(device, mount_point),
                            total_bytes=usage.total,
                            free_bytes=usage.free,
                            used_bytes=usage.used,
                            is_removable=self._is_removable(device, mount_point),
                            is_system=mount_point in ("/", "/boot", "/boot/efi", "/efi"),
                            filesystem=fstype,
                        )
                        disks.append(disk)
                    except (OSError, PermissionError):
                        continue
        except Exception as e:
            print(f"[StorageEngine] Error leyendo /proc/mounts: {e}")

        # 2. Asegurar que incluimos mounts externos típicos aunque no estén en /proc/mounts
        for base_path in self.EXTERNAL_MOUNT_PATHS:
            if os.path.isdir(base_path):
                for entry in os.listdir(base_path):
                    full_path = os.path.join(base_path, entry)
                    if os.path.ismount(full_path) and full_path not in seen_mounts:
                        try:
                            usage = shutil.disk_usage(full_path)
                            disk = DiskInfo(
                                mount_point=full_path,
                                label=entry,
                                total_bytes=usage.total,
                                free_bytes=usage.free,
                                used_bytes=usage.used,
                                is_removable=True,
                                is_system=False,
                            )
                            disks.append(disk)
                            seen_mounts.add(full_path)
                        except (OSError, PermissionError):
                            continue

        self._disks_cache = disks
        return disks

    def _get_disk_label(self, device: str, mount_point: str) -> str:
        """Obtiene etiqueta legible del disco (lsblk, udev, o fallback)."""
        # Intentar lsblk para label
        try:
            result = subprocess.run(
                ["lsblk", "-no", "LABEL", device], 
                capture_output=True, text=True, timeout=2
            )
            label = result.stdout.strip()
            if label:
                return label
        except Exception:
            pass
        
        # Fallback: nombre del mount point
        name = os.path.basename(mount_point.rstrip("/"))
        return name if name else "Disco raíz"

    def _is_removable(self, device: str, mount_point: str) -> bool:
        """Detecta si el disco es extraíble (USB, SD, etc.)."""
        # Verificar en /sys/block
        try:
            # device suele ser /dev/sdX o /dev/nvmeXnY
            block_name = os.path.basename(device)
            if block_name.startswith("sd") or block_name.startswith("mmcblk"):
                removable_path = f"/sys/block/{block_name}/removable"
                if os.path.exists(removable_path):
                    with open(removable_path, "r") as f:
                        return f.read().strip() == "1"
        except Exception:
            pass
        # Heurística: montado en /media, /run/media, /mnt = probablemente extraíble
        return any(mount_point.startswith(p) for p in ["/media", "/run/media", "/mnt"])

    def get_disk_for_path(self, path: str) -> Optional[DiskInfo]:
        """Encuentra el DiskInfo que contiene la ruta dada."""
        path = os.path.abspath(path)
        disks = self.scan_disks()
        
        # Buscar el mount point más específico que contenga la ruta
        best_match = None
        best_len = -1
        for disk in disks:
            if path.startswith(disk.mount_point.rstrip("/") + "/") or path == disk.mount_point.rstrip("/"):
                if len(disk.mount_point) > best_len:
                    best_match = disk
                    best_len = len(disk.mount_point)
        return best_match

    # ============================================================
    # 2. CÁLCULO DE VIABILIDAD
    # ============================================================
    
    def calculate_estimate(
        self, 
        current_zip_size: int, 
        bytes_to_remove: int, 
        bytes_to_add: int,
        compression_ratio: float = 0.5
    ) -> CompressionEstimate:
        """
        Calcula el tamaño estimado final y espacio necesario para reescritura segura.
        
        Args:
            current_zip_size: Tamaño actual del archivo .zip/.7z/.tar.gz
            bytes_to_remove: Bytes que se eliminarán del archivo (descomprimidos)
            bytes_to_add: Bytes que se añadirán al archivo (descomprimidos)
            compression_ratio: Ratio de compresión esperado (0.1-1.0, default 0.5)
        
        Returns:
            CompressionEstimate con estimated_final_size y space_needed_for_rewrite
        """
        return CompressionEstimate(
            current_zip_size=current_zip_size,
            bytes_to_remove=bytes_to_remove,
            bytes_to_add=bytes_to_add,
            compression_ratio=compression_ratio,
        )

    # ============================================================
    # 3. ENRUTAMIENTO INTELIGENTE DE CACHÉ
    # ============================================================
    
    def decide_workspace(
        self, 
        source_path: str, 
        estimate: CompressionEstimate,
        safety_margin_bytes: int = 512 * 1024 * 1024  # 512 MB margen
    ) -> StorageVerdict:
        """
        Decide dónde realizar la operación de reescritura.
        
        Lógica:
        1. Identifica disco origen (donde está el archivo)
        2. Verifica si hay espacio: free > space_needed_for_rewrite + margin
        3. Si NO hay espacio, busca disco externo con espacio suficiente
        4. Retorna veredicto con parámetro -w para 7z si usa disco externo
        """
        source_disk = self.get_disk_for_path(source_path)
        if not source_disk:
            # Fallback: disco raíz
            source_disk = next((d for d in self.scan_disks() if d.mount_point == "/"), None)
            if not source_disk:
                return StorageVerdict(
                    verdict=VerdictType.CRITICAL_ERROR,
                    source_disk=DiskInfo("/", "Desconocido", 0, 0, 0),
                    message="No se pudo determinar el disco de origen",
                )

        space_needed = estimate.space_needed_for_rewrite + safety_margin_bytes
        free_space = source_disk.free_bytes

        # Caso 1: Espacio suficiente en disco origen
        if free_space >= space_needed:
            return StorageVerdict(
                verdict=VerdictType.INTERNAL_OK,
                source_disk=source_disk,
                estimate=estimate,
                message=(
                    f"Espacio suficiente en '{source_disk.label}' "
                    f"({free_space / (1024**3):.1f} GB libres / "
                    f"{space_needed / (1024**3):.1f} GB requeridos)"
                ),
                requires_user_confirmation=False,
                sevenzip_workdir_param=None,  # Usa temp por defecto
            )

        # Caso 2: Buscar disco externo con espacio
        external_disks = [
            d for d in self.scan_disks() 
            if d.mount_point != source_disk.mount_point 
            and d.free_bytes >= space_needed
            and not d.is_system
        ]
        
        # Ordenar por: extraíble primero, luego más espacio libre
        external_disks.sort(key=lambda d: (not d.is_removable, -d.free_bytes))
        
        if external_disks:
            workspace = external_disks[0]
            workdir = os.path.join(workspace.mount_point, ".triplewrapper_cache")
            
            return StorageVerdict(
                verdict=VerdictType.EXTERNAL_REQUIRED,
                source_disk=source_disk,
                workspace_disk=workspace,
                estimate=estimate,
                message=(
                    f"⚠ Espacio insuficiente en '{source_disk.label}' "
                    f"({free_space / (1024**3):.1f} GB libres, "
                    f"se necesitan {space_needed / (1024**3):.1f} GB).\n"
                    f"✓ Usando caché en '{workspace.label}' "
                    f"({workspace.free_gb:.1f} GB libres en {workspace.mount_point})"
                ),
                requires_user_confirmation=True,
                sevenzip_workdir_param=f"-w{workdir}",
            )

        # Caso 3: Sin espacio en ningún lado
        all_disks = self.scan_disks()
        max_free = max((d.free_bytes for d in all_disks), default=0)
        
        return StorageVerdict(
            verdict=VerdictType.CRITICAL_ERROR,
            source_disk=source_disk,
            estimate=estimate,
            message=(
                f"❌ ALMACENAMIENTO INSUFICIENTE EN TODO EL SISTEMA\n"
                f"Disco origen '{source_disk.label}': {free_space / (1024**3):.1f} GB libres\n"
                f"Mejor disco disponible: {max_free / (1024**3):.1f} GB libres\n"
                f"Se requieren: {space_needed / (1024**3):.1f} GB para operación segura"
            ),
            requires_user_confirmation=False,
            sevenzip_workdir_param=None,
        )

    # ============================================================
    # 4. EJECUCIÓN CON TELEMETRÍA (para integración real)
    # ============================================================
    
    def execute_with_telemetry(
        self,
        verdict: StorageVerdict,
        archive_path: str,
        operation: str,  # "extract", "modify", "clean"
        files_to_process: List[str] = None,
        progress_callback: Callable = None,
    ) -> bool:
        """
        Ejecuta la operación real (7z/tar) emitiendo telemetría para la gráfica Steam.
        
        NOTA: Esta es una implementación de referencia. En producción:
        - Usar subprocess con pipes para leer stdout/stderr de 7z
        - Parsear líneas de progreso de 7z (ej: "Compressing file.txt 45%")
        - Calcular MB/s leyendo /proc/<pid>/io o usando psutil.Process.io_counters()
        - Emitir señales via self.telemetry.emit_speeds()
        
        Args:
            verdict: StorageVerdict del decide_workspace()
            archive_path: Ruta al archivo comprimido
            operation: Tipo de operación
            files_to_process: Lista de archivos internos a procesar
            progress_callback: Callback opcional (procesado, total, archivo_actual)
        
        Returns:
            True si éxito, False si error
        """
        import time
        import threading
        
        def worker():
            try:
                # Determinar directorio de trabajo
                workdir = None
                if verdict.sevenzip_workdir_param:
                    workdir = verdict.sevenzip_workdir_param[2:]  # Quitar "-w"
                    os.makedirs(workdir, exist_ok=True)
                
                # Construir comando 7z
                cmd = ["7z"]
                if workdir:
                    cmd.append(f"-w{workdir}")
                
                if operation == "extract":
                    cmd.extend(["x", archive_path, "-y"])
                elif operation == "modify":
                    # Para modificar: borrar + añadir (7z d + 7z a)
                    pass
                elif operation == "clean":
                    cmd.extend(["d", archive_path] + (files_to_process or []))
                
                # Simulación de telemetría para demo
                # En producción: subprocess.Popen con stdout/stderr pipes
                self._simulate_telemetry(workdir, files_to_process)
                
                self.telemetry.emit_complete(True, "Operación completada")
                return True
                
            except Exception as e:
                self.telemetry.emit_error(str(e))
                return False
        
        # Ejecutar en hilo separado para no bloquear GTK
        thread = threading.Thread(target=worker, daemon=True)
        thread.start()
        return True

    def _simulate_telemetry(self, workdir: str, files: List[str] = None):
        """
        Simula telemetría realista para testing del widget de gráficas.
        En producción, reemplazar con parsing real de 7z + /proc/pid/io.
        """
        import random
        import math
        
        files = files or [f"pakchunk{i}.pak" for i in range(10)]
        total_size = sum(random.randint(100_000_000, 3_000_000_000) for _ in files)
        processed = 0
        start_time = time.time()
        
        for i, fname in enumerate(files):
            file_size = random.randint(100_000_000, 3_000_000_000)
            self.telemetry.set_current_file(fname)
            
            # Simular procesamiento por chunks
            chunks = max(1, file_size // (64 * 1024 * 1024))  # chunks de 64MB
            for chunk in range(chunks):
                # Velocidades variables (simulan compresión, I/O, CPU)
                elapsed = time.time() - start_time
                phase = elapsed * 0.8
                
                read_speed = 80 + 60 * math.sin(phase) + random.uniform(-20, 20)
                write_speed = 60 + 40 * math.sin(phase + 1.5) + random.uniform(-15, 15)
                compress_speed = 30 + 25 * math.sin(phase + 3) + random.uniform(-8, 8)
                
                read_speed = max(0, read_speed)
                write_speed = max(0, write_speed)
                compress_speed = max(0, compress_speed)
                
                self.telemetry.emit_speeds(read_speed, write_speed, compress_speed)
                
                chunk_bytes = file_size // chunks
                processed += chunk_bytes
                eta = int((total_size - processed) / max(write_speed * 1_000_000, 0.1))
                self.telemetry.emit_progress(processed, total_size, eta)
                
                time.sleep(0.05)  # 20 updates/sec
        
        self.telemetry.emit_complete(True, "Completado")


# ============================================================
# FUNCIONES DE CONVENIENCIA PARA LA GUI
# ============================================================

def quick_verdict(archive_path: str, bytes_to_remove: int, bytes_to_add: int) -> StorageVerdict:
    """
    Función de conveniencia: escanea, calcula y decide en una llamada.
    Útil para botón 'Analizar' en la GUI.
    """
    engine = StorageDecisionEngine()
    current_size = os.path.getsize(archive_path) if os.path.exists(archive_path) else 0
    estimate = engine.calculate_estimate(current_size, bytes_to_remove, bytes_to_add)
    return engine.decide_workspace(archive_path, estimate)


def format_bytes(bytes_val: int) -> str:
    """Formatea bytes a string legible."""
    for unit in ['B', 'KB', 'MB', 'GB', 'TB']:
        if bytes_val < 1024:
            return f"{bytes_val:.1f} {unit}"
        bytes_val /= 1024
    return f"{bytes_val:.1f} PB"


# ============================================================
# DEMO / TESTING
# ============================================================

if __name__ == "__main__":
    print("=" * 60)
    print("TripleWrapper - Storage Decision Engine Demo")
    print("=" * 60)
    
    engine = StorageDecisionEngine()
    
    # 1. Escanear discos
    print("\n📊 ESCANEO DE DISCOS:")
    disks = engine.scan_disks()
    for d in disks:
        icon = "💿" if d.is_system else ("🔌" if d.is_removable else "💾")
        print(f"  {icon} {d.label:20s} | {d.mount_point:25s} | "
              f"Libre: {d.free_gb:6.1f} GB / {d.total_gb:6.1f} GB "
              f"({d.usage_percent:.1f}% usado) {'[SISTEMA]' if d.is_system else ''}")
    
    # 2. Simular decisión para un archivo grande
    print("\n🧮 SIMULACIÓN DE DECISIÓN:")
    test_archive = "/home/user/juegos/juego_gigante.pak"  # Ejemplo
    test_archive = "/tmp/test_archive.zip"  # Usar ruta real para test
    
    # Crear archivo de prueba si no existe
    if not os.path.exists(test_archive):
        with open(test_archive, "wb") as f:
            f.write(b"x" * (2 * 1024**3))  # 2 GB fake
    
    estimate = engine.calculate_estimate(
        current_zip_size=2 * 1024**3,      # 2 GB actual
        bytes_to_remove=500 * 1024**2,     # Eliminar 500 MB
        bytes_to_add=1.5 * 1024**3,        # Añadir 1.5 GB
        compression_ratio=0.45
    )
    
    print(f"\n  Archivo actual:     {format_bytes(estimate.current_zip_size)}")
    print(f"  A eliminar:         {format_bytes(estimate.bytes_to_remove)}")
    print(f"  A añadir (raw):     {format_bytes(estimate.bytes_to_add)}")
    print(f"  Ratio compresión:   {estimate.compression_ratio:.0%}")
    print(f"  ───")
    print(f"  Tamaño estimado:    {format_bytes(estimate.estimated_final_size)}")
    print(f"  Espacio reescritura: {format_bytes(estimate.space_needed_for_rewrite)}")
    
    verdict = engine.decide_workspace(test_archive, estimate)
    
    print(f"\n📋 VEREDICTO: {verdict.verdict.value.upper()}")
    print(f"  Disco origen:    {verdict.source_disk.label} ({verdict.source_disk.mount_point})")
    if verdict.workspace_disk:
        print(f"  Disco caché:     {verdict.workspace_disk.label} ({verdict.workspace_disk.mount_point})")
        print(f"  Parámetro 7z:    {verdict.sevenzip_workdir_param}")
    print(f"  Mensaje:         {verdict.message}")
    print(f"  Confirmación UI: {verdict.requires_user_confirmation}")
    
    print("\n📄 JSON PARA GUI:")
    print(verdict.to_json())
    
    # 3. Test telemetría (opcional - requiere GTK main loop)
    print("\n📡 Telemetría disponible via engine.telemetry.connect('speed-update', callback)")
    print("   Señales: speed-update, progress-update, operation-complete, operation-error")