use engine::{Body, Config, Ctx, Gfx, Key, Look, Rect, Scene, Tilemap, Trans, World, draw_list};

const SPEED: f32 = 120.0;

struct Fly {
    map: Tilemap,
    world: World,
    me: engine::Entity,
    zoom: f32,
}

impl Fly {
    fn new(ctx: &mut Ctx) -> Self {
        let map = Tilemap::load("assets/levels/level1.txt", 16, ctx.assets().sprite("assets/images/tiles.png"))
            .expect("level1");
        let engine::Vec2 { x, y } = map.marks(b'P').next().expect("level1 has a P");
        let mut world = World::new();
        let me = world.spawn((Body(Rect { x, y, w: 12.0, h: 12.0 }), Look::Color(0xffd040)));
        ctx.cam().reset();
        Fly { map, world, me, zoom: 1.0 }
    }
}

impl Scene for Fly {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input().pressed("back") {
            return Trans::Quit;
        }
        if ctx.input().pressed("restart") {
            return Trans::Fade(Box::new(Trans::Replace(Box::new(Fly::new(ctx)))), 0.8);
        }
        let dt = ctx.dt();
        let (dx, dy) = (ctx.input().axis("left", "right") * SPEED * dt, ctx.input().axis("up", "down") * SPEED * dt);
        let mut body = self.world.get::<&mut Body>(self.me).unwrap();
        body.0 = self.map.slide(body.0, dx, dy).0;
        let rect = body.0;
        drop(body);
        let step = ctx.input().pressed("zoom_in") as i32 - ctx.input().pressed("zoom_out") as i32;
        if step != 0 {
            self.zoom = (self.zoom * 1.5f32.powi(step)).clamp(0.5, 6.0);
            ctx.cam().zoom_to(self.zoom, 0.3);
        }
        if ctx.input().pressed("shake") {
            ctx.cam().shake(4.0, 0.4);
        }
        ctx.cam().follow(rect, self.map.pixel_size(), dt);
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        let mut cmds = self.map.draw_list(g.view());
        cmds.extend(draw_list(&self.world));
        g.draw(cmds);
        g.text("arrows: move   Z/X: zoom   SPACE: shake   ENTER: restart with a fade", 16, 16, 1, 0xaaaaaa);
    }
}

fn main() {
    engine::run(Config { title: "camera", scale: 2, ..Config::default() }, |ctx| {
        ctx.bind("left", &[Key::Left]);
        ctx.bind("right", &[Key::Right]);
        ctx.bind("up", &[Key::Up]);
        ctx.bind("down", &[Key::Down]);
        ctx.bind("shake", &[Key::Space]);
        ctx.bind("zoom_in", &[Key::Z]);
        ctx.bind("zoom_out", &[Key::X]);
        ctx.bind("restart", &[Key::Enter]);
        ctx.bind("back", &[Key::Escape]);
        Box::new(Fly::new(ctx))
    });
}
