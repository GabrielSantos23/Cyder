use std::time::Duration;

use serde_json::Value;
use tokio::sync::Mutex;

use crate::{ArtistInfo, Card, Device, Kind, Library, Page, SearchKind, SearchResults, Track, auth::WebAuth, player::Repeat};

/// Estado do player da conta pela Web API: o que outro aparelho está tocando.
pub struct PlayerState {
    pub device_id: String,
    pub device: String,
    pub volume_percent: Option<u64>,
    pub playing: bool,
    pub position_ms: u32,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub track: Option<Track>,
}
use serde_json::json;

const BASE: &str = "https://api.spotify.com/v1";

/// O login é de antes do Cyder pedir a permissão de enviar imagens.
pub const COVER_PERMISSION: &str = "Falta a permissão para enviar capas. Em Configurações, clique em Trocar no Client ID e depois em Salvar e entrar para dar a permissão.";

/// Retry-After até isto: espera e tenta de novo. Mais que isso, falha na hora em vez de deixar a tela carregando.
const MAX_WAIT_SECS: u64 = 5;

pub struct WebApi {
    http: reqwest::Client,
    auth: Mutex<WebAuth>,
    /// Até quando o Spotify mandou parar (429 com Retry-After longo). Nesse meio-tempo os pedidos falham sem sair.
    blocked: std::sync::Mutex<Option<std::time::Instant>>,
    /// Avisa a UI quando o bloqueio começa, para mostrar até quando.
    events: tokio::sync::mpsc::UnboundedSender<crate::AppEvent>,
    ceiling: std::sync::Mutex<Ceiling>,
}

/// Teto de segurança: pedidos à Web API por minuto. Uso normal fica bem abaixo (abrir ~2, atualizar a
/// biblioteca ~9, digitar uma busca ~10); um defeito que dispare pedidos em laço (como o timer que mandou
/// ~380 num segundo) para aqui, em vez de o Spotify bloquear o Client ID por horas.
const MAX_PER_MINUTE: usize = 90;

#[derive(Default)]
struct Ceiling {
    sent: std::collections::VecDeque<std::time::Instant>,
    /// Já avisou no log nesta rajada (um aviso por rajada, não um por pedido recusado).
    warned: bool,
}

impl Ceiling {
    /// Conta um pedido agora; false se o último minuto já teve `MAX_PER_MINUTE`.
    fn take(&mut self, now: std::time::Instant) -> bool {
        while self.sent.front().is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60)) {
            self.sent.pop_front();
        }
        if self.sent.len() >= MAX_PER_MINUTE {
            return false;
        }
        self.sent.push_back(now);
        self.warned = false;
        true
    }
}

fn limit_message(left: Duration) -> String {
    let min = left.as_secs().div_ceil(60);
    format!("O Spotify limitou os pedidos do seu Client ID. Tente de novo em {}.", if min <= 1 { "1 minuto".to_string() } else { format!("{min} minutos") })
}

impl WebApi {
    pub fn new(http: reqwest::Client, auth: WebAuth, events: tokio::sync::mpsc::UnboundedSender<crate::AppEvent>) -> Self {
        Self { http, auth: Mutex::new(auth), blocked: std::sync::Mutex::new(None), events, ceiling: Default::default() }
    }

    /// Antes de cada pedido: o Spotify mandou parar, ou o Cyder mesmo passou do teto do minuto.
    fn check_blocked(&self) -> Result<(), String> {
        let now = std::time::Instant::now();
        if let Some(until) = *self.blocked.lock().unwrap_or_else(|e| e.into_inner()) {
            if until > now {
                return Err(limit_message(until - now));
            }
        }
        let mut ceiling = self.ceiling.lock().unwrap_or_else(|e| e.into_inner());
        if !ceiling.take(now) {
            if !std::mem::replace(&mut ceiling.warned, true) {
                log::warn!("Web API: {MAX_PER_MINUTE} pedidos no último minuto; segurando os próximos para o Spotify não bloquear o Client ID");
            }
            return Err("Muitos pedidos ao Spotify em pouco tempo. Tente de novo em instantes.".into());
        }
        Ok(())
    }

