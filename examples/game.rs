use engine::{Config, Key, Save, Scene, data_dir};

mod level {
    pub const W: usize = 80;
    pub const H: usize = 23;
    const GROUND: usize = 21;

    pub struct Info {
        pub name: &'static str,
        pub intro: &'static str,
        pub dark: bool,
    }

    const fn info(name: &'static str, intro: &'static str) -> Info {
        Info { name, intro, dark: false }
    }

    pub const STAGES: [Info; 8] = [
        Info {
            name: "달리기",
            intro: "방향키로 걷고 Shift로 달리고 스페이스로 뛰세요. 타일맵, 물리, 카메라 추적, 시차 배경",
            dark: false,
        },
        info("구르는 동전", "애니메이션 상태, 파티클, 트리거 존, 번쩍임. 동전을 모으세요"),
        info("언덕과 발판", "경사 타일, 일방통행 발판, 움직이는 발판. 아래 키로 발판을 통과합니다"),
        info("상자와 밧줄", "물리 상자, 공, 밧줄, 스프링. 상자를 벽까지 밀어 올라가세요"),
        info("경비병", "레이캐스트 시야. 상자 뒤에 숨으면 경비병이 보지 못합니다"),
        info("유령", "A* 길찾기. 유령은 벽을 돌아 쫓아옵니다"),
        Info {
            name: "어둠", intro: "빛 맵, 비네트, 스캔라인, 화면 흔들림. 횃불을 따라가세요", dark: true
        },
        info("마무리", "지금까지 나온 모든 것. 끝나면 이름을 남깁니다"),
    ];

    struct Grid(Vec<Vec<u8>>);

    impl Grid {
        fn new() -> Self {
            let mut g = Grid(vec![vec![b'.'; W]; H]);
            g.fill(0, GROUND, W, H - GROUND, b'#');
            g.fill(0, 0, 1, H, b'#');
            g.fill(W - 1, 0, 1, H, b'#');
            g
        }

        fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, c: u8) {
            for row in &mut self.0[y..y + h] {
                row[x..x + w].fill(c);
            }
        }

        fn put(&mut self, x: usize, y: usize, s: &str) {
            self.0[y][x..x + s.len()].copy_from_slice(s.as_bytes());
        }

        fn gap(&mut self, x: usize, w: usize) {
            self.fill(x, GROUND, w, H - GROUND, b'.');
        }

        fn block(&mut self, x: usize, w: usize, h: usize) {
            self.fill(x, GROUND - h, w, h, b'#');
        }

        fn hill(&mut self, x: usize, h: usize, flat: usize) {
            let base = GROUND - 1;
            for i in 0..h {
                self.put(x + i, base - i, "/");
                self.fill(x + i, base - i + 1, 1, i, b'%');
                let (r, top) = (x + h + flat + i, base - (h - 1 - i));
                self.put(r, top, "\\");
                self.fill(r, top + 1, 1, h - 1 - i, b'%');
            }
            self.fill(x + h, base - h + 1, flat, h, b'#');
        }

