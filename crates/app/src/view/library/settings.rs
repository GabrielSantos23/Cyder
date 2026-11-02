//! Configurações: conta, reprodução, aparência, armazenamento e sobre. As de reprodução religam o player
//! (mesma fila e posição) um pouco depois da última mudança; as outras valem na hora.
use std::time::{Duration, Instant};

use cyder_core::{Cmd, config::{self, Quality, Settings, Theme}, images};
use gpui::{AnyElement, Context, FontWeight, ObjectFit, ParentElement, SharedString, Styled, Window, div, img, prelude::*, px, rgba};

use super::LIKED_GREEN;
use crate::theme;
use crate::view::{Ask, Cyder, Dialog, Help, KEEP_PLAYING, Phase, Tip, icon};

/// Espera antes de religar o player: clicar em três opções seguidas religa uma vez só.
pub(in crate::view) const RESTART_AFTER: Duration = Duration::from_millis(900);
const MAX_W: f32 = 760.;
const CACHE_SIZES: [(u32, &str); 3] = [(512, "512 MB"), (1024, "1 GB"), (4096, "4 GB")];

fn size_label(bytes: u64) -> String {
    let mb = bytes as f64 / (1024. * 1024.);
    if mb >= 1024. { format!("{:.1} GB", mb / 1024.).replace('.', ",") } else { format!("{} MB", mb.round() as u64) }
}

/// "2c38327a19a54c4f88c2b89af901246d" → "2c38…246d".
fn masked(id: &str) -> String {
    if id.len() > 8 { format!("{}…{}", &id[..4], &id[id.len() - 4..]) } else { id.to_string() }
}

