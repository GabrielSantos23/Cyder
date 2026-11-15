//! Copied from ytm-rs (https://github.com/grapedjuice/ytm-rs, GPL-3.0-only); see LICENSE.
//! Kept close to upstream so fixes there can be brought across, so unused parts stay.
#![allow(dead_code, clippy::all)]

//! Time-synced lyrics from every source the Better Lyrics extension uses, parsed the way
//! its parsers (`@braccato/parsers`) parse them, so any of them can be shown and the
//! listener can switch between them.
//!
//! - The Better Lyrics API streams its providers as server-sent events: Better Lyrics
//!   (Apple-style TTML), BiniLyrics (TTML), Musixmatch (word-by-word and line LRC), LRCLib
//!   (LRC and plain), Legato (KuGou LRC) and Portato (QQ QRC).
//! - Unison, the community lyrics service, answers a plain GET.
//!
//! The API needs a JWT, obtained by exchanging a Cloudflare Turnstile token (see
//! turnstile.rs). JWTs last 24 h and are cached on disk.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// One source of lyrics, in the extension's default priority order (its
/// `PROVIDER_CONFIGS`, without the two YouTube sources).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Provider {
    BLyricsRich,
    UnisonRich,
    BinimumRich,
    UnisonWord,
    PortatoRich,
    MusixmatchRich,
    BLyricsSynced,
    UnisonSynced,
    BinimumSynced,
    LrclibSynced,
    LegatoSynced,
    MusixmatchSynced,
    UnisonPlain,
    LrclibPlain,
}

/// How finely a provider's lyrics are timed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncKind {
    Syllable,
    Word,
    Line,
    Unsynced,
}

impl Provider {
    pub const ALL: [Provider; 14] = [
        Provider::BLyricsRich,
        Provider::UnisonRich,
        Provider::BinimumRich,
        Provider::UnisonWord,
        Provider::PortatoRich,
        Provider::MusixmatchRich,
        Provider::BLyricsSynced,
        Provider::UnisonSynced,
        Provider::BinimumSynced,
        Provider::LrclibSynced,
        Provider::LegatoSynced,
        Provider::MusixmatchSynced,
        Provider::UnisonPlain,
        Provider::LrclibPlain,
    ];

    /// The extension's key for it (also how the order is stored).
    pub fn key(self) -> &'static str {
        match self {
            Provider::BLyricsRich => "bLyrics-richsynced",
            Provider::UnisonRich => "unison-richsynced",
            Provider::BinimumRich => "binimum-richsynced",
            Provider::UnisonWord => "unison-wordsynced",
            Provider::PortatoRich => "portato-richsynced",
            Provider::MusixmatchRich => "musixmatch-richsync",
            Provider::BLyricsSynced => "bLyrics-synced",
            Provider::UnisonSynced => "unison-synced",
            Provider::BinimumSynced => "binimum-synced",
            Provider::LrclibSynced => "lrclib-synced",
            Provider::LegatoSynced => "legato-synced",
            Provider::MusixmatchSynced => "musixmatch-synced",
            Provider::UnisonPlain => "unison-plain",
            Provider::LrclibPlain => "lrclib-plain",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    pub fn name(self) -> &'static str {
        match self {
            Provider::BLyricsRich | Provider::BLyricsSynced => "Better Lyrics",
            Provider::UnisonRich
            | Provider::UnisonWord
            | Provider::UnisonSynced
            | Provider::UnisonPlain => "Unison",
            Provider::BinimumRich | Provider::BinimumSynced => "BiniLyrics",
            Provider::PortatoRich => "Better Lyrics Portato",
            Provider::MusixmatchRich | Provider::MusixmatchSynced => "Musixmatch",
            Provider::LrclibSynced | Provider::LrclibPlain => "LRCLib",
            Provider::LegatoSynced => "Better Lyrics Legato",
        }
    }

