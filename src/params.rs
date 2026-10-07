use bevy::prelude::*;

/// Tunable generation knobs, exposed as sliders in the map viewer. Defaults reproduce the
/// values the generator used before these were made adjustable.
#[derive(Resource, Clone, Copy, Debug)]
pub struct GenParams {
    /// Multiplies the terrain's broad, rolling and small-scale relief.
    pub relief_scale: f32,
    /// Number of hydraulic erosion droplets simulated.
    pub erosion_droplets: u32,
    /// How much a climb costs the river's route, relative to distance.
    pub river_slope_weight: f32,
    /// Spacing between field seeds: larger means fewer, bigger fields.
    pub field_spacing: f32,
    /// How strongly a field's growth avoids crossing slopes (contour-following strength).
    pub field_contour_weight: f32,
    /// Multiplies both road kinds' slope cost: higher means roads avoid climbing more.
    pub road_slope_scale: f32,
    /// Multiplies both road kinds' water-crossing cost: higher means fewer, narrower bridges.
    pub road_water_scale: f32,
    /// Hard slope-change limit a single field may grow across in one step: steeper than
    /// this acts as a barrier, like a road or water, forcing a boundary there regardless of
    /// cost. Left near the top of its slider range this stays unconstrained beyond what the
    /// zone slope limits below already prevent.
    pub field_max_slope: f32,
    /// Steepest ground farmland can form on when it's ploughed (the Arable zone).
    pub max_arable_slope: f32,
    /// Steepest ground farmland can form on when it's grazed (the Pasture zone).
    pub max_pasture_slope: f32,
}

impl Default for GenParams {
    fn default() -> Self {
        Self {
            relief_scale: 1.0,
            erosion_droplets: 300_000,
            river_slope_weight: 6.0,
            field_spacing: 95.0,
            field_contour_weight: 6.0,
            road_slope_scale: 1.0,
            road_water_scale: 1.0,
            field_max_slope: 2.0,
            max_arable_slope: 0.5,
            max_pasture_slope: 0.55,
        }
    }
}