        fn rows(&self) -> Vec<String> {
            self.0.iter().map(|r| String::from_utf8(r.clone()).unwrap()).collect()
        }
    }

    pub fn rows(stage: usize) -> Vec<String> {
        let mut g = Grid::new();
        g.put(3, GROUND - 1, "P");
        g.put(76, GROUND - 1, "G");
        match stage {
            0 => {
                g.block(14, 3, 2);
                g.block(24, 3, 3);
                g.gap(34, 4);
                g.block(44, 2, 2);
                g.block(50, 2, 1);
                g.block(53, 2, 2);
                g.block(56, 2, 3);
                g.gap(62, 4);
                g.block(68, 3, 2);
            }
            1 => {
                g.block(16, 3, 2);
                g.gap(30, 4);
                g.block(40, 2, 3);
                g.gap(52, 5);
                g.block(66, 3, 2);
                for x in [10, 12, 14, 18, 20, 28, 31, 32, 42, 54, 55, 56, 70, 72] {
                    g.put(x, 18, "C");
                }
                for x in [24, 26, 36, 38, 46, 48, 60, 62] {
                    g.put(x, GROUND - 1, "C");
                }
            }
            2 => {
                g.hill(10, 4, 4);
                g.put(30, 18, "====");
                g.put(36, 15, "====");
                g.put(42, 18, "====");
                g.gap(48, 12);
                g.put(52, GROUND - 1, "M");
                g.hill(62, 3, 2);
                for (x, y) in [(31, 17), (37, 14), (43, 17), (17, 14), (54, 17)] {
                    g.put(x, y, "C");
                }
            }
            3 => {
                g.put(30, GROUND - 1, "X");
                g.block(38, 3, 4);
                g.put(18, GROUND - 1, "O");
                g.put(24, GROUND - 1, "O");
                g.put(52, 6, "R");
                g.put(66, 8, "S");
                g.gap(58, 3);
                for x in [14, 46, 47, 70] {
                    g.put(x, GROUND - 1, "C");
                }
            }
            4 => {
                g.put(26, GROUND - 1, "X");
                g.put(27, GROUND - 1, "X");
                g.put(34, GROUND - 1, "E");
                g.block(44, 2, 3);
                g.put(58, GROUND - 1, "E");
                g.put(66, GROUND - 1, "X");
                for x in [20, 40, 52, 62] {
                    g.put(x, GROUND - 1, "C");
                }
            }
            5 => {
                for x in [20, 34, 48, 62] {
                    g.fill(x, 0, 2, 18, b'#');
                }
                g.block(27, 2, 2);
                g.block(55, 2, 2);
                g.put(14, 6, "Z");
                g.put(42, 6, "Z");
                for x in [24, 38, 52, 66] {
                    g.put(x, GROUND - 1, "C");
                }
            }
            6 => {
                g.hill(12, 3, 3);
                g.gap(34, 4);
                g.put(30, 17, "===");
                g.put(41, 17, "===");
                g.block(48, 3, 2);
                g.gap(58, 5);
                for x in [8, 24, 44, 56, 70] {
                    g.put(x, GROUND - 1, "T");
                }
                for x in [22, 31, 42, 52, 66] {
                    g.put(x, GROUND - 1, "C");
                }
            }
            _ => {
                g.hill(8, 3, 2);
                g.put(24, GROUND - 1, "E");
                g.put(32, GROUND - 1, "X");
                g.block(36, 3, 4);
                g.put(42, 17, "====");
                g.gap(48, 12);
                g.put(52, GROUND - 1, "M");
                g.put(60, 6, "Z");
                for x in [18, 28, 44, 53, 64, 72] {
                    g.put(x, GROUND - 1, "C");
                }
            }
        }
        g.rows()
    }

    pub fn total_coins() -> usize {
        (0..STAGES.len()).map(|n| rows(n).concat().matches('C').count()).sum()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn every_level_is_rectangular_with_a_start_and_a_goal() {
            for n in 0..STAGES.len() {
                let rows = rows(n);
                assert_eq!(rows.len(), H);
                assert!(rows.iter().all(|r| r.len() == W), "stage {n}");
                let all = rows.concat();
                assert_eq!(all.matches('P').count(), 1, "stage {n}");
                assert_eq!(all.matches('G').count(), 1, "stage {n}");
            }
        }

        #[test]
        fn no_coin_is_sealed_inside_the_terrain() {
            for n in 0..STAGES.len() {
                let rows = rows(n);
                for (y, row) in rows.iter().enumerate() {
                    for (x, _) in row.match_indices('C') {
                        let around = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)];
                        let shut = around.iter().all(|&(a, b)| b"#%/\\".contains(&rows[b].as_bytes()[a]));
                        assert!(!shut, "stage {n}: the coin at ({x}, {y}) cannot be reached");
                    }
                }
            }
        }
    }
}

mod stage {
    use super::ending::Ending;
    use super::level::{self, STAGES};
    use super::pause::Pause;
    use super::title::Title;
    use engine::{
        Anim, AnimStates, Body, Burst, Clip, Collider, Command, Console, Ctx, DrawCmd, Entity, Filter, Font, Gfx,
        Inspector, Layer, Light, LightMap, Look, Overlaps, Paint, Physics, Rect, RigidBody, Scene, Slope, Sprite,
        Style, Tile, Tilemap, Timer, Trans, Transform, Vec2, Vel, World, animate, burst, collisions, draw_list,
        draw_panel, draw_wrapped, particles, raycast, scanlines, vignette,
    };
    use std::{cell::Cell, cell::RefCell, rc::Rc, sync::Arc};

    const PLAYER: u32 = 1;
    const ENEMY: u32 = 2;
    const CRATE: u32 = 4;
    const ITEM: u32 = 8;
    const SIGHT: f32 = 150.0;
    const GHOST_SPEED: f32 = 50.0;
    const SIZE: (usize, usize) = (640, 360);

    #[derive(Debug)]
    struct Coin;
    #[derive(Debug)]
    struct Goal;
    #[derive(Debug)]
    struct Hazard;
    #[derive(Debug)]
    struct Guard;
    #[derive(Debug)]
    struct Patrol {
        home: f32,
        range: f32,
        speed: f32,
    }
    struct Chaser {
        path: Vec<(usize, usize)>,
        replan: Timer,
    }

    fn tile(ch: u8, id: u8, solid: bool, one_way: bool, slope: Option<Slope>) -> Tile {
        Tile { ch, id, solid, front: false, auto: false, one_way, slope }
    }

    fn legend() -> [Tile; 5] {
        [
            tile(b'#', 1, true, false, None),
            tile(b'=', 2, false, true, None),
            tile(b'/', 3, false, false, Some(Slope::BottomRight)),
            tile(b'\\', 4, false, false, Some(Slope::BottomLeft)),
            tile(b'%', 5, true, false, None),
        ]
    }

    fn beam(a: Vec2, b: Vec2, color: u32) -> DrawCmd {
        let (d, mid) = (b - a, (a + b) * 0.5);
        let rect = Rect::new(mid.x - d.length() / 2.0, mid.y - 0.5, d.length(), 1.0);
        DrawCmd {
            layer: 8,
            xform: Transform { angle: d.angle(), ..Transform::IDENTITY },
            ..DrawCmd::new(rect, Paint::Color(color))
        }
    }

