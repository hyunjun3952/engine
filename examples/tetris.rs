use engine::{Burst, Config, Ctx, Gfx, Key, Rect, Save, Scene, Trans, World, burst, draw_list, draw_panel, particles};

const W: usize = 10;
const H: usize = 20;
const CELL: f32 = 16.0;
const BOARD: Rect = Rect { x: 220.0, y: 20.0, w: W as f32 * CELL, h: H as f32 * CELL };
const SAVE: &str = "tetris.save";
const DAS_DELAY: f32 = 0.17;
const DAS_REPEAT: f32 = 0.05;
const POINTS: [u32; 5] = [0, 100, 300, 500, 800];
const SPARK: Burst = Burst { count: 2, color: 0, speed: 90.0, life: 0.4, size: 3.0, gravity: 200.0 };

// I O T S Z J L: the side of the box the piece turns in, and its cells in that box.
const SHAPES: [(i32, [(i32, i32); 4]); 7] = [
    (4, [(0, 1), (1, 1), (2, 1), (3, 1)]),
    (2, [(0, 0), (1, 0), (0, 1), (1, 1)]),
    (3, [(1, 0), (0, 1), (1, 1), (2, 1)]),
    (3, [(1, 0), (2, 0), (0, 1), (1, 1)]),
    (3, [(0, 0), (1, 0), (1, 1), (2, 1)]),
    (3, [(0, 0), (0, 1), (1, 1), (2, 1)]),
    (3, [(2, 0), (0, 1), (1, 1), (2, 1)]),
];
const COLORS: [u32; 7] = [0x55e0e0, 0xe0e055, 0xa055e0, 0x55e055, 0xe05555, 0x5588e0, 0xe0a055];

#[derive(Clone, Copy)]
struct Piece {
    kind: usize,
    turns: usize,
    x: i32,
    y: i32,
}

impl Piece {
    fn cells(&self) -> [(i32, i32); 4] {
        let (n, mut cells) = SHAPES[self.kind];
        for _ in 0..self.turns {
            cells = cells.map(|(x, y)| (n - 1 - y, x));
        }
        cells.map(|(x, y)| (x + self.x, y + self.y))
    }
}

fn full_rows(board: &[[u8; W]]) -> Vec<usize> {
    (0..board.len()).filter(|&y| !board[y].contains(&0)).collect()
}

// Full rows go and empty ones come in at the top, so everything above falls.
fn remove_rows(board: &mut Vec<[u8; W]>) {
    let height = board.len();
    board.retain(|row| row.contains(&0));
    while board.len() < height {
        board.insert(0, [0; W]);
    }
}

fn cell_rect(x: i32, y: i32) -> Rect {
    Rect { x: BOARD.x + x as f32 * CELL, y: BOARD.y + y as f32 * CELL, w: CELL - 1.0, h: CELL - 1.0 }
}

struct Over {
    score: u32,
    hi: u32,
}

impl Scene for Over {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input().pressed("confirm") {
            return Trans::Replace(Box::new(Tetris::new(ctx)));
        }
        if ctx.input().pressed("back") {
            return Trans::Quit;
        }
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        g.text("GAME OVER", 200, 100, 4, 0xffff66);
        g.text(&format!("SCORE {}", self.score), 240, 170, 2, 0xffffff);
        g.text(&format!("BEST  {}", self.hi), 240, 195, 2, 0xaaaaaa);
        g.text("ENTER: again   ESC: quit", 200, 260, 2, 0xaaaaaa);
    }
}

struct Tetris {
    world: World,
    board: Vec<[u8; W]>,
    cur: Piece,
    next: usize,
    bag: Vec<usize>,
    fall: f32,
    das: f32,
    dir: i32,
    score: u32,
    lines: u32,
    hi: u32,
    // False when the save file exists but could not be read: it is left alone instead of overwritten.
    save_ok: bool,
    paused: bool,
}

impl Tetris {
    fn new(ctx: &mut Ctx) -> Self {
        let (hi, save_ok) = match Save::load(SAVE) {
            Ok(save) => (save.get_or("hi", 0), true),
            Err(e) => {
                eprintln!("{e}; the best score will not be saved");
                (0, false)
            }
        };
        let mut game = Tetris {
            world: World::new(),
            board: vec![[0; W]; H],
            cur: Piece { kind: 0, turns: 0, x: 3, y: 0 },
            next: 0,
            bag: Vec::new(),
            fall: 0.0,
            das: 0.0,
            dir: 0,
            score: 0,
            lines: 0,
            hi,
            save_ok,
            paused: false,
        };
        game.next = game.pull(ctx);
        game.spawn(ctx);
        game
    }

