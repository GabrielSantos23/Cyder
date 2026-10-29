//! Cache de imagens com limite: sem ele, o `img()` do GPUI guarda para sempre cada capa decodificada
//! (e a textura dela na GPU), e a RAM só cresce enquanto se navega. Baseado no exemplo `image_gallery` do GPUI.
use std::{collections::HashMap, sync::Arc};

use gpui::{App, AppContext, Context, ElementId, Entity, ImageCache, ImageCacheError, ImageCacheItem, ImageCacheProvider, RenderImage, Resource, Window, hash};

pub fn lru(id: impl Into<ElementId>, max_items: usize) -> LruProvider {
    LruProvider { id: id.into(), max_items }
}

pub struct LruProvider {
    id: ElementId,
    max_items: usize,
}

impl ImageCacheProvider for LruProvider {
    fn provide(&mut self, window: &mut Window, cx: &mut App) -> gpui::AnyImageCache {
        window
            .with_global_id(self.id.clone(), |global_id, window| {
                window.with_element_state::<Entity<Lru>, _>(global_id, |cache, _| {
                    let cache = cache.unwrap_or_else(|| cx.new(|cx| Lru::new(self.max_items, cx)));
                    (cache.clone(), cache)
                })
            })
            .into()
    }
}

pub struct Lru {
    max_items: usize,
    /// Mais recente primeiro.
    usages: Vec<u64>,
    items: HashMap<u64, ImageCacheItem>,
}

impl Lru {
    fn new(max_items: usize, cx: &mut Context<Self>) -> Self {
        cx.on_release(|lru, cx| {
            for (_, item) in std::mem::take(&mut lru.items) {
                if let Some(Ok(image)) = item.get() {
                    cx.drop_image(image, None);
                }
            }
        })
        .detach();
        Self { max_items, usages: Vec::with_capacity(max_items), items: HashMap::with_capacity(max_items) }
    }
}

impl ImageCache for Lru {
    fn load(&mut self, resource: &Resource, window: &mut Window, cx: &mut App) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        let key = hash(resource);
        if let Some(item) = self.items.get_mut(&key) {
            if let Some(ix) = self.usages.iter().position(|k| *k == key) {
                self.usages.remove(ix);
            }
            self.usages.insert(0, key);
            return item.use_image(window);
        }
        // Cheio: sai a usada há mais tempo, liberando a imagem e a textura.
        while self.usages.len() >= self.max_items {
            let Some(oldest) = self.usages.pop() else { break };
            if let Some(Ok(image)) = self.items.remove(&oldest).and_then(|item| item.get()) {
                cx.drop_image(image, Some(window));
            }
        }
        let item = ImageCacheItem::new(resource, cx);
        let result = item.use_image(window);
        self.items.insert(key, item);
        self.usages.insert(0, key);
        result
    }
}