    const TUNING: &str = "assets/tuning.txt";

    struct Tuning {
        walk: f32,
        run: f32,
        jump: f32,
    }

    impl Tuning {
        fn load() -> Self {
            let save = engine::Save::load(TUNING).unwrap_or_else(|e| {
                eprintln!("{e}");
                engine::Save::new()
            });
            let get = |key, default: f32| Some(save.get_or(key, default)).filter(|v| v.is_finite()).unwrap_or(default);
            Tuning { walk: get("walk", 120.0), run: get("run", 160.0), jump: get("jump", 330.0) }
        }
    }

    pub struct Stage {
        n: usize,
        coins: u32,
        picked: u32,
        total: usize,
        world: World,
        physics: Physics,
        map: Tilemap,
        player: Entity,
        start: Rect,
        font: Arc<Font>,
        overlaps: Overlaps,
        flash: Option<Timer>,
        drop: Option<Timer>,
        banner: Timer,
        tuning: Tuning,
        console: Console,
        inspector: Inspector,
        ropes: Vec<(Entity, Entity)>,
        sights: Vec<(Vec2, Vec2)>,
        torches: Vec<Vec2>,
        lights: RefCell<LightMap>,
        vignette: Arc<Sprite>,
        scan: Arc<Sprite>,
        quit: Rc<Cell<bool>>,
    }

    impl Stage {
        pub fn new(ctx: &mut Ctx, n: usize, coins: u32) -> Self {
            ctx.watch(TUNING);
            let sheet = ctx.assets().sprite("assets/images/platformer_tiles.png");
            let rows = level::rows(n);
            let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
            let map = Tilemap::try_from_ascii_with(&rows, 16, sheet, &legend()).expect("stage level");
            let mut world = World::new();
            // Things stand on the floor of the marker's cell.
            let at = |p: Vec2, w: f32, h: f32| Rect::new(p.x + (16.0 - w) / 2.0, p.y + 16.0 - h, w, h);
            let anywhere = Filter { group: CRATE, mask: u32::MAX };
            let only_player = |group| Filter { group, mask: PLAYER };

            let spawn = map.marks(b'P').next().expect("a P in every level");
            let start = at(spawn, 12.0, 24.0);
            let player = world.spawn((
                Body(start),
                Vel::default(),
                RigidBody::dynamic(1.0).friction(0.0),
                Collider,
                Filter { group: PLAYER, mask: u32::MAX },
                Layer(5),
            ));
            let sheet = ctx.assets().sprite("assets/images/player.png");
            let states = AnimStates::new()
                .state("idle", Clip::new(0, 2, 6.0))
                .state("run", Clip::new(1, 4, 12.0))
                .state("air", Clip::new(2, 1, 1.0));
            world.insert(player, (Anim::new(sheet, 12, 24), states)).unwrap();

            for p in map.marks(b'C') {
                let rect = Rect::new(p.x + 4.0, p.y + 4.0, 8.0, 8.0);
                world.spawn((Body(rect), Look::Color(0xffd040), Collider, Coin, only_player(ITEM), Layer(2)));
            }
            if let Some(p) = map.marks(b'G').next() {
                world.spawn((
                    Body(at(p, 12.0, 32.0)),
                    Look::Color(0x60e060),
                    Collider,
                    Goal,
                    only_player(ITEM),
                    Layer(1),
                ));
            }
            for p in map.marks(b'M') {
                let rect = Rect::new(p.x + 8.0, p.y + 16.0, 48.0, 8.0);
                let patrol = Patrol { home: rect.x, range: 56.0, speed: 40.0 };
                let vel = Vel { x: patrol.speed, y: 0.0 };
                world.spawn((Body(rect), Look::Color(0x7a9a4a), RigidBody::kinematic(), vel, patrol, Layer(1)));
            }
            for p in map.marks(b'X') {
                let body = RigidBody::dynamic(0.6).friction(0.6);
                world.spawn((Body(at(p, 14.0, 14.0)), Look::Color(0xa0703a), Collider, anywhere, Layer(3), body));
            }
            let ball = ctx.assets().sprite("assets/images/ball.png");
            for p in map.marks(b'O') {
                let body = RigidBody::dynamic(0.4).circle().bounce(0.6).friction(0.1);
                world.spawn((Body(at(p, 16.0, 16.0)), Look::Sprite(ball.clone()), Collider, anywhere, Layer(3), body));
            }
            let mut physics = Physics::default();
            let mut ropes = Vec::new();
            let anchor = |world: &mut World, p: Vec2| {
                let rect = Rect::new(p.x + 6.0, p.y + 6.0, 4.0, 4.0);
                world.spawn((Body(rect), Look::Color(0x888888), RigidBody::fixed(), Layer(3)))
            };
            for p in map.marks(b'R') {
                let top = anchor(&mut world, p);
                let rect = Rect::new(p.x + 56.0, p.y, 16.0, 16.0);
                let body = RigidBody::dynamic(1.0).circle().bounce(0.3);
                let hanging = world.spawn((Body(rect), Look::Sprite(ball.clone()), Collider, anywhere, Layer(3), body));
                physics.rope(top, hanging, 56.0);
                ropes.push((top, hanging));
            }
            for p in map.marks(b'S') {
                let top = anchor(&mut world, p);
                let rect = Rect::new(p.x, p.y + 56.0, 16.0, 16.0);
                let body = RigidBody::dynamic(1.0).friction(0.6);
                let hanging = world.spawn((Body(rect), Look::Color(0xa0703a), Collider, anywhere, Layer(3), body));
                physics.spring(top, hanging, 40.0, 1.2, 0.15);
                ropes.push((top, hanging));
            }
            for p in map.marks(b'E') {
                let patrol = Patrol { home: p.x, range: 48.0, speed: 40.0 };
                let (rect, filter) = (at(p, 14.0, 20.0), only_player(ENEMY));
                let look = (Look::Color(0xd04040), Collider, Hazard, Guard, Layer(4));
                world.spawn((
                    Body(rect),
                    look.0,
                    look.1,
                    look.2,
                    look.3,
                    look.4,
                    RigidBody::kinematic(),
                    Vel::default(),
                    patrol,
                    filter,
                ));
            }
            for p in map.marks(b'Z') {
                let rect = Rect::new(p.x + 1.0, p.y + 1.0, 14.0, 14.0);
                let ghost = world.spawn((
                    Body(rect),
                    Look::Alpha(0xb0b0ff, 190),
                    Collider,
                    Hazard,
                    only_player(ENEMY),
                    Layer(6),
                ));
                world.insert_one(ghost, Chaser { path: Vec::new(), replan: Timer::looping(0.4) }).unwrap();
            }
            let torches: Vec<Vec2> = map.marks(b'T').map(|p| p + Vec2::new(8.0, 6.0)).collect();
            for t in &torches {
                world.spawn((Body(Rect::new(t.x - 2.0, t.y - 2.0, 4.0, 10.0)), Look::Color(0xff9030), Layer(2)));
            }

            let mut inspector = Inspector::new();
            inspector.register::<Coin>("coin").register::<Hazard>("hazard").register::<Patrol>("patrol");
            Stage {
                n,
                coins,
                picked: 0,
                total: map.marks(b'C').count(),
                world,
                physics,
                map,
                player,
                start,
                font: ctx.assets().font("assets/fonts/galmuri9.bdf"),
                overlaps: Overlaps::new(),
                flash: None,
                drop: None,
                banner: Timer::once(6.0),
                tuning: Tuning::load(),
                console: Console::new(),
                inspector,
                ropes,
                sights: Vec::new(),
                torches,
                lights: RefCell::new(LightMap::new(SIZE.0, SIZE.1)),
                vignette: vignette(SIZE.0, SIZE.1, 0.7),
                scan: scanlines(SIZE.0, SIZE.1, 40),
                quit: Rc::new(Cell::new(false)),
            }
        }

