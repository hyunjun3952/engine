use crate::{Entity, TiledObject, World};

type Maker = Box<dyn Fn(&mut World, &TiledObject) -> Entity>;

/// Turns the objects of a Tiled map into entities: one function per object kind.
#[derive(Default)]
pub struct Spawner {
    makers: Vec<(String, Option<Maker>)>,
}

impl Spawner {
    pub fn new() -> Self {
        Spawner::default()
    }

    /// Objects of `kind` become whatever entity `make` builds from them.
    pub fn on(mut self, kind: &str, make: impl Fn(&mut World, &TiledObject) -> Entity + 'static) -> Self {
        self.makers.push((kind.to_string(), Some(Box::new(make))));
        self
    }

    /// Objects of `kind` stay data for the game (a spawn point, say) and make no entity.
    pub fn skip(mut self, kind: &str) -> Self {
        self.makers.push((kind.to_string(), None));
        self
    }

    /// Spawns every object in order and returns the entities made. A kind nobody handles is an error naming it and
    /// nothing is spawned, so a typo in the map editor is not silently dropped.
    pub fn spawn(&self, world: &mut World, objects: &[TiledObject]) -> Result<Vec<Entity>, String> {
        let find = |o: &TiledObject| self.makers.iter().rev().find(|m| m.0 == o.kind).map(|m| &m.1);
        if let Some(o) = objects.iter().find(|o| find(o).is_none()) {
            return Err(format!("object {:?} has kind {:?}, which nothing spawns", o.name, o.kind));
        }
        Ok(objects.iter().filter_map(|o| find(o).and_then(Option::as_ref).map(|make| make(world, o))).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Body, Rect};

    fn object(name: &str, kind: &str, x: f32) -> TiledObject {
        TiledObject {
            name: name.into(),
            kind: kind.into(),
            x,
            y: 10.0,
            w: 8.0,
            h: 8.0,
            props: vec![("hp".into(), "3".into())],
        }
    }

    fn spawner() -> Spawner {
        Spawner::new()
            .on("coin", |w, o| w.spawn((Body(o.rect()),)))
            .on("enemy", |w, o| {
                let hp: u32 = o.prop("hp").and_then(|v| v.parse().ok()).unwrap_or(1);
                w.spawn((Body(o.rect()), hp))
            })
            .skip("spawn")
    }

    #[test]
    fn each_object_becomes_the_entity_its_kind_builds_in_order() {
        let mut w = World::new();
        let objects = [object("a", "coin", 1.0), object("start", "spawn", 5.0), object("b", "enemy", 2.0)];
        let made = spawner().spawn(&mut w, &objects).unwrap();
        assert_eq!(made.len(), 2, "the skipped kind makes none");
        assert_eq!(w.get::<&Body>(made[0]).unwrap().0, Rect { x: 1.0, y: 10.0, w: 8.0, h: 8.0 });
        assert_eq!(*w.get::<&u32>(made[1]).unwrap(), 3, "properties reach the maker");
    }

    #[test]
    fn an_unknown_kind_is_an_error_and_spawns_nothing() {
        let mut w = World::new();
        let err = spawner().spawn(&mut w, &[object("a", "coin", 1.0), object("typo", "enmey", 2.0)]).unwrap_err();
        assert!(err.contains("typo") && err.contains("enmey"), "{err}");
        assert_eq!(w.len(), 0);
    }

    #[test]
    fn a_later_handler_for_a_kind_replaces_an_earlier_one() {
        let mut w = World::new();
        let s = Spawner::new().on("x", |w, _| w.spawn((1u8,))).on("x", |w, _| w.spawn((2u8,)));
        let made = s.spawn(&mut w, &[object("o", "x", 0.0)]).unwrap();
        assert_eq!(*w.get::<&u8>(made[0]).unwrap(), 2);
    }
}
