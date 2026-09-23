# LinearAlgebra-rs

A planned linear algebra expression workbench in Rust, with CLI and TUI interfaces built on a shared calculation core.

## Status

This repository currently contains project documentation and development conventions only. There is no runnable calculator, Rust package, dependency configuration, or CI workflow yet. All capabilities and examples below describe planned behavior, not existing APIs or commands.

## Goals

Make everyday linear algebra calculations quick to enter, inspect, and reuse through expressions and named variables.

- **CLI:** evaluate a single expression or keep working in an interactive session.
- **TUI:** enter expressions, browse variables, and inspect results and calculation steps.
- **Shared behavior:** both interfaces use the same calculation core, expression semantics, and variable handling.

The planned calculation modes are:

| Mode | Intended use | Result expectations |
| --- | --- | --- |
| Floating point | Everyday numerical calculations | Approximate results with explicit numerical limitations |
| Rational | Exact calculations with integers and fractions | Exact rational results and inspectable elimination steps |
| Symbolic | Expressions containing named parameters | Symbolic results with relevant assumptions and validity conditions |

Exact results must not silently become floating-point approximations. Mode selection and conversion rules will be defined with the corresponding implementation.

## Design examples

These examples illustrate the intended interaction. They are **not executable today**; syntax, vector representation, and function signatures remain subject to their implementation PRs.

```text
A = [[1, 2], [3, 4]]
b = [5, 6]
A * A
det(A)
solve(A, b)
```

A future symbolic session could introduce a parameter inside a matrix:

```text
S = [[x, 1], [0, x]]
det(S)
```

Symbolic operations will need to preserve conditions such as a parameter being nonzero when an inverse or a unique solution depends on it. The symbolic engine and supported expression set have not been selected.

## Roadmap

1. **Shared calculation core:** establish matrix and vector operations, dimension validation, linear systems, numerical and rational modes, and exact elimination steps through bounded increments.
2. **Expressions and variables:** add parsing, named values, and shared session behavior, with clear errors and explicit calculation modes.
3. **CLI:** expose single-expression evaluation and persistent interactive sessions.
4. **TUI:** add an expression workbench with variable browsing and result/step inspection using the shared core.
5. **Symbolic extensions:** add parameterized expressions and document supported operations, assumptions, and limitations.

The **first implementation task** is limited to a basic matrix type, dimension validation, addition, multiplication, and their tests. Parsing, interfaces, linear-system solvers, and symbolic algebra belong to subsequent tasks. Concrete mathematical libraries, syntax, and the symbolic engine will be decided in the relevant feature PRs.

## Development workflow

See [AGENTS.md](AGENTS.md) for the repository's development conventions.

- Start each task on a new `feat/`, `fix/`, or `docs/` branch based on the updated `main` branch.
- Make coherent commits using English Conventional Commits.
- Validate the change, push the branch, and open a PR targeting `main` that explains the problem, changes, validation, and limitations.
- Keep the PR open for review. Merging or enabling auto-merge requires explicit authorization from the repository owner.
- Update this English README when implemented user-facing behavior changes, keeping planned and available capabilities clearly distinguished.

The initial empty commit only establishes the default branch; project documentation and subsequent changes are delivered through PRs.

For this documentation-only stage, review Markdown structure, relative links, example labels, and the change scope, and run `git diff --check`. Cargo checks do not apply yet.

Once a Rust workspace exists, the required checks will be:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Tests should cover mathematical behavior, incompatible dimensions, relevant error paths, and mode-specific exactness or numerical tolerance. Any validation that could not run must be reported explicitly.