        fn center(&self, e: Entity) -> Vec2 {
            self.world.get::<&Body>(e).unwrap().0.center()
        }

        fn command(&mut self, ctx: &mut Ctx, cmd: Command) -> Option<Trans> {
            match cmd.name.as_str() {
                "tp" => match (cmd.arg::<f32>(0), cmd.arg::<f32>(1)) {
                    (Some(x), Some(y)) => {
                        let mut body = self.world.get::<&mut Body>(self.player).unwrap();
                        (body.0.x, body.0.y) = (x, y);
                        self.console.print(format!("moved to {x} {y}"));
                    }
                    _ => self.console.print("usage: tp x y"),
                },
                "stage" => match cmd.arg::<usize>(0) {
                    Some(n) if n < STAGES.len() => {
                        return Some(Trans::Replace(Box::new(Stage::new(ctx, n, self.coins))));
                    }
                    _ => self.console.print(format!("usage: stage 0..{}", STAGES.len() - 1)),
                },
                "help" => self.console.print("tp x y | stage n"),
                other => self.console.print(format!("unknown command {other:?}, try help")),
            }
            None
        }

        fn patrol(&mut self) {
            for (body, vel, p) in self.world.query_mut::<(&Body, &mut Vel, &Patrol)>() {
                vel.x = if body.0.x > p.home + p.range {
                    -p.speed
                } else if body.0.x < p.home - p.range || vel.x == 0.0 {
                    p.speed
                } else {
                    vel.x.signum() * p.speed
                };
            }
        }

        fn look(&mut self) {
            self.sights.clear();
            let target = self.center(self.player);
            let eyes: Vec<(Entity, Vec2)> =
                self.world.query::<(Entity, &Body, &Guard)>().iter().map(|(e, b, _)| (e, b.0.center())).collect();
            for (e, eye) in eyes {
                let to = target - eye;
                if to.length() > SIGHT {
                    continue;
                }
                let wall = self.map.raycast(eye, to, 1.0).map(|h| h.t);
                let hit = raycast(&self.world, eye, to, 1.0, PLAYER | CRATE);
                if hit.is_some_and(|h| h.entity == self.player && wall.is_none_or(|w| h.t < w)) {
                    self.world.get::<&mut Vel>(e).unwrap().x = 90.0 * to.x.signum();
                    self.sights.push((eye, target));
                }
            }
        }

