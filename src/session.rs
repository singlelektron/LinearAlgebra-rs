//! Shared expression parsing, workspace state, and calculator commands.
//!
//! Statements are atomic as a batch: a failed expression leaves variables,
//! settings, and `ans` unchanged. Symbolic assumptions follow variable uses.
use crate::core::{CalcError, CalcResult, Context, Matrix, Mode, Scalar, Value};
use std::collections::{BTreeMap, BTreeSet};

const MAX_INPUT_BYTES: usize = 65_536;
const MAX_STATEMENTS: usize = 256;
const MAX_TOKENS: usize = 16_384;
const MAX_DEPTH: usize = 128;
const MAX_VARIABLES: usize = 256;
const MAX_IDENTIFIER_CHARS: usize = 64;

#[derive(Clone, Debug)]
pub struct Output {
    pub mode: Mode,
    pub precision: usize,
    pub tolerance: f64,
    pub show_steps: bool,
    pub title: String,
    pub value: Option<Value>,
    pub text: Option<String>,
    pub steps: Vec<String>,
    pub conditions: Vec<String>,
    pub quit: bool,
}

impl Session {
    fn message(&self, text: impl Into<String>) -> Output {
        Output {
            mode: self.mode,
            precision: self.precision,
            tolerance: self.tolerance,
            show_steps: self.show_steps,
            title: String::new(),
            value: None,
            text: Some(text.into()),
            steps: Vec::new(),
            conditions: Vec::new(),
            quit: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Session {
    pub mode: Mode,
    pub precision: usize,
    pub tolerance: f64,
    pub show_steps: bool,
    variables: BTreeMap<String, Value>,
    variable_conditions: BTreeMap<String, BTreeSet<String>>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new(Mode::Exact)
    }
}

impl Session {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            precision: 6,
            tolerance: 1e-12,
            show_steps: false,
            variables: BTreeMap::new(),
            variable_conditions: BTreeMap::new(),
        }
    }

    pub fn variables(&self) -> &BTreeMap<String, Value> {
        &self.variables
    }

    /// Evaluate a batch and return its final result. Use `execute_script` to
    /// retain every statement's output.
    pub fn execute(&mut self, input: &str) -> CalcResult<Output> {
        self.execute_script(input)?
            .pop()
            .ok_or_else(|| CalcError("Enter an expression or :help.".into()))
    }

    /// Evaluate newline/semicolon-separated statements as one transaction.
    pub fn execute_script(&mut self, input: &str) -> CalcResult<Vec<Output>> {
        if input.len() > MAX_INPUT_BYTES {
            return Err(CalcError(format!(
                "Input exceeds the {MAX_INPUT_BYTES}-byte limit."
            )));
        }
        let uncommented = remove_comments(input);
        let statements = split_statements(&uncommented)?;
        let mut working = self.clone();
        let mut outputs = Vec::with_capacity(statements.len());
        for (offset, statement) in statements {
            let output = if statement.starts_with(':') {
                working.command(statement)
            } else {
                working.expression(statement, input, offset)
            }
            .map_err(|error| {
                let (line, column) = location(input, offset);
                CalcError(format!(
                    "{error} (statement at line {line}, column {column})"
                ))
            })?;
            let quit = output.quit;
            outputs.push(output);
            if quit {
                break;
            }
        }
        *self = working;
        Ok(outputs)
    }

    fn expression(&mut self, statement: &str, source: &str, offset: usize) -> CalcResult<Output> {
        if !self.tolerance.is_finite() || self.tolerance <= 0.0 || self.tolerance >= 1.0 {
            return Err(CalcError(
                "Tolerance must be finite and strictly between 0 and 1; use :tolerance.".into(),
            ));
        }
        let tokens = lex(statement, source, offset)?;
        let mut context = Context::new(self.mode, self.tolerance);
        let mut parser = Parser {
            tokens,
            position: 0,
            depth: 0,
            source,
            variables: &self.variables,
            variable_conditions: &self.variable_conditions,
            context: &mut context,
        };
        let name = match (&parser.current().kind, parser.tokens.get(1)) {
            (
                TokenKind::Ident(name),
                Some(Token {
                    kind: TokenKind::Equal,
                    ..
                }),
            ) => {
                if name == "ans" {
                    return Err(parser.error("'ans' is reserved for the previous result."));
                }
                let name = name.clone();
                parser.position = 2;
                Some(name)
            }
            _ => None,
        };
        let value = parser.expression(0, false)?;
        if !matches!(parser.current().kind, TokenKind::End) {
            return Err(parser.error(match parser.current().kind {
                TokenKind::Equal => "Assignment needs a single variable name on the left, for example A = [1 2; 3 4].",
                _ => "Unexpected input after the expression. Use '*' for multiplication or ';' between statements.",
            }));
        }
        if let Some(name) = &name
            && !self.variables.contains_key(name)
            && self
                .variables
                .keys()
                .filter(|key| key.as_str() != "ans")
                .count()
                >= MAX_VARIABLES
        {
            return Err(CalcError(format!(
                "Workspace is limited to {MAX_VARIABLES} named variables; use :clear."
            )));
        }
        context.conditions.extend(value.conditions());
        if let Some(name) = &name {
            self.variables.insert(name.clone(), value.clone());
            self.variable_conditions
                .insert(name.clone(), context.conditions.clone());
        }
        self.variables.insert("ans".into(), value.clone());
        self.variable_conditions
            .insert("ans".into(), context.conditions.clone());
        Ok(Output {
            mode: self.mode,
            precision: self.precision,
            tolerance: self.tolerance,
            show_steps: self.show_steps,
            title: name.unwrap_or_else(|| "ans".into()),
            value: Some(value),
            text: None,
            steps: context.steps,
            conditions: context.conditions.into_iter().collect(),
            quit: false,
        })
    }

