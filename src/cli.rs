//! Batch evaluation and an intentionally small, pipe-friendly REPL.

use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::core::Mode;
use crate::format::{render_latex, render_output};
use crate::session::Session;

const MAX_INPUT_BYTES: u64 = 65_536;

#[derive(Debug, Parser)]
#[command(
    name = "la",
    version,
    about = "An exact-first linear algebra workbench",
    after_help = "Examples:\n  la -e 'A = [1 2; 3 4]; inv(A)'\n  la --mode symbolic -e 'det([a 1; 0 a])'\n  la --steps -e 'rref([1 2 3; 2 4 7])'\n  printf 'A = [1 2; 3 4]\\ndet(A)\\n' | la\n  la tui\n\nMatrices: [1 2; 3 4] or [[1, 2], [3, 4]]. Vectors: [1, 2].\nType :help in an interactive session for functions and commands."
)]
struct Arguments {
    #[command(subcommand)]
    command: Option<Interface>,
    /// Evaluate an expression or script; repeat to share one session.
    #[arg(short = 'e', long = "eval", global = true, allow_hyphen_values = true)]
    expressions: Vec<String>,
    /// Evaluate a UTF-8 script after any --eval expressions.
    #[arg(long, global = true)]
    file: Option<PathBuf>,
    /// Arithmetic domain: exact, float, or symbolic.
    #[arg(long, default_value = "exact", global = true)]
    mode: String,
    /// Significant digits for approximate output (1 to 16).
    #[arg(long, default_value_t = 6, global = true)]
    precision: usize,
    /// Positive finite pivot tolerance in float mode.
    #[arg(long, default_value_t = 1e-12, global = true)]
    tolerance: f64,
    /// Show the elimination operations actually performed.
    #[arg(long, global = true)]
    steps: bool,
    /// Use plain ASCII brackets and presentation labels.
    #[arg(long, global = true)]
    ascii: bool,
    /// Write a standalone LaTeX document (batch input only).
    #[arg(long, global = true)]
    latex: bool,
}

#[derive(Debug, Subcommand)]
enum Interface {
    /// Start a line-oriented interactive session.
    Repl,
    /// Open the terminal workspace with history and variables.
    Tui,
}