        fn chase(&mut self, dt: f32) {
            let target = self.center(self.player);
            let ghosts: Vec<Entity> = self.world.query::<(Entity, &Chaser)>().iter().map(|(e, _)| e).collect();
            for e in ghosts {
                let me = self.center(e);
                let goal = {
                    let mut chaser = self.world.get::<&mut Chaser>(e).unwrap();
                    if chaser.replan.tick(dt) || chaser.path.is_empty() {
                        chaser.path = match (self.map.cell_of(me), self.map.cell_of(target)) {
                            (Some(a), Some(b)) => self.map.path(a, b, true).unwrap_or_default(),
                            _ => Vec::new(),
                        };
                    }
                    if chaser.path.len() > 1 && (self.map.cell_center(chaser.path[1]) - me).length() < 2.0 {
                        chaser.path.remove(0);
                    }
                    chaser.path.get(1).map_or(target, |&c| self.map.cell_center(c))
                };
                let step = (goal - me).clamp_length(GHOST_SPEED * dt);
                let mut body = self.world.get::<&mut Body>(e).unwrap();
                body.0.x += step.x;
                body.0.y += step.y;
            }
        }

        fn control(&mut self, ctx: &mut Ctx, typing: bool) -> f32 {
            let dt = ctx.dt();
            let walk = if typing { 0.0 } else { ctx.input().axis("left", "right") };
            let grounded = self.physics.grounded(self.player);
            {
                // Standing on a moving platform, walking is relative to it.
                let under = self.physics.ground(self.player).and_then(|c| c.other(self.player));
                let carried = under.and_then(|e| self.world.get::<&Vel>(e).ok().map(|v| v.x)).unwrap_or(0.0);
                let mut vel = self.world.get::<&mut Vel>(self.player).unwrap();
                let speed = if ctx.input().down("run") { self.tuning.run } else { self.tuning.walk };
                vel.x = walk * speed + carried;
                if ctx.input().pressed("jump") && grounded && !typing {
                    vel.y = -self.tuning.jump;
                    ctx.audio().play("assets/sounds/jump.wav");
                } else if grounded {
                    // Walking up a slope turns sideways speed into upward speed that would launch the player at the crest.
                    vel.y = vel.y.max(0.0);
                }
                if !typing && ctx.input().pressed("down") {
                    self.drop = Some(Timer::once(0.25));
                }
            }
            let dropping = self.drop.as_mut().is_some_and(|t| !t.tick(dt) && !t.done());
            if !dropping {
                self.drop = None;
            }
            let mask = if dropping { !Filter::PLATFORM } else { u32::MAX };
            self.world.insert_one(self.player, Filter { group: PLAYER, mask }).unwrap();
            walk
        }

        fn hurt(&mut self, ctx: &mut Ctx) {
            let at = self.center(self.player);
            burst(
                &mut self.world,
                ctx.rng(),
                at,
                &Burst { count: 16, color: 0xff5050, speed: 120.0, life: 0.5, size: 3.0, gravity: 300.0 },
            );
            {
                let mut body = self.world.get::<&mut Body>(self.player).unwrap();
                body.0 = self.start;
                *self.world.get::<&mut Vel>(self.player).unwrap() = Vel::default();
            }
            self.flash = Some(Timer::once(0.5));
            ctx.audio().play("assets/sounds/hurt.wav");
            ctx.cam().cut();
            ctx.cam().shake(5.0, 0.3);
        }

        fn finish(&self, ctx: &mut Ctx) -> Trans {
            ctx.audio().play("assets/sounds/goal.wav");
            let mut save = super::load_save();
            let cleared: usize = save.get_or("cleared", 0);
            save.set("cleared", cleared.max(self.n + 1));
            super::store_save(&save);
            let next: Box<dyn Scene> = if self.n + 1 < STAGES.len() {
                Box::new(Stage::new(ctx, self.n + 1, self.coins))
            } else {
                Box::new(Ending::new(ctx, self.coins))
            };
            Trans::Fade(Box::new(Trans::Replace(next)), 0.8)
        }
    }

    impl Scene for Stage {
        fn on_enter(&mut self, ctx: &mut Ctx) {
            ctx.cam().reset();
        }

