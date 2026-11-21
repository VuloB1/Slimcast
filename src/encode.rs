//! Detección de dispositivos y compresión con ffmpeg (port de comprimir.py).
use std::io::{BufRead, BufReader};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

const NO_WINDOW: u32 = 0x0800_0000; // sin consolas negras al lanzar ffmpeg
pub const EXTS: &[&str] = &["mp4", "mkv", "flv", "ts", "mov", "webm", "avi", "m4v"];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Encoder {
    Nvenc(u32), // índice entre las NVIDIA
    Amf,
    Qsv,
    Cpu,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub name: String,
    pub encoder: Encoder,
    pub ok: bool,
}

fn cmd(program: &str) -> Command {
    // el instalador deja ffmpeg junto al .exe; en desarrollo se usa el del PATH
    let bundled = std::env::current_exe().map(|e| e.with_file_name(format!("{program}.exe")));
    let mut c = match bundled {
        Ok(p) if p.exists() => Command::new(p),
        _ => Command::new(program),
    };
    c.creation_flags(NO_WINDOW);
    c
}

/// GPUs físicas (DXGI) + CPU, cada una probada con un fotograma real en ffmpeg.
pub fn devices() -> Vec<Device> {
    use windows::Win32::Graphics::Dxgi::*;
    let mut found = Vec::new();
    let mut nvidia = 0;
    unsafe {
        if let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() {
            let mut i = 0;
            while let Ok(adapter) = factory.EnumAdapters1(i) {
                i += 1;
                let Ok(d) = adapter.GetDesc1() else { continue };
                let len = d.Description.iter().position(|&c| c == 0).unwrap_or(128);
                let name = String::from_utf16_lossy(&d.Description[..len]);
                // shortcut: con varias AMD/Intel ffmpeg usa la principal; elegir tarjeta exacta si alguien tiene dos
                let encoder = match d.VendorId {
                    0x10DE => { nvidia += 1; Encoder::Nvenc(nvidia - 1) }
                    0x1002 => Encoder::Amf,
                    0x8086 => Encoder::Qsv,
                    _ => continue, // pantallas virtuales (Parsec, etc.) y el renderizador por software
                };
                if !found.iter().any(|f: &Device| f.name == name) {
                    found.push(Device { name, encoder, ok: false });
                }
            }
        }
    }
    found.push(Device { name: "Procesador (CPU)".into(), encoder: Encoder::Cpu, ok: false });
    let handles: Vec<_> = found.iter().map(|d| {
        let e = d.encoder;
        std::thread::spawn(move || works(e))
    }).collect();
    for (d, h) in found.iter_mut().zip(handles) {
        d.ok = h.join().unwrap_or(false);
    }
    found
}

fn works(e: Encoder) -> bool {
    cmd("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=black:s=256x256:d=0.1", "-frames:v", "1"])
        .args(video_args(e, 28))
        .args(["-f", "null", "-"])
        .stdout(Stdio::null()).stderr(Stdio::null())
        .status().is_ok_and(|s| s.success())
}

/// `calidad` en escala CRF (24 máxima calidad, 28 equilibrado, 32 más pequeño).
fn video_args(e: Encoder, calidad: u32) -> Vec<String> {
    let q = calidad.to_string();
    let amf_qp = (calidad - 5).to_string(); // medido en un directo real: AMF QP 23 ≈ NVENC CQ 28 (SSIM 0.991, tamaño parecido)
    let v: Vec<&str> = match e {
        Encoder::Nvenc(_) => vec!["-c:v", "hevc_nvenc", "-preset", "p7", "-rc", "vbr", "-cq", &q, "-b:v", "0"],
        Encoder::Amf => vec!["-c:v", "hevc_amf", "-quality", "quality", "-rc", "cqp", "-qp_i", &amf_qp, "-qp_p", &amf_qp],
        // shortcut: QSV sin probar (no hay Intel aquí); ajustar si alguien lo usa
        Encoder::Qsv => vec!["-c:v", "hevc_qsv", "-global_quality", &q, "-preset", "slower"],
        Encoder::Cpu => vec!["-c:v", "libx265", "-crf", &q, "-preset", "slow"],
    };
    let mut v: Vec<String> = v.into_iter().map(String::from).collect();
    if let Encoder::Nvenc(i) = e {
        v.extend(["-gpu".into(), i.to_string()]);
    }
    v
}

