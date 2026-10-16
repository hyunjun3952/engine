use crate::base::in_file;
use crate::base::save::{escape, unescape};
use crate::{Body, Collider, Entity, Filter, Layer, Rect, Transform, Vel, World};
use hecs::{EntityBuilder, EntityRef};
use std::collections::VecDeque;

/// A component's text form for a saved world. `read` gets back what `write` made; `None` means malformed.
///
/// ```
/// use engine::Persist;
///
/// struct Hp(i32);
///
/// impl Persist for Hp {
///     fn write(&self) -> String {
///         self.0.to_string()
///     }
///     fn read(text: &str) -> Option<Self> {
///         text.trim().parse().ok().map(Hp)
///     }
/// }
///
/// assert_eq!(Hp::read(&Hp(3).write()).unwrap().0, 3);
/// ```
pub trait Persist: Sized + Send + Sync + 'static {
    /// The text form of the value, on one line.
    fn write(&self) -> String;
    /// Parses what `write` made; `None` if it is malformed.
    fn read(text: &str) -> Option<Self>;
}

/// The parse_numbers of a line like `1 2.5 3`, exactly `N` of them.
pub(crate) fn parse_numbers<const N: usize>(text: &str) -> Option<[f32; N]> {
    let mut it = text.split_whitespace().map(|v| v.parse::<f32>().ok().filter(|v| v.is_finite()));
    let mut out = [0.0; N];
    for slot in &mut out {
        *slot = it.next()??;
    }
    it.next().is_none().then_some(out)
}

macro_rules! persist_by_parse {
    ($($t:ty),*) => {$(
        impl Persist for $t {
            fn write(&self) -> String {
                self.to_string()
            }
            fn read(text: &str) -> Option<Self> {
                text.trim().parse().ok()
            }
        }
    )*};
}
persist_by_parse!(f32, i32, u32, bool);

impl Persist for String {
    fn write(&self) -> String {
        self.clone()
    }
    fn read(text: &str) -> Option<Self> {
        Some(text.to_string())
    }
}

impl Persist for Body {
    fn write(&self) -> String {
        let r = self.0;
        format!("{} {} {} {}", r.x, r.y, r.w, r.h)
    }
    fn read(text: &str) -> Option<Self> {
        parse_numbers(text).map(|[x, y, w, h]| Body(Rect { x, y, w, h }))
    }
}

impl Persist for Vel {
    fn write(&self) -> String {
        format!("{} {}", self.x, self.y)
    }
    fn read(text: &str) -> Option<Self> {
        parse_numbers(text).map(|[x, y]| Vel { x, y })
    }
}

impl Persist for Collider {
    fn write(&self) -> String {
        String::new()
    }
    fn read(text: &str) -> Option<Self> {
        text.trim().is_empty().then_some(Collider)
    }
}

impl Persist for Layer {
    fn write(&self) -> String {
        self.0.to_string()
    }
    fn read(text: &str) -> Option<Self> {
        text.trim().parse().ok().map(Layer)
    }
}

impl Persist for Filter {
    fn write(&self) -> String {
        format!("{} {}", self.group, self.mask)
    }
    fn read(text: &str) -> Option<Self> {
        let mut it = text.split_whitespace().map(|v| v.parse::<u32>().ok());
        let (group, mask) = (it.next()??, it.next()??);
        it.next().is_none().then_some(Filter { group, mask })
    }
}

impl Persist for Transform {
    fn write(&self) -> String {
        format!("{} {} {}", self.angle, self.sx, self.sy)
    }
    fn read(text: &str) -> Option<Self> {
        parse_numbers(text).map(|[angle, sx, sy]| Transform { angle, sx, sy })
    }
}

type Reader = Box<dyn Fn(&mut EntityBuilder, &str) -> Option<()>>;

struct Entry {
    name: String,
    write: Box<dyn Fn(EntityRef<'_>) -> Option<String>>,
    read: Reader,
}

/// Saves and loads the entities of a `World` as text: a `[entity]` line, then one `name = value` line for each of
/// the registered components the entity has.
///
/// Only registered components are written, and an entity with none of them is left out. Not saved: what is not a
/// registered component (the physics state such as sleep and joints, `Look::Sprite`, animation), and links between
/// entities (an `Entity` id is not stable across a load).
///
/// ```
/// use engine::{Vel, World, WorldFile};
///
/// let file = WorldFile::new().component::<Vel>("vel");
/// let mut world = World::new();
/// world.spawn((Vel { x: 1.0, y: 2.0 },));
/// let text = file.to_text(&world);
///
/// let mut copy = World::new();
/// assert_eq!(file.spawn_from(&mut copy, &text).unwrap().len(), 1);
/// assert_eq!(file.to_text(&copy), text);
/// ```
#[derive(Default)]
pub struct WorldFile {
    entries: Vec<Entry>,
}

impl WorldFile {
    /// A file that saves no component yet.
    pub fn new() -> Self {
        WorldFile::default()
    }