    /// 429: com espera curta, dorme e o chamador tenta de novo (Ok); com espera longa, bloqueia até lá (Err).
    async fn rate_limited(&self, res: &reqwest::Response, path: &str) -> Result<(), String> {
        let secs = res.headers().get("retry-after").and_then(|h| h.to_str().ok()?.parse().ok()).unwrap_or(2);
        log::warn!("Web API: limite de pedidos em {path}; o Spotify pediu {secs} s");
        if secs <= MAX_WAIT_SECS {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            return Ok(());
        }
        let wait = Duration::from_secs(secs);
        *self.blocked.lock().unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now() + wait);
        let _ = self.events.send(crate::AppEvent::RateLimited { secs });
        Err(limit_message(wait))
    }

    /// GET com retentativa em 429 curto (respeita `Retry-After`).
    async fn get(&self, path: &str) -> Result<Value, String> {
        log::debug!("Web API: GET {path}");
        for _ in 0..5 {
            self.check_blocked()?;
            let token = self.auth.lock().await.access_token().await?;
            let res = self.http.get(format!("{BASE}{path}")).bearer_auth(token).send().await.map_err(|e| e.to_string())?;
            if res.status() == 429 {
                self.rate_limited(&res, path).await?;
                continue;
            }
            let status = res.status();
            // 204: sem conteúdo (ex.: nada tocando em /me/player).
            if status == 204 {
                return Ok(Value::Null);
            }
            let body: Value = res.json().await.map_err(|e| e.to_string())?;
            if !status.is_success() {
                let msg = body.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("erro desconhecido");
                return Err(format!("Spotify respondeu {status}: {msg}"));
            }
            return Ok(body);
        }
        Err("O Spotify está limitando as requisições. Tente de novo em instantes.".into())
    }

    async fn put(&self, path: &str, body: Value) -> Result<(), String> {
        self.write(reqwest::Method::PUT, path, body).await
    }

    /// PUT/DELETE sem resposta útil (o Spotify devolve 200 ou 204), com retentativa em 429.
    async fn write(&self, method: reqwest::Method, path: &str, body: Value) -> Result<(), String> {
        self.send(method, path, body).await.map(drop)
    }

    /// POST/PUT/DELETE com corpo JSON; devolve a resposta (Null quando vem vazia).
    async fn send(&self, method: reqwest::Method, path: &str, body: Value) -> Result<Value, String> {
        log::debug!("Web API: {method} {path}");
        for _ in 0..5 {
            self.check_blocked()?;
            let token = self.auth.lock().await.access_token().await?;
            let res = self.http.request(method.clone(), format!("{BASE}{path}")).bearer_auth(token).json(&body).send().await.map_err(|e| e.to_string())?;
            if res.status() == 429 {
                self.rate_limited(&res, path).await?;
                continue;
            }
            let status = res.status();
            if status.is_success() {
                return Ok(res.json().await.unwrap_or_default());
            }
            let body: Value = res.json().await.unwrap_or_default();
            let msg = body.pointer("/error/message").and_then(|m| m.as_str()).unwrap_or("erro desconhecido");
            return Err(format!("Spotify respondeu {status}: {msg}"));
        }
        Err("O Spotify está limitando as requisições. Tente de novo em instantes.".into())
    }

    /// Últimas 50 faixas tocadas em qualquer aparelho, com quando e de onde (álbum/playlist).
    /// O que toca pelo Cyder não entra aqui (doc 07).
    pub async fn recently_played(&self) -> Result<Vec<Played>, String> {
        let body = self.get("/me/player/recently-played?limit=50").await?;
        Ok(body["items"].as_array().into_iter().flatten().filter_map(|i| {
            Some(Played {
                track: parse_track(&i["track"])?,
                at: i["played_at"].as_str().and_then(iso_to_unix).unwrap_or(0),
                context: i["context"]["uri"].as_str().map(String::from),
                album: album_card(&i["track"]["album"]),
            })
        }).collect())
    }

    pub async fn top_artists(&self) -> Result<Vec<Card>, String> {
        let body = self.get("/me/top/artists?limit=20&time_range=short_term").await?;
        Ok(body["items"].as_array().into_iter().flatten().filter_map(artist_card).collect())
    }

    /// Aparelhos Spotify Connect da conta, incluindo o próprio Cyder.
    pub async fn devices(&self) -> Result<Vec<Device>, String> {
        let body = self.get("/me/player/devices").await?;
        Ok(body["devices"].as_array().map(|d| d.iter().filter_map(parse_device).collect()).unwrap_or_default())
    }

    /// Nome e foto da conta, para o rodapé da sidebar.
    pub async fn me(&self) -> Result<(String, Option<String>), String> {
        let me = self.get("/me").await?;
        let name = me["display_name"].as_str().filter(|n| !n.is_empty()).or(me["id"].as_str()).unwrap_or_default();
        Ok((name.to_string(), pick_image(&me["images"])))
    }

    /// O que está tocando na conta, em qualquer aparelho. None: nada tocando.
    pub async fn player_state(&self) -> Result<Option<PlayerState>, String> {
        let b = self.get("/me/player").await?;
        if b.is_null() {
            return Ok(None);
        }
        Ok(Some(PlayerState {
            device_id: b["device"]["id"].as_str().unwrap_or_default().to_string(),
            device: b["device"]["name"].as_str().unwrap_or_default().to_string(),
            volume_percent: b["device"]["volume_percent"].as_u64(),
            playing: b["is_playing"].as_bool().unwrap_or(false),
            position_ms: b["progress_ms"].as_u64().unwrap_or(0) as u32,
            shuffle: b["shuffle_state"].as_bool().unwrap_or(false),
            repeat: match b["repeat_state"].as_str() {
                Some("track") => Repeat::One,
                Some("context") => Repeat::All,
                _ => Repeat::Off,
            },
            track: parse_track(&b["item"]),
        }))
    }

    /// As próximas da fila do Spotify (até ~20), já na ordem do aleatório e com o autoplay.
    pub async fn queue(&self) -> Result<Vec<Track>, String> {
        let b = self.get("/me/player/queue").await?;
        Ok(b["queue"].as_array().into_iter().flatten().filter_map(parse_track).collect())
    }

    /// Controle do aparelho ativo quando ele não é o Cyder (`/me/player/...`).
    pub async fn control(&self, method: reqwest::Method, path: &str) -> Result<(), String> {
        self.write(method, &format!("/me/player{path}"), json!({})).await
    }

    /// Salva ou tira da biblioteca (faixas curtidas, álbuns, artistas seguidos), pelo `/me/library` de 2026.
    pub async fn set_saved(&self, uris: &[String], save: bool) -> Result<(), String> {
        let method = if save { reqwest::Method::PUT } else { reqwest::Method::DELETE };
        self.write(method, &format!("/me/library?uris={}", uris.join(",")), json!({})).await
    }

    /// Quais destas URIs estão salvas (curtidas, no caso de faixas). Em lotes de 40.
    pub async fn saved_contains(&self, uris: &[String]) -> Result<Vec<bool>, String> {
        let mut out = Vec::with_capacity(uris.len());
        for chunk in uris.chunks(40) {
            let body = self.get(&format!("/me/library/contains?uris={}", chunk.join(","))).await?;
            let flags: Vec<bool> = body.as_array().map(|a| a.iter().map(|v| v.as_bool().unwrap_or(false)).collect()).unwrap_or_default();
            if flags.len() != chunk.len() {
                return Err("resposta inesperada de /me/library/contains".into());
            }
            out.extend(flags);
        }
        Ok(out)
    }

    pub async fn add_to_playlist(&self, playlist_id: &str, uri: &str) -> Result<(), String> {
        self.write(reqwest::Method::POST, &format!("/playlists/{playlist_id}/items"), json!({ "uris": [uri] })).await
    }

    pub async fn add_to_queue(&self, uri: &str) -> Result<(), String> {
        self.write(reqwest::Method::POST, &format!("/me/player/queue?uri={uri}"), json!({})).await
    }

    /// Playlist nova, privada, da conta.
    pub async fn create_playlist(&self, name: &str) -> Result<Card, String> {
        let p = self.send(reqwest::Method::POST, "/me/playlists", json!({ "name": name, "public": false })).await?;
        playlist_card(&p).ok_or_else(|| "o Spotify não devolveu a playlist criada".into())
    }

    pub async fn rename_playlist(&self, id: &str, name: &str) -> Result<(), String> {
        self.put(&format!("/playlists/{id}"), json!({ "name": name })).await
    }

    /// Tira todas as ocorrências da faixa. O campo mudou de `tracks` para `items` em 2026; vão os dois.
    pub async fn remove_from_playlist(&self, id: &str, uri: &str) -> Result<(), String> {
        self.write(reqwest::Method::DELETE, &format!("/playlists/{id}/items"), json!({ "items": [{ "uri": uri }], "tracks": [{ "uri": uri }] })).await
    }

    /// Move o item da posição `from` para ficar na posição `to`.
    pub async fn move_in_playlist(&self, id: &str, from: usize, to: usize) -> Result<(), String> {
        // O Spotify pede "inserir antes de": descendo, é depois do item que estava em `to`.
        let insert_before = if to > from { to + 1 } else { to };
        self.put(&format!("/playlists/{id}/items"), json!({ "range_start": from, "insert_before": insert_before, "range_length": 1 })).await
    }

    /// Capa nova: JPEG em base64, no máximo 256 KB.
    pub async fn set_playlist_cover(&self, id: &str, jpeg_base64: String) -> Result<(), String> {
        log::debug!("Web API: PUT /playlists/{id}/images");
        self.check_blocked()?;
        let token = self.auth.lock().await.access_token().await?;
        let res = self.http.put(format!("{BASE}/playlists/{id}/images")).bearer_auth(token).header("content-type", "image/jpeg").body(jpeg_base64).send().await.map_err(|e| e.to_string())?;
        match res.status() {
            s if s.is_success() => Ok(()),
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => Err(COVER_PERMISSION.into()),
            s => Err(format!("Spotify respondeu {s}")),
        }
    }

    /// Deixa de seguir a playlist: some da biblioteca (dá para seguir de novo; não apaga a playlist).
    pub async fn unfollow_playlist(&self, id: &str) -> Result<(), String> {
        self.write(reqwest::Method::DELETE, &format!("/playlists/{id}/followers"), json!({})).await
    }

    /// Passa a reprodução para outro aparelho, continuando a tocar.
    pub async fn transfer(&self, device_id: &str) -> Result<(), String> {
        self.put("/me/player", json!({ "device_ids": [device_id], "play": true })).await
    }

    /// Músicas curtidas, da mais recente para a mais antiga.
    pub async fn liked_tracks(&self, limit: usize) -> Result<Vec<Track>, String> {
        let mut out = Vec::new();
        while out.len() < limit {
            let page = self.get(&format!("/me/tracks?limit=50&offset={}", out.len())).await?;
            let items = page["items"].as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                break;
            }
            out.extend(items.iter().filter_map(|i| parse_track(&i["track"])));
        }
        out.truncate(limit);
        Ok(out)
    }

    /// Próximas curtidas a partir de `offset`, até `count`; `true` quando ainda há mais depois delas.
    pub async fn liked_page(&self, offset: usize, count: usize) -> Result<(Vec<Track>, bool), String> {
        let mut out = Vec::new();
        let mut total = usize::MAX;
        while out.len() < count && offset + out.len() < total {
            let page = self.get(&format!("/me/tracks?limit=50&offset={}", offset + out.len())).await?;
            total = page["total"].as_u64().map_or(usize::MAX, |t| t as usize);
            let items = page["items"].as_array().cloned().unwrap_or_default();
            if items.is_empty() {
                total = offset + out.len();
                break;
            }
            out.extend(items.iter().filter_map(|i| parse_track(&i["track"])));
        }
        let more = offset + out.len() < total;
        Ok((out, more))
    }

    /// Junta as páginas seguindo `next`. `key` é o objeto que contém a página
    /// (`/me/following` devolve `{ "artists": { items, next } }`).
    async fn paged(&self, first: String, key: Option<&str>, max: usize) -> Result<Vec<Value>, String> {
        let mut out = Vec::new();
        let mut next = Some(first);
        while let Some(path) = next.take() {
            let body = self.get(&path).await?;
            let page = key.map_or(&body, |k| &body[k]);
            out.extend(page["items"].as_array().cloned().unwrap_or_default());
            if out.len() >= max {
                break;
            }
            next = page["next"].as_str().map(|u| u.trim_start_matches(BASE).to_string());
        }
        out.truncate(max);
        Ok(out)
    }

    pub async fn library(&self) -> Result<Library, String> {
        let albums = self.paged("/me/albums?limit=50".into(), None, LIBRARY_MAX).await?;
        let artists = self.paged("/me/following?type=artist&limit=50".into(), Some("artists"), LIBRARY_MAX).await?;
        let playlists = self.paged("/me/playlists?limit=50".into(), None, LIBRARY_MAX).await?;
        // Sem podcasts a biblioteca abre do mesmo jeito.
        let shows = self.paged("/me/shows?limit=50".into(), None, LIBRARY_MAX).await.unwrap_or_else(|e| {
            log::warn!("podcasts salvos: {e}");
            Vec::new()
        });
        Ok(Library {
            albums: albums.iter().filter_map(|i| album_card(&i["album"])).collect(),
            artists: artists.iter().filter_map(artist_card).collect(),
            playlists: playlists.iter().filter_map(playlist_card).collect(),
            shows: shows.iter().filter_map(|i| show_card(&i["show"])).collect(),
        })
    }

    /// Busca de um ou mais tipos a partir de `offset`. Desde 2026 vêm no máximo 10 de cada tipo por pedido.
    pub async fn search(&self, query: &str, kinds: &[SearchKind], offset: usize) -> Result<SearchResults, String> {
        let mut url = reqwest::Url::parse(BASE).map_err(|e| e.to_string())?;
        let types = kinds.iter().map(|k| k.api()).collect::<Vec<_>>().join(",");
        url.query_pairs_mut().append_pair("q", query).append_pair("type", &types).append_pair("limit", "10").append_pair("offset", &offset.to_string());
        let body = self.get(&format!("/search?{}", url.query().unwrap_or_default())).await?;
        // Playlists e podcasts às vezes vêm como null na lista; o filter_map pula.
        let items = |key: &str| body[key]["items"].as_array().cloned().unwrap_or_default();
        Ok(SearchResults {
            query: query.to_string(),
            tracks: items("tracks").iter().filter_map(parse_track).collect(),
            artists: items("artists").iter().filter_map(artist_card).collect(),
            albums: items("albums").iter().filter_map(album_card).collect(),
            playlists: items("playlists").iter().filter_map(playlist_card).collect(),
            shows: items("shows").iter().filter_map(show_card).collect(),
        })
    }

    pub async fn page(&self, card: &Card) -> Result<Page, String> {
        match card.kind {
            Kind::Album => {
                let a = self.get(&format!("/albums/{}", card.id)).await?;
                let mut items = a["tracks"]["items"].as_array().cloned().unwrap_or_default();
                if let Some(next) = a["tracks"]["next"].as_str() {
                    items.extend(self.paged(next.trim_start_matches(BASE).to_string(), None, 500).await?);
                }
                // As faixas do álbum vêm sem o álbum; copia para ter capa e nome em cada uma.
                let album = json!({ "id": a["id"], "name": a["name"], "images": a["images"] });
                let tracks: Vec<Track> = items.into_iter().filter_map(|mut t| { t["album"] = album.clone(); parse_track(&t) }).collect();
                let year = a["release_date"].as_str().unwrap_or_default().get(..4).unwrap_or_default();
                Ok(Page { card: album_card(&a).unwrap_or_else(|| card.clone()), meta: join_meta(year, tracks.len()), tracks, albums: Vec::new(), note: None, artist: None, owner_image: None, more_from: None, total: 0 })
            }
            Kind::Artist => {
                let a = self.get(&format!("/artists/{}", card.id)).await?;
                // limit=10: o modo de desenvolvimento de 2026 reduziu o máximo de vários endpoints.
                let albums = self.paged(format!("/artists/{}/albums?include_groups=album,compilation&limit=10", card.id), None, 50).await?;
                let singles = self.paged(format!("/artists/{}/albums?include_groups=single&limit=10", card.id), None, 50).await?;
                let genres = a["genres"].as_array().map(|g| g.iter().filter_map(|x| x.as_str()).take(3).collect::<Vec<_>>().join(" · ")).unwrap_or_default();
                // Datas ISO (às vezes só o ano): a maior em texto é a mais recente.
                let latest = albums.iter().chain(&singles).max_by_key(|r| r["release_date"].as_str().unwrap_or_default().to_string())
                    .and_then(|r| Some((release_card(r)?, date_label(r["release_date"].as_str().unwrap_or_default()))));
                Ok(Page {
                    card: artist_card(&a).unwrap_or_else(|| card.clone()),
                    meta: genres,
                    tracks: Vec::new(),
                    albums: albums.iter().chain(&singles).filter_map(release_card).collect(),
                    note: None,
                    owner_image: None, more_from: None, total: 0,
                    artist: Some(ArtistInfo {
                        hero: a["images"].as_array().and_then(|i| i.first()).and_then(|i| i["url"].as_str()).map(String::from),
                        album_count: albums.iter().filter(|r| release_card(r).is_some()).count(),
                        latest,
                        related: Vec::new(),
                    }),
                })
            }
            Kind::Show => {
                let s = self.get(&format!("/shows/{}", card.id)).await?;
                let head = show_card(&s).unwrap_or_else(|| card.clone());
                let mut items = s["episodes"]["items"].as_array().cloned().unwrap_or_default();
                if let Some(next) = s["episodes"]["next"].as_str() {
                    items.extend(self.paged(next.trim_start_matches(BASE).to_string(), None, EPISODES_MAX).await?);
                }
                let tracks: Vec<Track> = items.iter().filter_map(|e| parse_episode(e, &head.title)).take(EPISODES_MAX).collect();
                let total = s["total_episodes"].as_u64().map_or(tracks.len(), |n| n as usize);
                let count = if total == 1 { "1 episódio".to_string() } else { format!("{total} episódios") };
                let meta = if head.subtitle.is_empty() { count } else { format!("{} · {count}", head.subtitle) };
                Ok(Page { meta, card: head, tracks, albums: Vec::new(), note: None, artist: None, owner_image: None, more_from: None, total: 0 })
            }
            Kind::Playlist => {
                let p = self.get(&format!("/playlists/{}?fields=id,uri,name,owner,images", card.id)).await?;
                let head = playlist_card(&p).unwrap_or_else(|| card.clone());
                // Desde 2026 só playlists próprias ou colaborativas listam as faixas (doc 01).
                match self.paged(format!("/playlists/{}/items?limit=50", card.id), None, 1000).await {
                    Ok(items) => {
                        // O campo mudou de `track` para `item` na migração de 2026; aceita os dois.
                        let tracks: Vec<Track> = items.iter().filter_map(|i| parse_track(if i["item"].is_object() { &i["item"] } else { &i["track"] })).collect();
                        Ok(Page { meta: join_meta(&head.subtitle, tracks.len()), card: head, tracks, albums: Vec::new(), note: None, artist: None, owner_image: None, more_from: None, total: 0 })
                    }
                    Err(e) => {
                        log::info!("faixas da playlist {}: {e}", card.id);
                        Ok(Page {
                            meta: head.subtitle.clone(),
                            card: head,
                            tracks: Vec::new(),
                            albums: Vec::new(),
                            note: Some("O Spotify só mostra as faixas de playlists suas. Dá para tocar mesmo assim.".into()),
                            artist: None,
                            owner_image: None, more_from: None, total: 0,
                        })
                    }
                }
            }
        }
    }
}

