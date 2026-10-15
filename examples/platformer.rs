use engine::{
    Anim, AnimStates, Body, Clip, Command, Config, Console, Ctx, DrawCmd, Entity, Filter, Gfx, Inspector, Key, Light,
    LightMap, Look, Paint, Physics, Rect, RigidBody, Scene, Slope, Sprite, Style, Tile, Tilemap, Timer, Trans, Vel,
    World, draw_list, vignette,
};

use std::{cell::RefCell, sync::Arc};

const DROP_GROUP: u32 = 2;

struct Platformer {
    world: World,
    physics: Physics,
    map: Tilemap,
    player: Entity,
    mover: Entity,
    flash: Option<Timer>,
    drop: Option<Timer>,
    console: Console,
    inspector: Inspector,
    night: bool,
    lights: RefCell<LightMap>,
    vignette: Arc<Sprite>,
}

impl Platformer {
    fn new(ctx: &mut Ctx) -> Self {
        let sheet = ctx.assets().sprite("assets/images/platformer_tiles.png");
        let tile = |ch, id, one_way, slope| Tile { ch, id, solid: false, front: false, auto: false, one_way, slope };
        let legend = [
            Tile::DEFAULT[0],
            tile(b'=', 2, true, None),
            tile(b'/', 3, false, Some(Slope::BottomRight)),
            tile(b'\\', 4, false, Some(Slope::BottomLeft)),
            Tile { ch: b'%', id: 5, ..Tile::DEFAULT[0] },
        ];
        let map = Tilemap::load_with("assets/levels/platformer.txt", 16, sheet, &legend).expect("platformer level");
        let mut world = World::new();
        let slab = |x, y, w, color| (Body(Rect { x, y, w, h: 8.0 }), Look::Color(color));
        let (body, look) = slab(200.0, 300.0, 56.0, 0x7a9a4a);
        let mover = world.spawn((body, look, RigidBody::kinematic(), Vel { x: 40.0, y: 0.0 }));
        for (x, y) in [(60.0, 300.0), (120.0, 250.0), (60.0, 200.0)] {
            let (body, look) = slab(x, y, 64.0, 0x4a7a9a);
            let filter = Filter { group: DROP_GROUP, mask: u32::MAX };
            world.spawn((body, look, RigidBody::fixed().one_way(), filter));
        }
        let sheet = ctx.assets().sprite("assets/images/player.png");
        let states = AnimStates::new()
            .state("idle", Clip::new(0, 2, 6.0))
            .state("run", Clip::new(1, 4, 12.0).event(0, "step").event(2, "step"))
            .state("air", Clip::new(2, 1, 1.0));
        let rect = Rect::new(120.0, 100.0, 12.0, 24.0);
        let player = world.spawn((
            Body(rect),
            Vel::default(),
            RigidBody::dynamic(1.0).friction(0.0),
            Anim::new(sheet, 12, 24),
            states,
        ));
        Platformer {
            world,
            physics: Physics::default(),
            map,
            player,
            mover,
            flash: None,
            drop: None,
            console: Console::new(),
            inspector: Inspector::new(),
            night: false,
            lights: RefCell::new(LightMap::new(640, 360)),
            vignette: vignette(640, 360, 0.7),
        }
    }

    fn command(&mut self, cmd: Command) {
        match cmd.name.as_str() {
            "tp" => match (cmd.arg::<f32>(0), cmd.arg::<f32>(1)) {
                (Some(x), Some(y)) => {
                    let mut body = self.world.get::<&mut Body>(self.player).unwrap();
                    (body.0.x, body.0.y) = (x, y);
                    self.console.print(format!("moved to {x} {y}"));
                }
                _ => self.console.print("usage: tp x y"),
            },
            "flash" => self.flash = Some(Timer::once(0.3)),
            "gravity" => {
                self.physics.gravity.y = cmd.arg(0).unwrap_or(900.0);
                self.console.print(format!("gravity is {}", self.physics.gravity.y));
            }
            "help" => self.console.print("tp x y | flash | gravity g"),
            other => self.console.print(format!("unknown command {other:?}, try help")),
        }
    }
}

