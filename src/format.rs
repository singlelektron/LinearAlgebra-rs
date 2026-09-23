//! Presentation shared by the command line and terminal workspace.

use unicode_width::UnicodeWidthStr;

use crate::core::{Mode, Scalar, Value};
use crate::session::{Output, Session};

pub fn render_value(value: &Value, precision: usize, ascii: bool) -> String {
    let rendered = match value {
        Value::Scalar(scalar) => scalar.format(precision),
        Value::Matrix(matrix) => {
            let cells: Vec<Vec<String>> = matrix
                .data()
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| {
                            let text = cell.format(precision);
                            if ascii { ascii_text(&text) } else { text }
                        })
                        .collect()
                })
                .collect();
            let widths: Vec<usize> = (0..matrix.cols())
                .map(|col| cells.iter().map(|row| row[col].width()).max().unwrap_or(0))
                .collect();
            cells
                .iter()
                .enumerate()
                .map(|(index, row)| {
                    let (left, right) = if ascii || cells.len() == 1 {
                        ("[", "]")
                    } else if index == 0 {
                        ("⎡", "⎤")
                    } else if index + 1 == cells.len() {
                        ("⎣", "⎦")
                    } else {
                        ("⎢", "⎥")
                    };
                    let entries: Vec<String> = row
                        .iter()
                        .zip(&widths)
                        .map(|(cell, width)| format!("{}{cell}", " ".repeat(width - cell.width())))
                        .collect();
                    format!("{left} {} {right}", entries.join("  "))
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    };
    if ascii {
        ascii_text(&rendered)
    } else {
        rendered
    }
}

fn ascii_text(text: &str) -> String {
    let mut output = String::new();
    for character in text.chars() {
        match character {
            'ℚ' => output.push('Q'),
            'ℝ' => output.push('R'),
            '×' | '·' => output.push('*'),
            '→' => output.push_str("->"),
            '←' => output.push_str("<-"),
            '↔' => output.push_str("<->"),
            '−' => output.push('-'),
            '≠' => output.push_str("!="),
            '≤' => output.push_str("<="),
            '≥' => output.push_str(">="),
            '≈' => output.push('~'),
            '⎡' | '⎢' | '⎣' => output.push('['),
            '⎤' | '⎥' | '⎦' => output.push(']'),
            'ᵢ' => output.push_str("_i"),
            'ⱼ' => output.push_str("_j"),
            character if character.is_ascii() => output.push(character),
            character => output.push_str(&format!("\\u{{{:x}}}", character as u32)),
        }
    }
    output
}

fn mode_label(mode: Mode, tolerance: f64, ascii: bool) -> String {
    match mode {
        Mode::Exact => if ascii { "exact Q" } else { "exact ℚ" }.into(),
        Mode::Float => format!(
            "approximate {}; tolerance {}",
            if ascii { "R" } else { "ℝ" },
            tolerance
        ),
        Mode::Symbolic => "symbolic rational functions".into(),
    }
}

