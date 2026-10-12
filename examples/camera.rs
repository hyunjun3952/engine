use engine::{Body, Config, Ctx, Gfx, Key, Look, Rect, Scene, Tilemap, Trans, World, draw_list};

const SPEED: f32 = 120.0;

struct Fly {
    map: Tilemap,
    world: World,
    me: engine::Entity,
}

impl Fly {
    fn new(ctx: &mut Ctx) -> Self {
        let map = Tilemap::load("assets/levels/level1.txt", 16, ctx.assets().sprite("assets/images/tiles.png"));
        let (x, y) = map.marks(b'P').next().expect("level1 has a P");
        let mut world = World::new();
        let me = world.spawn((Body(Rect { x, y, w: 12.0, h: 12.0 }), Look::Color(0xffd040)));
        ctx.cam().reset();
        Fly { map, world, me }
    }
}

impl Scene for Fly {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input().pressed("back") {
            return Trans::Quit;
        }
        let dt = ctx.dt();
        let (dx, dy) = (ctx.input().axis("left", "right") * SPEED * dt, ctx.input().axis("up", "down") * SPEED * dt);
        let mut body = self.world.get::<&mut Body>(self.me).unwrap();
        body.0 = self.map.slide(&body.0, dx, dy).0;
        let rect = body.0;
        drop(body);
        if ctx.input().pressed("shake") {
            ctx.cam().shake(4.0, 0.4);
        }
        ctx.cam().follow(&rect, self.map.pixel_size(), dt);
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        let mut cmds = self.map.draw_list(g.view());
        cmds.extend(draw_list(&self.world));
        g.draw(cmds);
        g.text("arrows: move   SPACE: shake", 16, 16, 1, 0xaaaaaa);
    }
}

fn main() {
    engine::run(Config { title: "camera", scale: 2, ..Config::default() }, |ctx| {
        ctx.bind("left", &[Key::Left]);
        ctx.bind("right", &[Key::Right]);
        ctx.bind("up", &[Key::Up]);
        ctx.bind("down", &[Key::Down]);
        ctx.bind("shake", &[Key::Space]);
        ctx.bind("back", &[Key::Escape]);
        Box::new(Fly::new(ctx))
    });
}
