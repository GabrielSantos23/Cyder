// Spike descartável: as mesmas telas do spike Tauri (lista de 5 000 + imersivo) em GPUI, para comparar RAM.
// Sem áudio: o librespot mediu ~12 MB no spike Tauri e seria o mesmo aqui.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use gpui::{
    App, Application, Bounds, Context, FontWeight, ObjectFit, SharedString, Window, WindowBounds,
    WindowOptions, div, img, prelude::*, px, relative, rgba, size, uniform_list,
    ScrollStrategy, UniformListScrollHandle,
};
use serde_json::Value;

const ROWS: usize = 5000;
const LINE_H: f32 = 84.0;

struct Track {
    name: SharedString,
    sub: SharedString,
    dur: SharedString,
    cover64: PathBuf,
}

#[derive(PartialEq)]
enum View {
    Library,
    Immersive,
}

struct Spike {
    tracks: Vec<Track>,
    view: View,
    scroll: UniformListScrollHandle,
    lines: Vec<(u64, SharedString)>,
    current: usize,
    cover640: PathBuf,
    background: PathBuf,
    started: Option<Instant>,
}

fn out_dir() -> PathBuf {
    let d = std::env::temp_dir().join("cyder-spike-gpui");
    let _ = fs::create_dir_all(d.join("covers"));
    d
}

fn mark(phase: &str) {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(out_dir().join("marks.txt")) {
        let _ = writeln!(f, "{ms} {phase}");
    }
}

fn download(url: &str, to: &Path) {
    if to.exists() {
        return;
    }
    if let Ok(mut r) = ureq::get(url).call() {
        if let Ok(bytes) = r.body_mut().read_to_vec() {
            let _ = fs::write(to, bytes);
        }
    }
}

