//! A small XML tree reader for MyGUI's resource and layout files (elements and attributes;
//! text, comments, declarations and CDATA are skipped).

#[derive(Debug, Clone, Default)]
pub struct Elem {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Elem>,
}

impl Elem {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }

    pub fn kids<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Elem> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Every element below this one, depth first.
    pub fn descendants(&self) -> Vec<&Elem> {
        let mut out = Vec::new();
        let mut stack: Vec<&Elem> = self.children.iter().rev().collect();
        while let Some(e) = stack.pop() {
            out.push(e);
            stack.extend(e.children.iter().rev());
        }
        out
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(j) = rest.find(';') else { break };
        let ent = &rest[1..j];
        match ent {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            e if e.starts_with("#x") => {
                if let Some(c) = u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            e if e.starts_with('#') => {
                if let Some(c) = e[1..].parse::<u32>().ok().and_then(char::from_u32) {
                    out.push(c);
                }
            }
            other => {
                out.push('&');
                out.push_str(other);
                out.push(';');
            }
        }
        rest = &rest[j + 1..];
    }
    out.push_str(rest);
    out
}

/// The document's top-level elements, under an unnamed root.
pub fn parse(text: &str) -> Elem {
    let s = text.trim_start_matches('\u{feff}');
    let b = s.as_bytes();
    let mut stack: Vec<Elem> = vec![Elem::default()];
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &s[i..];
        if rest.starts_with("<!--") {
            i += rest.find("-->").map_or(b.len() - i, |k| k + 3);
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            i += rest.find("]]>").map_or(b.len() - i, |k| k + 3);
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            i += rest.find('>').map_or(b.len() - i, |k| k + 1);
            continue;
        }
        if rest.starts_with("</") {
            i += rest.find('>').map_or(b.len() - i, |k| k + 1);
            if stack.len() > 1 {
                let e = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(e);
            }
            continue;
        }
        // an opening tag: name, attributes, maybe self-closing; quotes may hold '>'
        let mut j = i + 1;
        let mut quote = 0u8;
        while j < b.len() {
            let c = b[j];
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' {
                quote = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        let inner = &s[i + 1..j.min(b.len())];
        let closed = inner.ends_with('/');
        let inner = inner.trim_end_matches('/');
        let name_end = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
        let mut e = Elem { name: inner[..name_end].to_string(), ..Default::default() };
        let mut a = &inner[name_end..];
        loop {
            a = a.trim_start();
            let Some(eq) = a.find('=') else { break };
            let key = a[..eq].trim().to_string();
            let v = a[eq + 1..].trim_start();
            let Some(q) = v.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
            let Some(end) = v[1..].find(q) else { break };
            e.attrs.push((key, unescape(&v[1..1 + end])));
            a = &v[end + 2..];
        }
        if closed {
            stack.last_mut().unwrap().children.push(e);
        } else {
            stack.push(e);
        }
        i = j + 1;
    }
    while stack.len() > 1 {
        let e = stack.pop().unwrap();
        stack.last_mut().unwrap().children.push(e);
    }
    stack.pop().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree() {
        let x = parse("\u{feff}<?xml version=\"1.0\"?><!-- c --><A k=\"v &amp; w\"><B x='1'/><C><D/></C></A>");
        let a = &x.children[0];
        assert_eq!(a.name, "A");
        assert_eq!(a.attr("k"), Some("v & w"));
        assert_eq!(a.children.len(), 2);
        assert_eq!(a.children[1].children[0].name, "D");
    }
}
