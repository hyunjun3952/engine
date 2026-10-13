use super::tilemap::Tilemap;
use crate::{Rect, Sprite};
use std::sync::Arc;

struct Node {
    name: String,
    attrs: Vec<(String, String)>,
    kids: Vec<Node>,
    text: String,
}

impl Node {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|a| a.0 == name).map(|a| a.1.as_str())
    }

    fn kids<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.kids.iter().filter(move |k| k.name == name)
    }

    fn number<T: std::str::FromStr>(&self, name: &str) -> Result<T, String> {
        let v = self.attr(name).ok_or_else(|| format!("<{}> has no {name}", self.name))?;
        v.parse().map_err(|_| format!("<{}> {name}={v:?} is not a number", self.name))
    }

    // The `<properties>` of an element as (name, value) pairs.
    fn properties(&self) -> Vec<(String, String)> {
        self.kids("properties")
            .flat_map(|p| p.kids("property"))
            .filter_map(|p| {
                Some((p.attr("name")?.to_string(), p.attr("value").map_or(p.text.trim(), |v| v).to_string()))
            })
            .collect()
    }
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

// A reader for the part of XML a Tiled file uses: elements, attributes, text, comments and the prolog.
struct Reader<'a> {
    src: &'a str,
    at: usize,
}

impl<'a> Reader<'a> {
    fn err(&self, why: &str) -> String {
        format!("line {}: {why}", self.src[..self.at.min(self.src.len())].matches('\n').count() + 1)
    }

