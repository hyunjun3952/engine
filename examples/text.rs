use engine::{Config, Ctx, Font, Gfx, Key, Menu, Scene, TextField, Timer, Trans};
use std::sync::Arc;

struct Text {
    font: Arc<Font>,
    field: TextField,
    menu: Menu,
    said: String,
    blink: Timer,
}

impl Scene for Text {
    fn update(&mut self, ctx: &mut Ctx) -> Trans {
        if ctx.input().pressed("back") {
            return Trans::Quit;
        }
        self.blink.tick(ctx.dt());
        if self.field.update(ctx.input()) {
            self.said = format!("입력: {}", self.field.text());
            self.field.clear();
        }
        if let Some(i) = self.menu.update(ctx.input(), "up", "down", "accept") {
            self.said = format!("메뉴 {i} 선택");
        }
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        let f = &self.font;
        g.text_font(f, "글자를 입력하고 Enter (한글은 OS 입력기가 보낸 글자만)", 16, 16, 0xaaaaaa);
        self.field.draw(g, f, 16, 40, 0xffffff, self.blink.fraction() < 0.5);
        self.menu.draw(g, f, 16, 80, 0xcccccc, 0xffd040);
        g.text_font_scaled(f, &self.said, 16, 180, 2, 0x80ff80);
    }
}

fn main() {
    engine::run(Config { title: "text", scale: 2, ..Config::default() }, |ctx| {
        ctx.bind("up", &[Key::Up]);
        ctx.bind("down", &[Key::Down]);
        ctx.bind("accept", &[Key::Tab]);
        ctx.bind("back", &[Key::Escape]);
        let font = ctx.assets().font("assets/fonts/galmuri9.bdf");
        Box::new(Text {
            font,
            field: TextField::new(20),
            menu: Menu::new(&["시작", "설정", "끝"]),
            said: String::new(),
            blink: Timer::looping(1.0),
        })
    });
}