    fn command(&mut self, statement: &str) -> CalcResult<Output> {
        let words: Vec<_> = statement.split_whitespace().collect();
        let usage = |text: &str| CalcError(format!("Usage: {text}"));
        let no_args = |text: &str| {
            if words.len() == 1 {
                Ok(())
            } else {
                Err(usage(text))
            }
        };
        match words[0] {
            ":help" => {
                no_args(":help")?;
                Ok(self.message(HELP))
            }
            ":vars" => {
                no_args(":vars")?;
                if self.variables.is_empty() {
                    return Ok(
                        self.message("Workspace is empty. Assign a value with A = [1 2; 3 4].")
                    );
                }
                let mut lines = Vec::new();
                for (name, value) in &self.variables {
                    let description = match value {
                        Value::Scalar(scalar) => scalar.format(self.precision),
                        Value::Matrix(matrix) => {
                            format!("{} × {} matrix", matrix.rows(), matrix.cols())
                        }
                    };
                    lines.push(format!("{name} = {description}"));
                    if let Some(conditions) = self.variable_conditions.get(name)
                        && !conditions.is_empty()
                    {
                        lines.push(format!(
                            "  assuming {}",
                            conditions.iter().cloned().collect::<Vec<_>>().join(", ")
                        ));
                    }
                }
                Ok(self.message(lines.join("\n")))
            }
            ":clear" => {
                no_args(":clear")?;
                self.variables.clear();
                self.variable_conditions.clear();
                Ok(self.message("Workspace cleared, including ans and symbolic assumptions."))
            }
            ":mode" => {
                if words.len() == 1 {
                    return Ok(self.message(format!("Mode: {}", self.mode)));
                }
                if words.len() != 2 {
                    return Err(usage(":mode exact|float|symbolic"));
                }
                let mode: Mode = words[1].parse()?;
                if mode == self.mode {
                    return Ok(self.message(format!("Mode is already {mode}; workspace retained.")));
                }
                self.mode = mode;
                self.variables.clear();
                self.variable_conditions.clear();
                Ok(self.message(format!("Mode: {mode}. Workspace cleared to prevent implicit conversion between arithmetic modes.")))
            }
            ":precision" => {
                if words.len() != 2 {
                    return Err(usage(":precision N (1 through 16)"));
                }
                let precision = words[1]
                    .parse::<usize>()
                    .map_err(|_| usage(":precision N (1 through 16)"))?;
                if !(1..=16).contains(&precision) {
                    return Err(usage(":precision N (1 through 16)"));
                }
                self.precision = precision;
                Ok(self.message(format!(
                    "Floating-point display precision: {precision}. Exact values remain exact."
                )))
            }
            ":tolerance" => {
                if words.len() != 2 {
                    return Err(usage(":tolerance X (finite and strictly between 0 and 1)"));
                }
                let tolerance = words[1]
                    .parse::<f64>()
                    .map_err(|_| usage(":tolerance X (finite and strictly between 0 and 1)"))?;
                if !tolerance.is_finite() || tolerance <= 0.0 || tolerance >= 1.0 {
                    return Err(usage(":tolerance X (finite and strictly between 0 and 1)"));
                }
                self.tolerance = tolerance;
                Ok(self.message(format!("Floating-point pivot tolerance: {tolerance:e}.")))
            }
            ":steps" => {
                if words.len() != 2 {
                    return Err(usage(":steps on|off"));
                }
                self.show_steps = match words[1] {
                    "on" => true,
                    "off" => false,
                    _ => return Err(usage(":steps on|off")),
                };
                Ok(self.message(format!("Elimination steps: {}.", words[1])))
            }
            ":quit" | ":q" => {
                no_args(":quit")?;
                let mut output = self.message("Goodbye.");
                output.quit = true;
                Ok(output)
            }
            _ => Err(CalcError(format!(
                "Unknown command '{}'. Use :help for available commands.",
                words[0]
            ))),
        }
    }
}

