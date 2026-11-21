#![windows_subsystem = "windows"] // sin consola detrás de la ventana
mod encode;

use encode::{Device, Encoder, Job, Outcome};
use gpui::{
    App, Application, Bounds, ClickEvent, Context, ExternalPaths, FontWeight, PathPromptOptions,
    PromptLevel, SharedString, Stateful, Div, Window, WindowBounds, WindowOptions, div, prelude::*,
    px, relative, rgb, rgba, size,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// Paleta grafito, un acento ámbar (luz de tally) y rojo solo para el punto REC
const BG: u32 = 0x141415;
const DEEP: u32 = 0x0f0f10;
const CARD: u32 = 0x19191b;
const RAISED: u32 = 0x232325;
const HOVER: u32 = 0x2b2b2e;
const LINE: u32 = 0xededed14; // texto al 8%
const TEXT: u32 = 0xededed;
const TEXT2: u32 = 0xa1a1a6;
const MUTED: u32 = 0x85858b;
const ACCENT: u32 = 0xe8a33d;
const REC: u32 = 0xe5484d;
const OK: u32 = 0x4cc38a;
const TRACK: u32 = 0x2c2c2f; // más claro que RAISED para que la barra se vea con la fila resaltada
const USED: u32 = 0x47474c;
const QUEUED: u32 = 0x707077;
const SANS: &str = "Segoe UI";
const MONO: &str = "Cascadia Mono";

#[derive(Clone)]
enum State {
    Queued,
    Running(f32),
    Finished(Outcome),
}

#[derive(Clone)]
struct Item {
    path: PathBuf,
    name: SharedString,
    size: u64,
    state: State,
}

struct Compresor {
    items: Arc<Mutex<Vec<Item>>>, // el hilo de trabajo escribe aquí; la UI lo lee cada 250 ms
    running: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    devices: Option<Vec<Device>>,
    device: usize,
    calidad: u32,
    cap30: bool,
    disk: Option<(String, u64, u64)>, // unidad, total, libre
}

fn gb(n: u64) -> String {
    if n >= 1_000_000_000_000 { format!("{:.2} TB", n as f64 / 1e12) } else { format!("{:.2} GB", n as f64 / 1e9) }
}

fn short_name(name: &str) -> String {
    ["NVIDIA ", "GeForce ", "AMD ", "Radeon ", "Intel(R) ", "Intel "].iter()
        .fold(name.to_string(), |n, p| n.replace(p, ""))
}

impl Compresor {
    fn new(running: Arc<AtomicBool>, stop: Arc<AtomicBool>, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            let devices = cx.background_executor().spawn(async { encode::devices() }).await;
            this.update(cx, |v, cx| {
                v.device = devices.iter().position(|d| d.ok).unwrap_or(0);
                v.devices = Some(devices);                cx.notify();
            }).ok();
        }).detach();
        let mut v = Self {
            items: Default::default(), running, stop, devices: None, device: 0, calidad: 28, cap30: false, disk: None,
        };
        v.add(std::env::args().skip(1).map(PathBuf::from).collect()); // videos soltados sobre el .exe
        v
    }

    fn add(&mut self, paths: Vec<PathBuf>) {
        let mut items = self.items.lock().unwrap();
        for p in encode::find_videos(&paths) {
            if !items.iter().any(|i| i.path == p) {
                let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                let name = p.file_name().unwrap_or_default().to_string_lossy().to_string().into();
                items.push(Item { path: p, name, size, state: State::Queued });
            }
        }
        drop(items);
        self.refresh_disk();
    }

    fn refresh_disk(&mut self) {
        let items = self.items.lock().unwrap();
        self.disk = items.first().and_then(|i| i.path.parent()).map(|dir| {
            let drive = dir.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
            let (total, free) = encode::disk(dir);
            (drive, total, free)
        });
    }

    fn pick(&mut self, folder: bool, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: !folder, directories: folder, multiple: !folder, prompt: None });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                this.update(cx, |v, cx| { v.add(paths); cx.notify() }).ok();
            }
        }).detach();
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let Some(device) = self.devices.as_ref().and_then(|d| d.get(self.device)).filter(|d| d.ok) else { return };
        let job = Job { encoder: device.encoder, calidad: self.calidad, cap30: self.cap30 };
        let (items, running, stop) = (self.items.clone(), self.running.clone(), self.stop.clone());
        stop.store(false, Ordering::Relaxed);
        running.store(true, Ordering::Relaxed);
        std::thread::spawn(move || {
            let set = |i: usize, s: State| items.lock().unwrap()[i].state = s;
            let queued: Vec<(usize, PathBuf)> = items.lock().unwrap().iter().enumerate()
                .filter(|(_, it)| matches!(it.state, State::Queued)).map(|(i, it)| (i, it.path.clone())).collect();
            for (i, path) in queued {
                if stop.load(Ordering::Relaxed) { break }
                set(i, State::Running(0.0));
                let r = encode::compress(&path, &job, &stop, |p| set(i, State::Running(p)));
                set(i, State::Finished(r));
            }
            running.store(false, Ordering::Relaxed);
        });
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let alive = this.update(cx, |v, cx| { v.refresh_disk(); cx.notify(); v.running.load(Ordering::Relaxed) });
            if !alive.unwrap_or(false) { break }
        }).detach();
        cx.notify();
    }
}

