#![windows_subsystem = "windows"] // sin consola detrás de la ventana
mod encode;
mod i18n;
mod update;

use encode::{Device, Encoder, Job, Outcome};
use i18n::{t, tf};
use gpui::{
    App, Application, Bounds, ClickEvent, Context, Div, ExternalPaths, FontWeight,
    ObjectFit, PathPromptOptions, PromptLevel, ScrollWheelEvent, SharedString, Stateful, StyledImage, Window,
    WindowBounds, WindowControlArea, WindowOptions, canvas, div, img, prelude::*, px, relative, rgb, rgba, size,
};
use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// Paleta grafito, un acento ámbar (luz de tally) y rojo solo para el punto REC
const BG: u32 = 0x141415;
const DEEP: u32 = 0x0f0f10;
const CARD: u32 = 0x19191b;
const RAISED: u32 = 0x232325;
const HOVER: u32 = 0x2b2b2e;
const PRESSED: u32 = 0x353538;
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
const MONO: &str = "Cascadia Mono";
const ROW_H: f32 = 52.;
const ROW_GAP: f32 = 2.;

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
    born: Instant,                     // para la animación de entrada
    thumb: Option<(PathBuf, Instant)>, // miniatura y cuándo llegó (para el fundido)
}

/// Avance 0..1 de una animación de `ms` que empezó en `start`, con salida suave (quint).
fn fade(start: Instant, now: Instant, ms: f32) -> f32 {
    let t = (now.duration_since(start).as_secs_f32() * 1000. / ms).min(1.);
    1. - (1. - t).powi(5)
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
    // scroll propio: el de GPUI salta a golpes con la rueda, así que se suaviza a mano
    scroll_y: f32,
    scroll_target: f32,
    list_h: Rc<Cell<f32>>,
    shown: HashMap<PathBuf, f32>, // progreso que se dibuja; persigue al real
    last_frame: Instant,
    update: Option<update::Release>, // versión nueva en GitHub
    update_msg: Option<SharedString>, // mensaje de descarga o el error
}

fn gb(n: u64) -> String {
    if n >= 1_000_000_000_000 { format!("{:.2} TB", n as f64 / 1e12) } else { format!("{:.2} GB", n as f64 / 1e9) }
}

fn short_name(name: &str) -> String {
    ["NVIDIA ", "GeForce ", "AMD ", "Radeon ", "Intel(R) ", "Intel "].iter()
        .fold(name.to_string(), |n, p| n.replace(p, ""))
}

/// Miniatura en %TEMP%; cambia de nombre si el video cambia de tamaño o de fecha.
fn thumb_path(src: &PathBuf) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    if let Ok(m) = std::fs::metadata(src) {
        m.len().hash(&mut h);
        m.modified().ok().hash(&mut h);
    }
    let dir = std::env::temp_dir().join("slimcast-thumbs");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{:016x}.png", h.finish()))
}

/// Acerca `v` a `target` de forma exponencial; devuelve true cuando ya llegó.
fn ease(v: &mut f32, target: f32, dt: f32, speed: f32, eps: f32) -> bool {
    *v += (target - *v) * (1.0 - (-dt * speed).exp());
    if (target - *v).abs() < eps { *v = target; true } else { false }
}