fn lrclib(t: &Value) -> Option<String> {
    let secs = ((t["duration_ms"].as_u64()? + 500) / 1000).to_string();
    for _ in 0..5 {
        let res = ureq::get("https://lrclib.net/api/get")
            .header("User-Agent", "cyder-spike/0.1")
            .query("track_name", t["name"].as_str()?)
            .query("artist_name", t["artists"][0]["name"].as_str()?)
            .query("album_name", t["album"]["name"].as_str()?)
            .query("duration", &secs)
            .config().http_status_as_error(false).build()
            .call()
            .ok()?;
        if res.status() == 429 {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        let v: Value = serde_json::from_reader(res.into_body().into_reader()).ok()?;
        return v["syncedLyrics"].as_str().map(String::from);
    }
    None
}

fn parse_lrc(lrc: &str) -> Vec<(u64, SharedString)> {
    let mut out = Vec::new();
    for raw in lrc.lines() {
        let Some(rest) = raw.strip_prefix('[') else { continue };
        let Some((tag, text)) = rest.split_once(']') else { continue };
        let Some((m, s)) = tag.split_once(':') else { continue };
        let (Ok(m), Ok(s)) = (m.parse::<f64>(), s.parse::<f64>()) else { continue };
        let text = if text.trim().is_empty() { "…" } else { text.trim() };
        out.push((((m * 60.0 + s) * 1000.0) as u64, SharedString::from(text.to_string())));
    }
    out
}

fn fmt(ms: u64) -> String {
    format!("{}:{:02}", ms / 60000, ms / 1000 % 60)
}

/// Prepara os dados antes de abrir a janela (fora da medição): capas em disco, fundo pré-borrado, letra.
fn load(json: &Path) -> Spike {
    let raw: Vec<Value> = serde_json::from_str(&fs::read_to_string(json).expect("liked_tracks.json")).unwrap();
    let dir = out_dir();
    let tracks: Vec<Track> = raw
        .iter()
        .enumerate()
        .map(|(i, t)| Track {
            name: t["name"].as_str().unwrap_or_default().to_string().into(),
            sub: format!("{} — {}", t["artists"][0]["name"].as_str().unwrap_or_default(), t["album"]["name"].as_str().unwrap_or_default()).into(),
            dur: fmt(t["duration_ms"].as_u64().unwrap_or(0)).into(),
            cover64: dir.join("covers").join(format!("{i}.jpg")),
        })
        .collect();
    let pairs: Vec<_> = raw.iter().zip(&tracks).collect();
    std::thread::scope(|s| {
        for chunk in pairs.chunks(25) {
            s.spawn(move || {
                for (t, tr) in chunk {
                    if let Some(u) = t["album"]["images"].as_array().and_then(|i| i.last()).and_then(|i| i["url"].as_str()) {
                        download(u, &tr.cover64);
                    }
                }
            });
        }
    });

    let (current, lyrics) = raw.iter().enumerate().take(30).find_map(|(i, t)| lrclib(t).map(|l| (i, l))).unwrap_or((0, String::new()));
    let cover640 = dir.join("cover640.jpg");
    let _ = fs::remove_file(&cover640);
    download(raw[current]["album"]["images"][0]["url"].as_str().unwrap_or_default(), &cover640);
    // GPUI não tem filtro de blur: borra a capa de 64 px no Rust e a GPU só estica (doc 03 regra 3).
    let background = dir.join("background.png");
    if let Ok(im) = image::open(&tracks[current].cover64) {
        let _ = im.blur(6.0).save(&background);
    }

    Spike {
        lines: parse_lrc(&lyrics),
        tracks,
        view: View::Library,
        scroll: UniformListScrollHandle::new(),
        current,
        cover640,
        background,
        started: None,
    }
}

impl Spike {
    /// Mesmo roteiro do spike Tauri: parado 10 s → rolando 30 s → imersivo 30 s.
    fn script(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let exec = cx.background_executor().clone();
            let wait = |ms| exec.timer(Duration::from_millis(ms));
            mark("start");
            wait(10_000).await;
            mark("library");
            for i in 1..=30 {
                let _ = this.update(cx, |s, cx| {
                    s.scroll.scroll_to_item(i * 27, ScrollStrategy::Top);
                    cx.notify();
                });
                wait(1000).await;
            }
            mark("immersive");
            let _ = this.update(cx, |s, cx| {
                s.view = View::Immersive;
                s.started = Some(Instant::now());
                cx.notify();
            });
            // 30 s re-renderizando a 10 fps, como a letra avançando.
            for _ in 0..300 {
                wait(100).await;
                let _ = this.update(cx, |_, cx| cx.notify());
            }
            mark("done");
        })
        .detach();
    }

    fn position(&self) -> u64 {
        let first = self.lines.first().map(|l| l.0).unwrap_or(0);
        first + self.started.map(|s| s.elapsed().as_millis() as u64).unwrap_or(0)
    }

    fn library(&self, cx: &mut Context<Self>) -> impl IntoElement {
        uniform_list(
            "library",
            ROWS,
            cx.processor(|this, range: std::ops::Range<usize>, _w, _cx| {
                range
                    .map(|ix| {
                        let t = &this.tracks[ix % this.tracks.len()];
                        div()
                            .id(ix)
                            .h(px(52.))
                            .flex()
                            .items_center()
                            .gap_3()
                            .px_3()
                            .rounded(px(12.))
                            .hover(|s| s.bg(rgba(0xffffff0d)))
                            .child(div().w(px(40.)).text_xs().text_color(rgba(0xffffff73)).child(format!("{}", ix + 1)))
                            .child(img(t.cover64.clone()).size(px(40.)).rounded(px(6.)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(div().text_sm().overflow_hidden().whitespace_nowrap().text_ellipsis().child(t.name.clone()))
                                    .child(div().text_xs().text_color(rgba(0xffffffb3)).overflow_hidden().whitespace_nowrap().text_ellipsis().child(t.sub.clone())),
                            )
                            .child(div().text_xs().text_color(rgba(0xffffff73)).child(t.dur.clone()))
                    })
                    .collect()
            }),
        )
        .track_scroll(self.scroll.clone())
        .size_full()
        .px_6()
        .pt_6()
    }

    fn immersive(&self) -> impl IntoElement {
        let pos = self.position();
        let idx = self.lines.iter().rposition(|l| l.0 <= pos).unwrap_or(0);
        div()
            .size_full()
            .relative()
            .child(img(self.background.clone()).absolute().inset_0().size_full().object_fit(ObjectFit::Cover))
            .child(div().absolute().inset_0().bg(rgba(0x00000073)))
            .child(
                div().absolute().top(px(40.)).bottom(px(40.)).left(px(40.)).w(relative(0.38)).flex().items_center()
                    .child(img(self.cover640.clone()).size(px(500.)).rounded(px(24.)).shadow_2xl()),
            )
            .child(
                div()
                    .absolute().top(px(40.)).bottom(px(40.)).right(px(40.)).w(relative(0.48))
                    .rounded(px(24.)).overflow_hidden()
                    .bg(rgba(0x1414188c)).border_1().border_color(rgba(0xffffff14))
                    .child(
                        div().absolute().left_0().right_0().top(relative(0.3)).px(px(48.))
                            .mt(px(-(idx as f32) * LINE_H))
                            .children(self.lines.iter().enumerate().map(|(i, (_, text))| {
                                let d = i.abs_diff(idx) as f32;
                                // GPUI não borra texto: só a opacidade cai com a distância.
                                let opacity = if d == 0. { 1. } else { (0.45 - d * 0.08).max(0.12) };
                                div().h(px(LINE_H)).flex().items_center().text_size(px(36.)).font_weight(FontWeight::EXTRA_BOLD)
                                    .opacity(opacity).child(text.clone())
                            })),
                    ),
            )
    }

    fn player(&self) -> impl IntoElement {
        let t = &self.tracks[self.current];
        div().absolute().bottom(px(16.)).left_0().right_0().flex().justify_center().child(
            div()
                .w(px(900.)).flex().items_center().gap_4().px_5().py_2().rounded_full()
                .bg(rgba(0x1414188c)).border_1().border_color(rgba(0xffffff14))
                .child(div().text_xl().child("❚❚"))
                .child(div().text_xs().text_color(rgba(0xffffffb3)).child(fmt(self.position())))
                .child(
                    div().flex_1().min_w_0().px_3().py_1().rounded(px(8.)).bg(rgba(0xffffff0d))
                        .child(div().text_sm().child(t.name.clone()))
                        .child(div().text_xs().text_color(rgba(0xffffffb3)).child(t.sub.clone())),
                ),
        )
    }
}

