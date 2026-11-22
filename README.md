<p align="center"><img src="assets/icon-256.png" width="96" alt=""></p>

<h1 align="center">Slimcast</h1>

<p align="center">Comprime tus grabaciones y directos de OBS a HEVC y recupera espacio en el disco, sin perder calidad visible.</p>

<p align="center"><a href="https://github.com/VuloB1/Slimcast/releases/latest"><b>Descargar para Windows</b></a></p>

![Slimcast](docs/screenshot.png)

## Qué hace

- Convierte tus videos a **HEVC (H.265)** con la gráfica que elijas: NVIDIA (NVENC), AMD (AMF), Intel (Quick Sync, sin probar) o la CPU.
- Detecta las gráficas del equipo y prueba cada una antes de ofrecerla.
- **Nunca toca los originales.** El resultado va a una carpeta `comprimidos` junto al video, y se descarta si no queda más pequeño o si su duración no coincide.
- Antes de empezar comprueba que haya espacio libre en el disco, y muestra cuánto liberarías si borras los originales.
- Copia el audio tal cual (todas las pistas) y, si quieres, baja a 30 fps.
- **Se actualiza sola**: avisa cuando hay una versión nueva y la instala con un clic.
- Es ligera: está hecha en Rust con [GPUI](https://www.gpui.rs/) y usa unos 50 MB de RAM.

Con un directo real de 30,7 GB, el resultado fue de 13,2 GB (−57 %).

## Instalación

Descarga `Slimcast-x.y.z-setup.exe` desde [Releases](https://github.com/VuloB1/Slimcast/releases/latest) y ejecútalo. No necesita permisos de administrador y ya trae ffmpeg incluido.

### Actualizaciones

Al abrirse, Slimcast consulta la última release de este repositorio. Si hay una versión más nueva, aparece un aviso con el botón **Actualizar**: descarga el instalador, actualiza sin preguntar nada y vuelve a abrir el programa. Si estás comprimiendo, espera a que termine.

- Solo instala archivos de las [releases de VuloB1/Slimcast](https://github.com/VuloB1/Slimcast/releases).
- Sin conexión no pasa nada: la app funciona igual.
- Las versiones 1.0.0 y 1.1.0 no tienen actualizador. Si tienes una de ellas, instala la última a mano una vez.

> NVIDIA: probado con el driver 582. Las versiones de ffmpeg posteriores a la 8.0.1 piden el driver 610 o superior, por eso la app usa la 8.0.1.

## Compilar desde el código

Requisitos: [Rust](https://rustup.rs/), [Inno Setup 6](https://jrsoftware.org/isinfo.php) y ffmpeg en el PATH (`winget install Gyan.FFmpeg`).

```powershell
cargo run --release                                          # la app, usando el ffmpeg del PATH
powershell -ExecutionPolicy Bypass -File installer\build.ps1 # genera dist\Slimcast-<versión>-setup.exe
```

Para publicar una versión nueva, sube `version` en `Cargo.toml`, genera el instalador y crea una release con la etiqueta `vX.Y.Z` y el `Slimcast-X.Y.Z-setup.exe` adjunto. Las copias instaladas la detectan al abrirse.

`installer\art.ps1` vuelve a generar el icono y las imágenes del instalador.

También hay un script de línea de comandos más simple, `comprimir.py` (solo NVENC o CPU): arrastra videos encima de `comprimir.bat`.

## Licencia

[GPL-3.0](LICENSE). El instalador incluye [FFmpeg](https://ffmpeg.org/) 8.0.1 (build *full* de [gyan.dev](https://www.gyan.dev/ffmpeg/builds/)), que también se distribuye bajo GPL-3.0. Su código fuente está en [FFmpeg/FFmpeg@n8.0.1](https://github.com/FFmpeg/FFmpeg/tree/n8.0.1), y las bibliotecas que lleva la build se listan en [gyan.dev](https://www.gyan.dev/ffmpeg/builds/).
