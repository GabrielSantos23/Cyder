//! Configurações: conta, reprodução, aparência, armazenamento e sobre. As de reprodução religam o player
//! (mesma fila e posição) um pouco depois da última mudança; as outras valem na hora.
use cyder_core::{i18n::t, tr};
use std::time::{Duration, Instant};

use cyder_core::{Cmd, config::{self, Accent, Language, Settings, Theme}, images};
use gpui::{AnyElement, Context, FontWeight, ObjectFit, ParentElement, SharedString, Styled, Window, div, img, prelude::*, px, rgba};

use crate::theme;
use crate::view::{Ask, Cyder, Dialog, Help, KEEP_PLAYING, Phase, Tip, icon};

/// Espera antes de religar o player: clicar em três opções seguidas religa uma vez só.
pub(in crate::view) const RESTART_AFTER: Duration = Duration::from_millis(900);
const MAX_W: f32 = 760.;

fn size_label(bytes: u64) -> String {
    let mb = bytes as f64 / (1024. * 1024.);
    if mb >= 1024. { format!("{:.1} GB", mb / 1024.).replace('.', ",") } else { format!("{} MB", mb.round() as u64) }
}

/// "2c38327a19a54c4f88c2b89af901246d" → "2c38…246d".
fn masked(id: &str) -> String {
    if id.len() > 8 { format!("{}…{}", &id[..4], &id[id.len() - 4..]) } else { id.to_string() }
}

fn plan_label(plan: &str) -> String {
    match plan {
        "premium" => "Spotify Premium".into(),
        "free" | "open" => "Spotify Free".into(),
        other => format!("Spotify {other}"),
    }
}

impl Cyder {
    /// Muda e salva. `restart`: é de reprodução, o player religa com ela.
    pub(in crate::view) fn change(&mut self, f: impl FnOnce(&mut Settings), restart: bool) {
        let before = self.settings.clone();
        f(&mut self.settings);
        if self.settings == before {
            return;
        }
        #[cfg(windows)]
        if self.settings.open_at_login != before.open_at_login {
            if let Err(e) = crate::autostart::set(self.settings.open_at_login) {
                self.settings.open_at_login = before.open_at_login;
                self.notify(tr!("Não deu para mudar a abertura com o Windows: {e}", "Couldn't change opening with Windows: {e}"));
                return;
            }
        }
        #[cfg(windows)]
        if self.settings.spotify_links != before.spotify_links {
            let done = if self.settings.spotify_links { crate::protocol::sync(true) } else { crate::protocol::release() };
            if let Err(e) = done {
                self.settings.spotify_links = before.spotify_links;
                self.notify(tr!("Não deu para mudar quem abre os links do Spotify: {e}", "Couldn't change what opens Spotify links: {e}"));
                return;
            }
        }
        config::save_settings(&self.settings);
        if self.settings.send_errors != before.send_errors {
            crate::reports::set(self.settings.send_errors == Some(true));
        }
        KEEP_PLAYING.store(self.settings.keep_playing_on_close, std::sync::atomic::Ordering::Relaxed);
        #[cfg(windows)]
        if (self.settings.global_hotkeys, &self.settings.hotkeys) != (before.global_hotkeys, &before.hotkeys) {
            self.apply_hotkeys();
        }
        if restart {
            self.restart_due = Some(Instant::now());
            self.playback_note = Some(t("Vale em instantes; a música pode parar por um segundo."));
        }
    }

