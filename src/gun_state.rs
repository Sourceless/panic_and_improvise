//! A gun's mechanism as a state machine.
//!
//! What a gun can do depends on where its action is: an open-bolt gun (like the Sterling) is
//! carried with its bolt held to the rear, and pulling the trigger lets the bolt run forward,
//! strip a round from the magazine and fire it; a closed-bolt gun has a round chambered and the bolt
//! forward, ready to fire the moment the trigger is pulled. When the magazine runs dry the action
//! is left in the *wrong* place for the type: an open-bolt gun's bolt is forward on an empty chamber,
//! a closed-bolt gun's slide is locked back. Putting in a new magazine then also needs the action
//! charged, which takes longer than just swapping magazines.
//!
//! The states are:
//! - `Ready`: the action is where it should be; the gun fires if there are rounds.
//! - `Cycling`: a shot is going through the action.
//! - `Dry`: out of rounds with the action in the wrong place; the trigger does nothing, and
//!   the next reload has to charge the action as well.
//! - `Reloading`: the magazine is being changed.
//!
//! Everything here is a pure function of the state, so the rules can be tested without a game.

/// The kind of action.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoltType {
    Open,
    Closed,
}

/// Where a bolt (or slide) rests.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bolt {
    Forward,
    Rear,
}

/// The mechanical details of a gun.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Mechanism {
    pub bolt_type: BoltType,
    /// Seconds for a shot to cycle the action, which sets the rate of fire.
    pub cycle_time: f32,
    /// Seconds to change a magazine in a gun that is ready.
    pub reload_time: f32,
    /// Extra seconds to charge the action when reloading a dry gun.
    pub charge_time: f32,
}

/// The Sterling: open bolt, about 500 rounds a minute. Changing a magazine takes two seconds with
/// the bolt back, and another 0.7 s to haul the bolt back on the cocking handle if it's forward.
pub const STERLING: Mechanism = Mechanism { bolt_type: BoltType::Open, cycle_time: 0.12, reload_time: 2.0, charge_time: 0.7 };

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Ready,
    /// A shot is cycling; `left` seconds until the gun can fire again.
    Cycling { left: f32 },
    Dry,
    /// A reload: `elapsed` of `total` seconds done.
    Reloading { elapsed: f32, total: f32 },
}

impl State {
    pub fn is_reloading(&self) -> bool {
        matches!(self, State::Reloading { .. })
    }
}

impl Mechanism {
    /// Where the bolt is while the gun is at rest in `state` (it's moving, or being worked, in the
    /// other states).
    pub fn bolt(&self, state: State) -> Option<Bolt> {
        let ready = if self.bolt_type == BoltType::Open { Bolt::Rear } else { Bolt::Forward };
        let wrong = if ready == Bolt::Rear { Bolt::Forward } else { Bolt::Rear };
        match state {
            State::Ready => Some(ready),
            State::Dry => Some(wrong),
            State::Cycling { .. } | State::Reloading { .. } => None,
        }
    }

    /// The trigger is pulled. Returns the new state and whether a round was fired.
    pub fn pull_trigger(&self, state: State, ammo: u32) -> (State, bool) {
        match state {
            State::Ready if ammo > 0 => (State::Cycling { left: self.cycle_time }, true),
            // An open-bolt gun with an empty magazine: the bolt is released and runs forward on
            // nothing. A closed-bolt gun never gets here (it goes dry on its last round).
            State::Ready if self.bolt_type == BoltType::Open => (State::Dry, false),
            other => (other, false),
        }
    }

    /// The reload key is pressed. A magazine that is full needs no reload, unless the action does.
    pub fn press_reload(&self, state: State, ammo: u32, capacity: u32) -> State {
        let charge = match state {
            State::Ready if ammo < capacity => 0.0,
            State::Dry => self.charge_time,
            _ => return state,
        };
        State::Reloading { elapsed: 0.0, total: self.reload_time + charge }
    }

