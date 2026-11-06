//! A small XML reader over `quick_xml` that hands out names without their prefixes and never expands a custom entity.

use quick_xml::events::Event;
use quick_xml::Reader;

pub(crate) enum Ev {
    Start(String, Vec<(String, String)>),
    End(String),
    Text(String),
}

const MAX_DEPTH: usize = 256;

/// The value of an attribute by its name without prefix.
pub(crate) fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

fn local(raw: &str) -> String {
    raw.rsplit(':').next().unwrap_or("").to_string()
}

fn predefined(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    }
}

/// Calls `f` for every event until it returns false. At most `max_events` events are read, and elements may nest at most
/// 256 deep. A DOCTYPE and any entity other than the five predefined ones and numeric references are ignored, never
/// expanded, so a file cannot grow its own text.
pub(crate) fn walk(
    bytes: &[u8],
    max_events: usize,
    mut f: impl FnMut(Ev) -> bool,
) -> Result<(), String> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().check_end_names = true;
    let mut buf = Vec::new();
    let mut depth = 0usize;
    let mut events = 0usize;
    loop {
        events += 1;
        if events > max_events {
            return Err("too many elements".into());
        }
        let ev = reader
            .read_event_into(&mut buf)
            .map_err(|e| e.to_string())?;
        let go = match ev {
            Event::Start(e) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err("elements nested too deeply".into());
                }
                f(Ev::Start(local(e.name().as_ref()), attrs(&e)))
            }
            // an element written `<x/>` is a start and an end, so every reader sees the same pairs
            Event::Empty(e) => {
                let name = local(e.name().as_ref());
                f(Ev::Start(name.clone(), attrs(&e))) && f(Ev::End(name))
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                f(Ev::End(local(e.name().as_ref())))
            }
            Event::Text(t) => f(Ev::Text(t.xml10_content().into_owned())),
            Event::CData(t) => f(Ev::Text(t.xml10_content().into_owned())),
            Event::GeneralRef(r) => {
                let name: &str = &r;
                let c = match r.resolve_char_ref() {
                    Ok(Some(c)) => Some(c),
                    _ => predefined(name),
                };
                match c {
                    Some(c) if !c.is_control() || c == '\n' || c == '\t' => {
                        f(Ev::Text(c.to_string()))
                    }
                    _ => true,
                }
            }
            Event::Eof => return Ok(()),
            _ => true,
        };
        if !go {
            return Ok(());
        }
        buf.clear();
    }
}

fn attrs(e: &quick_xml::events::BytesStart<'_>) -> Vec<(String, String)> {
    e.attributes()
        .flatten()
        .map(|a| {
            let key = local(a.key.as_ref());
            let value = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(|v| v.into_owned())
                .unwrap_or_default();
            (key, value)
        })
        .collect()
}
