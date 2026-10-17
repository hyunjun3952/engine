use crate::{Rect, Sprite, World};
use std::sync::Arc;

#[derive(Clone)]
enum Timing {
    // `frames` frames of equal length, `fps` of them a second.
    Even { frames: usize, fps: f32 },
    // Seconds each frame is shown.
    Each(Arc<[f32]>),
}

/// One row of a sprite sheet played as an animation: how long each frame shows, whether it loops, and named events
/// that fire when a frame starts (a footstep on the frame the foot lands).
///
/// ```
/// use engine::Clip;
///
/// let hit = Clip::new(2, 3, 12.0).once().event(1, "damage");
/// let walk = Clip::timed(0, &[0.1, 0.2, 0.1]);
/// # let _ = (hit, walk);
/// ```
#[derive(Clone)]
pub struct Clip {
    row: usize,
    timing: Timing,
    looping: bool,
    events: Option<Arc<[(usize, &'static str)]>>,
}

impl Clip {
    /// `frames` frames of the sheet row `row`, shown `fps` per second, looping.
    pub fn new(row: usize, frames: usize, fps: f32) -> Self {
        assert!(frames > 0 && fps > 0.0, "animation needs frames and fps");
        Clip { row, timing: Timing::Even { frames, fps }, looping: true, events: None }
    }

    /// A clip whose frames each last their own number of seconds (the first is column 0).
    pub fn timed(row: usize, seconds: &[f32]) -> Self {
        assert!(!seconds.is_empty() && seconds.iter().all(|&s| s > 0.0), "every frame needs a positive duration");
        Clip { row, timing: Timing::Each(seconds.into()), looping: true, events: None }
    }

    /// Plays once and holds the last frame; `Anim::finished` turns true.
    pub fn once(mut self) -> Self {
        self.looping = false;
        self
    }

    /// Fires `name` (see `Anim::fired`) when `frame` starts.
    pub fn event(mut self, frame: usize, name: &'static str) -> Self {
        let mut all = self.events.as_deref().unwrap_or_default().to_vec();
        all.push((frame, name));
        self.events = Some(all.into());
        self
    }

    fn frames(&self) -> usize {
        match &self.timing {
            Timing::Even { frames, .. } => *frames,
            Timing::Each(s) => s.len(),
        }
    }

    fn total(&self) -> f32 {
        match &self.timing {
            Timing::Even { frames, fps } => *frames as f32 / fps,
            Timing::Each(s) => s.iter().sum(),
        }
    }

    // When frame `k` starts, counted from the start of the clip.
    fn start(&self, k: usize) -> f32 {
        match &self.timing {
            Timing::Even { fps, .. } => k as f32 / fps,
            Timing::Each(s) => s[..k.min(s.len())].iter().sum(),
        }
    }

    fn frame_at(&self, t: f32) -> usize {
        match &self.timing {
            Timing::Even { frames, fps } => ((t * fps) as usize).min(frames - 1),
            Timing::Each(s) => {
                let mut end = 0.0;
                s.iter()
                    .position(|d| {
                        end += d;
                        t < end
                    })
                    .unwrap_or(s.len() - 1)
            }
        }
    }
}

/// Plays frames from a sheet cut into a grid of equal cells, one row per clip; [`animate`] advances it.
///
/// `play(row, frames, fps)` restarts only when the row changes; `play_clip`/`start` take a [`Clip`] with per-frame
/// times, `once()` and named events (read with `fired` after `animate`). An [`AnimStates`] next to it picks the clip
/// by state name.
///
/// ```
/// use engine::{Anim, Clip, Sprite};
/// use std::sync::Arc;
///
/// let sheet = Arc::new(Sprite::new(32, 32, vec![0; 1024]));
/// let mut anim = Anim::new(sheet, 16, 16);
/// anim.start(&Clip::new(1, 2, 10.0).once());
/// assert_eq!(anim.frame(), 0);
/// assert!(!anim.finished());
/// ```
pub struct Anim {
    pub(super) sheet: Arc<Sprite>,
    size: (usize, usize),
    clip: Clip,
    t: f32,
    // Nothing has been advanced since the clip started, so an event on its first frame is still due.
    fresh: bool,
    done: bool,
    fired: Vec<&'static str>,
    /// Draw the frames mirrored.
    pub flip: bool,
}

impl Anim {
    /// Cuts `sheet` into frames of `frame_w` by `frame_h`, with the first row playing.
    pub fn new(sheet: Arc<Sprite>, frame_w: usize, frame_h: usize) -> Self {
        Anim {
            sheet,
            size: (frame_w, frame_h),
            clip: Clip::new(0, 1, 1.0),
            t: 0.0,
            fresh: true,
            done: false,
            fired: Vec::new(),
            flip: false,
        }
    }

    /// Plays `frames` frames of row `row` at `fps`, looping; restarts only when the row changes, so a game can call
    /// it every step.
    pub fn play(&mut self, row: usize, frames: usize, fps: f32) {
        self.play_clip(&Clip::new(row, frames, fps));
    }

    /// Plays a clip; restarts only when its row differs from what is playing (see `start`).
    pub fn play_clip(&mut self, clip: &Clip) {
        let restart = self.clip.row != clip.row || self.done;
        self.clip = clip.clone();
        if restart {
            self.rewind();
        }
    }

    /// Plays a clip from its first frame, even if it is the one already playing.
    pub fn start(&mut self, clip: &Clip) {
        self.clip = clip.clone();
        self.rewind();
    }

    fn rewind(&mut self) {
        (self.t, self.fresh, self.done) = (0.0, true, false);
    }

    /// Whether a clip that plays once has reached its end.
    pub fn finished(&self) -> bool {
        self.done
    }

    /// The events that fired during the last `animate`.
    pub fn events(&self) -> &[&'static str] {
        &self.fired
    }

    /// True if the event `name` fired during the last `animate`.
    pub fn fired(&self, name: &str) -> bool {
        self.fired.contains(&name)
    }

    /// Where the clip is now, for `restore`.
    pub fn state(&self) -> AnimState {
        AnimState { t: self.t, flip: self.flip, done: self.done }
    }

    /// Puts the clip back where `state` was, after the game started the same clip again. No event fires for it.
    pub fn restore(&mut self, state: &AnimState) {
        let total = self.clip.total();
        (self.t, self.flip, self.done, self.fresh) = (state.t.clamp(0.0, total), state.flip, state.done, false);
        self.fired.clear();
    }

    pub(super) fn src(&self) -> Rect {
        let (w, h) = self.size;
        Rect::new((self.frame() * w) as f32, (self.clip.row * h) as f32, w as f32, h as f32)
    }

    /// The column of the sheet being shown.
    pub fn frame(&self) -> usize {
        self.clip.frame_at(self.t)
    }

    fn advance(&mut self, dt: f32) {
        self.fired.clear();
        if self.done {
            return;
        }
        let total = self.clip.total();
        let (from, to) = (self.t, self.t + dt);
        if let Some(events) = &self.clip.events {
            for &(frame, name) in events.iter().filter(|e| e.0 < self.clip.frames()) {
                let at = self.clip.start(frame);
                // Every time the event's moment comes round inside this step (a looping clip may wrap once).
                let mut moment = at;
                while moment <= to {
                    if (moment > from || (self.fresh && moment >= from)) && (self.clip.looping || moment < total) {
                        self.fired.push(name);
                    }
                    if !self.clip.looping {
                        break;
                    }
                    moment += total;
                }
            }
        }
        self.fresh = false;
        if self.clip.looping {
            self.t = to % total;
        } else if to >= total {
            // Held on the last frame, a hair before the end.
            (self.t, self.done) = (total * (1.0 - f32::EPSILON), true);
        } else {
            self.t = to;
        }
    }
}

/// Where an [`Anim`] is in its clip, to save beside the entity: the sheet and clip come from the game's own code.
///
/// ```
/// use engine::{Anim, Clip, Sprite};
/// use std::sync::Arc;
///
/// let sheet = Arc::new(Sprite::new(32, 16, vec![0; 512]));
/// let mut anim = Anim::new(sheet.clone(), 16, 16);
/// anim.start(&Clip::new(0, 2, 10.0));
/// anim.flip = true;
/// let saved = anim.state();
///
/// let mut copy = Anim::new(sheet, 16, 16);
/// copy.start(&Clip::new(0, 2, 10.0));
/// copy.restore(&saved);
/// assert!(copy.flip);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnimState {
    /// Seconds into the clip.
    pub t: f32,
    /// Whether the frames are drawn mirrored.
    pub flip: bool,
    /// Whether a clip that plays once has ended.
    pub done: bool,
}

/// The states of an animated thing and the clip of each, with the moves between them. Put it on an entity next to
/// its `Anim`; `animate` plays the clip of the current state.
///
/// ```ignore
/// let states = AnimStates::new()
///     .state("idle", Clip::new(0, 4, 6.0))
///     .state("hit", Clip::new(1, 3, 12.0).once())
///     .then("hit", "idle");
/// ```
pub struct AnimStates {
    states: Vec<(&'static str, Clip)>,
    after: Vec<(&'static str, &'static str)>,
    current: usize,
    wanted: Option<usize>,
    started: bool,
}

impl AnimStates {
    /// No states yet; add them with `state`.
    pub fn new() -> Self {
        AnimStates { states: Vec::new(), after: Vec::new(), current: 0, wanted: None, started: false }
    }

