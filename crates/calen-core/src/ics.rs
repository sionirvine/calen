//! Minimal, lossless iCalendar (RFC 5545) reader/writer.
//!
//! Unknown properties and components are preserved verbatim so that editing
//! an event in Calen never drops data another client stored on it.

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Property {
    pub name: String,
    pub params: Vec<(String, String)>,
    pub value: String,
}

impl Property {
    pub fn new(name: &str, value: impl Into<String>) -> Self {
        Self { name: name.to_ascii_uppercase(), params: Vec::new(), value: value.into() }
    }

    pub fn with_param(mut self, k: &str, v: &str) -> Self {
        self.params.push((k.to_ascii_uppercase(), v.to_string()));
        self
    }

    pub fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }

    /// Value with TEXT escapes removed.
    pub fn text(&self) -> String {
        unescape_text(&self.value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Component {
    pub name: String,
    pub props: Vec<Property>,
    pub children: Vec<Component>,
}

impl Component {
    pub fn new(name: &str) -> Self {
        Self { name: name.to_ascii_uppercase(), ..Default::default() }
    }

    pub fn prop(&self, name: &str) -> Option<&Property> {
        self.props.iter().find(|p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn props_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Property> + 'a {
        self.props.iter().filter(move |p| p.name.eq_ignore_ascii_case(name))
    }

    pub fn text(&self, name: &str) -> Option<String> {
        self.prop(name).map(|p| p.text())
    }

    /// Replace (or insert) the single property `name`.
    pub fn set(&mut self, prop: Property) {
        if let Some(i) = self.props.iter().position(|p| p.name == prop.name) {
            self.props[i] = prop;
            let name = self.props[i].name.clone();
            let mut seen = false;
            self.props.retain(|p| {
                if p.name == name {
                    let keep = !seen;
                    seen = true;
                    keep
                } else {
                    true
                }
            });
        } else {
            self.props.push(prop);
        }
    }

    pub fn set_text(&mut self, name: &str, value: &str) {
        if value.is_empty() {
            self.remove(name);
        } else {
            self.set(Property::new(name, escape_text(value)));
        }
    }

    pub fn remove(&mut self, name: &str) {
        self.props.retain(|p| !p.name.eq_ignore_ascii_case(name));
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Component> + 'a {
        self.children.iter().filter(move |c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn to_ics(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        fold_line(out, &format!("BEGIN:{}", self.name));
        for p in &self.props {
            let mut line = p.name.clone();
            for (k, v) in &p.params {
                let needs_quote = v.contains([':', ';', ',']);
                if needs_quote {
                    let _ = write!(line, ";{k}=\"{v}\"");
                } else {
                    let _ = write!(line, ";{k}={v}");
                }
            }
            line.push(':');
            line.push_str(&p.value);
            fold_line(out, &line);
        }
        for c in &self.children {
            c.write(out);
        }
        fold_line(out, &format!("END:{}", self.name));
    }
}

#[derive(Debug)]
pub enum IcsError {
    Unbalanced(String),
    Empty,
}

impl std::error::Error for IcsError {}

impl std::fmt::Display for IcsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IcsError::Unbalanced(s) => write!(f, "unbalanced component: {s}"),
            IcsError::Empty => write!(f, "no calendar data"),
        }
    }
}

/// Parse an iCalendar stream, returning the top-level component (usually VCALENDAR).
pub fn parse(input: &str) -> Result<Component, IcsError> {
    let mut stack: Vec<Component> = Vec::new();
    let mut root: Option<Component> = None;
    for line in unfold(input) {
        if line.trim().is_empty() {
            continue;
        }
        let prop = parse_line(&line);
        if prop.name == "BEGIN" {
            stack.push(Component::new(&prop.value));
        } else if prop.name == "END" {
            let done = stack.pop().ok_or_else(|| IcsError::Unbalanced(prop.value.clone()))?;
            if !done.name.eq_ignore_ascii_case(&prop.value) {
                return Err(IcsError::Unbalanced(prop.value));
            }
            match stack.last_mut() {
                Some(parent) => parent.children.push(done),
                None => {
                    root = Some(done);
                    break;
                }
            }
        } else if let Some(cur) = stack.last_mut() {
            cur.props.push(prop);
        }
    }
    root.ok_or(IcsError::Empty)
}

fn unfold(input: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in input.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if (raw.starts_with(' ') || raw.starts_with('\t')) && !lines.is_empty() {
            lines.last_mut().unwrap().push_str(&raw[1..]);
        } else {
            lines.push(raw.to_string());
        }
    }
    lines
}

fn parse_line(line: &str) -> Property {
    // Find the first ':' that is not inside a quoted parameter value.
    let mut in_quote = false;
    let mut colon = line.len();
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            ':' if !in_quote => {
                colon = i;
                break;
            }
            _ => {}
        }
    }
    let (head, value) = if colon < line.len() { (&line[..colon], &line[colon + 1..]) } else { (line, "") };
    let mut parts = split_unquoted(head, ';').into_iter();
    let name = parts.next().unwrap_or_default().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.to_ascii_uppercase(), v.trim_matches('"').to_string()))
        })
        .collect();
    Property { name, params, value: value.to_string() }
}

fn split_unquoted(s: &str, sep: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for ch in s.chars() {
        if ch == '"' {
            in_quote = !in_quote;
            cur.push(ch);
        } else if ch == sep && !in_quote {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out
}

fn fold_line(out: &mut String, line: &str) {
    let mut count = 0;
    for ch in line.chars() {
        let len = ch.len_utf8();
        if count + len > 75 {
            out.push_str("\r\n ");
            count = 1;
        }
        out.push(ch);
        count += len;
    }
    out.push_str("\r\n");
}

pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

pub fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(c) => out.push(c),
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//test//EN\r\nBEGIN:VEVENT\r\nUID:abc\r\nSUMMARY:Lunch\\, with Sue\r\nDESCRIPTION:line one\\nline two that is long enough to need folding when we wri\r\n te it out again\r\nX-UNKNOWN;X-PARAM=\"a:b\":keep me\r\nDTSTART;TZID=Europe/Berlin:20241015T120000\r\nBEGIN:VALARM\r\nTRIGGER:-PT15M\r\nACTION:DISPLAY\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn parses_and_unfolds() {
        let cal = parse(SAMPLE).unwrap();
        let ev = cal.children_named("VEVENT").next().unwrap();
        assert_eq!(ev.text("SUMMARY").unwrap(), "Lunch, with Sue");
        assert!(ev.text("DESCRIPTION").unwrap().ends_with("when we write it out again"));
        assert_eq!(ev.prop("X-UNKNOWN").unwrap().param("X-PARAM"), Some("a:b"));
        assert_eq!(ev.prop("DTSTART").unwrap().param("TZID"), Some("Europe/Berlin"));
        assert_eq!(ev.children.len(), 1);
    }

    #[test]
    fn round_trip_is_stable() {
        let cal = parse(SAMPLE).unwrap();
        let again = parse(&cal.to_ics()).unwrap();
        assert_eq!(cal, again);
        for line in cal.to_ics().split("\r\n") {
            assert!(line.len() <= 75);
        }
    }
}