const HELP: &str = "Exact-first linear algebra workbench\n\
  A = [1 2; 3 4]         compact matrix (spaces separate columns)\n\
  A = [[1, 2], [3, 4]]   nested matrix\n\
  b = [5, 6]             column vector (commas without row separators)\n\
  1/3 + 1/6              exact rational arithmetic\n\
  A' * A                 transpose and matrix product\n\
  A^(-1)                 inverse; powers have precedence over unary minus\n\
  solve(A, b)             solve a uniquely determined linear system\n\
Functions: det, inv, trace, transpose, rank, rref, solve, eye, zeros, augment\n\
Use + - * / ^ and parentheses; × · ⋅ ÷ − and ᵀ are accepted aliases.\n\
Separate statements with semicolons or newlines outside brackets/parentheses.\n\
# starts a comment until the end of its line.\n\
In compact rows, [1 -2] has two entries; [1 - 2] has one subtraction.\n\
ans refers to the previous expression result. Assignments snapshot values.\n\
:vars  :clear  :mode exact|float|symbolic  :precision N  :tolerance X\n\
:steps on|off  :help  :quit\n\
Mode changes clear the workspace. Symbolic mode supports rational functions\n\
of commuting parameters and reports nonzero conditions; it is not a general CAS.";

// Replace comments with spaces of the same byte length, so diagnostic locations
// continue referring to the user's original source, including Unicode comments.
fn remove_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_comment = false;
    for ch in input.chars() {
        if ch == '\n' {
            in_comment = false;
            result.push(ch);
        } else if ch == '#' || in_comment {
            in_comment = true;
            for _ in 0..ch.len_utf8() {
                result.push(' ');
            }
        } else {
            result.push(ch);
        }
    }
    result
}

fn location(source: &str, offset: usize) -> (usize, usize) {
    let before = &source[..offset.min(source.len())];
    let line = before.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    (line, column)
}

fn located_error(source: &str, offset: usize, message: &str) -> CalcError {
    let (line, column) = location(source, offset);
    CalcError(format!("{message} at line {line}, column {column}"))
}

fn split_statements(input: &str) -> CalcResult<Vec<(usize, &str)>> {
    let mut statements = Vec::new();
    let mut stack = Vec::new();
    let mut start = 0;
    fn push<'a>(
        input: &'a str,
        statements: &mut Vec<(usize, &'a str)>,
        start: usize,
        end: usize,
    ) -> CalcResult<()> {
        let raw = &input[start..end];
        let statement = raw.trim();
        if !statement.is_empty() {
            if statements.len() >= MAX_STATEMENTS {
                return Err(CalcError(format!(
                    "A batch is limited to {MAX_STATEMENTS} statements."
                )));
            }
            statements.push((start + raw.len() - raw.trim_start().len(), statement));
        }
        Ok(())
    }
    for (offset, ch) in input.char_indices() {
        match ch {
            '[' | '(' => {
                if stack.len() >= MAX_DEPTH {
                    return Err(located_error(
                        input,
                        offset,
                        "Expression nesting limit exceeded",
                    ));
                }
                stack.push((ch, offset));
            }
            ']' | ')' => {
                let expected = if ch == ']' { '[' } else { '(' };
                match stack.pop() {
                    Some((open, _)) if open == expected => {}
                    _ => {
                        return Err(located_error(
                            input,
                            offset,
                            "Mismatched closing bracket or parenthesis",
                        ));
                    }
                }
            }
            ';' | '\n' if stack.is_empty() => {
                push(input, &mut statements, start, offset)?;
                start = offset + ch.len_utf8();
            }
            _ => {}
        }
    }
    if let Some((open, offset)) = stack.pop() {
        return Err(located_error(input, offset, &format!("Unclosed '{open}'")));
    }
    push(input, &mut statements, start, input.len())?;
    Ok(statements)
}

#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Number(String),
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Transpose,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Semicolon,
    Equal,
    End,
}

#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    start: usize,
    space_before: bool,
}

fn lex(input: &str, source: &str, base: usize) -> CalcResult<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut position = 0;
    let mut spaced = false;
    while position < input.len() {
        let ch = input[position..]
            .chars()
            .next()
            .expect("valid character boundary");
        if ch.is_whitespace() {
            spaced = true;
            position += ch.len_utf8();
            continue;
        }
        if tokens.len() >= MAX_TOKENS {
            return Err(located_error(
                source,
                base + position,
                "Expression token limit exceeded",
            ));
        }
        let start = position;
        position += ch.len_utf8();
        let kind = match ch {
            '+' => TokenKind::Plus,
            '-' | '−' => TokenKind::Minus,
            '*' | '×' | '·' | '⋅' => TokenKind::Star,
            '/' | '÷' => TokenKind::Slash,
            '^' => TokenKind::Caret,
            '\'' | 'ᵀ' => TokenKind::Transpose,
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            '[' => TokenKind::LeftBracket,
            ']' => TokenKind::RightBracket,
            ',' => TokenKind::Comma,
            ';' => TokenKind::Semicolon,
            '=' => TokenKind::Equal,
            digit if digit.is_ascii_digit() || digit == '.' => {
                while input
                    .as_bytes()
                    .get(position)
                    .is_some_and(u8::is_ascii_digit)
                {
                    position += 1;
                }
                if ch != '.' && input.as_bytes().get(position) == Some(&b'.') {
                    position += 1;
                    while input
                        .as_bytes()
                        .get(position)
                        .is_some_and(u8::is_ascii_digit)
                    {
                        position += 1;
                    }
                }
                if input
                    .as_bytes()
                    .get(position)
                    .is_some_and(|byte| *byte == b'e' || *byte == b'E')
                {
                    position += 1;
                    if input
                        .as_bytes()
                        .get(position)
                        .is_some_and(|byte| *byte == b'+' || *byte == b'-')
                    {
                        position += 1;
                    }
                    let digits = position;
                    while input
                        .as_bytes()
                        .get(position)
                        .is_some_and(u8::is_ascii_digit)
                    {
                        position += 1;
                    }
                    if digits == position {
                        return Err(located_error(
                            source,
                            base + position,
                            "Scientific notation needs exponent digits",
                        ));
                    }
                }
                if &input[start..position] == "." {
                    return Err(located_error(
                        source,
                        base + start,
                        "A decimal point needs digits",
                    ));
                }
                TokenKind::Number(input[start..position].into())
            }
            letter if letter.is_alphabetic() || letter == '_' => {
                while let Some(next) = input[position..].chars().next() {
                    if next == 'ᵀ' || !(next.is_alphanumeric() || next == '_') {
                        break;
                    }
                    position += next.len_utf8();
                }
                let name = &input[start..position];
                if name.chars().count() > MAX_IDENTIFIER_CHARS {
                    return Err(located_error(
                        source,
                        base + start,
                        "Variable names are limited to 64 characters",
                    ));
                }
                TokenKind::Ident(name.into())
            }
            _ => {
                return Err(located_error(
                    source,
                    base + start,
                    &format!("Unexpected character '{ch}'"),
                ));
            }
        };
        tokens.push(Token {
            kind,
            start: base + start,
            space_before: spaced,
        });
        spaced = false;
    }
    tokens.push(Token {
        kind: TokenKind::End,
        start: base + input.len(),
        space_before: spaced,
    });
    Ok(tokens)
}

