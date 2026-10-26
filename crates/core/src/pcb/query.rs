//! Design-rule scopes and rule lookup.
//!
//! Rule scopes are small query expressions, e.g. `All`, `InNet('GND')`,
//! `InNetClass('Power') and OnLayer('Top Layer')`. [`RuleSet`] answers "which
//! value applies to this object (pair)?" for every rule kind; the first
//! enabled matching rule in list order wins, then the net class, then a
//! built-in default.
//!
//! Grammar (keywords and function names are case-insensitive, strings use
//! single or double quotes, no escapes):
//!
//! ```text
//! expr    := and (('or' | '||') and)*
//! and     := unary (('and' | '&&') unary)*
//! unary   := ('not' | '!') unary | primary
//! primary := '(' expr ')' | Name [ '(' [ 'string' ] ')' ]
//! ```
//!
//! Predicates: `All`, `InNet('X')`, `InNetClass('X')`, `OnLayer('Top Layer')`,
//! `IsPad`, `IsSmd`, `IsThroughHole`, `IsVia`, `IsTrack` (straight and arc
//! tracks), `IsArc`, `IsZone`, `IsText`, `IsFootprint`, `InComponent('U1')`,
//! `HasFootprint('SOIC*')`, `InDiffPair('USB')`, `InNetPattern('DDR_*')`.
//! Net names compare case-sensitively; class, layer, designator, footprint and
//! diff-pair names case-insensitively. `*` / `?` wildcards work in
//! `InNetPattern`, `InComponent` and `HasFootprint`. An empty scope means `All`.

use std::collections::HashMap;
use std::fmt;

use crate::document::LayerId;
use crate::layer;
use crate::units::{Coord, mm};

use super::board::{BoardSettings, RuleKind};
use super::items::ZoneConnect;

/// What kind of board object a scope is evaluated against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjKind {
    Track,
    Arc,
    Via,
    Pad,
    Zone,
    Text,
    Graphic,
    Footprint,
}

/// The facts a scope expression can test.
#[derive(Clone, Copy, Debug)]
pub struct Obj<'a> {
    pub kind: ObjKind,
    pub net: &'a str,
    pub layer: Option<LayerId>,
    /// Owning footprint designator (pads, footprint graphics).
    pub designator: Option<&'a str>,
    /// Owning footprint cache key / name.
    pub footprint: Option<&'a str>,
    /// SMD pad (vs through-hole).
    pub smd: bool,
}

impl<'a> Obj<'a> {
    pub fn new(kind: ObjKind, net: &'a str, layer: Option<LayerId>) -> Self {
        Obj { kind, net, layer, designator: None, footprint: None, smd: false }
    }
}

// ---------- AST ----------

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Const(bool),
    Pred(Pred),
    Not(Box<Node>),
    And(Box<Node>, Box<Node>),
    Or(Box<Node>, Box<Node>),
}

#[derive(Clone, Debug, PartialEq)]
enum Pred {
    InNet(String),
    InNetClass(String),
    OnLayer(String),
    IsPad,
    IsSmd,
    IsThroughHole,
    IsVia,
    IsTrack,
    IsArc,
    IsZone,
    IsText,
    IsFootprint,
    /// Lower-cased glob.
    InComponent(String),
    /// Lower-cased glob.
    HasFootprint(String),
    InDiffPair(String),
    InNetPattern(String),
    // Forms resolved against a board by `RuleSet` (faster to evaluate).
    ClassIdx(Option<usize>),
    Layers(Vec<LayerId>),
    Nets(Vec<String>),
}

/// A parsed scope expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Query {
    source: String,
    node: Node,
}

/// Parses a scope expression. Errors read `column N: message` (1-based).
pub fn parse(src: &str) -> Result<Query, String> {
    let tokens = tokenize(src)?;
    let node = if tokens.len() == 1 {
        Node::Const(true) // empty scope = All
    } else {
        let mut p = Parser { tokens, pos: 0 };
        let n = p.or()?;
        let t = p.peek();
        if t.tok != Tok::End {
            return Err(format!("column {}: unexpected {}", t.col, t.tok.describe()));
        }
        n
    };
    Ok(Query { source: src.trim().to_string(), node })
}

impl Query {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn matches(&self, o: &Obj, board: &BoardSettings) -> bool {
        eval(&self.node, o, &Env { board, classes: None })
    }
}

impl fmt::Display for Query {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.source)
    }
}

// ---------- tokenizer ----------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    LParen,
    RParen,
    Comma,
    And,
    Or,
    Not,
    End,
}

impl Tok {
    fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("'{s}'"),
            Tok::Str(s) => format!("string '{s}'"),
            Tok::LParen => "'('".into(),
            Tok::RParen => "')'".into(),
            Tok::Comma => "','".into(),
            Tok::And => "'and'".into(),
            Tok::Or => "'or'".into(),
            Tok::Not => "'not'".into(),
            Tok::End => "end of expression".into(),
        }
    }
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    col: usize,
}

