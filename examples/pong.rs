use engine::{
    Body, Collider, Config, Ctx, Entity, Gfx, Key, Look, Rect, Scene, Trans, Vel, World, collisions, movement, render,
};

const W: f32 = 640.0;
const H: f32 = 360.0;

struct Menu;

impl Scene for Menu {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input.pressed(Key::Enter) {
            return Trans::Push(Box::new(Pong::new(ctx)));
        }
        if ctx.input.pressed(Key::Escape) {
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

struct Pause;

impl Scene for Pause {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input.pressed(Key::P) { Trans::Pop } else { Trans::None }
    }
    fn draw(&self, g: &mut Gfx) {
        g.fill(&Rect { x: 200.0, y: 140.0, w: 240.0, h: 80.0 }, 0x000000);
        g.text("PAUSED", 240, 170, 4, 0xffff66);
    }
}

struct Paddle {
    up: Key,
    down: Key,
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
        world.spawn(paddle(20.0, Key::W, Key::S));
        world.spawn(paddle(W - 30.0, Key::Up, Key::Down));
        let ball = world.spawn((
            Body(Rect { x: 0.0, y: 0.0, w: 16.0, h: 16.0 }),
            Vel { x: 0.0, y: 0.0 },
            Look::Sprite(ctx.assets.sprite("assets/ball.png")),
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
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input.pressed(Key::Escape) {
            return Trans::Pop;
        }
        if ctx.input.pressed(Key::P) {
            return Trans::Push(Box::new(Pause));
        }

        for (b, p) in self.world.query_mut::<(&mut Body, &Paddle)>() {
            let dy = ctx.input.axis(p.up, p.down) * 300.0 * ctx.dt;
            b.0.y = (b.0.y + dy).clamp(0.0, H - b.0.h);
        }
        movement(&mut self.world, ctx.dt);

        let hit = collisions(&self.world).iter().any(|c| c.other(self.ball).is_some());
        let (b, v) = self.world.query_one_mut::<(&mut Body, &mut Vel)>(self.ball).unwrap();
        if b.0.y < 0.0 || b.0.y + b.0.h > H {
            v.y = -v.y;
        }
        if hit {
            ctx.audio.play("assets/hit.wav");
            v.x = -v.x * 1.05;
        }
        if b.0.x < -20.0 || b.0.x > W + 20.0 {
            self.score[(b.0.x > 0.0) as usize] += 1;
            ctx.audio.play("assets/score.wav");
            ctx.cam.shake(4.0, 0.2);
            self.serve();
        }
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        render(&self.world, g);
        g.text(&format!("{}  {}", self.score[0], self.score[1]), W as i32 / 2 - 40, 10, 3, 0xaaaaaa);
    }
}

fn main() {
    engine::run(Config::default(), |_| Box::new(Menu));
}
