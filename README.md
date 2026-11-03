# CleanShini

Herramienta para limpiar archivos temporales y residuos de la PC, con **control total** sobre qué se borra.

- **Motor** en Rust (`cleanshini-core`): escaneo por categorías, reglas de exclusión, dry-run, papelera de reciclaje por defecto.
- Un solo ejecutable `cleanshini.exe` que incluye CLI y GUI (`--gui` o sin argumentos abre la interfaz gráfica).

## Categorías de limpieza

- `temp`: archivos temporales del sistema y `.tmp`/`.crdownload`.
- `cache`: cachés de apps/navegadores, `Thumbs.db`, `desktop.ini`.
- `residue`: residuos de desinstalación.
- `log`: archivos `.log`/`.old`/`.bak`/`.dmp` y carpetas de logs.
- `oldfile`: archivos sin modificar hace N días (configurable).
- `large`: archivos que pesan gigas (umbral configurable, por defecto 1 GB), buscados en carpetas del usuario.
- `recyclebin`: vaciar la papelera de reciclaje del sistema.
- `winupdate`: caché de Windows Update.
- `windowsold`: instalación anterior de Windows (Windows.old).
- `crashdump`: volcados de memoria y reportes WER.
- `appcache`: cachés de apps (Spotify, Discord, Teams, Slack, etc.).
- `devcache`: cachés de desarrollo (npm, cargo, gradle, nuget, etc.).
- `aimodel`: modelos de IA local (Ollama, HuggingFace, LM Studio, GPT4All, ComfyUI, Stable Diffusion WebUI, KoboldCpp, text-generation-webui, archivos .gguf/.safetensors/.onnx, etc.).
- `virtualdisk`: discos virtuales de Docker/WSL/máquinas virtuales (.vhdx, .vmdk, .qcow2).

## Uso CLI

```bash
# Escanear y generar plan
cleanshini scan
cleanshini scan --categories temp,cache --min-age 30 --min-size 1024

# Ver plan
cleanshini plan

# Limpiar (con confirmación)
cleanshini clean
cleanshini clean --permanent -y          # borrado permanente sin confirmación
cleanshini clean --dry-run                # solo muestra
cleanshini clean --categories temp

# Reglas
cleanshini rules show
cleanshini rules add-exclude "C:\ruta\importante"
cleanshini rules set-min-age 14
cleanshini rules enable-category cache

# Auditoría
cleanshini audit --limit 50
cleanshini config show
```

## Limpieza del sistema (libera más espacio)

En la GUI (pestaña "Escanear") o por CLI:
- `cleanshini empty-recycle-bin`
- `cleanshini system-cleanup --all` (DISM + cleanmgr + hibernación off + shadow copies)
- `cleanshini system-cleanup --dism --hibernation`

## Control total

- **Dry-run** antes de borrar.
- **Revisión por archivo** (checkbox) en la GUI.
- **Whitelist/blacklist** de rutas y extensiones (`rules`).
- **Umbrales** de edad y tamaño.
- **Papelera por defecto**, borrado permanente solo con flag explícito.
- **Auditoría** JSONL de cada operación (ruta, tamaño, modo, resultado).

## Compilación

```bash
cargo build --release
# binarios en target/release/cleanshini.exe y target/release/cleanshini-gui.exe
```

## Portabilidad (Linux / macOS)

El **núcleo** (`cleanshini-core`) compila en Linux y macOS para el escaneo de archivos por categorías, reglas, dry-run y auditoría. Las funciones específicas de Windows están detrás de `#[cfg(windows)]`:

- Papelera de reciclaje (`empty_recycle_bin`) — en Linux/macOS se puede adaptar a `gio trash` / `trash` crate (ya multiplataforma en parte).
- DISM, cleanmgr, powercfg, vssadmin — solo Windows.
- Rutas por defecto (`%TEMP%`, `%WINDIR%`, `%LOCALAPPDATA%`) — en Linux/macOS habría que mapear a `~/.cache`, `/tmp`, `/var/log`, etc.

La **GUI** (egui) es multiplataforma. Para portar a Linux/macOS bastaría con:
1. Mapear las rutas por defecto por SO.
2. Adaptar o deshabilitar las tareas de sistema específicas de Windows.
3. Compilar con `cargo build --target ...`.

Estado actual: **Windows-first**, Linux/macOS parcial (escaneo de archivos funciona, tareas de sistema no).

## Auto-configuración por SO

La app detecta el sistema operativo en tiempo de compilación (`#[cfg(target_os = ...)]`) y se auto-configura:

- **Rutas por defecto**: `%TEMP%`, `%WINDIR%` en Windows; `/tmp`, `~/.cache`, `~/.local/share/Trash` en Linux; `~/Library/Caches`, `~/.Trash` en macOS.
- **Tareas de sistema**: DISM/cleanmgr/powercfg en Windows; apt/journalctl en Linux; brew/periodic en macOS. Solo se muestran las disponibles para el SO.
- **Papelera**: `SHEmptyRecycleBinW` en Windows; `gio trash --empty`/`trash-empty` en Linux; vaciado de `~/.Trash` en macOS.

El mismo código compila y funciona en los tres sistemas sin cambios manuales.

## Arquitectura

```
crates/
  cleanshini-core/   # motor: model, config, scanner, executor, audit, system
  cleanshini/        # binario unificado: CLI (clap) + GUI (egui) + i18n
```

- `model.rs`: tipos (Category, RuleSet, Plan, ScanItem, ExecutionReport).
- `config.rs`: carga/guardado de reglas (con merge de defaults).
- `scanner.rs`: escaneo por categorías + pase profundo de modelos IA.
- `executor.rs`: ejecución del plan (papelera o permanente).
- `audit.rs`: log JSONL.
- `system.rs`: papelera, DISM, cleanmgr, hibernación, shadow copies.
- `i18n.rs`: traducciones (ES/EN).

## Seguridad

- Por defecto los archivos van a la Papelera de reciclaje (recuperables).
- El borrado permanente requiere `--permanent` o la opción en la GUI.
- Las rutas excluidas nunca se tocan.
- Se registra cada borrado en el log de auditoría.