    fn rest(&self) -> &'a str {
        &self.src[self.at..]
    }

    fn skip_space(&mut self) {
        self.at += self.rest().len() - self.rest().trim_start().len();
    }

    // Past a `<?...?>`, `<!--...-->` or `<!DOCTYPE ...>`, if one is next.
    fn skip_misc(&mut self) -> Result<bool, String> {
        for (open, close) in [("<?", "?>"), ("<!--", "-->"), ("<!", ">")] {
            if self.rest().starts_with(open) {
                let end = self.rest().find(close).ok_or_else(|| self.err(&format!("`{open}` is never closed")))?;
                self.at += end + close.len();
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn name(&mut self) -> Result<&'a str, String> {
        let len =
            self.rest().find(|c: char| c.is_whitespace() || matches!(c, '/' | '>' | '=')).unwrap_or(self.rest().len());
        if len == 0 {
            return Err(self.err("a name was expected"));
        }
        let name = &self.rest()[..len];
        self.at += len;
        Ok(name)
    }

    fn element(&mut self) -> Result<Node, String> {
        self.at += 1;
        let mut node =
            Node { name: self.name()?.to_string(), attrs: Vec::new(), kids: Vec::new(), text: String::new() };
        loop {
            self.skip_space();
            if self.rest().starts_with("/>") {
                self.at += 2;
                return Ok(node);
            }
            if self.rest().starts_with('>') {
                self.at += 1;
                break;
            }
            let key = self.name()?.to_string();
            self.skip_space();
            if !self.rest().starts_with('=') {
                return Err(self.err(&format!("attribute {key} has no value")));
            }
            self.at += 1;
            self.skip_space();
            let quote = self
                .rest()
                .chars()
                .next()
                .filter(|c| matches!(c, '"' | '\''))
                .ok_or_else(|| self.err("a quote was expected"))?;
            let end = self.rest()[1..].find(quote).ok_or_else(|| self.err("a quote is never closed"))?;
            node.attrs.push((key, unescape(&self.rest()[1..1 + end])));
            self.at += end + 2;
        }
        loop {
            let text_end =
                self.rest().find('<').ok_or_else(|| self.err(&format!("<{}> is never closed", node.name)))?;
            node.text.push_str(&unescape(&self.rest()[..text_end]));
            self.at += text_end;
            if self.skip_misc()? {
                continue;
            }
            if self.rest().starts_with("</") {
                self.at += 2;
                let close = self.name()?;
                if close != node.name {
                    return Err(self.err(&format!("</{close}> closes <{}>", node.name)));
                }
                self.skip_space();
                if !self.rest().starts_with('>') {
                    return Err(self.err("`>` was expected"));
                }
                self.at += 1;
                return Ok(node);
            }
            node.kids.push(self.element()?);
        }
    }
}

fn parse_xml(src: &str) -> Result<Node, String> {
    let mut r = Reader { src: src.strip_prefix('\u{feff}').unwrap_or(src), at: 0 };
    loop {
        r.skip_space();
        if !r.skip_misc()? {
            break;
        }
    }
    if !r.rest().starts_with('<') {
        return Err(r.err("this is not XML"));
    }
    r.element()
}

/// A thing placed on an object layer in Tiled: a spawn point, a trigger area, a sign. Positions are pixels, from
/// the top left (for a point `w` and `h` are 0).
#[derive(Clone, Debug, PartialEq)]
pub struct TiledObject {
    pub name: String,
    /// The object's type (`class` in newer versions of Tiled).
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub props: Vec<(String, String)>,
}

impl TiledObject {
    pub fn rect(&self) -> Rect {
        Rect { x: self.x, y: self.y, w: self.w, h: self.h }
    }

    pub fn prop(&self, name: &str) -> Option<&str> {
        self.props.iter().find(|p| p.0 == name).map(|p| p.1.as_str())
    }
}

/// A map made in Tiled (`.tmx`), as one `Tilemap` per tile layer plus the objects of every object layer.
///
/// The map must be orthogonal, finite, with square tiles and one tileset, and its layers saved as CSV. The sheet
/// you pass is the tileset's image (its tiles number from 1 along each row, as Tiled counts them from 0), at most 255
/// tiles. A tile is solid when it has a bool property `solid`, and is drawn over entities when it has `front`.
/// Flipped or rotated tiles are not supported.
pub struct Tiled {
    pub layers: Vec<(String, Tilemap)>,
    pub objects: Vec<TiledObject>,
}

impl Tiled {
    pub fn load(path: &str, sheet: Arc<Sprite>) -> Result<Tiled, String> {
        let text = crate::assets::read_text(path)?;
        let dir = std::path::Path::new(path).parent().map(std::path::Path::to_path_buf).unwrap_or_default();
        let read = |source: &str| crate::assets::read_text(&dir.join(source).to_string_lossy());
        Tiled::parse(&text, read, sheet).map_err(|e| format!("{path}: {e}"))
    }

    /// Like `load` from text, with `read` giving the text of an external tileset file (`.tsx`) by its `source`.
    pub fn parse(
        text: &str,
        mut read: impl FnMut(&str) -> Result<String, String>,
        sheet: Arc<Sprite>,
    ) -> Result<Tiled, String> {
        let map = parse_xml(text)?;
        if map.name != "map" {
            return Err(format!("the root is <{}>, not <map>", map.name));
        }
        if map.attr("orientation").is_some_and(|o| o != "orthogonal") || map.attr("infinite") == Some("1") {
            return Err("only finite orthogonal maps are supported".into());
        }
        let (w, h): (usize, usize) = (map.number("width")?, map.number("height")?);
        let (tw, th): (usize, usize) = (map.number("tilewidth")?, map.number("tileheight")?);
        if tw != th || tw == 0 {
            return Err(format!("tiles are {tw}x{th}; only square tiles are supported"));
        }
        let sets: Vec<&Node> = map.kids("tileset").collect();
        let [set] = sets[..] else {
            return Err(format!("the map has {} tilesets; exactly one is supported", sets.len()));
        };
        let first: u32 = set.number("firstgid")?;
        let external;
        let tileset = match set.attr("source") {
            Some(source) => {
                external = parse_xml(&read(source)?).map_err(|e| format!("{source}: {e}"))?;
                &external
            }
            None => set,
        };
        let count = {
            let (sw, sh) = sheet.size();
            ((sw / tw) * (sh / th).max(1)).min(255)
        };
        let mut solid = [false; 256];
        let mut front = [false; 256];
        for tile in tileset.kids("tile") {
            let id: usize = tile.number("id")?;
            if let Some(n) = (id + 1 < 256).then_some(id + 1) {
                for (name, value) in tile.properties() {
                    match name.as_str() {
                        "solid" => solid[n] = value == "true",
                        "front" => front[n] = value == "true",
                        _ => {}
                    }
                }
            }
        }
        let mut layers = Vec::new();
        for layer in map.kids("layer") {
            let name = layer.attr("name").unwrap_or("").to_string();
            let what = |why: String| format!("layer {name:?}: {why}");
            let (lw, lh): (usize, usize) = (layer.number("width")?, layer.number("height")?);
            if (lw, lh) != (w, h) {
                return Err(what(format!("is {lw}x{lh} but the map is {w}x{h}")));
            }
            let data = layer.kids("data").next().ok_or_else(|| what("has no <data>".into()))?;
            let encoding = data.attr("encoding").unwrap_or("xml");
            if encoding != "csv" {
                return Err(what(format!("is saved as {encoding}; save the layers as CSV in Tiled")));
            }
            let gids = data
                .text
                .split(|c: char| c == ',' || c.is_whitespace())
                .filter(|v| !v.is_empty())
                .map(|v| v.parse::<u32>().map_err(|_| what(format!("{v:?} is not a tile number"))))
                .collect::<Result<Vec<_>, _>>()?;
            if gids.len() != w * h {
                return Err(what(format!("has {} tiles, expected {}", gids.len(), w * h)));
            }
            let mut tiles = Vec::with_capacity(gids.len());
            for (i, &gid) in gids.iter().enumerate() {
                let at = format!("tile ({}, {})", i % w, i / w);
                if gid & 0xF000_0000 != 0 {
                    return Err(what(format!("{at} is flipped or rotated, which is not supported")));
                }
                tiles.push(match gid {
                    0 => 0,
                    g if g < first => {
                        return Err(what(format!("{at} is {g}, before the tileset's first tile {first}")));
                    }
                    g => match u8::try_from(g - first + 1) {
                        Ok(id) if id as usize <= count => id,
                        _ => return Err(what(format!("{at} is {g}, past the {count} tiles of the sheet"))),
                    },
                });
            }
            let map = Tilemap::from_ids(w, h, tw, sheet.clone(), tiles, |id| (solid[id as usize], front[id as usize]));
            layers.push((name, map));
        }
        let mut objects = Vec::new();
        for group in map.kids("objectgroup") {
            for o in group.kids("object") {
                let number = |name: &str| {
                    o.attr(name)
                        .map_or(Ok(0.0), |v| v.parse().map_err(|_| format!("object {name}={v:?} is not a number")))
                };
                objects.push(TiledObject {
                    name: o.attr("name").unwrap_or("").to_string(),
                    kind: o.attr("type").or(o.attr("class")).unwrap_or("").to_string(),
                    x: number("x")?,
                    y: number("y")?,
                    w: number("width")?,
                    h: number("height")?,
                    props: o.properties(),
                });
            }
        }
        Ok(Tiled { layers, objects })
    }

    pub fn layer(&self, name: &str) -> Option<&Tilemap> {
        self.layers.iter().find(|l| l.0 == name).map(|l| &l.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- made by hand -->
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="4" height="3" tilewidth="16" tileheight="16" infinite="0">
 <tileset firstgid="1" name="tiles" tilewidth="16" tileheight="16" tilecount="4" columns="4">
  <image source="tiles.png" width="64" height="16"/>
  <tile id="0"><properties><property name="solid" type="bool" value="true"/></properties></tile>
  <tile id="3"><properties><property name="front" type="bool" value="true"/></properties></tile>
 </tileset>
 <layer id="1" name="ground" width="4" height="3">
  <data encoding="csv">
0,0,0,0,
0,4,0,0,
1,1,1,1
</data>
 </layer>
 <layer id="2" name="deco" width="4" height="3"><data encoding="csv">0,0,2,0,0,0,0,0,0,0,0,0</data></layer>
 <objectgroup id="3" name="things">
  <object id="1" name="spawn" type="player" x="20" y="30.5"><point/><properties><property name="hp" value="3"/></properties></object>
  <object id="2" name="exit" x="48" y="0" width="16" height="32"/>
 </objectgroup>
</map>"#;

    fn parse(text: &str) -> Result<Tiled, String> {
        Tiled::parse(text, |s| Err(format!("no file {s}")), Sprite::test_tiles())
    }

    #[test]
    fn it_reads_layers_tile_flags_and_objects() {
        let t = parse(MAP).unwrap();
        assert_eq!(t.layers.iter().map(|l| l.0.as_str()).collect::<Vec<_>>(), ["ground", "deco"]);
        let ground = t.layer("ground").unwrap();
        assert_eq!((ground.w, ground.h, ground.size), (4, 3, 16));
        assert_eq!((ground.tile(0, 2), ground.tile(1, 1), ground.tile(0, 0)), (1, 4, 0));
        assert!(!ground.solid_in(Rect { x: 0.0, y: 32.0, w: 64.0, h: 16.0 }).is_empty(), "tile 1 is solid");
        assert!(ground.solid_in(Rect { x: 16.0, y: 16.0, w: 16.0, h: 16.0 }).is_empty(), "tile 4 is not");
        let cmds = ground.draw_list(Rect { x: 0.0, y: 0.0, w: 64.0, h: 48.0 });
        assert_eq!(cmds.len(), 2, "the tile with `front` is drawn over entities, the rest behind");
        assert_eq!(t.layer("deco").unwrap().tile(2, 0), 2);
        assert!(t.layer("nope").is_none());
        let spawn = &t.objects[0];
        assert_eq!(
            (spawn.name.as_str(), spawn.kind.as_str(), spawn.x, spawn.y, spawn.prop("hp")),
            ("spawn", "player", 20.0, 30.5, Some("3"))
        );
        assert_eq!(t.objects[1].rect(), Rect { x: 48.0, y: 0.0, w: 16.0, h: 32.0 });
    }

    #[test]
    fn an_external_tileset_is_read_through_the_callback() {
        let tsx = r#"<tileset name="t" tilewidth="16" tileheight="16" tilecount="4" columns="4"><tile id="1"><properties><property name="solid" value="true"/></properties></tile></tileset>"#;
        let map = r#"<map orientation="orthogonal" width="2" height="1" tilewidth="16" tileheight="16"><tileset firstgid="1" source="t.tsx"/><layer name="a" width="2" height="1"><data encoding="csv">2,0</data></layer></map>"#;
        let t = Tiled::parse(
            map,
            |s| if s == "t.tsx" { Ok(tsx.to_string()) } else { Err("?".into()) },
            Sprite::test_tiles(),
        )
        .unwrap();
        assert!(
            !t.layers[0].1.solid_in(Rect { x: 0.0, y: 0.0, w: 16.0, h: 16.0 }).is_empty(),
            "tile id 1 here is gid 2"
        );
        assert!(Tiled::parse(map, |_| Err("missing".into()), Sprite::test_tiles()).err().unwrap().contains("missing"));
    }

    #[test]
    fn what_it_cannot_draw_is_an_error_that_says_why() {
        let with = |from: &str, to: &str| parse(&MAP.replace(from, to)).err().unwrap_or_default();
        assert!(with("infinite=\"0\"", "infinite=\"1\"").contains("finite orthogonal"));
        assert!(with("orthogonal", "isometric").contains("finite orthogonal"));
        assert!(
            with("tilewidth=\"16\" tileheight=\"16\" infinite", "tilewidth=\"16\" tileheight=\"8\" infinite")
                .contains("square")
        );
        assert!(with("encoding=\"csv\">\n0,0,0,0", "encoding=\"base64\">\n0,0,0,0").contains("base64"));
        assert!(with("0,4,0,0", "0,2147483652,0,0").contains("flipped"));
        assert!(with("0,4,0,0", "0,9,0,0").contains("past the 4 tiles"));
        assert!(with("0,4,0,0", "0,4,0").contains("has 11 tiles, expected 12"));
        assert!(with("0,4,0,0", "0,x,0,0").contains("not a tile number"));
        assert!(
            with("<layer id=\"2\" name=\"deco\" width=\"4\"", "<layer id=\"2\" name=\"deco\" width=\"5\"")
                .contains("5x3")
        );
        assert!(
            with("</tileset>", "</tileset><tileset firstgid=\"9\" tilewidth=\"16\" tileheight=\"16\"/>")
                .contains("2 tilesets")
        );
        assert!(with("</layer>\n <layer", "</layr>\n <layer").contains("closes"));
        assert!(parse("not xml").err().unwrap().contains("not XML"));
        assert!(
            parse("<map").is_err()
                && parse("<map a=1/>").is_err()
                && parse("<other/>").err().unwrap().contains("not <map>")
        );
    }

    #[test]
    fn entities_comments_and_both_kinds_of_quote_are_read() {
        let node =
            parse_xml("<?xml version='1.0'?><!DOCTYPE x><a b='1 &amp; 2' c=\"&lt;\"><!-- hi --><d/>t&gt;<e></e></a>")
                .unwrap();
        assert_eq!(
            (node.attr("b"), node.attr("c"), node.kids.len(), node.text.as_str()),
            (Some("1 & 2"), Some("<"), 2, "t>")
        );
    }
}
