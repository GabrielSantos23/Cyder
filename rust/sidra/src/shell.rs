//! Gradient helpers `karaoke.rs` draws with, copied from ytm-rs's `ui/shell.rs` (GPL-3.0-only).
#![allow(clippy::all)]

use crate::theme;
use egui::{Color32, Rect, vec2};

pub fn vgradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    gradient(painter, rect, [top, top, bottom, bottom]);
}

fn gradient(painter: &egui::Painter, rect: Rect, c: [Color32; 4]) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), c[0]);
    mesh.colored_vertex(rect.right_top(), c[1]);
    mesh.colored_vertex(rect.left_bottom(), c[2]);
    mesh.colored_vertex(rect.right_bottom(), c[3]);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    painter.add(egui::Shape::mesh(mesh));
}

pub fn overlay_light(
    painter: &egui::Painter,
    area: Rect,
    o: &crate::preset::Overlay,
    accent: Color32,
) {
    const STEPS: usize = 40;
    const RINGS: [(f32, f32); 4] = [(0.0, 1.0), (0.4, 0.55), (0.72, 0.18), (1.0, 0.0)];
    let color = theme::with_alpha(crate::preset::paint(&o.color, accent), o.alpha);
    let c = area.min + vec2(area.width() * o.x, area.height() * o.y);
    let (rx, ry) = (area.width() * o.rx, area.height() * o.ry);
    let mut m = egui::Mesh::default();
    for &(t, a) in &RINGS {
        for i in 0..STEPS {
            let (sin, cos) = (i as f32 / STEPS as f32 * std::f32::consts::TAU).sin_cos();
            m.colored_vertex(c + vec2(cos * rx, sin * ry) * t, color.gamma_multiply(a));
        }
    }
    let at = |ring: usize, i: usize| (ring * STEPS + i % STEPS) as u32;
    for ring in 0..RINGS.len() - 1 {
        for i in 0..STEPS {
            m.add_triangle(at(ring, i), at(ring, i + 1), at(ring + 1, i));
            m.add_triangle(at(ring, i + 1), at(ring + 1, i + 1), at(ring + 1, i));
        }
    }
    painter.add(m);
}