pub fn run() -> Result<(), String> {
    let arguments = Arguments::parse();
    let mode: Mode = arguments.mode.parse().map_err(|error| format!("{error}"))?;
    if !(1..=16).contains(&arguments.precision) {
        return Err("precision must be between 1 and 16".into());
    }
    if !arguments.tolerance.is_finite() || arguments.tolerance <= 0.0 || arguments.tolerance >= 1.0
    {
        return Err("tolerance must be finite and strictly between 0 and 1".into());
    }
    if arguments.command.is_some()
        && (!arguments.expressions.is_empty() || arguments.file.is_some())
    {
        return Err("use either repl/tui or batch input (--eval/--file)".into());
    }
    let mut session = Session::new(mode);
    session.precision = arguments.precision;
    session.tolerance = arguments.tolerance;
    session.show_steps = arguments.steps;

    if let Some(interface) = arguments.command {
        if arguments.latex {
            return Err("--latex needs batch input (--eval, --file, or piped stdin)".into());
        }
        return match interface {
            Interface::Repl => run_repl(session, arguments.ascii),
            Interface::Tui => {
                if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                    return Err("the TUI needs an interactive terminal; use --eval or piped stdin for batch work".into());
                }
                crate::tui::run(session, arguments.ascii)
            }
        };
    }

    let mut scripts = arguments.expressions;
    if let Some(path) = arguments.file {
        let file = std::fs::File::open(&path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        scripts.push(read_script(file)?);
    }
    if scripts.is_empty() {
        if io::stdin().is_terminal() {
            if arguments.latex {
                return Err("--latex needs batch input (--eval, --file, or piped stdin)".into());
            }
            return run_repl(session, arguments.ascii);
        }
        scripts.push(read_script(io::stdin().lock())?);
    }

    let mut rendered = Vec::new();
    'scripts: for script in scripts {
        let outputs = session
            .execute_script(&script)
            .map_err(|error| error.to_string())?;
        for output in outputs {
            if output.quit {
                break 'scripts;
            }
            let text = if arguments.latex {
                render_latex(&output, &session)
            } else {
                render_output(&output, &session, arguments.ascii)
            };
            if !text.is_empty() {
                rendered.push(text);
            }
        }
    }
    let mut stdout = io::stdout().lock();
    if arguments.latex {
        writeln!(stdout, "\\documentclass{{article}}\n\\usepackage[T1]{{fontenc}}\n\\usepackage{{amsmath}}\n\\setcounter{{MaxMatrixCols}}{{64}}\n\\begin{{document}}")
            .map_err(|error| error.to_string())?;
    }
    if !rendered.is_empty() {
        writeln!(stdout, "{}", rendered.join("\n\n")).map_err(|error| error.to_string())?;
    }
    if arguments.latex {
        writeln!(stdout, "\\end{{document}}").map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn read_script(input: impl Read) -> Result<String, String> {
    let mut text = String::new();
    input
        .take(MAX_INPUT_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| format!("cannot read UTF-8 input: {error}"))?;
    if text.len() as u64 > MAX_INPUT_BYTES {
        return Err(format!("input exceeds {MAX_INPUT_BYTES} bytes"));
    }
    Ok(text)
}

/// Only determines whether to offer a continuation prompt. Syntax validation
/// and all expression semantics remain the session's responsibility.
fn needs_continuation(input: &str) -> bool {
    let mut brackets = 0_i64;
    let mut parentheses = 0_i64;
    for character in input
        .lines()
        .flat_map(|line| line.split('#').next().unwrap_or_default().chars())
    {
        match character {
            '[' => brackets += 1,
            ']' => brackets -= 1,
            '(' => parentheses += 1,
            ')' => parentheses -= 1,
            _ => {}
        }
    }
    brackets > 0 || parentheses > 0
}

fn run_repl(mut session: Session, ascii: bool) -> Result<(), String> {
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    if interactive {
        println!(
            "Linear Algebra  |  exact first, explicit approximations\n[1 2; 3 4]   det(A)   inv(A)   A'   :help   :quit\n"
        );
    }
    let mut input = String::new();
    loop {
        if interactive {
            let prompt = if input.is_empty() {
                format!("{}{} ", session.mode, if ascii { ">" } else { " ❯" })
            } else {
                "    ... ".into()
            };
            print!("{prompt}");
            io::stdout().flush().map_err(|error| error.to_string())?;
        }
        let mut line = String::new();
        let bytes = io::stdin()
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if bytes == 0 {
            if !input.trim().is_empty() {
                return Err("input ended with an incomplete expression".into());
            }
            if interactive {
                println!();
            }
            return Ok(());
        }
        input.push_str(&line);
        if needs_continuation(&input) && input.len() <= MAX_INPUT_BYTES as usize {
            continue;
        }
        if input.trim().is_empty() {
            input.clear();
            continue;
        }
        match session.execute_script(&input) {
            Ok(outputs) => {
                for output in outputs {
                    if output.quit {
                        return Ok(());
                    }
                    let text = render_output(&output, &session, ascii);
                    if !text.is_empty() {
                        println!("{text}\n");
                    }
                }
            }
            Err(error) => {
                if !interactive {
                    return Err(error.to_string());
                }
                eprintln!("error: {error}\n");
            }
        }
        input.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuation_tracks_matrix_and_function_delimiters() {
        assert!(needs_continuation("A = [1 2;\n"));
        assert!(needs_continuation("inv(\n[1 2; 3 4]"));
        assert!(!needs_continuation("A = [1 2; 3 4]"));
        assert!(!needs_continuation("[1 2]]"));
        assert!(!needs_continuation("1 + 1 # commentary ("));
    }

    #[test]
    fn script_reader_rejects_oversized_and_non_utf8_input() {
        assert!(read_script(&vec![b'1'; MAX_INPUT_BYTES as usize + 1][..]).is_err());
        assert!(read_script(&[0xff][..]).is_err());
        assert_eq!(
            read_script("det([1 2; 3 4])".as_bytes()).unwrap(),
            "det([1 2; 3 4])"
        );
    }

    #[test]
    fn options_are_available_after_the_subcommand() {
        let arguments =
            Arguments::try_parse_from(["la", "tui", "--mode", "symbolic", "--steps"]).unwrap();
        assert_eq!(arguments.mode, "symbolic");
        assert!(arguments.steps);
        let negative = Arguments::try_parse_from(["la", "-e", "-2^2"]).unwrap();
        assert_eq!(negative.expressions, ["-2^2"]);
    }
}
