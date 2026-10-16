use crate::{
    Anim, AnimStates, Body, Ctx, DrawCmd, Entity, Gfx, Input, Key, Layer, Look, Paint, Rect, TextField, Vec2, World,
};
use std::collections::VecDeque;
use std::fmt::Debug;

type Show = Box<dyn Fn(&World, Entity) -> Option<String>>;

/// Lists what an entity has: press F4 to open it, click an entity in the world to pick it. The engine's own
/// components are shown; `register` adds your own. Call `update` in `Scene::update` and `draw` in `Scene::draw`
/// after the world; the text is the built-in 8x8 ASCII font.
///
/// ```
/// use engine::{Body, Inspector, Rect, World};
///
/// let mut world = World::new();
/// let e = world.spawn((Body(Rect::new(0.0, 0.0, 8.0, 8.0)),));
/// let inspector = Inspector::new();
/// assert!(!inspector.describe(&world, e).is_empty());
/// ```
pub struct Inspector {
    shows: Vec<(&'static str, Show)>,
    open: bool,
    picked: Option<Entity>,
}

impl Inspector {
    /// A closed inspector that knows the engine's components.
    pub fn new() -> Self {
        let mut i = Inspector { shows: Vec::new(), open: false, picked: None };
        i.register::<Body>("Body");
        i.register::<crate::Vel>("Vel");
        i.register::<Layer>("Layer");
        i.register::<crate::Collider>("Collider");
        i.register::<crate::Filter>("Filter");
        i.register::<crate::Transform>("Transform");
        i.register::<crate::Style>("Style");
        i.register::<crate::RigidBody>("RigidBody");
        i.register_with("Look", |w, e| {
            w.get::<&Look>(e).ok().map(|l| match &*l {
                Look::Color(c) => format!("color {c:06x}"),
                Look::Alpha(c, a) => format!("color {c:06x} alpha {a}"),
                Look::Sprite(s) => format!("sprite {}x{}", s.size().0, s.size().1),
            })
        });
        i.register_with("Anim", |w, e| {
            w.get::<&Anim>(e)
                .ok()
                .map(|a| format!("frame {}{}", a.frame(), if a.finished() { " finished" } else { "" }))
        });
        i.register_with("AnimStates", |w, e| w.get::<&AnimStates>(e).ok().map(|s| format!("state {}", s.current())));
        i
    }

    /// Shows component `T` of an entity (through its `Debug`) under `name`.
    pub fn register<T: Debug + Send + Sync + 'static>(&mut self, name: &'static str) -> &mut Self {
        self.register_with(name, |w, e| w.get::<&T>(e).ok().map(|c| format!("{:?}", *c)))
    }

    /// Shows whatever `show` returns for an entity (or nothing, when it has no such thing) under `name`.
    pub fn register_with(
        &mut self,
        name: &'static str,
        show: impl Fn(&World, Entity) -> Option<String> + 'static,
    ) -> &mut Self {
        self.shows.push((name, Box::new(show)));
        self
    }

    /// True while the panel is shown.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The entity whose components are listed.
    pub fn selected(&self) -> Option<Entity> {
        self.picked
    }

    /// Picks an entity, or none.
    pub fn select(&mut self, e: Option<Entity>) {
        self.picked = e;
    }

    /// F4 opens and closes it; while open a left click picks the entity under the cursor (the one on the highest
    /// layer; empty ground picks nothing).
    pub fn update(&mut self, ctx: &Ctx, world: &World) {
        if ctx.input().key_pressed(Key::F4) {
            self.open = !self.open;
        }
        if self.open
            && ctx.input().mouse_pressed(crate::MouseButton::Left)
            && let Some(Vec2 { x, y }) = ctx.mouse_world()
        {
            self.picked = pick(world, x, y);
        }
    }

    /// The lines it would show for an entity: its id, then one for each component it has.
    pub fn describe(&self, world: &World, e: Entity) -> Vec<String> {
        if !world.contains(e) {
            return vec![format!("{e:?} is gone")];
        }
        let mut lines = vec![format!("entity {e:?}")];
        lines.extend(self.shows.iter().filter_map(|(name, show)| Some(format!("{name}: {}", show(world, e)?))));
        lines
    }

    /// Draws the panel when it is open.
    pub fn draw(&self, gfx: &mut Gfx, world: &World) {
        if !self.open {
            return;
        }
        let lines = match self.picked {
            Some(e) => {
                if let Ok(body) = world.get::<&Body>(e) {
                    gfx.draw([DrawCmd { layer: i32::MAX, ..DrawCmd::new(body.0, Paint::Outline(0xffff00)) }]);
                }
                self.describe(world, e)
            }
            None => vec!["F4: click an entity".to_string()],
        };
        let width = lines.iter().map(std::string::String::len).max().unwrap_or(0) as f32 * 8.0 + 8.0;
        let panel = Rect::new(gfx.w as f32 - width, 0.0, width, lines.len() as f32 * 10.0 + 6.0);
        gfx.fill_alpha(panel, 0x000000, 200);
        for (i, line) in lines.iter().enumerate() {
            gfx.text(line, panel.x as i32 + 4, 4 + i as i32 * 10, 1, 0xdddddd);
        }
    }
}

