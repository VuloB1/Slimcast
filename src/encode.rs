//! Detección de dispositivos y compresión con ffmpeg (port de comprimir.py).
use crate::i18n::{t, tf};
use std::io::{BufRead, BufReader};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(windows)]
const NO_WINDOW: u32 = 0x0800_0000; // sin consolas negras al lanzar ffmpeg
pub const EXTS: &[&str] = &["mp4", "mkv", "flv", "ts", "mov", "webm", "avi", "m4v"];

#[derive(Clone, Copy, PartialEq, Debug)]
#[allow(dead_code)] // AMF y QSV solo existen en Windows; VAAPI solo en Linux
pub enum Encoder {
    Nvenc(u32), // índice entre las NVIDIA
    Amf,
    Qsv,
    Vaapi(u32), // número del nodo /dev/dri/renderD*
    Cpu,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub name: String,
    pub encoder: Encoder,
    pub ok: bool,
}

fn cmd(program: &str) -> Command {
    // el instalador (o el AppImage) deja ffmpeg junto al ejecutable; en desarrollo se usa el del PATH
    let bundled = std::env::current_exe().map(|e| e.with_file_name(format!("{program}{}", std::env::consts::EXE_SUFFIX)));
    #[allow(unused_mut)]
    let mut c = match bundled {
        Ok(p) if p.exists() => Command::new(p),
        _ => Command::new(program),
    };
    // ffmpeg escucha el teclado (la tecla q); en Linux, si Slimcast se lanzó en segundo plano desde un terminal,
    // leerlo congela a ffmpeg (SIGTTIN) y la detección o las miniaturas no terminan nunca
    c.stdin(Stdio::null());
    #[cfg(windows)]
    c.creation_flags(NO_WINDOW);
    c
}

/// GPUs físicas + CPU, cada una probada con un fotograma real en ffmpeg.
pub fn devices() -> Vec<Device> {
    let mut found = gpus();
    found.push(Device { name: "CPU".into(), encoder: Encoder::Cpu, ok: false });
    let handles: Vec<_> = found.iter().map(|d| {
        let e = d.encoder;
        std::thread::spawn(move || works(e))
    }).collect();
    for (d, h) in found.iter_mut().zip(handles) {
        d.ok = h.join().unwrap_or(false);
    }
    found
}

#[cfg(not(windows))]
fn gpus() -> Vec<Device> {
    gpus_in(Path::new("/sys/class/drm"), lspci_name)
}

/// Gráficas según el kernel: cada /sys/class/drm/renderD* con el fabricante PCI de su dispositivo.
/// NVIDIA usa NVENC; AMD e Intel, VAAPI sobre su nodo de render.
#[cfg(not(windows))]
fn gpus_in(drm: &Path, name_of: impl Fn(&str) -> Option<String>) -> Vec<Device> {
    let mut nodes: Vec<(u32, PathBuf)> = std::fs::read_dir(drm).into_iter().flatten().flatten()
        .filter_map(|e| Some((e.file_name().to_str()?.strip_prefix("renderD")?.parse().ok()?, e.path())))
        .collect();
    nodes.sort();
    let mut found = Vec::new();
    let mut nvidia = 0;
    for (n, node) in nodes {
        let Ok(vendor) = std::fs::read_to_string(node.join("device/vendor")) else { continue };
        let slot = std::fs::read_link(node.join("device")).ok()
            .and_then(|p| Some(p.file_name()?.to_string_lossy().into_owned())).unwrap_or_default();
        // shortcut: con varias NVIDIA, el orden de /sys puede no coincidir con el de CUDA; revisar si alguien tiene dos
        let (encoder, brand) = match vendor.trim() {
            "0x10de" => { nvidia += 1; (Encoder::Nvenc(nvidia - 1), "NVIDIA") }
            "0x1002" => (Encoder::Vaapi(n), "AMD Radeon"),
            "0x8086" => (Encoder::Vaapi(n), "Intel"),
            _ => continue, // llvmpipe, virtio y demás dispositivos sin codificador
        };
        found.push(Device { name: name_of(&slot).unwrap_or_else(|| brand.into()), encoder, ok: false });
    }
    found
}

