//! The pieces a building is made of, in the building's own frame (x along its width, z along its
//! depth with the front at -z, y up from the floor of the ground storey), and how they become a mesh
//! and solids.

use bevy::prelude::*;

use crate::collision::{Material, Solid};
use crate::meshbake::{MeshBuilder, Rgb};

/// Which mesh a piece is baked into: the shell is always there; the interior is only built while the
/// player is near enough to go in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Shell,
    Interior,
    /// Window glass: baked apart so that it can be see-through.
    Glass,
}

#[derive(Clone, Copy, Debug)]
pub enum Piece {
    Block { centre: Vec3, size: Vec3, yaw: f32 },
    Gable { base: Vec3, length: f32, span: f32, rise: f32, overhang: f32, yaw: f32 },
    Cylinder { base: Vec3, radius: f32, height: f32 },
    Cone { base: Vec3, radius: f32, height: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Item {
    pub piece: Piece,
    pub colour: Rgb,
    pub layer: Layer,
    /// What it is made of, if it can be bumped into and shot.
    pub solid: Option<Material>,
}

/// An axis-aligned rectangle on the floor plan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x0: f32,
    pub z0: f32,
    pub x1: f32,
    pub z1: f32,
}

impl Rect {
    pub fn new(x0: f32, z0: f32, x1: f32, z1: f32) -> Rect {
        Rect { x0: x0.min(x1), z0: z0.min(z1), x1: x0.max(x1), z1: z0.max(z1) }
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn depth(&self) -> f32 {
        self.z1 - self.z0
    }

    pub fn area(&self) -> f32 {
        self.width() * self.depth()
    }

    pub fn centre(&self) -> Vec2 {
        Vec2::new((self.x0 + self.x1) * 0.5, (self.z0 + self.z1) * 0.5)
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x0 && p.x <= self.x1 && p.y >= self.z0 && p.y <= self.z1
    }

    pub fn grown(&self, by: f32) -> Rect {
        Rect { x0: self.x0 - by, z0: self.z0 - by, x1: self.x1 + by, z1: self.z1 + by }
    }

    pub fn overlaps(&self, other: &Rect) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.z0 < other.z1 && other.z0 < self.z1
    }

    /// What is left of this rectangle when `hole` is cut out of it: up to four rectangles.
    pub fn minus(&self, hole: &Rect) -> Vec<Rect> {
        if !self.overlaps(hole) {
            return vec![*self];
        }
        let mut out = Vec::new();
        let h = Rect { x0: hole.x0.max(self.x0), z0: hole.z0.max(self.z0), x1: hole.x1.min(self.x1), z1: hole.z1.min(self.z1) };
        for r in [
            Rect { x0: self.x0, z0: self.z0, x1: self.x1, z1: h.z0 },
            Rect { x0: self.x0, z0: h.z1, x1: self.x1, z1: self.z1 },
            Rect { x0: self.x0, z0: h.z0, x1: h.x0, z1: h.z1 },
            Rect { x0: h.x1, z0: h.z0, x1: self.x1, z1: h.z1 },
        ] {
            if r.width() > 1e-3 && r.depth() > 1e-3 {
                out.push(r);
            }
        }
        out
    }
}

/// Everything a building is made of.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub items: Vec<Item>,
    /// Where lights hang inside, and how far each reaches: (position, range).
    pub lights: Vec<(Vec3, f32)>,
    /// Where loot lies, in the building's frame: the middle of the underside of each box.
    pub loot: Vec<(Vec3, crate::loot::Rarity)>,
}

impl Model {
    pub fn push(&mut self, piece: Piece, colour: Rgb, layer: Layer, solid: Option<Material>) {
        self.items.push(Item { piece, colour, layer, solid });
    }

    /// A box from its centre and size.
    pub fn block(&mut self, centre: Vec3, size: Vec3, colour: Rgb, layer: Layer, solid: Option<Material>) {
        self.push(Piece::Block { centre, size, yaw: 0.0 }, colour, layer, solid);
    }

    pub fn block_turned(&mut self, centre: Vec3, size: Vec3, yaw: f32, colour: Rgb, layer: Layer, solid: Option<Material>) {
        self.push(Piece::Block { centre, size, yaw }, colour, layer, solid);
    }

    /// A box between two corners.
    pub fn span(&mut self, lo: Vec3, hi: Vec3, colour: Rgb, layer: Layer, solid: Option<Material>) {
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        if (hi - lo).min_element() < 1e-3 {
            return;
        }
        self.block((lo + hi) * 0.5, hi - lo, colour, layer, solid);
    }

    /// A flat slab over the rectangle with its top at `top`, `thick` deep.
    pub fn slab(&mut self, r: &Rect, top: f32, thick: f32, colour: Rgb, layer: Layer, solid: Option<Material>) {
        self.span(Vec3::new(r.x0, top - thick, r.z0), Vec3::new(r.x1, top, r.z1), colour, layer, solid);
    }

    /// A slab with rectangular holes cut out of it.
    pub fn slab_with_holes(&mut self, r: &Rect, holes: &[Rect], top: f32, thick: f32, colour: Rgb, layer: Layer, solid: Option<Material>) {
        let mut pieces = vec![*r];
        for hole in holes {
            pieces = pieces.iter().flat_map(|p| p.minus(hole)).collect();
        }
        for p in pieces {
            self.slab(&p, top, thick, colour, layer, solid);
        }
    }

