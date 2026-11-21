"""Comprime videos / directos de Twitch a H.265 (HEVC) con ffmpeg.

Basado en la skill compress-video de pixel-point (ffmpeg + H.265), adaptado a directos largos:
- Por defecto usa la GPU NVIDIA (hevc_nvenc): horas de directo en tiempo razonable.
- --cpu usa libx265: archivos algo más pequeños, pero mucho más lento.
- Conserva todas las pistas de audio sin recodificar (el audio de Twitch ya es ligero).
- Nunca borra los originales: escribe en una carpeta "comprimidos" junto a cada video.

Uso:
  python comprimir.py VIDEO_O_CARPETA [...] [--calidad 28] [--cpu] [--30fps]
"""
import argparse
import json
import shutil
import subprocess
import sys
from pathlib import Path

NOWIN = getattr(subprocess, "CREATE_NO_WINDOW", 0)  # sin ventanas negras de consola desde el .exe

EXTS = {".mp4", ".mkv", ".flv", ".ts", ".mov", ".webm", ".avi", ".m4v"}


def probe(path):
    out = subprocess.run(
        ["ffprobe", "-v", "error", "-show_format", "-show_streams", "-of", "json", str(path)],
        capture_output=True, text=True, check=True, creationflags=NOWIN,
    ).stdout
    return json.loads(out)


def fps_of(info):
    v = next(s for s in info["streams"] if s["codec_type"] == "video")
    num, den = v.get("avg_frame_rate", "0/1").split("/")
    return float(num) / float(den) if float(den) else 0.0


def video_bitrate(info):
    v = next(s for s in info["streams"] if s["codec_type"] == "video")
    return int(v.get("bit_rate") or info["format"].get("bit_rate") or 0)


def build_cmd(src, dst, calidad, cpu, cap30, fps, src_bps):
    cmd = ["ffmpeg", "-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-nostats",
           "-progress", "pipe:1", "-i", str(src),
           "-map", "0:v:0", "-map", "0:a?", "-c:a", "copy"]
    if cpu:
        cmd += ["-c:v", "libx265", "-crf", str(calidad), "-preset", "slow"]
    else:
        cmd += ["-c:v", "hevc_nvenc", "-preset", "p7", "-rc", "vbr", "-cq", str(calidad), "-b:v", "0"]
    if src_bps:  # tope: como mucho 60% del bitrate original, así nunca sale más grande
        cap = int(src_bps * 0.6)
        cmd += ["-maxrate", str(cap), "-bufsize", str(cap * 2)]
    if cap30 and fps > 30:
        cmd += ["-vf", "fps=30"]
    cmd += ["-pix_fmt", "yuv420p", "-tag:v", "hvc1", "-movflags", "+faststart", "-f", "mp4", str(dst)]
    return cmd


def compress(src, calidad=28, cpu=False, cap30=False, log=print, progress=None, stop=None):
    """Comprime un video. progress(fraccion 0-1); stop: threading.Event para cancelar.
    Devuelve (bytes_original, bytes_comprimido), o (0, 0) si no se generó nada."""
    outdir = src.parent / "comprimidos"
    dst = outdir / f"{src.stem}_hevc.mp4"
    if dst.exists():
        log(f"[saltado] ya existe: {dst.name}")
        return 0, 0
    a = src.stat().st_size
    # el tope de bitrate deja la salida en <=60% del original (+ audio): si no cabe, ni empezar
    if shutil.disk_usage(src.parent).free < a * 0.65:
        log(f"[saltado] {src.name}: no hay espacio libre suficiente en el disco")
        return 0, 0
    outdir.mkdir(exist_ok=True)
    part = dst.with_suffix(".part.mp4")  # si se corta a medias no queda un archivo "terminado" falso

    info = probe(src)
    dur = float(info["format"].get("duration", 0))
    log(f"==> {src.name}  ({dur / 3600:.2f} h)")
    cmd = build_cmd(src, part, calidad, cpu, cap30, fps_of(info), video_bitrate(info))
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True, encoding="utf-8", errors="replace", creationflags=NOWIN)
    errors = []
    for line in proc.stdout:
        if stop is not None and stop.is_set():
            proc.terminate()
            break
        key, sep, val = line.strip().partition("=")
        if key == "out_time_us" and val.isdigit() and dur and progress:
            progress(min(1.0, int(val) / 1e6 / dur))
        elif not sep and key:  # sin "=" -> mensaje de error de ffmpeg
            errors = (errors + [line.strip()])[-5:]
    proc.wait()
    if stop is not None and stop.is_set():
        part.unlink(missing_ok=True)
        log(f"[cancelado] {src.name}")
        return 0, 0
    if proc.returncode != 0:
        part.unlink(missing_ok=True)
        log(f"[error] ffmpeg falló con {src.name} (¿GPU sin NVENC? prueba con CPU)")
        for e in errors:
            log("    " + e)
        return 0, 0

    out_dur = float(probe(part)["format"].get("duration", 0))
    if abs(out_dur - dur) > max(2.0, dur * 0.01):
        part.unlink(missing_ok=True)
        log(f"[error] duración no coincide ({out_dur:.0f}s vs {dur:.0f}s), descartado")
        return 0, 0
    b = part.stat().st_size
    if b >= a:
        part.unlink()
        log("[saltado] el comprimido no es más pequeño, se queda el original")
        return 0, 0
    part.rename(dst)

    log(f"[ok] {a / 1e9:.2f} GB -> {b / 1e9:.2f} GB  (ahorro {100 * (1 - b / a):.0f}%)")
    return a, b


def find_videos(paths):
    files = []
    for r in map(Path, paths):
        if r.is_dir():
            files += sorted(f for f in r.iterdir() if f.is_file() and f.suffix.lower() in EXTS)
        elif r.is_file():
            files.append(r)
    return files


def main():
    p = argparse.ArgumentParser(description="Comprime videos a H.265 sin tocar los originales.")
    p.add_argument("rutas", nargs="+", type=Path, help="videos o carpetas")
    p.add_argument("--calidad", type=int, default=28,
                   help="más alto = más pequeño y peor calidad (24-32 razonable, def. 28)")
    p.add_argument("--cpu", action="store_true", help="usar libx265 (CPU) en vez de la GPU NVIDIA")
    p.add_argument("--30fps", dest="cap30", action="store_true", help="bajar a 30 fps (aún más pequeño)")
    args = p.parse_args()

    files = find_videos(args.rutas)
    if not files:
        sys.exit("No hay videos que comprimir.")

    total_a = total_b = 0
    for f in files:
        a, b = compress(f, args.calidad, args.cpu, args.cap30,
                        progress=lambda x: print(f"\r  {x:6.1%}", end="", flush=True))
        print()
        total_a, total_b = total_a + a, total_b + b
    if total_a:
        print(f"\nTOTAL: {total_a / 1e9:.2f} GB -> {total_b / 1e9:.2f} GB "
              f"(liberarías {(total_a - total_b) / 1e9:.2f} GB borrando los originales)")


if __name__ == "__main__":
    main()
