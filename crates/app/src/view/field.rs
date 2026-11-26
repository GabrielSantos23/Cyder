//! Os campos de texto de uma linha (buscas, Client ID, a janela de nome): o texto mora no `Cyder`, e um só cursor e
//! seleção valem para o campo focado. Teclas de editar e mover chegam pelo `on_key_down` de cada campo; o que se
//! digita chega pelo sistema de texto do Windows (`EntityInputHandler`), que traz IME, o painel de emoji e o AltGr.
use std::ops::Range;

use cyder_core::i18n::t;
use gpui::{
    Bounds, ClipboardItem, Context, Div, ElementInputHandler, EntityInputHandler, FocusHandle, Keystroke, Pixels, Point, ShapedLine,
    SharedString, TextRun, UTF16Selection, Window, canvas, div, prelude::*, px, rgba,
};

use super::{Cyder, Dialog, library::Screen, menus::MenuTarget};

/// Fundo do texto selecionado nos campos.
const SELECTION: u32 = 0x3d7eff66;
/// Tamanho máximo de um campo, em letras.
const MAX: usize = 80;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Field {
    Search,
    Side,
    Liked,
    ClientId,
    Dialog,
}

/// Onde o texto do campo foi desenhado, para achar a letra sob o mouse e pôr a janela do IME junto do cursor.
pub(crate) struct FieldLayout {
    field: Field,
    bounds: Bounds<Pixels>,
    line: ShapedLine,
}