        fn update(&mut self, ctx: &mut Ctx) -> Trans {
            let dt = ctx.dt();
            if self.quit.get() {
                return Trans::Fade(Box::new(Trans::Replace(Box::new(Title::new(ctx)))), 0.6);
            }
            let typing = self.console.is_open();
            if let Some(cmd) = self.console.update(ctx.input())
                && let Some(trans) = self.command(ctx, cmd)
            {
                return trans;
            }
            self.inspector.update(ctx, &self.world);
            if !typing && ctx.input().pressed("back") {
                return Trans::Push(Box::new(Pause::new(ctx, self.quit.clone())));
            }
            self.banner.tick(dt);
            self.patrol();
            self.look();
            self.chase(dt);
            if ctx.changed(TUNING) {
                self.tuning = Tuning::load();
            }
            let walk = self.control(ctx, typing);
            self.physics.step(&mut self.world, Some(&self.map), dt);

            let mut won = false;
            let mut hurt = self.center(self.player).y > self.map.pixel_size().y + 48.0;
            let changes = self.overlaps.update(&collisions(&self.world));
            for (a, b) in changes.began {
                let other = if a == self.player {
                    b
                } else if b == self.player {
                    a
                } else {
                    continue;
                };
                if self.world.get::<&Coin>(other).is_ok() {
                    let at = self.center(other);
                    burst(
                        &mut self.world,
                        ctx.rng(),
                        at,
                        &Burst { count: 8, color: 0xffd040, speed: 90.0, life: 0.4, size: 2.0, gravity: 200.0 },
                    );
                    self.world.despawn(other).unwrap();
                    ctx.audio().play("assets/sounds/coin.wav");
                    (self.picked, self.coins) = (self.picked + 1, self.coins + 1);
                } else if self.world.get::<&Goal>(other).is_ok() {
                    won = true;
                } else if self.world.get::<&Hazard>(other).is_ok() {
                    hurt = true;
                }
            }
            if self.physics.events_of(self.player).any(|c| c.began && c.b.is_none() && c.impulse > 300.0) {
                ctx.cam().shake(2.0, 0.12);
            }
            if won {
                return self.finish(ctx);
            }
            if hurt {
                self.hurt(ctx);
            }

            let flashing = self.flash.as_mut().is_some_and(|t| !t.tick(dt) && !t.done());
            if !flashing {
                self.flash = None;
            }
            let style = if flashing { Style::PLAIN.flashed(0xffffff, 220) } else { Style::PLAIN };
            self.world.insert_one(self.player, style).unwrap();

            let grounded = self.physics.grounded(self.player);
            if let Ok(mut anim) = self.world.get::<&mut Anim>(self.player) {
                anim.flip = if walk != 0.0 { walk < 0.0 } else { anim.flip };
            }
            if let Ok(mut states) = self.world.get::<&mut AnimStates>(self.player) {
                states.set(if !grounded {
                    "air"
                } else if walk != 0.0 {
                    "run"
                } else {
                    "idle"
                });
            }
            animate(&mut self.world, dt);
            particles(&mut self.world, dt);
            let target = self.world.get::<&Body>(self.player).unwrap().0;
            ctx.cam().follow(target, self.map.pixel_size(), dt);
            Trans::None
        }

        fn draw(&self, g: &mut Gfx) {
            let info = &STAGES[self.n];
            g.clear(if info.dark { 0x0a0c18 } else { 0x6fa8e0 });
            let hills = (0..12).map(|i| {
                let (x, h) = (i as f32 * 140.0, 60.0 + (i * 37 % 5) as f32 * 14.0);
                let color = if info.dark { 0x16202a } else { 0x4f8f5f };
                DrawCmd::new(Rect { x, y: 340.0 - h, w: 160.0, h }, Paint::Color(color))
            });
            g.draw_parallax(hills, (0.4, 0.0));
            g.draw(self.map.draw_list(g.view()));
            g.draw(draw_list(&self.world));
            g.draw(self.ropes.iter().map(|&(a, b)| beam(self.center(a), self.center(b), 0xcccccc)));
            g.draw(self.sights.iter().map(|&(a, b)| beam(a, b, 0xff4040)));
            if info.dark {
                let mut lights = self.lights.borrow_mut();
                lights.clear(0x0b0e1c);
                lights.add_world(g, Light::new(self.center(self.player), 120.0).colored(0xffc070));
                for &t in &self.torches {
                    lights.add_world(g, Light::new(t, 90.0).colored(0xff9040).intensity(0.9));
                }
                lights.draw(g);
                g.multiply(&self.vignette);
                g.multiply(&self.scan);
            }
            self.inspector.draw(g, &self.world);
            self.console.draw(g, &self.font);

            draw_panel(g, Rect::new(4.0, 4.0, 200.0, 20.0), 0x202030, 0x8080a0);
            let hud = format!("{}-{}  동전 {}/{} ({})", self.n + 1, info.name, self.picked, self.total, self.coins);
            g.text_font(&self.font, &hud, 8, 8, 0xffffff);
            if !self.banner.done() {
                let area = Rect::new(60.0, 30.0, 520.0, 36.0);
                draw_panel(g, area, 0x202030, 0x8080a0);
                draw_wrapped(g, &self.font, info.intro, Rect::new(68.0, 36.0, 504.0, 28.0), 0xffffff);
            }
        }
    }
}

mod title {
    use super::level::STAGES;
    use super::stage::Stage;
    use engine::{Ctx, Font, Gfx, Rect, Scene, ScrollList, Theme, Trans};
    use std::sync::Arc;

    pub struct Title {
        font: Arc<Font>,
        menu: ScrollList,
        list: Option<ScrollList>,
        cleared: usize,
    }

