//! A reload as a run of steps, and the game of timing it.
//!
//! A magazine gun's reload is one step: the magazine is changed. A shotgun's is a step to open the
//! action and then a step for each shell; a rifle that takes clips has a step for each clip and then one
//! for each loose round. Each step that puts rounds in has a sweet spot. Pressing reload in it is a
//! *perfect* press: that step goes by four times as fast. Pressing reload anywhere else, with more rounds
//! to load after this step, *stops* the reload once this step is done, so a reload can be cut short and
//! keep what it has loaded. A reload with a press in every step it has is a perfect reload.
//!
//! All of it is a plain value stepped by time, so it can be tested without a game.

/// How much faster a step goes by after a perfect press.
pub const PERFECT_SPEED: f32 = 4.0;
/// Where in a step, as fractions of its time, the sweet spot is.
pub const SWEET_SPOT: (f32, f32) = (0.3, 0.7);

/// One part of a reload.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Step {
    /// How many seconds it takes.
    pub time: f32,
    /// How many rounds it puts in the gun when it is done.
    pub rounds: u32,
    /// Whether it has a sweet spot: it is the part of the reload that is the loading.
    pub timed: bool,
}

/// What pressing reload during a reload did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    /// In the sweet spot: the step is hurried.
    Perfect,
    /// Out of it, with more to come: the reload will stop after this step.
    Stop,
    /// Nothing: no sweet spot to hit, nothing to cut short, or the step is already hurried.
    Ignored,
}

/// What a moment of reloading got done.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct Progress {
    /// Rounds that have gone into the gun.
    pub rounds: u32,
    /// Whether the reload is over.
    pub done: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ReloadPlan {
    steps: Vec<Step>,
    index: usize,
    /// Seconds (as the plan counts them, a hurried step's too) into the current step.
    in_step: f32,
    /// The seconds the finished steps came to.
    done_time: f32,
    hurried: bool,
    /// For each timed step finished, whether it was a perfect press.
    results: Vec<bool>,
    /// Whether the load that will go in is a different one: the magazine's old rounds come out.
    pub swaps_load: bool,
}

impl ReloadPlan {
    pub fn new(steps: Vec<Step>) -> ReloadPlan {
        ReloadPlan { steps, index: 0, in_step: 0.0, done_time: 0.0, hurried: false, results: Vec::new(), swaps_load: false }
    }

    /// Seconds the whole reload takes, as it now stands.
    pub fn total(&self) -> f32 {
        self.steps.iter().map(|s| s.time).sum()
    }

    /// Seconds of it done.
    pub fn elapsed(&self) -> f32 {
        self.done_time + self.in_step
    }

    pub fn is_done(&self) -> bool {
        self.index >= self.steps.len()
    }

    /// The step in progress.
    pub fn current(&self) -> Option<&Step> {
        self.steps.get(self.index)
    }

    /// How far through the current step, 0 to 1.
    pub fn fraction(&self) -> f32 {
        self.current().map_or(1.0, |s| (self.in_step / s.time).clamp(0.0, 1.0))
    }

    /// The sweet spot of the step in progress, as fractions of it, if there is one to be hit (it has one, and
    /// has not been hurried already).
    pub fn sweet_spot(&self) -> Option<(f32, f32)> {
        self.current().filter(|s| s.timed && !self.hurried).map(|_| SWEET_SPOT)
    }

    pub fn in_sweet_spot(&self) -> bool {
        self.sweet_spot().is_some_and(|(a, b)| (a..=b).contains(&self.fraction()))
    }

    pub fn hurried(&self) -> bool {
        self.hurried
    }

    /// How many rounds the steps still to come will put in, this one included.
    pub fn rounds_to_come(&self) -> u32 {
        self.steps.iter().skip(self.index).map(|s| s.rounds).sum()
    }

    /// Whether every timed step so far was a perfect press, there have been at least two of them and none remain:
    /// the reload was worked like an expert.
    pub fn was_perfect(&self) -> bool {
        self.is_done() && self.results.len() >= 2 && self.results.iter().all(|&r| r)
    }

