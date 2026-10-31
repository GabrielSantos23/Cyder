#![warn(missing_docs)]
#![doc=include_str!("../README.md")]

#[macro_use]
extern crate log;

use librespot_core as core;
use librespot_playback as playback;
use librespot_protocol as protocol;

mod context_resolver;
mod model;
mod shuffle_vec;
mod spirc;
mod state;

/// Cyder: o id da última reprodução de cada faixa (uri), para o evento do histórico.
pub static PLAYBACK_IDS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, String>>> =
    std::sync::LazyLock::new(Default::default);

/// Cyder: os arquivos de áudio de cada faixa (uri) como (código do formato, id em base16), para o
/// `AudioFileSelection`/`Download` do evento do histórico.
pub static FILE_IDS: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, Vec<(i32, String)>>>> =
    std::sync::LazyLock::new(Default::default);

pub use model::*;
pub use spirc::*;
pub use state::*;
