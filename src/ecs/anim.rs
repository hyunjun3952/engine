use crate::{Rect, Sprite, World};
use std::sync::Arc;

pub struct Anim {
    pub(super) sheet: Arc<Sprite>,
    size: (usize, usize),
    row: usize,
    frames: usize,
    fps: f32,
    t: f32,
    pub flip: bool,
}

impl Anim {
    pub fn new(sheet: Arc<Sprite>, frame_w: usize, frame_h: usize) -> Self {
        Anim { sheet, size: (frame_w, frame_h), row: 0, frames: 1, fps: 1.0, t: 0.0, flip: false }
    }

    pub fn play(&mut self, row: usize, frames: usize, fps: f32) {
        assert!(frames > 0 && fps > 0.0, "animation needs frames and fps");
        if self.row != row {
            self.t = 0.0;
        }
        (self.row, self.frames, self.fps) = (row, frames, fps);
    }

    pub(super) fn src(&self) -> Rect {
        let (w, h) = self.size;
        Rect { x: (self.frame() * w) as f32, y: (self.row * h) as f32, w: w as f32, h: h as f32 }
    }

    fn frame(&self) -> usize {
        (self.t * self.fps) as usize % self.frames
    }
}

pub fn animate(world: &mut World, dt: f32) {
    for a in world.query_mut::<&mut Anim>() {
        a.t = (a.t + dt) % (a.frames as f32 / a.fps);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animation_advances_loops_and_resets_on_clip_change() {
        let mut w = World::new();
        let mut a = Anim::new(Arc::new(Sprite::load("assets/images/player.png")), 12, 24);
        a.play(1, 4, 10.0);
        let e = w.spawn((a,));
        animate(&mut w, 0.25);
        assert_eq!(w.get::<&Anim>(e).unwrap().frame(), 2);
        animate(&mut w, 0.2);
        assert_eq!(w.get::<&Anim>(e).unwrap().frame(), 0);
        w.query_one_mut::<&mut Anim>(e).unwrap().play(0, 2, 2.0);
        assert_eq!(w.get::<&Anim>(e).unwrap().frame(), 0);
    }
}