    pub fn sync(self) -> SyncKind {
        match self {
            Provider::BLyricsRich | Provider::UnisonRich | Provider::BinimumRich => {
                SyncKind::Syllable
            }
            Provider::UnisonWord | Provider::PortatoRich | Provider::MusixmatchRich => {
                SyncKind::Word
            }
            Provider::BLyricsSynced
            | Provider::UnisonSynced
            | Provider::BinimumSynced
            | Provider::LrclibSynced
            | Provider::LegatoSynced
            | Provider::MusixmatchSynced => SyncKind::Line,
            Provider::UnisonPlain | Provider::LrclibPlain => SyncKind::Unsynced,
        }
    }
}

/// How a set of lines is timed, decided from the lines themselves (`deriveSyncType`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub enum Sync {
    None,
    Line,
    Word,
}

/// A timed piece of a line: a syllable, a word or several words.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub start: i32,
    pub dur: i32,
    pub text: String,
    /// Background vocals, shown on their own row under the line.
    pub bg: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Line {
    pub start: i32,
    pub dur: i32,
    pub text: String,
    /// Empty for line-synced and plain lyrics.
    pub parts: Vec<Part>,
    /// Singer: `v1`, `v2`, … or `v1000` for everyone.
    pub agent: Option<String>,
    /// A gap of more than five seconds nobody sings over.
    pub instrumental: bool,
    /// Translations shipped with the lyrics, by language.
    pub translations: Vec<(String, String)>,
    /// Romanization shipped with the lyrics, and its timing when it has some.
    pub romanization: Option<String>,
    pub timed_roman: Vec<Part>,
}

impl Line {
    pub fn end(&self) -> i32 {
        self.start + self.dur
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    pub sync: Sync,
    pub provider: Provider,
    /// Language the source says the lyrics are in.
    pub language: Option<String>,
    pub songwriters: Vec<String>,
}

impl Lyrics {
    pub fn new(
        provider: Provider,
        lines: Vec<Line>,
        language: Option<String>,
        songwriters: Vec<String>,
    ) -> Option<Self> {
        if lines.is_empty() {
            return None;
        }
        let sync = derive_sync(&lines);
        Some(Self {
            lines,
            sync,
            provider,
            language,
            songwriters,
        })
    }
}

/// `deriveSyncType`: richsync if any sung line has a part with a duration.
fn derive_sync(lines: &[Line]) -> Sync {
    if lines
        .iter()
        .any(|l| !l.instrumental && l.parts.iter().any(|p| p.dur != 0))
    {
        Sync::Word
    } else if lines.iter().all(|l| l.start == 0) {
        Sync::None
    } else {
        Sync::Line
    }
}

/// Every source that had lyrics for a song, in default priority order.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LyricsSet {
    pub sources: Vec<Lyrics>,
}

impl LyricsSet {
    pub fn get(&self, p: Provider) -> Option<&Lyrics> {
        self.sources.iter().find(|l| l.provider == p)
    }
}

const INSTRUMENTAL_GAP_MS: i32 = 5000;
fn insert_instrumentals(lines: Vec<Line>, song_ms: i32) -> Vec<Line> {
    if lines.is_empty() {
        return lines;
    }
    let gap = |start: i32, dur: i32| Line {
        start,
        dur,
        instrumental: true,
        ..Default::default()
    };
    let mut out = Vec::with_capacity(lines.len() + 4);
    if lines[0].start > INSTRUMENTAL_GAP_MS {
        out.push(gap(0, lines[0].start));
    }
    let n = lines.len();
    let last_end = lines[n - 1].end();
    for (i, l) in lines.iter().enumerate() {
        out.push(l.clone());
        if i + 1 < n {
            let end = l.end();
            let f = lines[i + 1].start - end;
            if f > INSTRUMENTAL_GAP_MS {
                out.push(gap(end, f));
            }
        }
    }
    if song_ms - last_end > INSTRUMENTAL_GAP_MS {
        out.push(gap(last_end, song_ms - last_end));
    }
    out
}