    impl Title {
        pub fn new(ctx: &mut Ctx) -> Self {
            Title {
                font: ctx.assets().font("assets/fonts/galmuri9.bdf"),
                menu: ScrollList::menu(260, 170, &["시작", "스테이지 선택", "끝"]),
                list: None,
                cleared: super::load_save().get_or("cleared", 0),
            }
        }

        fn start(ctx: &mut Ctx, n: usize) -> Trans {
            Trans::Fade(Box::new(Trans::Replace(Box::new(Stage::new(ctx, n, 0)))), 0.6)
        }
    }

    impl Scene for Title {
        fn update(&mut self, ctx: &mut Ctx) -> Trans {
            let back = ctx.input().pressed("back");
            if let Some(list) = &mut self.list {
                if back {
                    self.list = None;
                } else if let Some(i) = list.update(ctx.input(), &self.font, "up", "down", "accept")
                    && i <= self.cleared
                {
                    return Title::start(ctx, i);
                }
                return Trans::None;
            }
            if back {
                return Trans::Quit;
            }
            match self.menu.update(ctx.input(), &self.font, "up", "down", "accept") {
                Some(0) => return Title::start(ctx, 0),
                Some(1) => {
                    let names: Vec<String> = STAGES
                        .iter()
                        .enumerate()
                        .map(|(i, s)| {
                            if i <= self.cleared { format!("{}. {}", i + 1, s.name) } else { format!("{}. ???", i + 1) }
                        })
                        .collect();
                    let names: Vec<&str> = names.iter().map(String::as_str).collect();
                    self.list = Some(ScrollList::new(Rect::new(200.0, 150.0, 240.0, 120.0), &names));
                }
                Some(_) => return Trans::Quit,
                None => {}
            }
            Trans::None
        }

        fn draw(&self, g: &mut Gfx) {
            g.clear(0x101828);
            g.text("TOWER", 192, 50, 8, 0xffd040);
            g.text_font(&self.font, "엔진 기능이 하나씩 나오는 플랫폼 게임", 190, 130, 0xaaaaaa);
            let theme = Theme::default();
            match &self.list {
                Some(list) => list.draw(g, &self.font, &theme),
                None => self.menu.draw(g, &self.font, &theme),
            }
            g.text_font(&self.font, "위아래: 고르기   Enter: 결정   Esc: 뒤로", 190, 330, 0x808090);
        }
    }
}

mod pause {
    use engine::{Ctx, Font, Gfx, Key, Rebind, Rect, Scene, ScrollList, Theme, Trans, draw_panel};
    use std::{cell::Cell, rc::Rc, sync::Arc};

    pub struct Pause {
        font: Arc<Font>,
        menu: ScrollList,
        rebind: Option<Rebind>,
        quit: Rc<Cell<bool>>,
    }

    impl Pause {
        pub fn new(ctx: &mut Ctx, quit: Rc<Cell<bool>>) -> Self {
            let font = ctx.assets().font("assets/fonts/galmuri9.bdf");
            let mut menu = ScrollList::menu(260, 130, &["계속", "", "키 바꾸기", "전체 화면", "타이틀로"]);
            menu.set_item(1, volume_label(ctx.audio().volume()));
            Pause { font, menu, rebind: None, quit }
        }
    }

    fn volume_label(v: f32) -> String {
        format!("소리: < {}/10 >", (v * 10.0).round())
    }

    impl Scene for Pause {
        fn update(&mut self, ctx: &mut Ctx) -> Trans {
            if let Some(rebind) = &mut self.rebind {
                let was_waiting = rebind.waiting();
                if rebind.update(ctx)
                    && let Some(path) = super::bindings_file()
                    && let Err(e) = ctx.save_bindings(&path)
                {
                    eprintln!("{e}");
                }
                if !was_waiting && ctx.input().key_pressed(Key::Escape) {
                    self.rebind = None;
                }
                return Trans::None;
            }
            if ctx.input().pressed("back") {
                return Trans::Pop;
            }
            let step = ctx.input().repeated("right") as i32 - ctx.input().repeated("left") as i32;
            if self.menu.selected() == 1 && step != 0 {
                let v = ((ctx.audio().volume() * 10.0).round() + step as f32).clamp(0.0, 10.0) / 10.0;
                ctx.audio().set_volume(v);
                self.menu.set_item(1, volume_label(v));
                ctx.audio().play("assets/sounds/coin.wav");
                let mut save = super::load_save();
                save.set("volume", v);
                super::store_save(&save);
            }
            match self.menu.update(ctx.input(), &self.font, "up", "down", "accept") {
                Some(0) => return Trans::Pop,
                Some(2) => self.rebind = Some(Rebind::new(ctx.input(), 200, 80)),
                Some(3) => ctx.toggle_fullscreen(),
                Some(1) => {}
                Some(_) => {
                    self.quit.set(true);
                    return Trans::Pop;
                }
                None => {}
            }
            Trans::None
        }

        fn draw(&self, g: &mut Gfx) {
            g.fill_alpha(Rect::new(0.0, 0.0, 640.0, 360.0), 0x000000, 150);
            draw_panel(g, Rect::new(180.0, 60.0, 280.0, 240.0), 0x202030, 0x8080a0);
            let theme = Theme::default();
            match &self.rebind {
                Some(r) => r.draw(g, &self.font, &theme),
                None => self.menu.draw(g, &self.font, &theme),
            }
        }
    }
}

