use engine::{
    Anim, Body, Burst, Config, Ctx, Entity, Gfx, Key, Physics, Rect, RigidBody, Scene, Tilemap, Trans, Vel, World,
    animate, burst, draw_list, particles,
};

const TILE: usize = 16;
const DUST: Burst = Burst { count: 6, color: 0xdddddd, speed: 50.0, life: 0.3, size: 3.0, gravity: 0.0 };

struct Platformer {
    world: World,
    physics: Physics,
    player: Entity,
    map: Tilemap,
    start: Rect,
    grounded: bool,
    level: String,
}

fn load_level(ctx: &mut Ctx, path: &str) -> Result<(Tilemap, Rect), String> {
    let map = Tilemap::try_load(path, TILE, ctx.assets().sprite("assets/images/tiles.png"))?;
    let (x, y) = map.marks(b'P').next().ok_or(format!("{path}: level needs a P marker"))?;
    Ok((map, Rect { x, y: y + TILE as f32 - 24.0, w: 12.0, h: 24.0 }))
}

impl Platformer {
    fn new(ctx: &mut Ctx, level: &str) -> Self {
        let (map, start) = load_level(ctx, level).unwrap_or_else(|e| panic!("{e}"));
        ctx.watch(level);
        let mut world = World::new();
        let player = world.spawn((
            Body(start),
            Vel { x: 0.0, y: 0.0 },
            RigidBody::dynamic(1.0).with_friction(0.0),
            Anim::new(ctx.assets().sprite("assets/images/player.png"), 12, 24),
        ));
        Platformer { world, physics: Physics::default(), player, map, start, grounded: false, level: level.to_string() }
    }
}

impl Scene for Platformer {
    fn on_enter(&mut self, ctx: &mut Ctx) {
        ctx.cam().reset();
        ctx.audio().music("assets/audio/music.wav");
    }

    fn on_exit(&mut self, ctx: &mut Ctx) {
        ctx.audio().stop_music();
    }

    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input().pressed("back") {
            return Trans::Quit;
        }

        if ctx.changed(&self.level) {
            match load_level(ctx, &self.level) {
                Ok((map, start)) => (self.map, self.start) = (map, start),
                Err(e) => eprintln!("{e}"),
            }
        }

        let v = self.world.query_one_mut::<&mut Vel>(self.player).unwrap();
        let (was_grounded, mut dust) = (self.grounded, false);
        v.x = ctx.input().axis("left", "right") * 160.0;
        if self.grounded && ctx.input().pressed("jump") {
            v.y = -330.0;
            ctx.audio().play("assets/audio/jump.wav");
            dust = true;
        }

        let fall = v.y + self.physics.gravity.1 * ctx.dt();
        self.physics.step(&mut self.world, Some(&self.map), ctx.dt());
        let (b, v, anim) = self.world.query_one_mut::<(&mut Body, &mut Vel, &mut Anim)>(self.player).unwrap();
        v.y = v.y.min(600.0);
        let feet = Rect { x: b.0.x, y: b.0.y + b.0.h, w: b.0.w, h: 1.0 };
        self.grounded = v.y >= 0.0 && !self.map.solid_in(feet).is_empty();
        dust |= self.grounded && !was_grounded;
        if self.grounded && !was_grounded && fall > 450.0 {
            ctx.cam().shake(3.0, 0.2);
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
            ctx.cam().cut();
            ctx.cam().shake(6.0, 0.3);
        }
        let dt = ctx.dt();
        ctx.cam().follow(&b.0, self.map.pixel_size(), dt);
        let feet = (b.0.x + b.0.w / 2.0, b.0.y + b.0.h);
        if dust {
            burst(&mut self.world, ctx.rng(), feet, &DUST);
        }
        animate(&mut self.world, ctx.dt());
        particles(&mut self.world, ctx.dt());
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x6fa8e0);
        let mut cmds = self.map.draw_list(g.view());
        cmds.extend(draw_list(&self.world));
        g.draw(cmds);
        g.text("ARROWS or A/D: move   SPACE: jump   F3: debug", 8, 340, 1, 0xffffff);
    }
}

fn main() {
    let level = std::env::args().nth(1).unwrap_or_else(|| "assets/levels/level1.txt".into());
    engine::run(Config { title: "platformer", scale: 2, ..Config::default() }, |ctx| {
        ctx.bind("left", &[Key::Left, Key::A]);
        ctx.bind("right", &[Key::Right, Key::D]);
        ctx.bind("jump", &[Key::Space, Key::Up, Key::W]);
        ctx.bind("back", &[Key::Escape]);
        if let Err(e) = ctx.load_bindings("input.txt") {
            eprintln!("{e}");
        }
        Box::new(Platformer::new(ctx, &level))
    });
}