    /// The mesh of everything on a layer.
    pub fn bake(&self, layer: Layer) -> Mesh {
        let mut b = if layer == Layer::Glass { MeshBuilder::with_alpha(0.22) } else { MeshBuilder::default() };
        for item in self.items.iter().filter(|i| i.layer == layer) {
            match item.piece {
                Piece::Block { centre, size, yaw } => b.cuboid(centre, size, yaw, item.colour),
                Piece::Gable { base, length, span, rise, overhang, yaw } => b.gable(base, length, span, rise, overhang, yaw, item.colour),
                Piece::Cylinder { base, radius, height } => b.cylinder(base, radius, height, 16, item.colour),
                Piece::Cone { base, radius, height } => b.cone(base, radius, height, 16, item.colour),
            }
        }
        b.into_mesh()
    }

    pub fn extend(&mut self, other: Model) {
        self.items.extend(other.items);
        self.lights.extend(other.lights);
        self.loot.extend(other.loot);
    }

    pub fn has(&self, layer: Layer) -> bool {
        self.items.iter().any(|i| i.layer == layer)
    }

    /// The solids the model makes when its origin is at `origin` (the middle of the building at the
    /// level of its floor) and it is turned `yaw`.
    pub fn solids(&self, origin: Vec3, yaw: f32) -> Vec<Solid> {
        let turn = Quat::from_rotation_y(yaw);
        let mut out = Vec::new();
        for item in &self.items {
            let Some(material) = item.solid else { continue };
            match item.piece {
                Piece::Block { centre, size, yaw: own } => {
                    let at = origin + turn * centre;
                    let solid = Solid::rect(Vec2::new(at.x, at.z), Vec2::new(size.x, size.z) * 0.5, yaw + own, at.y + size.y * 0.5).of(material);
                    let bottom = centre.y - size.y * 0.5;
                    out.push(if bottom <= 0.05 { solid } else { solid.from(origin.y + bottom) });
                }
                Piece::Cylinder { base, radius, height } => {
                    let at = origin + turn * base;
                    let solid = Solid::circle(Vec2::new(at.x, at.z), radius, at.y + height).of(material);
                    out.push(if base.y <= 0.05 { solid } else { solid.from(at.y) });
                }
                Piece::Gable { .. } | Piece::Cone { .. } => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cutting_a_hole_leaves_pieces_that_tile_the_rest() {
        let r = Rect::new(0.0, 0.0, 10.0, 8.0);
        let hole = Rect::new(2.0, 3.0, 4.0, 7.0);
        let left = r.minus(&hole);
        assert_eq!(left.len(), 4);
        let area: f32 = left.iter().map(Rect::area).sum();
        assert!((area - (80.0 - 8.0)).abs() < 1e-4);
        assert!(left.iter().all(|p| !p.overlaps(&hole)));
        // A hole against an edge leaves fewer.
        assert_eq!(r.minus(&Rect::new(0.0, 0.0, 3.0, 8.0)).len(), 1);
        // A hole elsewhere leaves it be.
        assert_eq!(r.minus(&Rect::new(20.0, 0.0, 30.0, 8.0)), vec![r]);
    }

    #[test]
    fn a_block_makes_a_solid_where_and_how_it_is_drawn() {
        let mut m = Model::default();
        m.block_turned(Vec3::new(3.0, 1.0, -5.0), Vec3::new(8.0, 2.0, 6.0), 0.0, [1.0; 3], Layer::Shell, Some(Material::Stone));
        let origin = Vec3::new(100.0, 20.0, 50.0);
        let solids = m.solids(origin, 0.6);
        assert_eq!(solids.len(), 1);
        let s = solids[0];
        assert!((s.top_at(Vec2::ZERO) - 22.0).abs() < 1e-4, "reaches 2 m above the floor");
        // Its middle is where the turned building puts it.
        let at = origin + Quat::from_rotation_y(0.6) * Vec3::new(3.0, 1.0, -5.0);
        assert!(s.shape.separation(Vec2::new(at.x, at.z)).0 < -2.9);
        assert_eq!(s.bottom, f32::NEG_INFINITY, "a wall standing on the floor stands on the ground");
    }

    #[test]
    fn a_floor_above_hangs_and_can_be_walked_under() {
        let mut m = Model::default();
        m.slab(&Rect::new(-2.0, -2.0, 2.0, 2.0), 3.0, 0.2, [1.0; 3], Layer::Shell, Some(Material::Wood));
        let s = m.solids(Vec3::new(0.0, 10.0, 0.0), 0.0)[0];
        assert!((s.bottom - 12.8).abs() < 1e-4 && (s.top_at(Vec2::ZERO) - 13.0).abs() < 1e-4);
    }

    #[test]
    fn layers_bake_separately() {
        let mut m = Model::default();
        m.block(Vec3::ZERO, Vec3::ONE, [1.0; 3], Layer::Shell, None);
        assert!(m.has(Layer::Shell) && !m.has(Layer::Interior));
    }
}