    // Seven pieces in random order, then seven again, so no piece is missing for long.
    fn pull(&mut self, ctx: &mut Ctx) -> usize {
        if self.bag.is_empty() {
            self.bag = (0..SHAPES.len()).collect();
            for i in (1..self.bag.len()).rev() {
                let j = ctx.rng().range(0.0, (i + 1) as f32) as usize;
                self.bag.swap(i, j.min(i));
            }
        }
        self.bag.pop().unwrap()
    }

    fn fits(&self, p: &Piece) -> bool {
        p.cells().iter().all(|&(x, y)| {
            (0..W as i32).contains(&x) && y < H as i32 && y >= 0 && self.board[y as usize][x as usize] == 0
        })
    }

    // False when the new piece has no room: the stack reached the top.
    fn spawn(&mut self, ctx: &mut Ctx) -> bool {
        self.cur = Piece { kind: self.next, turns: 0, x: 3, y: 0 };
        self.next = self.pull(ctx);
        self.fits(&self.cur)
    }

    fn shifted(&self, dx: i32, dy: i32) -> Option<Piece> {
        let p = Piece { x: self.cur.x + dx, y: self.cur.y + dy, ..self.cur };
        self.fits(&p).then_some(p)
    }

    fn rotate(&mut self, ctx: &mut Ctx) {
        let turned = Piece { turns: (self.cur.turns + 1) % 4, ..self.cur };
        for kick in [0, -1, 1, -2, 2] {
            let p = Piece { x: turned.x + kick, ..turned };
            if self.fits(&p) {
                self.cur = p;
                ctx.audio().play("assets/audio/jump.wav");
                return;
            }
        }
    }

    fn landing(&self) -> Piece {
        let mut p = self.cur;
        while self.fits(&Piece { y: p.y + 1, ..p }) {
            p.y += 1;
        }
        p
    }

    // Writes the piece into the board, clears full rows and brings the next piece; false when the game is over.
    fn lock(&mut self, ctx: &mut Ctx) -> bool {
        for (x, y) in self.cur.cells() {
            self.board[y as usize][x as usize] = self.cur.kind as u8 + 1;
        }
        ctx.audio().play("assets/audio/hit.wav");
        let full = full_rows(&self.board);
        for &y in &full {
            for (x, &kind) in self.board[y].iter().enumerate() {
                let at = cell_rect(x as i32, y as i32).center();
                burst(&mut self.world, ctx.rng(), at, &Burst { color: COLORS[kind as usize - 1], ..SPARK });
            }
        }
        if !full.is_empty() {
            ctx.audio().play("assets/audio/brick.wav");
            remove_rows(&mut self.board);
            self.score += POINTS[full.len()] * (self.level() + 1);
            self.lines += full.len() as u32;
        }
        self.spawn(ctx)
    }

    fn level(&self) -> u32 {
        self.lines / 10
    }

    fn interval(&self) -> f32 {
        (0.8 - self.level() as f32 * 0.07).max(0.05)
    }

    fn game_over(&mut self, ctx: &mut Ctx) -> Trans {
        ctx.audio().play("assets/audio/score.wav");
        if self.score > self.hi {
            self.hi = self.score;
            if self.save_ok {
                let mut save = Save::new();
                save.set("hi", self.hi);
                if let Err(e) = save.save(SAVE) {
                    eprintln!("{SAVE}: {e}");
                }
            }
        }
        Trans::Replace(Box::new(Over { score: self.score, hi: self.hi }))
    }
}

impl Scene for Tetris {
    fn on_enter(&mut self, ctx: &mut Ctx) {
        ctx.cam().reset();
    }

    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        let dt = ctx.dt();
        if ctx.input().pressed("back") {
            return Trans::Quit;
        }
        if ctx.input().pressed("pause") {
            self.paused = !self.paused;
        }
        if self.paused {
            return Trans::None;
        }
        particles(&mut self.world, dt);

        let dir = ctx.input().axis("left", "right") as i32;
        // One step on the press, then a repeat once the key has been held for a moment.
        let mut moves = 0;
        if dir != self.dir {
            (self.dir, self.das, moves) = (dir, 0.0, (dir != 0) as i32);
        } else if dir != 0 {
            self.das += dt;
            while self.das >= DAS_DELAY {
                self.das -= DAS_REPEAT;
                moves += 1;
            }
        }
        for _ in 0..moves {
            if let Some(p) = self.shifted(dir, 0) {
                self.cur = p;
            }
        }
        if ctx.input().pressed("rotate") {
            self.rotate(ctx);
        }
        if ctx.input().pressed("drop") {
            let cells = self.landing().y - self.cur.y;
            self.score += 2 * cells as u32;
            self.cur = self.landing();
            return if self.lock(ctx) { Trans::None } else { self.game_over(ctx) };
        }