pub struct Probe {
    pub duration: f64,
    pub fps: f64,
    pub bitrate: u64,
}

pub fn probe(path: &Path) -> Option<Probe> {
    let out = cmd("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0", "-show_entries",
               "stream=avg_frame_rate,bit_rate:format=duration,bit_rate", "-of", "default=nw=1"])
        .arg(path).output().ok()?;
    let (mut duration, mut fps, mut bitrate) = (0.0, 0.0, 0);
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        match line.split_once('=') {
            Some(("duration", v)) => duration = v.parse().unwrap_or(0.0),
            Some(("avg_frame_rate", v)) => {
                if let Some((n, d)) = v.split_once('/') {
                    let (n, d): (f64, f64) = (n.parse().unwrap_or(0.0), d.parse().unwrap_or(0.0));
                    if d > 0.0 { fps = n / d }
                }
            }
            // el del stream viene antes que el del contenedor; nos quedamos con el primero válido
            Some(("bit_rate", v)) if bitrate == 0 => bitrate = v.parse().unwrap_or(0),
            _ => {}
        }
    }
    (duration > 0.0).then_some(Probe { duration, fps, bitrate })
}

/// Guarda en `out` un fotograma pequeño (PNG de 128 px de ancho) tomado al 10 % del video.
pub fn thumbnail(src: &Path, out: &Path) -> bool {
    let at = probe(src).map_or(0.0, |p| p.duration * 0.1);
    cmd("ffmpeg").args(["-v", "error", "-y", "-ss", &format!("{at:.1}")]).arg("-i").arg(src)
        .args(["-frames:v", "1", "-vf", "scale=128:-2"]).arg(out)
        .status().is_ok_and(|s| s.success()) && out.exists()
}

/// (total, libre) en bytes de la unidad que contiene `dir`.
pub fn disk(dir: &Path) -> (u64, u64) {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::HSTRING;
    let (mut total, mut free) = (0u64, 0u64);
    unsafe { let _ = GetDiskFreeSpaceExW(&HSTRING::from(dir.as_os_str()), Some(&mut free), Some(&mut total), None); }
    (total, free)
}

pub fn find_videos(paths: &[PathBuf]) -> Vec<PathBuf> {
    let is_video = |p: &Path| p.is_file() && p.extension().and_then(|e| e.to_str())
        .is_some_and(|e| EXTS.contains(&e.to_ascii_lowercase().as_str()));
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let mut v: Vec<_> = std::fs::read_dir(p).into_iter().flatten().flatten()
                .map(|e| e.path()).filter(|p| is_video(p)).collect();
            v.sort();
            out.extend(v);
        } else if is_video(p) {
            out.push(p.clone());
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Done(u64), // bytes del comprimido
    Skipped(String),
    Failed(String),
    Cancelled,
}

pub struct Job {
    pub encoder: Encoder,
    pub calidad: u32,
    pub cap30: bool,
}

