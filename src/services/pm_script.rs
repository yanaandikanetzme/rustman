//! Runs the everyday subset of Postman scripts: `var`, `JSON.parse`,
//! `responseBody`, `pm.environment`, `pm.collectionVariables`, `pm.test`,
//! `pm.expect`, `console.log`, `if`.
//!
//! This is not JavaScript. Like `rustman-engine` it is a pure function from a
//! script and a [`HostInput`] to [`Effect`]s, with the same guarantees: no
//! loops, no user-defined functions, no network, no filesystem, so every
//! script terminates. Anything outside the subset fails with a line number
//! and the name of what is missing, rather than doing something else.
//!
//! Rustman has no collection variable store, so `pm.collectionVariables` and
//! `pm.globals` write to the active environment: `{{name}}` then resolves,
//! which is what such a script is after.

use rustman_engine::{Effect, HostInput, ResponseInput};
use serde_json::{Map, Number, Value};
use std::collections::HashMap;
use std::rc::Rc;

/// `None` is JavaScript's `undefined`.
type Val = Option<Value>;

const MAX_NESTING: usize = 64;
const MAX_STRING_BYTES: usize = 16 * 1024 * 1024;
const MAX_VALUE_NODES: usize = 1_000_000;

/// Names only a Postman script uses at the top of an expression. `let`,
/// `if` and `response` are shared with the Rustman language, so they say
/// nothing.
const POSTMAN_ROOTS: &[&str] = &[
    "pm",
    "postman",
    "responseBody",
    "responseCode",
    "tests",
    "console",
    "JSON",
    "var",
    "const",
];

/// Whether a script is written for Postman rather than the Rustman language.
pub fn is_postman_script(source: &str) -> bool {
    let Ok(tokens) = lex(source) else {
        return false;
    };
    tokens.iter().enumerate().any(|(i, t)| match &t.tok {
        Tok::Ident(name) => {
            let member = i > 0 && matches!(tokens[i - 1].tok, Tok::Punct("." | "?."));
            !member && POSTMAN_ROOTS.contains(&name.as_str())
        }
        _ => false,
    })
}

/// Run a script. The error is ready to show, with its line number.
pub fn run(source: &str, input: HostInput) -> Result<Vec<Effect>, String> {
    let tokens = lex(source).map_err(Fail::text)?;
    let mut parser = Parser {
        tokens,
        at: 0,
        depth: 0,
    };
    let program = parser.program().map_err(Fail::text)?;
    let mut run = Runner::new(input);
    run.block(&program).map_err(Fail::text)?;
    Ok(run.effects)
}

#[derive(Debug)]
struct Fail {
    line: Option<usize>,
    message: String,
}

impl Fail {
    fn new(message: impl Into<String>) -> Self {
        Self {
            line: None,
            message: message.into(),
        }
    }

    fn at(line: usize, message: impl Into<String>) -> Self {
        Self {
            line: Some(line),
            message: message.into(),
        }
    }

    fn text(self) -> String {
        match self.line {
            Some(line) => format!("line {line}: {}", self.message),
            None => self.message,
        }
    }
}

// ---- Lexer ----------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Template(Vec<TemplatePart>),
    Ident(String),
    Punct(&'static str),
    End,
}

#[derive(Debug, Clone, PartialEq)]
enum TemplatePart {
    Text(String),
    Code(String),
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    line: usize,
}

/// Longest first, so `===` is not read as `==` `=`.
const PUNCTS: &[&str] = &[
    "===", "!==", "==", "!=", "<=", ">=", "&&", "||", "??", "?.", "=>", "+=", "(", ")", "{", "}",
    "[", "]", ",", ".", ";", ":", "?", "+", "-", "*", "/", "%", "!", "<", ">", "=",
];

fn lex(source: &str) -> Result<Vec<Token>, Fail> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let (mut i, mut line) = (0, 1);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '\n' {
            line += 1;
            i += 1;
        } else if c.is_whitespace() {
            i += 1;
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            i += 2;
        } else if c == '"' || c == '\'' {
            let start = line;
            let (text, after) = string_literal(&chars, i, c, start)?;
            tokens.push(Token {
                tok: Tok::Str(text),
                line: start,
            });
            i = after;
        } else if c == '`' {
            let start = line;
            let (parts, after, lines) = template_literal(&chars, i, start)?;
            tokens.push(Token {
                tok: Tok::Template(parts),
                line: start,
            });
            line += lines;
            i = after;
        } else if c.is_ascii_digit() {
            let from = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let text: String = chars[from..i].iter().collect();
            let number = text
                .parse()
                .map_err(|_| Fail::at(line, format!("bad number `{text}`")))?;
            tokens.push(Token {
                tok: Tok::Num(number),
                line,
            });
        } else if c.is_alphabetic() || c == '_' || c == '$' {
            let from = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '$')
            {
                i += 1;
            }
            tokens.push(Token {
                tok: Tok::Ident(chars[from..i].iter().collect()),
                line,
            });
        } else {
            let Some(punct) = PUNCTS.iter().copied().find(|p| {
                p.chars()
                    .enumerate()
                    .all(|(n, pc)| chars.get(i + n) == Some(&pc))
            }) else {
                return Err(Fail::at(line, format!("unexpected character `{c}`")));
            };
            tokens.push(Token {
                tok: Tok::Punct(punct),
                line,
            });
            i += punct.len();
        }
    }
    tokens.push(Token {
        tok: Tok::End,
        line,
    });
    Ok(tokens)
}

fn escape(c: char) -> char {
    match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        other => other,
    }
}

fn string_literal(
    chars: &[char],
    open: usize,
    quote: char,
    line: usize,
) -> Result<(String, usize), Fail> {
    let mut text = String::new();
    let mut i = open + 1;
    while let Some(&c) = chars.get(i) {
        if c == quote {
            return Ok((text, i + 1));
        }
        if c == '\n' {
            break;
        }
        if c == '\\' {
            i += 1;
            if let Some(&e) = chars.get(i) {
                text.push(escape(e));
            }
        } else {
            text.push(c);
        }
        i += 1;
    }
    Err(Fail::at(line, "unterminated string"))
}