/// Modelo de la gráfica según `lspci -mm`, si es inequívoco (en AMD suele listar varios modelos juntos).
#[cfg(not(windows))]
fn lspci_name(slot: &str) -> Option<String> {
    let out = Command::new("lspci").args(["-mm", "-s", slot]).stdin(Stdio::null()).output().ok()?;
    // 01:00.0 "VGA compatible controller" "NVIDIA Corporation" "GP107 [GeForce GTX 1050 Ti]" ...
    let fields: Vec<&str> = std::str::from_utf8(&out.stdout).ok()?.split('"').skip(1).step_by(2).collect();
    model(fields.get(2)?)
}

/// "GP107 [GeForce GTX 1050 Ti]" → "GeForce GTX 1050 Ti"; None si el texto junta varios modelos.
#[cfg(not(windows))]
fn model(device: &str) -> Option<String> {
    let inner = device.split_once('[')?.1.split_once(']')?.0.trim();
    (!inner.is_empty() && !inner.contains('/')).then(|| inner.to_string())
}

#[cfg(windows)]
fn gpus() -> Vec<Device> {
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
    found
}

/// Codifica 10 fotogramas de prueba. Hay drivers que aceptan el primero y fallan (o se cuelgan) a partir del
/// quinto, como el VAAPI HEVC de Mesa sobre DirectX; por eso varios fotogramas y un tiempo máximo.
fn works(e: Encoder) -> bool {
    let Ok(mut child) = cmd("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-f", "lavfi", "-i", "color=black:s=256x256:d=1", "-frames:v", "10"])
        .args(video_args(e, 28, false))
        .args(["-f", "null", "-"])
        .stdout(Stdio::null()).stderr(Stdio::null())
        .spawn() else { return false };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

/// Argumentos de vídeo de ffmpeg: códec, calidad, filtros y formato de píxel.
/// `calidad` en escala CRF (24 máxima calidad, 28 equilibrado, 32 más pequeño).
fn video_args(e: Encoder, calidad: u32, fps30: bool) -> Vec<String> {
    let q = calidad.to_string();
    let qp = (calidad - 5).to_string(); // medido en un directo real: AMF QP 23 ≈ NVENC CQ 28 (SSIM 0.991, tamaño parecido)
    let node = if let Encoder::Vaapi(n) = e { format!("vaapi=va:/dev/dri/renderD{n}") } else { String::new() };
    let v: Vec<&str> = match e {
        Encoder::Nvenc(_) => vec!["-c:v", "hevc_nvenc", "-preset", "p7", "-rc", "vbr", "-cq", &q, "-b:v", "0"],
        Encoder::Amf => vec!["-c:v", "hevc_amf", "-quality", "quality", "-rc", "cqp", "-qp_i", &qp, "-qp_p", &qp],
        // shortcut: QSV sin probar (no hay Intel aquí); ajustar si alguien lo usa
        Encoder::Qsv => vec!["-c:v", "hevc_qsv", "-global_quality", &q, "-preset", "slower"],
        // shortcut: VAAPI sin medir en hardware real; usa la QP de AMF porque en AMD es el mismo chip (VCN)
        Encoder::Vaapi(_) => vec!["-init_hw_device", &node, "-filter_hw_device", "va",
            "-c:v", "hevc_vaapi", "-rc_mode", "CQP", "-qp", &qp],
        Encoder::Cpu => vec!["-c:v", "libx265", "-crf", &q, "-preset", "slow"],
    };
    let mut v: Vec<String> = v.into_iter().map(String::from).collect();
    if let Encoder::Nvenc(i) = e {
        v.extend(["-gpu".into(), i.to_string()]);
    }
    // VAAPI codifica fotogramas que ya están en la gráfica: se convierten a NV12 y se suben allí
    let mut filters: Vec<&str> = fps30.then_some("fps=30").into_iter().collect();
    if let Encoder::Vaapi(_) = e {
        filters.push("format=nv12,hwupload");
    } else {
        v.extend(["-pix_fmt".into(), "yuv420p".into()]);
    }
    if !filters.is_empty() {
        v.extend(["-vf".into(), filters.join(",")]);
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
/// Nombre corto de la unidad que contiene `dir`: "C:" en Windows; en Linux, su punto de montaje ("/home").
#[cfg(windows)]
pub fn drive_name(dir: &Path) -> String {
    dir.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default()
}

#[cfg(unix)]
pub fn drive_name(dir: &Path) -> String {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    mount_point(&std::fs::read_to_string("/proc/self/mounts").unwrap_or_default(), &dir)
}

/// El punto de montaje más largo de /proc/self/mounts que contiene `dir` (los espacios vienen como \040).
#[cfg(unix)]
fn mount_point(mounts: &str, dir: &Path) -> String {
    mounts.lines().filter_map(|l| l.split(' ').nth(1)).map(|m| m.replace("\\040", " "))
        .filter(|m| dir.starts_with(m)).max_by_key(|m| m.len()).unwrap_or_else(|| "/".into())
}

#[cfg(unix)]
pub fn disk(dir: &Path) -> (u64, u64) {
    use std::os::unix::ffi::OsStrExt;
    let Ok(path) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else { return (0, 0) };
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut s) } != 0 { return (0, 0) }
    let block = s.f_frsize as u64;
    (s.f_blocks as u64 * block, s.f_bavail as u64 * block) // f_bavail: lo que puede usar un usuario normal
}