    /// Registers `T` under `name` in the file. Panics on a name that is empty, has `=`, `#`, `[` or a line break, or
    /// was used before.
    pub fn component<T: Persist>(mut self, name: &str) -> Self {
        let bad = name.is_empty() || name.contains(['=', '#', '[', '\n', '\r']) || name.trim() != name;
        assert!(!bad, "bad component name {name:?}");
        assert!(self.entries.iter().all(|e| e.name != name), "component {name:?} is registered twice");
        self.entries.push(Entry {
            name: name.to_string(),
            write: Box::new(|e| e.get::<&T>().map(|c| c.write())),
            read: Box::new(|b, text| {
                b.add(T::read(text)?);
                Some(())
            }),
        });
        self
    }

    /// One `[entity]` block per entity with a registered component.
    pub fn to_text(&self, world: &World) -> String {
        let mut ents: Vec<EntityRef<'_>> = world.iter().collect();
        ents.sort_by_key(|e| e.entity().to_bits());
        let mut out = String::new();
        for e in ents {
            let lines: Vec<String> = self
                .entries
                .iter()
                .filter_map(|en| (en.write)(e).map(|v| format!("{} = {}\n", en.name, escape(&v))))
                .collect();
            if !lines.is_empty() {
                out += "[entity]\n";
                out += &lines.concat();
            }
        }
        out
    }

    /// A 32-bit FNV-1a hash of `to_text(world)`: equal worlds (as far as the registered components go) give equal
    /// sums, so two runs (a replay against its recording, say) can be compared cheaply.
    pub fn checksum(&self, world: &World) -> u32 {
        self.to_text(world).bytes().fold(0x811c_9dc5, |h, b| (h ^ b as u32).wrapping_mul(0x0100_0193))
    }

    /// Adds the entities in `text` to `world` (clear it first to replace what is there) and returns them. A line that
    /// names an unregistered component, repeats one, or has a value that does not read is an error, and then nothing
    /// is added.
    pub fn spawn_from(&self, world: &mut World, text: &str) -> Result<Vec<Entity>, String> {
        let mut builders: Vec<EntityBuilder> = Vec::new();
        let mut seen: Vec<&str> = Vec::new();
        for (i, line) in text.lines().enumerate() {
            // Only the left is trimmed for the value: trailing spaces belong to it.
            let (n, line) = (i + 1, line.trim_start());
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            if line.trim_end() == "[entity]" {
                builders.push(EntityBuilder::new());
                seen.clear();
                continue;
            }
            let (name, value) =
                line.split_once('=').ok_or(format!("line {n}: expected `name = value` or `[entity]`"))?;
            let (name, value) = (name.trim(), unescape(value.strip_prefix(' ').unwrap_or(value)));
            let builder = builders.last_mut().ok_or(format!("line {n}: a component before any `[entity]`"))?;
            let entry =
                self.entries.iter().find(|e| e.name == name).ok_or(format!("line {n}: unknown component {name:?}"))?;
            if seen.contains(&entry.name.as_str()) {
                return Err(format!("line {n}: {name:?} appears twice in one entity"));
            }
            seen.push(&entry.name);
            (entry.read)(builder, &value).ok_or(format!("line {n}: {name:?} cannot read {value:?}"))?;
        }
        Ok(builders.iter_mut().map(|b| world.spawn(b.build())).collect())
    }

    /// Writes beside the target and renames, so a crash mid-write cannot destroy the previous file. Text that would
    /// not load back (a NaN position, say) is an error and the previous file stays.
    pub fn save(&self, world: &World, path: &str) -> Result<(), String> {
        let text = self.to_text(world);
        self.spawn_from(&mut World::new(), &text).map_err(|e| format!("{path}: not saved, it would not load: {e}"))?;
        crate::base::write_atomic(path, &text)
    }