fn tokenize(src: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let col = i + 1;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let two = |a: char, b: char| c == a && chars.get(i + 1) == Some(&b);
        let tok = if c == '(' {
            i += 1;
            Tok::LParen
        } else if c == ')' {
            i += 1;
            Tok::RParen
        } else if c == ',' {
            i += 1;
            Tok::Comma
        } else if two('&', '&') {
            i += 2;
            Tok::And
        } else if two('|', '|') {
            i += 2;
            Tok::Or
        } else if c == '!' {
            i += 1;
            Tok::Not
        } else if c == '\'' || c == '"' {
            let end = chars[i + 1..].iter().position(|&x| x == c).ok_or_else(|| format!("column {col}: unterminated string"))?;
            let s: String = chars[i + 1..i + 1 + end].iter().collect();
            i += end + 2;
            Tok::Str(s)
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.to_ascii_lowercase().as_str() {
                "and" => Tok::And,
                "or" => Tok::Or,
                "not" => Tok::Not,
                _ => Tok::Ident(word),
            }
        } else if c == '&' || c == '|' {
            return Err(format!("column {col}: use '{c}{c}' (or '{}')", if c == '&' { "and" } else { "or" }));
        } else {
            return Err(format!("column {col}: unexpected character '{c}'"));
        };
        out.push(Token { tok, col });
    }
    out.push(Token { tok: Tok::End, col: chars.len() + 1 });
    Ok(out)
}

// ---------- parser ----------

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

