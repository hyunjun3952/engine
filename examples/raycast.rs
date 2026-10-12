use engine::{
    Body, Collider, Config, Ctx, Filter, Gfx, Key, Look, Rect, Scene, Trans, Transform, World, draw_list, raycast,
};

const W: f32 = 640.0;
const H: f32 = 360.0;
const FROM: (f32, f32) = (W / 2.0, H / 2.0);

struct Rays {
    world: World,
    mask: u32,
    hit: Option<f32>,
    dir: (f32, f32),
}

impl Rays {
    fn new() -> Self {
        let mut world = World::new();
        // Group 1 is blue and group 2 is orange; the turned boxes are hit where they are drawn.
        for (i, &(x, y, angle, group)) in
            [(80.0, 60.0, 0.4, 1), (500.0, 80.0, 0.0, 2), (120.0, 260.0, 0.9, 2), (460.0, 250.0, 0.2, 1)]
                .iter()
                .enumerate()
        {
            let color = if group == 1 { 0x4a6aff } else { 0xff8a3a };
            let rect = Rect { x, y, w: 50.0 + 10.0 * i as f32, h: 30.0 };
            let turn = Transform { angle, ..Transform::IDENTITY };
            world.spawn((Body(rect), Collider, Look::Color(color), turn, Filter { group, mask: u32::MAX }));
        }
        Rays { world, mask: u32::MAX, hit: None, dir: (1.0, 0.0) }
    }
}

impl Scene for Rays {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input().pressed("back") {
            return Trans::Quit;
        }
        if ctx.input().pressed("mask") {
            self.mask = match self.mask {
                u32::MAX => 1,
                1 => 2,
                _ => u32::MAX,
            };
        }
        if let Some((x, y)) = ctx.input().mouse() {
            self.dir = (x - FROM.0, y - FROM.1);
        }
        self.hit = raycast(&self.world, FROM, self.dir, 1000.0, self.mask).map(|(_, t)| t);
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        g.draw(draw_list(&self.world));
        let len = self.dir.0.hypot(self.dir.1).max(1.0);
        let end = self.hit.unwrap_or(f32::MAX).min(1000.0 / len) * len;
        for i in 0..(end / 4.0) as i32 {
            let t = i as f32 * 4.0 / len;
            let dot = Rect { x: FROM.0 + self.dir.0 * t - 1.0, y: FROM.1 + self.dir.1 * t - 1.0, w: 2.0, h: 2.0 };
            g.fill(&dot, 0xffffff);
        }
        let name = match self.mask {
            u32::MAX => "all groups",
            1 => "blue only",
            _ => "orange only",
        };
        g.text(&format!("mouse aims   SPACE: mask ({name})"), 16, 16, 1, 0xaaaaaa);
    }
}

fn main() {
    engine::run(Config { title: "raycast", scale: 2, ..Config::default() }, |ctx| {
        ctx.bind("mask", &[Key::Space]);
        ctx.bind("back", &[Key::Escape]);
        Box::new(Rays::new())
    });
}
