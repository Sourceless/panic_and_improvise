//! External ballistics: how a bullet flies once it has left the barrel.
//!
//! A bullet is pulled down by gravity and slowed by the air. The drag is the standard "G1"
//! model, scaled by the bullet's ballistic coefficient: `a = (pi/8) * rho * v^2 * Cd(M) / BC`,
//! where `Cd(M)` is the G1 drag coefficient at the bullet's Mach number and `BC` the coefficient
//! in kg/m^2. The drag works on the bullet's velocity *relative to the air*, so a crosswind
//! pushes it sideways, and a bullet that sheds speed quickly (a low BC) is pushed further.
//!
//! The bullet's weight enters through its ballistic coefficient: `BC = mass / (diameter^2 *
//! form factor)`, so of two bullets of the same shape and calibre the heavier has the higher BC,
//! holds its speed better, and drifts less.

use bevy::prelude::*;

/// Standard gravity, m/s^2.
pub const GRAVITY: f32 = 9.80665;
/// Air density at sea level, 15 C, kg/m^3.
pub const AIR_DENSITY: f32 = 1.225;
/// Speed of sound at 15 C, m/s.
pub const SPEED_OF_SOUND: f32 = 340.3;
/// 1 lb/in^2 in kg/m^2: the unit ballistic coefficients are quoted in.
const BC_UNIT: f32 = 703.069_6;

/// What is fired: the bullet's weight and shape, and how fast the gun sends it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cartridge {
    /// Bullet mass, kg.
    pub mass: f32,
    /// Bullet diameter, m.
    pub diameter: f32,
    /// G1 ballistic coefficient, lb/in^2 (the figure on the box).
    pub bc: f32,
    /// Speed at the muzzle, m/s.
    pub muzzle_velocity: f32,
}

/// The Sterling's ammunition: 9x19 mm Parabellum, a 124 grain (8.04 g) round-nose full metal jacket
/// bullet of .355" (9.02 mm). The ballistic coefficient is the published G1 value for that bullet
/// (Hornady 124 gr FMJ-RN: 0.145). The muzzle velocity is the L2A3's, from its 196 mm barrel:
/// about 368 m/s.
pub const NINE_PARA: Cartridge = Cartridge { mass: 0.008_035, diameter: 0.009_02, bc: 0.145, muzzle_velocity: 368.0 };

impl Cartridge {
    /// Mass over diameter squared, in lb/in^2: how much weight the bullet has behind its frontal area.
    pub fn sectional_density(&self) -> f32 {
        self.mass / (self.diameter * self.diameter) / BC_UNIT
    }

    /// How draggy the bullet's shape is compared with the G1 standard (lower is sleeker).
    pub fn form_factor(&self) -> f32 {
        self.sectional_density() / self.bc
    }

    /// The same shape and calibre at a different weight. The ballistic coefficient follows the
    /// weight, since the shape (and so the form factor) is unchanged.
    pub fn with_mass(self, mass: f32) -> Cartridge {
        let form_factor = self.form_factor();
        let heavier = Cartridge { mass, ..self };
        Cartridge { bc: heavier.sectional_density() / form_factor, ..heavier }
    }

    /// Kinetic energy at `speed`, joules.
    pub fn energy(&self, speed: f32) -> f32 {
        0.5 * self.mass * speed * speed
    }
}

