use std::fmt;

/// Non-fatal notes collected while importing (unsupported features, guesses).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    pub messages: Vec<ReportMessage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Warning,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReportMessage {
    pub severity: Severity,
    /// Where it happened, e.g. "Device:R_Small" or "sheet main.kicad_sch".
    pub context: String,
    pub text: String,
}

impl ImportReport {
    pub fn info(&mut self, context: impl Into<String>, text: impl Into<String>) {
        self.messages.push(ReportMessage { severity: Severity::Info, context: context.into(), text: text.into() });
    }

    pub fn warn(&mut self, context: impl Into<String>, text: impl Into<String>) {
        self.messages.push(ReportMessage { severity: Severity::Warning, context: context.into(), text: text.into() });
    }

    pub fn extend(&mut self, other: ImportReport) {
        self.messages.extend(other.messages);
    }
}

impl fmt::Display for ReportMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.context, self.text)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("syntax error at line {line}: {message}")]
    Syntax { line: usize, message: String },
    #[error("unsupported: {0}")]
    Unsupported(String),
    #[error("invalid file: {0}")]
    Invalid(String),
}