/// shortcut: no máximo 500 de cada tipo na biblioteca; paginar sob demanda se alguém tiver mais.
const LIBRARY_MAX: usize = 500;
/// shortcut: só os 200 episódios mais novos de um podcast; carregar aos poucos se fizer falta.
const EPISODES_MAX: usize = 200;

pub struct Played {
    pub track: Track,
    /// Segundos Unix.
    pub at: u64,
    pub context: Option<String>,
    pub album: Option<Card>,
}

/// "2026-10-28T16:06:30.123Z" → segundos Unix (UTC). Sem dependência de data só para isso.
fn iso_to_unix(s: &str) -> Option<u64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    // Dias desde 1970-01-01 (algoritmo de Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

fn parse_device(d: &Value) -> Option<Device> {
    Some(Device {
        // Aparelhos restritos vêm sem id e não aceitam transferência.
        id: d["id"].as_str()?.to_string(),
        name: d["name"].as_str()?.to_string(),
        kind: d["type"].as_str().unwrap_or_default().to_string(),
        active: d["is_active"].as_bool().unwrap_or(false),
    })
}

pub(crate) fn join_meta(first: &str, tracks: usize) -> String {
    let count = if tracks == 1 { "1 faixa".to_string() } else { format!("{tracks} faixas") };
    if first.is_empty() { count } else { format!("{first} · {count}") }
}

