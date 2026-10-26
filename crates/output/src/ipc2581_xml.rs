//! Tiny indenting XML writer with attribute escaping (IPC-2581 helper).

pub struct Xml {
    out: String,
    stack: Vec<String>,
    base: usize,
}

/// Escapes text for an attribute value; drops characters XML 1.0 forbids.
pub fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => out.push(' '),
            c => out.push(c),
        }
    }
}

impl Xml {
    /// `base` is the indentation level of the first element.
    pub fn new(base: usize) -> Xml {
        Xml { out: String::new(), stack: Vec::new(), base }
    }

    fn indent(&mut self) {
        for _ in 0..self.base + self.stack.len() {
            self.out.push_str("  ");
        }
    }

    fn start(&mut self, tag: &str, attrs: &[(&str, String)]) {
        self.indent();
        self.out.push('<');
        self.out.push_str(tag);
        for (k, v) in attrs {
            self.out.push(' ');
            self.out.push_str(k);
            self.out.push_str("=\"");
            escape(v, &mut self.out);
            self.out.push('"');
        }
    }

    pub fn open(&mut self, tag: &str, attrs: &[(&str, String)]) {
        self.start(tag, attrs);
        self.out.push_str(">\n");
        self.stack.push(tag.to_string());
    }

    pub fn leaf(&mut self, tag: &str, attrs: &[(&str, String)]) {
        self.start(tag, attrs);
        self.out.push_str("/>\n");
    }

    pub fn close(&mut self) {
        if let Some(tag) = self.stack.pop() {
            self.indent();
            self.out.push_str("</");
            self.out.push_str(&tag);
            self.out.push_str(">\n");
        }
    }

    /// Appends pre-rendered markup verbatim.
    pub fn raw(&mut self, s: &str) {
        self.out.push_str(s);
    }

    /// Closes any open elements and returns the text.
    pub fn finish(mut self) -> String {
        while !self.stack.is_empty() {
            self.close();
        }
        self.out
    }
}
