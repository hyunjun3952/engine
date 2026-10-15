use engine::{Button, Checkbox, Config, Ctx, Font, Gfx, Key, Menu, Rect, Scene, TextField, Theme, Timer, Trans};
use std::sync::Arc;

struct Text {
    font: Arc<Font>,
    field: TextField,
    menu: Menu,
    said: String,
    blink: Timer,
    clear: Button,
    korean: Checkbox,
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
        if self.clear.update(ctx.input()) {
            self.field.clear();
        }
        if self.korean.update(ctx.input()) {
            self.field.set_korean(self.korean.checked);
        }
        self.korean.checked = self.field.korean();
        if let Some(i) = self.menu.update(ctx.input(), &self.font, "up", "down", "accept") {
            self.said = format!("메뉴 {i} 선택");
        }
        Trans::None
    }

    fn draw(&self, g: &mut Gfx) {
        g.clear(0x101018);
        let f = &self.font;
        let mode = if self.field.korean() { "한" } else { "영" };
        g.text_font(f, &format!("[{mode}] 오른쪽 Alt: 한/영   Enter: 입력   Tab: 메뉴 선택"), 16, 16, 0xaaaaaa);
        let theme = Theme::default();
        self.field.draw(g, f, &theme, self.blink.fraction() < 0.5);
        self.menu.draw(g, f, &theme);
        self.clear.draw(g, f, &theme);
        self.korean.draw(g, f, &theme);
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
        let korean = Checkbox::new(&font, 300, 80, "한글 입력", false);
        Box::new(Text {
            font,
            field: TextField::new(16, 40, 20),
            menu: Menu::new(16, 80, &["시작", "설정", "끝"]),
            said: String::new(),
            blink: Timer::looping(1.0),
            clear: Button::new(Rect::new(300.0, 36.0, 60.0, 18.0), "지우기"),
            korean,
        })
    });
}
