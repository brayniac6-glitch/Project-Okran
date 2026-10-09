//! Ogre's script syntax as ParticleUniverse scripts (.pu) use it: objects `class name values...
//! { ... }` (the brace may follow on the next line) holding properties `name values...` (one a
//! line) and further objects; `//` and `/* */` comments; quoted strings.

/// An object: its class (the first word), name (the second) and further values.
#[derive(Debug, Clone, Default)]
pub struct Node {
    pub class: String,
    pub name: String,
    pub values: Vec<String>,
    pub props: Vec<Prop>,
    pub children: Vec<Node>,
    /// Properties and objects in script order (an index into `props` or `children`).
    pub order: Vec<Item>,
}

#[derive(Debug, Clone, Copy)]
pub enum Item {
    Prop(usize),
    Child(usize),
}

#[derive(Debug, Clone)]
pub struct Prop {
    pub name: String,
    pub values: Vec<String>,
}

impl Prop {
    pub fn f(&self, i: usize) -> Option<f32> {
        self.values.get(i).and_then(|v| parse_f32(v))
    }

    pub fn vec3(&self) -> Option<bevy::math::Vec3> {
        if self.values.len() < 3 {
            return None;
        }
        Some(bevy::math::Vec3::new(self.f(0)?, self.f(1)?, self.f(2)?))
    }

    pub fn boolean(&self) -> Option<bool> {
        self.values.first().and_then(|v| parse_bool(v))
    }

    pub fn string(&self) -> Option<&str> {
        self.values.first().map(|s| s.as_str())
    }
}

/// Ogre's StringConverter::parseReal-like reading of a number (a leading number; else none).
pub fn parse_f32(s: &str) -> Option<f32> {
    s.parse::<f32>().ok().or_else(|| {
        // tolerate trailing junk like "1.0f"
        let end = s
            .char_indices()
            .take_while(|(_, c)| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E'))
            .last()
            .map(|(i, c)| i + c.len_utf8())?;
        s[..end].parse().ok()
    })
}

/// Ogre's script booleans.
pub fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => Some(true),
        "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Open,
    Close,
    Newline,
}

fn lex(src: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let b: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == '\n' {
            out.push(Tok::Newline);
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && b.get(i + 1) == Some(&'/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && b.get(i + 1) == Some(&'*') {
            i += 2;
            while i < b.len() && !(b[i] == '*' && b.get(i + 1) == Some(&'/')) {
                if b[i] == '\n' {
                    out.push(Tok::Newline);
                }
                i += 1;
            }
            i += 2;
        } else if c == '{' {
            out.push(Tok::Open);
            i += 1;
        } else if c == '}' {
            out.push(Tok::Close);
            i += 1;
        } else if c == '"' {
            let mut s = String::new();
            i += 1;
            while i < b.len() && b[i] != '"' {
                s.push(b[i]);
                i += 1;
            }
            i += 1;
            out.push(Tok::Word(s));
        } else {
            let mut s = String::new();
            while i < b.len() && !b[i].is_whitespace() && b[i] != '{' && b[i] != '}' {
                if b[i] == '/' && matches!(b.get(i + 1), Some('/') | Some('*')) {
                    break;
                }
                s.push(b[i]);
                i += 1;
            }
            out.push(Tok::Word(s));
        }
    }
    out
}

/// The top-level objects of a script.
pub fn parse(src: &str) -> Vec<Node> {
    let toks = lex(src);
    let mut i = 0;
    let mut root = Node::default();
    parse_body(&toks, &mut i, &mut root, true);
    root.children
}

fn parse_body(t: &[Tok], i: &mut usize, node: &mut Node, top: bool) {
    loop {
        while *i < t.len() && t[*i] == Tok::Newline {
            *i += 1;
        }
        if *i >= t.len() {
            return;
        }
        match &t[*i] {
            Tok::Close => {
                *i += 1;
                if !top {
                    return;
                }
            }
            Tok::Open => {
                // a brace without a header: an anonymous object
                *i += 1;
                let mut child = Node::default();
                parse_body(t, i, &mut child, false);
                node.order.push(Item::Child(node.children.len()));
                node.children.push(child);
            }
            Tok::Word(_) => {
                let mut words = Vec::new();
                while let Some(Tok::Word(w)) = t.get(*i) {
                    words.push(w.clone());
                    *i += 1;
                }
                // an object if a brace follows (possibly after line breaks)
                let mut j = *i;
                while j < t.len() && t[j] == Tok::Newline {
                    j += 1;
                }
                if t.get(j) == Some(&Tok::Open) {
                    *i = j + 1;
                    let mut child = Node {
                        class: words[0].clone(),
                        name: words.get(1).cloned().unwrap_or_default(),
                        values: words.iter().skip(2).cloned().collect(),
                        ..Default::default()
                    };
                    parse_body(t, i, &mut child, false);
                    node.order.push(Item::Child(node.children.len()));
                    node.children.push(child);
                } else {
                    node.order.push(Item::Prop(node.props.len()));
                    node.props.push(Prop {
                        name: words[0].clone(),
                        values: words[1..].to_vec(),
                    });
                }
            }
            Tok::Newline => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_and_properties() {
        let n = parse(
            "system Rain\n{\n  technique\n  {\n    material x // c\n    emitter Box E1 { emission_rate dyn_random\n {\n min 1\n max 2\n }\n }\n  }\n}\n",
        );
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].class, "system");
        assert_eq!(n[0].name, "Rain");
        let t = &n[0].children[0];
        assert_eq!(t.props[0].name, "material");
        let e = &t.children[0];
        assert_eq!((e.class.as_str(), e.name.as_str(), e.values[0].as_str()), ("emitter", "Box", "E1"));
        assert_eq!(e.children[0].class, "emission_rate");
        assert_eq!(e.children[0].name, "dyn_random");
        assert_eq!(e.children[0].props.len(), 2);
    }
}
