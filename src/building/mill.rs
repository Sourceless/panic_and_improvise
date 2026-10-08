//! The mill: a round tower of stone with a door at the water side, three floors joined by two stairs,
//! the millstone at the bottom, and sacks on every floor.

use std::f32::consts::{PI, TAU};

use bevy::prelude::*;

use super::colour;
use super::geom::{Layer, Model, Piece, Rect};
use super::plan::{self, Climb, Stair, Style};
use super::{steps_at, Site};
use crate::collision::Material;
use crate::settlement_plan::Building;

const FACETS: usize = 12;
const WALL: f32 = 0.5;
const STOREYS: usize = 3;

/// A wall of a facet: `len` long, `thick` thick, at `centre` on the ring, facing outward along `phi`,
/// between heights y0 and y1, with openings (along the facet from its middle: lo, hi, sill, head).
#[allow(clippy::too_many_arguments)]
fn facet(model: &mut Model, centre: Vec2, phi: f32, len: f32, thick: f32, y0: f32, y1: f32, openings: &[(f32, f32, f32, f32)], floor: f32) {
    let tangent = Vec2::new(-phi.sin(), phi.cos());
    let mut piece = |a: f32, b: f32, from: f32, to: f32| {
        if b - a < 0.01 || to - from < 0.01 {
            return;
        }
        let mid = centre + tangent * ((a + b) * 0.5);
        model.block_turned(Vec3::new(mid.x, (from + to) * 0.5, mid.y), Vec3::new(thick, to - from, b - a), -phi, colour::STONE, Layer::Shell, Some(Material::Stone));
    };
    let mut open: Vec<_> = openings.to_vec();
    open.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cursor = -len * 0.5;
    for &(lo, hi, sill, head) in &open {
        piece(cursor, lo, y0, y1);
        piece(lo, hi, y0, floor + sill);
        piece(lo, hi, floor + head, y1);
        cursor = hi;
    }
    piece(cursor, len * 0.5, y0, y1);
}

pub fn build(b: &Building, site: &Site) -> Model {
    let mut model = Model::default();
    let r = b.width * 0.5;
    let total = b.wall_height;
    let storey = total / STOREYS as f32;
    let apothem = r * (PI / FACETS as f32).cos();
    let facet_len = 2.0 * r * (PI / FACETS as f32).sin() * 1.06;
    let inner = apothem - WALL - 0.05;
    let style = Style::default();

    // The walls, a facet at a time, the door in the one facing the front (-z).
    for i in 0..FACETS {
        let phi = TAU * i as f32 / FACETS as f32;
        let centre = Vec2::new(phi.cos(), phi.sin()) * (apothem - WALL * 0.5);
        let facing_front = (phi.sin() + 1.0).abs() < 1e-3;
        for s in 0..STOREYS {
            let floor = s as f32 * storey;
            let bottom = if s == 0 { -site.skirt } else { floor - 0.2 };
            let mut openings: Vec<(f32, f32, f32, f32)> = Vec::new();
            if facing_front && s == 0 {
                openings.push((-0.6, 0.6, 0.0, 2.2));
            } else if i % 2 == 1 || s == STOREYS - 1 {
                // A narrow window, a tall slit.
                openings.push((-0.25, 0.25, 1.0, 2.2));
            }
            facet(&mut model, centre, phi, facet_len, WALL, bottom, floor + storey, &openings, floor);
        }
    }
    // The steps up to the door, at the front.
    steps_at(&mut model, 0.0, 1.2, -apothem, site.door_rise);

    // Floors: the ground is a slab, the others are strips across the circle with the holes for the stairs cut out.
    model.push(Piece::Cylinder { base: Vec3::new(0.0, -1.0, 0.0), radius: apothem + 0.1, height: 1.0 }, colour::STONE, Layer::Shell, Some(Material::Stone));
    let stair_run = super::kinds::stair_run(storey);
    let first = Stair { rect: Rect::new(-1.65, -inner * 0.0 - 2.35, -0.55, -2.35 + stair_run), climbs: Climb::PosZ };
    let second = Stair { rect: Rect::new(0.55, 2.35 - stair_run, 1.65, 2.35), climbs: Climb::NegZ };
    let stairs = [first, second];
    let bounds = Rect::new(-inner, -inner, inner, inner);
    for s in 1..STOREYS {
        let y = s as f32 * storey;
        let hole = stairs[s - 1].rect;
        let mut x = -inner;
        while x < inner - 0.01 {
            let x1 = (x + 0.5).min(inner);
            let mid = (x + x1) * 0.5;
            let chord = (inner * inner - mid * mid).max(0.0).sqrt();
            let strip = Rect::new(x, -chord - 0.1, x1, chord + 0.1);
            for piece in strip.minus(&hole) {
                model.slab(&piece, y, 0.2, colour::TIMBER, Layer::Shell, Some(Material::Wood));
            }
            x = x1;
        }
        plan::balustrade(&mut model, &hole, Some(stairs[s - 1].climbs), y, &style, &bounds);
    }
    for (s, st) in stairs.iter().enumerate() {
        plan::stair(&mut model, st, s as f32 * storey, storey, s == 0, &style);
    }
    // The ceiling.
    let top = total;
    let mut x = -inner;
    while x < inner - 0.01 {
        let x1 = (x + 0.5).min(inner);
        let mid = (x + x1) * 0.5;
        let chord = (inner * inner - mid * mid).max(0.0).sqrt();
        model.slab(&Rect::new(x, -chord - 0.1, x1, chord + 0.1), top, 0.2, colour::TIMBER, Layer::Shell, Some(Material::Wood));
        x = x1;
    }

    // The roof and the wheel, outside.
    model.push(Piece::Cone { base: Vec3::new(0.0, total, 0.0), radius: r + 0.8, height: 4.5 }, colour::TILE, Layer::Shell, None);
    model.block(Vec3::new(0.0, 2.5, -(r + 0.5)), Vec3::new(0.6, 6.0, 6.0), colour::TIMBER, Layer::Shell, None);

    // Inside: the millstone and its hopper, sacks against the wall on each floor, a shaft up the middle.
    let sack = |model: &mut Model, x: f32, z: f32, y: f32| model.block(Vec3::new(x, y + 0.45, z), Vec3::new(0.5, 0.9, 0.4), [0.8, 0.72, 0.5], Layer::Interior, Some(Material::Wood));
    model.push(Piece::Cylinder { base: Vec3::new(1.5, 0.0, 0.3), radius: 1.0, height: 0.45 }, [0.6, 0.6, 0.58], Layer::Interior, Some(Material::Stone));
    model.push(Piece::Cylinder { base: Vec3::new(1.5, 0.45, 0.3), radius: 0.9, height: 0.15 }, [0.5, 0.5, 0.48], Layer::Interior, None);
    for k in 0..4 {
        let a = 0.9 + k as f32 * 0.5;
        sack(&mut model, a.cos() * (inner - 0.6), a.sin() * (inner - 0.6), 0.0);
    }
    for s in 1..STOREYS {
        let y = s as f32 * storey;
        for k in 0..5 {
            let a = 3.6 + k as f32 * 0.45 + s as f32;
            sack(&mut model, a.cos() * (inner - 0.55), a.sin() * (inner - 0.55), y);
        }
        model.block(Vec3::new(0.2, y + 0.4, 0.2), Vec3::new(0.9, 0.8, 0.9), colour::TIMBER, Layer::Interior, Some(Material::Wood));
    }
    for s in 0..STOREYS {
        model.lights.push((Vec3::new(0.0, s as f32 * storey + storey - 0.5, 0.0), inner * 1.8));
    }
    model
}