mod ending {
    use super::level::total_coins;
    use super::title::Title;
    use engine::{
        Button, Ctx, Font, Gfx, Rect, Scene, TextField, Theme, Timer, Trans, draw_bar, draw_panel, draw_wrapped,
    };
    use std::sync::Arc;

    pub struct Ending {
        font: Arc<Font>,
        coins: u32,
        total: usize,
        best: u32,
        field: TextField,
        button: Button,
        blink: Timer,
        saved: bool,
    }

    impl Ending {
        pub fn new(ctx: &mut Ctx, coins: u32) -> Self {
            let mut save = super::load_save();
            let best = save.get_or("best", 0u32);
            if coins > best {
                save.set("best", coins);
                super::store_save(&save);
            }
            let mut field = TextField::new(200, 200, 12);
            field.set(&save.get_or("name", String::new()));
            Ending {
                font: ctx.assets().font("assets/fonts/galmuri9.bdf"),
                coins,
                total: total_coins(),
                best: best.max(coins),
                field,
                button: Button::new(Rect::new(260.0, 270.0, 120.0, 22.0), "타이틀로"),
                blink: Timer::looping(1.0),
                saved: false,
            }
        }
    }

    impl Scene for Ending {
        fn update(&mut self, ctx: &mut Ctx) -> Trans {
            self.blink.tick(ctx.dt());
            if self.field.update(ctx.input()) {
                let mut save = super::load_save();
                save.set("name", self.field.text());
                super::store_save(&save);
                self.saved = true;
            }
            if self.button.update(ctx.input()) || ctx.input().pressed("back") {
                return Trans::Fade(Box::new(Trans::Replace(Box::new(Title::new(ctx)))), 0.6);
            }
            Trans::None
        }

        fn draw(&self, g: &mut Gfx) {
            g.clear(0x101828);
            draw_panel(g, Rect::new(120.0, 40.0, 400.0, 270.0), 0x202030, 0x8080a0);
            g.text_font_scaled(&self.font, "클리어!", 270, 56, 2, 0xffd040);
            let total = self.total;
            let text = format!("동전 {} / {total}  (최고 {})", self.coins, self.best);
            g.text_font(&self.font, &text, 200, 100, 0xffffff);
            draw_bar(g, Rect::new(200.0, 124.0, 240.0, 10.0), self.coins as f32 / total as f32, 0xffd040, 0x404058);
            let note = "이름을 입력하고 Enter를 누르면 저장됩니다. 오른쪽 Alt로 한글과 영문을 바꿉니다.";
            draw_wrapped(g, &self.font, note, Rect::new(200.0, 150.0, 240.0, 40.0), 0xaaaaaa);
            let theme = Theme::default();
            self.field.draw(g, &self.font, &theme, self.blink.fraction() < 0.5);
            if self.saved {
                g.text_font(&self.font, "저장했습니다", 200, 230, 0x80ff80);
            }
            self.button.draw(g, &self.font, &theme);
        }
    }
}

fn data_file(name: &str) -> Option<String> {
    data_dir("tower").ok().map(|d| format!("{d}/{name}"))
}

pub fn load_save() -> Save {
    let Some(path) = data_file("save.txt") else { return Save::default() };
    Save::load(&path).unwrap_or_else(|e| {
        // Saving over a file that would not load would destroy what the player had.
        eprintln!("{e}; keeping it as {path}.bak");
        let _ = std::fs::rename(&path, format!("{path}.bak"));
        Save::default()
    })
}

pub fn store_save(save: &Save) {
    if let Some(p) = data_file("save.txt")
        && let Err(e) = save.save(&p)
    {
        eprintln!("{e}");
    }
}

pub fn bindings_file() -> Option<String> {
    data_file("input.txt")
}

fn main() {
    let start = std::env::args().nth(1).and_then(|s| s.parse::<usize>().ok());
    engine::run(Config { title: "tower", scale: 2, ..Config::default() }, move |ctx| {
        ctx.bind("left", &[Key::Left, Key::A]);
        ctx.bind("right", &[Key::Right, Key::D]);
        ctx.bind("jump", &[Key::Space, Key::Up, Key::W]);
        ctx.bind("down", &[Key::Down, Key::S]);
        ctx.bind("run", &[Key::LeftShift, Key::RightShift]);
        ctx.bind("up", &[Key::Up, Key::W]);
        ctx.bind("accept", &[Key::Enter]);
        ctx.bind("back", &[Key::Escape]);
        if let Some(path) = bindings_file()
            && let Err(e) = ctx.load_bindings(&path)
        {
            eprintln!("{e}");
        }
        let volume: f32 = load_save().get_or("volume", 1.0);
        ctx.audio().set_volume(if volume.is_finite() { volume } else { 1.0 });
        match start {
            Some(n) if n < level::STAGES.len() => Box::new(stage::Stage::new(ctx, n, 0)) as Box<dyn Scene>,
            _ => Box::new(title::Title::new(ctx)),
        }
    });
}