fn button(id: impl Into<SharedString>, label: impl Into<SharedString>, enabled: bool) -> Stateful<Div> {
    div().id(id.into()).flex().items_center().h(px(32.)).px(px(12.)).rounded(px(8.))
        .bg(rgb(RAISED)).border_1().border_color(rgba(LINE)).child(label.into())
        .when(enabled, |b| b.cursor_pointer().hover(|s| s.bg(rgb(HOVER))))
        .when(!enabled, |b| b.opacity(0.4))
}

fn segment(id: impl Into<SharedString>, label: impl Into<SharedString>, selected: bool, enabled: bool) -> Stateful<Div> {
    div().id(id.into()).flex().items_center().h(px(28.)).px(px(12.)).rounded(px(8.))
        .text_color(rgb(if selected { TEXT } else { TEXT2 })).child(label.into())
        .when(selected, |b| b.bg(rgb(0x323235)))
        .when(enabled && !selected, |b| b.cursor_pointer().hover(|s| s.text_color(rgb(TEXT))))
        .when(!enabled, |b| b.opacity(0.4))
}

fn meter_part(frac: f64, color: u32) -> Div {
    div().h_full().w(relative(frac.clamp(0.0, 1.0) as f32)).bg(rgb(color))
}

impl Render for Compresor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.items.lock().unwrap().clone();
        let running = self.running.load(Ordering::Relaxed);
        let queued = items.iter().filter(|i| matches!(i.state, State::Queued)).count();
        let device_ok = self.devices.as_ref().and_then(|d| d.get(self.device)).is_some_and(|d| d.ok);
        let can_start = !running && queued > 0 && device_ok;
        let has = !items.is_empty();

        let header = div().flex().items_center().justify_between().h(px(56.)).px(px(24.))
            .border_b_1().border_color(rgba(LINE))
            .child(div().text_size(px(16.)).font_weight(FontWeight::MEDIUM).child("Slimcast"))
            .when(has, |h| h.child(div().flex().gap(px(8.))
                .child(button("add-files", "Añadir videos", !running)
                    .when(!running, |b| b.on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.pick(false, cx)))))
                .child(button("add-folder", "Añadir carpeta", !running)
                    .when(!running, |b| b.on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.pick(true, cx)))))));

        let disk = self.disk.clone().map(|(drive, total, free)| {
            let pending: u64 = items.iter().filter(|i| matches!(i.state, State::Queued | State::Running(_))).map(|i| i.size).sum();
            let freed: u64 = items.iter().map(|i| match i.state { State::Finished(Outcome::Done(out)) => i.size - out, _ => 0 }).sum();
            let used = total.saturating_sub(free);
            let t = total.max(1) as f64;
            let figure = |value: String, label: &'static str, color: u32| div().flex().items_baseline().gap(px(8.))
                .child(div().font_family(MONO).text_size(px(32.)).line_height(px(32.)).font_weight(FontWeight::MEDIUM).text_color(rgb(color)).child(value))
                .child(div().text_color(rgb(TEXT2)).child(label));
            let legend = |color: u32, label: String| div().flex().items_center().gap(px(8.))
                .child(div().size(px(8.)).rounded(px(2.)).bg(rgb(color))).child(label);
            div().flex().flex_col().p(px(24.)).pt(px(20.)).rounded(px(16.)).bg(rgb(CARD)).border_1().border_color(rgba(LINE))
                .child(div().flex().justify_between()
                    .child(div().text_color(rgb(TEXT2)).child(format!("Disco {drive}")))
                    .child(div().font_family(MONO).text_size(px(12.)).text_color(rgb(MUTED)).child(gb(total))))
                .child(div().flex().items_baseline().gap(px(40.)).mt(px(8.))
                    .child(figure(gb(free), "libres", TEXT))
                    .when(freed > 0, |d| d.child(figure(format!("+{}", gb(freed)), "al borrar los originales", ACCENT))))
                .child(div().flex().h(px(12.)).mt(px(20.)).rounded(px(6.)).overflow_hidden().bg(rgb(RAISED))
                    .child(meter_part(used.saturating_sub(pending + freed) as f64 / t, USED))
                    .child(meter_part(pending as f64 / t, QUEUED))
                    .child(meter_part(freed as f64 / t, ACCENT)))
                .child(div().flex().gap(px(20.)).mt(px(12.)).text_size(px(12.)).text_color(rgb(TEXT2))
                    .child(legend(USED, "Ocupado".into()))
                    .child(legend(QUEUED, format!("En cola {}", gb(pending))))
                    .child(legend(ACCENT, "Liberable".into())))
        });

        let rows = items.iter().enumerate().map(|(i, it)| {
            let (bar, bar_color, sizes, state, state_color) = match &it.state {
                State::Queued => (0.0, TEXT2, gb(it.size), "En cola".to_string(), MUTED),
                State::Running(p) => (*p, TEXT2, gb(it.size), format!("Comprimiendo {}%", (p * 100.0) as u32), TEXT),
                State::Finished(Outcome::Done(out)) => (*out as f32 / it.size as f32, ACCENT,
                    format!("{} → {}  −{}%", gb(it.size), gb(*out), 100 - out * 100 / it.size.max(1)), "Listo".into(), OK),
                State::Finished(Outcome::Skipped(m)) => (0.0, TEXT2, gb(it.size), m.clone(), MUTED),
                State::Finished(Outcome::Failed(m)) => (0.0, TEXT2, gb(it.size), m.clone(), REC),
                State::Finished(Outcome::Cancelled) => (0.0, TEXT2, gb(it.size), "Cancelado".into(), MUTED),
            };
            let live = matches!(it.state, State::Running(_));
            div().id(("row", i)).flex().items_center().gap(px(12.)).p(px(12.)).rounded(px(8.))
                .hover(|s| s.bg(rgb(RAISED)))
                .child(div().size(px(8.)).rounded_full().when(live, |d| d.bg(rgb(REC))))
                .child(div().flex_1().min_w(px(0.)).flex().flex_col().gap(px(8.))
                    .child(div().truncate().child(it.name.clone()))
                    .child(div().h(px(4.)).rounded(px(2.)).bg(rgb(TRACK)).overflow_hidden()
                        .child(div().h_full().w(relative(bar.clamp(0.0, 1.0))).bg(rgb(bar_color)))))
                .child(div().w(px(220.)).flex().justify_end().font_family(MONO).text_size(px(12.)).text_color(rgb(TEXT2)).child(sizes))
                .child(div().w(px(150.)).truncate().text_size(px(13.)).text_color(rgb(state_color)).child(state))
                .child(div().id(("rm", i)).size(px(28.)).flex().items_center().justify_center().rounded(px(8.))
                    .text_color(rgb(MUTED)).child("×")
                    .when(running, |b| b.opacity(0.4))
                    .when(!running, |b| b.cursor_pointer().hover(|s| s.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                            v.items.lock().unwrap().remove(i);
                            v.refresh_disk();
                            cx.notify();
                        }))))
        });

        let queue = div().id("queue").flex_1().min_h(px(0.)).flex().flex_col().rounded(px(16.)).bg(rgb(CARD))
            .border_1().border_color(rgba(LINE))
            .drag_over::<ExternalPaths>(|s, _, _, _| s.border_color(rgb(ACCENT)).bg(rgb(0x1d1c1a)))
            .when(has, |q| q
                .child(div().flex().justify_between().px(px(24.)).pt(px(16.)).pb(px(8.)).text_color(rgb(TEXT2))
                    .child(div().flex().gap(px(8.)).items_baseline().child("Cola")
                        .child(div().font_family(MONO).text_size(px(12.)).text_color(rgb(MUTED))
                            .child(format!("{} {}", items.len(), if items.len() == 1 { "video" } else { "videos" }))))
                    .child(div().text_size(px(12.)).text_color(rgb(MUTED)).child("Los originales no se tocan, el resultado va a la carpeta comprimidos")))
                .child(div().id("list").flex_1().min_h(px(0.)).overflow_y_scroll().px(px(12.)).pb(px(12.)).flex().flex_col().gap(px(2.)).children(rows)))
            .when(!has, |q| q.child(div().flex_1().m(px(12.)).flex().flex_col().items_center().justify_center().gap(px(16.))
                .rounded(px(12.)).border_1().border_dashed().border_color(rgba(0xededed24)).text_color(rgb(TEXT2))
                .child("Arrastra aquí tus grabaciones o una carpeta")
                .child(div().flex().gap(px(8.))
                    .child(button("e-files", "Añadir videos", true).on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.pick(false, cx))))
                    .child(button("e-folder", "Añadir carpeta", true).on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.pick(true, cx)))))));

        let seg_group = |children: Vec<Stateful<Div>>| div().flex().p(px(2.)).rounded(px(10.)).bg(rgb(RAISED))
            .border_1().border_color(rgba(LINE)).children(children);
        let qualities = [(24, "Máxima calidad"), (28, "Equilibrado"), (32, "Más pequeño")].into_iter().map(|(q, label)| {
            segment(SharedString::from(format!("q{q}")), label, self.calidad == q, !running)
                .when(!running, |b| b.on_click(cx.listener(move |v, _: &ClickEvent, _, cx| { v.calidad = q; cx.notify() })))
        }).collect();
        let devices: Vec<Stateful<Div>> = match &self.devices {
            None => vec![segment("detect", "Detectando…", false, false)],
            Some(list) => list.iter().enumerate().map(|(i, d)| {
                let label = if d.encoder == Encoder::Cpu { "CPU".to_string() } else { short_name(&d.name) };
                let label = if d.ok { label } else { format!("{label} (no disponible)") };
                let enabled = d.ok && !running;
                segment(SharedString::from(format!("dev{i}")), label, self.device == i, enabled)
                    .when(enabled, |b| b.on_click(cx.listener(move |v, _: &ClickEvent, _, cx| { v.device = i; cx.notify() })))
            }).collect(),
        };
        let cap30 = div().id("fps30").flex().items_center().gap(px(8.)).text_color(rgb(TEXT2))
            .child(div().size(px(16.)).rounded(px(4.)).border_1().flex().items_center().justify_center().text_size(px(11.))
                .when(self.cap30, |c| c.bg(rgb(ACCENT)).border_color(rgb(ACCENT)).text_color(rgb(BG)).child("✓"))
                .when(!self.cap30, |c| c.bg(rgb(RAISED)).border_color(rgba(0xededed30))))
            .child("Bajar a 30 fps")
            .when(running, |b| b.opacity(0.4))
            .when(!running, |b| b.cursor_pointer().on_click(cx.listener(|v, _: &ClickEvent, _, cx| { v.cap30 = !v.cap30; cx.notify() })));

        let footer = div().flex().items_center().gap(px(24.)).px(px(24.)).py(px(16.)).bg(rgb(DEEP))
            .border_t_1().border_color(rgba(LINE)).whitespace_nowrap()
            .child(div().flex().items_center().gap(px(12.)).text_color(rgb(TEXT2)).child("Dispositivo").child(seg_group(devices)))
            .child(div().flex().items_center().gap(px(12.)).text_color(rgb(TEXT2)).child("Tamaño").child(seg_group(qualities)))
            .child(cap30)
            .child(div().flex_1())
            .when(running, |f| f.child(button("cancel", "Cancelar", true)
                .on_click(cx.listener(|v, _: &ClickEvent, _, _| v.stop.store(true, Ordering::Relaxed)))))
            .child(div().id("start").flex().items_center().h(px(40.)).px(px(20.)).rounded(px(8.))
                .bg(rgb(TEXT)).text_color(rgb(BG)).font_weight(FontWeight::MEDIUM)
                .child(if queued > 1 { format!("Comprimir {queued} videos") } else { "Comprimir".into() })
                .when(can_start, |b| b.cursor_pointer().hover(|s| s.bg(rgb(0xffffff)))
                    .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.start(cx))))
                .when(!can_start, |b| b.opacity(0.4)));

        let ffmpeg_missing = self.devices.as_ref().is_some_and(|d| d.iter().all(|d| !d.ok));
        div().id("root").size_full().flex().flex_col().bg(rgb(BG)).text_color(rgb(TEXT)).font_family(SANS).text_size(px(14.))
            .on_drop(cx.listener(|v, paths: &ExternalPaths, _, cx| {
                if !v.running.load(Ordering::Relaxed) { v.add(paths.paths().to_vec()); cx.notify() }
            }))
            .child(header)
            .child(div().flex_1().min_h(px(0.)).flex().flex_col().gap(px(16.)).p(px(24.))
                .when(ffmpeg_missing, |m| m.child(div().px(px(16.)).py(px(12.)).rounded(px(12.)).bg(rgba(0xe5484d1a))
                    .border_1().border_color(rgba(0xe5484d4d)).text_color(rgb(0xf2a7a9))
                    .child("No se encuentra ffmpeg. Instálalo con  winget install Gyan.FFmpeg --version 8.0.1  y vuelve a abrir el programa")))
                .children(disk)
                .child(queue))
            .child(footer)
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let running = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let bounds = Bounds::centered(None, size(px(1180.), px(640.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(1140.), px(520.))),
            titlebar: Some(gpui::TitlebarOptions { title: Some("Slimcast".into()), ..Default::default() }),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            let (r, s) = (running.clone(), stop.clone());
            window.on_window_should_close(cx, move |window, cx| {
                if !r.load(Ordering::Relaxed) { return true }
                let answer = window.prompt(PromptLevel::Warning, "Hay una compresión en marcha",
                    Some("Si sales se cancela y se borra el archivo a medias"), &["Cancelar y salir", "Seguir"], cx);
                let (r, s) = (r.clone(), s.clone());
                cx.spawn(async move |cx| {
                    if answer.await != Ok(0) { return }
                    s.store(true, Ordering::Relaxed);
                    while r.load(Ordering::Relaxed) { // espera a que ffmpeg pare y se borre el .part
                        cx.background_executor().timer(Duration::from_millis(100)).await;
                    }
                    cx.update(|cx| cx.quit()).ok();
                }).detach();
                false
            });
            cx.new(|cx| Compresor::new(running.clone(), stop.clone(), cx))
        }).unwrap();
        cx.on_window_closed(|cx| cx.quit()).detach();
        cx.activate(true);
    });
}