/// The standard G1 drag coefficient at a Mach number (the Ingalls/Mayevski table).
pub fn g1_drag(mach: f32) -> f32 {
    const TABLE: [(f32, f32); 79] = [
        (0.00, 0.2629), (0.05, 0.2558), (0.10, 0.2487), (0.15, 0.2413), (0.20, 0.2344), (0.25, 0.2278),
        (0.30, 0.2214), (0.35, 0.2155), (0.40, 0.2104), (0.45, 0.2061), (0.50, 0.2032), (0.55, 0.2020),
        (0.60, 0.2034), (0.70, 0.2165), (0.725, 0.2230), (0.75, 0.2313), (0.775, 0.2417), (0.80, 0.2546),
        (0.825, 0.2706), (0.85, 0.2901), (0.875, 0.3136), (0.90, 0.3415), (0.925, 0.3734), (0.95, 0.4084),
        (0.975, 0.4448), (1.00, 0.4805), (1.025, 0.5136), (1.05, 0.5427), (1.075, 0.5677), (1.10, 0.5883),
        (1.125, 0.6053), (1.15, 0.6191), (1.20, 0.6393), (1.25, 0.6518), (1.30, 0.6589), (1.35, 0.6621),
        (1.40, 0.6625), (1.45, 0.6607), (1.50, 0.6573), (1.55, 0.6528), (1.60, 0.6474), (1.65, 0.6413),
        (1.70, 0.6347), (1.75, 0.6280), (1.80, 0.6210), (1.85, 0.6141), (1.90, 0.6072), (1.95, 0.6003),
        (2.00, 0.5934), (2.05, 0.5867), (2.10, 0.5804), (2.15, 0.5743), (2.20, 0.5685), (2.25, 0.5630),
        (2.30, 0.5577), (2.35, 0.5527), (2.40, 0.5481), (2.45, 0.5438), (2.50, 0.5397), (2.60, 0.5325),
        (2.70, 0.5264), (2.80, 0.5211), (2.90, 0.5168), (3.00, 0.5133), (3.10, 0.5096), (3.20, 0.5061),
        (3.30, 0.5030), (3.40, 0.5001), (3.50, 0.4974), (3.60, 0.4950), (3.70, 0.4928), (3.80, 0.4908),
        (3.90, 0.4890), (4.00, 0.4874), (4.20, 0.4846), (4.40, 0.4823), (4.60, 0.4806), (4.80, 0.4792),
        (5.00, 0.4782),
    ];
    let mach = mach.max(0.0);
    let i = TABLE.partition_point(|&(m, _)| m <= mach);
    if i == 0 {
        return TABLE[0].1;
    }
    if i == TABLE.len() {
        return TABLE[TABLE.len() - 1].1;
    }
    let ((m0, c0), (m1, c1)) = (TABLE[i - 1], TABLE[i]);
    c0 + (c1 - c0) * (mach - m0) / (m1 - m0)
}

/// The acceleration of a bullet moving at `velocity` through air that is itself moving at `wind`
/// (both in world space): gravity, plus drag against the velocity relative to the air.
pub fn acceleration(cartridge: &Cartridge, velocity: Vec3, wind: Vec3) -> Vec3 {
    let relative = velocity - wind;
    let speed = relative.length();
    let drag = if speed > 1e-3 {
        let k = std::f32::consts::FRAC_PI_8 * AIR_DENSITY * g1_drag(speed / SPEED_OF_SOUND) / (cartridge.bc * BC_UNIT);
        -relative * (k * speed)
    } else {
        Vec3::ZERO
    };
    drag + Vec3::NEG_Y * GRAVITY
}

/// A bullet in flight: where it is and how it is moving.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Flight {
    pub position: Vec3,
    pub velocity: Vec3,
}

/// Advances a bullet by `dt` seconds (fourth-order Runge-Kutta; keep `dt` to a few milliseconds).
pub fn step(cartridge: &Cartridge, flight: Flight, wind: Vec3, dt: f32) -> Flight {
    let a = |v: Vec3| acceleration(cartridge, v, wind);
    let (v1, a1) = (flight.velocity, a(flight.velocity));
    let (v2, a2) = (flight.velocity + a1 * (dt * 0.5), a(flight.velocity + a1 * (dt * 0.5)));
    let (v3, a3) = (flight.velocity + a2 * (dt * 0.5), a(flight.velocity + a2 * (dt * 0.5)));
    let (v4, a4) = (flight.velocity + a3 * dt, a(flight.velocity + a3 * dt));
    Flight {
        position: flight.position + (v1 + (v2 + v3) * 2.0 + v4) * (dt / 6.0),
        velocity: flight.velocity + (a1 + (a2 + a3) * 2.0 + a4) * (dt / 6.0),
    }
}