pub fn render_output(output: &Output, _session: &Session, ascii: bool) -> String {
    let mut sections = Vec::new();
    if let Some(value) = &output.value {
        let label = mode_label(output.mode, output.tolerance, ascii);
        let rendered = render_value(value, output.precision, ascii);
        match value {
            Value::Scalar(_) => sections.push(format!(
                "{} {} {rendered}  [{label}]",
                output.title,
                if output.mode == Mode::Float {
                    if ascii { "~=" } else { "≈" }
                } else {
                    "="
                }
            )),
            Value::Matrix(matrix) => sections.push(format!(
                "{}  [{}{}{}; {label}]\n{rendered}",
                output.title,
                matrix.rows(),
                if ascii { "x" } else { "×" },
                matrix.cols()
            )),
        }
    }
    if let Some(text) = &output.text {
        sections.push(text.clone());
    }
    if !output.conditions.is_empty() {
        sections.push(format!("Assumptions: {}", output.conditions.join(", ")));
    }
    if output.show_steps && !output.steps.is_empty() {
        sections.push(format!(
            "Elimination steps:\n{}",
            output
                .steps
                .iter()
                .enumerate()
                .map(|(index, step)| format!("  {}. {step}", index + 1))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let rendered = sections.join("\n\n");
    if ascii {
        ascii_text(&rendered)
    } else {
        rendered
    }
}

/// Escapes arbitrary user identifiers and session text for a LaTeX text node.
fn latex_text(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        match character {
            '\\' => escaped.push_str("\\textbackslash{}"),
            '{' => escaped.push_str("\\{"),
            '}' => escaped.push_str("\\}"),
            '$' => escaped.push_str("\\$"),
            '&' => escaped.push_str("\\&"),
            '#' => escaped.push_str("\\#"),
            '_' => escaped.push_str("\\_"),
            '%' => escaped.push_str("\\%"),
            '~' => escaped.push_str("\\textasciitilde{}"),
            '^' => escaped.push_str("\\textasciicircum{}"),
            'ℚ' => escaped.push('Q'),
            'ℝ' => escaped.push('R'),
            '×' => escaped.push_str(" x "),
            '→' => escaped.push_str("\\ensuremath{\\rightarrow}"),
            '←' => escaped.push_str("\\ensuremath{\\leftarrow}"),
            '↔' => escaped.push_str("\\ensuremath{\\leftrightarrow}"),
            '−' => escaped.push('-'),
            '≠' => escaped.push_str("\\ensuremath{\\ne}"),
            '≤' => escaped.push_str("\\ensuremath{\\le}"),
            '≥' => escaped.push_str("\\ensuremath{\\ge}"),
            '≈' => escaped.push_str("\\ensuremath{\\approx}"),
            'ᵢ' => escaped.push_str("\\ensuremath{{}_i}"),
            'ⱼ' => escaped.push_str("\\ensuremath{{}_j}"),
            '₀'..='₉' => escaped.push_str(&format!(
                "\\ensuremath{{{{}}_{}}}",
                character as u32 - '₀' as u32
            )),
            character if greek_command(character).is_some() => {
                escaped.push_str("\\ensuremath{\\");
                escaped.push_str(greek_command(character).unwrap_or_default());
                escaped.push('}');
            }
            character => escaped.push(character),
        }
    }
    escaped
}

fn greek_command(character: char) -> Option<&'static str> {
    Some(match character {
        'α' => "alpha",
        'β' => "beta",
        'γ' => "gamma",
        'δ' => "delta",
        'ε' => "epsilon",
        'ζ' => "zeta",
        'η' => "eta",
        'θ' => "theta",
        'ι' => "iota",
        'κ' => "kappa",
        'λ' => "lambda",
        'μ' => "mu",
        'ν' => "nu",
        'ξ' => "xi",
        'ο' => "mathrm{o}",
        'π' => "pi",
        'ρ' => "rho",
        'σ' => "sigma",
        'ς' => "varsigma",
        'τ' => "tau",
        'υ' => "upsilon",
        'φ' => "phi",
        'χ' => "chi",
        'ψ' => "psi",
        'ω' => "omega",
        'Γ' => "Gamma",
        'Δ' => "Delta",
        'Θ' => "Theta",
        'Λ' => "Lambda",
        'Ξ' => "Xi",
        'Π' => "Pi",
        'Σ' => "Sigma",
        'Υ' => "Upsilon",
        'Φ' => "Phi",
        'Ψ' => "Psi",
        'Ω' => "Omega",
        _ => return None,
    })
}

fn scalar_latex(scalar: &Scalar, precision: usize) -> String {
    scalar.to_latex(precision)
}

/// Render one safe LaTeX fragment; the CLI wraps fragments in a document.
pub fn render_latex(output: &Output, _session: &Session) -> String {
    let mut sections = Vec::new();
    if let Some(value) = &output.value {
        let rendered = match value {
            Value::Scalar(scalar) => scalar_latex(scalar, output.precision),
            Value::Matrix(matrix) => format!(
                "\\begin{{pmatrix}}\n{}\n\\end{{pmatrix}}",
                matrix
                    .data()
                    .iter()
                    .map(|row| row
                        .iter()
                        .map(|cell| scalar_latex(cell, output.precision))
                        .collect::<Vec<_>>()
                        .join(" & "))
                    .collect::<Vec<_>>()
                    .join(" \\\\\n")
            ),
        };
        sections.push(format!(
            "\\[\n\\text{{{}}} {} {rendered}\n\\]\n\\noindent\\textit{{{}}}\\par",
            latex_text(&output.title),
            if output.mode == Mode::Float {
                "\\approx"
            } else {
                "="
            },
            latex_text(&mode_label(output.mode, output.tolerance, true))
        ));
    }
    if let Some(text) = &output.text {
        sections.extend(
            text.lines()
                .map(|line| format!("\\noindent {}\\par", latex_text(line))),
        );
    }
    if !output.conditions.is_empty() {
        sections.push(format!(
            "\\noindent Assumptions: {}\\par",
            latex_text(&output.conditions.join(", "))
        ));
    }
    if output.show_steps && !output.steps.is_empty() {
        sections.push("\\begin{enumerate}".into());
        sections.extend(
            output
                .steps
                .iter()
                .map(|step| format!("\\item {}", latex_text(step))),
        );
        sections.push("\\end{enumerate}".into());
    }
    sections.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Matrix;

    #[test]
    fn matrix_columns_are_aligned_and_vectors_stay_vertical() {
        let matrix = Matrix::new(vec![
            vec![
                Scalar::integer(1, Mode::Exact),
                Scalar::integer(200, Mode::Exact),
            ],
            vec![
                Scalar::integer(-30, Mode::Exact),
                Scalar::integer(4, Mode::Exact),
            ],
        ])
        .unwrap();
        assert_eq!(
            render_value(&Value::Matrix(matrix), 6, false),
            "⎡   1  200 ⎤\n⎣ -30    4 ⎦"
        );
        let vector = Matrix::new(vec![
            vec![Scalar::integer(1, Mode::Exact)],
            vec![Scalar::integer(2, Mode::Exact)],
        ])
        .unwrap();
        assert_eq!(
            render_value(&Value::Matrix(vector), 6, true),
            "[ 1 ]\n[ 2 ]"
        );
    }

    #[test]
    fn tex_escapes_user_text() {
        assert_eq!(
            latex_text(r"A_1 & 50% \input{x}"),
            r"A\_1 \& 50\% \textbackslash{}input\{x\}"
        );
        assert_eq!(
            scalar_latex(&Scalar::parse("1/2", Mode::Exact).unwrap(), 6),
            r"\frac{1}{2}"
        );
        assert_eq!(
            latex_text("R₂ ← R₂ − α"),
            "R\\ensuremath{{}_2} \\ensuremath{\\leftarrow} R\\ensuremath{{}_2} - \\ensuremath{\\alpha}"
        );
    }

    #[test]
    fn ascii_steps_have_no_unicode_and_script_settings_are_preserved() {
        let mut session = Session::new(Mode::Exact);
        let outputs = session
            .execute_script(":steps on\nrref([1 2; 3 4])\n:mode float\n:precision 3\n1/3")
            .unwrap();
        let exact = render_output(&outputs[1], &session, true);
        assert!(exact.is_ascii(), "{exact}");
        assert!(exact.contains("exact Q"));
        assert!(exact.contains("Elimination steps"));
        let approximate = render_output(outputs.last().unwrap(), &session, false);
        assert!(approximate.contains("0.333"));
        assert!(approximate.contains("approximate"));
    }
}
