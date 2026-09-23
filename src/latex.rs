//! ASCII-only TeX encoding shared by symbolic values and explanatory text.
//!
//! The generated document needs only T1 fonts and amsmath. Unicode without a
//! portable glyph is preserved as an explicit code-point label, not passed to
//! pdfLaTeX or silently replaced by a different identifier.

fn greek(character: char) -> Option<&'static str> {
    Some(match character {
        'α' => r"\alpha",
        'β' => r"\beta",
        'γ' => r"\gamma",
        'δ' => r"\delta",
        'ε' => r"\varepsilon",
        'ζ' => r"\zeta",
        'η' => r"\eta",
        'θ' => r"\theta",
        'ι' => r"\iota",
        'κ' => r"\kappa",
        'λ' => r"\lambda",
        'μ' => r"\mu",
        'ν' => r"\nu",
        'ξ' => r"\xi",
        'ο' => r"\mathrm{o}",
        'π' => r"\pi",
        'ρ' => r"\rho",
        'σ' => r"\sigma",
        'ς' => r"\varsigma",
        'τ' => r"\tau",
        'υ' => r"\upsilon",
        'φ' => r"\phi",
        'χ' => r"\chi",
        'ψ' => r"\psi",
        'ω' => r"\omega",
        'Α' => r"\mathrm{A}",
        'Β' => r"\mathrm{B}",
        'Γ' => r"\Gamma",
        'Δ' => r"\Delta",
        'Ε' => r"\mathrm{E}",
        'Ζ' => r"\mathrm{Z}",
        'Η' => r"\mathrm{H}",
        'Θ' => r"\Theta",
        'Ι' => r"\mathrm{I}",
        'Κ' => r"\mathrm{K}",
        'Λ' => r"\Lambda",
        'Μ' => r"\mathrm{M}",
        'Ν' => r"\mathrm{N}",
        'Ξ' => r"\Xi",
        'Ο' => r"\mathrm{O}",
        'Π' => r"\Pi",
        'Ρ' => r"\mathrm{P}",
        'Σ' => r"\Sigma",
        'Τ' => r"\mathrm{T}",
        'Υ' => r"\Upsilon",
        'Φ' => r"\Phi",
        'Χ' => r"\mathrm{X}",
        'Ψ' => r"\Psi",
        'Ω' => r"\Omega",
        'ϵ' => r"\epsilon",
        'ϑ' => r"\vartheta",
        'ϖ' => r"\varpi",
        'ϱ' => r"\varrho",
        'ϕ' => r"\varphi",
        _ => return None,
    })
}

fn codepoint(character: char) -> String {
    format!("[U+{:04X}]", character as u32)
}

/// One atomic identifier, so an exponent applies to its entire name. Roman
/// multi-letter names remain distinguishable from products of Latin letters.
/// Greek commands ignore `\mathrm`, so compound names containing Greek also
/// need visible delimiters to distinguish them from products of parameters.
pub(crate) fn symbol(name: &str) -> String {
    let mut characters = name.chars();
    if let Some(character) = characters.next()
        && characters.next().is_none()
    {
        if character.is_ascii_alphabetic() {
            return character.to_string();
        }
        if let Some(command) = greek(character) {
            return command.to_owned();
        }
    }
    let mut result = String::new();
    let mut contains_greek = false;
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            result.push(character);
        } else if character == '_' {
            result.push_str(r"\_");
        } else if let Some(command) = greek(character) {
            contains_greek = true;
            result.push_str(&format!("{{{command}}}"));
        } else {
            result.push_str(&format!("\\text{{{}}}", codepoint(character)));
        }
    }
    let identifier = format!("\\mathrm{{{result}}}");
    if contains_greek {
        format!("\\mathord{{\\langle{identifier}\\rangle}}")
    } else {
        identifier
    }
}