// Credit lines ("作词 : …", "Written by: …") that some providers put in the lyrics.
const WRITER_TAGS: &[&str] = &[
    "词",
    "詞",
    "作词",
    "作詞",
    "曲",
    "作曲",
    "词曲",
    "詞曲",
    "作词作曲",
    "作詞作曲",
    "writtenby",
    "lyricsby",
    "composedby",
    "lyricist",
    "composer",
];
const CREDIT_TAGS: &[&str] = &[
    "编曲",
    "編曲",
    "和声",
    "和聲",
    "混音",
    "吉他",
    "制作人",
    "製作人",
    "演唱",
    "原唱",
    "翻唱",
    "后期",
    "後期",
    "和音",
    "录音",
    "錄音",
    "策划",
    "策劃",
    "伴奏",
    "美工",
    "海报",
    "海報",
    "旁白",
    "producedby",
    "arrangedby",
    "mixing",
    "mastering",
    "vocal",
    "vocals",
    "guitar",
    "bass",
    "drums",
    "producer",
    "arranger",
];
const CREDIT_SUFFIXES: &[char] = &['词', '詞', '曲', '声', '聲', '音'];

fn norm_tag(t: &str) -> String {
    t.to_lowercase().split_whitespace().collect()
}

fn is_credit_tag(t: &str) -> bool {
    WRITER_TAGS.contains(&t)
        || CREDIT_TAGS.contains(&t)
        || (t.chars().count() <= 4 && t.ends_with(CREDIT_SUFFIXES))
}

