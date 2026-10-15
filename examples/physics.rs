use engine::{
    Body, Config, Ctx, Entity, Gfx, Key, Look, MouseButton, Physics, Rect, RigidBody, Scene, Trans, Transform, Vel,
    World, draw_list,
};

const W: f32 = 640.0;
const H: f32 = 360.0;

struct Sandbox {
    world: World,
    physics: Physics,
}

impl Sandbox {
    fn new(ctx: &mut Ctx) -> Self {
        let mut world = World::new();
        let wall = |x, y, w, h| (Body(Rect { x, y, w, h }), Look::Color(0x4a4a5a), RigidBody::fixed());
        world.spawn(wall(0.0, H - 20.0, W, 20.0));
        world.spawn(wall(0.0, 0.0, 10.0, H));
        world.spawn(wall(W - 10.0, 0.0, 10.0, H));
        let mut sandbox = Sandbox { world, physics: Physics::default() };
        for i in 0..36 {
            sandbox.drop_box(ctx, -20.0 * i as f32);
        }
        for i in 0..12 {
            let x = ctx.rng().range(20.0, W - 36.0);
            sandbox.drop_ball(ctx, x, -40.0 * i as f32);
        }
        sandbox
    }

    fn drop_box(&mut self, ctx: &mut Ctx, y: f32) -> Entity {
        let rng = ctx.rng();
        let size = rng.range(14.0, 34.0);
        let bounce = rng.range(0.0, 0.7);
        let shade = (80.0 + bounce * 250.0) as u32;
        let color = (shade + 40) << 16 | (200 - shade / 2) << 8 | 120;
        let rect = Rect { x: rng.range(20.0, W - 20.0 - size), y, w: size, h: size };
        let spin = rng.range(-4.0, 4.0);
        let angle = rng.range(0.0, 3.0);
        let body = RigidBody::dynamic(size * size / 100.0).bounce(bounce).friction(0.5).spin(spin);
        let turn = Transform { angle, ..Transform::IDENTITY };
        self.world.spawn((Body(rect), Vel::default(), Look::Color(color), body, turn))
    }
}

impl Sandbox {
    fn drop_ball(&mut self, ctx: &mut Ctx, x: f32, y: f32) -> Entity {
        let sprite = ctx.assets().sprite("assets/images/ball.png");
        let body = RigidBody::dynamic(2.5).bounce(0.7).friction(0.2).circle();
        let rect = Rect { x, y, w: 16.0, h: 16.0 };
        self.world.spawn((Body(rect), Vel::default(), Look::Sprite(sprite), body, Transform::IDENTITY))
    }
}

impl Scene for Sandbox {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input().pressed("back") {
            return Trans::Quit;
        }
        if ctx.input().pressed("drop") {
            let x = ctx.rng().range(20.0, W - 36.0);
            self.drop_ball(ctx, x, 0.0);
        }
        if let Some(engine::Vec2 { x, y }) =
            ctx.input().mouse().filter(|_| ctx.input().mouse_pressed(MouseButton::Left))
        {
            self.drop_ball(ctx, x - 8.0, y - 8.0);
        }
        self.physics.step(&mut self.world, None, ctx.dt());
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        g.draw(draw_list(&self.world));
        g.text("SPACE or click: drop a ball   brighter boxes bounce more", 16, 16, 1, 0xaaaaaa);
    }
}

fn main() {
    engine::run(Config { title: "physics", scale: 2, ..Config::default() }, |ctx| {
        ctx.bind("drop", &[Key::Space]);
        ctx.bind("back", &[Key::Escape]);
        if let Err(e) = ctx.load_bindings("input.txt") {
            eprintln!("{e}");
        }
        Box::new(Sandbox::new(ctx))
    });
}