    fn measure_storage(&mut self, cx: &mut Context<Self>) {
        self.storage = Some(0);
        cx.spawn(async move |this, cx| {
            let sizes = cx.background_executor().spawn(async {
                images::art_dirs().iter().map(|d| images::dir_size(d)).sum()
            }).await;
            let _ = this.update(cx, |s, cx| {
                s.storage = Some(sizes);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn settings_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if self.storage.is_none() {
            self.measure_storage(cx);
        }
        // Cada seção é um filho direto da rolagem: o índice no topo rola até ela (`scroll_to_item`).
        let mut sections: Vec<(&'static str, AnyElement)> = vec![
            (t("Conta"), self.account_section(cx)),
            (t("Reprodução"), self.playback_section(cx)),
            (t("Aparência"), self.appearance_section(cx)),
        ];
        #[cfg(windows)]
        sections.push((t("Atalhos"), self.hotkeys_section(window, cx)));
        sections.extend([
            (t("Integrações"), self.integrations_section(cx)),
            (t("Armazenamento"), self.storage_section(cx)),
            (t("Sobre"), self.about_section(cx)),
        ]);
        let handle = self.scroll_handle("settings");
        let index = div().pb_4().flex().flex_wrap().gap_2().children(sections.iter().enumerate().map(|(i, (name, _))| {
            let handle = handle.clone();
            Self::focusable(("settings-index", i))
                .h(px(30.))
                .px_3()
                .rounded_full()
                .flex()
                .items_center()
                .text_sm()
                .bg(rgba(theme::wash(0x12)))
                .hover(|s| s.bg(rgba(theme::wash(0x24))))
                .on_click(move |_, _, _| handle.scroll_to_top_of_item(i))
                .child(*name)
        }));
        let body = div()
            .id("settings-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&handle)
            .pb(px(super::LIST_BOTTOM))
            .flex()
            .flex_col()
            .gap_8()
            .children(sections.into_iter().map(|(_, el)| div().w_full().max_w(px(MAX_W)).child(el)));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().pb_3().text_size(px(28.)).font_weight(FontWeight::EXTRA_BOLD).child(t("Configurações")))
            .child(index)
            .child(super::super::scrollbar::scrolled(body, super::panel_bar("settings-bar", handle, false)))
            .into_any_element()
    }

    fn account_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (name, image) = self.profile.clone().unwrap_or_else(|| (t("Sua conta").into(), None));
        let avatar = match image.and_then(|u| self.thumbs_small.get(&u).cloned()) {
            Some(p) => img(p).size(px(52.)).flex_none().rounded_full().object_fit(ObjectFit::Cover).into_any_element(),
            None => div().size(px(52.)).flex_none().rounded_full().bg(rgba(theme::wash(0x1f))).flex().items_center().justify_center()
                .text_lg().font_weight(FontWeight::BOLD).child(name.chars().next().unwrap_or('?').to_uppercase().to_string()).into_any_element(),
        };
        let client_id = config::load().ok().map(|c| c.client_id).unwrap_or_default();
        let profile = div().flex().items_center().gap_4()
            .child(avatar)
            .child(
                div().flex_1().min_w_0().flex().flex_col()
                    .child(div().text_base().font_weight(FontWeight::BOLD).overflow_hidden().whitespace_nowrap().text_ellipsis().child(name))
                    .children(self.plan.as_deref().map(|p| div().text_sm().text_color(rgba(theme::ink_2())).child(plan_label(p)))),
            )
            .child(Self::secondary_button("settings-logout", t("Sair da conta"))
                .on_click(cx.listener(|t, _, _, cx| { t.dialog = Some(Dialog::Logout); cx.notify(); })));
        let current = client_id.clone();
        Self::section(t("Conta"), vec![
            profile.into_any_element(),
            Self::row(
                "Client ID",
                tr!("O app que você criou no painel do Spotify ({}). Trocar abre o navegador para entrar de novo.", "The app you created in the Spotify dashboard ({}). Changing it opens the browser to sign in again.", masked(&client_id)),
                Self::secondary_button("settings-client-id", t("Trocar")).on_click(cx.listener(move |t, _, _, cx| {
                    t.client_id = current.clone();
                    t.client_id_error = None;
                    t.setup_return = true;
                    t.focus_client_id = true;
                    t.onboard_step = crate::view::onboarding::STEP_CLIENT_ID;
                    t.phase = Phase::Setup;
                    cx.notify();
                })),
            ),
            Self::row(
                t("Painel do Spotify"),
                t("Onde ficam o seu app e o Client ID. Criar um é de graça e leva uns cinco minutos.").into(),
                Self::secondary_button("settings-dashboard", t("Abrir painel")).on_click(|_, _, cx| cx.open_url("https://developer.spotify.com/dashboard")),
            ),
        ])
    }

    fn playback_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.clone();
        let status = match self.player_status.clone() {
            None => Self::row(t("Status: conectado"), t("O Spotify toca pelo Soloist; o Apple Music, pelo player dele.").into(), div()),
            Some(why) => Self::row(
                t("Status: sem conexão"),
                // "Reconectando…" é nosso; o resto vem do librespot, em inglês: a frase amigável no lugar.
                if why.is_ascii() || why.contains('{') || why.contains("Connect") { crate::view::friendly_error(&why).1.to_string() } else { why },
                Self::primary_button("settings-retry", t("Tentar de novo")).on_click(cx.listener(|t, _, _, cx| {
                    t.playback_note = Some(cyder_core::i18n::t("Aplicando…"));
                    t.send(Cmd::RestartPlayer);
                    cx.notify();
                })),
            ),
        };
        let catalog = self.segmented("settings-catalog", [(false, "Spotify"), (true, "Apple Music")], s.apple_catalog, |st, on| st.apple_catalog = on, true, cx);
        let mut rows = vec![
            status,
            self.soloist_row(cx),
            Self::row(
                t("Tocar pelo Apple Music"),
                t("Toca a sua fila do Spotify pelo Apple Music, achando cada música nele. Precisa de assinatura do Apple Music; na primeira vez abre uma janela para entrar com o Apple ID.").into(),
                self.toggle("settings-apple-music", s.apple_music, |st| st.apple_music = !st.apple_music, true, cx),
            ),
            Self::row(t("Continuar tocando ao fechar a janela"), t("Fechar esconde o Cyder no ícone da bandeja, perto do relógio. Para sair, use Ctrl+Q ou Sair no ícone.").into(),
                self.toggle("settings-keep-playing", s.keep_playing_on_close, |st| st.keep_playing_on_close = !st.keep_playing_on_close, false, cx)),
            Self::row(t("Abrir com o Windows"), t("Abre o Cyder escondido no ícone da bandeja quando você entra no Windows, pronto para tocar.").into(),
                self.toggle("settings-open-at-login", s.open_at_login, |st| st.open_at_login = !st.open_at_login, false, cx)),
        ];
        // Com as duas contas, a pessoa escolhe de qual vêm a biblioteca, a tela inicial e a busca.
        if s.apple_music {
            rows.insert(3, Self::row(
                t("Biblioteca e busca"),
                t("De onde vêm as suas playlists, álbuns e artistas, a tela inicial e a busca. Curtidas e Novidades continuam do Spotify.").into(),
                catalog,
            ));
        }
        // Sem o app oficial do Spotify, os links já abrem no Cyder; com ele, só se a pessoa pedir.
        #[cfg(windows)]
        if crate::protocol::other_app() {
            rows.push(Self::row(t("Abrir links do Spotify no Cyder"), t("Links spotify: de sites e mensagens abrem aqui, e não no app do Spotify.").into(),
                self.toggle("settings-spotify-links", s.spotify_links, |st| st.spotify_links = !st.spotify_links, false, cx)));
        }
        if let Some(note) = self.playback_note {
            rows.push(div().text_sm().text_color(rgba(theme::ink_2())).child(note).into_any_element());
        }
        Self::section(t("Reprodução"), rows)
    }

    /// O Soloist: para contas em que o Spotify recusa o áudio ao player do Cyder.
    fn soloist_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use cyder_core::soloist::{DEVICE_NAME, KEY_PAGE, Status};
        let off = || Self::secondary_button("settings-soloist-off", t("Desligar")).on_click(cx.listener(|t, _, _, cx| { t.send(Cmd::SoloistKey(None)); cx.notify(); }));
        let detail = match self.soloist.clone() {
            Status::Off => {
                return self.help_row(
                    t("Player oficial do Spotify (Soloist)"),
                    Help::Soloist,
                    t("O Spotify toca pelo player oficial dele, o Soloist: sem ele ligado, as músicas do Spotify não tocam no Cyder. Precisa de uma chave sua (Premium).").into(),
                    div().flex().gap_2()
                        .child(Self::secondary_button("settings-soloist-page", t("Gerar chave")).on_click(|_, _, cx| cx.open_url(KEY_PAGE)))
                        .child(Self::secondary_button("settings-soloist-on", t("Ligar")).on_click(cx.listener(|t, _, _, cx| { t.ask(crate::view::Ask::SoloistKey, String::new()); cx.notify(); }))),
                    cx,
                );
            }
            Status::Starting(what) => what,
            Status::Pairing => tr!("Falta parear, uma vez só: no app do Spotify, na mesma rede, escolha “{DEVICE_NAME}” nos aparelhos.", "Pairing left, only once: in the Spotify app, on the same network, pick “{DEVICE_NAME}” under devices."),
            Status::Ready => t("Ligado: o Spotify toca pelo Soloist.").into(),
            Status::Failed(why) => why,
        };
        let off = off();
        self.help_row(t("Player oficial do Spotify (Soloist)"), Help::Soloist, detail, off, cx)
    }

    fn appearance_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let on = self.settings.cover_colors;
        let theme = self.segmented(
            "settings-theme",
            [(Theme::Dark, t("Escuro")), (Theme::Light, t("Claro")), (Theme::System, t("Igual ao sistema"))],
            self.settings.theme,
            |st, t| st.theme = t,
            false,
            cx,
        );
        let accent = self.segmented("settings-accent", [(Accent::Green, t("Verde")), (Accent::System, t("Do Windows"))], self.settings.accent, |st, a| st.accent = a, false, cx);
        let canvas = self.toggle("settings-canvas", self.settings.canvas, |st| st.canvas = !st.canvas, false, cx);
        let mut rows = vec![
            Self::row(t("Capa animada"), t("Mostra no modo imersivo o Canvas do Spotify ou, quando ele não tiver, a capa animada do Apple Music.").into(), canvas),
            Self::row(t("Tema"), t("Igual ao sistema segue o claro ou escuro do Windows. O modo imersivo fica sempre escuro.").into(), theme),
            Self::row(t("Cor de destaque"), t("Curtidas, botões ligados e a faixa tocando. A do Windows é ajustada para ficar legível.").into(), accent),
        ];
        // Lista que abre num menu (e não botões lado a lado): cabe mais idiomas depois.
        let language = Self::focusable("settings-language")
            .h(px(32.))
            .pl_4()
            .pr_3()
            .flex()
            .items_center()
            .gap_2()
            .rounded_full()
            .bg(rgba(theme::wash(if theme::light() { 0x2b } else { 0x1f })))
            .hover(|s| s.bg(rgba(theme::wash(if theme::light() { 0x3a } else { 0x2e }))))
            .text_sm()
            .font_weight(FontWeight::SEMIBOLD)
            .aria_label(t("Idioma"))
            .on_click(cx.listener(|t, e: &gpui::ClickEvent, _, cx| { t.open_menu(crate::view::menus::MenuTarget::Language, e.position()); cx.notify(); }))
            .child(match self.settings.language { Language::Portuguese => "Português", Language::English => "English" })
            .child(icon("chevron-down", 14., theme::ink_2()));
        rows.insert(0, Self::row(t("Idioma"), t("O idioma dos textos do Cyder. Os nomes de playlists e seções da tela inicial vêm do Spotify.").into(), language));
        let cover_frame = self.toggle("settings-cover-frame", self.settings.cover_frame, |st| st.cover_frame = !st.cover_frame, false, cx);
        rows.push(Self::row(t("Capa no fundo"), t("Mostra a capa da música, borrada, atrás do app inteiro. Nos álbuns e playlists, a capa deles.").into(), cover_frame));
        rows.push(Self::row(
            t("Cores da capa"),
            t("Pinta o fundo das páginas de álbum, playlist e podcast e o destaque do player com as cores da capa.").into(),
            self.toggle("settings-cover-colors", on, |st| st.cover_colors = !st.cover_colors, false, cx),
        ));
        // É uma janela do app, não uma integração: fica com o resto da aparência.
        let mini_on = self.mini.is_some();
        rows.push(Self::row(
            "Mini player",
            t("Uma janela pequena por cima das outras com a capa e os controles. Também abre pelo ícone da bandeja.").into(),
            Self::secondary_button("settings-mini", if mini_on { t("Fechar") } else { t("Abrir") }).on_click(cx.listener(|t, _, _, cx| { t.toggle_mini(cx); cx.notify(); })),
        ));
        Self::section(t("Aparência"), rows)
    }