/// A menor imagem com pelo menos 250 px; a de 640 seria desperdício numa grade.
fn pick_image(images: &Value) -> Option<String> {
    let list = images.as_array()?;
    let fit = list.iter().filter(|i| i["width"].as_u64().is_some_and(|w| w >= 250)).min_by_key(|i| i["width"].as_u64());
    fit.or(list.first()).and_then(|i| i["url"].as_str()).map(String::from)
}

fn names(v: &Value) -> String {
    v.as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default()
}

fn card(kind: Kind, v: &Value, subtitle: String) -> Option<Card> {
    Some(Card {
        kind,
        id: v["id"].as_str()?.to_string(),
        uri: v["uri"].as_str()?.to_string(),
        title: v["name"].as_str()?.to_string(),
        subtitle,
        image: pick_image(&v["images"]),
    })
}

fn album_card(a: &Value) -> Option<Card> {
    card(Kind::Album, a, names(&a["artists"]))
}

/// Lançamento na discografia do artista: o artista já está no topo, então o subtítulo é ano e tipo.
fn release_card(a: &Value) -> Option<Card> {
    let year = a["release_date"].as_str().unwrap_or_default().get(..4).unwrap_or_default();
    let kind = match a["album_type"].as_str() {
        Some("single") if a["total_tracks"].as_u64().is_some_and(|n| n > 3) => "EP",
        Some("single") => "Single",
        Some("compilation") => "Coletânea",
        _ => "Álbum",
    };
    card(Kind::Album, a, if year.is_empty() { kind.into() } else { format!("{year} · {kind}") })
}

