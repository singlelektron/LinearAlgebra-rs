# LinearAlgebra-rs

An exact-first linear algebra workbench in Rust. Write mathematics, keep named
matrices, and inspect the calculation in a command line or terminal workspace.
Both interfaces use the same parser, session, and arithmetic core.

```text
A = [1 2; 3 4]
b = [5, 6]
solve(A, b)

     ⎡  -4 ⎤
     ⎣ 9/2 ⎦
```

## Run

Install a current stable Rust toolchain, then:

```sh
cargo run -- -e 'det([1 2; 3 4])'
cargo run -- repl
cargo run -- tui
```

For a local executable:

```sh
cargo install --path . --locked
la -e 'A = [1 2; 3 4]' -e 'inv(A)'
la --steps -e 'rref([1 2 3; 2 4 7])'
la --file examples/exact.la
printf 'A = [1 2; 3 4]\ndet(A)\n' | la
```

With no arguments, `la` opens the REPL when stdin is a terminal; otherwise it
reads a script from stdin. Quote expressions in the shell, especially matrix
semicolons, `*`, and transpose `'`. Use `la --help` for all command-line options.

## Mathematical input

| Input | Meaning |
| --- | --- |
| `A = [[1, 2], [3, 4]]` | A named 2 × 2 matrix |
| `A = [1 2; 3 4]` | The same matrix, with spaces between entries |
| `b = [5, 6]` | A column vector, shape 2 × 1 |
| `[5 6]` or `[[5, 6]]` | A row vector, shape 1 × 2 |
| `1/3`, `0.125`, `1e-3` | Exact rational literals in the default mode |
| `A + B`, `A - B`, `A * B` | Matrix addition, subtraction, multiplication |
| `2 * A`, `A / 3` | Scalar multiplication and division |
| `A^3`, `A^-1` | Integer matrix power; a negative power requires an inverse |
| `A'`, `transpose(A)` | Transpose |
| `ans` | The previous result |

Greek variable names, Unicode `×`, `·`, `⋅`, `÷`, `−`, and transpose `ᵀ` are
accepted: `α = 1/3; α × 6` and `Aᵀ * A` work as written.

Powers associate to the right: `2^3^2 = 512`; unary minus follows mathematical
precedence: `-2^2 = -4`. Use explicit multiplication (`2*x`, not `2x`).
In compact matrices, parentheses make compound entries unambiguous:
`[(1 + 2) (-3); 4 (5/7)]`. Nested comma notation also permits arbitrary scalar
expressions. Empty or ragged matrices are rejected.
In a compact row, `[1 -2]` has two entries; `[1 - 2]` is one subtraction.
Assignments store a snapshot; changing a variable later does not rewrite earlier
symbolic expressions.

Newlines and semicolons outside brackets separate statements. Matrices and
parenthesized expressions may span lines. `#` starts a comment. Failed scripts
do not partly change the session's variables.

| Function | Result |
| --- | --- |
| `det(A)` | Determinant of a square matrix |
| `inv(A)` | Inverse, if it exists |
| `trace(A)` | Trace of a square matrix |
| `rank(A)` | Rank; tolerance-dependent in floating-point mode |
| `rref(A)` | Reduced row echelon form |
| `solve(A, b)` | Unique solution of `A*x = b`, including multiple RHS columns |
| `transpose(A)` | Transpose |
| `augment(A, b)` | Horizontal concatenation with equal row counts |
| `eye(n)` | Identity matrix |
| `zeros(m, n)` | Zero matrix |

`solve` accepts consistent overdetermined systems with a unique solution.
Inconsistent systems and systems with free variables return an explanatory
error; parametric solution families are not implemented.

## Three explicit modes

| Mode | Semantics |
| --- | --- |
| `exact` (default) | Arbitrary-precision rational arithmetic. Decimal and scientific literals are parsed exactly. No silent floating-point fallback. |
| `float` | Finite `f64` arithmetic. Results are labeled approximate; display precision is separate from the elimination tolerance. |
| `symbolic` | Rational functions of commuting named parameters with rational coefficients. Unknown names become parameters. Nonzero assumptions are shown when division or elimination needs them. |

```sh
la --mode exact -e '0.1 + 0.2'       # 3/10
la --mode float -e '0.1 + 0.2'       # approximate
la --mode symbolic -e 'det([x 1; 0 x])'
la --mode symbolic -e 'inv([x 1; 0 x])'
```