/// Where a bullet fired from `muzzle` along `direction` is, in still air, when it has gone
/// `range` metres along `line` (a unit vector, normally the line of sight to the zero point, which
/// is not quite the bullet's own heading). Interpolated between steps, so it is exact to a
/// fraction of a millimetre rather than to a step's length.
pub fn point_at_range(cartridge: &Cartridge, muzzle: Vec3, direction: Vec3, line: Vec3, range: f32) -> Flight {
    const DT: f32 = 0.001;
    let mut flight = Flight { position: muzzle, velocity: direction * cartridge.muzzle_velocity };
    let mut time = 0.0;
    loop {
        let before = flight;
        flight = step(cartridge, flight, Vec3::ZERO, DT);
        time += DT;
        let (gone_before, gone_now) = ((before.position - muzzle).dot(line), (flight.position - muzzle).dot(line));
        if gone_now >= range || time > 5.0 {
            let t = ((range - gone_before) / (gone_now - gone_before).max(1e-6)).clamp(0.0, 1.0);
            return Flight {
                position: before.position.lerp(flight.position, t),
                velocity: before.velocity.lerp(flight.velocity, t),
            };
        }
    }
}

/// The direction to send a bullet from `muzzle` so that, in still air, it crosses the line of sight
/// (the ray from `eye` along `view_forward`) `zero_distance` metres out. That means pointing the
/// barrel at the zero point and then lifting it by the angle the bullet will drop through on
/// the way there: the sights are "zeroed" for that range. The bullet starts off the line of sight,
/// since the barrel is below and beside it, and crosses it on its way to the zero.
pub fn zeroed_direction(cartridge: &Cartridge, muzzle: Vec3, eye: Vec3, view_forward: Vec3, zero_distance: f32) -> Vec3 {
    let target = eye + view_forward * zero_distance;
    let to_target = target - muzzle;
    let length = to_target.length();
    if length < 0.5 {
        return view_forward;
    }
    let line = to_target / length;
    // Lift the barrel in the vertical plane through the line of sight.
    let up = (Vec3::Y - line * line.dot(Vec3::Y)).try_normalize().unwrap_or(Vec3::Z);

    // How far above the line of sight the bullet is when it gets to the zero range.
    let height_error = |lift: f32| {
        let direction = line * lift.cos() + up * lift.sin();
        (point_at_range(cartridge, muzzle, direction, line, length).position - muzzle).dot(up)
    };
    // Bisection on the lift angle between "none" (which drops short) and a generous upper bound.
    let (mut low, mut high) = (0.0f32, 0.05f32);
    for _ in 0..30 {
        let mid = 0.5 * (low + high);
        if height_error(mid) < 0.0 {
            low = mid;
        } else {
            high = mid;
        }
    }
    let lift = 0.5 * (low + high);
    line * lift.cos() + up * lift.sin()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Flies a bullet fired level from 1.5 m up until it has gone `range` metres, in `wind`.
    fn fly(cartridge: &Cartridge, direction: Vec3, wind: Vec3, range: f32) -> (Flight, f32) {
        let mut flight = Flight { position: Vec3::new(0.0, 1.5, 0.0), velocity: direction * cartridge.muzzle_velocity };
        let mut time = 0.0;
        while flight.position.z.abs() < range && time < 10.0 {
            flight = step(cartridge, flight, wind, 0.001);
            time += 0.001;
        }
        (flight, time)
    }

    const FORWARD: Vec3 = Vec3::NEG_Z;

    #[test]
    fn g1_drag_follows_the_standard_table() {
        assert!((g1_drag(0.0) - 0.2629).abs() < 1e-4);
        assert!((g1_drag(1.0) - 0.4805).abs() < 1e-4);
        assert!((g1_drag(2.0) - 0.5934).abs() < 1e-4);
        // Between table entries it interpolates, and beyond them it holds the end values.
        assert!(g1_drag(0.9125) > g1_drag(0.90) && g1_drag(0.9125) < g1_drag(0.925));
        assert_eq!(g1_drag(9.0), g1_drag(5.0));
        // Drag climbs steeply through the sound barrier, which 9 mm bullets start just above.
        assert!(g1_drag(1.1) > 1.5 * g1_drag(0.7));
    }

    #[test]
    fn the_sterling_load_has_real_world_numbers() {
        let c = NINE_PARA;
        assert!((c.mass * 15432.36 - 124.0).abs() < 0.5, "124 grains");
        assert!((c.sectional_density() - 0.141).abs() < 0.005, "published sectional density .141");
        assert!((0.8..1.2).contains(&c.form_factor()), "form factor {}", c.form_factor());
        // About 540 J at the muzzle, as for any 9 mm carbine-length load.
        assert!((480.0..600.0).contains(&c.energy(c.muzzle_velocity)), "{}", c.energy(c.muzzle_velocity));
    }

    #[test]
    fn a_bullet_slows_down_and_falls() {
        let (at_100, time) = fly(&NINE_PARA, FORWARD, Vec3::ZERO, 100.0);
        let speed = at_100.velocity.length();
        assert!((290.0..340.0).contains(&speed), "speed at 100 m: {speed}");
        // 100 m takes about 0.3 s: a little longer than at the muzzle velocity.
        assert!((0.27..0.34).contains(&time), "time to 100 m: {time}");
        let drop = 1.5 - at_100.position.y;
        assert!((0.35..0.6).contains(&drop), "drop at 100 m fired level: {drop}");
    }

    #[test]
    fn drag_slows_a_bullet_more_the_lower_its_ballistic_coefficient() {
        let slick = Cartridge { bc: 0.30, ..NINE_PARA };
        let draggy = Cartridge { bc: 0.10, ..NINE_PARA };
        let speed = |c: &Cartridge| fly(c, FORWARD, Vec3::ZERO, 100.0).0.velocity.length();
        assert!(speed(&slick) > speed(&NINE_PARA) && speed(&NINE_PARA) > speed(&draggy));
    }

    #[test]
    fn with_no_drag_it_would_be_a_plain_parabola() {
        // A bullet with an enormous BC barely feels the air: its drop is ½ g t².
        let ideal = Cartridge { bc: 1.0e6, ..NINE_PARA };
        let (flight, time) = fly(&ideal, FORWARD, Vec3::ZERO, 100.0);
        let drop = 1.5 - flight.position.y;
        assert!((drop - 0.5 * GRAVITY * time * time).abs() < 0.005, "{drop}");
        assert!((time - 100.0 / ideal.muzzle_velocity).abs() < 0.002);
    }

    #[test]
    fn a_crosswind_pushes_the_bullet_sideways() {
        let wind = Vec3::new(5.0, 0.0, 0.0);
        let (flight, _) = fly(&NINE_PARA, FORWARD, wind, 100.0);
        // A 5 m/s breeze moves a pistol bullet by well over ten centimetres at 100 m.
        assert!((0.1..0.35).contains(&flight.position.x), "drift {}", flight.position.x);
        // ...and in the still it goes dead straight.
        let (still, _) = fly(&NINE_PARA, FORWARD, Vec3::ZERO, 100.0);
        assert!(still.position.x.abs() < 1e-3);
    }

    #[test]
    fn wind_drift_depends_on_ballistic_coefficient() {
        let wind = Vec3::new(5.0, 0.0, 0.0);
        let drift = |c: &Cartridge| fly(c, FORWARD, wind, 100.0).0.position.x;
        let slick = Cartridge { bc: 0.30, ..NINE_PARA };
        let draggy = Cartridge { bc: 0.10, ..NINE_PARA };
        assert!(drift(&slick) < drift(&NINE_PARA) && drift(&NINE_PARA) < drift(&draggy));
    }

    #[test]
    fn a_heavier_bullet_of_the_same_shape_drifts_less() {
        let wind = Vec3::new(5.0, 0.0, 0.0);
        let heavy = NINE_PARA.with_mass(0.0105); // 162 grains
        let light = NINE_PARA.with_mass(0.0065); // 100 grains
        assert!((heavy.form_factor() - NINE_PARA.form_factor()).abs() < 1e-3, "same shape");
        assert!(heavy.bc > NINE_PARA.bc && NINE_PARA.bc > light.bc);
        let drift = |c: &Cartridge| fly(c, FORWARD, wind, 100.0).0.position.x;
        assert!(drift(&heavy) < drift(&NINE_PARA) && drift(&NINE_PARA) < drift(&light));
    }

    #[test]
    fn a_headwind_slows_it_and_a_tailwind_speeds_it() {
        let speed = |wind| fly(&NINE_PARA, FORWARD, wind, 100.0).0.velocity.length();
        assert!(speed(Vec3::new(0.0, 0.0, 8.0)) < speed(Vec3::ZERO), "headwind (blowing back at it)");
        assert!(speed(Vec3::new(0.0, 0.0, -8.0)) > speed(Vec3::ZERO), "tailwind");
    }

    #[test]
    fn drift_is_to_leeward_whichever_way_the_wind_blows() {
        let drift = |wind| fly(&NINE_PARA, FORWARD, wind, 100.0).0.position.x;
        assert!(drift(Vec3::new(5.0, 0.0, 0.0)) > 0.0);
        assert!(drift(Vec3::new(-5.0, 0.0, 0.0)) < 0.0);
        let (a, b) = (drift(Vec3::new(5.0, 0.0, 0.0)), drift(Vec3::new(-5.0, 0.0, 0.0)));
        assert!((a + b).abs() < 1e-3, "symmetric");
    }

    #[test]
    fn the_sights_are_zeroed_at_the_chosen_range() {
        // The muzzle is down and to the right of the eye, as on the gun.
        let eye = Vec3::new(0.0, 1.8, 0.0);
        let muzzle = eye + Vec3::new(0.15, -0.12, -0.5);
        let zero = 20.0;
        let direction = zeroed_direction(&NINE_PARA, muzzle, eye, FORWARD, zero);
        let target = eye + FORWARD * zero;
        let line = (target - muzzle).normalize();
        let flight = point_at_range(&NINE_PARA, muzzle, direction, line, (target - muzzle).length());
        // At the zero range it is on the line of sight, to a few millimetres.
        assert!((flight.position - target).length() < 0.005, "off by {:?}", flight.position - target);
    }

    #[test]
    fn the_zero_works_whichever_way_you_look() {
        let eye = Vec3::new(10.0, 50.0, -30.0);
        for pitch in [-0.5f32, 0.0, 0.4, 1.0] {
            for yaw in [0.0f32, 1.0, 2.5, -2.0] {
                let forward = (Quat::from_rotation_y(yaw) * Quat::from_rotation_x(pitch)) * FORWARD;
                let right = forward.cross(Vec3::Y).normalize();
                let muzzle = eye + right * 0.15 + Vec3::NEG_Y * 0.12 + forward * 0.5;
                let direction = zeroed_direction(&NINE_PARA, muzzle, eye, forward, 20.0);
                let target = eye + forward * 20.0;
                let line = (target - muzzle).normalize();
                let flight = point_at_range(&NINE_PARA, muzzle, direction, line, (target - muzzle).length());
                assert!(flight.position.distance(target) < 0.005, "pitch {pitch} yaw {yaw}: off by {}", flight.position.distance(target));
            }
        }
    }

    #[test]
    fn beyond_the_zero_the_bullet_falls_below_the_sights() {
        let eye = Vec3::new(0.0, 1.8, 0.0);
        let muzzle = eye + Vec3::new(0.0, -0.05, -0.5);
        let direction = zeroed_direction(&NINE_PARA, muzzle, eye, FORWARD, 20.0);
        let flight = point_at_range(&NINE_PARA, muzzle, direction, FORWARD, 100.0 + 0.5);
        let below = eye.y - flight.position.y;
        assert!((0.08..0.25).contains(&below), "at 100 m a 20 m zero is {below} m below the line of sight");
    }
}