struct Parser<'a> {
    tokens: Vec<Token>,
    position: usize,
    depth: usize,
    source: &'a str,
    variables: &'a BTreeMap<String, Value>,
    variable_conditions: &'a BTreeMap<String, BTreeSet<String>>,
    context: &'a mut Context,
}

impl Parser<'_> {
    fn current(&self) -> &Token {
        &self.tokens[self.position]
    }
    fn error(&self, message: &str) -> CalcError {
        located_error(self.source, self.current().start, message)
    }
    fn consume(&mut self, kind: &TokenKind) -> bool {
        if &self.current().kind == kind {
            self.position += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, kind: &TokenKind, message: &str) -> CalcResult<()> {
        if self.consume(kind) {
            Ok(())
        } else {
            Err(self.error(message))
        }
    }
    fn expression(&mut self, minimum: u8, compact_entry: bool) -> CalcResult<Value> {
        if self.depth >= MAX_DEPTH {
            return Err(self.error("Expression nesting limit exceeded"));
        }
        self.depth += 1;
        let result = self.expression_inner(minimum, compact_entry);
        self.depth -= 1;
        result
    }
    fn expression_inner(&mut self, minimum: u8, compact_entry: bool) -> CalcResult<Value> {
        let mut left = self.prefix(compact_entry)?;
        loop {
            if matches!(self.current().kind, TokenKind::Transpose) {
                if 40 < minimum {
                    break;
                }
                self.position += 1;
                left = Value::Matrix(require_matrix(left, "transpose")?.transpose());
                continue;
            }
            let (power, right_power) = match self.current().kind {
                TokenKind::Plus | TokenKind::Minus => (10, 11),
                TokenKind::Star | TokenKind::Slash => (20, 21),
                TokenKind::Caret => (30, 30),
                _ => break,
            };
            if power < minimum {
                break;
            }
            // In a compact row, a spaced sign attached to its following term
            // starts a new entry: [1 -2], while [1 - 2] is subtraction.
            if compact_entry
                && matches!(self.current().kind, TokenKind::Plus | TokenKind::Minus)
                && self.current().space_before
                && self
                    .tokens
                    .get(self.position + 1)
                    .is_some_and(|next| !next.space_before && can_start_expression(&next.kind))
            {
                break;
            }
            let operator = self.current().kind.clone();
            self.position += 1;
            let right = self.expression(right_power, compact_entry)?;
            left = self.binary(left, operator, right)?;
        }
        Ok(left)
    }
    fn prefix(&mut self, compact_entry: bool) -> CalcResult<Value> {
        let token = self.current().kind.clone();
        match token {
            TokenKind::Number(number) => {
                self.position += 1;
                Ok(Value::Scalar(Scalar::parse(&number, self.context.mode)?))
            }
            TokenKind::Ident(name) => {
                self.position += 1;
                if (!compact_entry
                    || !self.current().space_before
                    || function_arity(&name).is_some())
                    && self.consume(&TokenKind::LeftParen)
                {
                    let mut arguments = Vec::new();
                    if !self.consume(&TokenKind::RightParen) {
                        loop {
                            if arguments.len() >= 3 {
                                return Err(self.error("Functions accept at most three arguments"));
                            }
                            arguments.push(self.expression(0, false)?);
                            if self.consume(&TokenKind::RightParen) {
                                break;
                            }
                            self.expect(
                                &TokenKind::Comma,
                                "Expected ',' between function arguments or ')' after them",
                            )?;
                        }
                    }
                    self.function(&name, arguments)
                } else if let Some(value) = self.variables.get(&name) {
                    let mode = match value {
                        Value::Scalar(scalar) => scalar.mode(),
                        Value::Matrix(matrix) => matrix.mode(),
                    };
                    if mode != self.context.mode {
                        return Err(self.error("Stored value belongs to a different arithmetic mode. Use :clear; switch modes with :mode to avoid implicit conversions"));
                    }
                    if let Some(conditions) = self.variable_conditions.get(&name) {
                        self.context.conditions.extend(conditions.iter().cloned());
                    }
                    Ok(value.clone())
                } else if name == "ans" {
                    Err(self.error("No previous result is available for 'ans'"))
                } else if self.context.mode == Mode::Symbolic {
                    Ok(Value::Scalar(Scalar::symbol(&name)))
                } else {
                    Err(self.error(&format!("Unknown variable '{name}'. Assign it first, or use :mode symbolic for parameters")))
                }
            }
            TokenKind::Plus => {
                self.position += 1;
                self.expression(25, compact_entry)
            }
            TokenKind::Minus => {
                self.position += 1;
                let value = self.expression(25, compact_entry)?;
                match value {
                    Value::Scalar(scalar) => Ok(Value::Scalar(scalar.neg())),
                    Value::Matrix(matrix) => Ok(Value::Matrix(
                        matrix.scale(&Scalar::integer(-1, self.context.mode), self.context)?,
                    )),
                }
            }
            TokenKind::LeftParen => {
                self.position += 1;
                let value = self.expression(0, false)?;
                self.expect(
                    &TokenKind::RightParen,
                    "Expected ')' to close the expression",
                )?;
                Ok(value)
            }
            TokenKind::LeftBracket => {
                self.position += 1;
                self.matrix()
            }
            _ => Err(self.error(
                "Expected a number, variable, matrix, function call, or parenthesized expression",
            )),
        }
    }

    fn matrix(&mut self) -> CalcResult<Value> {
        if self.consume(&TokenKind::RightBracket) {
            return Err(self.error("Matrices cannot be empty"));
        }
        if self.consume(&TokenKind::LeftBracket) {
            let mut rows = Vec::new();
            loop {
                let (row, _) = self.matrix_row(false)?;
                self.expect(
                    &TokenKind::RightBracket,
                    "Expected ']' after a nested matrix row",
                )?;
                rows.push(row);
                if rows.len() > 64 {
                    return Err(self.error("Matrices are limited to 64 rows"));
                }
                if self.consume(&TokenKind::RightBracket) {
                    break;
                }
                self.expect(&TokenKind::Comma, "Separate nested matrix rows with commas")?;
                self.expect(
                    &TokenKind::LeftBracket,
                    "Expected '[' to begin the next matrix row",
                )?;
            }
            return Ok(Value::Matrix(Matrix::new(rows)?));
        }
        let mut rows = Vec::new();
        let mut any_space_separator = false;
        let mut any_row_separator = false;
        loop {
            let (row, had_space) = self.matrix_row(true)?;
            any_space_separator |= had_space;
            rows.push(row);
            if rows.len() > 64 {
                return Err(self.error("Matrices are limited to 64 rows"));
            }
            if self.consume(&TokenKind::RightBracket) {
                break;
            }
            self.expect(
                &TokenKind::Semicolon,
                "Expected ';' between matrix rows or ']' after the matrix",
            )?;
            any_row_separator = true;
        }
        // A comma-only literal is a column vector. Semicolons or compact
        // whitespace explicitly request the conventional row-major layout.
        if !any_row_separator && !any_space_separator {
            rows = rows
                .pop()
                .expect("one matrix row")
                .into_iter()
                .map(|entry| vec![entry])
                .collect();
        }
        Ok(Value::Matrix(Matrix::new(rows)?))
    }

    fn matrix_row(&mut self, compact: bool) -> CalcResult<(Vec<Scalar>, bool)> {
        let mut row = Vec::new();
        let mut had_space = false;
        loop {
            let value = self.expression(0, compact)?;
            row.push(require_scalar(value, "matrix entries")?);
            if row.len() > 64 {
                return Err(self.error("Matrix rows are limited to 64 entries"));
            }
            if matches!(
                self.current().kind,
                TokenKind::RightBracket | TokenKind::Semicolon
            ) {
                break;
            }
            if self.consume(&TokenKind::Comma) {
                continue;
            }
            if self.current().space_before && can_start_expression(&self.current().kind) {
                had_space = true;
                continue;
            }
            return Err(
                self.error("Separate matrix entries with spaces or commas, and rows with ';'")
            );
        }
        Ok((row, had_space))
    }

    fn binary(&mut self, left: Value, operator: TokenKind, right: Value) -> CalcResult<Value> {
        let context = &mut *self.context;
        match (left, operator, right) {
            (Value::Scalar(a), TokenKind::Plus, Value::Scalar(b)) => Ok(Value::Scalar(a.add(&b, context)?)),
            (Value::Scalar(a), TokenKind::Minus, Value::Scalar(b)) => Ok(Value::Scalar(a.sub(&b, context)?)),
            (Value::Scalar(a), TokenKind::Star, Value::Scalar(b)) => Ok(Value::Scalar(a.mul(&b, context)?)),
            (Value::Scalar(a), TokenKind::Slash, Value::Scalar(b)) => Ok(Value::Scalar(a.div(&b, context)?)),
            (Value::Matrix(a), TokenKind::Plus, Value::Matrix(b)) => Ok(Value::Matrix(a.add(&b, context)?)),
            (Value::Matrix(a), TokenKind::Minus, Value::Matrix(b)) => Ok(Value::Matrix(a.sub(&b, context)?)),
            (Value::Matrix(a), TokenKind::Star, Value::Matrix(b)) => Ok(Value::Matrix(a.mul(&b, context)?)),
            (Value::Matrix(matrix), TokenKind::Star, Value::Scalar(scalar)) |
            (Value::Scalar(scalar), TokenKind::Star, Value::Matrix(matrix)) => Ok(Value::Matrix(matrix.scale(&scalar, context)?)),
            (Value::Matrix(matrix), TokenKind::Slash, Value::Scalar(scalar)) => Ok(Value::Matrix(matrix.div(&scalar, context)?)),
            (value, TokenKind::Caret, Value::Scalar(exponent)) => {
                let exponent = i32::try_from(exponent.as_i64()?).map_err(|_| CalcError("Power exponent must be an integer between -128 and 128.".into()))?;
                match value {
                    Value::Scalar(scalar) => Ok(Value::Scalar(scalar.pow(exponent, context)?)),
                    Value::Matrix(matrix) => Ok(Value::Matrix(matrix.pow(exponent, context)?)),
                }
            }
            (_, TokenKind::Slash, Value::Matrix(_)) => Err(CalcError("Division by a matrix is not defined. Use A * inv(B), or solve(B, A) for a linear system.".into())),
            (_, TokenKind::Caret, _) => Err(CalcError("A power exponent must be an integer scalar.".into())),
            _ => Err(CalcError("Addition and subtraction require two scalars or two matrices of the same dimensions. For a scalar diagonal shift, use A + c * eye(n).".into())),
        }
    }

    fn function(&mut self, name: &str, arguments: Vec<Value>) -> CalcResult<Value> {
        let arity = function_arity(name).ok_or_else(|| {
            self.error(&format!(
                "Unknown function '{name}'. Use :help for available functions"
            ))
        })?;
        if arguments.len() != arity {
            return Err(self.error(&format!(
                "{name} expects {arity} argument(s), received {}",
                arguments.len()
            )));
        }
        let mut arguments = arguments.into_iter();
        let first = arguments.next().expect("checked arity");
        let context = &mut *self.context;
        match name {
            "eye" => Ok(Value::Matrix(Matrix::identity(
                dimension(first, "eye")?,
                context.mode,
            )?)),
            "zeros" => Ok(Value::Matrix(Matrix::zeros(
                dimension(first, "zeros")?,
                dimension(arguments.next().expect("checked arity"), "zeros")?,
                context.mode,
            )?)),
            "det" => Ok(Value::Scalar(require_matrix(first, name)?.det(context)?)),
            "trace" => Ok(Value::Scalar(require_matrix(first, name)?.trace(context)?)),
            "inv" => Ok(Value::Matrix(
                require_matrix(first, name)?.inverse(context)?,
            )),
            "transpose" => Ok(Value::Matrix(require_matrix(first, name)?.transpose())),
            "rank" => Ok(Value::Scalar(Scalar::integer(
                require_matrix(first, name)?.rank(context)? as i64,
                context.mode,
            ))),
            "rref" => Ok(Value::Matrix(require_matrix(first, name)?.rref(context)?)),
            "solve" | "augment" => {
                let first = require_matrix(first, name)?;
                let second = require_matrix(arguments.next().expect("checked arity"), name)?;
                Ok(Value::Matrix(if name == "solve" {
                    first.solve(&second, context)?
                } else {
                    first.augment(&second, context)?
                }))
            }
            _ => unreachable!("function name checked above"),
        }
    }
}