/// The parts, the index after the closing backtick and the newlines crossed.
fn template_literal(
    chars: &[char],
    open: usize,
    line: usize,
) -> Result<(Vec<TemplatePart>, usize, usize), Fail> {
    let (mut parts, mut text, mut lines) = (Vec::new(), String::new(), 0);
    let mut i = open + 1;
    while let Some(&c) = chars.get(i) {
        match c {
            '`' => {
                parts.push(TemplatePart::Text(text));
                return Ok((parts, i + 1, lines));
            }
            '\\' => {
                i += 1;
                if let Some(&e) = chars.get(i) {
                    text.push(escape(e));
                }
            }
            '$' if chars.get(i + 1) == Some(&'{') => {
                parts.push(TemplatePart::Text(std::mem::take(&mut text)));
                let from = i + 2;
                let mut depth = 1;
                i = from;
                while i < chars.len() {
                    match chars[i] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                if depth != 0 {
                    return Err(Fail::at(line, "unterminated `${` in template string"));
                }
                parts.push(TemplatePart::Code(chars[from..i].iter().collect()));
            }
            other => {
                if other == '\n' {
                    lines += 1;
                }
                text.push(other);
            }
        }
        i += 1;
    }
    Err(Fail::at(line, "unterminated template string"))
}

// ---- Parser ---------------------------------------------------------------

#[derive(Debug, Clone)]
enum Expr {
    Lit(Val),
    Ident(String),
    Member(Box<Expr>, String, bool),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Unary(&'static str, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Array(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    /// A callback: only `pm.test` accepts one.
    Func(Rc<Vec<Stmt>>),
    Template(Vec<TemplateExpr>),
}

#[derive(Debug, Clone)]
enum TemplateExpr {
    Text(String),
    Code(Expr),
}

#[derive(Debug, Clone)]
struct Stmt {
    line: usize,
    kind: StmtKind,
}

#[derive(Debug, Clone)]
enum StmtKind {
    Let(String, Expr),
    Assign(Expr, Expr),
    Expr(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    Block(Vec<Stmt>),
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    depth: usize,
}

/// Statements the subset refuses by name, so the message says what to change.
const UNSUPPORTED_KEYWORDS: &[&str] = &[
    "for", "while", "do", "function", "try", "class", "switch", "throw", "return", "async",
    "await", "import", "new", "delete", "break", "continue",
];

impl Parser {
    fn peek(&self) -> &Tok {
        &self.tokens[self.at].tok
    }

    fn line(&self) -> usize {
        self.tokens[self.at].line
    }

    fn advance(&mut self) -> Tok {
        let tok = self.tokens[self.at].tok.clone();
        if self.at + 1 < self.tokens.len() {
            self.at += 1;
        }
        tok
    }

    fn is(&self, punct: &str) -> bool {
        matches!(self.peek(), Tok::Punct(p) if *p == punct)
    }

    fn eat(&mut self, punct: &str) -> bool {
        let found = self.is(punct);
        if found {
            self.advance();
        }
        found
    }

    fn is_word(&self, word: &str) -> bool {
        matches!(self.peek(), Tok::Ident(w) if w == word)
    }

    fn expect(&mut self, punct: &str) -> Result<(), Fail> {
        if self.eat(punct) {
            return Ok(());
        }
        Err(Fail::at(
            self.line(),
            format!("expected `{punct}`, found {}", describe(self.peek())),
        ))
    }

    fn deeper(&mut self) -> Result<(), Fail> {
        self.depth += 1;
        if self.depth > MAX_NESTING {
            return Err(Fail::at(self.line(), "nested too deeply"));
        }
        Ok(())
    }

    fn program(&mut self) -> Result<Vec<Stmt>, Fail> {
        let mut out = Vec::new();
        while *self.peek() != Tok::End {
            out.push(self.statement()?);
        }
        Ok(out)
    }

    fn block_body(&mut self) -> Result<Vec<Stmt>, Fail> {
        self.expect("{")?;
        let mut out = Vec::new();
        while !self.is("}") {
            if *self.peek() == Tok::End {
                return Err(Fail::at(self.line(), "missing `}`"));
            }
            out.push(self.statement()?);
        }
        self.advance();
        Ok(out)
    }

    fn statement(&mut self) -> Result<Stmt, Fail> {
        self.deeper()?;
        let line = self.line();
        let kind = self.statement_kind(line)?;
        self.depth -= 1;
        while self.eat(";") {}
        Ok(Stmt { line, kind })
    }

    fn statement_kind(&mut self, line: usize) -> Result<StmtKind, Fail> {
        if self.is("{") {
            return Ok(StmtKind::Block(self.block_body()?));
        }
        if let Tok::Ident(word) = self.peek().clone() {
            if UNSUPPORTED_KEYWORDS.contains(&word.as_str()) {
                return Err(Fail::at(line, format!("`{word}` is not supported yet")));
            }
            match word.as_str() {
                "var" | "let" | "const" => {
                    self.advance();
                    let Tok::Ident(name) = self.advance() else {
                        return Err(Fail::at(line, "expected a variable name"));
                    };
                    let value = if self.eat("=") {
                        self.expression()?
                    } else {
                        Expr::Lit(None)
                    };
                    return Ok(StmtKind::Let(name, value));
                }
                "if" => {
                    self.advance();
                    self.expect("(")?;
                    let condition = self.expression()?;
                    self.expect(")")?;
                    let then = self.branch()?;
                    let otherwise = if self.is_word("else") {
                        self.advance();
                        self.branch()?
                    } else {
                        Vec::new()
                    };
                    return Ok(StmtKind::If(condition, then, otherwise));
                }
                _ => {}
            }
        }
        let target = self.expression()?;
        if self.eat("=") {
            let value = self.expression()?;
            return Ok(StmtKind::Assign(target, value));
        }
        if self.eat("+=") {
            let value = self.expression()?;
            let sum = Expr::Binary("+", Box::new(target.clone()), Box::new(value));
            return Ok(StmtKind::Assign(target, sum));
        }
        Ok(StmtKind::Expr(target))
    }

    fn branch(&mut self) -> Result<Vec<Stmt>, Fail> {
        if self.is("{") {
            self.block_body()
        } else {
            Ok(vec![self.statement()?])
        }
    }

    fn expression(&mut self) -> Result<Expr, Fail> {
        self.deeper()?;
        let condition = self.binary(0)?;
        let out = if self.eat("?") {
            let then = self.expression()?;
            self.expect(":")?;
            let otherwise = self.expression()?;
            Expr::Cond(Box::new(condition), Box::new(then), Box::new(otherwise))
        } else {
            condition
        };
        self.depth -= 1;
        Ok(out)
    }

    fn binary(&mut self, level: usize) -> Result<Expr, Fail> {
        const LEVELS: &[&[&str]] = &[
            &["||", "??"],
            &["&&"],
            &["===", "!==", "==", "!="],
            &["<=", ">=", "<", ">"],
            &["+", "-"],
            &["*", "/", "%"],
        ];
        let Some(ops) = LEVELS.get(level) else {
            return self.unary();
        };
        let mut left = self.binary(level + 1)?;
        loop {
            let found = match self.peek() {
                Tok::Punct(p) => ops.iter().copied().find(|o| o == p),
                _ => None,
            };
            let Some(op) = found else {
                return Ok(left);
            };
            self.advance();
            let right = self.binary(level + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Expr, Fail> {
        if self.eat("!") {
            self.deeper()?;
            let inner = self.unary()?;
            self.depth -= 1;
            return Ok(Expr::Unary("!", Box::new(inner)));
        }
        if self.eat("-") {
            self.deeper()?;
            let inner = self.unary()?;
            self.depth -= 1;
            return Ok(Expr::Unary("-", Box::new(inner)));
        }
        if self.is_word("typeof") {
            self.advance();
            self.deeper()?;
            let inner = self.unary()?;
            self.depth -= 1;
            return Ok(Expr::Unary("typeof", Box::new(inner)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, Fail> {
        let mut out = self.primary()?;
        loop {
            if self.is(".") || self.is("?.") {
                let optional = self.is("?.");
                self.advance();
                let Tok::Ident(name) = self.advance() else {
                    return Err(Fail::at(self.line(), "expected a name after `.`"));
                };
                out = Expr::Member(Box::new(out), name, optional);
            } else if self.eat("[") {
                let index = self.expression()?;
                self.expect("]")?;
                out = Expr::Index(Box::new(out), Box::new(index));
            } else if self.eat("(") {
                let mut args = Vec::new();
                while !self.is(")") {
                    args.push(self.argument()?);
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect(")")?;
                out = Expr::Call(Box::new(out), args);
            } else {
                return Ok(out);
            }
        }
    }

    /// An argument may be a callback; nothing else may.
    fn argument(&mut self) -> Result<Expr, Fail> {
        if self.is_word("function") {
            self.advance();
            self.expect("(")?;
            self.expect(")")?;
            return Ok(Expr::Func(Rc::new(self.block_body()?)));
        }
        let arrow = self.is("(")
            && matches!(
                self.tokens.get(self.at + 1).map(|t| &t.tok),
                Some(Tok::Punct(")"))
            )
            && matches!(
                self.tokens.get(self.at + 2).map(|t| &t.tok),
                Some(Tok::Punct("=>"))
            );
        if arrow {
            self.advance();
            self.advance();
            self.advance();
            if self.is("{") {
                return Ok(Expr::Func(Rc::new(self.block_body()?)));
            }
            let line = self.line();
            let kind = StmtKind::Expr(self.expression()?);
            return Ok(Expr::Func(Rc::new(vec![Stmt { line, kind }])));
        }
        self.expression()
    }

    fn primary(&mut self) -> Result<Expr, Fail> {
        let line = self.line();
        match self.advance() {
            Tok::Num(n) => Ok(Expr::Lit(Some(number(n)))),
            Tok::Str(s) => Ok(Expr::Lit(Some(Value::String(s)))),
            Tok::Template(parts) => {
                let mut out = Vec::new();
                for part in parts {
                    out.push(match part {
                        TemplatePart::Text(text) => TemplateExpr::Text(text),
                        TemplatePart::Code(code) => {
                            let mut tokens = lex(&code)?;
                            for token in &mut tokens {
                                token.line = line;
                            }
                            let mut inner = Parser {
                                tokens,
                                at: 0,
                                depth: self.depth + 1,
                            };
                            let expr = inner.expression()?;
                            if *inner.peek() != Tok::End {
                                return Err(Fail::at(line, "bad `${...}` in template string"));
                            }
                            TemplateExpr::Code(expr)
                        }
                    });
                }
                Ok(Expr::Template(out))
            }
            Tok::Punct("(") => {
                let inner = self.expression()?;
                self.expect(")")?;
                Ok(inner)
            }
            Tok::Punct("[") => {
                let mut items = Vec::new();
                while !self.is("]") {
                    items.push(self.expression()?);
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect("]")?;
                Ok(Expr::Array(items))
            }
            Tok::Punct("{") => {
                let mut fields = Vec::new();
                while !self.is("}") {
                    let name = match self.advance() {
                        Tok::Ident(name) | Tok::Str(name) => name,
                        other => {
                            return Err(Fail::at(
                                line,
                                format!("expected a field name, found {}", describe(&other)),
                            ));
                        }
                    };
                    self.expect(":")?;
                    fields.push((name, self.expression()?));
                    if !self.eat(",") {
                        break;
                    }
                }
                self.expect("}")?;
                Ok(Expr::Object(fields))
            }
            Tok::Ident(word) => match word.as_str() {
                "true" => Ok(Expr::Lit(Some(Value::Bool(true)))),
                "false" => Ok(Expr::Lit(Some(Value::Bool(false)))),
                "null" => Ok(Expr::Lit(Some(Value::Null))),
                "undefined" => Ok(Expr::Lit(None)),
                w if UNSUPPORTED_KEYWORDS.contains(&w) => {
                    Err(Fail::at(line, format!("`{w}` is not supported yet")))
                }
                _ => Ok(Expr::Ident(word)),
            },
            other => Err(Fail::at(line, format!("unexpected {}", describe(&other)))),
        }
    }
}

fn describe(tok: &Tok) -> String {
    match tok {
        Tok::Num(n) => format!("number {n}"),
        Tok::Str(_) | Tok::Template(_) => "a string".into(),
        Tok::Ident(name) => format!("`{name}`"),
        Tok::Punct(p) => format!("`{p}`"),
        Tok::End => "the end of the script".into(),
    }
}

// ---- Values ---------------------------------------------------------------

fn number(n: f64) -> Value {
    Number::from_f64(n).map_or(Value::Null, Value::Number)
}

fn to_number(v: &Val) -> f64 {
    match v {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                0.0
            } else {
                t.parse().unwrap_or(f64::NAN)
            }
        }
        Some(_) => f64::NAN,
    }
}

fn truthy(v: &Val) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn number_text(n: &Number) -> String {
    match n.as_f64() {
        Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
        _ => n.to_string(),
    }
}

/// What `String(v)` gives, except objects read as JSON rather than
/// `[object Object]`: that is more useful in a variable or a console line.
fn display(v: &Val) -> String {
    match v {
        None => "undefined".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => number_text(n),
        Some(other) => other.to_string(),
    }
}

fn type_name(v: &Val) -> &'static str {
    match v {
        None => "undefined",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(_) => "object",
    }
}

fn strict_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

fn loose_eq(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (None | Some(Value::Null), None | Some(Value::Null)) => true,
        (None | Some(Value::Null), _) | (_, None | Some(Value::Null)) => false,
        _ if type_name(a) == type_name(b) => strict_eq(a, b),
        (Some(Value::Array(_) | Value::Object(_)), _)
        | (_, Some(Value::Array(_) | Value::Object(_))) => false,
        _ => to_number(a) == to_number(b),
    }
}

fn nodes_within(v: &Value, budget: &mut usize) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    match v {
        Value::Array(items) => items.iter().all(|i| nodes_within(i, budget)),
        Value::Object(fields) => fields.values().all(|i| nodes_within(i, budget)),
        _ => true,
    }
}

fn dotted(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name) => Some(name.clone()),
        Expr::Member(object, name, _) => Some(format!("{}.{name}", dotted(object)?)),
        _ => None,
    }
}

/// `pm.expect(x).to.be.a` as (x's argument list, `["to", "be", "a"]`).
fn expect_chain(expr: &Expr) -> Option<(&Vec<Expr>, Vec<&str>)> {
    let mut words = Vec::new();
    let mut at = expr;
    while let Expr::Member(object, name, _) = at {
        words.push(name.as_str());
        at = object;
    }
    let Expr::Call(callee, args) = at else {
        return None;
    };
    if words.is_empty() || dotted(callee).as_deref() != Some("pm.expect") {
        return None;
    }
    words.reverse();
    Some((args, words))
}

// ---- Runner ---------------------------------------------------------------

struct Runner {
    vars: HashMap<String, Val>,
    env: HashMap<String, String>,
    headers: Vec<(String, String)>,
    url: String,
    body: String,
    response: Option<ResponseInput>,
    effects: Vec<Effect>,
    inside_test: bool,
}

impl Runner {
    fn new(input: HostInput) -> Self {
        Self {
            vars: HashMap::new(),
            env: input.env_vars.into_iter().collect(),
            headers: input.headers,
            url: input.url,
            body: input.body,
            response: input.response,
            effects: Vec::new(),
            inside_test: false,
        }
    }

    fn block(&mut self, statements: &[Stmt]) -> Result<(), Fail> {
        for statement in statements {
            self.statement(statement).map_err(|mut fail| {
                fail.line.get_or_insert(statement.line);
                fail
            })?;
        }
        Ok(())
    }

    fn statement(&mut self, statement: &Stmt) -> Result<(), Fail> {
        match &statement.kind {
            StmtKind::Let(name, value) => {
                let value = self.eval(value)?;
                check_size(&value)?;
                self.vars.insert(name.clone(), value);
            }
            StmtKind::Assign(target, value) => {
                let value = self.eval(value)?;
                check_size(&value)?;
                self.assign(target, value)?;
            }
            StmtKind::Expr(expr) => {
                self.eval(expr)?;
            }
            StmtKind::If(condition, then, otherwise) => {
                let condition = self.eval(condition)?;
                self.block(if truthy(&condition) { then } else { otherwise })?;
            }
            StmtKind::Block(statements) => self.block(statements)?,
        }
        Ok(())
    }

    fn assign(&mut self, target: &Expr, value: Val) -> Result<(), Fail> {
        match target {
            Expr::Ident(name) => {
                self.vars.insert(name.clone(), value);
                Ok(())
            }
            // The old-style Postman test: `tests["name"] = condition;`.
            Expr::Index(object, index) if matches!(&**object, Expr::Ident(n) if n == "tests") => {
                let name = display(&self.eval(index)?);
                self.effects.push(Effect::Test {
                    name,
                    passed: truthy(&value),
                });
                Ok(())
            }
            _ => Err(Fail::new("only plain variables can be assigned to")),
        }
    }

    fn eval(&mut self, expr: &Expr) -> Result<Val, Fail> {
        match expr {
            Expr::Lit(v) => Ok(v.clone()),
            Expr::Ident(name) => self.ident(name),
            Expr::Member(..) => self.member(expr),
            Expr::Index(object, index) => {
                let object = self.eval(object)?;
                let index = self.eval(index)?;
                let Some(object) = object.filter(|o| !o.is_null()) else {
                    return Err(Fail::new(format!(
                        "cannot read `{}` of undefined",
                        display(&index)
                    )));
                };
                Ok(match (&object, &index) {
                    (Value::Array(items), Some(Value::Number(n))) => n
                        .as_f64()
                        .filter(|f| *f >= 0.0)
                        .and_then(|f| items.get(f as usize).cloned()),
                    (Value::Object(fields), key) => fields.get(&display(key)).cloned(),
                    _ => None,
                })
            }
            Expr::Call(callee, args) => self.call(callee, args),
            Expr::Unary(op, inner) => {
                let inner = self.eval_lenient(*op, inner)?;
                Ok(Some(match *op {
                    "!" => Value::Bool(!truthy(&inner)),
                    "-" => number(-to_number(&inner)),
                    _ => Value::String(type_name(&inner).to_owned()),
                }))
            }
            Expr::Binary(op, left, right) => self.binary(op, left, right),
            Expr::Cond(condition, then, otherwise) => {
                if truthy(&self.eval(condition)?) {
                    self.eval(then)
                } else {
                    self.eval(otherwise)
                }
            }
            Expr::Array(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.eval(item)?.unwrap_or(Value::Null));
                }
                Ok(Some(Value::Array(out)))
            }
            Expr::Object(fields) => {
                let mut out = Map::new();
                for (name, value) in fields {
                    if let Some(value) = self.eval(value)? {
                        out.insert(name.clone(), value);
                    }
                }
                Ok(Some(Value::Object(out)))
            }
            Expr::Func(_) => Err(Fail::new("only `pm.test` takes a function")),
            Expr::Template(parts) => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        TemplateExpr::Text(text) => out.push_str(text),
                        TemplateExpr::Code(code) => out.push_str(&display(&self.eval(code)?)),
                    }
                    if out.len() > MAX_STRING_BYTES {
                        return Err(Fail::new("string too long"));
                    }
                }
                Ok(Some(Value::String(out)))
            }
        }
    }

    /// `typeof missing` is `"undefined"`, not an error.
    fn eval_lenient(&mut self, op: &str, inner: &Expr) -> Result<Val, Fail> {
        if op == "typeof"
            && let Expr::Ident(name) = inner
            && !self.vars.contains_key(name)
        {
            return Ok(None);
        }
        self.eval(inner)
    }

    fn ident(&mut self, name: &str) -> Result<Val, Fail> {
        if let Some(value) = self.vars.get(name) {
            return Ok(value.clone());
        }
        match name {
            "responseBody" => Ok(Some(Value::String(self.response_input(name)?.body.clone()))),
            "NaN" | "Infinity" => Ok(Some(Value::Null)),
            _ => Err(Fail::new(format!("`{name}` is not defined"))),
        }
    }

    fn response_input(&self, what: &str) -> Result<&ResponseInput, Fail> {
        self.response.as_ref().ok_or_else(|| {
            Fail::new(format!(
                "`{what}` is only available in the After response script"
            ))
        })
    }

    fn is_local_root(&self, path: &str) -> bool {
        self.vars
            .contains_key(path.split('.').next().unwrap_or_default())
    }

    fn member(&mut self, expr: &Expr) -> Result<Val, Fail> {
        let Expr::Member(object, name, optional) = expr else {
            return self.eval(expr);
        };
        if let Some((args, words)) = expect_chain(expr) {
            self.assertion(args, &words, None)?;
            return Ok(None);
        }
        if let Some(path) = dotted(expr)
            && !self.is_local_root(&path)
            && let Some(value) = self.global_value(&path)?
        {
            return Ok(value);
        }
        let object = self.eval(object)?;
        let Some(object) = object.filter(|o| !o.is_null()) else {
            if *optional {
                return Ok(None);
            }
            return Err(Fail::new(format!("cannot read `{name}` of undefined")));
        };
        Ok(match &object {
            Value::Object(fields) => fields.get(name).cloned(),
            Value::Array(items) if name == "length" => Some(number(items.len() as f64)),
            Value::String(text) if name == "length" => Some(number(text.chars().count() as f64)),
            _ => None,
        })
    }

    /// `Some` when `path` names a value of the host API.
    fn global_value(&self, path: &str) -> Result<Option<Val>, Fail> {
        Ok(Some(match path {
            "pm.response.code" | "responseCode.code" => {
                Some(number(f64::from(self.response_input(path)?.status)))
            }
            "pm.request.url" => Some(Value::String(self.url.clone())),
            "pm.request.body" => Some(Value::String(self.body.clone())),
            _ => return Ok(None),
        }))
    }

    fn binary(&mut self, op: &str, left: &Expr, right: &Expr) -> Result<Val, Fail> {
        let a = self.eval(left)?;
        match op {
            "&&" => return if truthy(&a) { self.eval(right) } else { Ok(a) },
            "||" => return if truthy(&a) { Ok(a) } else { self.eval(right) },
            "??" => {
                return if matches!(a, None | Some(Value::Null)) {
                    self.eval(right)
                } else {
                    Ok(a)
                };
            }
            _ => {}
        }
        let b = self.eval(right)?;
        let (x, y) = (to_number(&a), to_number(&b));
        Ok(Some(match op {
            "+" => {
                let textual = |v: &Val| {
                    matches!(
                        v,
                        Some(Value::String(_) | Value::Array(_) | Value::Object(_))
                    )
                };
                if textual(&a) || textual(&b) {
                    let (l, r) = (display(&a), display(&b));
                    if l.len() + r.len() > MAX_STRING_BYTES {
                        return Err(Fail::new("string too long"));
                    }
                    Value::String(l + &r)
                } else {
                    number(x + y)
                }
            }
            "-" => number(x - y),
            "*" => number(x * y),
            "/" => number(x / y),
            "%" => number(x % y),
            "===" => Value::Bool(strict_eq(&a, &b)),
            "!==" => Value::Bool(!strict_eq(&a, &b)),
            "==" => Value::Bool(loose_eq(&a, &b)),
            "!=" => Value::Bool(!loose_eq(&a, &b)),
            _ => {
                let ordering = match (&a, &b) {
                    (Some(Value::String(l)), Some(Value::String(r))) => l.partial_cmp(r),
                    _ => x.partial_cmp(&y),
                };
                let holds = ordering.is_some_and(|o| match op {
                    "<" => o.is_lt(),
                    ">" => o.is_gt(),
                    "<=" => o.is_le(),
                    _ => o.is_ge(),
                });
                Value::Bool(holds)
            }
        }))
    }

    fn call(&mut self, callee: &Expr, args: &[Expr]) -> Result<Val, Fail> {
        if let Expr::Member(object, method, _) = callee
            && let Some((subject, mut words)) = expect_chain(object).or_else(|| match &**object {
                Expr::Call(inner, subject) if dotted(inner).as_deref() == Some("pm.expect") => {
                    Some((subject, Vec::new()))
                }
                _ => None,
            })
        {
            words.push(method);
            let expected = match args.first() {
                Some(arg) => self.eval(arg)?,
                None => None,
            };
            self.assertion(subject, &words, Some(expected))?;
            return Ok(None);
        }
        if let Some(path) = dotted(callee)
            && !self.is_local_root(&path)
        {
            if path == "pm.test" {
                return self.pm_test(args);
            }
            let values = self.eval_args(args)?;
            return self.host_call(&path, &values);
        }
        let Expr::Member(object, method, optional) = callee else {
            return Err(Fail::new("this value cannot be called"));
        };
        let object = self.eval(object)?;
        let values = self.eval_args(args)?;
        let Some(object) = object.filter(|o| !o.is_null()) else {
            if *optional {
                return Ok(None);
            }
            return Err(Fail::new(format!("cannot call `{method}` of undefined")));
        };
        value_method(&object, method, &values)
    }

    fn eval_args(&mut self, args: &[Expr]) -> Result<Vec<Val>, Fail> {
        let mut out = Vec::with_capacity(args.len());
        for arg in args {
            out.push(self.eval(arg)?);
        }
        Ok(out)
    }

    fn pm_test(&mut self, args: &[Expr]) -> Result<Val, Fail> {
        let [name, Expr::Func(body)] = args else {
            return Err(Fail::new("`pm.test` takes a name and a function"));
        };
        let name = display(&self.eval(name)?);
        let outer = std::mem::replace(&mut self.inside_test, true);
        let body = Rc::clone(body);
        let outcome = self.block(&body);
        self.inside_test = outer;
        if let Err(fail) = &outcome {
            self.effects
                .push(Effect::Log(format!("{name}: {}", fail.message)));
        }
        self.effects.push(Effect::Test {
            name,
            passed: outcome.is_ok(),
        });
        Ok(None)
    }

    /// One `pm.expect(subject).to...` step. `expected` is `Some` for a call
    /// (`.eql(1)`), `None` for a property (`.to.be.true`).
    fn assertion(
        &mut self,
        subject: &[Expr],
        words: &[&str],
        expected: Option<Val>,
    ) -> Result<(), Fail> {
        let actual = match subject.first() {
            Some(expr) => self.eval(expr)?,
            None => None,
        };
        let negated = words.contains(&"not");
        let Some(check) = words.last().copied() else {
            return Ok(());
        };
        let wanted = expected.clone().flatten();
        let holds = match (check, expected.is_some()) {
            ("eql" | "equal" | "equals" | "eq", true) => strict_eq(&actual, &wanted),
            ("include" | "contain" | "contains" | "includes", true) => match &actual {
                Some(Value::String(text)) => text.contains(&display(&wanted)),
                Some(Value::Array(items)) => wanted.as_ref().is_some_and(|w| items.contains(w)),
                _ => false,
            },
            ("above" | "greaterThan" | "gt", true) => to_number(&actual) > to_number(&wanted),
            ("below" | "lessThan" | "lt", true) => to_number(&actual) < to_number(&wanted),
            ("a" | "an", true) => {
                let name = display(&wanted);
                type_name(&actual) == name
                    || (name == "array" && matches!(actual, Some(Value::Array(_))))
            }
            ("true", false) => actual == Some(Value::Bool(true)),
            ("false", false) => actual == Some(Value::Bool(false)),
            ("null", false) => actual == Some(Value::Null),
            ("undefined", false) => actual.is_none(),
            ("ok", false) => truthy(&actual),
            ("exist", false) => !matches!(actual, None | Some(Value::Null)),
            ("empty", false) => match &actual {
                Some(Value::String(s)) => s.is_empty(),
                Some(Value::Array(a)) => a.is_empty(),
                Some(Value::Object(o)) => o.is_empty(),
                _ => false,
            },
            _ => {
                return Err(Fail::new(format!(
                    "`pm.expect(...).{}` is not supported yet",
                    words.join(".")
                )));
            }
        };
        if holds != negated {
            return Ok(());
        }
        let shown = wanted
            .as_ref()
            .map(|w| format!(" {}", display(&Some(w.clone()))))
            .unwrap_or_default();
        let not = if negated { "not " } else { "" };
        Err(Fail::new(format!(
            "expected {} {not}{check}{shown}",
            display(&actual)
        )))
    }

    fn set_variable(&mut self, label: &str, args: &[Val]) -> Val {
        let name = display(args.first().unwrap_or(&None));
        let value = args.get(1).unwrap_or(&None);
        if value.is_none() {
            self.effects.push(Effect::Log(format!(
                "{label}: \"{name}\" not changed, the value is undefined"
            )));
            return None;
        }
        let text = display(value);
        self.env.insert(name.clone(), text.clone());
        self.effects.push(Effect::SetEnv(name, text));
        None
    }

    fn get_variable(&self, args: &[Val]) -> Val {
        self.env
            .get(&display(args.first().unwrap_or(&None)))
            .map(|v| Value::String(v.clone()))
    }

    fn host_call(&mut self, path: &str, args: &[Val]) -> Result<Val, Fail> {
        let first = args.first().unwrap_or(&None);
        Ok(match path {
            "pm.environment.set"
            | "pm.collectionVariables.set"
            | "pm.globals.set"
            | "pm.variables.set"
            | "postman.setEnvironmentVariable"
            | "postman.setGlobalVariable" => self.set_variable(path, args),
            "pm.environment.get"
            | "pm.collectionVariables.get"
            | "pm.globals.get"
            | "pm.variables.get"
            | "postman.getEnvironmentVariable"
            | "postman.getGlobalVariable" => self.get_variable(args),
            "pm.environment.has"
            | "pm.collectionVariables.has"
            | "pm.globals.has"
            | "pm.variables.has" => Some(Value::Bool(self.get_variable(args).is_some())),
            "pm.response.json" => {
                let text = &self.response_input(path)?.body;
                Some(
                    serde_json::from_str(text)
                        .map_err(|e| Fail::new(format!("the response body is not JSON: {e}")))?,
                )
            }
            "pm.response.text" => Some(Value::String(self.response_input(path)?.body.clone())),
            "pm.response.headers.get" => {
                self.response_input(path)?;
                let name = display(first);
                self.headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(&name))
                    .map(|(_, v)| Value::String(v.clone()))
            }
            "pm.response.to.have.status" => {
                let status = self.response_input(path)?.status;
                if f64::from(status) != to_number(first) {
                    return Err(Fail::new(format!(
                        "expected status {} but got {status}",
                        display(first)
                    )));
                }
                None
            }
            "pm.response.to.have.header" => {
                self.response_input(path)?;
                let name = display(first);
                if !self
                    .headers
                    .iter()
                    .any(|(k, _)| k.eq_ignore_ascii_case(&name))
                {
                    return Err(Fail::new(format!("expected header {name}")));
                }
                None
            }
            "pm.request.headers.add" | "pm.request.headers.upsert" => {
                let (Some(Value::Object(fields)), true) = (first, self.response.is_none()) else {
                    return Err(Fail::new(format!(
                        "`{path}` takes {{key, value}} in the Before request script"
                    )));
                };
                let field = |name: &str| display(&fields.get(name).cloned());
                self.effects
                    .push(Effect::SetHeader(field("key"), field("value")));
                None
            }
            "console.log" | "console.info" | "console.warn" | "console.error" | "console.debug" => {
                let line: Vec<String> = args.iter().map(display).collect();
                self.effects.push(Effect::Log(line.join(" ")));
                None
            }
            "JSON.parse" => Some(
                serde_json::from_str(&display(first))
                    .map_err(|e| Fail::new(format!("JSON.parse: {e}")))?,
            ),
            "JSON.stringify" => {
                let Some(value) = first else {
                    return Ok(None);
                };
                let pretty = args.get(2).is_some_and(truthy);
                let text = if pretty {
                    serde_json::to_string_pretty(value)
                } else {
                    serde_json::to_string(value)
                };
                Some(Value::String(
                    text.map_err(|e| Fail::new(format!("JSON.stringify: {e}")))?,
                ))
            }
            "String" => Some(Value::String(display(first))),
            "Number" | "parseFloat" => Some(number(to_number(first))),
            "parseInt" => Some(number(to_number(first).trunc())),
            "Boolean" => Some(Value::Bool(truthy(first))),
            "Math.floor" => Some(number(to_number(first).floor())),
            "Math.ceil" => Some(number(to_number(first).ceil())),
            "Math.round" => Some(number(to_number(first).round())),
            "Math.abs" => Some(number(to_number(first).abs())),
            "Date.now" => Some(number(chrono::Utc::now().timestamp_millis() as f64)),
            _ => {
                return Err(Fail::new(format!("`{path}` is not supported yet")));
            }
        })
    }
}