    /// `spawn_from` the text of a file; a missing file is an error here, unlike `Save`.
    pub fn load(&self, world: &mut World, path: &str) -> Result<Vec<Entity>, String> {
        let text = crate::base::read_text(path)?;
        self.spawn_from(world, &text).map_err(in_file(path))
    }
}

/// The last `cap` snapshots of a world, to step back through: rewind, checkpoints. Call `record` once per fixed step
/// (or every few) and `back` to restore. The world is replaced as `WorldFile` saves it, so entities are new, and what
/// it does not save is lost (an entity with no registered component, physics sleep and joints): rebuild `Physics` after.
///
/// ```
/// use engine::{Rewind, Vel, World, WorldFile};
///
/// let file = WorldFile::new().component::<Vel>("vel");
/// let mut world = World::new();
/// let e = world.spawn((Vel { x: 1.0, y: 0.0 },));
/// let mut rewind = Rewind::new(4);
/// rewind.record(&file, &world);
/// world.get::<&mut Vel>(e).unwrap().x = 9.0;
/// rewind.record(&file, &world);
///
/// rewind.back(&file, &mut world, 2).unwrap();
/// let x = world.query::<&Vel>().iter().next().unwrap().x;
/// assert_eq!(x, 1.0);
/// ```
pub struct Rewind {
    frames: VecDeque<String>,
    cap: usize,
}

impl Rewind {
    /// Keeps at most `cap` snapshots.
    pub fn new(cap: usize) -> Self {
        Rewind { frames: VecDeque::new(), cap: cap.max(1) }
    }

    /// How many snapshots are kept.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// True when there is nothing to go back to.
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Stores a snapshot of `world`; the oldest goes when full.
    pub fn record(&mut self, file: &WorldFile, world: &World) {
        if self.frames.len() == self.cap {
            self.frames.pop_front();
        }
        self.frames.push_back(file.to_text(world));
    }