fn function_arity(name: &str) -> Option<usize> {
    match name {
        "det" | "inv" | "trace" | "transpose" | "rank" | "rref" | "eye" => Some(1),
        "solve" | "zeros" | "augment" => Some(2),
        _ => None,
    }
}

fn can_start_expression(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Number(_)
            | TokenKind::Ident(_)
            | TokenKind::Plus
            | TokenKind::Minus
            | TokenKind::LeftParen
            | TokenKind::LeftBracket
    )
}

fn require_scalar(value: Value, operation: &str) -> CalcResult<Scalar> {
    match value {
        Value::Scalar(scalar) => Ok(scalar),
        Value::Matrix(_) => Err(CalcError(format!(
            "{operation} requires a scalar; received a matrix."
        ))),
    }
}

fn require_matrix(value: Value, operation: &str) -> CalcResult<Matrix> {
    match value {
        Value::Matrix(matrix) => Ok(matrix),
        Value::Scalar(_) => Err(CalcError(format!(
            "{operation} requires a matrix; received a scalar."
        ))),
    }
}

fn dimension(value: Value, operation: &str) -> CalcResult<usize> {
    let number = require_scalar(value, operation)?.as_i64()?;
    if !(1..=64).contains(&number) {
        return Err(CalcError(
            "Matrix dimensions must be integers between 1 and 64.".into(),
        ));
    }
    Ok(number as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar(session: &mut Session, input: &str) -> String {
        match session.execute(input).unwrap().value.unwrap() {
            Value::Scalar(scalar) => scalar.format(12),
            Value::Matrix(_) => panic!("expected scalar"),
        }
    }

    fn matrix(session: &mut Session, input: &str) -> Vec<Vec<String>> {
        match session.execute(input).unwrap().value.unwrap() {
            Value::Matrix(matrix) => matrix
                .data()
                .iter()
                .map(|row| row.iter().map(|scalar| scalar.format(12)).collect())
                .collect(),
            Value::Scalar(_) => panic!("expected matrix"),
        }
    }

    #[test]
    fn exact_arithmetic_and_conventional_precedence() {
        let mut session = Session::default();
        assert_eq!(scalar(&mut session, "1/3 + 1/6"), "1/2");
        assert_eq!(scalar(&mut session, "0.1 + 0.2"), "3/10");
        assert_eq!(scalar(&mut session, "1e-2 + .5"), "51/100");
        assert_eq!(scalar(&mut session, "-2^2"), "-4");
        assert_eq!(scalar(&mut session, "(-2)^2"), "4");
        assert_eq!(scalar(&mut session, "2^-2"), "1/4");
        assert_eq!(scalar(&mut session, "2^3^2"), "512");
        assert_eq!(scalar(&mut session, "3 − 2 × 4 ÷ 2"), "-1");
    }

    #[test]
    fn matrix_forms_vectors_and_transpose() {
        let mut session = Session::default();
        let compact = matrix(&mut session, "[1 2; 3 4]");
        assert_eq!(compact, matrix(&mut session, "[[1, 2], [3, 4]]"));
        assert_eq!(matrix(&mut session, "[5, 6]"), vec![vec!["5"], vec!["6"]]);
        assert_eq!(matrix(&mut session, "[5 6]"), vec![vec!["5", "6"]]);
        assert_eq!(
            matrix(&mut session, "[1 -2; -3 4]"),
            vec![vec!["1", "-2"], vec!["-3", "4"]]
        );
        assert_eq!(matrix(&mut session, "[1 - 2]"), vec![vec!["-1"]]);
        assert_eq!(matrix(&mut session, "[[1 -2, 3]]"), vec![vec!["-1", "3"]]);
        assert_eq!(
            matrix(&mut session, "[[1, 2], [3, 4]]'"),
            vec![vec!["1", "3"], vec!["2", "4"]]
        );
        assert_eq!(
            matrix(&mut session, "A=[1 2;3 4]; Aᵀ"),
            vec![vec!["1", "3"], vec!["2", "4"]]
        );
        assert_eq!(
            matrix(&mut session, "[[1/2, -1/3]]"),
            vec![vec!["1/2", "-1/3"]]
        );
    }

    #[test]
    fn shared_functions_and_snapshots() {
        let mut session = Session::default();
        assert_eq!(
            matrix(&mut session, "A=[2 0;0 3]; b=[4,9]; solve(A,b)"),
            vec![vec!["2"], vec!["3"]]
        );
        assert_eq!(scalar(&mut session, "det(A)"), "6");
        assert_eq!(scalar(&mut session, "trace(A)"), "5");
        assert_eq!(scalar(&mut session, "rank(A)"), "2");
        assert_eq!(
            matrix(&mut session, "inv(A) * A"),
            vec![vec!["1", "0"], vec!["0", "1"]]
        );
        assert_eq!(
            matrix(&mut session, "rref(augment(A,b))"),
            vec![vec!["1", "0", "2"], vec!["0", "1", "3"]]
        );
        assert_eq!(
            matrix(&mut session, "zeros(2,2)+eye(2)"),
            vec![vec!["1", "0"], vec!["0", "1"]]
        );
        assert_eq!(
            matrix(&mut session, "B=A; A=eye(2); B"),
            vec![vec!["2", "0"], vec!["0", "3"]]
        );
    }

    #[test]
    fn batches_are_atomic_and_multiline_literals_are_kept_together() {
        let mut session = Session::default();
        scalar(&mut session, "x=7");
        let outputs = session
            .execute_script("A = [1 2;\n3 4]\ny = det(A); y + x")
            .unwrap();
        assert_eq!(outputs.len(), 3);
        assert_eq!(scalar(&mut session, "ans"), "5");
        assert!(session.execute("x=8; z=1/0").is_err());
        assert_eq!(scalar(&mut session, "x"), "7");
        assert!(!session.variables().contains_key("z"));
        assert!(session.execute(":precision 12\nx = missing").is_err());
        assert_eq!(session.precision, 6);
        assert_eq!(scalar(&mut session, "(1 +\n2)"), "3");
    }

    #[test]
    fn command_validation_and_explicit_mode_changes() {
        let mut session = Session::default();
        scalar(&mut session, "x=1/3");
        session.execute(":mode exact").unwrap();
        assert!(session.variables().contains_key("x"));
        assert!(
            session
                .execute(":mode float")
                .unwrap()
                .text
                .unwrap()
                .contains("cleared")
        );
        assert!(session.variables().is_empty());
        for command in [
            ":precision 0",
            ":precision 17",
            ":precision -1",
            ":tolerance 0",
            ":tolerance NaN",
            ":tolerance inf",
            ":steps yes",
            ":clear extra",
            ":unknown",
        ] {
            assert!(session.execute(command).is_err(), "{command}");
        }
        session
            .execute(":precision 12\n:tolerance 1e-8\n:steps on")
            .unwrap();
        assert_eq!(session.precision, 12);
        assert_eq!(session.tolerance, 1e-8);
        assert!(session.show_steps);
        assert!(session.execute(":quit").unwrap().quit);
    }

    #[test]
    fn symbolic_dependencies_keep_nonzero_conditions() {
        let mut session = Session::new(Mode::Symbolic);
        let original = session.execute("A=inv([[x]])").unwrap();
        assert!(!original.conditions.is_empty());
        let reused = session.execute("B=A; det(B)").unwrap();
        assert!(!reused.conditions.is_empty());
        let cancelled = session.execute("a=x/x; b=a; b").unwrap();
        assert!(!cancelled.conditions.is_empty());
        assert!(session.execute("1").unwrap().conditions.is_empty());
        session.execute(":clear").unwrap();
        assert!(session.variables().is_empty());
    }

    #[test]
    fn invalid_inputs_are_errors_with_source_location() {
        let mut session = Session::default();
        for expression in [
            "[]",
            "[1 2;3]",
            "[[1,2],[3]]",
            "1 +",
            "(1]",
            "eye(0)",
            "zeros(1,65)",
            "solve([1], 2)",
            "det(1)",
            "det()",
            "det([1], [2])",
            "unknown(1)",
            "2x",
            "a =",
            "1=2",
            "ans=2",
            "1e-",
            "1/0",
            "2^0.5",
            "[1]/[2]",
            "[1] + 2",
            "1..2",
            "[1,,2]",
            "[1;]",
            "[1,]",
            "@",
            "det([1]) extra",
        ] {
            let error = session.execute(expression).unwrap_err().to_string();
            assert!(error.contains("line"), "{expression}: {error}");
        }
        assert!(
            session
                .execute("1\n2 + @")
                .unwrap_err()
                .to_string()
                .contains("line 2")
        );
        assert!(session.execute("π").is_err());
        session.execute(":mode symbolic").unwrap();
        assert!(session.execute("π + α").is_ok());
    }

    #[test]
    fn resource_limits_reject_before_unbounded_recursion() {
        let mut session = Session::default();
        assert!(session.execute(&" ".repeat(MAX_INPUT_BYTES + 1)).is_err());
        assert!(
            session
                .execute(&format!("{}1{}", "(".repeat(129), ")".repeat(129)))
                .is_err()
        );
        assert!(session.execute(&format!("{}1", "-".repeat(129))).is_err());
        assert!(session.execute(&"1;".repeat(257)).is_err());
        assert!(session.execute(&format!("{}=1", "a".repeat(65))).is_err());
    }
    #[test]
    fn comments_empty_batches_and_per_statement_settings() {
        let mut session = Session::default();
        assert!(
            session
                .execute_script(" # a comment with ] and (\n # 数学说明")
                .unwrap()
                .is_empty()
        );
        assert!(session.execute_script("\n ; ").unwrap().is_empty());
        assert_eq!(
            scalar(&mut session, "A=[1 2; # comment ]\n3 4]\ndet(A) # exact"),
            "-2"
        );
        let outputs = session
            .execute_script("1/3\n:mode float # change mode\n:precision 12\n1/3")
            .unwrap();
        assert_eq!(outputs[0].mode, Mode::Exact);
        assert_eq!(outputs[0].precision, 6);
        assert_eq!(outputs[3].mode, Mode::Float);
        assert_eq!(outputs[3].precision, 12);
        session.execute(":mode symbolic").unwrap();
        assert_eq!(
            matrix(&mut session, "[x (x + 1)]").first().unwrap().len(),
            2
        );
    }
    #[test]
    fn spaced_function_calls_in_compact_rows_preserve_column_boundaries() {
        let mut session = Session::default();
        assert_eq!(
            matrix(&mut session, "x=3; [det ([1]) x (x+1)]"),
            vec![vec!["1", "3", "4"]]
        );
        assert_eq!(
            matrix(&mut session, "[trace ([2]) -det ([3])]"),
            vec![vec!["2", "-3"]]
        );
    }

    #[test]
    fn direct_public_setting_changes_cannot_mislabel_stored_values() {
        let mut session = Session::default();
        session.execute("A=[1 2;3 4]; n=2").unwrap();
        session.mode = Mode::Float;
        for expression in ["A", "A'", "n", "zeros(n,n)"] {
            assert!(session.execute(expression).is_err(), "{expression}");
        }
        session.execute(":clear").unwrap();
        assert!(session.execute("1/3").is_ok());
        session.tolerance = f64::NAN;
        assert!(session.execute("1").is_err());
        session.execute(":tolerance 1e-12").unwrap();
        assert!(session.execute("1").is_ok());
    }
}
