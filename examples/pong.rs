use engine::{
    Body, Burst, Collider, Config, Ctx, Ease, Entity, Gfx, Key, Look, Rect, Scene, Trans, Tween, Vel, World, burst,
    collisions, draw_list, movement, particles,
};

const SPARK: Burst = Burst { count: 6, color: 0xffffff, speed: 120.0, life: 0.3, size: 3.0, gravity: 0.0 };
// Faster than a ball plus a paddle per tick and the ball would skip through the paddle.
const MAX_SPEED: f32 = 900.0;
const W: f32 = 640.0;
const H: f32 = 360.0;

struct Menu;

impl Scene for Menu {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input().pressed("confirm") {
            return Trans::Push(Box::new(Pong::new(ctx)));
        }
        if ctx.input().pressed("back") {
            return Trans::Quit;
        }
        Trans::None
    }
    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        g.text("PONG", 240, 120, 6, 0xffffff);
        g.text("ENTER: play   ESC: quit", 200, 220, 2, 0xaaaaaa);
    }
}

struct Pause {
    fade: Tween,
}

impl Pause {
    fn new() -> Self {
        Pause { fade: Tween::new(0.0, 150.0, 0.15, Ease::OutQuad) }
    }
}

impl Scene for Pause {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        self.fade.tick(ctx.dt());
        if ctx.input().pressed("pause") { Trans::Pop } else { Trans::None }
    }
    fn draw(&self, g: &mut Gfx) {
        g.fill_alpha(&Rect { x: 0.0, y: 0.0, w: W, h: H }, 0x000000, self.fade.value() as u8);
        g.text("PAUSED", 240, 170, 4, 0xffff66);
    }
}

struct Paddle {
    up: &'static str,
    down: &'static str,
}

struct Pong {
    world: World,
    ball: Entity,
    score: [u32; 2],
}

impl Pong {
    fn new(ctx: &mut Ctx) -> Self {
        let mut world = World::new();
        let paddle = |x, up, down| {
            (Body(Rect { x, y: 150.0, w: 10.0, h: 60.0 }), Look::Color(0xffffff), Paddle { up, down }, Collider)
        };
        world.spawn(paddle(20.0, "p1_up", "p1_down"));
        world.spawn(paddle(W - 30.0, "p2_up", "p2_down"));
        let ball = world.spawn((
            Body(Rect { x: 0.0, y: 0.0, w: 16.0, h: 16.0 }),
            Vel { x: 0.0, y: 0.0 },
            Look::Sprite(ctx.assets().sprite("assets/images/ball.png")),
            Collider,
        ));
        let mut p = Pong { world, ball, score: [0, 0] };
        p.serve();
        p
    }

    fn serve(&mut self) {
        let (b, v) = self.world.query_one_mut::<(&mut Body, &mut Vel)>(self.ball).unwrap();
        (b.0.x, b.0.y, v.x, v.y) = (W / 2.0, H / 2.0, 220.0, 130.0);
    }
}

impl Scene for Pong {
    fn on_enter(&mut self, ctx: &mut Ctx) {
        ctx.cam().reset();
    }

    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input().pressed("back") {
            return Trans::Pop;
        }
        if ctx.input().pressed("pause") {
            return Trans::Push(Box::new(Pause::new()));
        }

        for (b, p) in self.world.query_mut::<(&mut Body, &Paddle)>() {
            let dy = ctx.input().axis(p.up, p.down) * 300.0 * ctx.dt();
            b.0.y = (b.0.y + dy).clamp(0.0, H - b.0.h);
        }
        movement(&mut self.world, ctx.dt());
        particles(&mut self.world, ctx.dt());

        let mut spark = None;
        let paddle = collisions(&self.world).iter().find_map(|c| c.other(self.ball));
        let paddle = paddle.map(|e| self.world.get::<&Body>(e).unwrap().0);
        let (b, v) = self.world.query_one_mut::<(&mut Body, &mut Vel)>(self.ball).unwrap();
        // Set the direction instead of flipping it: a ball still overlapping next tick would flip back and get stuck.
        if b.0.y < 0.0 {
            (b.0.y, v.y) = (0.0, v.y.abs());
        } else if b.0.y + b.0.h > H {
            (b.0.y, v.y) = (H - b.0.h, -v.y.abs());
        }
        if let Some(p) = paddle {
            ctx.audio().play("assets/audio/hit.wav");
            let away = if b.0.x + b.0.w / 2.0 < p.x + p.w / 2.0 { -1.0 } else { 1.0 };
            b.0.x = if away < 0.0 { p.x - b.0.w } else { p.x + p.w };
            v.x = away * (v.x.abs() * 1.05).min(MAX_SPEED);
            spark = Some((b.0.x + b.0.w / 2.0, b.0.y + b.0.h / 2.0));
        }
        if b.0.x < -20.0 || b.0.x > W + 20.0 {
            self.score[(b.0.x > 0.0) as usize] += 1;
            ctx.audio().play("assets/audio/score.wav");
            ctx.cam().shake(4.0, 0.2);
            self.serve();
        }
        if let Some(at) = spark {
            burst(&mut self.world, ctx.rng(), at, &SPARK);
        }
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        g.draw(draw_list(&self.world));
        g.text(&format!("{}  {}", self.score[0], self.score[1]), W as i32 / 2 - 40, 10, 3, 0xaaaaaa);
    }
}

fn main() {
    engine::run(Config { scale: 2, ..Config::default() }, |ctx| {
        for (action, keys) in [
            ("confirm", &[Key::Enter][..]),
            ("back", &[Key::Escape]),
            ("pause", &[Key::P]),
            ("p1_up", &[Key::W]),
            ("p1_down", &[Key::S]),
            ("p2_up", &[Key::Up]),
            ("p2_down", &[Key::Down]),
        ] {
            ctx.bind(action, keys);
        }
        if let Err(e) = ctx.load_bindings("input.txt") {
            eprintln!("{e}");
        }
        Box::new(Menu)
    });
}
