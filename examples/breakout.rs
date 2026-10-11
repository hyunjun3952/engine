use engine::{
    Body, Collider, Config, Ctx, Entity, Gfx, Key, Look, Rect, Scene, Trans, Vel, World, collisions, movement, render,
};

const W: f32 = 640.0;
const H: f32 = 360.0;

struct Slider {
    left: Key,
    right: Key,
}
struct Brick;

struct Over(&'static str);

impl Scene for Over {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input.pressed(Key::Enter) {
            return Trans::Replace(Box::new(Breakout::new(ctx)));
        }
        if ctx.input.pressed(Key::Escape) {
            return Trans::Quit;
        }
        Trans::None
    }
    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        g.text(self.0, 200, 140, 4, 0xffff66);
        g.text("ENTER: again   ESC: quit", 200, 220, 2, 0xaaaaaa);
    }
}

struct Breakout {
    world: World,
    paddle: Entity,
    ball: Entity,
    score: u32,
    lives: u32,
}

impl Breakout {
    fn new(ctx: &mut Ctx) -> Self {
        let mut world = World::new();
        let paddle = world.spawn((
            Body(Rect { x: W / 2.0 - 40.0, y: H - 30.0, w: 80.0, h: 10.0 }),
            Look::Color(0xffffff),
            Slider { left: Key::Left, right: Key::Right },
            Collider,
        ));
        for row in 0..5 {
            for col in 0..10 {
                world.spawn((
                    Body(Rect { x: 20.0 + col as f32 * 60.0, y: 50.0 + row as f32 * 22.0, w: 56.0, h: 18.0 }),
                    Look::Color([0xe05555, 0xe0a055, 0xe0e055, 0x55e055, 0x5588e0][row]),
                    Brick,
                    Collider,
                ));
            }
        }
        let ball = world.spawn((
            Body(Rect { x: 0.0, y: 0.0, w: 16.0, h: 16.0 }),
            Vel { x: 0.0, y: 0.0 },
            Look::Sprite(ctx.assets.sprite("assets/ball.png")),
            Collider,
        ));
        let mut g = Breakout { world, paddle, ball, score: 0, lives: 3 };
        g.serve();
        g
    }

    fn serve(&mut self) {
        let (b, v) = self.world.query_one_mut::<(&mut Body, &mut Vel)>(self.ball).unwrap();
        (b.0.x, b.0.y, v.x, v.y) = (W / 2.0, H / 2.0, 120.0, -240.0);
    }
}

impl Scene for Breakout {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input.pressed(Key::Escape) {
            return Trans::Quit;
        }

        for (b, s) in self.world.query_mut::<(&mut Body, &Slider)>() {
            let dx = ctx.input.axis(s.left, s.right) * 400.0 * ctx.dt;
            b.0.x = (b.0.x + dx).clamp(0.0, W - b.0.w);
        }
        movement(&mut self.world, ctx.dt);

        let contacts: Vec<_> = collisions(&self.world).into_iter().filter(|c| c.other(self.ball).is_some()).collect();
        let paddle = self.world.get::<&Body>(self.paddle).unwrap().0;
        let bricks_left = self.world.query::<&Brick>().iter().count();
        let (b, v) = self.world.query_one_mut::<(&mut Body, &mut Vel)>(self.ball).unwrap();

        if b.0.x < 0.0 {
            b.0.x = 0.0;
            v.x = v.x.abs();
        }
        if b.0.x + b.0.w > W {
            b.0.x = W - b.0.w;
            v.x = -v.x.abs();
        }
        if b.0.y < 0.0 {
            b.0.y = 0.0;
            v.y = v.y.abs();
        }
        let lost = b.0.y > H;

        let mut broken = None;
        for c in &contacts {
            let other = c.other(self.ball).unwrap();
            if other == self.paddle {
                if v.y > 0.0 {
                    v.y = -v.y.abs();
                    ctx.audio.play("assets/hit.wav");
                    v.x = ((b.0.x + b.0.w / 2.0) - (paddle.x + paddle.w / 2.0)) / (paddle.w / 2.0) * 250.0;
                }
            } else if broken.is_none() {
                if c.x_axis() {
                    v.x = -v.x
                } else {
                    v.y = -v.y
                }
                broken = Some(other);
            }
        }

        if let Some(e) = broken {
            ctx.audio.play("assets/brick.wav");
            self.world.despawn(e).unwrap();
            self.score += 10;
            if bricks_left == 1 {
                return Trans::Replace(Box::new(Over("YOU WIN")));
            }
        }
        if lost {
            self.lives -= 1;
            ctx.audio.play("assets/score.wav");
            if self.lives == 0 {
                return Trans::Replace(Box::new(Over("GAME OVER")));
            }
            self.serve();
        }
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        render(&self.world, g);
        g.text(&format!("SCORE {}", self.score), 20, 10, 2, 0xaaaaaa);
        g.text(&format!("LIVES {}", self.lives), 500, 10, 2, 0xaaaaaa);
    }
}

fn main() {
    engine::run(Config { title: "breakout", ..Config::default() }, |ctx| Box::new(Breakout::new(ctx)));
}