fn tag_pieces(n: &str) -> Vec<String> {
    let no_latin: String = n.chars().filter(|c| !c.is_ascii_lowercase()).collect();
    no_latin
        .split(['/', '&', '、', ',', '，', '・', '·'])
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `X`: a label that only names credits.
fn is_credit_label(t: &str) -> bool {
    let n = norm_tag(t);
    if WRITER_TAGS.contains(&n.as_str()) || CREDIT_TAGS.contains(&n.as_str()) {
        return true;
    }
    let pieces = tag_pieces(&n);
    !pieces.is_empty() && pieces.iter().all(|p| is_credit_tag(p))
}

fn is_writer_label(t: &str) -> bool {
    let n = norm_tag(t);
    WRITER_TAGS.contains(&n.as_str())
        || (is_credit_label(t)
            && tag_pieces(&n)
                .iter()
                .any(|p| WRITER_TAGS.contains(&p.as_str())))
}

fn split_label(t: &str) -> Option<(&str, &str)> {
    let i = t.find([':', '：'])?;
    let sep = t[i..].chars().next()?.len_utf8();
    let (name, rest) = (&t[..i], t[i + sep..].trim_start());
    (!name.is_empty() && !rest.is_empty()).then_some((name, rest))
}

/// `yt`: a "Label: names" credit line rather than a lyric.
fn is_credit_line(t: &str) -> bool {
    split_label(t.trim()).is_some_and(|(name, _)| is_credit_label(name))
}

fn split_names(t: &str) -> Vec<String> {
    t.split(['/', '、', '，', ','])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

/// `tt`: the writers named on a credit line.
fn writers_on(t: &str) -> Vec<String> {
    match split_label(t.trim()) {
        Some((name, rest)) if is_writer_label(name) => split_names(rest),
        _ => vec![],
    }
}

fn dedup(names: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let n = n.trim().to_owned();
        if !n.is_empty() && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

// ------------------------------------------------------------------- LRC

/// `V`: an LRC timestamp (`mm:ss.xx`, `h:mm:ss.xx` or seconds) in ms.
// ------------------------------------------------------------------- TTML

pub struct Ttml {
    pub lines: Vec<Line>,
    pub word_synced: bool,
    pub language: Option<String>,
    pub songwriters: Vec<String>,
}

/// `C` (parseTTMLTime): `1.5s` / `2m` / `1h` / `250ms`, or `h:m:s.fff`.
pub fn ttml_time(t: &str) -> i32 {
    let t = t.trim();
    if t.is_empty() {
        return 0;
    }
    for (suffix, mul) in [
        ("ms", 1.0),
        ("h", 3_600_000.0),
        ("m", 60_000.0),
        ("s", 1000.0),
    ] {
        if let Some(n) = t.strip_suffix(suffix)
            && !n.is_empty()
            && n.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        {
            return (n.parse::<f64>().unwrap_or(0.0) * mul).round() as i32;
        }
    }
    let s: Vec<f64> = t
        .split(':')
        .map(|p| {
            p.chars()
                .filter(|c| c.is_ascii_digit() || *c == '.')
                .collect::<String>()
                .parse::<f64>()
                .unwrap_or(0.0)
        })
        .collect();
    let ms = match s.len() {
        1 => s[0] * 1000.0,
        2 => s[0].trunc() * 60_000.0 + s[1] * 1000.0,
        3 => s[0].trunc() * 3_600_000.0 + s[1].trunc() * 60_000.0 + s[2] * 1000.0,
        _ => 0.0,
    };
    ms.round() as i32
}

/// `At`: declare prefixes the file uses without binding (an XML parser refuses them).
fn bind_prefixes(t: &str) -> String {
    let Some(start) = t.find("<tt") else {
        return t.to_owned();
    };
    let Some(len) = t[start..].find('>') else {
        return t.to_owned();
    };
    let tag = &t[start..start + len + 1];
    let mut declared: Vec<String> = vec!["xml".into(), "xmlns".into()];
    for (i, _) in tag.match_indices("xmlns:") {
        let name: String = tag[i + 6..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '-'))
            .collect();
        declared.push(name);
    }
    let mut used: Vec<String> = Vec::new();
    let bytes: Vec<char> = t.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        // `<prefix:` / `</prefix:` and ` prefix:attr=`
        let at_tag = bytes[i] == '<' || (bytes[i] == '/' && i > 0 && bytes[i - 1] == '<');
        let at_attr = bytes[i].is_whitespace();
        if at_tag || at_attr {
            let mut j = i + 1;
            let mut name = String::new();
            while j < bytes.len()
                && (bytes[j].is_alphanumeric() || matches!(bytes[j], '_' | '.' | '-'))
            {
                name.push(bytes[j]);
                j += 1;
            }
            if !name.is_empty()
                && name.starts_with(|c: char| c.is_ascii_alphabetic())
                && j < bytes.len()
                && bytes[j] == ':'
            {
                let ok = if at_attr {
                    let mut k = j + 1;
                    while k < bytes.len()
                        && (bytes[k].is_alphanumeric() || matches!(bytes[k], '_' | '.' | '-'))
                    {
                        k += 1;
                    }
                    while k < bytes.len() && bytes[k].is_whitespace() {
                        k += 1;
                    }
                    k < bytes.len() && bytes[k] == '='
                } else {
                    true
                };
                if ok && !used.contains(&name) {
                    used.push(name);
                }
            }
        }
        i += 1;
    }
    let missing: Vec<&String> = used.iter().filter(|u| !declared.contains(u)).collect();
    if missing.is_empty() {
        return t.to_owned();
    }
    let extra: String = missing
        .iter()
        .map(|m| format!(" xmlns:{m}=\"urn:braccato:unbound:{m}\""))
        .collect();
    let new_tag = format!("{}{}>", &tag[..tag.len() - 1], extra);
    t.replacen(tag, &new_tag, 1)
}

fn attr<'a>(n: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == name).map(|a| a.value())
}

/// `D`: text of a node and its spans.
fn span_text(n: roxmltree::Node) -> String {
    let mut s = String::new();
    for c in n.children() {
        if let Some(t) = c.text().filter(|_| c.is_text()) {
            s.push_str(t);
        } else if c.is_element() && c.tag_name().name() == "span" {
            s.push_str(&span_text(c));
        }
    }
    s
}

