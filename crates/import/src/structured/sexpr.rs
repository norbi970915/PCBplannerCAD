//! A small, allocation-light S-expression reader for structured format files.
//!
//! Atoms borrow from the source text unless they contain escapes. Parsing
//! is iterative (no recursion), nesting depth is capped, and every list and
//! atom remembers its 1-based line number for error messages.

use std::borrow::Cow;

use crate::report::ImportError;

const MAX_DEPTH: usize = 512;

#[derive(Clone, Debug, PartialEq)]
pub enum Sexpr<'a> {
    List(List<'a>),
    Atom(Atom<'a>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Atom<'a> {
    pub text: Cow<'a, str>,
    /// Written as a double-quoted string.
    pub quoted: bool,
    pub line: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct List<'a> {
    pub items: Vec<Sexpr<'a>>,
    pub line: usize,
}

/// Parses the first top-level list of `text` (anything after it is ignored).
pub fn parse(text: &str) -> Result<List<'_>, ImportError> {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut line = 1;
    let mut stack: Vec<List<'_>> = Vec::new();
    let err = |line: usize, message: &str| ImportError::Syntax { line, message: message.to_string() };

    // Skip a UTF-8 byte order mark.
    if text.starts_with('\u{feff}') {
        i = 3;
    }

    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'\n' => {
                line += 1;
                i += 1;
            }
            b' ' | b'\t' | b'\r' | 0x0c => i += 1,
            b'(' => {
                if stack.len() >= MAX_DEPTH {
                    return Err(err(line, "lists nested too deeply"));
                }
                stack.push(List { items: Vec::new(), line });
                i += 1;
            }
            b')' => {
                let Some(done) = stack.pop() else {
                    return Err(err(line, "unexpected ')'"));
                };
                i += 1;
                match stack.last_mut() {
                    Some(parent) => parent.items.push(Sexpr::List(done)),
                    None => return Ok(done),
                }
            }
            b'"' => {
                let start_line = line;
                let (atom, next, newlines) = read_string(text, i + 1).ok_or_else(|| err(start_line, "unterminated string"))?;
                line += newlines;
                i = next;
                match stack.last_mut() {
                    Some(parent) => parent.items.push(Sexpr::Atom(Atom { text: atom, quoted: true, line: start_line })),
                    None => return Err(err(start_line, "expected '(' at top level")),
                }
            }
            _ => {
                let start = i;
                while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'\r' | b'\n' | 0x0c | b'(' | b')' | b'"') {
                    i += 1;
                }
                match stack.last_mut() {
                    Some(parent) => parent.items.push(Sexpr::Atom(Atom { text: Cow::Borrowed(&text[start..i]), quoted: false, line })),
                    None => return Err(err(line, "expected '(' at top level")),
                }
            }
        }
    }
    match stack.first() {
        Some(open) => Err(err(open.line, &format!("unexpected end of file ({} unclosed list(s))", stack.len()))),
        None => Err(err(line, "file contains no S-expression")),
    }
}

/// Reads a string body starting just after the opening quote. Returns the
/// contents, the index after the closing quote and the newlines consumed.
fn read_string(text: &str, start: usize) -> Option<(Cow<'_, str>, usize, usize)> {
    let bytes = text.as_bytes();
    let mut i = start;
    let mut newlines = 0;
    // Fast path: no escapes.
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Some((Cow::Borrowed(&text[start..i]), i + 1, newlines)),
            b'\\' => break,
            b'\n' => newlines += 1,
            _ => {}
        }
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    let mut out = String::with_capacity(i - start + 16);
    out.push_str(&text[start..i]);
    let mut seg = i;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                out.push_str(&text[seg..i]);
                return Some((Cow::Owned(out), i + 1, newlines));
            }
            b'\\' => {
                out.push_str(&text[seg..i]);
                let next = *bytes.get(i + 1)?;
                match next {
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    b'\\' => out.push('\\'),
                    b'"' => out.push('"'),
                    b'\n' => {
                        newlines += 1;
                        out.push('\n');
                    }
                    _ => {
                        // Unknown escape: keep the backslash and let the next
                        // segment copy the (possibly multi-byte) character.
                        out.push('\\');
                        i += 1;
                        seg = i;
                        continue;
                    }
                }
                i += 2;
                seg = i;
            }
            b'\n' => {
                newlines += 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

impl<'a> Sexpr<'a> {
    pub fn as_list(&self) -> Option<&List<'a>> {
        match self {
            Sexpr::List(l) => Some(l),
            Sexpr::Atom(_) => None,
        }
    }

    pub fn as_atom(&self) -> Option<&str> {
        match self {
            Sexpr::Atom(a) => Some(&a.text),
            Sexpr::List(_) => None,
        }
    }
}

impl<'a> List<'a> {
    /// The leading keyword, or "" when the list does not start with an atom.
    pub fn head(&self) -> &str {
        match self.items.first() {
            Some(Sexpr::Atom(a)) if !a.quoted => &a.text,
            _ => "",
        }
    }

    pub fn atom(&self, i: usize) -> Option<&str> {
        self.items.get(i).and_then(Sexpr::as_atom)
    }

    /// Atom `i` parsed as a finite number.
    pub fn num(&self, i: usize) -> Option<f64> {
        self.atom(i).and_then(parse_num)
    }

    /// Sub-lists, in order.
    pub fn lists(&self) -> impl Iterator<Item = &List<'a>> {
        self.items.iter().filter_map(Sexpr::as_list)
    }

    pub fn children<'s>(&'s self, name: &'s str) -> impl Iterator<Item = &'s List<'a>> + 's {
        self.lists().filter(move |l| l.head() == name)
    }

    pub fn child(&self, name: &str) -> Option<&List<'a>> {
        self.lists().find(|l| l.head() == name)
    }

    /// `(name "value")` → "value".
    pub fn child_str(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|l| l.atom(1))
    }

    /// `(name 1.5)` → 1.5.
    pub fn child_num(&self, name: &str) -> Option<f64> {
        self.child(name).and_then(|l| l.num(1))
    }

    /// Whether a bare (unquoted) atom `name` appears after the head.
    pub fn has_atom(&self, name: &str) -> bool {
        self.items.iter().skip(1).any(|s| matches!(s, Sexpr::Atom(a) if !a.quoted && a.text == name))
    }

    /// A boolean flag in any structured format spelling: bare `name`, `(name)`,
    /// `(name yes)` / `(name true)`; `(name no)` is false.
    pub fn flag(&self, name: &str) -> bool {
        if self.has_atom(name) {
            return true;
        }
        match self.child(name) {
            Some(l) => !matches!(l.atom(1), Some("no" | "false")),
            None => false,
        }
    }

    /// Items 1 and 2 as numbers, e.g. `(xy 1 2)` or `(at 1 2 90)`.
    pub fn xy(&self) -> Option<(f64, f64)> {
        Some((self.num(1)?, self.num(2)?))
    }
}

pub fn parse_num(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}