    /// Time passes. Returns the new state, and whether a reload has just finished (so that the
    /// caller can fill the magazine). `trigger_held` matters only as a shot finishes cycling.
    pub fn tick(&self, state: State, ammo: u32, trigger_held: bool, dt: f32) -> (State, bool) {
        match state {
            State::Cycling { left } => {
                let left = left - dt;
                if left > 0.0 {
                    return (State::Cycling { left }, false);
                }
                let after = if ammo > 0 {
                    State::Ready
                } else {
                    match self.bolt_type {
                        // The bolt goes forward on the empty chamber if the trigger is still held to
                        // let it; if it has been let go the sear catches the bolt at the rear.
                        BoltType::Open if trigger_held => State::Dry,
                        BoltType::Open => State::Ready,
                        // The slide locks back on the last round.
                        BoltType::Closed => State::Dry,
                    }
                };
                (after, false)
            }
            State::Reloading { elapsed, total } => {
                let elapsed = elapsed + dt;
                if elapsed >= total {
                    (State::Ready, true)
                } else {
                    (State::Reloading { elapsed, total }, false)
                }
            }
            other => (other, false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOSED: Mechanism = Mechanism { bolt_type: BoltType::Closed, ..STERLING };

    /// Runs time forward in small steps.
    fn run(m: &Mechanism, mut state: State, ammo: u32, held: bool, seconds: f32) -> (State, bool) {
        let mut finished = false;
        for _ in 0..(seconds / 0.005) as usize {
            let (s, done) = m.tick(state, ammo, held, 0.005);
            state = s;
            finished |= done;
        }
        (state, finished)
    }

    #[test]
    fn a_ready_gun_with_rounds_fires_and_cycles() {
        let (state, fired) = STERLING.pull_trigger(State::Ready, 5);
        assert!(fired);
        assert!(matches!(state, State::Cycling { .. }));
        let (state, _) = run(&STERLING, state, 4, true, 0.2);
        assert_eq!(state, State::Ready, "ready again after the cycle");
    }

    #[test]
    fn it_cannot_fire_while_cycling_or_reloading_or_dry() {
        for state in [State::Cycling { left: 0.05 }, State::Dry, State::Reloading { elapsed: 0.0, total: 2.0 }] {
            let (after, fired) = STERLING.pull_trigger(state, 10);
            assert!(!fired, "{state:?}");
            assert_eq!(after, state);
        }
    }

    #[test]
    fn the_sterlings_bolt_rests_to_the_rear_when_ready() {
        assert_eq!(STERLING.bolt(State::Ready), Some(Bolt::Rear));
        assert_eq!(STERLING.bolt(State::Dry), Some(Bolt::Forward));
        assert_eq!(CLOSED.bolt(State::Ready), Some(Bolt::Forward));
        assert_eq!(CLOSED.bolt(State::Dry), Some(Bolt::Rear));
        assert_eq!(STERLING.bolt(State::Cycling { left: 0.1 }), None);
    }

    #[test]
    fn emptying_an_open_bolt_gun_with_the_trigger_held_leaves_the_bolt_forward() {
        let (state, _) = STERLING.pull_trigger(State::Ready, 1);
        let (state, _) = run(&STERLING, state, 0, true, 0.2);
        assert_eq!(state, State::Dry);
        assert_eq!(STERLING.bolt(state), Some(Bolt::Forward));
    }

    #[test]
    fn letting_go_of_the_trigger_leaves_an_open_bolt_gun_with_its_bolt_back() {
        let (state, _) = STERLING.pull_trigger(State::Ready, 1);
        let (state, _) = run(&STERLING, state, 0, false, 0.2);
        assert_eq!(state, State::Ready);
        assert_eq!(STERLING.bolt(state), Some(Bolt::Rear));
    }

    #[test]
    fn pulling_the_trigger_on_an_empty_open_bolt_gun_drops_the_bolt_forward() {
        let (state, fired) = STERLING.pull_trigger(State::Ready, 0);
        assert!(!fired);
        assert_eq!(state, State::Dry);
        assert_eq!(STERLING.pull_trigger(state, 0), (State::Dry, false), "and then nothing more happens");
    }

    #[test]
    fn a_closed_bolt_gun_locks_open_on_its_last_round_whatever_the_trigger_does() {
        for held in [true, false] {
            let (state, _) = CLOSED.pull_trigger(State::Ready, 1);
            let (state, _) = run(&CLOSED, state, 0, held, 0.2);
            assert_eq!(state, State::Dry, "held {held}");
        }
    }

    #[test]
    fn reloading_with_the_bolt_back_takes_the_basic_time() {
        let State::Reloading { total, .. } = STERLING.press_reload(State::Ready, 12, 30) else { panic!("should reload") };
        assert_eq!(total, STERLING.reload_time);
    }

    #[test]
    fn reloading_with_the_bolt_forward_takes_longer() {
        let State::Reloading { total: slow, .. } = STERLING.press_reload(State::Dry, 0, 30) else { panic!("should reload") };
        let State::Reloading { total: quick, .. } = STERLING.press_reload(State::Ready, 0, 30) else { panic!("should reload") };
        assert_eq!(slow, quick + STERLING.charge_time);
        assert!(slow > quick, "longer, but only a little");
        assert!(slow < quick * 1.5);
    }

    #[test]
    fn a_full_magazine_needs_no_reload_and_a_reload_cannot_be_interrupted() {
        assert_eq!(STERLING.press_reload(State::Ready, 30, 30), State::Ready);
        let reloading = State::Reloading { elapsed: 1.0, total: 2.0 };
        assert_eq!(STERLING.press_reload(reloading, 0, 30), reloading);
        let cycling = State::Cycling { left: 0.05 };
        assert_eq!(STERLING.press_reload(cycling, 3, 30), cycling, "not mid-shot");
    }

    #[test]
    fn a_dry_gun_is_reloaded_even_if_the_magazine_somehow_looks_full() {
        assert!(STERLING.press_reload(State::Dry, 30, 30).is_reloading());
    }

    #[test]
    fn a_reload_finishes_after_its_time_and_leaves_the_gun_ready() {
        let State::Reloading { total, .. } = STERLING.press_reload(State::Dry, 0, 30) else { panic!() };
        let (during, finished) = run(&STERLING, State::Reloading { elapsed: 0.0, total }, 0, false, total - 0.1);
        assert!(during.is_reloading() && !finished);
        let (after, finished) = run(&STERLING, during, 0, false, 0.3);
        assert_eq!(after, State::Ready);
        assert!(finished);
        assert_eq!(STERLING.bolt(after), Some(Bolt::Rear), "charged and cocked");
    }

    #[test]
    fn a_cycle_takes_the_cycle_time() {
        let (state, _) = STERLING.pull_trigger(State::Ready, 5);
        let (state, _) = run(&STERLING, state, 4, true, STERLING.cycle_time - 0.02);
        assert!(matches!(state, State::Cycling { .. }));
        let (state, _) = run(&STERLING, state, 4, true, 0.05);
        assert_eq!(state, State::Ready);
    }
}
