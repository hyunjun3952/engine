use crate::{Anim, Body, Collider, Gfx, Layer, Look, Rect, World};

enum Draw<'a> {
    Look(&'a Look),
    Anim(&'a Anim),
}

pub fn render(world: &World, gfx: &mut Gfx) {
    let (cx, cy) = gfx.cam;
    let view = |r: &Rect| Rect { x: r.x - cx, y: r.y - cy, ..*r };
    let mut query = world.query::<(&Body, Option<&Layer>, Option<&Look>, Option<&Anim>)>();
    let mut items = Vec::new();
    for (body, layer, look, anim) in query.iter() {
        let (z, r) = (layer.map_or(0, |l| l.0), view(&body.0));
        if let Some(l) = look {
            items.push((z, r, Draw::Look(l)));
        }
        if let Some(a) = anim {
            items.push((z, r, Draw::Anim(a)));
        }
    }
    items.sort_by_key(|i| i.0);
    for (_, r, draw) in items {
        match draw {
            Draw::Look(Look::Color(c)) => gfx.fill(&r, *c),
            Draw::Look(Look::Alpha(c, a)) => gfx.fill_alpha(&r, *c, *a),
            Draw::Look(Look::Sprite(s)) => gfx.blit(s, r.x as i32, r.y as i32),
            Draw::Anim(a) => gfx.blit_region(&a.sheet, a.src(), r.x as i32, r.y as i32, a.flip),
        }
    }
    if gfx.debug {
        for (b, _) in world.query::<(&Body, &Collider)>().iter() {
            gfx.outline(&view(&b.0), 0x00ff00);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn higher_layers_draw_on_top_regardless_of_spawn_order() {
        let mut w = World::new();
        let square = |c| (Body(Rect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 }), Look::Color(c));
        let (top, bottom) = (square(0xff0000), square(0x0000ff));
        w.spawn((top.0, top.1, Layer(5)));
        w.spawn((bottom.0, bottom.1));
        let mut g = Gfx::new(4, 4);
        render(&w, &mut g);
        assert_eq!(g.buf[0], 0xff0000);
    }
}