/// `U`: a paragraph's (or transliteration's) children as parts and text.
fn ttml_parts(node: roxmltree::Node, line_start: i32) -> (Vec<Part>, String, bool) {
    let mut s = String::new();
    let mut e: Vec<Part> = Vec::new();
    let mut timed = false;
    for u in node.children() {
        let bg = u.is_element() && attr(u, "role") == Some("x-bg");
        let group: Vec<roxmltree::Node> = if bg { u.children().collect() } else { vec![u] };
        for m in group {
            if m.is_text() {
                let t = m.text().unwrap_or_default();
                if t.is_empty() {
                    continue;
                }
                s.push_str(t);
                let start = e.last().map_or(line_start, |p| p.start + p.dur);
                e.push(Part {
                    start,
                    dur: 0,
                    text: t.to_owned(),
                    bg,
                });
            } else if m.is_element() && m.tag_name().name() == "span" {
                let p = span_text(m);
                if p.is_empty() {
                    continue;
                }
                let b = ttml_time(attr(m, "begin").unwrap_or_default());
                let h = ttml_time(attr(m, "end").unwrap_or_default());
                e.push(Part {
                    start: b,
                    dur: h - b,
                    text: p.clone(),
                    bg,
                });
                s.push_str(&p);
                timed = true;
            }
        }
    }
    if !timed {
        e.clear();
    }
    (e, s, timed)
}