    /// Atalhos globais: ligar e trocar a combinação de cada comando. Clicar num atalho espera a combinação nova.
    #[cfg(windows)]
    fn hotkeys_section(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        use crate::view::hotkeys::{NAMES, label};
        // Saiu do atalho sem apertar nada (clicou fora): volta a valer o que estava.
        if self.hotkeys.recording.is_some() && !self.hotkeys.focus.is_focused(window) {
            self.hotkeys.error = None;
            self.stop_recording_hotkey();
        }
        let on = self.settings.global_hotkeys;
        let mut rows = vec![Self::row(
            t("Usar atalhos globais"),
            t("Controlam a música com o Cyder escondido ou atrás de outras janelas, para teclados sem teclas de mídia.").into(),
            self.toggle("settings-hotkeys", on, |st| st.global_hotkeys = !st.global_hotkeys, false, cx),
        )];
        if on {
            for (i, name) in NAMES.into_iter().map(t).enumerate() {
                let recording = self.hotkeys.recording == Some(i);
                let taken = self.hotkeys.taken[i] && !recording;
                let keys = label(&self.settings.hotkeys[i]);
                let detail = if recording {
                    t("Aperte a combinação nova. Esc cancela.")
                } else if taken {
                    t("Outro programa já usa este atalho. Clique para trocar.")
                } else {
                    ""
                };
                let chip = Self::focusable(("settings-hotkey", i))
                    .min_w(px(150.))
                    .h(px(32.))
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.))
                    .bg(rgba(theme::wash(0x14)))
                    .hover(|s| s.bg(rgba(theme::wash(0x1f))))
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .aria_label(SharedString::from(tr!("{name}: {keys}. Trocar", "{name}: {keys}. Change")))
                    .when(taken, |d| d.border_color(rgba((theme::DANGER << 8) | 0xff)).text_color(rgba((theme::DANGER << 8) | 0xff)))
                    .when(recording, |d| {
                        d.track_focus(&self.hotkeys.focus)
                            .key_context("TextField")
                            .border_color(rgba(theme::ink_1()))
                            .on_key_down(cx.listener(|t, e: &gpui::KeyDownEvent, window, cx| t.hotkey_key(e, window, cx)))
                    })
                    .on_click(cx.listener(move |t, _, window, cx| {
                        if t.hotkeys.recording != Some(i) {
                            t.record_hotkey(i, window, cx);
                            cx.notify();
                        }
                    }))
                    .child(if recording { t("Aperte as teclas…").to_string() } else { keys });
                rows.push(Self::row(name, detail.into(), chip));
            }
            if let Some(error) = self.hotkeys.error.clone() {
                rows.push(div().text_sm().text_color(rgba((theme::DANGER << 8) | 0xff)).child(error).into_any_element());
            }
        }
        Self::section(t("Atalhos globais"), rows)
    }

    fn integrations_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let lastfm = match self.lastfm_user.clone() {
            Some(user) => self.help_row(
                "Last.fm",
                Help::Lastfm,
                tr!("Ligado como {user}. O que toca neste computador entra no seu histórico do Last.fm.", "Connected as {user}. What plays on this computer goes into your Last.fm history."),
                Self::secondary_button("settings-lastfm-off", t("Desligar")).on_click(cx.listener(|t, _, _, cx| {
                    t.send(Cmd::LastfmDisconnect);
                    cx.notify();
                })),
                cx,
            ),
            None => self.help_row(
                "Last.fm",
                Help::Lastfm,
                t("O que toca pelo Cyder não entra no histórico do Spotify; o Last.fm guarda o que você ouve. Precisa de uma chave de API do Last.fm, de graça.").into(),
                div().flex().gap_2()
                    .child(Self::secondary_button("settings-lastfm-create", t("Criar chave")).on_click(|_, _, cx| cx.open_url("https://www.last.fm/api/account/create")))
                    .child(Self::secondary_button("settings-lastfm-on", t("Ligar")).on_click(cx.listener(|t, _, _, cx| { t.ask(Ask::LastfmKey, String::new()); cx.notify(); }))),
                cx,
            ),
        };
        let has_app = !self.settings.discord_app_id.trim().is_empty();
        let discord_control = if has_app {
            div().flex().items_center().gap_3()
                .child(Self::secondary_button("settings-discord-app", t("Trocar app")).on_click(cx.listener(|t, _, _, cx| {
                    let id = t.settings.discord_app_id.clone();
                    t.ask(Ask::DiscordApp, id);
                    cx.notify();
                })))
                .child(self.toggle("settings-discord", self.settings.discord, |st| st.discord = !st.discord, false, cx))
                .into_any_element()
        } else {
            div().flex().gap_2()
                .child(Self::secondary_button("settings-discord-portal", t("Abrir portal")).on_click(|_, _, cx| cx.open_url("https://discord.com/developers/applications")))
                .child(Self::secondary_button("settings-discord-on", t("Configurar")).on_click(cx.listener(|t, _, _, cx| { t.ask(Ask::DiscordApp, String::new()); cx.notify(); })))
                .into_any_element()
        };
        let discord_detail = if has_app {
            t("Mostra no seu perfil do Discord o que está tocando, com a capa e o tempo. Precisa do Discord aberto neste computador.")
        } else {
            t("Mostra no seu perfil do Discord o que está tocando. Precisa de um aplicativo seu no portal do Discord (o nome dele aparece como “Ouvindo …”).")
        };
        let mut rows = vec![lastfm];
        // Só faz sentido com as duas coisas: o Last.fm do Cyder e o Apple Music como saída.
        if self.lastfm_user.is_some() && self.settings.apple_music {
            rows.push(Self::row(
                t("Last.fm só pelo Apple Music"),
                t("Manda ao Last.fm só o que toca pelo Apple Music. Use se a sua conta do Spotify já está ligada ao Last.fm, para não contar duas vezes.").into(),
                self.toggle("settings-lastfm-apple", self.settings.lastfm_apple_only, |st| st.lastfm_apple_only = !st.lastfm_apple_only, false, cx),
            ));
        }
        rows.extend([
            self.help_row("Discord", Help::Discord, discord_detail.into(), discord_control, cx),
            Self::row(
                t("Avisar a troca de música"),
                {
                    #[cfg(windows)]
                    let off = self.settings.toasts && crate::view::toast::blocked();
                    #[cfg(not(windows))]
                    let off = false;
                    if off {
                        t("As notificações estão desligadas no Windows. Ligue em Configurações › Sistema › Notificações.").into()
                    } else {
                        t("Mostra uma notificação do Windows com a capa e o botão Pular quando a música muda e o Cyder não está na frente.").into()
                    }
                },
                self.toggle("settings-toasts", self.settings.toasts, |st| st.toasts = !st.toasts, false, cx),
            ),
        ]);
        Self::section(t("Integrações"), rows)
    }

    fn storage_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let art = self.storage.unwrap_or_default();
        let cache = config::cache_dir();
        let reveal = |id: &'static str, path: std::path::PathBuf| {
            Self::secondary_button(id, t("Mostrar pasta")).on_click(move |_, _, cx| cx.reveal_path(&path))
        };
        let art_row = if images::clear_requested() {
            Self::row(t("Capas"), t("Serão apagadas na próxima vez que o Cyder abrir.").into(), div())
        } else {
            Self::row(
                t("Capas"),
                tr!("{} em {}", "{} in {}", size_label(art), cache.display()),
                Self::secondary_button("settings-clear-art", t("Limpar capas")).on_click(cx.listener(|t, _, _, cx| {
                    // Apagar agora deixaria as capas da tela quebradas; o core apaga ao abrir.
                    images::request_clear();
                    t.notify(cyder_core::i18n::t("As capas serão apagadas na próxima vez que o Cyder abrir."));
                    cx.notify();
                })),
            )
        };
        Self::section(t("Armazenamento"), vec![
            art_row,
            Self::row("Login", tr!("Guardado em {}", "Stored in {}", config::credentials_dir().display()), reveal("settings-login-dir", config::credentials_dir())),
        ])
    }

    fn about_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let config_dir = config::config_dir();
        let reports = crate::reports::available().then(|| Self::row(t("Enviar relatórios de erro"), t(crate::view::REPORTS_TEXT).into(),
            self.toggle("settings-reports", self.settings.send_errors == Some(true), |st| st.send_errors = Some(st.send_errors != Some(true)), false, cx)));
        let mut rows = vec![
            div().flex().items_center().gap_4()
                .child(crate::view::logo(48.))
                .child(
                    div().flex().flex_col()
                        .child(div().text_base().font_weight(FontWeight::BOLD).child(concat!("Cyder ", env!("CARGO_PKG_VERSION"))))
                        .child(div().text_sm().text_color(rgba(theme::ink_2())).child(t("Feito com Rust, GPUI e librespot. Não é afiliado ao Spotify."))),
                )
                .into_any_element(),
            self.update_row(cx),
        ];
        rows.extend(reports);
        rows.push(
            div().flex().gap_2()
                .child(Self::secondary_button("settings-shortcuts", t("Atalhos do teclado"))
                    .on_click(cx.listener(|t, _, _, cx| { t.dialog = Some(Dialog::Shortcuts); cx.notify(); })))
                .child(Self::secondary_button("settings-config-dir", t("Pasta de configuração")).on_click(move |_, _, cx| cx.reveal_path(&config_dir)))
                // O que mandar quando algo der errado: o log desta abertura (e o da anterior, na mesma pasta).
                .child(Self::secondary_button("settings-log", t("Registro")).on_click(|_, _, cx| cx.reveal_path(&config::log_path())))
                .into_any_element(),
        );
        Self::section(t("Sobre"), rows)
    }

    /// Título e o cartão com as linhas.
    fn section(title: &'static str, rows: Vec<AnyElement>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().text_lg().font_weight(FontWeight::BOLD).child(title))
            .child(
                div().px_5().py_4().rounded(px(16.)).bg(rgba(theme::wash(0x0a))).border_1().border_color(rgba(theme::wash(0x12)))
                    .flex().flex_col().gap_5().children(rows),
            )
            .into_any_element()
    }

    /// Como `row`, com o botão (i) ao lado do nome que abre o passo a passo.
    fn help_row(&mut self, title: &'static str, help: Help, detail: String, control: impl IntoElement, cx: &mut Context<Self>) -> AnyElement {
        let info = Self::focusable(("settings-help", help as usize))
            .size(px(20.))
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .hover(|s| s.bg(rgba(theme::wash(0x1a))))
            .tip(match help { Help::Lastfm => t("Como ligar o Last.fm"), Help::Discord => t("Como ligar o Discord"), Help::Soloist => t("Como ligar o Soloist") })
            .on_click(cx.listener(move |t, _, _, cx| { t.dialog = Some(Dialog::Help(help)); cx.notify(); }))
            .child(icon("info", 14., theme::ink_2()));
        div()
            .flex()
            .items_center()
            .gap_6()
            .child(
                div().flex_1().min_w_0().flex().flex_col().gap_0p5()
                    .child(div().flex().items_center().gap_1().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title)).child(info))
                    .child(div().text_xs().text_color(rgba(theme::ink_2())).child(detail)),
            )
            .child(div().flex_none().child(control))
            .into_any_element()
    }

    /// Nome e explicação à esquerda, controle à direita.
    pub(in crate::view) fn row(title: &'static str, detail: String, control: impl IntoElement) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_6()
            .child(
                div().flex_1().min_w_0().flex().flex_col().gap_0p5()
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().text_xs().text_color(rgba(theme::ink_2())).child(detail)),
            )
            .child(div().flex_none().child(control))
            .into_any_element()
    }

    fn toggle(&mut self, id: &'static str, on: bool, flip: fn(&mut Settings), restart: bool, cx: &mut Context<Self>) -> AnyElement {
        // Sobre o destaque do Windows, a bolinha segue a cor que contrasta (escura num azul claro, como no Windows 11).
        let knob = if on && self.settings.accent == Accent::System { theme::on_accent() } else { 0xffffffff };
        Self::focusable(id)
            .w(px(40.))
            .h(px(22.))
            .p(px(2.))
            .rounded_full()
            .flex()
            .items_center()
            .when(on, |d| d.justify_end().bg(rgba(theme::accent())))
            // Desligado com trilho mais forte: o 0x26 ficava com ~1,3:1 contra o cartão e o controle sumia.
            .when(!on, |d| d.bg(rgba(theme::wash(0x47))).hover(|s| s.bg(rgba(theme::wash(0x5c)))))
            .aria_label(if on { t("Ligado") } else { t("Desligado") })
            .aria_selected(on)
            .on_click(cx.listener(move |t, _, _, cx| { t.change(flip, restart); cx.notify(); }))
            .child(div().size(px(18.)).rounded_full().bg(rgba(knob)).shadow_sm())
            .into_any_element()
    }

    /// Opções lado a lado; a escolhida em destaque, como os filtros. `restart`: é de reprodução.
    fn segmented<T: Copy + PartialEq + 'static, const N: usize>(
        &mut self,
        id: &'static str,
        options: [(T, &'static str); N],
        selected: T,
        set: fn(&mut Settings, T),
        restart: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Num trilho, como o seletor segmentado do Windows: as opções não escolhidas não parecem texto solto.
        div()
            .flex()
            .gap_0p5()
            .p(px(3.))
            .rounded_full()
            .bg(rgba(theme::wash(0x12)))
            .border_1()
            .border_color(rgba(theme::wash(0x14)))
            .children(options.into_iter().enumerate().map(|(i, (value, label))| {
                let on = value == selected;
                Self::focusable((id, i))
                    .h(px(28.))
                    .px_3()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .aria_selected(on)
                    .when(on, |d| d.bg(rgba(theme::ink_1())).text_color(rgba(theme::on_ink())))
                    .when(!on, |d| d.hover(|s| s.bg(rgba(theme::wash(0x1a)))))
                    .on_click(cx.listener(move |t, _, _, cx| { t.change(|st| set(st, value), restart); cx.notify(); }))
                    .child(label)
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn labels() {
        assert_eq!(super::size_label(5 * 1024 * 1024), "5 MB");
        assert_eq!(super::size_label(3 * 1024 * 1024 * 1024 / 2), "1,5 GB");
        assert_eq!(super::masked("2c38327a19a54c4f88c2b89af901246d"), "2c38…246d");
    }
}
