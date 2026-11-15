//! Engine-independent logic ported from `src/`: nothing here touches a window,
//! a web view or the operating system, so every module is tested on its own.

pub mod i18n;
pub mod itms;
pub mod lastfm;
pub mod music_service;
pub mod storefront;
pub mod theme;