/// Editar pelo menu do botão direito.
#[derive(Clone, Copy)]
pub(crate) enum Edit {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

/// Posição do cursor e da seleção contadas do fim (letras depois deles): assim continuam certas quando o texto
/// muda por fora, e 0 é o fim, o padrão ao entrar num campo.
fn positions(len: usize, back: usize, anchor: Option<usize>) -> (usize, Option<usize>) {
    (len - back.min(len), anchor.map(|a| len - a.min(len)))
}

/// A seleção (do menor ao maior), se houver.
fn selection(at: usize, from: Option<usize>) -> Option<Range<usize>> {
    from.filter(|&f| f != at).map(|f| f.min(at)..f.max(at))
}

/// Teclas de editar e mover: setas, Home/End (com Shift selecionam, com Ctrl pulam palavras), Backspace/Delete,
/// Ctrl+A/C/X/V. O que se digita não passa aqui (vem pelo sistema de texto). False: a tecla não é do campo.
pub(crate) fn type_into(text: &mut String, k: &Keystroke, anchor: &mut Option<usize>, back: &mut usize, cx: &mut Context<Cyder>) -> bool {
    let mut chars: Vec<char> = text.chars().collect();
    let (mut at, mut from) = positions(chars.len(), *back, *anchor);
    let sel = selection(at, from);
    let (word, shift) = (k.modifiers.control, k.modifiers.shift);
    match k.key.as_str() {
        "a" if k.modifiers.secondary() => (from, at) = (Some(0), chars.len()),
        "c" | "x" if k.modifiers.secondary() => {
            let Some(r) = sel else { return false };
            cx.write_to_clipboard(ClipboardItem::new_string(chars[r.clone()].iter().collect()));
            if k.key == "x" {
                chars.drain(r.clone());
                (at, from) = (r.start, None);
            }
        }
        "backspace" | "delete" if sel.is_some() => {
            let r = sel.unwrap_or_default();
            chars.drain(r.clone());
            (at, from) = (r.start, None);
        }
        "backspace" => {
            let start = if word { word_left(&chars, at) } else { at.saturating_sub(1) };
            chars.drain(start..at);
            at = start;
        }
        "delete" => {
            let end = if word { word_right(&chars, at) } else { (at + 1).min(chars.len()) };
            chars.drain(at..end);
        }
        "left" | "right" | "home" | "end" => {
            let to = match k.key.as_str() {
                "home" => 0,
                "end" => chars.len(),
                // Sem Shift, a seta só recolhe a seleção para o lado dela, como num campo do Windows.
                "left" if !shift && !word && sel.is_some() => sel.clone().unwrap_or_default().start,
                "right" if !shift && !word && sel.is_some() => sel.clone().unwrap_or_default().end,
                "left" if word => word_left(&chars, at),
                "left" => at.saturating_sub(1),
                _ if word => word_right(&chars, at),
                _ => (at + 1).min(chars.len()),
            };
            from = if shift { from.or(Some(at)) } else { None };
            at = to;
        }
        "v" if k.modifiers.secondary() => {
            let Some(pasted) = cx.read_from_clipboard().and_then(|c| c.text()) else { return false };
            let r = sel.unwrap_or(at..at);
            let new: Vec<char> = clean(pasted.lines().next().unwrap_or_default());
            at = r.start + new.len();
            chars.splice(r, new);
            from = None;
        }
        _ => return false,
    }
    store(text, chars, at, from, anchor, back);
    true
}

/// Sem quebras de linha nem caracteres de controle (o campo é de uma linha).
fn clean(s: &str) -> Vec<char> {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// Grava texto, cursor e seleção, cortando no tamanho máximo.
fn store(text: &mut String, mut chars: Vec<char>, at: usize, from: Option<usize>, anchor: &mut Option<usize>, back: &mut usize) {
    chars.truncate(MAX);
    let len = chars.len();
    *back = len - at.min(len);
    *anchor = from.map(|f| len - f.min(len));
    *text = chars.into_iter().collect();
}

/// Início da palavra à esquerda do cursor (pula os espaços antes dela), como o Ctrl+← dos campos do Windows.
pub(crate) fn word_left(chars: &[char], at: usize) -> usize {
    let mut i = at;
    while i > 0 && !chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    while i > 0 && chars[i - 1].is_alphanumeric() {
        i -= 1;
    }
    i
}

/// Fim da palavra à direita do cursor (pula os espaços antes dela).
pub(crate) fn word_right(chars: &[char], at: usize) -> usize {
    let mut i = at;
    while i < chars.len() && !chars[i].is_alphanumeric() {
        i += 1;
    }
    while i < chars.len() && chars[i].is_alphanumeric() {
        i += 1;
    }
    i
}

fn utf16_of(chars: &[char], at: usize) -> usize {
    chars[..at.min(chars.len())].iter().map(|c| c.len_utf16()).sum()
}

fn char_of(chars: &[char], utf16: usize) -> usize {
    let mut n = 0;
    for (i, c) in chars.iter().enumerate() {
        if n >= utf16 {
            return i;
        }
        n += c.len_utf16();
    }
    chars.len()
}

impl Cyder {
    pub(crate) fn field_focus(&self, f: Field) -> &FocusHandle {
        match f {
            Field::Search => &self.search_focus,
            Field::Side => &self.side_search_focus,
            Field::Liked => &self.liked_search_focus,
            Field::ClientId => &self.client_id_focus,
            Field::Dialog => &self.dialog_focus,
        }
    }

    fn focused_field(&self, window: &Window) -> Option<Field> {
        [Field::Search, Field::Side, Field::Liked, Field::ClientId, Field::Dialog].into_iter().find(|&f| self.field_focus(f).is_focused(window))
    }

    fn field_string(&mut self, f: Field) -> Option<&mut String> {
        match f {
            Field::Search => Some(&mut self.search_query),
            Field::Side => Some(&mut self.side_query),
            Field::Liked => Some(&mut self.liked_query),
            Field::ClientId => Some(&mut self.client_id),
            Field::Dialog => match self.dialog.as_mut() {
                Some(Dialog::Input { text, .. }) => Some(text),
                _ => None,
            },
        }
    }

    /// O que cada campo faz quando o texto muda (por tecla, colando ou pelo menu).
    pub(crate) fn field_edited(&mut self, f: Field) {
        match f {
            Field::Search => {
                self.search_due = Some(std::time::Instant::now());
                if !matches!(self.screen, Screen::Search) {
                    self.go(Screen::Search);
                    // O go devolve o foco à raiz no próximo render; aqui ele fica no campo.
                    self.refocus = false;
                }
            }
            Field::Liked => self.liked_changed(),
            Field::ClientId => {
                // O Client ID não tem espaços: os que vêm ao colar saem.
                self.client_id = self.client_id.chars().filter(|c| !c.is_whitespace()).take(super::CLIENT_ID_MAX).collect();
                self.client_id_error = None;
                self.client_id_pasted = false;
            }
            Field::Dialog => {
                if let Some(Dialog::Input { ask, text }) = self.dialog.as_mut() {
                    let max = ask.labels().4;
                    *text = text.chars().take(max).collect();
                }
            }
            Field::Side => {}
        }
    }

    /// Clique no campo: põe o cursor na letra clicada; com Shift, seleciona até ela; duplo clique seleciona tudo.
    pub(crate) fn click_field(&mut self, f: Field, at: Point<Pixels>, clicks: usize, shift: bool) {
        self.text_marked = None;
        let index = self.field_layout.borrow().as_ref().filter(|l| l.field == f).map(|l| {
            let byte = l.line.closest_index_for_x(at.x - l.bounds.left());
            l.line.text[..byte.min(l.line.text.len())].chars().count()
        });
        let Some(len) = self.field_string(f).map(|s| s.chars().count()) else { return };
        let (caret, from) = positions(len, self.text_back, self.text_anchor);
        if clicks >= 2 {
            (self.text_back, self.text_anchor) = (0, Some(len));
            return;
        }
        let to = index.unwrap_or(len).min(len);
        self.text_anchor = if shift { Some(len - from.unwrap_or(caret)) } else { None };
        self.text_back = len - to;
    }

    /// Recortar, copiar, colar e selecionar tudo pelo menu, como as teclas.
    pub(crate) fn field_menu_edit(&mut self, f: Field, edit: Edit, cx: &mut Context<Self>) {
        let key = match edit {
            Edit::Cut => "ctrl-x",
            Edit::Copy => "ctrl-c",
            Edit::Paste => "ctrl-v",
            Edit::SelectAll => "ctrl-a",
        };
        let Ok(k) = Keystroke::parse(key) else { return };
        let (mut anchor, mut back) = (self.text_anchor, self.text_back);
        let Some(text) = self.field_string(f) else { return };
        let before = text.clone();
        if type_into(text, &k, &mut anchor, &mut back, cx) {
            let changed = *text != before;
            (self.text_anchor, self.text_back) = (anchor, back);
            if changed {
                self.field_edited(f);
            }
        }
    }

    /// O texto de um campo: antes do cursor, o cursor (`caret`) e depois, com o fundo de seleção na parte
    /// selecionada. Também guarda onde o texto foi desenhado (`field_layout`), para o clique e o IME.
    pub(crate) fn field_text(&self, f: Field, text: impl Into<SharedString>, focused: bool, caret: Div) -> Div {
        let text: SharedString = text.into();
        let layout = self.field_layout.clone();
        let measured = text.clone();
        let probe = canvas(
            move |bounds, window, _| {
                let style = window.text_style();
                let run = TextRun { len: measured.len(), font: style.font(), color: style.color, background_color: None, underline: None, strikethrough: None };
                let line = window.text_system().shape_line(measured.clone(), style.font_size.to_pixels(window.rem_size()), &[run], None);
                *layout.borrow_mut() = Some(FieldLayout { field: f, bounds, line });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0();
        let row = div().relative().flex().items_center().overflow_hidden().whitespace_nowrap().child(probe);
        if !focused {
            return row.child(text);
        }
        let chars: Vec<char> = text.chars().collect();
        let (at, from) = positions(chars.len(), self.text_back, self.text_anchor);
        let piece = |r: Range<usize>| chars[r].iter().collect::<String>();
        let Some(sel) = selection(at, from) else {
            return row.when(at > 0, |d| d.child(piece(0..at))).child(caret).when(at < chars.len(), |d| d.child(piece(at..chars.len())));
        };
        let marked = div().rounded(px(3.)).bg(rgba(SELECTION)).child(piece(sel.clone()));
        // O cursor fica na ponta onde a seleção está crescendo.
        let caret_first = at == sel.start;
        let row = row.when(sel.start > 0, |d| d.child(piece(0..sel.start)));
        let row = if caret_first { row.child(caret).child(marked) } else { row.child(marked).child(caret) };
        row.when(sel.end < chars.len(), |d| d.child(piece(sel.end..chars.len())))
    }

    /// Liga o campo ao sistema de texto do Windows e ao menu do botão direito. Vai em todo campo, vazio ou não.
    pub(crate) fn wire_field(&self, d: Div, f: Field, cx: &mut Context<Self>) -> Div {
        let focus = self.field_focus(f).clone();
        let entity = cx.entity();
        d.relative()
            .child(
                canvas(|_, _, _| {}, move |bounds, _, window, cx| {
                    if focus.is_focused(window) {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                    }
                })
                .absolute()
                .inset_0(),
            )
            .on_mouse_down(gpui::MouseButton::Right, cx.listener(move |t, e: &gpui::MouseDownEvent, window, cx| {
                window.focus(t.field_focus(f), cx);
                t.open_menu(MenuTarget::Field(f), e.position);
                cx.stop_propagation();
                cx.notify();
            }))
    }

    /// Troca um trecho (UTF-16, ou o texto em composição, ou a seleção) por `new`. `mark`: o IME ainda está compondo.
    fn field_replace(&mut self, range: Option<Range<usize>>, new: &str, mark: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(f) = self.focused_field(window) else { return };
        let (mut anchor, mut back, marked) = (self.text_anchor, self.text_back, self.text_marked.clone());
        let Some(text) = self.field_string(f) else { return };
        let mut chars: Vec<char> = text.chars().collect();
        let (at, from) = positions(chars.len(), back, anchor);
        let r = match range {
            Some(r) => char_of(&chars, r.start)..char_of(&chars, r.end),
            None => marked.unwrap_or_else(|| selection(at, from).unwrap_or(at..at)),
        };
        let r = r.start.min(chars.len())..r.end.min(chars.len());
        let new = clean(new);
        let end = r.start + new.len();
        chars.splice(r.clone(), new);
        store(text, chars, end, None, &mut anchor, &mut back);
        let len = text.chars().count();
        (self.text_anchor, self.text_back) = (anchor, back);
        self.text_marked = (mark && end > r.start).then(|| r.start.min(len)..end.min(len));
        self.field_edited(f);
        cx.notify();
    }

    fn field_chars(&mut self, window: &Window) -> Option<Vec<char>> {
        let f = self.focused_field(window)?;
        Some(self.field_string(f)?.chars().collect())
    }
}

impl EntityInputHandler for Cyder {
    fn text_for_range(&mut self, range: Range<usize>, adjusted: &mut Option<Range<usize>>, window: &mut Window, _: &mut Context<Self>) -> Option<String> {
        let chars = self.field_chars(window)?;
        let r = char_of(&chars, range.start)..char_of(&chars, range.end).max(char_of(&chars, range.start));
        *adjusted = Some(utf16_of(&chars, r.start)..utf16_of(&chars, r.end));
        Some(chars[r].iter().collect())
    }

    fn selected_text_range(&mut self, _: bool, window: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let chars = self.field_chars(window)?;
        let (at, from) = positions(chars.len(), self.text_back, self.text_anchor);
        let r = selection(at, from).unwrap_or(at..at);
        Some(UTF16Selection { range: utf16_of(&chars, r.start)..utf16_of(&chars, r.end), reversed: from.is_some_and(|f| f > at) })
    }

    fn marked_text_range(&self, window: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let r = self.text_marked.clone()?;
        let f = self.focused_field(window)?;
        let chars: Vec<char> = match f {
            Field::Search => self.search_query.chars().collect(),
            Field::Side => self.side_query.chars().collect(),
            Field::Liked => self.liked_query.chars().collect(),
            Field::ClientId => self.client_id.chars().collect(),
            Field::Dialog => match &self.dialog {
                Some(Dialog::Input { text, .. }) => text.chars().collect(),
                _ => return None,
            },
        };
        Some(utf16_of(&chars, r.start)..utf16_of(&chars, r.end))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.text_marked = None;
    }

    fn replace_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.field_replace(range, text, false, window, cx);
    }

    fn replace_and_mark_text_in_range(&mut self, range: Option<Range<usize>>, text: &str, _: Option<Range<usize>>, window: &mut Window, cx: &mut Context<Self>) {
        self.field_replace(range, text, true, window, cx);
    }

    /// A janela do IME abre junto do cursor (ou no começo do campo, se o texto ainda não foi desenhado).
    fn bounds_for_range(&mut self, range: Range<usize>, element: Bounds<Pixels>, window: &mut Window, _: &mut Context<Self>) -> Option<Bounds<Pixels>> {
        let chars = self.field_chars(window)?;
        let at = char_of(&chars, range.start);
        let layout = self.field_layout.borrow();
        let x = match layout.as_ref() {
            Some(l) if l.bounds.intersects(&element) => {
                let byte = l.line.text.char_indices().nth(at).map_or(l.line.text.len(), |(i, _)| i);
                l.bounds.left() + l.line.x_for_index(byte)
            }
            _ => element.left(),
        };
        Some(Bounds::new(gpui::point(x, element.top()), gpui::size(px(2.), element.size.height)))
    }

    fn character_index_for_point(&mut self, at: Point<Pixels>, window: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let chars = self.field_chars(window)?;
        let layout = self.field_layout.borrow();
        let l = layout.as_ref()?;
        let byte = l.line.closest_index_for_x(at.x - l.bounds.left());
        Some(utf16_of(&chars, l.line.text[..byte.min(l.line.text.len())].chars().count()))
    }
}

/// Itens do menu do botão direito de um campo.
pub(crate) fn menu_labels() -> [(Edit, &'static str, &'static str); 4] {
    [
        (Edit::Cut, t("Recortar"), "scissors"),
        (Edit::Copy, t("Copiar"), "copy"),
        (Edit::Paste, t("Colar"), "clipboard-paste"),
        (Edit::SelectAll, t("Selecionar tudo"), "text-select"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_jumps() {
        let c: Vec<char> = "ana  maria-bê".chars().collect();
        assert_eq!(word_left(&c, c.len()), 11);
        assert_eq!(word_left(&c, 11), 5);
        assert_eq!(word_left(&c, 5), 0);
        assert_eq!(word_right(&c, 0), 3);
        assert_eq!(word_right(&c, 3), 10);
        assert_eq!(word_right(&c, c.len()), c.len());
    }

    #[test]
    fn utf16_round_trip() {
        let c: Vec<char> = "a😀b".chars().collect();
        assert_eq!(utf16_of(&c, 2), 3);
        assert_eq!(char_of(&c, 3), 2);
        assert_eq!(char_of(&c, 99), 3);
    }

    #[test]
    fn selection_from_back_offsets() {
        // "abcdef", cursor depois do "d" (2 do fim), âncora antes do "b" (5 do fim).
        let (at, from) = positions(6, 2, Some(5));
        assert_eq!((at, from), (4, Some(1)));
        assert_eq!(selection(at, from), Some(1..4));
        assert_eq!(selection(3, Some(3)), None);
    }
}
