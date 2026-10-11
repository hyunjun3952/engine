use crate::{Anim, Body, Collider, DrawCmd, Layer, Look, Paint, Transform, World};

pub fn draw_list(world: &World) -> Vec<DrawCmd> {
    let mut cmds = Vec::new();
    let mut query = world.query::<(&Body, Option<&Layer>, Option<&Look>, Option<&Anim>, Option<&Transform>)>();
    for (body, layer, look, anim, xform) in query.iter() {
        let (layer, rect, xform) = (layer.map_or(0, |l| l.0), body.0, xform.copied().unwrap_or_default());
        if let Some(look) = look {
            let paint = match look {
                Look::Color(c) => Paint::Color(*c),
                Look::Alpha(c, a) => Paint::Alpha(*c, *a),
                Look::Sprite(s) => Paint::Sprite(s.clone()),
            };
            cmds.push(DrawCmd { layer, rect, paint, debug_only: false, xform });
        }
        if let Some(a) = anim {
            let paint = Paint::Region { sheet: a.sheet.clone(), src: a.src(), flip: a.flip };
            cmds.push(DrawCmd { layer, rect, paint, debug_only: false, xform });
        }
    }
    cmds.sort_by_key(|c| c.layer);
    for (body, _) in world.query::<(&Body, &Collider)>().iter() {
        let outline = Paint::Outline(0x00ff00);
        cmds.push(DrawCmd {
            layer: i32::MAX,
            rect: body.0,
            paint: outline,
            debug_only: true,
            xform: Transform::IDENTITY,
        });
    }
    cmds
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rect;

    #[test]
    fn a_transform_component_reaches_the_command_and_others_get_the_identity() {
        let mut w = World::new();
        let body = || Body(Rect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 });
        let spun = Transform { angle: 1.0, sx: 2.0, sy: 3.0 };
        w.spawn((body(), Look::Color(1), spun));
        w.spawn((body(), Look::Color(2)));
        let cmds = draw_list(&w);
        let by_color = |c: u32| cmds.iter().find(|d| matches!(d.paint, Paint::Color(x) if x == c)).unwrap().xform;
        assert_eq!((by_color(1), by_color(2)), (spun, Transform::IDENTITY));
    }

    #[test]
    fn higher_layers_come_later_regardless_of_spawn_order_and_colliders_add_debug_outlines() {
        let mut w = World::new();
        let square = |c| (Body(Rect { x: 0.0, y: 0.0, w: 4.0, h: 4.0 }), Look::Color(c));
        let (top, bottom) = (square(0xff0000), square(0x0000ff));
        w.spawn((top.0, top.1, Layer(5), Collider));
        w.spawn((bottom.0, bottom.1));
        let cmds = draw_list(&w);
        let layers: Vec<_> = cmds.iter().map(|c| c.layer).collect();
        assert_eq!(layers, [0, 5, i32::MAX]);
        assert!(cmds[2].debug_only && !cmds[0].debug_only);
    }
}