fn check_size(value: &Val) -> Result<(), Fail> {
    let mut budget = MAX_VALUE_NODES;
    match value {
        Some(v) if !nodes_within(v, &mut budget) => Err(Fail::new("value too large")),
        _ => Ok(()),
    }
}

fn value_method(object: &Value, method: &str, args: &[Val]) -> Result<Val, Fail> {
    let first = args.first().unwrap_or(&None);
    let text_arg = display(first);
    if let Value::String(text) = object {
        return Ok(Some(match method {
            "trim" => Value::String(text.trim().to_owned()),
            "toUpperCase" => Value::String(text.to_uppercase()),
            "toLowerCase" => Value::String(text.to_lowercase()),
            "toString" => Value::String(text.clone()),
            "includes" => Value::Bool(text.contains(&text_arg)),
            "startsWith" => Value::Bool(text.starts_with(&text_arg)),
            "endsWith" => Value::Bool(text.ends_with(&text_arg)),
            "indexOf" => number(
                text.find(&text_arg)
                    .map_or(-1.0, |b| text[..b].chars().count() as f64),
            ),
            "replace" => {
                Value::String(text.replacen(&text_arg, &display(args.get(1).unwrap_or(&None)), 1))
            }
            "split" => Value::Array(
                text.split(&text_arg)
                    .map(|p| Value::String(p.to_owned()))
                    .collect(),
            ),
            "substring" | "slice" => {
                let chars: Vec<char> = text.chars().collect();
                let at = |v: Option<&Val>, default: usize| match v {
                    Some(v @ Some(_)) => (to_number(v).max(0.0) as usize).min(chars.len()),
                    _ => default,
                };
                let (from, to) = (at(args.first(), 0), at(args.get(1), chars.len()));
                Value::String(chars[from.min(to)..to].iter().collect())
            }
            _ => {
                return Err(Fail::new(format!(
                    "string `.{method}` is not supported yet"
                )));
            }
        }));
    }
    if let Value::Array(items) = object {
        return Ok(Some(match method {
            "includes" => Value::Bool(first.as_ref().is_some_and(|w| items.contains(w))),
            "join" => {
                let parts: Vec<String> = items.iter().map(|i| display(&Some(i.clone()))).collect();
                Value::String(parts.join(&if first.is_some() {
                    text_arg
                } else {
                    ",".to_owned()
                }))
            }
            _ => return Err(Fail::new(format!("array `.{method}` is not supported yet"))),
        }));
    }
    match method {
        "toString" => Ok(Some(Value::String(display(&Some(object.clone()))))),
        _ => Err(Fail::new(format!("`.{method}` is not supported yet"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(body: &str) -> HostInput {
        HostInput {
            env_vars: vec![("host".into(), "api.test".into())],
            headers: vec![("Content-Type".into(), "application/json".into())],
            response: Some(ResponseInput {
                status: 200,
                body: body.into(),
            }),
            ..Default::default()
        }
    }

    fn logs(effects: &[Effect]) -> Vec<&str> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Log(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_signature_script_fills_the_environment() {
        let script = r#"
            var jsonData = JSON.parse(responseBody);
            pm.environment.set("ppobSignatureURL", jsonData.signature);
            pm.collectionVariables.set("ppobSignatureURL", jsonData.signature);
        "#;
        let effects = run(script, after(r#"{"signature":"abc123"}"#)).unwrap();
        let sets: Vec<_> = effects
            .iter()
            .map(|e| match e {
                Effect::SetEnv(k, v) => (k.as_str(), v.as_str()),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            sets,
            [
                ("ppobSignatureURL", "abc123"),
                ("ppobSignatureURL", "abc123")
            ]
        );
    }

    #[test]
    fn a_missing_field_warns_instead_of_storing_undefined() {
        let script = r#"pm.environment.set("x", pm.response.json().nope);"#;
        let effects = run(script, after("{}")).unwrap();
        assert!(!effects.iter().any(|e| matches!(e, Effect::SetEnv(..))));
        assert!(logs(&effects)[0].contains("undefined"));
    }

    #[test]
    fn values_read_back_nested_paths_and_console_output() {
        let script = r#"
            // a comment
            const d = pm.response.json();
            pm.environment.set("first", d.items[0].id);
            pm.environment.set('n', d.count + 1);
            console.log("host", pm.environment.get("host"), d.items.length, pm.environment.has("no"));
            console.log(`got ${d.items[0].id}!`, d.items[1]?.id);
            let label = d.count > 1 ? "many" : "one";
            if (pm.response.code === 200 && label == "many") { console.log(label) } else { console.log("no") }
        "#;
        let effects = run(script, after(r#"{"items":[{"id":"a"}],"count":2}"#)).unwrap();
        assert!(effects.contains_env("first", "a"));
        assert!(effects.contains_env("n", "3"));
        assert_eq!(
            logs(&effects),
            ["host api.test 1 false", "got a! undefined", "many"]
        );
    }

    trait Contains {
        fn contains_env(&self, key: &str, value: &str) -> bool;
    }

    impl Contains for Vec<Effect> {
        fn contains_env(&self, key: &str, value: &str) -> bool {
            self.iter()
                .any(|e| matches!(e, Effect::SetEnv(k, v) if k == key && v == value))
        }
    }

    #[test]
    fn tests_pass_fail_and_report_why() {
        let script = r#"
            pm.test("status", function () { pm.response.to.have.status(200); });
            pm.test("wrong status", () => { pm.response.to.have.status(404); });
            pm.test("field", function () {
                var d = pm.response.json();
                pm.expect(d.a).to.eql(1);
                pm.expect(d.a).to.not.equal(2);
                pm.expect(d.s).to.include("ell");
                pm.expect(d.t).to.be.true;
                pm.expect(d.none).to.be.undefined;
            });
            pm.test("failing expect", function () { pm.expect(1).to.be.above(2); });
            tests["old style"] = responseCode.code === 200;
        "#;
        let effects = run(script, after(r#"{"a":1,"s":"hello","t":true}"#)).unwrap();
        let results: Vec<_> = effects
            .iter()
            .filter_map(|e| match e {
                Effect::Test { name, passed } => Some((name.as_str(), *passed)),
                _ => None,
            })
            .collect();
        assert_eq!(
            results,
            [
                ("status", true),
                ("wrong status", false),
                ("field", true),
                ("failing expect", false),
                ("old style", true),
            ]
        );
        assert!(logs(&effects)[0].contains("expected status 404"));
    }

    #[test]
    fn what_is_missing_is_named_with_its_line() {
        for (script, expected) in [
            (
                "var a = 1;\nfor (;;) {}",
                "line 2: `for` is not supported yet",
            ),
            (
                "\n\nCryptoJS.HmacSHA256('a', 'b')",
                "line 3: `CryptoJS.HmacSHA256` is not supported yet",
            ),
            (
                "pm.sendRequest('x')",
                "line 1: `pm.sendRequest` is not supported yet",
            ),
            ("var a = {};\na.b.c", "line 2: cannot read `c` of undefined"),
            ("JSON.parse('{')", "line 1: JSON.parse:"),
            ("nope", "line 1: `nope` is not defined"),
        ] {
            let error = run(script, after("{}")).unwrap_err();
            assert!(error.starts_with(expected), "{script}: {error}");
        }
    }

    #[test]
    fn pre_request_scripts_have_no_response_but_can_add_headers() {
        let input = HostInput::default();
        let error = run("var b = responseBody;", input.clone()).unwrap_err();
        assert!(error.contains("After response"), "{error}");
        let effects = run(
            r#"pm.request.headers.add({key: "X-A", value: "1"});"#,
            input,
        )
        .unwrap();
        assert!(matches!(&effects[0], Effect::SetHeader(k, v) if k == "X-A" && v == "1"));
    }

    #[test]
    fn scripts_cannot_grow_without_bound_or_recurse_past_the_limit() {
        let doubling = format!("var a = 'xxxxxxxxxxxxxxxx';\n{}", "a = a + a;\n".repeat(40));
        assert!(
            run(&doubling, after("{}"))
                .unwrap_err()
                .contains("too long")
        );
        let nesting = format!("var a = [1];\n{}", "a = [a, a];\n".repeat(40));
        assert!(
            run(&nesting, after("{}"))
                .unwrap_err()
                .contains("too large")
        );
        let deep = format!("var a = {}1{};", "(".repeat(5000), ")".repeat(5000));
        assert!(run(&deep, after("{}")).unwrap_err().contains("nested"));
    }

    #[test]
    fn postman_scripts_are_told_from_rustman_ones() {
        for postman in [
            "pm.environment.set('a', 1)",
            "var d = JSON.parse(responseBody);",
            "console.log(1)",
            "tests['a'] = true",
        ] {
            assert!(is_postman_script(postman), "{postman}");
        }
        for rustman in [
            "let t = env(\"token\")\nset_header(\"A\", t)",
            "test(\"ok\", response.status == 200)",
            "let d = response.json()\nprint(d.pm)",
            "// pm.environment.set('a', 1)\n// console.log(1)",
            "",
        ] {
            assert!(!is_postman_script(rustman), "{rustman}");
        }
    }
}