impl Render for Spike {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let immersive = self.view == View::Immersive;
        div()
            .size_full()
            .flex()
            .bg(rgba(0x0b0b0eff))
            .text_color(rgba(0xffffffff))
            .font_family("Segoe UI")
            .when(!immersive, |d| {
                d.child(
                    div().w(px(224.)).h_full().p_3().flex().flex_col().gap_1()
                        .border_r_1().border_color(rgba(0xffffff14)).bg(rgba(0x0000004d))
                        .child(div().px_3().py_1().text_sm().rounded(px(8.)).bg(rgba(0xffffff1a)).child("Biblioteca (5 000)"))
                        .child(div().px_3().py_1().text_sm().text_color(rgba(0xffffffb3)).child("Imersivo")),
                )
            })
            .child(
                div().flex_1().h_full().relative()
                    .map(|d| if immersive { d.child(self.immersive()) } else { d.child(self.library(cx)) })
                    .child(self.player()),
            )
    }
}

fn main() {
    let json = std::env::args().nth(1).map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("cyder-spike-run2").join("liked_tracks.json"));
    let _ = fs::remove_file(out_dir().join("marks.txt"));
    let spike = load(&json);
    mark("ready");
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1400.), px(860.)), cx);
        cx.open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            move |_, cx| {
                cx.new(|cx| {
                    Spike::script(cx);
                    spike
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