impl Default for Inspector {
    fn default() -> Self {
        Inspector::new()
    }
}

fn pick(world: &World, x: f32, y: f32) -> Option<Entity> {
    let mut best: Option<(i32, Entity)> = None;
    for (e, body, layer) in &mut world.query::<(Entity, &Body, Option<&Layer>)>() {
        let layer = layer.map_or(0, |l| l.0);
        if body.0.contains((x, y)) && best.is_none_or(|b| layer >= b.0) {
            best = Some((layer, e));
        }
    }
    best.map(|b| b.1)
}

/// A line of text typed into the console: the first word and the rest, where "double quotes" keep spaces inside one word.
///
/// ```
/// use engine::Command;
///
/// let cmd = Command::parse("spawn \"big goblin\" 3").unwrap();
/// assert_eq!(cmd.name, "spawn");
/// assert_eq!(cmd.args[0], "big goblin");
/// assert_eq!(cmd.arg::<i32>(1), Some(3));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    /// The first word of the line.
    pub name: String,
    /// The words after it.
    pub args: Vec<String>,
}

impl Command {
    /// `None` for a blank line.
    pub fn parse(line: &str) -> Option<Command> {
        let (mut words, mut word, mut quoted, mut any) = (Vec::new(), String::new(), false, false);
        for c in line.chars() {
            match c {
                '"' => (quoted, any) = (!quoted, true),
                c if c.is_whitespace() && !quoted => {
                    if any {
                        words.push(std::mem::take(&mut word));
                    }
                    any = false;
                }
                c => (word.push(c), any = true).1,
            }
        }
        if any {
            words.push(word);
        }
        let mut words = words.into_iter();
        Some(Command { name: words.next()?, args: words.collect() })
    }

    /// Argument `i` as a number or whatever `T` parses.
    pub fn arg<T: std::str::FromStr>(&self, i: usize) -> Option<T> {
        self.args.get(i)?.parse().ok()
    }
}

const SCROLLBACK: usize = 200;

/// A command line over the game: the backquote key (`) opens it, Enter sends the line, Up and Down recall earlier
/// lines, Escape closes it. The console only reads and prints text; the game decides what a command does:
///
/// ```ignore
/// if let Some(cmd) = self.console.update(ctx.input()) {
///     match cmd.name.as_str() {
///         "spawn" => self.console.print(format!("spawned {}", cmd.arg::<u32>(0).unwrap_or(1))),
///         other => self.console.print(format!("unknown command {other}")),
///     }
/// }
/// if self.console.is_open() { return Trans::None; } // keep the game from reading the keys it was typed with
/// ```
pub struct Console {
    open: bool,
    field: TextField,
    lines: VecDeque<String>,
    history: Vec<String>,
    recall: Option<usize>,
}

impl Console {
    /// A closed console with no lines.
    pub fn new() -> Self {
        Console {
            open: false,
            field: TextField::new(0, 0, 120),
            lines: VecDeque::new(),
            history: Vec::new(),
            recall: None,
        }
    }

    /// True while the console is shown.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Adds a line to the console's output.
    pub fn print(&mut self, text: impl Into<String>) {
        self.lines.push_back(text.into());
        while self.lines.len() > SCROLLBACK {
            self.lines.pop_front();
        }
    }

    /// Call once per step. Returns the command a finished line holds.
    pub fn update(&mut self, input: &Input) -> Option<Command> {
        if input.key_pressed(Key::Backquote) {
            self.open = !self.open;
            // The backquote that was just pressed is also among the typed characters.
            return None;
        }
        if !self.open {
            return None;
        }
        if input.key_pressed(Key::Escape) {
            self.open = false;
            return None;
        }
        let back = input.key_pressed(Key::Up) as isize - input.key_pressed(Key::Down) as isize;
        if back != 0 && !self.history.is_empty() {
            let at = match (self.recall, back) {
                (None, _) => self.history.len() - 1,
                (Some(i), 1) => i.saturating_sub(1),
                (Some(i), _) => (i + 1).min(self.history.len() - 1),
            };
            self.recall = Some(at);
            self.field.set(&self.history[at].clone());
        }
        if !self.field.update(input) {
            return None;
        }
        let line = self.field.text().to_string();
        self.field.clear();
        self.recall = None;
        if line.trim().is_empty() {
            return None;
        }
        self.print(format!("> {line}"));
        self.history.push(line.clone());
        Command::parse(&line)
    }