Floating-point elimination uses partial pivoting and a relative zero threshold
based on the largest absolute entry of the coefficient matrix. The default
tolerance is `1e-12`. It is a numerical rank decision, not an error bound or a
proof of singularity. Ill-conditioned systems may require a different tolerance
or exact arithmetic; results are not accompanied by a condition estimate.
For systems with multiple right-hand sides, consistency is checked against each
RHS column's own scale; a large column cannot hide an inconsistent smaller one.
Floating-point determinant products retain a binary scale until the final
conversion, avoiding intermediate overflow or underflow when the result is
representable. A nonzero determinant that rounds to zero or exceeds the finite
`f64` range returns an explicit error.

Symbolic determinants use a division-free algorithm, so `det([x 1; 0 x]) = x^2`
does not require `x` to be nonzero. Inverses and row reduction can require such
conditions. Those conditions remain attached when stored results are reused.
Symbolic elimination describes the generic branch under the displayed
assumptions; it does not enumerate special cases such as `x = 0`. Simplification
is bounded and is not a complete computer algebra system. There are no symbolic
transcendental functions, complex numbers, eigenvalue decompositions, or SVD.

## Terminal workspace

The TUI presents an expression editor, a scrollable calculation transcript, and
a variable browser. Wide terminals show the variables alongside the work;
smaller terminals prioritize the calculation. Matrix columns are aligned and
rendered with mathematical brackets. Exact fractions remain fractions.
Stored symbolic assumptions appear alongside variables marked `conditional` in
both the sidebar and the Tab browser. Clearing the notebook with Ctrl+L keeps
these assumptions visible with their values; long conditions wrap and can be
scrolled in the browser.

| Key | Action |
| --- | --- |
| Enter | Evaluate the editor contents |
| Alt+Enter or Ctrl+J | Insert a newline |
| Up / Down | Recall history and return to the unfinished input |
| Left / Right, Home / End | Move the input cursor |
| PageUp / PageDown | Scroll results |
| Alt+Left / Alt+Right | Scroll wide results horizontally |
| Tab | Open the variable browser; Up/Down and PageUp/PageDown scroll, Esc closes |
| F1 | Toggle the guide |
| Ctrl+C | Clear input |
| Ctrl+L | Clear the notebook display |
| Ctrl+Q | Quit and restore the terminal |

The CLI supports `--ascii` for plain terminals and `--latex` for a standalone
LaTeX document with matrices and exact fractions:

```sh
la --latex -e 'A = [1 2; 3 4]' -e 'inv(A)' > calculation.tex
la --ascii --steps -e 'rref([1 2 3; 2 4 7])'
```

LaTeX output is source code; compiling it requires a separate TeX installation.
Single Latin and standard Greek parameters use mathematical glyphs; multi-letter
parameters use upright, grouped names so `ab^2` is distinct from `(a*b)^2`.
Other Unicode identifier characters are preserved as explicit `[U+XXXX]` labels
in portable pdfLaTeX output, including in assumptions and explanatory text.
Session variables live in memory. Keep a `.la` script to reproduce a calculation.

## Session commands

These commands work in both interfaces and scripts:

| Command | Action |
| --- | --- |
| `:help` | Syntax, functions, and commands |
| `:vars` | Inspect named variables |
| `:clear` | Clear the workspace and associated assumptions |
| `:mode exact`, `:mode float`, `:mode symbolic` | Select a mode; changing modes clears variables with an explicit message |
| `:precision 12` | Set display precision (1–16 significant digits); exact values are unchanged |
| `:tolerance 1e-12` | Set floating-point elimination tolerance, strictly between 0 and 1 |
| `:steps on`, `:steps off` | Show or hide the actual elementary row operations |
| `:quit` | Exit |

## Limits

This is a small-matrix workbench. Matrix axes are limited to 64, with at most
4096 entries. Symbolic determinants are limited to order 8; symbolic expression
growth is limited to 256 terms per polynomial and total degree 128. Integer
powers have magnitude at most 128. Exact numerators and denominators are limited
to 16,384 bits; number literals to 4096 bytes and scientific exponents/decimal
scale to magnitude 4096. A script may contain at most 65,536 bytes and 256
statements; expression nesting is limited to 128. These limits reject excessive
calculations with an error. There is no implicit
conversion between arithmetic modes. Eigenvalues, decompositions, graphical
plots, persistent session storage, and parametric solution families remain
future work.

## Development

See [AGENTS.md](AGENTS.md) for contribution rules and
[docs/implementation.md](docs/implementation.md) for architecture and shared
interfaces. The arithmetic core, parser/session, and presentation are separate.

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Tests cover mathematical identities, exact fractions, numerical residuals,
symbolic assumptions, parser precedence, failed-input state, terminal input,
rendering, and command-line workflows. GitHub Actions runs the same quality
checks. Develop on a task branch, make coherent English Conventional Commits,
and open a PR against `main`. Do not merge without the owner's approval.
