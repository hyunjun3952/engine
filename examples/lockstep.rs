// Two or more copies of this program play one game over UDP, each sending only its arrow keys:
//   cargo run --example lockstep -- 0 127.0.0.1:7000 127.0.0.1:7001
//   cargo run --example lockstep -- 1 127.0.0.1:7000 127.0.0.1:7001
// The first argument is this copy's player number, the rest are where every player listens.
use engine::{
    Actions, Body, Config, Ctx, Entity, Gfx, Key, Lockstep, Look, Rect, Scene, Trans, Udp, World, WorldFile, draw_list,
};

const SPEED: f32 = 2.0;

struct Duel {
    world: World,
    ents: Vec<Entity>,
    file: WorldFile,
    actions: Actions,
    lock: Lockstep,
    udp: Udp,
    me: usize,
}

impl Duel {
    fn new(me: usize, peers: Vec<std::net::SocketAddr>) -> Self {
        let mut world = World::new();
        let colors = [0xe06060, 0x60a0e0, 0x80e080, 0xe0d060];
        let ents = (0..peers.len())
            .map(|p| {
                let rect = Rect { x: 100.0 + p as f32 * 80.0, y: 160.0, w: 20.0, h: 20.0 };
                world.spawn((Body(rect), Look::Color(colors[p % colors.len()])))
            })
            .collect();
        let udp = Udp::bind(&peers[me].to_string(), me, &peers).expect("bind the address of this player");
        Duel {
            world,
            ents,
            file: WorldFile::new().component::<Body>("body"),
            actions: Actions::new(&["left", "right", "up", "down"]),
            lock: Lockstep::new(me, peers.len(), 3),
            udp,
            me,
        }
    }
}

impl Scene for Duel {
    fn on_exit(&mut self, _: &mut Ctx) {
        let (tick, desync) = (self.lock.tick(), self.lock.desync());
        eprintln!("lockstep: player {} stopped at step {tick}, desync {desync:?}", self.me);
    }

    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input().pressed("back") {
            return Trans::Quit;
        }
        let mask = self.actions.mask(ctx.input());
        self.lock.push_local(mask);
        self.udp.recv(&mut self.lock);
        self.udp.send(&self.lock);
        // Catch up when the peers were late: a few steps for each tick, but never a spiral.
        for _ in 0..4 {
            let Some(input) = self.lock.ready() else { break };
            for (p, &e) in self.ents.iter().enumerate() {
                let held = self.actions.held(input.now[p], input.before[p]);
                let mut body = self.world.get::<&mut Body>(e).unwrap();
                body.0.x = (body.0.x + held.axis("left", "right") * SPEED).clamp(0.0, 620.0);
                body.0.y = (body.0.y + held.axis("up", "down") * SPEED).clamp(0.0, 340.0);
            }
            let tick = self.lock.tick();
            self.lock.advance((tick % 10 == 9).then(|| self.file.checksum(&self.world)));
        }
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x202830);
        g.draw(draw_list(&self.world));
        let status = match (self.lock.desync(), self.lock.waiting_on().as_slice()) {
            (Some(t), _) => format!("DESYNC at step {t}"),
            (None, []) => format!("step {}", self.lock.tick()),
            (None, waiting) => format!("step {}: waiting for player {}", self.lock.tick(), waiting[0] + 1),
        };
        g.text(&status, 8, 8, 1, 0xffffff);
        g.text("arrows: move   Esc: quit", 8, 20, 1, 0xa0a0a0);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: lockstep ME ADDR0 ADDR1 [ADDR2 ..]   (ME is this copy's number, the addresses are where each player listens)";
    let me: usize = args.first().and_then(|a| a.parse().ok()).unwrap_or_else(|| panic!("{usage}"));
    let peers: Vec<std::net::SocketAddr> =
        args[1..].iter().map(|a| a.parse().unwrap_or_else(|_| panic!("{usage}"))).collect();
    assert!(peers.len() >= 2 && me < peers.len(), "{usage}");
    engine::run(Config { title: "lockstep", ..Config::default() }, move |ctx| {
        ctx.bind("left", &[Key::Left]);
        ctx.bind("right", &[Key::Right]);
        ctx.bind("up", &[Key::Up]);
        ctx.bind("down", &[Key::Down]);
        ctx.bind("back", &[Key::Escape]);
        Box::new(Duel::new(me, peers.clone()))
    });
}