impl Compresor {
    fn new(running: Arc<AtomicBool>, stop: Arc<AtomicBool>, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this, cx| {
            let devices = cx.background_executor().spawn(async { encode::devices() }).await;
            this.update(cx, |v, cx| {
                v.device = devices.iter().position(|d| d.ok).unwrap_or(0);
                v.devices = Some(devices);
                cx.notify();
            }).ok();
        }).detach();
        cx.spawn(async move |this, cx| {
            let release = cx.background_executor().spawn(async { update::check() }).await;
            this.update(cx, |v, cx| { v.update = release; cx.notify() }).ok();
        }).detach();
        let mut v = Self {
            items: Default::default(), running, stop, devices: None, device: 0, calidad: 28, cap30: false, disk: None,
            scroll_y: 0., scroll_target: 0., list_h: Default::default(), shown: HashMap::new(), last_frame: Instant::now(),
            update: None, update_msg: None,
        };
        v.add(std::env::args().skip(1).map(PathBuf::from).collect(), cx); // videos soltados sobre el .exe
        v
    }

    fn add(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let mut items = self.items.lock().unwrap();
        let mut fresh = Vec::new();
        for p in encode::find_videos(&paths) {
            if !items.iter().any(|i| i.path == p) {
                let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
                let name = p.file_name().unwrap_or_default().to_string_lossy().to_string().into();
                fresh.push(p.clone());
                items.push(Item { path: p, name, size, state: State::Queued, born: Instant::now(), thumb: None });
            }
        }
        drop(items);
        self.refresh_disk();
        // miniaturas de una en una en segundo plano; cada una aparece en cuanto está lista
        cx.spawn(async move |this, cx| {
            for src in fresh {
                let out = thumb_path(&src);
                let (s, o) = (src.clone(), out.clone());
                let ok = cx.background_executor().spawn(async move { o.exists() || encode::thumbnail(&s, &o) }).await;
                if !ok { continue }
                let alive = this.update(cx, |v, cx| {
                    if let Some(it) = v.items.lock().unwrap().iter_mut().find(|i| i.path == src) { it.thumb = Some((out, Instant::now())) }
                    cx.notify();
                });
                if alive.is_err() { break }
            }
        }).detach();
    }

    fn refresh_disk(&mut self) {
        let items = self.items.lock().unwrap();
        self.disk = items.first().and_then(|i| i.path.parent()).map(|dir| {
            let drive = dir.components().next().map(|c| c.as_os_str().to_string_lossy().to_string()).unwrap_or_default();
            let (total, free) = encode::disk(dir);
            (drive, total, free)
        });
    }

    fn install_update(&mut self, cx: &mut Context<Self>) {
        let Some(release) = self.update.clone() else { return };
        self.update_msg = Some(t("downloading").into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            match cx.background_executor().spawn(async move { update::install(&release) }).await {
                Ok(()) => { cx.update(|cx| cx.quit()).ok(); } // el instalador cierra, actualiza y vuelve a abrir
                Err(e) => { this.update(cx, |v, cx| { v.update_msg = Some(e.into()); cx.notify() }).ok(); }
            }
        }).detach();
    }

    fn pick(&mut self, folder: bool, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions { files: !folder, directories: folder, multiple: !folder, prompt: None });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                this.update(cx, |v, cx| { v.add(paths, cx); cx.notify() }).ok();
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
            // por ruta y no por índice: la lista puede cambiar mientras se comprime
            let set = |path: &PathBuf, s: State| {
                if let Some(it) = items.lock().unwrap().iter_mut().find(|it| &it.path == path) { it.state = s }
            };
            let queued: Vec<PathBuf> = items.lock().unwrap().iter()
                .filter(|it| matches!(it.state, State::Queued)).map(|it| it.path.clone()).collect();
            for path in queued {
                if stop.load(Ordering::Relaxed) { break }
                set(&path, State::Running(0.0));
                let r = encode::compress(&path, &job, &stop, |p| set(&path, State::Running(p)));
                set(&path, State::Finished(r));
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
    div().id(id.into()).flex().items_center().h(px(28.)).px(px(12.)).rounded(px(7.))
        .bg(rgb(RAISED)).border_1().border_color(rgba(LINE)).child(label.into())
        .when(enabled, |b| b.cursor_pointer().hover(|s| s.bg(rgb(HOVER))).active(|s| s.bg(rgb(PRESSED))))
        .when(!enabled, |b| b.opacity(0.4))
}

fn segment(id: impl Into<SharedString>, label: impl Into<SharedString>, selected: bool, enabled: bool) -> Stateful<Div> {
    div().id(id.into()).flex().items_center().h(px(26.)).px(px(10.)).rounded(px(7.))
        .text_color(rgb(if selected { TEXT } else { TEXT2 })).child(label.into())
        .when(selected, |b| b.bg(rgb(0x323235)))
        .when(enabled && !selected, |b| b.cursor_pointer()
            .hover(|s| s.text_color(rgb(TEXT)).bg(rgb(HOVER))).active(|s| s.bg(rgb(PRESSED))))
        .when(!enabled, |b| b.opacity(0.4))
}

fn meter_part(frac: f64, color: u32) -> Div {
    div().h_full().w(relative(frac.clamp(0.0, 1.0) as f32)).bg(rgb(color))
}

