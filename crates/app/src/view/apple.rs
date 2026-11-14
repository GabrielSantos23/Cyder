//! Player do Apple Music (doc 13): o music.apple.com num WebView2 escondido, que tem Widevine.
//! O site cuida do token e do login; o Cyder só fala com o MusicKit dele por JavaScript e recebe o estado pela `ipc`.
//! O WebView2 roda numa thread própria, com uma janela dele: criado dentro da janela do GPUI, o `wry` processa as
//! mensagens do Windows no meio de uma atualização do GPUI, e o GPUI entra nele mesmo (RefCell already borrowed).
//! A mesma janela vira o "Entrar no Apple Music" quando falta o login.
use gpui::Context;
use cyder_core::apple::{AppleCmd, AppleState};
use tao::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    platform::{run_return::EventLoopExtRunReturn, windows::EventLoopBuilderExtWindows},
    window::WindowBuilder,
};
use wry::{WebContext, WebViewBuilder, WebViewBuilderExtWindows};

use super::Cyder;
use cyder_core::Cmd;

const SITE: &str = "https://music.apple.com/";

/// Roda antes do site: liga os eventos do MusicKit à `ipc` e expõe `window.cyder` para os comandos.
const BRIDGE: &str = r#"
(() => {
  const send = m => window.ipc.postMessage(JSON.stringify(m));
  // A Apple nega licença ao Widevine do WebView2 (status -1021) e concede ao PlayReady, mas o site prefere o Widevine:
  // com o PlayReady disponível, o Widevine "não existe" e o MusicKit cai para ele.
  const keys = navigator.requestMediaKeySystemAccess.bind(navigator);
  navigator.requestMediaKeySystemAccess = (ks, c) => ks !== 'com.widevine.alpha' ? keys(ks, c)
    : keys('com.microsoft.playready', c).then(() => Promise.reject(new DOMException('PlayReady primeiro', 'NotSupportedError')), () => keys(ks, c));
  const instance = () => { try { return window.MusicKit && MusicKit.getInstance(); } catch { return null; } };
  // Comandos que chegam antes do MusicKit subir esperam por ele.
  const ready = () => new Promise(done => { const t = setInterval(() => { const mk = instance(); if (mk) { clearInterval(t); done(mk); } }, 200); });
  const run = f => ready().then(f).catch(e => send({ error: String(e && (e.message || e)) }));
  let last = 0, atEnd = null;
  const state = mk => {
    const it = mk.nowPlayingItem;
    const s = { authorized: mk.isAuthorized, playing: mk.isPlaying,
      position_ms: Math.round((mk.currentPlaybackTime || 0) * 1000), duration_ms: Math.round((mk.currentPlaybackDuration || 0) * 1000),
      song: it ? String(it.id) : null, storefront: mk.storefrontId || null,
      ended: [MusicKit.PlaybackStates.ended, MusicKit.PlaybackStates.completed].includes(mk.playbackState) };
    // No fim da fila o MusicKit zera a posição e tira a música sem passar por "ended": chegou ao fim e sumiu, acabou.
    if (s.song && s.duration_ms > 0 && s.position_ms > 0) atEnd = s.position_ms >= s.duration_ms - 2000 ? s.song : null;
    if (!s.song && atEnd) Object.assign(s, { song: atEnd, ended: true }), atEnd = null;
    send(s);
  };
  ready().then(mk => {
    // Logo depois do login o site ainda não sabe da assinatura e só toca a prévia (90 s); recarregado, toca a faixa inteira.
    const authorizedAtLoad = mk.isAuthorized;
    mk.addEventListener('authorizationStatusDidChange', () => { if (!authorizedAtLoad && mk.isAuthorized) location.reload(); });
    for (const ev of ['authorizationStatusDidChange', 'playbackStateDidChange', 'nowPlayingItemDidChange']) mk.addEventListener(ev, () => state(mk));
    // A posição muda várias vezes por segundo: uma por segundo basta.
    mk.addEventListener('playbackTimeDidChange', () => { const now = Date.now(); if (now - last >= 1000) { last = now; state(mk); } });
    mk.addEventListener('mediaPlaybackError', e => send({ error: String(e && (e.message || e.errorCode || e.name) || 'erro de reprodução') }));
    state(mk);
  });
  window.cyder = {
    play: (id, ms) => run(async mk => { await mk.setQueue({ song: id, startPlaying: true }); if (ms > 0) await mk.seekToTime(ms / 1000); }),
    resume: () => run(mk => mk.play()),
    toggle: () => run(mk => mk.isPlaying ? mk.pause() : mk.play()),
    pause: () => run(mk => mk.pause()),
    seek: ms => run(mk => mk.seekToTime(ms / 1000)),
    volume: v => run(mk => { mk.volume = v; }),
  };
})();
"#;

#[derive(Debug)]
pub(super) enum AppleEvent {
    State(AppleState),
    Error(String),
}

fn parse(msg: &str) -> Option<AppleEvent> {
    let v: serde_json::Value = serde_json::from_str(msg).ok()?;
    match v.get("error").and_then(|e| e.as_str()) {
        Some(e) => Some(AppleEvent::Error(e.into())),
        None => serde_json::from_value(v).ok().map(AppleEvent::State),
    }
}

/// Pedidos da UI para a thread do WebView2.
enum Msg {
    Js(String),
    Show(bool),
    Quit,
}

pub(super) struct ApplePlayer {
    proxy: EventLoopProxy<Msg>,
    pub state: AppleState,
}

impl Drop for ApplePlayer {
    fn drop(&mut self) {
        let _ = self.proxy.send_event(Msg::Quit);
    }
}