/// Escape text in titles, commands, assumptions, and elimination steps.
pub(crate) fn text(input: &str) -> String {
    let mut escaped = String::new();
    for character in input.chars() {
        match character {
            '\\' => escaped.push_str(r"\textbackslash{}"),
            '{' => escaped.push_str(r"\{"),
            '}' => escaped.push_str(r"\}"),
            '$' => escaped.push_str(r"\$"),
            '&' => escaped.push_str(r"\&"),
            '#' => escaped.push_str(r"\#"),
            '_' => escaped.push_str(r"\_"),
            '%' => escaped.push_str(r"\%"),
            '~' => escaped.push_str(r"\textasciitilde{}"),
            '^' => escaped.push_str(r"\textasciicircum{}"),
            'ℚ' => escaped.push('Q'),
            'ℝ' => escaped.push('R'),
            '×' => escaped.push_str(r"\ensuremath{\times}"),
            '·' | '⋅' => escaped.push_str(r"\ensuremath{\cdot}"),
            '÷' => escaped.push_str(r"\ensuremath{\div}"),
            '→' => escaped.push_str(r"\ensuremath{\rightarrow}"),
            '←' => escaped.push_str(r"\ensuremath{\leftarrow}"),
            '↔' => escaped.push_str(r"\ensuremath{\leftrightarrow}"),
            '−' => escaped.push('-'),
            '≠' => escaped.push_str(r"\ensuremath{\ne}"),
            '≤' => escaped.push_str(r"\ensuremath{\le}"),
            '≥' => escaped.push_str(r"\ensuremath{\ge}"),
            '≈' => escaped.push_str(r"\ensuremath{\approx}"),
            'ᵀ' => escaped.push_str(r"\ensuremath{{}^{\mathsf{T}}}"),
            'ᵢ' => escaped.push_str(r"\ensuremath{{}_i}"),
            'ⱼ' => escaped.push_str(r"\ensuremath{{}_j}"),
            '₀'..='₉' => escaped.push_str(&format!(
                "\\ensuremath{{{{}}_{}}}",
                character as u32 - '₀' as u32
            )),
            character => {
                if let Some(command) = greek(character) {
                    escaped.push_str(&format!("\\ensuremath{{{command}}}"));
                } else if character.is_ascii_graphic() || character.is_ascii_whitespace() {
                    escaped.push(character);
                } else {
                    escaped.push_str(&format!("\\texttt{{{}}}", codepoint(character)));
                }
            }
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greek_glyphs_are_consistent_in_math_and_text() {
        for letter in "αβγδεζηθικλμνξοπρσςτυφχψωΑΒΓΔΕΖΗΘΙΚΛΜΝΞΟΠΡΣΤΥΦΧΨΩϵϑϖϱϕ".chars()
        {
            let name = letter.to_string();
            let math = symbol(&name);
            assert!(math.is_ascii());
            assert_eq!(text(&name), format!("\\ensuremath{{{math}}}"));
        }
    }

    #[test]
    fn arbitrary_unicode_and_tex_metacharacters_cannot_escape_their_node() {
        for name in ["Ж", "变量", "é", "𝛼", "x₂", "x²", r"\input{file}", "$#%&"] {
            assert!(symbol(name).is_ascii());
            assert!(text(name).is_ascii());
            assert!(!symbol(name).contains(r"\input"));
            assert!(!text(name).contains(r"\input"));
        }
        assert_eq!(symbol("αx"), r"\mathord{\langle\mathrm{{\alpha}x}\rangle}");
        assert_eq!(symbol("x_1"), r"\mathrm{x\_1}");
    }

    #[test]
    fn compound_greek_identifiers_are_visibly_delimited_as_single_atoms() {
        use crate::core::Mode;
        use crate::session::Session;

        for (name, product) in [("αβ", "α*β"), ("ΓΔ", "Γ*Δ"), ("αx", "α*x")] {
            let mut session = Session::new(Mode::Symbolic);
            let named = session.execute(&format!("{name}^2")).unwrap();
            let multiplied = session.execute(&format!("({product})^2")).unwrap();
            let named = crate::format::render_latex(&named, &session);
            let multiplied = crate::format::render_latex(&multiplied, &session);
            assert!(named.is_ascii(), "{named}");
            assert!(named.contains(r"\mathord{\langle"), "{named}");
            assert!(named.contains(r"\rangle}^{2}"), "{named}");
            assert!(!multiplied.contains(r"\langle"), "{multiplied}");
            assert!(!multiplied.contains(r"\rangle"), "{multiplied}");
        }
    }
}
