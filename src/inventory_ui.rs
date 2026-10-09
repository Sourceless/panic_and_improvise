//! The inventory screen: Tab opens it, the mouse picks things up and puts them down, R turns what is
//! being held. The rules are the inventory's own (`inventory`); this only shows it and turns clicks
//! into calls on it.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use crate::controls::{Action, Controls, Keyboard};
use crate::inventory::{Container, Inventory, Item, ItemKind, Place};
use crate::menu::Menu;

/// Pixels to a square.
const CELL: f32 = 44.0;
const GAP: f32 = 18.0;

#[derive(Resource, Default, Debug)]
pub struct InventoryScreen {
    pub open: bool,
    drag: Option<Drag>,
    /// A line at the bottom: why something couldn't be done.
    pub message: String,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    id: u32,
    turned: bool,
    /// Which square of the item was picked up, so it is held by the same part.
    grab: (u32, u32),
}

impl InventoryScreen {
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.drag = None;
        self.message.clear();
    }
}

/// Tab opens and closes the screen; Escape closes it (and doesn't open the menu).
pub fn inventory_keys(keys: Res<Keyboard>, controls: Res<Controls>, menu: Res<Menu>, mut screen: ResMut<InventoryScreen>) {
    if menu.open {
        return;
    }
    if controls.just_pressed(Action::Inventory, &keys) {
        screen.toggle();
    }
}

/// Where everything is drawn, in screen pixels.
struct Layout {
    origin: egui::Pos2,
    pack: Option<(u32, u32)>,
}

impl Layout {
    fn weapon_slot(&self, i: usize) -> egui::Rect {
        let top = self.origin + egui::vec2(i as f32 * (6.0 * CELL + GAP), 24.0);
        egui::Rect::from_min_size(top, egui::vec2(6.0 * CELL, 3.0 * CELL))
    }

    fn back(&self) -> egui::Rect {
        let top = self.origin + egui::vec2(2.0 * (6.0 * CELL + GAP), 24.0);
        egui::Rect::from_min_size(top, egui::vec2(4.0 * CELL, 3.0 * CELL))
    }

    fn pockets(&self) -> egui::Rect {
        let top = self.origin + egui::vec2(0.0, 24.0 + 3.0 * CELL + GAP + 22.0);
        egui::Rect::from_min_size(top, egui::vec2(6.0 * CELL, 4.0 * CELL))
    }

    fn pack(&self) -> Option<egui::Rect> {
        let (w, h) = self.pack?;
        let top = self.origin + egui::vec2(6.0 * CELL + GAP, 24.0 + 3.0 * CELL + GAP + 22.0);
        Some(egui::Rect::from_min_size(top, egui::vec2(w as f32 * CELL, h as f32 * CELL)))
    }

    fn size(&self) -> egui::Vec2 {
        let pack_h = self.pack.map_or(4, |(_, h)| h.max(4)) as f32;
        egui::vec2(2.0 * (6.0 * CELL + GAP) + 4.0 * CELL, 24.0 + 3.0 * CELL + GAP + 22.0 + pack_h * CELL)
    }
}

/// What the pointer is over.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Hover {
    Slot(usize),
    Back,
    Cell(Place, u32, u32),
}

fn hover_at(layout: &Layout, p: egui::Pos2) -> Option<Hover> {
    for i in 0..2 {
        if layout.weapon_slot(i).contains(p) {
            return Some(Hover::Slot(i));
        }
    }
    if layout.back().contains(p) {
        return Some(Hover::Back);
    }
    let cell = |rect: egui::Rect, place: Place| {
        rect.contains(p).then(|| {
            let d = p - rect.min;
            Hover::Cell(place, (d.x / CELL) as u32, (d.y / CELL) as u32)
        })
    };
    cell(layout.pockets(), Place::Pockets).or_else(|| layout.pack().and_then(|r| cell(r, Place::Pack)))
}

fn item_label(item: &Item) -> String {
    match item.kind {
        ItemKind::Ammo(a) => format!("{}\n{}", a.def().short, item.count),
        ItemKind::Weapon(w) => {
            let load = item.loaded_with.map_or("", |a| a.def().short);
            format!("{}\n{}/{} {}", w.def().name, item.loaded, w.def().magazine, load)
        }
        _ => item.kind.name().to_string(),
    }
}