impl ApplePlayer {
    fn call(&self, js: &str) {
        let _ = self.proxy.send_event(Msg::Js(format!("window.cyder && window.cyder.{js}")));
    }

    /// Faz o que o engine pediu.
    fn run(&self, cmd: AppleCmd) {
        match cmd {
            // O ID vira texto JSON: nada do catálogo entra cru no script.
            AppleCmd::Play { song, start_ms } => self.call(&format!("play({}, {start_ms})", serde_json::Value::from(song))),
            AppleCmd::Pause => self.call("pause()"),
            AppleCmd::Resume => self.call("resume()"),
            AppleCmd::Seek(ms) => self.call(&format!("seek({ms})")),
            AppleCmd::Volume(v) => self.call(&format!("volume({})", v.clamp(0., 1.))),
        }
    }

    /// Mostra a janela do site (para entrar com o Apple ID) ou esconde de novo.
    pub fn show(&self, on: bool) {
        let _ = self.proxy.send_event(Msg::Show(on));
    }
}

/// A thread do WebView2: cria a janela e o site, e fica processando as mensagens até o `Quit`.
fn run(ready: std::sync::mpsc::Sender<Result<EventLoopProxy<Msg>, String>>, events: tokio::sync::mpsc::UnboundedSender<String>) {
    let mut el = EventLoopBuilder::<Msg>::with_user_event().with_any_thread(true).build();
    let window = match WindowBuilder::new().with_title("Entrar no Apple Music").with_visible(false).with_inner_size(tao::dpi::LogicalSize::new(1000., 760.)).build(&el) {
        Ok(w) => w,
        Err(e) => return drop(ready.send(Err(e.to_string()))),
    };
    let mut context = WebContext::new(Some(cyder_core::config::cache_dir().join("apple-webview")));
    let built = WebViewBuilder::new_with_web_context(&mut context)
        .with_url(SITE)
        .with_initialization_script(BRIDGE)
        .with_ipc_handler(move |req| drop(events.send(req.into_body())))
        .with_autoplay(true)
        // Os padrões do wry, mais as teclas de mídia: quem responde a elas e aparece no Windows é o Cyder, não o site.
        .with_additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,HardwareMediaKeyHandling")
        .build(&window);
    let webview = match built {
        Ok(w) => w,
        Err(e) => return drop(ready.send(Err(e.to_string()))),
    };
    if ready.send(Ok(el.create_proxy())).is_err() {
        return;
    }
    // `run_return`: o `run` do tao encerra o processo inteiro quando o laço acaba.
    el.run_return(|ev, _, flow| {
        *flow = ControlFlow::Wait;
        match ev {
            Event::UserEvent(Msg::Js(js)) => {
                if let Err(e) = webview.evaluate_script(&js) {
                    log::warn!("apple music: {e}");
                }
            }
            Event::UserEvent(Msg::Show(on)) => {
                window.set_visible(on);
                if on {
                    window.set_focus();
                }
            }
            Event::UserEvent(Msg::Quit) => *flow = ControlFlow::Exit,
            // Fechar a janela de login só esconde: o player continua.
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => window.set_visible(false),
            _ => {}
        }
    });
}

impl Cyder {
    /// Pedido do engine para o player; o primeiro abre o WebView2.
    pub(super) fn apple_cmd(&mut self, cmd: AppleCmd, cx: &mut Context<Self>) {
        self.start_apple(cx);
        if let Some(apple) = &self.apple {
            apple.run(cmd);
        }
    }

    /// Abre o player do Apple Music escondido.
    fn start_apple(&mut self, cx: &mut Context<Self>) {
        if self.apple.is_some() {
            return;
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        if let Err(e) = std::thread::Builder::new().name("apple-music".into()).spawn(move || run(ready_tx, tx)) {
            return log::error!("apple music: {e}");
        }
        let proxy = match ready_rx.recv() {
            Ok(Ok(p)) => p,
            Ok(Err(e)) => return log::error!("apple music: o WebView2 não abriu: {e}"),
            Err(_) => return log::error!("apple music: a thread do WebView2 parou"),
        };
        self.apple = Some(ApplePlayer { proxy, state: AppleState::default() });
        cx.spawn(async move |this, cx| {
            while let Some(msg) = rx.recv().await {
                let Some(ev) = parse(&msg) else { continue };
                if this.update(cx, |s, cx| { s.apple_event(ev); cx.notify(); }).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    fn apple_event(&mut self, ev: AppleEvent) {
        let Some(apple) = self.apple.as_mut() else { return };
        match ev {
            AppleEvent::Error(e) => {
                log::warn!("apple music: {e}");
                let state = AppleState { error: Some(e), ..apple.state.clone() };
                self.send(Cmd::Apple(state));
            }
            AppleEvent::State(state) => {
                if state.authorized != apple.state.authorized || state.playing != apple.state.playing || state.song != apple.state.song {
                    log::info!("apple music: {state:?}");
                }
                // Sem login, a janela aparece para a pessoa entrar; entrou, volta a ficar escondida.
                if state.authorized != apple.state.authorized || apple.state == AppleState::default() {
                    apple.show(!state.authorized);
                }
                apple.state = state.clone();
                self.send(Cmd::Apple(state));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bridge_messages() {
        let Some(AppleEvent::State(s)) = parse(r#"{"authorized":true,"playing":true,"position_ms":1500,"duration_ms":127000,"song":"123"}"#) else { panic!() };
        assert_eq!((s.authorized, s.playing, s.position_ms, s.duration_ms, s.song.as_deref()), (true, true, 1500, 127000, Some("123")));
        assert!(matches!(parse(r#"{"error":"sem licença"}"#), Some(AppleEvent::Error(e)) if e == "sem licença"));
        assert!(parse("lixo").is_none());
    }
}