/// Comprime un video. Nunca toca el original; escribe en `comprimidos\` a través de un `.part.mp4`.
pub fn compress(src: &Path, job: &Job, stop: &AtomicBool, progress: impl Fn(f32)) -> Outcome {
    let outdir = src.parent().unwrap_or(Path::new(".")).join("comprimidos");
    let stem = src.file_stem().unwrap_or_default().to_string_lossy();
    let dst = outdir.join(format!("{stem}_hevc.mp4"));
    let part = outdir.join(format!("{stem}_hevc.part.mp4")); // si se corta no queda un "terminado" falso
    if dst.exists() {
        return Outcome::Skipped("Ya se comprimió antes".into());
    }
    let size = std::fs::metadata(src).map(|m| m.len()).unwrap_or(0);
    // el tope de bitrate deja la salida en <=60% del original (+ audio): si no cabe, ni empezar
    if (disk(src.parent().unwrap_or(Path::new("."))).1 as f64) < size as f64 * 0.65 {
        return Outcome::Skipped("Falta espacio en el disco".into());
    }
    let Some(info) = probe(src) else { return Outcome::Failed("No se pudo leer el video".into()) };
    if std::fs::create_dir_all(&outdir).is_err() {
        return Outcome::Failed("No se pudo crear la carpeta comprimidos".into());
    }

    let mut c = cmd("ffmpeg");
    c.args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-nostats", "-progress", "pipe:1", "-i"])
        .arg(src)
        .args(["-map", "0:v:0", "-map", "0:a?", "-c:a", "copy"])
        .args(video_args(job.encoder, job.calidad));
    if info.bitrate > 0 && job.encoder != Encoder::Amf {
        // tope: como mucho 60% del bitrate original, así nunca sale más grande (AMF en CQP no lo admite)
        let cap = info.bitrate * 6 / 10;
        c.args(["-maxrate", &cap.to_string(), "-bufsize", &(cap * 2).to_string()]);
    }
    if job.cap30 && info.fps > 30.0 {
        c.args(["-vf", "fps=30"]);
    }
    c.args(["-pix_fmt", "yuv420p", "-tag:v", "hvc1", "-movflags", "+faststart", "-f", "mp4"]).arg(&part);
    let Ok(mut child) = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() else {
        return Outcome::Failed("No se encuentra ffmpeg".into());
    };

    let stderr = child.stderr.take().unwrap();
    let errors = std::thread::spawn(move || {
        BufReader::new(stderr).lines().map_while(Result::ok).last().unwrap_or_default()
    });
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            break;
        }
        if let Some(us) = line.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok()) {
            progress((us / 1e6 / info.duration).clamp(0.0, 1.0) as f32);
        }
    }
    let status = child.wait();
    let last_error = errors.join().unwrap_or_default();
    let fail = |msg: String| {
        let _ = std::fs::remove_file(&part);
        msg
    };
    if stop.load(Ordering::Relaxed) {
        fail(String::new());
        return Outcome::Cancelled;
    }
    if !status.is_ok_and(|s| s.success()) {
        return Outcome::Failed(fail(if last_error.is_empty() { "ffmpeg falló".into() } else { last_error }));
    }
    let out_dur = probe(&part).map(|p| p.duration).unwrap_or(0.0);
    if (out_dur - info.duration).abs() > 2f64.max(info.duration * 0.01) {
        return Outcome::Failed(fail(format!("La duración no coincide ({out_dur:.0}s de {:.0}s)", info.duration)));
    }
    let out = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(u64::MAX);
    if out >= size {
        fail(String::new());
        return Outcome::Skipped("Ya está optimizado".into());
    }
    if std::fs::rename(&part, &dst).is_err() {
        return Outcome::Failed(fail("No se pudo renombrar el resultado".into()));
    }
    Outcome::Done(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Requiere ffmpeg y el clip de prueba: COMPRESOR_TEST_CLIP=ruta cargo test -- --nocapture
    #[test]
    fn compresses_clip_on_every_working_device() {
        let Ok(clip) = std::env::var("COMPRESOR_TEST_CLIP") else { return };
        let clip = PathBuf::from(clip);
        let devices = devices();
        println!("{devices:#?}");
        assert!(devices.iter().any(|d| d.encoder == Encoder::Cpu));
        for d in devices.iter().filter(|d| d.ok && d.encoder != Encoder::Cpu) {
            let _ = std::fs::remove_dir_all(clip.parent().unwrap().join("comprimidos"));
            let job = Job { encoder: d.encoder, calidad: 28, cap30: false };
            let r = compress(&clip, &job, &AtomicBool::new(false), |_| {});
            println!("{}: {r:?}", d.name);
            assert!(matches!(r, Outcome::Done(_)), "{}: {r:?}", d.name);
        }
        // cancelar a mitad borra el archivo a medias
        let _ = std::fs::remove_dir_all(clip.parent().unwrap().join("comprimidos"));
        let stop = AtomicBool::new(false);
        let job = Job { encoder: Encoder::Cpu, calidad: 28, cap30: false };
        let r = compress(&clip, &job, &stop, |p| if p > 0.2 { stop.store(true, Ordering::Relaxed) });
        assert_eq!(r, Outcome::Cancelled);
        assert!(std::fs::read_dir(clip.parent().unwrap().join("comprimidos")).unwrap().next().is_none());
    }
}
