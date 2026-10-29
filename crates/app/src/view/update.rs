//! Atualização do app: ao abrir, o core procura uma versão nova nas Releases do GitHub. Achando, aparece um
//! botão no cabeçalho (à esquerda do mini player); clicar baixa e instala, com o progresso no próprio botão,
//! e no fim um diálogo pergunta se quer reiniciar. Nas configurações, "Procurar atualizações" faz o mesmo à mão.
use std::time::Duration;

use cyder_core::{Cmd, update::{CURRENT, Release, State}};
use gpui::{Animation, AnimationExt, AnyElement, Context, FontWeight, ParentElement, Styled, div, prelude::*, px, rgba};

use super::{Cyder, Dialog, Tip, icon};
use crate::theme;

/// Argumento da versão nova ao reiniciar: ela espera a antiga terminar de fechar (main.rs).
pub const RESTARTED: &str = "--updated";

impl Cyder {
    /// Resposta do core. Uma procura automática que falha não aparece: só a pedida nas configurações.
    pub(super) fn on_update(&mut self, state: State) {
        self.update = match state {
            State::Failed(_) if !self.update_manual => State::Idle,
            State::Ready(release) => {
                self.dialog = Some(Dialog::Update(release.version.clone()));
                State::Ready(release)
            }
            other => other,
        };
    }

    pub(super) fn check_update(&mut self, manual: bool) {
        self.update_manual = manual;
        self.send(Cmd::CheckUpdate);
    }

    fn install_update(&mut self, release: Release) {
        self.update_manual = true;
        self.update = State::Downloading(release.clone(), 0.);
        self.send(Cmd::InstallUpdate(release));
    }

    /// Abre a versão nova (que espera esta fechar) e fecha esta.
    pub(super) fn restart_for_update(&mut self, cx: &mut Context<Self>) {
        match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg(RESTARTED).spawn()) {
            Ok(_) => cx.quit(),
            Err(e) => self.notify(format!("Não deu para reiniciar: {e}. Feche e abra o Cyder para usar a versão nova.")),
        }
    }

    /// O botão do cabeçalho: "Atualizar", o download em andamento ou "Reiniciar". Some sem atualização.
    pub(super) fn update_button(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pill = |id: &'static str| {
            Self::focusable(id)
                .relative()
                .h(px(36.))
                .px_4()
                .rounded_full()
                .overflow_hidden()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .font_weight(FontWeight::SEMIBOLD)
        };
        match &self.update {
            State::Available(release) => {
                let release = release.clone();
                Some(pill("toolbar-update")
                    .bg(rgba(theme::ink_1()))
                    .text_color(rgba(theme::on_ink()))
                    .hover(|s| s.bg(rgba(theme::wash(0xd9))))
                    .tip(format!("Baixar e instalar a versão {}", release.version))
                    .on_click(cx.listener(move |t, _, _, cx| { t.install_update(release.clone()); cx.notify(); }))
                    .child(icon("arrow-down", 15., theme::on_ink()))
                    .child("Atualizar")
                    .into_any_element())
            }
            State::Downloading(release, p) => {
                // A seta desce e volta enquanto baixa; o fundo enche da esquerda conforme o progresso.
                let arrow = icon("arrow-down", 15., theme::ink_1()).with_animation(
                    "update-arrow",
                    Animation::new(Duration::from_millis(900)).repeat(),
                    |s, t| s.mt(px(4. * (t * std::f32::consts::PI).sin())),
                );
                Some(pill("toolbar-update")
                    .min_w(px(132.))
                    .bg(rgba(theme::wash(0x1a)))
                    .border_1()
                    .border_color(rgba(theme::wash(0x26)))
                    .cursor_default()
                    .tip(format!("Baixando a versão {}", release.version))
                    .child(div().absolute().left_0().top_0().bottom_0().w(gpui::relative(p.clamp(0.02, 1.))).bg(rgba(theme::wash(0x33))))
                    .child(div().relative().flex().items_center().gap_2().child(arrow).child(format!("Baixando {}%", (p * 100.).round() as u32)))
                    .into_any_element())
            }
            State::Ready(release) => {
                let version = release.version.clone();
                Some(pill("toolbar-update")
                    .bg(rgba(theme::ink_1()))
                    .text_color(rgba(theme::on_ink()))
                    .hover(|s| s.bg(rgba(theme::wash(0xd9))))
                    .tip(format!("A versão {version} está instalada; reinicie para usar"))
                    .on_click(cx.listener(move |t, _, _, cx| { t.dialog = Some(Dialog::Update(version.clone())); cx.notify(); }))
                    .child(icon("rotate-cw", 15., theme::on_ink()))
                    .child("Reiniciar")
                    .into_any_element())
            }
            _ => None,
        }
    }

    /// Linha de Configurações › Sobre.
    pub(super) fn update_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let check = |label: &'static str, cx: &mut Context<Self>| {
            Self::secondary_button("settings-update-check", label).on_click(cx.listener(|t, _, _, cx| {
                t.update = State::Checking;
                t.check_update(true);
                cx.notify();
            }))
        };
        let (detail, control): (String, AnyElement) = match self.update.clone() {
            State::Idle => (format!("Você está na versão {CURRENT}."), check("Procurar atualizações", cx).into_any_element()),
            State::Checking => ("Procurando…".into(), div().into_any_element()),
            State::UpToDate => (format!("Você está na versão mais nova ({CURRENT})."), check("Procurar de novo", cx).into_any_element()),
            State::Available(release) => (
                format!("A versão {} está disponível. Você está na {CURRENT}.", release.version),
                Self::primary_button("settings-update-install", "Baixar e instalar")
                    .on_click(cx.listener(move |t, _, _, cx| { t.install_update(release.clone()); cx.notify(); }))
                    .into_any_element(),
            ),
            State::Downloading(release, p) => (format!("Baixando a versão {}: {}%.", release.version, (p * 100.).round() as u32), div().into_any_element()),
            State::Ready(release) => (
                format!("A versão {} está instalada. Reinicie para usar.", release.version),
                Self::primary_button("settings-update-restart", "Reiniciar").on_click(cx.listener(|t, _, _, cx| t.restart_for_update(cx))).into_any_element(),
            ),
            State::Failed(e) => (e, check("Tentar de novo", cx).into_any_element()),
        };
        Self::row("Atualizações", detail, control)
    }
}