/// `parseTTMLContent`: Apple-style TTML with agents, background vocals, and the
/// translations and transliterations in its iTunes metadata.
pub fn parse_ttml(text: &str, song_ms: i32) -> Ttml {
    let empty = |language| Ttml {
        lines: vec![],
        word_synced: false,
        language,
        songwriters: vec![],
    };
    let src = bind_prefixes(&text.replace("\\\"", "\""));
    let Ok(doc) = roxmltree::Document::parse_with_options(
        &src,
        roxmltree::ParsingOptions {
            allow_dtd: true,
            ..Default::default()
        },
    ) else {
        return empty(None);
    };
    let tt = doc.root_element();
    if tt.tag_name().name() != "tt" {
        return empty(None);
    }
    let body = tt.children().find(|n| n.tag_name().name() == "body");
    let language = attr(tt, "lang")
        .or_else(|| body.and_then(|b| attr(b, "lang")))
        .map(str::to_owned);
    let Some(body) = body else {
        return empty(language);
    };
    let head_meta = tt
        .children()
        .find(|n| n.tag_name().name() == "head")
        .and_then(|h| h.children().find(|n| n.tag_name().name() == "metadata"));
    // Agents: people and characters are v1, v2, …; anything else sings for everyone.
    let mut agents: HashMap<String, String> = HashMap::new();
    if let Some(m) = head_meta {
        let mut k = 0;
        for a in m.children().filter(|n| n.tag_name().name() == "agent") {
            let Some(id) = attr(a, "id") else { continue };
            let kind = attr(a, "type").unwrap_or_default();
            if kind == "person" || kind == "character" {
                k += 1;
                agents.insert(id.to_owned(), format!("v{k}"));
            } else {
                agents.insert(id.to_owned(), "v1000".to_owned());
            }
        }
    }
    let ps: Vec<roxmltree::Node> = body
        .children()
        .filter(|n| n.tag_name().name() == "div")
        .flat_map(|d| d.children().filter(|n| n.tag_name().name() == "p"))
        .collect();
    if ps.is_empty() {
        return empty(language);
    }
    let mut lines: Vec<Line> = Vec::new();
    let mut keys: HashMap<String, Vec<usize>> = HashMap::new();
    let mut word_synced = false;
    for p in ps {
        let Some(begin) = attr(p, "begin") else {
            continue;
        };
        let start = ttml_time(begin);
        let end = ttml_time(attr(p, "end").unwrap_or_default());
        let (parts, text, timed) = ttml_parts(p, start);
        word_synced |= timed;
        let agent =
            attr(p, "agent").map(|a| agents.get(a).cloned().unwrap_or_else(|| a.to_owned()));
        if let Some(k) = attr(p, "key") {
            keys.entry(k.to_owned()).or_default().push(lines.len());
        }
        lines.push(Line {
            start,
            dur: end - start,
            text,
            parts,
            agent,
            ..Default::default()
        });
    }
    // Translations and transliterations, matched to lines by key.
    let meta_root = head_meta
        .and_then(|m| {
            m.descendants()
                .find(|n| n.tag_name().name() == "iTunesMetadata")
        })
        .or(head_meta);
    if let Some(m) = meta_root {
        if let Some(ts) = m
            .descendants()
            .find(|n| n.tag_name().name() == "translations")
        {
            let default_lang = attr(ts, "lang");
            for t in ts
                .children()
                .filter(|n| n.tag_name().name() == "translation")
            {
                let lang = attr(t, "lang").or(default_lang);
                for item in t.children().filter(|n| n.is_element()) {
                    let Some(key) = attr(item, "for").or_else(|| attr(t, "for")) else {
                        continue;
                    };
                    let txt = span_text(item);
                    let (Some(lang), false) = (lang, txt.is_empty()) else {
                        continue;
                    };
                    for &i in keys.get(key).map(Vec::as_slice).unwrap_or_default() {
                        lines[i].translations.push((lang.to_owned(), txt.clone()));
                    }
                }
            }
        }
        if let Some(ts) = m
            .descendants()
            .find(|n| n.tag_name().name() == "transliterations")
        {
            for t in ts
                .children()
                .filter(|n| n.tag_name().name() == "transliteration")
            {
                for item in t.children().filter(|n| n.is_element()) {
                    let Some(key) = attr(item, "for") else {
                        continue;
                    };
                    for &i in keys.get(key).map(Vec::as_slice).unwrap_or_default() {
                        let (parts, text, _) = ttml_parts(item, lines[i].start);
                        if text.is_empty() {
                            continue;
                        }
                        lines[i].romanization = Some(text);
                        lines[i].timed_roman = parts;
                    }
                }
            }
        }
    }
    let song = attr(body, "dur").map(ttml_time).unwrap_or(song_ms);
    let songwriters = {
        let mut n = Vec::new();
        if let Some(m) = head_meta {
            if let Some(sw) = m
                .descendants()
                .find(|n| n.tag_name().name() == "songwriters")
            {
                for w in sw
                    .children()
                    .filter(|n| n.tag_name().name() == "songwriter")
                {
                    n.push(span_text(w));
                }
            }
            for meta in m.children().filter(|n| n.tag_name().name() == "meta") {
                if matches!(attr(meta, "key"), Some("songwriter" | "songwriters"))
                    && let Some(v) = attr(meta, "value")
                {
                    n.push(v.to_owned());
                }
            }
        }
        dedup(n)
    };
    Ttml {
        lines: insert_instrumentals(lines, song),
        word_synced,
        language,
        songwriters,
    }
}

#[cfg(test)]
mod sidra_tests {
    use super::*;

    /// Apple's TTML: a second singer's line, and background vocals inside a line.
    #[test]
    fn singers_and_background_vocals() {
        let ttml = r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata"><body><div>
<p begin="1.0" end="3.0" ttm:agent="v1"><span begin="1.0" end="2.0">Hello</span> <span begin="2.0" end="3.0">there</span><span ttm:role="x-bg"><span begin="2.5" end="3.0">(oh)</span></span></p>
<p begin="3.5" end="5.0" ttm:agent="v2"><span begin="3.5" end="5.0">Reply</span></p>
</div></body></tt>"#;
        let t = parse_ttml(ttml, 0);
        assert!(t.word_synced);
        assert_eq!(t.lines[0].agent.as_deref(), Some("v1"));
        assert_eq!(t.lines[1].agent.as_deref(), Some("v2"));
        assert!(
            t.lines[0]
                .parts
                .iter()
                .any(|p| p.bg && p.text.contains("oh"))
        );
        assert!(
            t.lines[0]
                .parts
                .iter()
                .any(|p| !p.bg && p.text.contains("Hello"))
        );
    }
}
