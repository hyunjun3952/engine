use engine::{Anim, Body, Config, Ctx, Entity, Gfx, Key, Rect, Scene, Tilemap, Trans, Vel, World, animate, render};

const LEVEL: [&str; 23] = [
    "#..........................................................#",
    "#..........................................................#",
    "#..........................................................#",
    "#..........................................................#",
    "#..........................................................#",
    "#..........................................................#",
    "#..........................................................#",
    "#..........................................................#",
    "#.........................++...............................#",
    "#..........................................................#",
    "#..........................................................#",
    "#.................................................++++++...#",
    "#...............................++++.......................#",
    "#.............+++..........................................#",
    "#..........................................................#",
    "#..........................................................#",
    "#.............................................++++++.......#",
    "#.....++++..............++++++.............................#",
    "#...................................##.....................#",
    "#..........................................................#",
    "#.........###..............................................#",
    "##################....##################....################",
    "##################....##################....################",
];

const TILE: usize = 16;
const START: Rect = Rect { x: 32.0, y: 21.0 * 16.0 - 24.0, w: 12.0, h: 24.0 };

struct Platformer {
    world: World,
    player: Entity,
    map: Tilemap,
    grounded: bool,
}

impl Platformer {
    fn new(ctx: &mut Ctx) -> Self {
        let mut world = World::new();
        let player = world.spawn((
            Body(START),
            Vel { x: 0.0, y: 0.0 },
            Anim::new(ctx.assets.sprite("assets/player.png"), 12, 24),
        ));
        ctx.audio.music("assets/music.wav");
        let map = Tilemap::from_ascii(&LEVEL, TILE, ctx.assets.sprite("assets/tiles.png"));
        Platformer { world, player, map, grounded: false }
    }
}

impl Scene for Platformer {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input.pressed(Key::Escape) {
            return Trans::Quit;
        }

        let (b, v, anim) = self.world.query_one_mut::<(&mut Body, &mut Vel, &mut Anim)>(self.player).unwrap();
        v.x = ctx.input.axis(Key::Left, Key::Right) * 160.0;
        v.y = (v.y + 900.0 * ctx.dt).min(600.0);
        if self.grounded && (ctx.input.pressed(Key::Space) || ctx.input.pressed(Key::Up)) {
            v.y = -330.0;
            ctx.audio.play("assets/jump.wav");
        }

        let (r, _, hit_y) = self.map.slide(&b.0, v.x * ctx.dt, v.y * ctx.dt);
        b.0 = r;
        self.grounded = hit_y && v.y > 0.0;
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
            b.0 = START;
            v.y = 0.0;
        }
        animate(&mut self.world, ctx.dt);
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        let player = self.world.get::<&Body>(self.player).unwrap().0;
        g.follow(&player, self.map.pixel_size());
        g.clear(0x6fa8e0);
        self.map.draw(g);
        render(&self.world, g);
        g.text("ARROWS: move   SPACE: jump   F3: debug", 8, 340, 1, 0xffffff);
    }
}

fn main() {
    engine::run(Config { title: "platformer", ..Config::default() }, |ctx| Box::new(Platformer::new(ctx)));
}