impl Render for Compresor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.items.lock().unwrap().clone();
        let running = self.running.load(Ordering::Relaxed);
        let queued = items.iter().filter(|i| matches!(i.state, State::Queued)).count();
        let device_ok = self.devices.as_ref().and_then(|d| d.get(self.device)).is_some_and(|d| d.ok);
        let can_start = !running && queued > 0 && device_ok;
        let has = !items.is_empty();

        // animación: el scroll y las barras persiguen su objetivo; se pide otro fotograma hasta que llegan
        let now = Instant::now();
        let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.05);
        self.last_frame = now;
        let content_h = if has { items.len() as f32 * (ROW_H + ROW_GAP) + 8. } else { 0. };
        let view_h = self.list_h.get();
        let max_scroll = (content_h - view_h).max(0.);
        self.scroll_target = self.scroll_target.clamp(0., max_scroll);
        let mut settled = ease(&mut self.scroll_y, self.scroll_target, dt, 18., 0.5);
        self.shown.retain(|p, _| items.iter().any(|i| &i.path == p));
        for it in &items {
            let target = match &it.state {
                State::Running(p) => *p,
                State::Finished(Outcome::Done(out)) => *out as f32 / it.size.max(1) as f32,
                _ => 0.0,
            };
            match self.shown.get_mut(&it.path) {
                Some(v) => settled &= ease(v, target, dt, 6., 0.001),
                None => { self.shown.insert(it.path.clone(), target); }
            }
        }
        // solo se construyen las filas a la vista (más una de margen): con cientos de videos el scroll sigue fluido
        let stride = ROW_H + ROW_GAP;
        let first = ((self.scroll_y / stride) as usize).min(items.len());
        let last = (((self.scroll_y + view_h) / stride).ceil() as usize + 1).min(items.len());
        for it in &items[first..last] {
            let thumb_in = it.thumb.as_ref().is_none_or(|(_, at)| fade(*at, now, 300.) >= 1.);
            settled &= fade(it.born, now, 260.) >= 1. && thumb_in;
        }
        if !settled { window.request_animation_frame() }

        // barra de título propia: logo, nombre y versión arrastran la ventana; los botones de Windows van a la derecha
        // el icono de la app dibujado a mano, con sus proporciones: barra gris larga y barra ámbar corta
        let mark = div().size(px(28.)).flex_none().flex().flex_col().justify_center().gap(px(3.)).pl(px(5.))
            .rounded(px(7.)).bg(rgb(CARD)).border_1().border_color(rgba(LINE))
            .child(div().w(px(17.)).h(px(4.)).rounded(px(1.)).bg(rgb(0x55555b)))
            .child(div().w(px(8.)).h(px(4.)).rounded(px(1.)).bg(rgb(ACCENT)));
        let caption = |id: &'static str, glyph: &'static str, area: WindowControlArea, close: bool|
            div().id(id).w(px(46.)).h_full().flex().items_center().justify_center()
                .font_family("Segoe MDL2 Assets").text_size(px(10.)).text_color(rgb(TEXT2))
                .window_control_area(area)
                .when(close, |b| b.hover(|s| s.bg(rgb(0xc42b1c)).text_color(rgb(0xffffff))))
                .when(!close, |b| b.hover(|s| s.bg(rgb(HOVER)).text_color(rgb(TEXT))))
                .child(glyph);
        let header = div().flex().items_center().h(px(44.)).bg(rgb(DEEP)).border_b_1().border_color(rgba(LINE))
            .child(div().flex_1().h_full().flex().items_center().gap(px(10.)).pl(px(16.))
                .window_control_area(WindowControlArea::Drag)
                .child(mark)
                .child(div().text_size(px(15.)).font_weight(FontWeight::MEDIUM).child("Slimcast"))
                .child(div().font_family(MONO).text_size(px(11.)).text_color(rgb(MUTED))
                    .child(concat!("v", env!("CARGO_PKG_VERSION")))))
            .when(has, |h| h.child(div().flex().gap(px(8.)).pr(px(12.))
                .child(button("add-files", t("add_videos"), !running)
                    .when(!running, |b| b.on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.pick(false, cx)))))
                .child(button("add-folder", t("add_folder"), !running)
                    .when(!running, |b| b.on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.pick(true, cx)))))))
            .child(caption("min", "\u{e921}", WindowControlArea::Min, false))
            .child(caption("max", if window.is_maximized() { "\u{e923}" } else { "\u{e922}" }, WindowControlArea::Max, false))
            .child(caption("close", "\u{e8bb}", WindowControlArea::Close, true));

        let disk = self.disk.clone().map(|(drive, total, free)| {
            let pending: u64 = items.iter().filter(|i| matches!(i.state, State::Queued | State::Running(_))).map(|i| i.size).sum();
            let freed: u64 = items.iter().map(|i| match i.state { State::Finished(Outcome::Done(out)) => i.size - out, _ => 0 }).sum();
            let used = total.saturating_sub(free);
            let tot = total.max(1) as f64;
            let legend = |color: u32, label: String| div().flex().items_center().gap(px(6.))
                .child(div().size(px(8.)).rounded(px(2.)).bg(rgb(color))).child(label);
            div().flex().flex_col().gap(px(10.)).px(px(16.)).py(px(12.)).rounded(px(12.)).bg(rgb(CARD))
                .border_1().border_color(rgba(LINE))
                .child(div().flex().items_baseline().gap(px(10.))
                    .child(div().text_color(rgb(TEXT2)).child(tf("disk", &[&drive])))
                    .child(div().font_family(MONO).text_size(px(20.)).font_weight(FontWeight::MEDIUM).child(gb(free)))
                    .child(div().text_color(rgb(TEXT2)).child(tf("free_of", &[&gb(total)])))
                    .when(freed > 0, |d| d.child(div().ml(px(8.)).text_color(rgb(ACCENT))
                        .child(tf("freed", &[&gb(freed)]))))
                    .child(div().flex_1())
                    .child(div().flex().gap(px(16.)).text_size(px(12.)).text_color(rgb(TEXT2))
                        .child(legend(USED, t("used").into()))
                        .child(legend(QUEUED, tf("queued_size", &[&gb(pending)])))
                        .child(legend(ACCENT, t("freeable").into()))))
                .child(div().flex().h(px(6.)).rounded(px(3.)).overflow_hidden().bg(rgb(RAISED))
                    .child(meter_part(used.saturating_sub(pending + freed) as f64 / tot, USED))
                    .child(meter_part(pending as f64 / tot, QUEUED))
                    .child(meter_part(freed as f64 / tot, ACCENT)))
        });

        let rows: Vec<_> = items.iter().enumerate().skip(first).take(last - first).map(|(i, it)| {
            let shown = self.shown.get(&it.path).copied().unwrap_or(0.0);
            let (bar_color, sizes, state, state_color) = match &it.state {
                State::Queued => (TEXT2, gb(it.size), t("queued").to_string(), MUTED),
                State::Running(p) => (TEXT2, gb(it.size), tf("compressing", &[&((p * 100.0) as u32)]), TEXT),
                State::Finished(Outcome::Done(out)) => (ACCENT,
                    format!("{} → {}  −{}%", gb(it.size), gb(*out), 100 - out * 100 / it.size.max(1)), t("done").into(), OK),
                State::Finished(Outcome::Skipped(m)) => (TEXT2, gb(it.size), m.clone(), TEXT2),
                State::Finished(Outcome::Failed(m)) => (TEXT2, gb(it.size), m.clone(), REC),
                State::Finished(Outcome::Cancelled) => (TEXT2, gb(it.size), t("cancelled").into(), MUTED),
            };
            let live = matches!(it.state, State::Running(_));
            // animaciones calculadas a mano: con with_animation se repetirían cada vez que una fila vuelve a la vista
            let enter = fade(it.born, now, 260.);
            let thumb = div().w(px(64.)).h(px(36.)).flex_none().rounded(px(4.)).overflow_hidden().bg(rgb(RAISED))
                .when_some(it.thumb.clone(), |d, (t, at)| d.child(img(t).size_full().object_fit(ObjectFit::Cover)
                    .opacity(fade(at, now, 300.))));
            div().id(("row", i)).opacity(enter).relative().left(px((1.0 - enter) * 12.)).flex().flex_none().items_center().gap(px(12.)).h(px(ROW_H)).px(px(8.)).rounded(px(8.))
                .hover(|s| s.bg(rgb(RAISED)))
                .child(div().size(px(6.)).flex_none().rounded_full().when(live, |d| d.bg(rgb(REC))))
                .child(thumb)
                .child(div().flex_1().min_w(px(0.)).flex().flex_col().gap(px(6.))
                    .child(div().truncate().text_size(px(13.)).child(it.name.clone()))
                    .child(div().h(px(3.)).rounded(px(2.)).bg(rgb(TRACK)).overflow_hidden()
                        .child(div().h_full().w(relative(shown.clamp(0.0, 1.0))).bg(rgb(bar_color)))))
                .child(div().w(px(200.)).flex_none().flex().justify_end().font_family(MONO).text_size(px(12.))
                    .text_color(rgb(TEXT2)).child(sizes))
                .child(div().w(px(180.)).flex_none().truncate().text_size(px(13.)).text_color(rgb(state_color)).child(state))
                .child(div().id(("rm", i)).size(px(24.)).flex_none().flex().items_center().justify_center().rounded(px(6.))
                    .text_color(rgb(MUTED)).child("×")
                    .when(running, |b| b.opacity(0.4))
                    .when(!running, |b| b.cursor_pointer().hover(|s| s.bg(rgb(HOVER)).text_color(rgb(TEXT)))
                        .on_click(cx.listener(move |v, _: &ClickEvent, _, cx| {
                            v.items.lock().unwrap().remove(i);
                            v.refresh_disk();
                            cx.notify();
                        }))))
        }).collect();

        let list_h = self.list_h.clone();
        let scrollbar = (max_scroll > 0. && view_h > 0.).then(|| {
            let thumb_h = (view_h * view_h / content_h).max(24.);
            let top = self.scroll_y / max_scroll * (view_h - thumb_h);
            div().absolute().right(px(3.)).top(px(top)).w(px(3.)).h(px(thumb_h)).rounded(px(2.)).bg(rgba(0xededed30))
        });
        let list = div().id("list").relative().flex_1().min_h(px(0.)).overflow_hidden()
            .on_scroll_wheel(cx.listener(|v, e: &ScrollWheelEvent, window, cx| {
                // Windows da 3 líneas (~1 fila) por muesca; se multiplica para avanzar unas 3 filas
                v.scroll_target -= f32::from(e.delta.pixel_delta(window.line_height()).y) * 2.5;
                cx.notify();
            }))
            .child(canvas(move |b, _, _| list_h.set(f32::from(b.size.height)), |_, _, _, _| {}).absolute().size_full())
            .child(div().absolute().left_0().right_0().top(px(first as f32 * stride - self.scroll_y)).px(px(8.))
                .flex().flex_col().gap(px(ROW_GAP)).children(rows))
            .children(scrollbar);

        let done = items.iter().filter(|i| matches!(i.state, State::Finished(_))).count();
        let queue = div().id("queue").flex_1().min_h(px(0.)).flex().flex_col().rounded(px(12.)).bg(rgb(CARD))
            .border_1().border_color(rgba(LINE))
            .drag_over::<ExternalPaths>(|s, _, _, _| s.border_color(rgb(ACCENT)).bg(rgb(0x1d1c1a)))
            .when(has, |q| q
                .child(div().flex().items_baseline().justify_between().px(px(16.)).pt(px(10.)).pb(px(6.)).text_color(rgb(TEXT2))
                    .child(div().flex().gap(px(8.)).items_baseline().child(t("queue"))
                        .child(div().font_family(MONO).text_size(px(12.)).text_color(rgb(MUTED))
                            .child(tf("count", &[&done, &items.len()]))))
                    .child(div().text_size(px(12.)).text_color(rgb(MUTED))
                        .child(t("note"))))
                .child(list))
            .when(!has, |q| q.child(div().flex_1().m(px(12.)).flex().flex_col().items_center().justify_center().gap(px(16.))
                .rounded(px(10.)).border_1().border_dashed().border_color(rgba(0xededed24)).text_color(rgb(TEXT2))
                .child(t("drop"))
                .child(div().flex().gap(px(8.))
                    .child(button("e-files", t("add_videos"), true).on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.pick(false, cx))))
                    .child(button("e-folder", t("add_folder"), true).on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.pick(true, cx)))))));

        let seg_group = |children: Vec<Stateful<Div>>| div().flex().p(px(2.)).rounded(px(9.)).bg(rgb(RAISED))
            .border_1().border_color(rgba(LINE)).children(children);
        let qualities = [(24, t("q_best")), (28, t("q_balanced")), (32, t("q_small"))].into_iter().map(|(q, label)| {
            segment(SharedString::from(format!("q{q}")), label, self.calidad == q, !running)
                .when(!running, |b| b.on_click(cx.listener(move |v, _: &ClickEvent, _, cx| { v.calidad = q; cx.notify() })))
        }).collect();
        let devices: Vec<Stateful<Div>> = match &self.devices {
            None => vec![segment("detect", t("detecting"), false, false)],
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
            .child(t("fps30"))
            .when(running, |b| b.opacity(0.4))
            .when(!running, |b| b.cursor_pointer().on_click(cx.listener(|v, _: &ClickEvent, _, cx| { v.cap30 = !v.cap30; cx.notify() })));

        // un solo hueco para la acción principal: mientras comprime, Comprimir pasa a ser Cancelar
        let action = div().id("start").flex().flex_none().items_center().justify_center().min_w(px(150.)).h(px(36.)).px(px(16.))
            .rounded(px(8.)).font_weight(FontWeight::MEDIUM);
        let action = if running {
            action.bg(rgb(RAISED)).border_1().border_color(rgba(LINE)).text_color(rgb(TEXT)).child(t("cancel"))
                .cursor_pointer().hover(|s| s.bg(rgb(HOVER))).active(|s| s.bg(rgb(PRESSED)))
                .on_click(cx.listener(|v, _: &ClickEvent, _, _| v.stop.store(true, Ordering::Relaxed)))
        } else {
            action.bg(rgb(TEXT)).text_color(rgb(BG))
                .child(if queued > 1 { tf("compress_n", &[&queued]) } else { t("compress").into() })
                .when(can_start, |b| b.cursor_pointer().hover(|s| s.bg(rgb(0xffffff))).active(|s| s.bg(rgb(0xd4d4d4)))
                    .on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.start(cx))))
                .when(!can_start, |b| b.opacity(0.4))
        };

        let footer = div().flex().items_center().gap(px(20.)).px(px(20.)).py(px(12.)).bg(rgb(DEEP))
            .border_t_1().border_color(rgba(LINE)).whitespace_nowrap()
            // los ajustes se recortan si un idioma no cabe; el botón principal nunca se mueve
            .child(div().flex_1().min_w(px(0.)).overflow_hidden().flex().items_center().gap(px(20.))
                .child(div().flex().flex_none().items_center().gap(px(10.)).text_color(rgb(TEXT2)).child(t("device")).child(seg_group(devices)))
                .child(div().flex().flex_none().items_center().gap(px(10.)).text_color(rgb(TEXT2)).child(t("size")).child(seg_group(qualities)))
                .child(cap30))
            .child(action);

        let ffmpeg_missing = self.devices.as_ref().is_some_and(|d| d.iter().all(|d| !d.ok));
        div().id("root").size_full().flex().flex_col().bg(rgb(BG)).text_color(rgb(TEXT)).font_family(i18n::font()).text_size(px(14.))
            .on_drop(cx.listener(|v, paths: &ExternalPaths, _, cx| {
                if !v.running.load(Ordering::Relaxed) { v.add(paths.paths().to_vec(), cx); cx.notify() }
            }))
            .child(header)
            .child(div().flex_1().min_h(px(0.)).flex().flex_col().gap(px(12.)).p(px(16.))
                .when(ffmpeg_missing, |m| m.child(div().px(px(16.)).py(px(10.)).rounded(px(10.)).bg(rgba(0xe5484d1a))
                    .border_1().border_color(rgba(0xe5484d4d)).text_color(rgb(0xf2a7a9))
                    .child(t("no_ffmpeg_banner"))))
                .when_some(self.update.clone(), |m, r| {
                    let busy = self.update_msg.as_ref().is_some_and(|m| m.as_ref() == t("downloading"));
                    let enabled = !running && !busy;
                    m.child(div().flex().items_center().gap(px(12.)).px(px(16.)).py(px(8.)).rounded(px(10.))
                        .bg(rgba(0xe8a33d14)).border_1().border_color(rgba(0xe8a33d40))
                        .child(div().text_color(rgb(ACCENT)).child(tf("update_available", &[&r.version])))
                        .child(div().flex_1().text_color(rgb(TEXT2)).text_size(px(12.))
                            .child(if busy { t("update_restart").into() }
                                else if running { t("update_later").into() }
                                else { self.update_msg.clone().unwrap_or_default() }))
                        .child(button("update", if busy { t("downloading") } else { t("update") }, enabled)
                            .when(enabled, |b| b.on_click(cx.listener(|v, _: &ClickEvent, _, cx| v.install_update(cx))))))
                })
                .children(disk)
                .child(queue))
            .child(footer)
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let running = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let bounds = Bounds::centered(None, size(px(1100.), px(640.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(1040.), px(440.))),
            titlebar: Some(gpui::TitlebarOptions { title: Some("Slimcast".into()), appears_transparent: true, ..Default::default() }),
            ..Default::default()
        };
        cx.open_window(options, |window, cx| {
            let (r, s) = (running.clone(), stop.clone());
            window.on_window_should_close(cx, move |window, cx| {
                if !r.load(Ordering::Relaxed) { return true }
                let answer = window.prompt(PromptLevel::Warning, t("running_title"),
                    Some(t("running_detail")), &[t("cancel_quit"), t("keep_going")], cx);
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
