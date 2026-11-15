//! The one shader type `karaoke.rs` names. Sidra passes no glass chip, so `frost` is never
//! reached; it draws a plain translucent disc in place of ytm-rs's blur.

pub struct Frost {
    pub look: [f32; 3],
    pub radius: f32,
    pub opacity: f32,
    pub fade: f32,
}

pub fn frost(painter: &egui::Painter, rect: egui::Rect, f: Frost) {
    let _ = f.look;
    let _ = f.fade;
    painter.rect_filled(
        rect,
        f.radius,
        egui::Color32::from_white_alpha((f.opacity * 40.0) as u8),
    );
}