fn colour_of(item: &Item) -> egui::Color32 {
    let c = item.kind.colour();
    egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

fn paint_item(painter: &egui::Painter, rect: egui::Rect, item: &Item, faded: bool) {
    let mut fill = colour_of(item);
    if faded {
        fill = fill.gamma_multiply(0.55);
    }
    painter.rect_filled(rect.shrink(2.0), 4.0, fill);
    painter.rect_stroke(rect.shrink(2.0), 4.0, egui::Stroke::new(1.5, egui::Color32::from_gray(25)), egui::StrokeKind::Inside);
    // The label, wrapped to the item's width, in the middle of it.
    let size = if matches!(item.kind, ItemKind::Ammo(_)) { 11.0 } else { 13.0 };
    let galley = painter.layout(item_label(item), egui::FontId::proportional(size), egui::Color32::WHITE, (rect.width() - 6.0).max(10.0));
    let at = rect.center() - galley.size() * 0.5;
    painter.galley(at, galley, egui::Color32::WHITE);
}

fn paint_container(painter: &egui::Painter, rect: egui::Rect, container: &Container, held: Option<u32>) {
    painter.rect_filled(rect, 4.0, egui::Color32::from_gray(26));
    for x in 0..container.width {
        for y in 0..container.height {
            let cell = egui::Rect::from_min_size(rect.min + egui::vec2(x as f32, y as f32) * CELL, egui::vec2(CELL, CELL));
            painter.rect_stroke(cell, 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(52)), egui::StrokeKind::Inside);
        }
    }
    for p in &container.items {
        let (w, h) = p.extent();
        let r = egui::Rect::from_min_size(rect.min + egui::vec2(p.x as f32, p.y as f32) * CELL, egui::vec2(w as f32, h as f32) * CELL);
        paint_item(painter, r, &p.item, held == Some(p.item.id));
    }
}

/// The squares an item picked up by `grab` covers, if its top-left is where the pointer's square less the grab is.
fn drop_origin(cell: (u32, u32), grab: (u32, u32)) -> Option<(u32, u32)> {
    Some((cell.0.checked_sub(grab.0)?, cell.1.checked_sub(grab.1)?))
}

