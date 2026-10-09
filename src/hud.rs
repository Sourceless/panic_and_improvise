//! What is drawn over the view that isn't the crosshair or the ammunition: what can be picked up,
//! what has just happened, and the circle of a telescopic sight.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::pickup::PickupPrompt;
use crate::weapon::Gun;

/// How far on the sights the gun must be before the scope fills the view, and the eased fade after it.
const SCOPE_FROM: f32 = 0.55;
const SCOPE_FULL: f32 = 0.92;

/// How much of the view the telescope's picture fills: its radius as a fraction of the screen's shorter side.
const SCOPE_RADIUS: f32 = 0.47;

/// How much of the scope picture is up, 0 to 1, for a gun that is `blend` of the way onto its sights.
pub fn scope_opacity(blend: f32) -> f32 {
    ((blend - SCOPE_FROM) / (SCOPE_FULL - SCOPE_FROM)).clamp(0.0, 1.0)
}

pub fn draw_hud(mut contexts: EguiContexts, prompt: Res<PickupPrompt>, guns: Query<&Gun>) -> Result {
    let ctx = contexts.ctx_mut()?;
    // The telescope.
    if let Ok(gun) = guns.single() {
        let opacity = if gun.def().scope.is_some() && !gun.holstered { scope_opacity(gun.aim_blend) } else { 0.0 };
        if opacity > 0.0 {
            draw_scope(ctx, opacity);
        }
    }
    // What can be picked up, and what has just happened.
    if prompt.looking_at.is_some() || !prompt.message.is_empty() {
        egui::Area::new(egui::Id::new("pickup prompt")).anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -140.0)).interactable(false).show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                if let Some(text) = &prompt.looking_at {
                    ui.label(egui::RichText::new(text).size(20.0).strong().color(egui::Color32::WHITE));
                    ui.label(egui::RichText::new("E: pick up").size(15.0).color(egui::Color32::from_rgb(230, 210, 120)));
                }
                if !prompt.message.is_empty() {
                    ui.label(egui::RichText::new(&prompt.message).size(16.0).color(egui::Color32::from_rgb(230, 230, 230)));
                }
            });
        });
    }
    Ok(())
}

fn draw_scope(ctx: &egui::Context, opacity: f32) {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("scope")));
    let centre = screen.center();
    let radius = screen.height().min(screen.width()) * SCOPE_RADIUS;
    let alpha = (opacity * 255.0) as u8;
    let black = egui::Color32::from_rgba_unmultiplied(0, 0, 0, alpha);
    // Everything outside the circle is black: a ring of stroke wide enough to reach the corners.
    let reach = screen.size().length();
    painter.circle_stroke(centre, radius + reach * 0.5, egui::Stroke::new(reach, black));
    painter.circle_stroke(centre, radius, egui::Stroke::new(3.0, black));
    // The reticle: a fine cross, thick toward the edge, with a gap in the middle and range marks.
    let line = egui::Stroke::new(1.3, egui::Color32::from_rgba_unmultiplied(0, 0, 0, (alpha as f32 * 0.9) as u8));
    let thick = egui::Stroke::new(4.0, egui::Color32::from_rgba_unmultiplied(0, 0, 0, (alpha as f32 * 0.9) as u8));
    let gap = radius * 0.04;
    let thick_from = radius * 0.28;
    for dir in [egui::vec2(1.0, 0.0), egui::vec2(-1.0, 0.0), egui::vec2(0.0, 1.0), egui::vec2(0.0, -1.0)] {
        painter.line_segment([centre + dir * gap, centre + dir * thick_from], line);
        painter.line_segment([centre + dir * thick_from, centre + dir * (radius - 2.0)], thick);
    }
    // A mark every so often down the lower bar, and along the cross.
    for k in 1..=5 {
        let d = gap + radius * 0.055 * k as f32;
        painter.circle_filled(centre + egui::vec2(0.0, d), 1.6, line.color);
        painter.circle_filled(centre + egui::vec2(d, 0.0), 1.6, line.color);
        painter.circle_filled(centre + egui::vec2(-d, 0.0), 1.6, line.color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scope_comes_up_as_the_gun_does_and_is_full_on_the_sights() {
        assert_eq!(scope_opacity(0.0), 0.0);
        assert_eq!(scope_opacity(SCOPE_FROM), 0.0);
        assert_eq!(scope_opacity(1.0), 1.0);
        assert!(scope_opacity(0.75) > 0.0 && scope_opacity(0.75) < 1.0);
    }
}