/// Opção que não vale agora (o som não sai do player do Cyder): apagada e sem responder ao mouse.
fn dimmed(off: bool, row: AnyElement) -> AnyElement {
    if !off {
        return row;
    }
    div().relative().opacity(0.4).child(row)
        .child(div().absolute().top_0().left_0().size_full().block_mouse_except_scroll())
        .into_any_element()
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
                self.notify(format!("Não deu para mudar a abertura com o Windows: {e}"));
                return;
            }
        }
        config::save_settings(&self.settings);
        if self.settings.send_errors != before.send_errors {
            crate::reports::set(self.settings.send_errors == Some(true));
        }
        cyder_core::fx::set(self.settings.fx());
        KEEP_PLAYING.store(self.settings.keep_playing_on_close, std::sync::atomic::Ordering::Relaxed);
        if restart {
            self.restart_due = Some(Instant::now());
            self.playback_note = Some("Vale em instantes; a música pode parar por um segundo.");
        }
    }

    fn measure_storage(&mut self, cx: &mut Context<Self>) {
        self.storage = Some((0, 0));
        cx.spawn(async move |this, cx| {
            let sizes = cx.background_executor().spawn(async {
                let art = images::art_dirs().iter().map(|d| images::dir_size(d)).sum();
                (art, images::dir_size(&config::audio_cache_dir()))
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
        let body = div()
            .w_full()
            .max_w(px(MAX_W))
            .flex()
            .flex_col()
            .gap_8()
            .child(self.account_section(cx))
            .child(self.playback_section(window, cx))
            .child(self.sound_section(cx))
            .child(self.appearance_section(cx))
            .child(self.integrations_section(cx))
            .child(self.storage_section(cx))
            .child(self.about_section(cx));
        let handle = self.scroll_handle("settings");
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(div().pb_4().text_size(px(24.)).font_weight(FontWeight::BOLD).child("Configurações"))
            .child(super::super::scrollbar::scrolled(
                div().id("settings-scroll").flex_1().min_h_0().overflow_y_scroll().track_scroll(&handle).pb(px(super::LIST_BOTTOM)).child(body),
                super::panel_bar("settings-bar", handle, false),
            ))
            .into_any_element()
    }

    fn account_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (name, image) = self.profile.clone().unwrap_or_else(|| ("Sua conta".into(), None));
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
            .child(Self::secondary_button("settings-logout", "Sair da conta")
                .on_click(cx.listener(|t, _, _, cx| { t.dialog = Some(Dialog::Logout); cx.notify(); })));
        let current = client_id.clone();
        Self::section("Conta", vec![
            profile.into_any_element(),
            Self::row(
                "Client ID",
                format!("O app que você criou no painel do Spotify ({}). Trocar abre o navegador para entrar de novo.", masked(&client_id)),
                Self::secondary_button("settings-client-id", "Trocar").on_click(cx.listener(move |t, _, _, cx| {
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
                "Painel do Spotify",
                "Onde ficam o seu app e o Client ID. Criar um é de graça e leva uns cinco minutos.".into(),
                Self::secondary_button("settings-dashboard", "Abrir painel").on_click(|_, _, cx| cx.open_url("https://developer.spotify.com/dashboard")),
            ),
        ])
    }

    fn playback_section(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.clone();
        let status = match self.player_status.clone() {
            None => Self::row("Status: conectado", format!("Aparece como “{}” no Spotify Connect.", s.device_name()), div()),
            Some(why) => Self::row(
                "Status: sem conexão",
                why,
                Self::primary_button("settings-retry", "Tentar de novo").on_click(cx.listener(|t, _, _, cx| {
                    t.playback_note = Some("Aplicando…");
                    t.send(Cmd::RestartPlayer);
                    cx.notify();
                })),
            ),
        };
        let quality = self.segmented(
            "settings-quality",
            [(Quality::Normal, "Normal"), (Quality::High, "Alta"), (Quality::VeryHigh, "Muito alta")],
            s.quality,
            |st, q| st.quality = q,
            true,
            cx,
        );
        let cache_sizes = self.segmented("settings-cache-size", CACHE_SIZES, s.audio_cache_mb, |st, mb| st.audio_cache_mb = mb, true, cx);
        let why = self.sound_elsewhere();
        let off = why.is_some();
        let mut rows = vec![
            status,
            self.soloist_row(cx),
            Self::row("Nome do aparelho", "Como este computador aparece no Spotify Connect. Enter para salvar.".into(), self.device_field(window, cx)),
            dimmed(off, Self::row("Qualidade do áudio", "Mais qualidade usa mais internet e mais espaço no cache.".into(), quality)),
            dimmed(off, Self::row("Normalizar volume", "Deixa músicas altas e baixas num volume parecido.".into(),
                self.toggle("settings-normalize", s.normalize, |st| st.normalize = !st.normalize, true, cx))),
            dimmed(off, Self::row("Autoplay", "Continua com músicas parecidas quando a lista acaba.".into(),
                self.toggle("settings-autoplay", s.autoplay, |st| st.autoplay = !st.autoplay, true, cx))),
            dimmed(off, Self::row("Sem pausa entre faixas", "Emenda uma faixa na outra, sem silêncio.".into(),
                self.toggle("settings-gapless", s.gapless, |st| st.gapless = !st.gapless, true, cx))),
            Self::row("Continuar tocando ao fechar a janela", "Fechar esconde o Cyder no ícone da bandeja, perto do relógio. Para sair, use Ctrl+Q ou Sair no ícone.".into(),
                self.toggle("settings-keep-playing", s.keep_playing_on_close, |st| st.keep_playing_on_close = !st.keep_playing_on_close, false, cx)),
            Self::row("Abrir com o Windows", "Abre o Cyder escondido no ícone da bandeja quando você entra no Windows, pronto para tocar.".into(),
                self.toggle("settings-open-at-login", s.open_at_login, |st| st.open_at_login = !st.open_at_login, false, cx)),
            dimmed(off, Self::row(
                "Cache de áudio",
                "Guarda o áudio baixado para não baixar de novo ao repetir.".into(),
                div().flex().items_center().gap_3()
                    .when(s.audio_cache, |d| d.child(cache_sizes))
                    .child(self.toggle("settings-cache", s.audio_cache, |st| st.audio_cache = !st.audio_cache, true, cx)),
            )),
        ];
        // Com o Soloist, a linha dele já explica; com outro aparelho, explica aqui.
        if let Some(why) = why.filter(|_| self.soloist != cyder_core::soloist::Status::Ready) {
            rows.insert(2, div().text_sm().text_color(rgba(theme::ink_2())).child(why).into_any_element());
        }
        if let Some(note) = self.playback_note {
            rows.push(div().text_sm().text_color(rgba(theme::ink_2())).child(note).into_any_element());
        }
        Self::section("Reprodução neste computador", rows)
    }

    /// O Soloist: para contas em que o Spotify recusa o áudio ao player do Cyder.
    fn soloist_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        use cyder_core::soloist::{DEVICE_NAME, KEY_PAGE, Status};
        let off = || Self::secondary_button("settings-soloist-off", "Desligar").on_click(cx.listener(|t, _, _, cx| { t.send(Cmd::SoloistKey(None)); cx.notify(); }));
        let detail = match self.soloist.clone() {
            Status::Off => {
                return self.help_row(
                    "Player oficial do Spotify (Soloist)",
                    Help::Soloist,
                    "Toca pelo player oficial do Spotify quando ele recusa o áudio desta conta ao Cyder. Precisa de uma chave sua (Premium).".into(),
                    div().flex().gap_2()
                        .child(Self::secondary_button("settings-soloist-page", "Gerar chave").on_click(|_, _, cx| cx.open_url(KEY_PAGE)))
                        .child(Self::primary_button("settings-soloist-on", "Ligar").on_click(cx.listener(|t, _, _, cx| { t.ask(crate::view::Ask::SoloistKey, String::new()); cx.notify(); }))),
                    cx,
                );
            }
            Status::Starting(what) => what,
            Status::Pairing => format!("Falta parear, uma vez só: no app do Spotify, na mesma rede, escolha “{DEVICE_NAME}” nos aparelhos."),
            Status::Ready => "Ligado: o Cyder toca pelo Soloist. As opções de som apagadas não valem nele.".into(),
            Status::Failed(why) => why,
        };
        let off = off();
        self.help_row("Player oficial do Spotify (Soloist)", Help::Soloist, detail, off, cx)
    }

    /// O som não sai do player do Cyder: pelo Soloist (pareado, é por onde tudo toca) ou outro aparelho.
    /// O texto diz por quê; None quando as opções de som valem.
    fn sound_elsewhere(&self) -> Option<String> {
        if self.soloist == cyder_core::soloist::Status::Ready {
            return Some("Pelo Soloist, o som sai dele direto para o Windows e o Cyder não tem como mexer. Estas opções voltam a valer se você desligar o Soloist.".into());
        }
        self.remote.as_ref().map(|d| format!("Tocando em {d}: o som sai de lá, não do Cyder. Estas opções voltam a valer quando a música tocar aqui."))
    }

    /// Equalizador e transição suave: valem na hora, sem religar o player.
    fn sound_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.settings.clone();
        let fade = self.segmented("settings-fade", [(0, "Desligada"), (3, "3 s"), (6, "6 s"), (10, "10 s")], s.fade_secs, |st, v| st.fade_secs = v, false, cx);
        let why = self.sound_elsewhere();
        let off = why.is_some();
        let mut rows = Vec::new();
        if let Some(why) = why {
            rows.push(div().text_sm().text_color(rgba(theme::ink_2())).child(why).into_any_element());
        }
        rows.push(dimmed(off, Self::row("Equalizador", "Ajusta graves, médios e agudos do que toca neste computador.".into(),
            self.toggle("settings-eq", s.eq_on, |st| st.eq_on = !st.eq_on, false, cx))));
        if s.eq_on {
            let presets = div().flex().flex_wrap().gap_1().children(cyder_core::fx::PRESETS.iter().enumerate().map(|(i, (name, gains))| {
                let on = s.eq == *gains;
                let gains = *gains;
                Self::focusable(("eq-preset", i))
                    .h(px(30.))
                    .px_3()
                    .rounded_full()
                    .flex()
                    .items_center()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .aria_selected(on)
                    .when(on, |d| d.bg(rgba(theme::ink_1())).text_color(rgba(theme::on_ink())))
                    .when(!on, |d| d.bg(rgba(theme::wash(0x0f))).hover(|s| s.bg(rgba(theme::wash(0x1a)))))
                    .on_click(cx.listener(move |t, _, _, cx| { t.change(|st| st.eq = gains, false); cx.notify(); }))
                    .child(*name)
            }));
            rows.push(dimmed(off, presets.into_any_element()));
            let bands = self.eq_bands(cx);
            rows.push(dimmed(off, bands));
        }
        rows.push(dimmed(off, Self::row(
            "Transição suave",
            "Abaixa o fim de cada música e sobe o começo da próxima. Elas não tocam juntas: o player do Cyder toca uma por vez.".into(),
            fade,
        )));
        Self::section("Som", rows)
    }

    /// As 6 faixas: barra do ganho (de −12 a +12 dB, a partir do meio) e botões de subir e descer 1 dB.
    fn eq_bands(&mut self, cx: &mut Context<Self>) -> AnyElement {
        const H: f32 = 120.;
        let gains = self.settings.eq;
        let columns = cyder_core::fx::BANDS.iter().enumerate().map(|(i, &freq)| {
            let g = gains[i];
            let label = if freq >= 1000. { format!("{} kHz", (freq / 1000.).to_string().replace('.', ",")) } else { format!("{freq} Hz") };
            let half = H / 2.;
            let fill = (g.abs() / cyder_core::fx::GAIN_MAX * half).min(half);
            let bar = div().relative().w(px(10.)).h(px(H)).rounded_full().bg(rgba(theme::wash(0x14)))
                .child(div().absolute().left_0().right_0().top(px(half - 0.5)).h(px(1.)).bg(rgba(theme::wash(0x40))))
                .child(div().absolute().left_0().right_0().rounded_full().bg(rgba(theme::ink_1()))
                    .when(g >= 0., |d| d.top(px(half - fill)).h(px(fill)))
                    .when(g < 0., |d| d.top(px(half)).h(px(fill))));
            let step = |id: &'static str, label: SharedString, icon_name: &'static str, by: f32| {
                Self::focusable((id, i))
                    .size(px(28.))
                    .rounded_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .hover(|s| s.bg(rgba(theme::wash(0x1a))))
                    .aria_label(label)
                    .on_click(cx.listener(move |t, _, _, cx| {
                        t.change(|st| st.eq[i] = (st.eq[i] + by).clamp(-cyder_core::fx::GAIN_MAX, cyder_core::fx::GAIN_MAX), false);
                        cx.notify();
                    }))
                    .child(icon(icon_name, 14., theme::ink_2()))
            };
            div().flex_1().flex().flex_col().items_center().gap_1()
                .child(div().text_xs().font_weight(FontWeight::SEMIBOLD).child(if g > 0. { format!("+{g:.0} dB") } else { format!("{g:.0} dB") }))
                .child(step("eq-up", format!("Aumentar {label}").into(), "chevron-up", 1.))
                .child(bar)
                .child(step("eq-down", format!("Diminuir {label}").into(), "chevron-down", -1.))
                .child(div().text_xs().text_color(rgba(theme::ink_3())).child(label.clone()))
        });
        div().flex().gap_2().children(columns).into_any_element()
    }

    fn appearance_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let on = self.settings.cover_colors;
        let theme = self.segmented(
            "settings-theme",
            [(Theme::Dark, "Escuro"), (Theme::Light, "Claro"), (Theme::System, "Sistema")],
            self.settings.theme,
            |st, t| st.theme = t,
            false,
            cx,
        );
        let canvas = self.toggle("settings-canvas", self.settings.canvas, |st| st.canvas = !st.canvas, false, cx);
        Self::section("Aparência", vec![Self::row("Canvas", "Mostra no fundo do modo imersivo o vídeo curto que algumas músicas têm no Spotify.".into(), canvas), Self::row("Tema", "Sistema segue o modo claro ou escuro do Windows. O modo imersivo fica sempre escuro.".into(), theme), Self::row(
            "Cores da capa",
            "Pinta o fundo das páginas de álbum, playlist e podcast e o destaque do player com as cores da capa.".into(),
            self.toggle("settings-cover-colors", on, |st| st.cover_colors = !st.cover_colors, false, cx),
        )])
    }

    fn integrations_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let lastfm = match self.lastfm_user.clone() {
            Some(user) => self.help_row(
                "Last.fm",
                Help::Lastfm,
                format!("Ligado como {user}. O que toca neste computador entra no seu histórico do Last.fm."),
                Self::secondary_button("settings-lastfm-off", "Desligar").on_click(cx.listener(|t, _, _, cx| {
                    t.send(Cmd::LastfmDisconnect);
                    cx.notify();
                })),
                cx,
            ),
            None => self.help_row(
                "Last.fm",
                Help::Lastfm,
                "O que toca pelo Cyder não entra no histórico do Spotify; o Last.fm guarda o que você ouve. Precisa de uma chave de API do Last.fm, de graça.".into(),
                div().flex().gap_2()
                    .child(Self::secondary_button("settings-lastfm-create", "Criar chave").on_click(|_, _, cx| cx.open_url("https://www.last.fm/api/account/create")))
                    .child(Self::primary_button("settings-lastfm-on", "Ligar").on_click(cx.listener(|t, _, _, cx| { t.ask(Ask::LastfmKey, String::new()); cx.notify(); }))),
                cx,
            ),
        };
        let has_app = !self.settings.discord_app_id.trim().is_empty();
        let discord_control = if has_app {
            div().flex().items_center().gap_3()
                .child(Self::secondary_button("settings-discord-app", "Trocar app").on_click(cx.listener(|t, _, _, cx| {
                    let id = t.settings.discord_app_id.clone();
                    t.ask(Ask::DiscordApp, id);
                    cx.notify();
                })))
                .child(self.toggle("settings-discord", self.settings.discord, |st| st.discord = !st.discord, false, cx))
                .into_any_element()
        } else {
            div().flex().gap_2()
                .child(Self::secondary_button("settings-discord-portal", "Abrir portal").on_click(|_, _, cx| cx.open_url("https://discord.com/developers/applications")))
                .child(Self::primary_button("settings-discord-on", "Configurar").on_click(cx.listener(|t, _, _, cx| { t.ask(Ask::DiscordApp, String::new()); cx.notify(); })))
                .into_any_element()
        };
        let discord_detail = if has_app {
            "Mostra no seu perfil do Discord o que está tocando, com a capa e o tempo. Precisa do Discord aberto neste computador."
        } else {
            "Mostra no seu perfil do Discord o que está tocando. Precisa de um aplicativo seu no portal do Discord (o nome dele aparece como “Ouvindo …”)."
        };
        let mini_on = self.mini.is_some();
        Self::section("Integrações", vec![
            lastfm,
            self.help_row("Discord", Help::Discord, discord_detail.into(), discord_control, cx),
            Self::row(
                "Mini player",
                "Uma janela pequena por cima das outras com a capa e os controles. Também abre pelo ícone da bandeja.".into(),
                Self::secondary_button("settings-mini", if mini_on { "Fechar" } else { "Abrir" }).on_click(cx.listener(|t, _, _, cx| { t.toggle_mini(cx); cx.notify(); })),
            ),
        ])
    }

    fn storage_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (art, audio) = self.storage.unwrap_or_default();
        let cache = config::cache_dir();
        let reveal = |id: &'static str, path: std::path::PathBuf| {
            Self::secondary_button(id, "Mostrar pasta").on_click(move |_, _, cx| cx.reveal_path(&path))
        };
        let art_row = if images::clear_requested() {
            Self::row("Capas", "Serão apagadas na próxima vez que o Cyder abrir.".into(), div())
        } else {
            Self::row(
                "Capas",
                format!("{} em {}", size_label(art), cache.display()),
                Self::secondary_button("settings-clear-art", "Limpar capas").on_click(cx.listener(|t, _, _, cx| {
                    // Apagar agora deixaria as capas da tela quebradas; o core apaga ao abrir.
                    images::request_clear();
                    t.notify("As capas serão apagadas na próxima vez que o Cyder abrir.");
                    cx.notify();
                })),
            )
        };
        Self::section("Armazenamento", vec![
            art_row,
            Self::row("Áudio", format!("{} em {}", size_label(audio), config::audio_cache_dir().display()), reveal("settings-audio-dir", config::audio_cache_dir())),
            Self::row("Login", format!("Guardado em {}", config::credentials_dir().display()), reveal("settings-login-dir", config::credentials_dir())),
        ])
    }

    fn about_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let config_dir = config::config_dir();
        let reports = crate::reports::available().then(|| Self::row("Enviar relatórios de erro", crate::view::REPORTS_TEXT.into(),
            self.toggle("settings-reports", self.settings.send_errors == Some(true), |st| st.send_errors = Some(st.send_errors != Some(true)), false, cx)));
        let mut rows = vec![
            div().flex().items_center().gap_4()
                .child(div().size(px(44.)).flex_none().rounded(px(12.)).bg(rgba(theme::ink_1())).flex().items_center().justify_center()
                    .child(icon("play", 20., theme::on_ink())))
                .child(
                    div().flex().flex_col()
                        .child(div().text_base().font_weight(FontWeight::BOLD).child(concat!("Cyder ", env!("CARGO_PKG_VERSION"))))
                        .child(div().text_sm().text_color(rgba(theme::ink_2())).child("Feito com Rust, GPUI e librespot. Não é afiliado ao Spotify.")),
                )
                .into_any_element(),
            self.update_row(cx),
        ];
        rows.extend(reports);
        rows.push(
            div().flex().gap_2()
                .child(Self::secondary_button("settings-shortcuts", "Atalhos do teclado")
                    .on_click(cx.listener(|t, _, _, cx| { t.dialog = Some(Dialog::Shortcuts); cx.notify(); })))
                .child(Self::secondary_button("settings-config-dir", "Pasta de configuração").on_click(move |_, _, cx| cx.reveal_path(&config_dir)))
                // O que mandar quando algo der errado: o log desta abertura (e o da anterior, na mesma pasta).
                .child(Self::secondary_button("settings-log", "Registro").on_click(|_, _, cx| cx.reveal_path(&config::log_path())))
                .into_any_element(),
        );
        Self::section("Sobre", rows)
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
            .tip(match help { Help::Lastfm => "Como ligar o Last.fm", Help::Discord => "Como ligar o Discord", Help::Soloist => "Como ligar o Soloist" })
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
        Self::focusable(id)
            .w(px(40.))
            .h(px(22.))
            .p(px(2.))
            .rounded_full()
            .flex()
            .items_center()
            .when(on, |d| d.justify_end().bg(rgba(LIKED_GREEN)))
            .when(!on, |d| d.bg(rgba(theme::wash(0x26))).hover(|s| s.bg(rgba(theme::wash(0x38)))))
            .aria_label(if on { "Ligado" } else { "Desligado" })
            .aria_selected(on)
            .on_click(cx.listener(move |t, _, _, cx| { t.change(flip, restart); cx.notify(); }))
            .child(div().size(px(18.)).rounded_full().bg(rgba(0xffffffff)).shadow_sm())
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
        div()
            .flex()
            .gap_1()
            .children(options.into_iter().enumerate().map(|(i, (value, label))| {
                let on = value == selected;
                Self::focusable((id, i))
                    .h(px(30.))
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

    fn device_field(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let focused = self.device_focus.is_focused(window);
        let caret = || div().flex_none().w(px(1.5)).h(px(16.)).bg(rgba(theme::ink_1()));
        div()
            .id("settings-device-name")
            .key_context("TextField")
            .track_focus(&self.device_focus)
            .aria_label("Nome do aparelho")
            .w(px(220.))
            .h(px(34.))
            .px_3()
            .flex()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .rounded(px(8.))
            .bg(rgba(theme::shade(0x40)))
            .border_1()
            .border_color(rgba(if focused { theme::ink_3() } else { theme::wash(0x1f) }))
            .text_sm()
            .cursor_text()
            .on_click(cx.listener(|t, e: &gpui::ClickEvent, window, cx| { t.text_selected = e.click_count() >= 2; window.focus(&t.device_focus, cx); cx.notify(); }))
            .on_key_down(cx.listener(|t, e: &gpui::KeyDownEvent, window, cx| t.device_key(e, window, cx)))
            .child(self.field_text(self.device_draft.clone(), focused))
            .when(focused, |d| d.child(caret()))
            .into_any_element()
    }

    /// Enter salva o nome (e religa o player); Esc desfaz.
    fn device_key(&mut self, e: &gpui::KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &e.keystroke;
        match k.key.as_str() {
            "enter" => {
                let name = self.device_draft.trim().to_string();
                self.change(|st| st.device_name = name, true);
                self.device_draft = self.settings.device_name();
                window.focus(&self.focus, cx);
            }
            "escape" => {
                self.device_draft = self.settings.device_name();
                window.focus(&self.focus, cx);
            }
            _ if super::super::type_into(&mut self.device_draft, k, &mut self.text_selected, cx) => self.device_draft = self.device_draft.chars().take(40).collect(),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
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