pub fn draw_inventory(mut contexts: EguiContexts, mut screen: ResMut<InventoryScreen>, mut inventory: ResMut<Inventory>) -> Result {
    if !screen.open {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let pack = inventory.pack.as_ref().map(|c| (c.width, c.height));
    let (pointer, pressed, released, rotate, secondary) = ctx.input(|i| {
        (i.pointer.latest_pos(), i.pointer.primary_pressed(), i.pointer.primary_released(), i.key_pressed(egui::Key::R), i.pointer.secondary_pressed())
    });
    let mut drop_message: Option<String> = None;
    let mut closed = false;
    egui::Window::new("Inventory").anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0)).collapsible(false).resizable(false).show(ctx, |ui| {
        let probe = Layout { origin: egui::Pos2::ZERO, pack };
        let (area, _) = ui.allocate_exact_size(probe.size(), egui::Sense::hover());
        let layout = Layout { origin: area.min, pack };
        let painter = ui.painter_at(area.expand(2.0));
        let held = screen.drag.map(|d| d.id);

        // The weapon slots and the back.
        for i in 0..2 {
            let r = layout.weapon_slot(i);
            painter.rect_filled(r, 4.0, egui::Color32::from_gray(26));
            let active = inventory.active == i;
            painter.rect_stroke(r, 4.0, egui::Stroke::new(if active { 2.5 } else { 1.0 }, if active { egui::Color32::from_rgb(230, 190, 70) } else { egui::Color32::from_gray(70) }), egui::StrokeKind::Inside);
            painter.text(r.left_top() - egui::vec2(0.0, 16.0), egui::Align2::LEFT_TOP, format!("Weapon {}", i + 1), egui::FontId::proportional(14.0), egui::Color32::LIGHT_GRAY);
            if let Some(item) = inventory.slots[i] {
                let (w, h) = item.size();
                let size = egui::vec2(w as f32, h as f32) * CELL;
                paint_item(&painter, egui::Rect::from_center_size(r.center(), size), &item, held == Some(item.id));
            }
        }
        let back = layout.back();
        painter.rect_filled(back, 4.0, egui::Color32::from_gray(26));
        painter.rect_stroke(back, 4.0, egui::Stroke::new(1.0, egui::Color32::from_gray(70)), egui::StrokeKind::Inside);
        painter.text(back.left_top() - egui::vec2(0.0, 16.0), egui::Align2::LEFT_TOP, "Back", egui::FontId::proportional(14.0), egui::Color32::LIGHT_GRAY);
        if let Some(item) = inventory.back {
            let (w, h) = item.size();
            let size = egui::vec2(w as f32, h as f32) * CELL;
            paint_item(&painter, egui::Rect::from_center_size(back.center(), size), &item, held == Some(item.id));
        }

        // The pockets and the pack.
        painter.text(layout.pockets().left_top() - egui::vec2(0.0, 18.0), egui::Align2::LEFT_TOP, "Pockets", egui::FontId::proportional(14.0), egui::Color32::LIGHT_GRAY);
        paint_container(&painter, layout.pockets(), &inventory.pockets, held);
        if let (Some(rect), Some(container)) = (layout.pack(), inventory.pack.as_ref()) {
            painter.text(rect.left_top() - egui::vec2(0.0, 18.0), egui::Align2::LEFT_TOP, inventory.back.map_or("Pack", |b| b.kind.name()), egui::FontId::proportional(14.0), egui::Color32::LIGHT_GRAY);
            paint_container(&painter, rect, container, held);
        }
        let (used, total) = inventory.space();
        painter.text(area.right_bottom(), egui::Align2::RIGHT_BOTTOM, format!("{used} / {total} squares"), egui::FontId::proportional(13.0), egui::Color32::GRAY);

        let over = pointer.and_then(|p| hover_at(&layout, p));

        // What's under the pointer, said.
        if screen.drag.is_none() {
            let under = match over {
                Some(Hover::Slot(i)) => inventory.slots[i],
                Some(Hover::Back) => inventory.back,
                Some(Hover::Cell(place, x, y)) => inventory.container(place).and_then(|c| c.at(x, y)).map(|p| p.item),
                None => None,
            };
            if let (Some(item), Some(p)) = (under, pointer) {
                let text = match item.kind {
                    ItemKind::Ammo(a) => format!("{}\n{} rounds", a.def().name, item.count),
                    ItemKind::Weapon(w) => format!("{}\n{}\nloaded: {} / {} {}", w.def().name, w.def().caliber.name(), item.loaded, w.def().magazine, item.loaded_with.map_or("", |a| a.def().name)),
                    _ => item.kind.name().to_string(),
                };
                painter.text(p + egui::vec2(14.0, 14.0), egui::Align2::LEFT_TOP, text, egui::FontId::proportional(14.0), egui::Color32::WHITE);
            }
        }

        // Picking up.
        if pressed && screen.drag.is_none() {
            match over {
                Some(Hover::Slot(i)) => {
                    if let Some(item) = inventory.slots[i] {
                        screen.drag = Some(Drag { id: item.id, turned: false, grab: (0, 0) });
                    }
                }
                Some(Hover::Back) => {
                    if let Some(item) = inventory.back {
                        screen.drag = Some(Drag { id: item.id, turned: false, grab: (0, 0) });
                    }
                }
                Some(Hover::Cell(place, x, y)) => {
                    if let Some(p) = inventory.container(place).and_then(|c| c.at(x, y)) {
                        screen.drag = Some(Drag { id: p.item.id, turned: p.turned, grab: (x - p.x, y - p.y) });
                    }
                }
                None => {}
            }
        }
        // Right click on a weapon takes it up in the first free slot; on a pack, puts it on.
        if secondary && screen.drag.is_none() {
            if let Some(Hover::Cell(place, x, y)) = over {
                if let Some(item) = inventory.container(place).and_then(|c| c.at(x, y)).map(|p| p.item) {
                    let done = match item.kind {
                        ItemKind::Weapon(_) => {
                            let slot = inventory.slots.iter().position(Option::is_none).unwrap_or(inventory.active);
                            inventory.equip_weapon(item.id, slot)
                        }
                        ItemKind::Pack(_) => inventory.wear_pack(item.id),
                        _ => false,
                    };
                    drop_message = Some(if done { String::new() } else { "That can't be done.".to_string() });
                }
            }
        }
        if rotate {
            if let Some(d) = &mut screen.drag {
                d.turned = !d.turned;
            }
        }

        // Putting down.
        if let Some(drag) = screen.drag {
            if let Some((_, item)) = inventory.find(drag.id) {
                let (w, h) = item.size();
                let (w, h) = if drag.turned { (h, w) } else { (w, h) };
                if let (Some(p), true) = (pointer, true) {
                    // The held thing follows the pointer, and shows green where it would go and red where it would not.
                    let ghost_origin = egui::Rect::from_min_size(p - egui::vec2(drag.grab.0 as f32 + 0.5, drag.grab.1 as f32 + 0.5) * CELL, egui::vec2(w as f32, h as f32) * CELL);
                    if let Some(Hover::Cell(place, x, y)) = over {
                        if let (Some(c), Some((ox, oy))) = (inventory.container(place), drop_origin((x, y), drag.grab)) {
                            let except = inventory.find(drag.id).filter(|(from, _)| *from == place).map(|_| drag.id);
                            let ok = c.fits((w, h), ox, oy, except);
                            if let Some(origin) = inventory.container(place).map(|_| match place {
                                Place::Pockets => layout.pockets().min,
                                _ => layout.pack().map_or(layout.pockets().min, |r| r.min),
                            }) {
                                let snapped = egui::Rect::from_min_size(origin + egui::vec2(ox as f32, oy as f32) * CELL, egui::vec2(w as f32, h as f32) * CELL);
                                painter.rect_filled(snapped, 4.0, if ok { egui::Color32::from_rgba_unmultiplied(60, 200, 80, 90) } else { egui::Color32::from_rgba_unmultiplied(220, 60, 60, 90) });
                            }
                        }
                    }
                    let ghost = Item { ..item };
                    painter.rect_filled(ghost_origin.shrink(2.0), 4.0, colour_of(&ghost).gamma_multiply(0.8));
                    painter.text(ghost_origin.center(), egui::Align2::CENTER_CENTER, item_label(&ghost), egui::FontId::proportional(13.0), egui::Color32::WHITE);
                }
                if released {
                    let mut done = false;
                    match over {
                        Some(Hover::Cell(place, x, y)) => {
                            if let Some((ox, oy)) = drop_origin((x, y), drag.grab) {
                                // Rounds dropped on rounds of the same kind join them.
                                let stack = inventory.container(place).and_then(|c| c.at(x, y)).map(|p| p.item).filter(|t| t.id != drag.id && t.kind == item.kind && matches!(item.kind, ItemKind::Ammo(_)));
                                if let Some(target) = stack {
                                    let room = item.kind.stack_limit() - target.count.min(item.kind.stack_limit());
                                    let moved = room.min(item.count);
                                    if moved > 0 {
                                        merge_rounds(&mut inventory, item.id, target.id, moved);
                                        done = true;
                                    }
                                } else {
                                    done = inventory.move_to_grid(drag.id, place, ox, oy, drag.turned);
                                }
                            }
                        }
                        Some(Hover::Slot(i)) => done = inventory.equip_weapon(drag.id, i),
                        Some(Hover::Back) => done = inventory.wear_pack(drag.id),
                        None => {}
                    }
                    drop_message = Some(if done || over.is_none() { String::new() } else { "It doesn't fit there.".to_string() });
                    screen.drag = None;
                }
            } else {
                screen.drag = None;
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Drag to move. R turns what you're holding. Right click equips a weapon or a pack.").small().weak());
            if !screen.message.is_empty() {
                ui.label(egui::RichText::new(&screen.message).color(egui::Color32::from_rgb(230, 160, 90)));
            }
        });
        if ui.button("Close").clicked() {
            closed = true;
        }
    });
    if let Some(m) = drop_message {
        screen.message = m;
    }
    if closed {
        screen.toggle();
    }
    Ok(())
}

/// Moves `count` rounds from one stack to another of the same kind (taking the first away if it empties).
fn merge_rounds(inventory: &mut Inventory, from: u32, to: u32, count: u32) {
    let Some((_, source)) = inventory.find(from) else { return };
    let remaining = source.count - count;
    for place in [Place::Pockets, Place::Pack] {
        let container = match place {
            Place::Pockets => Some(&mut inventory.pockets),
            _ => inventory.pack.as_mut(),
        };
        if let Some(c) = container {
            if let Some(p) = c.items.iter_mut().find(|p| p.item.id == to) {
                p.item.count += count;
            }
        }
    }
    if remaining == 0 {
        inventory.take(from);
    } else {
        for place in [Place::Pockets, Place::Pack] {
            let container = match place {
                Place::Pockets => Some(&mut inventory.pockets),
                _ => inventory.pack.as_mut(),
            };
            if let Some(c) = container {
                if let Some(p) = c.items.iter_mut().find(|p| p.item.id == from) {
                    p.item.count = remaining;
                }
            }
        }
    }
}