    /// The lower half of the screen, with the newest lines and the line being typed.
    pub fn draw(&self, gfx: &mut Gfx, font: &crate::Font) {
        if !self.open {
            return;
        }
        let (row, h) = (font.line_height() + 2, gfx.h as i32 / 2);
        let top = gfx.h as i32 - h;
        gfx.fill_alpha(Rect::new(0.0, top as f32, gfx.w as f32, h as f32), 0x000000, 210);
        let room = (h / row - 1).max(0) as usize;
        let skip = self.lines.len().saturating_sub(room);
        for (i, line) in self.lines.iter().skip(skip).enumerate() {
            gfx.text_font(font, line, 4, top + 2 + i as i32 * row, 0xcccccc);
        }
        gfx.text_font(font, "> ", 4, gfx.h as i32 - row, 0x66ff66);
        let prompt = font.width("> ");
        self.field.draw_at(gfx, font, 4 + prompt, gfx.h as i32 - row, 0xffffff, true);
    }
}

impl Default for Console {
    fn default() -> Self {
        Console::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Audio, Collider, Look, Vel};

    #[test]
    fn a_line_splits_into_a_name_and_arguments_and_quotes_keep_spaces() {
        let c = Command::parse("  spawn 3  \"big box\" x ").unwrap();
        assert_eq!(
            (c.name.as_str(), c.args.as_slice()),
            ("spawn", &["3".to_string(), "big box".to_string(), "x".to_string()][..])
        );
        assert_eq!((c.arg::<u32>(0), c.arg::<u32>(1), c.arg::<u32>(9)), (Some(3), None, None));
        assert_eq!(Command::parse("\"\" a").unwrap().name, "", "an empty quoted word is a word");
        assert!(Command::parse("   ").is_none() && Command::parse("").is_none());
    }

    fn typed(input: &mut Input, keys: &[Key], text: &str) {
        input.end_step();
        input.hold(keys);
        input.type_chars(text);
    }

    #[test]
    fn the_console_opens_takes_lines_recalls_them_and_closes() {
        let (mut input, mut console) = (Input::default(), Console::new());
        typed(&mut input, &[Key::Backquote], "`");
        assert_eq!(console.update(&input), None);
        assert!(console.is_open());
        assert_eq!(console.field.text(), "", "the key that opened it is not typed into it");
        typed(&mut input, &[], "go 5");
        assert_eq!(console.update(&input), None);
        typed(&mut input, &[Key::Enter], "");
        assert_eq!(console.update(&input), Some(Command { name: "go".into(), args: vec!["5".into()] }));
        assert_eq!(console.lines.back().map(String::as_str), Some("> go 5"));
        typed(&mut input, &[Key::Up], "");
        console.update(&input);
        assert_eq!(console.field.text(), "go 5", "Up brings the last line back");
        typed(&mut input, &[Key::Escape], "");
        console.update(&input);
        assert!(!console.is_open());
        typed(&mut input, &[], "ignored");
        assert_eq!(console.update(&input), None, "a closed console reads nothing");
        for i in 0..SCROLLBACK + 20 {
            console.print(format!("line {i}"));
        }
        assert_eq!(console.lines.len(), SCROLLBACK);
    }

    #[test]
    fn the_inspector_picks_the_top_entity_and_lists_its_components() {
        #[derive(Debug)]
        #[expect(dead_code)]
        struct Health(u32);
        let mut world = World::new();
        let rect = |x| Rect { x, y: 0.0, w: 10.0, h: 10.0 };
        let under = world.spawn((Body(rect(0.0)), Layer(0)));
        let over = world.spawn((
            Body(rect(5.0)),
            Layer(3),
            Vel { x: 1.0, y: 2.0 },
            Look::Color(0xff0000),
            Health(7),
            Collider,
        ));
        let mut insp = Inspector::new();
        insp.register::<Health>("Health");
        assert!(!insp.is_open());
        let mut ctx = Ctx::new(64, 64, 1.0 / 60.0, Audio::disabled());
        ctx.input.hold(&[Key::F4]);
        insp.update(&ctx, &world);
        assert!(insp.is_open());
        ctx.input.end_step();
        ctx.input.hold(&[]);
        ctx.input.point(Some((7.0, 5.0)), true);
        insp.update(&ctx, &world);
        assert_eq!(insp.selected(), Some(over), "both are under the cursor; the higher layer wins");
        ctx.input.end_step();
        ctx.input.point(Some((2.0, 5.0)), false);
        ctx.input.end_step();
        ctx.input.point(Some((2.0, 5.0)), true);
        insp.update(&ctx, &world);
        assert_eq!(insp.selected(), Some(under));
        let lines = insp.describe(&world, over);
        for want in [
            "Body:",
            "Vel: Vel { x: 1.0, y: 2.0 }",
            "Layer: Layer(3)",
            "Look: color ff0000",
            "Health: Health(7)",
            "Collider:",
        ] {
            assert!(lines.iter().any(|l| l.starts_with(want)), "{want} in {lines:?}");
        }
        assert!(
            !lines.iter().any(|l| l.starts_with("Anim") || l.starts_with("Transform")),
            "components it lacks are left out"
        );
        world.despawn(over).unwrap();
        assert!(insp.describe(&world, over)[0].contains("gone"));
        let mut gfx = Gfx::new(64, 64);
        insp.select(Some(under));
        insp.draw(&mut gfx, &world);
        assert!(gfx.cmds().len() > 3, "an outline, a panel and text");
    }
}