        let soft = ctx.input().down("down");
        self.fall += if soft { dt * 20.0 } else { dt };
        while self.fall >= self.interval() {
            self.fall -= self.interval();
            match self.shifted(0, 1) {
                Some(p) => {
                    self.cur = p;
                    self.score += soft as u32;
                }
                None => {
                    if !self.lock(ctx) {
                        return self.game_over(ctx);
                    }
                }
            }
        }
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        draw_panel(g, &BOARD, 0x181824, 0x444466);
        for (y, row) in self.board.iter().enumerate() {
            for (x, &kind) in row.iter().enumerate() {
                if kind != 0 {
                    g.fill(&cell_rect(x as i32, y as i32), COLORS[kind as usize - 1]);
                }
            }
        }
        let color = COLORS[self.cur.kind];
        for (x, y) in self.landing().cells() {
            g.fill_alpha(&cell_rect(x, y), color, 70);
        }
        for (x, y) in self.cur.cells() {
            g.fill(&cell_rect(x, y), color);
        }

        let side = Rect { x: 410.0, y: 20.0, w: 72.0, h: 56.0 };
        g.text("NEXT", 410, 8, 1, 0xaaaaaa);
        draw_panel(g, &side, 0x181824, 0x444466);
        let shown = Piece { kind: self.next, turns: 0, x: 0, y: 0 };
        for (x, y) in shown.cells() {
            let at = Rect { x: side.x + 10.0 + x as f32 * 12.0, y: side.y + 10.0 + y as f32 * 12.0, w: 11.0, h: 11.0 };
            g.fill(&at, COLORS[self.next]);
        }
        for (i, (label, value)) in
            [("SCORE", self.score), ("LINES", self.lines), ("LEVEL", self.level()), ("BEST", self.hi)]
                .into_iter()
                .enumerate()
        {
            let y = 100 + i as i32 * 40;
            g.text(label, 410, y, 1, 0xaaaaaa);
            g.text(&value.to_string(), 410, y + 12, 2, 0xffffff);
        }
        g.text("ARROWS: move  UP: turn", 8, 320, 1, 0x888899);
        g.text("SPACE: drop  P: pause", 8, 332, 1, 0x888899);
        g.draw(draw_list(&self.world));
        if self.paused {
            g.text("PAUSED", 252, 160, 3, 0xffff66);
        }
    }
}

fn main() {
    engine::run(Config { title: "tetris", scale: 2, ..Config::default() }, |ctx| {
        ctx.bind("left", &[Key::Left, Key::A]);
        ctx.bind("right", &[Key::Right, Key::D]);
        ctx.bind("down", &[Key::Down, Key::S]);
        ctx.bind("rotate", &[Key::Up, Key::W, Key::X]);
        ctx.bind("drop", &[Key::Space]);
        ctx.bind("pause", &[Key::P]);
        ctx.bind("confirm", &[Key::Enter]);
        ctx.bind("back", &[Key::Escape]);
        if let Err(e) = ctx.load_bindings("input.txt") {
            eprintln!("{e}");
        }
        Box::new(Tetris::new(ctx))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_turns_bring_every_piece_back_and_a_turn_keeps_four_distinct_cells() {
        for kind in 0..SHAPES.len() {
            let start = Piece { kind, turns: 0, x: 0, y: 0 };
            assert_eq!(Piece { turns: 4, ..start }.cells(), start.cells());
            for turns in 0..4 {
                let cells = Piece { turns, ..start }.cells();
                assert!(cells.iter().all(|&(x, y)| (0..4).contains(&x) && (0..4).contains(&y)), "{kind} {turns}");
                assert!(cells.iter().enumerate().all(|(i, c)| !cells[..i].contains(c)), "{kind} {turns}");
            }
        }
        let flat = Piece { kind: 0, turns: 0, x: 0, y: 0 };
        let upright = Piece { turns: 1, ..flat };
        assert!(flat.cells().iter().all(|c| c.1 == 1) && upright.cells().iter().all(|c| c.0 == 2));
    }

    #[test]
    fn full_rows_are_found_and_removed_and_the_rows_above_fall() {
        let mut board = vec![[0u8; W]; H];
        board[H - 1] = [1; W];
        board[H - 2] = [2; W];
        board[H - 3][4] = 7;
        board[H - 4] = [3; W];
        assert_eq!(full_rows(&board), [H - 4, H - 2, H - 1]);
        remove_rows(&mut board);
        assert_eq!(board.len(), H);
        assert_eq!(board[H - 1][4], 7, "the lone block fell to the floor");
        assert!(full_rows(&board).is_empty());
    }
}