const NO_ARG: &[&str] = &["all", "ispad", "issmd", "isthroughhole", "isvia", "istrack", "isarc", "iszone", "istext", "isfootprint"];
const ONE_ARG: &[&str] = &["innet", "innetclass", "onlayer", "incomponent", "hasfootprint", "indiffpair", "innetpattern"];

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn next(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn or(&mut self) -> Result<Node, String> {
        let mut n = self.and()?;
        while self.peek().tok == Tok::Or {
            self.next();
            n = Node::Or(Box::new(n), Box::new(self.and()?));
        }
        Ok(n)
    }

    fn and(&mut self) -> Result<Node, String> {
        let mut n = self.unary()?;
        while self.peek().tok == Tok::And {
            self.next();
            n = Node::And(Box::new(n), Box::new(self.unary()?));
        }
        Ok(n)
    }

    fn unary(&mut self) -> Result<Node, String> {
        if self.peek().tok == Tok::Not {
            self.next();
            return Ok(Node::Not(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn expect_rparen(&mut self, what: &str) -> Result<(), String> {
        let t = self.next();
        if t.tok == Tok::RParen { Ok(()) } else { Err(format!("column {}: expected ')' {what}, found {}", t.col, t.tok.describe())) }
    }

    fn primary(&mut self) -> Result<Node, String> {
        let t = self.next();
        match t.tok {
            Tok::LParen => {
                let n = self.or()?;
                self.expect_rparen(&format!("to close '(' at column {}", t.col))?;
                Ok(n)
            }
            Tok::Ident(name) => {
                let lower = name.to_ascii_lowercase();
                if NO_ARG.contains(&lower.as_str()) {
                    if self.peek().tok == Tok::LParen {
                        self.next();
                        let a = self.peek().clone();
                        if a.tok != Tok::RParen {
                            return Err(format!("column {}: {name} takes no argument", a.col));
                        }
                        self.next();
                    }
                    return Ok(no_arg_pred(&lower));
                }
                if ONE_ARG.contains(&lower.as_str()) {
                    let open = self.next();
                    if open.tok != Tok::LParen {
                        return Err(format!("column {}: {name} expects a quoted argument, e.g. {name}('X')", open.col));
                    }
                    let arg = self.next();
                    let Tok::Str(s) = arg.tok else {
                        return Err(format!("column {}: {name} expects a quoted string, found {}", arg.col, arg.tok.describe()));
                    };
                    self.expect_rparen(&format!("after the argument of {name}"))?;
                    return Ok(Node::Pred(one_arg_pred(&lower, s)));
                }
                Err(format!("column {}: unknown function '{name}'", t.col))
            }
            Tok::End => Err(format!("column {}: expression ends unexpectedly", t.col)),
            other => Err(format!("column {}: unexpected {}", t.col, other.describe())),
        }
    }
}

fn no_arg_pred(lower: &str) -> Node {
    Node::Pred(match lower {
        "all" => return Node::Const(true),
        "ispad" => Pred::IsPad,
        "issmd" => Pred::IsSmd,
        "isthroughhole" => Pred::IsThroughHole,
        "isvia" => Pred::IsVia,
        "istrack" => Pred::IsTrack,
        "isarc" => Pred::IsArc,
        "iszone" => Pred::IsZone,
        "istext" => Pred::IsText,
        _ => Pred::IsFootprint,
    })
}

fn one_arg_pred(lower: &str, s: String) -> Pred {
    match lower {
        "innet" => Pred::InNet(s),
        "innetclass" => Pred::InNetClass(s),
        "onlayer" => Pred::OnLayer(s),
        "incomponent" => Pred::InComponent(s.to_lowercase()),
        "hasfootprint" => Pred::HasFootprint(s.to_lowercase()),
        "indiffpair" => Pred::InDiffPair(s),
        _ => Pred::InNetPattern(s),
    }
}

// ---------- evaluation ----------

struct Env<'b> {
    board: &'b BoardSettings,
    /// Precomputed explicit net → class index (RuleSet only).
    classes: Option<&'b HashMap<&'b str, usize>>,
}

impl Env<'_> {
    fn class_index(&self, net: &str) -> Option<usize> {
        match self.classes {
            Some(m) => m.get(net).copied().or(if self.board.net_classes.is_empty() { None } else { Some(0) }),
            None => self.board.net_classes.iter().position(|c| c.nets.iter().any(|n| n == net)).or(if self.board.net_classes.is_empty() { None } else { Some(0) }),
        }
    }
}

fn eval(n: &Node, o: &Obj, env: &Env) -> bool {
    match n {
        Node::Const(b) => *b,
        Node::Not(a) => !eval(a, o, env),
        Node::And(a, b) => eval(a, o, env) && eval(b, o, env),
        Node::Or(a, b) => eval(a, o, env) || eval(b, o, env),
        Node::Pred(p) => eval_pred(p, o, env),
    }
}

fn eval_pred(p: &Pred, o: &Obj, env: &Env) -> bool {
    match p {
        Pred::InNet(n) => o.net == n.as_str(),
        Pred::InNetPattern(g) => glob(g, o.net),
        Pred::InNetClass(name) => env.class_index(o.net).is_some_and(|i| env.board.net_classes[i].name.eq_ignore_ascii_case(name)),
        Pred::ClassIdx(i) => i.is_some() && env.class_index(o.net) == *i,
        Pred::OnLayer(name) => o.layer.is_some_and(|l| resolve_layer(name, env.board).contains(&l)),
        Pred::Layers(ls) => o.layer.is_some_and(|l| ls.contains(&l)),
        Pred::IsPad => o.kind == ObjKind::Pad,
        Pred::IsSmd => o.kind == ObjKind::Pad && o.smd,
        Pred::IsThroughHole => o.kind == ObjKind::Pad && !o.smd,
        Pred::IsVia => o.kind == ObjKind::Via,
        Pred::IsTrack => matches!(o.kind, ObjKind::Track | ObjKind::Arc),
        Pred::IsArc => o.kind == ObjKind::Arc,
        Pred::IsZone => o.kind == ObjKind::Zone,
        Pred::IsText => o.kind == ObjKind::Text,
        Pred::IsFootprint => o.kind == ObjKind::Footprint,
        Pred::InComponent(g) => o.designator.is_some_and(|d| glob(g, &d.to_lowercase())),
        Pred::HasFootprint(g) => o.footprint.is_some_and(|f| {
            let f = f.to_lowercase();
            glob(g, &f) || f.rsplit('/').next().is_some_and(|name| glob(g, name))
        }),
        Pred::InDiffPair(name) => env.board.diff_pairs.iter().any(|d| d.name.eq_ignore_ascii_case(name) && (d.positive == o.net || d.negative == o.net)),
        Pred::Nets(ns) => ns.iter().any(|n| n.as_str() == o.net),
    }
}

/// `*` matches any run, `?` any single character. Case-sensitive.
pub fn glob(pattern: &str, s: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = s.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// PCBPlannerCAD's built-in layer names (mirrors the UI table): (id, name, short id).
const LAYER_NAMES: &[(LayerId, &str, &str)] = &[
    (layer::TOP_COPPER, "Top Layer", "T.Cu"),
    (layer::BOTTOM_COPPER, "Bottom Layer", "B.Cu"),
    (layer::TOP_SILK, "Top Overlay", "T.Silk"),
    (layer::BOTTOM_SILK, "Bottom Overlay", "B.Silk"),
    (layer::TOP_MASK, "Top Solder", "T.Mask"),
    (layer::BOTTOM_MASK, "Bottom Solder", "B.Mask"),
    (layer::TOP_PASTE, "Top Paste", "T.Paste"),
    (layer::BOTTOM_PASTE, "Bottom Paste", "B.Paste"),
    (layer::TOP_COURTYARD, "Top Courtyard", "T.Crt"),
    (layer::BOTTOM_COURTYARD, "Bottom Courtyard", "B.Crt"),
    (layer::TOP_ASSEMBLY, "Top Assembly", "T.Asm"),
    (layer::BOTTOM_ASSEMBLY, "Bottom Assembly", "B.Asm"),
    (layer::MECHANICAL_1, "Mechanical 1", "Mech1"),
    (layer::BOARD_OUTLINE, "Board Outline", "Edge"),
    (layer::KEEP_OUT, "Keep-Out", "Keep"),
    (layer::MULTI_LAYER, "Multi-Layer", "Multi"),
];

/// Layer ids a layer name refers to: stackup names, built-in names/short ids,
/// "Inner N" / "InN.Cu" / "Mid-Layer N", and "F.Cu"/"Top"/"Bottom" aliases.
pub fn resolve_layer(name: &str, board: &BoardSettings) -> Vec<LayerId> {
    let n = name.trim();
    let lower = n.to_ascii_lowercase();
    let mut out: Vec<LayerId> = board.stackup.layers.iter().filter(|l| l.name.eq_ignore_ascii_case(n)).filter_map(|l| l.layer).collect();
    for (id, full, short) in LAYER_NAMES {
        if full.eq_ignore_ascii_case(n) || short.eq_ignore_ascii_case(n) {
            out.push(*id);
        }
    }
    match lower.as_str() {
        "f.cu" | "top" => out.push(layer::TOP_COPPER),
        "bottom" => out.push(layer::BOTTOM_COPPER),
        _ => {}
    }
    let compact: String = lower.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    let inner = ["inner", "midlayer", "in"].iter().find_map(|p| {
        let rest = compact.strip_prefix(p)?;
        let rest = rest.strip_suffix(".cu").unwrap_or(rest);
        rest.parse::<u16>().ok().filter(|k| (1..=30).contains(k))
    });
    if let Some(k) = inner {
        out.push(layer::inner_copper(k));
    }
    out.sort();
    out.dedup();
    out
}

/// Resolves board-dependent predicates once, so rule lookups never parse
/// layer names or scan class lists.
fn bind(n: Node, board: &BoardSettings) -> Node {
    match n {
        Node::Not(a) => Node::Not(Box::new(bind(*a, board))),
        Node::And(a, b) => Node::And(Box::new(bind(*a, board)), Box::new(bind(*b, board))),
        Node::Or(a, b) => Node::Or(Box::new(bind(*a, board)), Box::new(bind(*b, board))),
        Node::Pred(Pred::OnLayer(name)) => Node::Pred(Pred::Layers(resolve_layer(&name, board))),
        Node::Pred(Pred::InNetClass(name)) => Node::Pred(Pred::ClassIdx(board.net_classes.iter().position(|c| c.name.eq_ignore_ascii_case(&name)))),
        Node::Pred(Pred::InDiffPair(name)) => Node::Pred(Pred::Nets(
            board.diff_pairs.iter().filter(|d| d.name.eq_ignore_ascii_case(&name)).flat_map(|d| [d.positive.clone(), d.negative.clone()]).collect(),
        )),
        other => other,
    }
}

// ---------- rule set ----------

/// Where a looked-up value came from (for DRC messages).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleSource<'a> {
    Rule(&'a str),
    NetClass(&'a str),
    Default,
}

impl fmt::Display for RuleSource<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleSource::Rule(n) => f.write_str(n),
            RuleSource::NetClass(n) => write!(f, "Net class {n}"),
            RuleSource::Default => f.write_str("Default"),
        }
    }
}

/// Precomputed clearance facts of one object on one layer: which clearance
/// rules' first/second scopes it matches (bit i = i-th clearance rule) and
/// its net-class clearance. Pairs then resolve in O(1) via
/// [`RuleSet::clearance_between`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClearanceProfile {
    first: u128,
    second: u128,
    class: Option<u16>,
    class_clearance: Coord,
}