#[cfg(windows)]
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
        return Outcome::Skipped(t("already").into());
    }
    let size = std::fs::metadata(src).map(|m| m.len()).unwrap_or(0);
    // el tope de bitrate deja la salida en <=60% del original (+ audio): si no cabe, ni empezar
    if (disk(src.parent().unwrap_or(Path::new("."))).1 as f64) < size as f64 * 0.65 {
        return Outcome::Skipped(t("no_space").into());
    }
    let Some(info) = probe(src) else { return Outcome::Failed(t("cant_read").into()) };
    if std::fs::create_dir_all(&outdir).is_err() {
        return Outcome::Failed(t("cant_mkdir").into());
    }

    let mut c = cmd("ffmpeg");
    c.args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-nostats", "-progress", "pipe:1", "-i"])
        .arg(src)
        .args(["-map", "0:v:0", "-map", "0:a?", "-c:a", "copy"])
        .args(video_args(job.encoder, job.calidad, job.cap30 && info.fps > 30.0));
    if info.bitrate > 0 && !matches!(job.encoder, Encoder::Amf | Encoder::Vaapi(_)) {
        // tope: como mucho 60% del bitrate original, así nunca sale más grande (AMF y VAAPI en CQP no lo admiten)
        let cap = info.bitrate * 6 / 10;
        c.args(["-maxrate", &cap.to_string(), "-bufsize", &(cap * 2).to_string()]);
    }
    c.args(["-tag:v", "hvc1", "-movflags", "+faststart", "-f", "mp4"]).arg(&part);
    let Ok(mut child) = c.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() else {
        return Outcome::Failed(t("no_ffmpeg").into());
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
        return Outcome::Failed(fail(if last_error.is_empty() { t("ffmpeg_failed").into() } else { last_error }));
    }
    let out_dur = probe(&part).map(|p| p.duration).unwrap_or(0.0);
    if (out_dur - info.duration).abs() > 2f64.max(info.duration * 0.01) {
        return Outcome::Failed(fail(tf("duration", &[&format!("{out_dur:.0}"), &format!("{:.0}", info.duration)])));
    }
    let out = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(u64::MAX);
    if out >= size {
        fail(String::new());
        return Outcome::Skipped(t("optimized").into());
    }
    if std::fs::rename(&part, &dst).is_err() {
        return Outcome::Failed(fail(t("cant_rename").into()));
    }
    Outcome::Done(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vaapi_uploads_frames_and_others_keep_yuv420p() {
        let v = video_args(Encoder::Vaapi(129), 28, true).join(" ");
        assert!(v.contains("vaapi=va:/dev/dri/renderD129") && v.contains("-vf fps=30,format=nv12,hwupload"), "{v}");
        assert!(!v.contains("yuv420p"), "{v}");
        let n = video_args(Encoder::Nvenc(0), 28, false).join(" ");
        assert!(n.contains("-pix_fmt yuv420p") && !n.contains("-vf"), "{n}");
    }

    // Con un driver que se cuelga (SLIMCAST_TEST_HANGING_VAAPI=128 y LIBVA_DRIVER_NAME=d3d12 en WSL),
    // la detección debe rendirse en ~20 s en vez de esperar para siempre
    #[test]
    fn detection_gives_up_on_hanging_driver() {
        let Some(node) = std::env::var("SLIMCAST_TEST_HANGING_VAAPI").ok().and_then(|n| n.parse().ok()) else { return };
        let start = std::time::Instant::now();
        assert!(!works(Encoder::Vaapi(node)));
        assert!(start.elapsed().as_secs() < 30, "tardó {:?}", start.elapsed());
    }

    #[cfg(unix)]
    #[test]
    fn picks_the_deepest_mount_point() {
        let mounts = "/dev/sda2 / ext4 rw 0 0\n/dev/sdb1 /mnt/juegos\\040y\\040directos ext4 rw 0 0\n/dev/sda3 /home ext4 rw 0 0\n";
        assert_eq!(mount_point(mounts, Path::new("/home/streamer/Videos")), "/home");
        assert_eq!(mount_point(mounts, Path::new("/mnt/juegos y directos/obs")), "/mnt/juegos y directos");
        assert_eq!(mount_point(mounts, Path::new("/opt/x")), "/");
        assert_eq!(mount_point(mounts, Path::new("/homework")), "/"); // por componentes, no por texto
    }

    #[cfg(not(windows))]
    #[test]
    fn reads_gpu_models_from_lspci() {
        assert_eq!(model("GP107 [GeForce GTX 1050 Ti]").as_deref(), Some("GeForce GTX 1050 Ti"));
        assert_eq!(model("Navi 22 [Radeon RX 6700/6700 XT/6750 XT / 6800M/6850M XT]"), None);
        assert_eq!(model("Device 1234"), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn finds_gpus_in_sysfs() {
        // /sys/class/drm falso: renderD128 = NVIDIA, renderD129 = AMD, renderD130 = virtio (se ignora)
        let root = std::env::temp_dir().join(format!("slimcast-drm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (n, slot, vendor) in [(129, "0000:03:00.0", "0x1002"), (128, "0000:01:00.0", "0x10de"), (130, "0000:00:02.0", "0x1af4")] {
            let dev = root.join("devices").join(slot);
            std::fs::create_dir_all(&dev).unwrap();
            std::fs::write(dev.join("vendor"), format!("{vendor}\n")).unwrap();
            let node = root.join("drm").join(format!("renderD{n}"));
            std::fs::create_dir_all(&node).unwrap();
            std::os::unix::fs::symlink(&dev, node.join("device")).unwrap();
        }
        let gpus = gpus_in(&root.join("drm"), |slot| (slot == "0000:01:00.0").then(|| "GeForce GTX 1050 Ti".into()));
        let _ = std::fs::remove_dir_all(&root);
        let got: Vec<_> = gpus.iter().map(|d| (d.name.as_str(), d.encoder)).collect();
        assert_eq!(got, [("GeForce GTX 1050 Ti", Encoder::Nvenc(0)), ("AMD Radeon", Encoder::Vaapi(129))]);
    }

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
            // un clip ya muy comprimido puede no quedar más pequeño: también es un resultado correcto
            let optimized = matches!(&r, Outcome::Skipped(m) if m.as_str() == t("optimized"));
            assert!(matches!(r, Outcome::Done(_)) || optimized, "{}: {r:?}", d.name);
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
