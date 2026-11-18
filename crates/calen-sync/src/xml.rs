//! Tiny namespace-agnostic XML tree for WebDAV multistatus responses.

use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Debug, Clone, Default)]
pub struct Node {
    /// Local name (prefix/namespace stripped), e.g. `href`, `displayname`.
    pub name: String,
    pub children: Vec<Node>,
    pub text: String,
    pub attrs: Vec<(String, String)>,
}

impl Node {
    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// First descendant with this name (depth-first).
    pub fn find(&self, name: &str) -> Option<&Node> {
        for c in &self.children {
            if c.name == name {
                return Some(c);
            }
            if let Some(n) = c.find(name) {
                return Some(n);
            }
        }
        None
    }

    pub fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.find_all(name, out);
        }
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn text_of(&self, name: &str) -> Option<String> {
        self.find(name).map(|n| n.text.trim().to_string()).filter(|s| !s.is_empty())
    }
}

fn local(name: &str) -> String {
    name.rsplit(':').next().unwrap_or("").to_string()
}

fn attrs(e: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    e.attributes().flatten().map(|a| (local(a.key.as_ref()), a.value.to_string())).collect()
}

pub fn parse(xml: &str) -> Result<Node, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut stack: Vec<Node> = vec![Node { name: "#root".into(), ..Default::default() }];
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => stack.push(Node { name: local(e.name().as_ref()), attrs: attrs(&e), ..Default::default() }),
            Ok(Event::Empty(e)) => {
                let n = Node { name: local(e.name().as_ref()), attrs: attrs(&e), ..Default::default() };
                stack.last_mut().unwrap().children.push(n);
            }
            Ok(Event::End(_)) => {
                let n = stack.pop().ok_or("unbalanced xml")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(n),
                    None => return Err("unbalanced xml".into()),
                }
            }
            Ok(Event::Text(t)) => {
                let s = t.into_inner();
                stack.last_mut().unwrap().text.push_str(&s);
            }
            Ok(Event::CData(t)) => {
                let s = t.into_inner().into_owned();
                stack.last_mut().unwrap().text.push_str(&s);
            }
            Ok(Event::GeneralRef(r)) => {
                let name = r.into_inner().into_owned();
                let ch = match name.as_str() {
                    "amp" => "&".to_string(),
                    "lt" => "<".to_string(),
                    "gt" => ">".to_string(),
                    "quot" => "\"".to_string(),
                    "apos" => "'".to_string(),
                    n if n.starts_with("#x") => {
                        u32::from_str_radix(&n[2..], 16).ok().and_then(char::from_u32).map(String::from).unwrap_or_default()
                    }
                    n if n.starts_with('#') => n[1..].parse::<u32>().ok().and_then(char::from_u32).map(String::from).unwrap_or_default(),
                    _ => String::new(),
                };
                stack.last_mut().unwrap().text.push_str(&ch);
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    if stack.len() != 1 {
        return Err("unterminated xml".into());
    }
    Ok(stack.pop().unwrap())
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multistatus() {
        let x = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav">
 <d:response><d:href>/cal/a.ics</d:href>
  <d:propstat><d:prop><d:getetag>"1&amp;2"</d:getetag>
   <cal:calendar-data><![CDATA[BEGIN:VCALENDAR]]></cal:calendar-data></d:prop>
   <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
        let root = parse(x).unwrap();
        let ms = root.child("multistatus").unwrap();
        let r = ms.child("response").unwrap();
        assert_eq!(r.text_of("href").unwrap(), "/cal/a.ics");
        assert_eq!(r.text_of("getetag").unwrap(), "\"1&2\"");
        assert_eq!(r.text_of("calendar-data").unwrap(), "BEGIN:VCALENDAR");
    }
}