struct Scoped<'a, T> {
    name: &'a str,
    scope: Node,
    value: T,
}

struct ClearanceRule<'a> {
    name: &'a str,
    first: Node,
    second: Node,
    min: Coord,
}

const MAX_CLEARANCE_RULES: usize = 128;
const DEFAULT_CLEARANCE: Coord = 200_000;

/// Compiled rules of a board.
pub struct RuleSet<'a> {
    board: &'a BoardSettings,
    errors: Vec<String>,
    classes: HashMap<&'a str, usize>,
    clearance: Vec<ClearanceRule<'a>>,
    width: Vec<Scoped<'a, (Coord, Coord, Coord)>>,
    via: Vec<Scoped<'a, (Coord, Coord, Coord)>>,
    hole: Vec<Scoped<'a, (Coord, Coord)>>,
    mask: Vec<Scoped<'a, Coord>>,
    paste: Vec<Scoped<'a, Coord>>,
    zone: Vec<Scoped<'a, (ZoneConnect, Coord, Coord)>>,
    diff: Vec<Scoped<'a, (Coord, Coord)>>,
    length: Vec<Scoped<'a, (Coord, Coord)>>,
    outline: Option<(Coord, &'a str)>,
    component: Option<(Coord, &'a str)>,
    silk: Option<(Coord, &'a str)>,
}

impl<'a> RuleSet<'a> {
    pub fn new(board: &'a BoardSettings) -> Self {
        let mut classes = HashMap::new();
        for (i, c) in board.net_classes.iter().enumerate() {
            for n in &c.nets {
                classes.entry(n.as_str()).or_insert(i);
            }
        }
        let mut rs = RuleSet {
            board,
            errors: Vec::new(),
            classes,
            clearance: Vec::new(),
            width: Vec::new(),
            via: Vec::new(),
            hole: Vec::new(),
            mask: Vec::new(),
            paste: Vec::new(),
            zone: Vec::new(),
            diff: Vec::new(),
            length: Vec::new(),
            outline: None,
            component: None,
            silk: None,
        };
        let mut errors = Vec::new();
        for rule in board.rules.iter().filter(|r| r.enabled) {
            let name = rule.name.as_str();
            let mut compile = |src: &str, what: &str| -> Option<Node> {
                match parse(src) {
                    Ok(q) => Some(bind(q.node, board)),
                    Err(e) => {
                        errors.push(format!("{name}: {what}{e}"));
                        None
                    }
                }
            };
            macro_rules! scoped {
                ($list:ident, $scope:expr, $value:expr) => {
                    if let Some(scope) = compile($scope, "") {
                        rs.$list.push(Scoped { name, scope, value: $value });
                    }
                };
            }
            match &rule.kind {
                RuleKind::Clearance { first, second, min } => {
                    let f = compile(first, "first scope: ");
                    let s = compile(second, "second scope: ");
                    if let (Some(first), Some(second)) = (f, s) {
                        if rs.clearance.len() < MAX_CLEARANCE_RULES {
                            rs.clearance.push(ClearanceRule { name, first, second, min: *min });
                        } else {
                            errors.push(format!("{name}: more than {MAX_CLEARANCE_RULES} clearance rules; ignored"));
                        }
                    }
                }
                RuleKind::Width { scope, min, preferred, max } => scoped!(width, scope, (*min, *preferred, *max)),
                RuleKind::ViaStyle { scope, diameter, drill, min_annular_ring } => scoped!(via, scope, (*diameter, *drill, *min_annular_ring)),
                RuleKind::HoleSize { scope, min, max } => scoped!(hole, scope, (*min, *max)),
                RuleKind::SolderMaskExpansion { scope, expansion } => scoped!(mask, scope, *expansion),
                RuleKind::PasteMaskExpansion { scope, expansion } => scoped!(paste, scope, *expansion),
                RuleKind::ZoneConnect { scope, style, gap, spoke_width } => scoped!(zone, scope, (*style, *gap, *spoke_width)),
                RuleKind::DiffPair { scope, width, gap } => scoped!(diff, scope, (*width, *gap)),
                RuleKind::Length { scope, min, max } => scoped!(length, scope, (*min, *max)),
                RuleKind::BoardOutlineClearance { min } => {
                    rs.outline.get_or_insert((*min, name));
                }
                RuleKind::ComponentClearance { min } => {
                    rs.component.get_or_insert((*min, name));
                }
                RuleKind::SilkClearance { min } => {
                    rs.silk.get_or_insert((*min, name));
                }
            }
        }
        rs.errors = errors;
        rs
    }

    pub fn board(&self) -> &BoardSettings {
        self.board
    }

    /// Scope expressions that failed to parse ("rule name: message").
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    fn env(&self) -> Env<'_> {
        Env { board: self.board, classes: Some(&self.classes) }
    }

    fn lookup<'s, T>(&'s self, list: &'s [Scoped<'a, T>], o: &Obj) -> Option<(&'s T, RuleSource<'a>)> {
        let env = self.env();
        list.iter().find(|r| eval(&r.scope, o, &env)).map(|r| (&r.value, RuleSource::Rule(r.name)))
    }

    fn class(&self, net: &str) -> Option<(usize, &'a super::board::NetClass)> {
        let i = self.env().class_index(net)?;
        Some((i, &self.board.net_classes[i]))
    }

    /// Clearance facts of one object (evaluate once per object and layer).
    pub fn clearance_profile(&self, o: &Obj) -> ClearanceProfile {
        let env = self.env();
        let mut p = ClearanceProfile::default();
        for (i, r) in self.clearance.iter().enumerate() {
            if eval(&r.first, o, &env) {
                p.first |= 1 << i;
            }
            if eval(&r.second, o, &env) {
                p.second |= 1 << i;
            }
        }
        if let Some((i, c)) = self.class(o.net) {
            p.class = Some(i as u16);
            p.class_clearance = c.clearance;
        }
        p
    }

    /// Clearance between two profiled objects: first matching rule (either
    /// order), else the larger net-class clearance, else 0.2 mm.
    pub fn clearance_between(&self, a: &ClearanceProfile, b: &ClearanceProfile) -> (Coord, RuleSource<'a>) {
        let m = (a.first & b.second) | (b.first & a.second);
        if m != 0 {
            let r = &self.clearance[m.trailing_zeros() as usize];
            return (r.min, RuleSource::Rule(r.name));
        }
        let pick = if a.class_clearance >= b.class_clearance { a } else { b };
        match pick.class {
            Some(i) => (pick.class_clearance, RuleSource::NetClass(&self.board.net_classes[i as usize].name)),
            None => (DEFAULT_CLEARANCE, RuleSource::Default),
        }
    }

    /// Largest clearance any object pair can require.
    pub fn max_clearance(&self) -> Coord {
        let rules = self.clearance.iter().map(|r| r.min);
        let classes = self.board.net_classes.iter().map(|c| c.clearance);
        let m = rules.chain(classes).max();
        m.unwrap_or(DEFAULT_CLEARANCE)
    }

    /// Minimum copper clearance between two objects of different nets.
    pub fn clearance(&self, a: &Obj, b: &Obj) -> Coord {
        self.clearance_rule(a, b).0
    }

    pub fn clearance_rule(&self, a: &Obj, b: &Obj) -> (Coord, RuleSource<'a>) {
        let env = self.env();
        for r in &self.clearance {
            if (eval(&r.first, a, &env) && eval(&r.second, b, &env)) || (eval(&r.first, b, &env) && eval(&r.second, a, &env)) {
                return (r.min, RuleSource::Rule(r.name));
            }
        }
        self.clearance_between(&self.clearance_profile_class_only(a), &self.clearance_profile_class_only(b))
    }

    fn clearance_profile_class_only(&self, o: &Obj) -> ClearanceProfile {
        let mut p = ClearanceProfile::default();
        if let Some((i, c)) = self.class(o.net) {
            p.class = Some(i as u16);
            p.class_clearance = c.clearance;
        }
        p
    }

    /// (min, preferred, max) track width.
    pub fn width(&self, o: &Obj) -> (Coord, Coord, Coord) {
        self.width_rule(o).0
    }

    pub fn width_rule(&self, o: &Obj) -> ((Coord, Coord, Coord), RuleSource<'a>) {
        if let Some((v, src)) = self.lookup(&self.width, o) {
            return (*v, src);
        }
        let (dmin, dpref, dmax) = (mm(0.15), mm(0.25), mm(3.0));
        match self.class(o.net) {
            Some((_, c)) if c.track_width > 0 => ((dmin.min(c.track_width), c.track_width, dmax.max(c.track_width)), RuleSource::NetClass(&c.name)),
            _ => ((dmin, dpref, dmax), RuleSource::Default),
        }
    }

    /// (diameter, drill, minimum annular ring) for new vias.
    pub fn via_style(&self, o: &Obj) -> (Coord, Coord, Coord) {
        self.via_style_rule(o).0
    }

    pub fn via_style_rule(&self, o: &Obj) -> ((Coord, Coord, Coord), RuleSource<'a>) {
        if let Some((v, src)) = self.lookup(&self.via, o) {
            return (*v, src);
        }
        let ring = mm(0.13);
        match self.class(o.net) {
            Some((_, c)) if c.via_diameter > 0 && c.via_drill > 0 => {
                ((c.via_diameter, c.via_drill, ring.min((c.via_diameter - c.via_drill) / 2).max(0)), RuleSource::NetClass(&c.name))
            }
            _ => ((mm(0.6), mm(0.3), ring), RuleSource::Default),
        }
    }

    /// (min, max) finished hole size.
    pub fn hole_size(&self, o: &Obj) -> (Coord, Coord) {
        self.hole_size_rule(o).0
    }

    pub fn hole_size_rule(&self, o: &Obj) -> ((Coord, Coord), RuleSource<'a>) {
        self.lookup(&self.hole, o).map_or(((mm(0.2), mm(6.3)), RuleSource::Default), |(v, s)| (*v, s))
    }

    pub fn outline_clearance(&self) -> Coord {
        self.outline_clearance_rule().0
    }

    pub fn outline_clearance_rule(&self) -> (Coord, RuleSource<'a>) {
        self.outline.map_or((mm(0.3), RuleSource::Default), |(v, n)| (v, RuleSource::Rule(n)))
    }

    pub fn component_clearance(&self) -> Coord {
        self.component_clearance_rule().0
    }

    pub fn component_clearance_rule(&self) -> (Coord, RuleSource<'a>) {
        self.component.map_or((0, RuleSource::Default), |(v, n)| (v, RuleSource::Rule(n)))
    }

    pub fn mask_expansion(&self, o: &Obj) -> Coord {
        self.lookup(&self.mask, o).map_or(mm(0.05), |(v, _)| *v)
    }

    pub fn paste_expansion(&self, o: &Obj) -> Coord {
        self.lookup(&self.paste, o).map_or(0, |(v, _)| *v)
    }

    /// (style, thermal gap, spoke width) for a pad/via inside a pour.
    pub fn zone_connect(&self, o: &Obj) -> (ZoneConnect, Coord, Coord) {
        self.lookup(&self.zone, o).map_or((ZoneConnect::Thermal, mm(0.3), mm(0.3)), |(v, _)| *v)
    }

    /// (track width, gap) for a differential pair.
    pub fn diff_pair(&self, o: &Obj) -> (Coord, Coord) {
        if let Some((v, _)) = self.lookup(&self.diff, o) {
            return *v;
        }
        match self.class(o.net) {
            Some((_, c)) if c.diff_pair_width > 0 && c.diff_pair_gap > 0 => (c.diff_pair_width, c.diff_pair_gap),
            _ => (mm(0.2), mm(0.2)),
        }
    }

    /// (min, max) routed length, when a length rule applies.
    pub fn length(&self, o: &Obj) -> Option<(Coord, Coord)> {
        self.lookup(&self.length, o).map(|(v, _)| *v)
    }

    pub fn silk_clearance(&self) -> Coord {
        self.silk_clearance_rule().0
    }

    pub fn silk_clearance_rule(&self) -> (Coord, RuleSource<'a>) {
        self.silk.map_or((mm(0.1), RuleSource::Default), |(v, n)| (v, RuleSource::Rule(n)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcb::board::{DiffPair, NetClass, Rule, Stackup};

    fn board() -> BoardSettings {
        let mut b = BoardSettings { stackup: Stackup::with_copper(4), ..BoardSettings::default() };
        b.net_classes.push(NetClass {
            name: "Power".into(),
            nets: vec!["VCC".into(), "GND".into()],
            track_width: mm(0.5),
            clearance: mm(0.3),
            via_diameter: mm(0.8),
            via_drill: mm(0.4),
            diff_pair_width: 0,
            diff_pair_gap: 0,
        });
        b.diff_pairs.push(DiffPair { name: "USB".into(), positive: "USB_P".into(), negative: "USB_N".into() });
        b
    }

    fn track<'a>(net: &'a str, l: LayerId) -> Obj<'a> {
        Obj::new(ObjKind::Track, net, Some(l))
    }

    fn q(src: &str) -> Query {
        parse(src).unwrap_or_else(|e| panic!("{src}: {e}"))
    }

    #[test]
    fn predicates_and_precedence() {
        let b = board();
        let gnd = track("GND", layer::TOP_COPPER);
        let sig = track("SIG", layer::BOTTOM_COPPER);
        assert!(q("All").matches(&sig, &b));
        assert!(q("").matches(&sig, &b));
        assert!(q("InNet('GND')").matches(&gnd, &b));
        assert!(!q("innet(\"gnd\")").matches(&gnd, &b), "net names are case-sensitive");
        // not > and > or
        assert!(q("InNet('X') and InNet('Y') or InNet('GND')").matches(&gnd, &b));
        assert!(!q("InNet('X') and (InNet('Y') or InNet('GND'))").matches(&gnd, &b));
        assert!(q("not InNet('X') && InNet('GND')").matches(&gnd, &b));
        assert!(!q("!(InNet('X') || InNet('GND'))").matches(&gnd, &b));
        assert!(q("NOT IsVia AND IsTrack").matches(&gnd, &b));
        assert!(q("IsTrack()").matches(&Obj::new(ObjKind::Arc, "", None), &b));
        assert!(!q("IsArc").matches(&gnd, &b));
    }

    #[test]
    fn layers_resolve_by_stackup_builtin_and_short_names() {
        let b = board();
        let top = track("A", layer::TOP_COPPER);
        let in2 = track("A", layer::inner_copper(2));
        for s in ["OnLayer('Top Layer')", "OnLayer('top layer')", "OnLayer('T.Cu')", "OnLayer('F.Cu')"] {
            assert!(q(s).matches(&top, &b), "{s}");
        }
        assert!(q("OnLayer('Inner 2')").matches(&in2, &b));
        assert!(q("OnLayer('In2.Cu')").matches(&in2, &b));
        assert!(!q("OnLayer('Inner 1')").matches(&in2, &b));
        assert!(!q("OnLayer('Top Layer')").matches(&Obj::new(ObjKind::Via, "A", None), &b));
    }

    #[test]
    fn wildcards_components_footprints_and_diff_pairs() {
        let b = board();
        let mut pad = Obj::new(ObjKind::Pad, "DDR_DQ3", Some(layer::TOP_COPPER));
        pad.designator = Some("U12");
        pad.footprint = Some("Generic/SOIC-8_3.9mm");
        pad.smd = true;
        assert!(q("InNetPattern('DDR_*')").matches(&pad, &b));
        assert!(q("InNetPattern('DDR_DQ?')").matches(&pad, &b));
        assert!(!q("InNetPattern('DDR_?')").matches(&pad, &b));
        assert!(q("HasFootprint('soic*')").matches(&pad, &b));
        assert!(q("HasFootprint('Generic/*')").matches(&pad, &b));
        assert!(q("InComponent('u12') and IsSmd and not IsThroughHole").matches(&pad, &b));
        assert!(q("InComponent('U*')").matches(&pad, &b));
        assert!(!q("InComponent('U1')").matches(&pad, &b));
        assert!(q("InDiffPair('usb')").matches(&track("USB_N", layer::TOP_COPPER), &b));
        assert!(!q("InDiffPair('USB')").matches(&track("USB", layer::TOP_COPPER), &b));
        assert!(glob("*a*b", "xxaxxb") && !glob("a*b", "ab_") && glob("", "") && glob("*", ""));
    }

    #[test]
    fn net_class_membership_falls_back_to_first_class() {
        let b = board();
        assert!(q("InNetClass('Power')").matches(&track("VCC", layer::TOP_COPPER), &b));
        assert!(q("InNetClass('default')").matches(&track("SIG", layer::TOP_COPPER), &b));
        assert!(!q("InNetClass('Default')").matches(&track("GND", layer::TOP_COPPER), &b));
    }

    #[test]
    fn errors_report_columns() {
        let e = |s: &str| parse(s).unwrap_err();
        assert_eq!(e("InNet('GND') and"), "column 17: expression ends unexpectedly");
        assert_eq!(e("Foo('x')"), "column 1: unknown function 'Foo'");
        assert_eq!(e("InNet('GND"), "column 7: unterminated string");
        assert_eq!(e("InNet(GND)"), "column 7: InNet expects a quoted string, found 'GND'");
        assert_eq!(e("(IsPad"), "column 7: expected ')' to close '(' at column 1, found end of expression");
        assert_eq!(e("IsPad IsVia"), "column 7: unexpected 'IsVia'");
        assert_eq!(e("IsPad & IsVia"), "column 7: use '&&' (or 'and')");
        assert_eq!(e("InNet"), "column 6: InNet expects a quoted argument, e.g. InNet('X')");
        assert_eq!(e("IsPad('x')"), "column 7: IsPad takes no argument");
    }

    #[test]
    fn rule_lookup_first_match_then_net_class() {
        let mut b = board();
        b.rules.insert(
            0,
            Rule { name: "PwrToAll".into(), enabled: true, kind: RuleKind::Clearance { first: "InNetClass('Power')".into(), second: "IsPad".into(), min: mm(0.5) } },
        );
        b.rules.insert(0, Rule { name: "Broken".into(), enabled: true, kind: RuleKind::Width { scope: "InNet('A'".into(), min: 1, preferred: 2, max: 3 } });
        b.rules.insert(0, Rule { name: "Off".into(), enabled: false, kind: RuleKind::Width { scope: "All".into(), min: 7, preferred: 8, max: 9 } });
        let rs = RuleSet::new(&b);
        assert_eq!(rs.errors().len(), 1);
        assert!(rs.errors()[0].starts_with("Broken: column"), "{:?}", rs.errors());
        let vcc = track("VCC", layer::TOP_COPPER);
        let pad = Obj::new(ObjKind::Pad, "SIG", Some(layer::TOP_COPPER));
        // Either order matches.
        assert_eq!(rs.clearance(&vcc, &pad), mm(0.5));
        assert_eq!(rs.clearance(&pad, &vcc), mm(0.5));
        assert_eq!(rs.clearance_rule(&pad, &vcc).1, RuleSource::Rule("PwrToAll"));
        // Profiles agree with direct evaluation.
        let (pa, pb) = (rs.clearance_profile(&pad), rs.clearance_profile(&vcc));
        assert_eq!(rs.clearance_between(&pa, &pb).0, mm(0.5));
        assert_eq!(rs.clearance(&vcc, &track("SIG", layer::TOP_COPPER)), mm(0.2));
        assert_eq!(rs.width(&vcc), (mm(0.15), mm(0.25), mm(3.0)));
        assert_eq!(rs.max_clearance(), mm(0.5));

        // Without rules: net classes decide.
        let mut b2 = board();
        b2.rules.clear();
        let rs = RuleSet::new(&b2);
        assert_eq!(rs.clearance(&vcc, &track("SIG", layer::TOP_COPPER)), mm(0.3));
        assert_eq!(rs.clearance_rule(&vcc, &track("SIG", layer::TOP_COPPER)).1, RuleSource::NetClass("Power"));
        assert_eq!(rs.clearance(&track("X", layer::TOP_COPPER), &track("SIG", layer::TOP_COPPER)), mm(0.2));
        assert_eq!(rs.width(&vcc).1, mm(0.5));
        assert_eq!(rs.via_style(&vcc), (mm(0.8), mm(0.4), mm(0.13)));
        assert_eq!(rs.via_style(&track("SIG", layer::TOP_COPPER)).0, mm(0.6));
        assert_eq!(rs.outline_clearance(), mm(0.3));
    }

    #[test]
    fn layer_scoped_rules() {
        let mut b = board();
        b.rules.insert(0, Rule { name: "Inner".into(), enabled: true, kind: RuleKind::Width { scope: "OnLayer('Inner 1')".into(), min: 1, preferred: 2, max: 3 } });
        let rs = RuleSet::new(&b);
        assert_eq!(rs.width(&track("A", layer::inner_copper(1))), (1, 2, 3));
        assert_eq!(rs.width(&track("A", layer::TOP_COPPER)).1, mm(0.25));
    }
}