/// "2026-05-14" → "14 de maio de 2026"; com só ano ou ano-mês, mostra o que tiver.
fn date_label(iso: &str) -> String {
    const MONTHS: [&str; 12] = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
    let mut parts = iso.split('-');
    let (y, m, d) = (parts.next().unwrap_or_default(), parts.next().and_then(|m| m.parse::<usize>().ok()), parts.next().and_then(|d| d.parse::<u32>().ok()));
    match (m.and_then(|m| MONTHS.get(m.wrapping_sub(1))), d) {
        (Some(month), Some(day)) => format!("{day} de {month} de {y}"),
        (Some(month), None) => format!("{month} de {y}"),
        _ => y.to_string(),
    }
}

fn artist_card(a: &Value) -> Option<Card> {
    card(Kind::Artist, a, "Artista".into())
}

fn playlist_card(p: &Value) -> Option<Card> {
    card(Kind::Playlist, p, p["owner"]["display_name"].as_str().unwrap_or_default().to_string())
}

fn show_card(s: &Value) -> Option<Card> {
    // A busca de 2026 às vezes vem sem `publisher`.
    card(Kind::Show, s, s["publisher"].as_str().filter(|p| !p.is_empty()).unwrap_or("Podcast").to_string())
}

/// Episódio como faixa: o podcast fica no lugar do artista, e a data no do álbum.
fn parse_episode(e: &Value, show: &str) -> Option<Track> {
    let images = e["images"].as_array();
    Some(Track {
        id: e["id"].as_str()?.to_string(),
        uri: e["uri"].as_str()?.to_string(),
        name: e["name"].as_str()?.to_string(),
        artist: show.to_string(),
        album: date_label(e["release_date"].as_str().unwrap_or_default()),
        album_id: String::new(),
        artists: Vec::new(),
        duration_ms: e["duration_ms"].as_u64()? as u32,
        cover_large: images.and_then(|i| i.first()).and_then(|i| i["url"].as_str()).map(String::from),
        cover_small: images.and_then(|i| i.last()).and_then(|i| i["url"].as_str()).map(String::from),
    })
}