    /// Goes `steps` snapshots back (at least one, at most all there are): the world's entities are replaced by the ones
    /// saved then and returned, and the snapshots after it are dropped. An `Err` leaves the world and the snapshots as
    /// they were.
    pub fn back(&mut self, file: &WorldFile, world: &mut World, steps: usize) -> Result<Vec<Entity>, String> {
        let at = self.frames.len().checked_sub(steps.clamp(1, self.frames.len().max(1))).filter(|_| !self.is_empty());
        let at = at.ok_or("no snapshot to go back to")?;
        let old: Vec<Entity> = world.iter().map(|e| e.entity()).collect();
        let made = file.spawn_from(world, &self.frames[at])?;
        old.into_iter().for_each(|e| drop(world.despawn(e)));
        self.frames.truncate(at);
        Ok(made)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RigidBody;

    #[derive(Debug, PartialEq)]
    struct Hp(u32, String);

    impl Persist for Hp {
        fn write(&self) -> String {
            format!("{} {}", self.0, self.1)
        }
        fn read(text: &str) -> Option<Self> {
            let (n, name) = text.split_once(' ')?;
            Some(Hp(n.parse().ok()?, name.to_string()))
        }
    }

    fn file() -> WorldFile {
        WorldFile::new()
            .component::<Body>("body")
            .component::<Vel>("vel")
            .component::<Transform>("turn")
            .component::<Filter>("filter")
            .component::<Layer>("layer")
            .component::<Collider>("collider")
            .component::<Hp>("hp")
    }

    #[test]
    fn a_world_survives_a_round_trip_with_awkward_text_and_values() {
        let mut w = World::new();
        let a = w.spawn((
            Body(Rect::new(1.5, -2.0, 3.25, 4.0)),
            Vel { x: 0.1, y: -300.0 },
            Transform { angle: 0.7, sx: 1.0, sy: -1.0 },
            Filter { group: 2, mask: u32::MAX },
            Layer(-3),
            Collider,
            Hp(9, "한글 = a # b\nsecond  ".into()),
        ));
        w.spawn((Hp(1, "x".into()),));
        w.spawn((5u8,));
        let text = file().to_text(&w);
        assert_eq!(text.matches("[entity]").count(), 2, "an entity with no registered component is left out");
        let mut back = World::new();
        let made = file().spawn_from(&mut back, &text).unwrap();
        assert_eq!(made.len(), 2);
        let e = made.iter().copied().find(|&e| back.get::<&Collider>(e).is_ok()).unwrap();
        assert_eq!(back.get::<&Body>(e).unwrap().0, w.get::<&Body>(a).unwrap().0);
        assert_eq!(back.get::<&Hp>(e).unwrap().1, "한글 = a # b\nsecond  ");
        assert_eq!(back.get::<&Layer>(e).unwrap().0, -3);
        assert_eq!(*back.get::<&Transform>(e).unwrap(), *w.get::<&Transform>(a).unwrap());
        assert_eq!(file().to_text(&back), text, "saving what was loaded gives the same text");
    }

    #[test]
    fn a_world_that_would_not_load_back_is_not_saved_and_the_old_file_stays() {
        let path = std::env::temp_dir().join(format!("engine-persist-nan-{}.txt", std::process::id()));
        let path = path.to_str().unwrap();
        let mut w = World::new();
        let e = w.spawn((Body(Rect::new(1.0, 2.0, 3.0, 4.0)),));
        file().save(&w, path).unwrap();
        let good = std::fs::read_to_string(path).unwrap();
        w.get::<&mut Body>(e).unwrap().0.x = f32::NAN;
        let err = file().save(&w, path).unwrap_err();
        assert!(err.contains("not saved") && err.contains("body"), "{err}");
        assert_eq!(std::fs::read_to_string(path).unwrap(), good);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_bad_file_names_its_line_and_adds_nothing() {
        let mut w = World::new();
        let f = file();
        let err = |text: &str| f.spawn_from(&mut World::new(), text).unwrap_err();
        assert!(err("[entity]\nvel = 1 2\nspeed = 3\n").starts_with("line 3: unknown component"));
        assert!(err("[entity]\nvel = 1\n").starts_with("line 2"));
        assert!(err("[entity]\nvel = 1 2\nvel = 3 4\n").contains("twice"));
        assert!(err("vel = 1 2\n").starts_with("line 1: a component before"));
        assert!(err("[entity]\njunk\n").starts_with("line 2"));
        assert!(f.spawn_from(&mut w, "[entity]\nvel = 1 2\n[entity]\nvel = x\n").is_err());
        assert_eq!(w.len(), 0, "the first entity was valid, but nothing is added when any line is bad");
    }

    #[test]
    fn the_checksum_follows_the_registered_state_and_nothing_else() {
        let mut w = World::new();
        let e = w.spawn((Vel { x: 1.0, y: 2.0 }, 7u8));
        let f = file();
        let before = f.checksum(&w);
        assert_eq!(f.checksum(&w), before, "it is repeatable");
        w.insert_one(e, 9u8).unwrap();
        assert_eq!(f.checksum(&w), before, "an unregistered component does not count");
        w.get::<&mut Vel>(e).unwrap().x = 1.5;
        assert_ne!(f.checksum(&w), before);
    }

    #[test]
    fn comments_blank_lines_and_a_second_entity_with_the_same_components_are_fine() {
        let mut w = World::new();
        let text = "# a level\n\n[entity]\nvel = 1 2\n\n[entity]\n# again\nvel = 3 4\n";
        assert_eq!(file().spawn_from(&mut w, text).unwrap().len(), 2);
    }

    #[test]
    fn save_and_load_go_through_a_file_and_a_missing_file_is_an_error() {
        let path = std::env::temp_dir().join(format!("engine-world-{}.txt", std::process::id()));
        let path = path.to_str().unwrap();
        let mut w = World::new();
        w.spawn((Vel { x: 7.0, y: 8.0 },));
        file().save(&w, path).unwrap();
        let mut back = World::new();
        let made = file().load(&mut back, path).unwrap();
        assert_eq!((back.get::<&Vel>(made[0]).unwrap().x, back.len()), (7.0, 1));
        let _ = std::fs::remove_file(path);
        assert!(file().load(&mut World::new(), path).is_err());
    }

    #[test]
    fn a_rigid_body_keeps_its_kind_and_numbers() {
        let mut w = World::new();
        let f = WorldFile::new().component::<RigidBody>("rb").component::<Body>("body");
        let shapes = [
            RigidBody::dynamic(2.0).circle().bounce(0.3).friction(0.7).spin(1.5),
            RigidBody::fixed().one_way(),
            RigidBody::kinematic(),
        ];
        for rb in shapes {
            w.spawn((Body(Rect::new(0.0, 0.0, 1.0, 1.0)), rb));
        }
        let text = f.to_text(&w);
        let mut back = World::new();
        f.spawn_from(&mut back, &text).unwrap();
        assert_eq!(f.to_text(&back), text);
        assert_eq!(back.len(), 3);
    }

    #[test]
    #[should_panic(expected = "registered twice")]
    fn registering_a_name_twice_panics() {
        WorldFile::new().component::<Vel>("v").component::<Body>("v");
    }

    #[test]
    fn rewind_restores_an_older_world_and_keeps_only_the_last_few() {
        let (f, mut w, mut r) = (file(), World::new(), Rewind::new(3));
        assert!(r.back(&f, &mut w, 1).is_err());
        let e = w.spawn((Vel { x: 0.0, y: 0.0 },));
        w.spawn((5u8,));
        for x in 1..=5 {
            w.get::<&mut Vel>(e).unwrap().x = x as f32;
            r.record(&f, &w);
        }
        assert_eq!(r.len(), 3, "snapshots 3, 4 and 5 are left");
        let made = r.back(&f, &mut w, 2).unwrap();
        assert_eq!((made.len(), w.len()), (1, 1), "every old entity goes, also the one with nothing registered");
        assert_eq!(w.get::<&Vel>(made[0]).unwrap().x, 4.0);
        assert_eq!(r.len(), 1);
        r.back(&f, &mut w, 99).unwrap();
        assert_eq!(w.get::<&Vel>(w.iter().find(hecs::EntityRef::has::<Vel>).unwrap().entity()).unwrap().x, 3.0);
        assert!(r.is_empty());
    }
}