    /// Reload is pressed again.
    pub fn press(&mut self) -> Press {
        if self.hurried {
            return Press::Ignored;
        }
        if self.in_sweet_spot() {
            self.hurried = true;
            return Press::Perfect;
        }
        // Out of the sweet spot: stop once the step that is loading is done. That is this one if it is a timed step
        // (the stop only counts if there is something after it to give up), else the next timed one.
        let Some(keep) = (self.index..self.steps.len()).find(|&i| self.steps[i].timed) else { return Press::Ignored };
        if keep + 1 >= self.steps.len() {
            return Press::Ignored;
        }
        self.steps.truncate(keep + 1);
        Press::Stop
    }

    /// Time passes: `dt` seconds. A hurried step goes by faster.
    pub fn tick(&mut self, dt: f32) -> Progress {
        let mut progress = Progress::default();
        let mut left = dt;
        while left > 0.0 {
            let Some(&step) = self.current() else { break };
            let speed = if self.hurried { PERFECT_SPEED } else { 1.0 };
            let remaining = step.time - self.in_step;
            if left * speed < remaining {
                self.in_step += left * speed;
                break;
            }
            // The step finishes; what is left of the moment goes on to the next.
            left -= remaining / speed;
            progress.rounds += step.rounds;
            if step.timed {
                self.results.push(self.hurried);
            }
            self.done_time += step.time;
            self.index += 1;
            self.in_step = 0.0;
            self.hurried = false;
        }
        progress.done = self.is_done();
        progress
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A shotgun's reload: an opening and four shells.
    fn shells() -> ReloadPlan {
        let mut steps = vec![Step { time: 0.5, rounds: 0, timed: false }];
        steps.extend((0..4).map(|_| Step { time: 0.6, rounds: 1, timed: true }));
        ReloadPlan::new(steps)
    }

    /// Two clips.
    fn clips() -> ReloadPlan {
        ReloadPlan::new(vec![Step { time: 0.5, rounds: 0, timed: false }, Step { time: 1.3, rounds: 5, timed: true }, Step { time: 1.3, rounds: 5, timed: true }])
    }

    fn run(plan: &mut ReloadPlan, seconds: f32) -> Progress {
        let mut total = Progress::default();
        for _ in 0..(seconds / 0.005).round() as usize {
            let p = plan.tick(0.005);
            total.rounds += p.rounds;
            total.done |= p.done;
        }
        total
    }

    #[test]
    fn an_unpressed_reload_takes_the_sum_of_its_steps_and_loads_a_round_a_step() {
        let mut plan = shells();
        assert!((plan.total() - 2.9).abs() < 1e-5);
        assert_eq!(plan.rounds_to_come(), 4);
        let first = run(&mut plan, 1.2);
        assert_eq!(first.rounds, 1, "the opening and one shell");
        assert!(!first.done);
        let rest = run(&mut plan, 1.8);
        assert_eq!(rest.rounds, 3);
        assert!(rest.done && plan.is_done());
        assert!(!plan.was_perfect());
    }

    #[test]
    fn there_is_nothing_to_hit_in_the_opening_and_the_sweet_spot_is_in_the_middle_of_a_loading_step() {
        let mut plan = shells();
        assert_eq!(plan.sweet_spot(), None);
        assert!(!plan.in_sweet_spot());
        run(&mut plan, 0.5 + 0.1);
        assert_eq!(plan.sweet_spot(), Some(SWEET_SPOT));
        assert!(!plan.in_sweet_spot(), "too early, a sixth of the way in");
        run(&mut plan, 0.2);
        assert!(plan.in_sweet_spot(), "half way");
        run(&mut plan, 0.2);
        assert!(!plan.in_sweet_spot(), "too late");
    }

    #[test]
    fn a_press_in_the_sweet_spot_hurries_the_step_and_only_that_one() {
        let mut plan = clips();
        run(&mut plan, 0.5 + 0.65);
        assert_eq!(plan.press(), Press::Perfect);
        assert!(plan.hurried());
        assert_eq!(plan.press(), Press::Ignored, "once is enough");
        // 0.65 s of the clip is left, which takes a quarter of that now.
        let got = run(&mut plan, 0.65 / PERFECT_SPEED + 0.01);
        assert_eq!(got.rounds, 5);
        assert!(!plan.hurried(), "the next step is at its own pace");
        assert_eq!(plan.current().map(|s| s.rounds), Some(5));
        assert!(plan.fraction() < 0.05);
    }

    #[test]
    fn a_reload_with_a_perfect_press_in_each_clip_is_much_faster_and_is_perfect() {
        let mut plan = clips();
        let mut seconds = 0.0;
        let (mut rounds, mut presses) = (0, 0);
        while !plan.is_done() {
            if plan.in_sweet_spot() && !plan.hurried() {
                assert_eq!(plan.press(), Press::Perfect);
                presses += 1;
            }
            let p = plan.tick(0.005);
            rounds += p.rounds;
            seconds += 0.005;
        }
        assert_eq!((rounds, presses), (10, 2));
        assert!(seconds < plan.total() * 0.65, "{seconds} s against {}", plan.total());
        assert!(plan.was_perfect());
    }

    #[test]
    fn missing_one_of_the_clips_spoils_the_perfect_reload() {
        let mut plan = clips();
        run(&mut plan, 0.5 + 0.65);
        assert_eq!(plan.press(), Press::Perfect);
        run(&mut plan, 2.0);
        assert!(plan.is_done());
        assert!(!plan.was_perfect());
        // And one clip alone is not enough for it, however well it went.
        let mut single = ReloadPlan::new(vec![Step { time: 0.5, rounds: 0, timed: false }, Step { time: 1.3, rounds: 5, timed: true }]);
        run(&mut single, 0.5 + 0.65);
        single.press();
        run(&mut single, 1.0);
        assert!(single.is_done() && !single.was_perfect());
    }

    #[test]
    fn a_press_out_of_the_sweet_spot_stops_the_reload_after_the_step_it_is_in() {
        let mut plan = shells();
        run(&mut plan, 0.5 + 0.05);
        assert_eq!(plan.press(), Press::Stop, "early in the first shell");
        let rest = run(&mut plan, 3.0);
        assert_eq!(rest.rounds, 1, "that shell goes in and the reload ends");
        assert!(rest.done);
        assert!((plan.total() - 1.1).abs() < 1e-5);
    }

    #[test]
    fn a_press_while_the_gun_is_being_opened_stops_it_after_the_first_round() {
        let mut plan = clips();
        run(&mut plan, 0.2);
        assert_eq!(plan.press(), Press::Stop);
        let all = run(&mut plan, 5.0);
        assert_eq!(all.rounds, 5, "one clip: what the player asked for");
    }

    #[test]
    fn there_is_nothing_to_stop_on_the_last_round_or_a_single_step_reload() {
        let mut plan = shells();
        run(&mut plan, 0.5 + 0.6 * 3.0 + 0.05);
        assert_eq!(plan.rounds_to_come(), 1);
        assert_eq!(plan.press(), Press::Ignored, "this is the last shell");
        let mut magazine = ReloadPlan::new(vec![Step { time: 2.0, rounds: 0, timed: true }]);
        run(&mut magazine, 0.1);
        assert_eq!(magazine.press(), Press::Ignored, "too early costs nothing");
        run(&mut magazine, 0.7);
        assert!(magazine.in_sweet_spot());
        assert_eq!(magazine.press(), Press::Perfect);
        let got = run(&mut magazine, 1.0);
        assert!(got.done, "a perfect press finishes a magazine change well before its 2 s");
    }

    #[test]
    fn a_long_moment_goes_through_several_steps_at_once() {
        let mut plan = shells();
        let p = plan.tick(10.0);
        assert_eq!(p, Progress { rounds: 4, done: true });
        assert_eq!(plan.tick(1.0), Progress { rounds: 0, done: true });
    }

    #[test]
    fn the_time_done_and_the_time_in_all_agree_as_it_goes() {
        let mut plan = clips();
        let total = plan.total();
        let mut last = 0.0;
        while !plan.is_done() {
            plan.tick(0.01);
            assert!(plan.elapsed() >= last - 1e-6 && plan.elapsed() <= total + 1e-4);
            last = plan.elapsed();
        }
        assert!((plan.elapsed() - total).abs() < 1e-4);
    }
}