pub fn parse_track(t: &Value) -> Option<Track> {
    let images = t["album"]["images"].as_array();
    Some(Track {
        id: t["id"].as_str()?.to_string(),
        uri: t["uri"].as_str()?.to_string(),
        name: t["name"].as_str()?.to_string(),
        artist: t["artists"].as_array()?.iter().filter_map(|a| a["name"].as_str()).collect::<Vec<_>>().join(", "),
        album: t["album"]["name"].as_str().unwrap_or_default().to_string(),
        album_id: t["album"]["id"].as_str().unwrap_or_default().to_string(),
        artists: t["artists"].as_array()?.iter().filter_map(|a| Some((a["id"].as_str()?.to_string(), a["name"].as_str()?.to_string()))).collect(),
        duration_ms: t["duration_ms"].as_u64()? as u32,
        // O Spotify devolve as capas da maior (640) para a menor (64).
        cover_large: images.and_then(|i| i.first()).and_then(|i| i["url"].as_str()).map(String::from),
        cover_small: images.and_then(|i| i.last()).and_then(|i| i["url"].as_str()).map(String::from),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn parses_track_with_artists_and_covers() {
        let t = super::parse_track(&json!({
            "id": "1", "uri": "spotify:track:1", "name": "Song", "duration_ms": 1000,
            "artists": [{"name": "A"}, {"name": "B"}],
            "album": {"id": "al", "name": "Album", "images": [{"url": "big"}, {"url": "mid"}, {"url": "small"}]}
        }))
        .unwrap();
        assert_eq!(t.artist, "A, B");
        assert_eq!(t.cover_large.as_deref(), Some("big"));
        assert_eq!(t.cover_small.as_deref(), Some("small"));
        assert!(super::parse_track(&json!({"name": "local file"})).is_none());
    }

    #[test]
    fn picks_smallest_image_that_fits_grid() {
        let imgs = json!([{"url": "640", "width": 640}, {"url": "300", "width": 300}, {"url": "64", "width": 64}]);
        assert_eq!(super::pick_image(&imgs).as_deref(), Some("300"));
        // Capa enviada pelo usuário vem sem largura.
        assert_eq!(super::pick_image(&json!([{"url": "u", "width": null}])).as_deref(), Some("u"));
        assert_eq!(super::pick_image(&json!([])), None);
    }

    #[test]
    fn device_without_id_is_skipped() {
        let d = super::parse_device(&json!({"id": "x", "name": "Cyder", "type": "Computer", "is_active": true})).unwrap();
        assert!(d.active && d.kind == "Computer");
        assert!(super::parse_device(&json!({"id": null, "name": "TV restrita"})).is_none());
    }

    #[test]
    fn date_label_in_portuguese() {
        assert_eq!(super::date_label("2026-05-14"), "14 de maio de 2026");
        assert_eq!(super::date_label("2019-12"), "dezembro de 2019");
        assert_eq!(super::date_label("2007"), "2007");
    }

    #[test]
    fn iso_dates_become_unix_seconds() {
        assert_eq!(super::iso_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(super::iso_to_unix("2026-10-28T16:06:30.123Z"), Some(1_793_203_590));
        assert_eq!(super::iso_to_unix("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        assert_eq!(super::iso_to_unix("lixo"), None);
    }

    #[test]
    fn meta_pluralizes() {
        assert_eq!(super::join_meta("2025", 1), "2025 · 1 faixa");
        assert_eq!(super::join_meta("", 12), "12 faixas");
    }

    #[test]
    fn ceiling_holds_a_burst_and_frees_after_a_minute() {
        let mut c = super::Ceiling::default();
        let t0 = std::time::Instant::now();
        assert!((0..super::MAX_PER_MINUTE).all(|_| c.take(t0)));
        assert!(!c.take(t0 + std::time::Duration::from_secs(59)));
        assert!(c.take(t0 + std::time::Duration::from_secs(60)));
    }
}
