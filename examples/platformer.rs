use engine::{
    Anim, Body, Burst, Config, Ctx, Entity, Gfx, Key, Rect, Scene, Tilemap, Trans, Vel, World, animate, burst,
    particles, render,
};

const TILE: usize = 16;
const DUST: Burst = Burst { count: 6, color: 0xdddddd, speed: 50.0, life: 0.3, size: 3.0, gravity: 0.0 };

struct Platformer {
    world: World,
    player: Entity,
    map: Tilemap,
    start: Rect,
    grounded: bool,
}

impl Platformer {
    fn new(ctx: &mut Ctx, level: &str) -> Self {
        let map = Tilemap::load(level, TILE, ctx.assets.sprite("assets/tiles.png"));
        let (x, y) = map.marks(b'P').next().expect("level needs a P marker");
        let start = Rect { x, y: y + TILE as f32 - 24.0, w: 12.0, h: 24.0 };
        let mut world = World::new();
        let player = world.spawn((
            Body(start),
            Vel { x: 0.0, y: 0.0 },
            Anim::new(ctx.assets.sprite("assets/player.png"), 12, 24),
        ));
        ctx.audio.music("assets/music.wav");
        Platformer { world, player, map, start, grounded: false }
    }
}

impl Scene for Platformer {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input.pressed(Key::Escape) {
            return Trans::Quit;
        }

        let (b, v, anim) = self.world.query_one_mut::<(&mut Body, &mut Vel, &mut Anim)>(self.player).unwrap();
        let (was_grounded, mut dust) = (self.grounded, false);
        v.x = ctx.input.axis(Key::Left, Key::Right) * 160.0;
        v.y = (v.y + 900.0 * ctx.dt).min(600.0);
        if self.grounded && (ctx.input.pressed(Key::Space) || ctx.input.pressed(Key::Up)) {
            v.y = -330.0;
            ctx.audio.play("assets/jump.wav");
            dust = true;
        }

        let (r, _, hit_y) = self.map.slide(&b.0, v.x * ctx.dt, v.y * ctx.dt);
        b.0 = r;
        self.grounded = hit_y && v.y > 0.0;
        dust |= self.grounded && !was_grounded;
        if self.grounded && v.y > 450.0 {
            ctx.cam.shake(3.0, 0.2);
        }
        if hit_y {
            v.y = 0.0;
        }
        if v.x != 0.0 {
            anim.flip = v.x < 0.0;
        }
        if !self.grounded {
            anim.play(2, 1, 1.0);
        } else if v.x != 0.0 {
            anim.play(1, 4, 10.0);
        } else {
            anim.play(0, 2, 2.0);
        }
        if b.0.y > self.map.pixel_size().1 {
            b.0 = self.start;
            v.y = 0.0;
            ctx.cam.cut();
            ctx.cam.shake(6.0, 0.3);
        }
        ctx.cam.follow(&b.0, self.map.pixel_size(), ctx.dt);
        let feet = (b.0.x + b.0.w / 2.0, b.0.y + b.0.h);
        if dust {
            burst(&mut self.world, &mut ctx.rng, feet, &DUST);
        }
        animate(&mut self.world, ctx.dt);
        particles(&mut self.world, ctx.dt);
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x6fa8e0);
        self.map.draw(g);
        render(&self.world, g);
        g.text("ARROWS: move   SPACE: jump   F3: debug", 8, 340, 1, 0xffffff);
    }
}

fn main() {
    let level = std::env::args().nth(1).unwrap_or_else(|| "assets/level1.txt".into());
    engine::run(Config { title: "platformer", ..Config::default() }, |ctx| Box::new(Platformer::new(ctx, &level)));
}