    /// Adds a state; the first one added is where it starts.
    pub fn state(mut self, name: &'static str, clip: Clip) -> Self {
        assert!(!self.states.iter().any(|s| s.0 == name), "state {name:?} is added twice");
        self.states.push((name, clip));
        self
    }

    /// When the clip of `from` (which plays once) finishes, move on to `to`.
    pub fn then(mut self, from: &'static str, to: &'static str) -> Self {
        self.index(from);
        self.index(to);
        self.after.push((from, to));
        self
    }

    fn index(&self, name: &str) -> usize {
        self.states.iter().position(|s| s.0 == name).unwrap_or_else(|| panic!("unknown animation state {name:?}"))
    }

    /// Asks for a state; it starts from its first frame at the next `animate`, unless it is already the current one.
    /// Panics on a name that was not added, like an unbound action.
    pub fn set(&mut self, name: &str) {
        let i = self.index(name);
        self.wanted = (i != self.current || !self.started).then_some(i);
    }

    /// The name of the state now playing.
    pub fn current(&self) -> &'static str {
        self.states[self.current].0
    }

    /// Puts the thing back in state `name` with `anim` where `state` was, after a load: it starts the clip and restores
    /// the position in one go. Panics on a name that was not added, like `set`.
    pub fn restore(&mut self, anim: &mut Anim, name: &str, state: &AnimState) {
        self.current = self.index(name);
        (self.wanted, self.started) = (None, true);
        anim.start(&self.states[self.current].1);
        anim.restore(state);
    }