impl Scene for Platformer {
    fn on_enter(&mut self, ctx: &mut Ctx) {
        ctx.cam().reset();
    }

    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        let dt = ctx.dt();
        if let Some(cmd) = self.console.update(ctx.input()) {
            self.command(cmd);
        }
        self.inspector.update(ctx, &self.world);
        let typing = self.console.is_open();
        if !typing && ctx.input().pressed("back") {
            return Trans::Quit;
        }
        if !typing && ctx.input().pressed("night") {
            self.night = !self.night;
        }
        // The moving platform turns round at either end.
        {
            let (x, mut vel) =
                (self.world.get::<&Body>(self.mover).unwrap().0.x, self.world.get::<&mut Vel>(self.mover).unwrap());
            vel.x = if x > 420.0 {
                -40.0
            } else if x < 200.0 {
                40.0
            } else {
                vel.x
            };
        }
        let grounded = self.physics.grounded(self.player);
        let (walk, jump) = (if typing { 0.0 } else { ctx.input().axis("left", "right") }, ctx.input().pressed("jump"));
        if !typing {
            // Standing on a moving platform, walking is relative to it.
            let under = self.physics.ground(self.player).and_then(|c| c.other(self.player));
            let carried = under.and_then(|e| self.world.get::<&Vel>(e).ok().map(|v| v.x)).unwrap_or(0.0);
            let mut vel = self.world.get::<&mut Vel>(self.player).unwrap();
            vel.x = walk * 120.0 + carried;
            if jump && grounded {
                vel.y = -330.0;
            } else if grounded {
                // Walking up a slope turns sideways speed into upward speed that would launch the player at the crest.
                vel.y = vel.y.max(0.0);
            }
            if ctx.input().pressed("down") {
                self.drop = Some(Timer::once(0.25));
            }
            if ctx.input().pressed("flash") {
                self.flash = Some(Timer::once(0.15));
            }
        }
        // Down drops through the blue platforms and the plank tiles for a moment by leaving their groups out of the mask.
        let dropping = self.drop.as_mut().is_some_and(|t| !t.tick(dt) && !t.done());
        if !dropping {
            self.drop = None;
        }
        let mask = if dropping { !(DROP_GROUP | Filter::PLATFORM) } else { u32::MAX };
        self.world.insert_one(self.player, Filter { group: u32::MAX, mask }).unwrap();
        let flashing = self.flash.as_mut().is_some_and(|t| !t.tick(dt) && !t.done());
        if !flashing {
            self.flash = None;
        }
        let style = if flashing { Style::PLAIN.flashed(0xffffff, 220) } else { Style::PLAIN };
        self.world.insert_one(self.player, style).unwrap();
        self.physics.step(&mut self.world, Some(&self.map), dt);
        let mut anim = self.world.get::<&mut Anim>(self.player).unwrap();
        anim.flip = if walk != 0.0 { walk < 0.0 } else { anim.flip };
        let state = if !self.physics.grounded(self.player) {
            "air"
        } else if walk != 0.0 {
            "run"
        } else {
            "idle"
        };
        self.world.get::<&mut AnimStates>(self.player).unwrap().set(state);
        drop(anim);
        engine::animate(&mut self.world, dt);
        let target = self.world.get::<&Body>(self.player).unwrap().0;
        ctx.cam().follow(target, self.map.pixel_size(), dt);
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x6fa8e0);
        let hills = (0..12).map(|i| {
            let (x, h) = (i as f32 * 140.0, 60.0 + (i * 37 % 5) as f32 * 14.0);
            DrawCmd::new(Rect { x, y: 340.0 - h, w: 160.0, h }, Paint::Color(0x4f8f5f))
        });
        g.draw_parallax(hills, (0.4, 0.0));
        g.draw(self.map.draw_list(g.view()));
        g.draw(draw_list(&self.world));
        if self.night {
            // The torch is in the world, so it follows the camera; the lights go on after everything they light.
            let player = self.world.get::<&Body>(self.player).unwrap().0.center();
            let mut lights = self.lights.borrow_mut();
            lights.clear(0x141c38);
            lights.add_world(g, Light::new((player.x, player.y), 110.0).colored(0xffc070));
            lights.draw(g);
            g.multiply(&self.vignette);
        }
        self.inspector.draw(g, &self.world);
        g.text(
            "arrows: walk   space: jump   down: drop through planks   N: night   F: flash   F4: inspector   `: console",
            8,
            8,
            1,
            0xffffff,
        );
    }
}

fn main() {
    engine::run(Config { title: "platformer", scale: 2, ..Config::default() }, |ctx| {
        ctx.bind("left", &[Key::Left, Key::A]);
        ctx.bind("right", &[Key::Right, Key::D]);
        ctx.bind("jump", &[Key::Space, Key::Up, Key::W]);
        ctx.bind("down", &[Key::Down, Key::S]);
        ctx.bind("flash", &[Key::F]);
        ctx.bind("night", &[Key::N]);
        ctx.bind("back", &[Key::Escape]);
        Box::new(Platformer::new(ctx))
    });
}
