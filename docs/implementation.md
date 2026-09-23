# Expression workbench design

The calculator keeps arithmetic, expression/session semantics, and terminal
presentation separate. The default is exact rational arithmetic. Floating-point
and symbolic modes are explicit and never silently substitute for exact results.

## Shared interfaces

- `core::{CalcError, CalcResult, Mode, Context, Scalar, Matrix, Value}` owns math.
- `Mode::{Exact, Float, Symbolic}` implements Display and FromStr.
- `Context::new(mode, tolerance)` has public `mode`, `tolerance`, `steps:
  Vec<String>`, and `conditions: BTreeSet<String>` fields.
- `Scalar::parse(&str, Mode)`, `Scalar::symbol(&str)`, and
  `Scalar::integer(i64, Mode)` create numbers. Parse returns CalcResult.
- Scalar `add/sub/mul/div(&self, &Self, &mut Context)` and
  `pow(&self, i32, &mut Context)` return CalcResult; `neg(&self)` returns Self.
  `as_i64()` returns CalcResult; `format(precision)` returns String.
- `Matrix::new(Vec<Vec<Scalar>>)` validates a nonempty rectangular matrix.
  `rows()`, `cols()`, `get(row,col)`, and `data()` expose immutable structure.
  `add/sub/mul(&self, &Self, &mut Context)`, `scale/div(&self, &Scalar,
  &mut Context)`, `pow(&self, i32, &mut Context)`, `transpose(&self)`,
  `trace/det(&self, &mut Context)`, `rank(&self, &mut Context)`,
  `rref/inverse(&self, &mut Context)`, `solve/augment(&self, &Self,
  &mut Context)` implement matrix operations. Rank returns usize; transpose
  returns Self; all other methods return CalcResult. Identity and zeros use
  `Matrix::identity(n, mode)` / `Matrix::zeros(rows, cols, mode)`, returning
  CalcResult.
- `Value::{Scalar(Scalar), Matrix(Matrix)}` is the expression value type.
- `Session::new(mode)`; public settings `mode`, `precision`, `tolerance`,
  `show_steps`; `variables()` returns `&BTreeMap<String, Value>`;
  `variable_conditions(name)` exposes the stored assumptions for each value,
  including conditions behind a constant symbolic rank result;
  `execute(&str)` returns `CalcResult<Output>`.
- `Output` has public `title: String`, `value: Option<Value>`,
  `text: Option<String>`, `steps: Vec<String>`, `conditions: Vec<String>`,
  and `quit: bool`, plus snapshots of `mode`, `precision`, `tolerance`, and
  `show_steps`. A later command must not relabel an earlier result.
  `execute_script(&str)` returns every output and applies state changes only
  after the entire script succeeds. Both interfaces share this behavior.
- `format::render_output(&Output, &Session, ascii: bool) -> String` and
  `format::render_value(&Value, precision: usize, ascii: bool) -> String`.

## Interaction

Support traditional nested matrices `[[1, 2], [3, 4]]`, compact matrices
`[1 2; 3 4]`, column vectors `[5, 6]`, fractions, named assignments,
`+ - * / ^`, postfix transpose `'`, and functions `det`, `inv`, `rank`,
`rref`, `solve`, `transpose`, `trace`, `eye`, `zeros`, and `augment`.
Semicolons outside brackets separate statements. Modes are exact, float,
and symbolic. Symbolic arithmetic is restricted to rational functions of
commuting parameters, with nonzero assumptions reported for divisions/pivots;
it is not a general-purpose computer algebra system.

Symbolic determinants use division-free expansion to preserve unconditional
polynomial identities. Elimination and inverses select a generic branch and
record any required nonzero conditions. Assigned values carry their dependencies'
conditions into later expressions. Exact values do not need numerical tolerances;
float mode uses partial pivoting with a scale-relative threshold.

Session commands: `:help`, `:vars`, `:clear`, `:mode exact|float|symbolic`,
`:precision N`, `:tolerance X`, `:steps on|off`, `:quit`. Mode changes clear
variables explicitly with an explanatory message; no implicit conversion.
CLI: `la -e EXPR` (repeatable), `la --file PATH`, stdin scripts, `la repl`,
`la tui`; `--mode`, `--precision`, `--tolerance`, `--steps`, `--ascii`,
and `--latex` control presentation. The TUI provides editable expression
input, history, workspace variables, result/steps scrolling, and help.

Resource limits and numerical/symbolic boundaries must be documented in the
README alongside runnable examples and tested as part of the implementation.