    fn sync(&mut self, anim: &mut Anim) {
        assert!(!self.states.is_empty(), "AnimStates has no states");
        let next = self.wanted.take().or_else(|| {
            let from = self.states[self.current].0;
            anim.finished().then(|| self.after.iter().find(|a| a.0 == from).map(|a| self.index(a.1))).flatten()
        });
        if next.is_some() || !self.started {
            self.current = next.unwrap_or(self.current);
            self.started = true;
            anim.start(&self.states[self.current].1);
        }
    }
}

impl Default for AnimStates {
    fn default() -> Self {
        AnimStates::new()
    }
}

/// ```
/// use engine::{Anim, Body, Clip, Rect, Sprite, World, animate};
/// use std::sync::Arc;
///
/// let mut world = World::new();
/// let mut anim = Anim::new(Arc::new(Sprite::new(32, 16, vec![0; 512])), 16, 16);
/// anim.play_clip(&Clip::new(0, 2, 10.0));
/// let e = world.spawn((Body(Rect::new(0.0, 0.0, 16.0, 16.0)), anim));
/// animate(&mut world, 0.1);
/// assert_eq!(world.get::<&Anim>(e).unwrap().frame(), 1);
/// ```
pub fn animate(world: &mut World, dt: f32) {
    for (anim, states) in world.query_mut::<(&mut Anim, &mut AnimStates)>() {
        states.sync(anim);
    }
    for a in world.query_mut::<&mut Anim>() {
        a.advance(dt);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anim() -> Anim {
        Anim::new(Arc::new(Sprite::new(40, 20, vec![0; 800])), 10, 10)
    }

    #[test]
    fn restore_puts_back_the_state_and_the_position() {
        let states =
            || AnimStates::new().state("idle", Clip::new(0, 4, 10.0)).state("hit", Clip::new(1, 4, 10.0).once());
        let (mut s, mut a) = (states(), anim());
        s.set("hit");
        s.sync(&mut a);
        a.advance(0.25);
        let (name, saved) = (s.current(), a.state());

        let (mut s2, mut b) = (states(), anim());
        s2.restore(&mut b, name, &saved);
        s2.sync(&mut b);
        assert_eq!((s2.current(), b.state(), b.frame()), ("hit", saved, a.frame()));
    }

    fn frames(a: &mut Anim, steps: usize, dt: f32) -> Vec<usize> {
        (0..steps)
            .map(|_| {
                a.advance(dt);
                a.frame()
            })
            .collect()
    }

    #[test]
    fn an_even_clip_loops_through_its_frames_at_its_rate() {
        let mut a = anim();
        a.play(1, 4, 10.0);
        assert_eq!(a.src(), Rect::new(0.0, 10.0, 10.0, 10.0));
        assert_eq!(frames(&mut a, 10, 0.1), [1, 2, 3, 0, 1, 2, 3, 0, 1, 2]);
        a.play(1, 4, 10.0);
        assert_eq!(a.frame(), 2, "the same row keeps going");
        a.play(0, 4, 10.0);
        assert_eq!(a.frame(), 0, "a new row starts over");
    }

    #[test]
    fn frames_can_last_their_own_time() {
        let mut a = anim();
        a.play_clip(&Clip::timed(0, &[0.3, 0.1, 0.2]));
        let seen: Vec<usize> = (0..12)
            .map(|_| {
                a.advance(0.05);
                a.frame()
            })
            .collect();
        assert_eq!(seen, [0, 0, 0, 0, 0, 1, 1, 2, 2, 2, 2, 0]);
    }

    #[test]
    fn a_clip_that_plays_once_holds_its_last_frame_and_says_it_is_done() {
        let mut a = anim();
        a.play_clip(&Clip::new(0, 3, 10.0).once());
        assert!(!a.finished());
        assert_eq!(frames(&mut a, 5, 0.1), [1, 2, 2, 2, 2]);
        assert!(a.finished());
        a.start(&Clip::new(0, 3, 10.0).once());
        assert!(!a.finished() && a.frame() == 0, "starting again rewinds it");
        frames(&mut a, 5, 0.1);
        a.play(0, 3, 10.0);
        assert_eq!(frames(&mut a, 3, 0.1), [1, 2, 0], "a finished clip played again on its row runs, not frozen");
    }

    #[test]
    fn events_fire_once_when_their_frame_starts_including_the_first_and_after_a_wrap() {
        let mut a = anim();
        a.play_clip(&Clip::new(0, 4, 10.0).event(0, "start").event(2, "step"));
        let mut log = Vec::new();
        for _ in 0..10 {
            a.advance(0.1);
            log.push(a.events().to_vec());
        }
        let count = |name: &str| log.iter().flatten().filter(|n| **n == name).count();
        assert_eq!((count("start"), count("step")), (3, 3), "at 0, 0.4 and 0.8 s and at 0.2, 0.6 and 1.0 s: {log:?}");
        assert_eq!(log[0], ["start"], "frame 0's event fires on the first step");
        assert!(log[1].contains(&"step") && log[5].contains(&"step") && log[9].contains(&"step"), "{log:?}");
        assert!(!a.fired("nope"));
        a.advance(0.4);
        assert_eq!(a.events().len(), 2, "a long step fires everything it passed over: {:?}", a.events());
    }

    #[test]
    fn a_once_clip_fires_its_events_once_and_not_again_when_held() {
        let mut a = anim();
        a.play_clip(&Clip::new(0, 2, 10.0).once().event(1, "land"));
        let mut n = 0;
        for _ in 0..6 {
            a.advance(0.1);
            n += a.events().len();
        }
        assert_eq!(n, 1);
    }

    #[test]
    fn states_switch_clips_and_move_on_when_a_once_clip_ends() {
        let mut w = World::new();
        let states = AnimStates::new()
            .state("idle", Clip::new(0, 4, 4.0))
            .state("hit", Clip::new(1, 2, 10.0).once())
            .then("hit", "idle");
        let e = w.spawn((anim(), states));
        let row = |w: &World| w.get::<&Anim>(e).unwrap().src().y;
        animate(&mut w, 0.0);
        assert_eq!(w.get::<&AnimStates>(e).unwrap().current(), "idle");
        assert_eq!(row(&w), 0.0);
        w.get::<&mut AnimStates>(e).unwrap().set("hit");
        animate(&mut w, 0.05);
        assert_eq!((w.get::<&AnimStates>(e).unwrap().current(), row(&w)), ("hit", 10.0));
        for _ in 0..4 {
            animate(&mut w, 0.1);
        }
        assert_eq!((w.get::<&AnimStates>(e).unwrap().current(), row(&w)), ("idle", 0.0), "it went back by itself");
        let before = w.get::<&Anim>(e).unwrap().frame();
        w.get::<&mut AnimStates>(e).unwrap().set("idle");
        animate(&mut w, 0.25);
        assert_ne!(w.get::<&Anim>(e).unwrap().frame(), before, "asking for the current state does not restart it");
    }

    #[test]
    #[should_panic(expected = "unknown animation state")]
    fn asking_for_a_state_that_was_not_added_panics() {
        AnimStates::new().state("idle", Clip::new(0, 1, 1.0)).set("walk");
    }
}
