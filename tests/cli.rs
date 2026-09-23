use std::io::Write;
use std::process::{Command, Stdio};

fn la() -> Command {
    Command::new(env!("CARGO_BIN_EXE_la"))
}

#[test]
fn repeated_expressions_share_variables() {
    let output = la()
        .args(["--ascii", "-e", "A = [1 2; 3 4]", "-e", "det(A)"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("-2"), "{stdout}");
    assert!(stdout.is_ascii());
}

#[test]
fn piped_script_is_noninteractive() {
    let mut process = la()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(b"A = [1 2; 3 4]\nb = [5, 6]\nsolve(A, b)\n")
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("9/2"), "{stdout}");
    assert!(!stdout.contains("la>"), "{stdout}");
}

#[test]
fn malformed_input_reports_error_and_failure_status() {
    for expr in ["det([1 2 3; 4 5 6])", "1/0", "[1 2; 3]", "2 +"] {
        let output = la().args(["-e", expr]).output().unwrap();
        assert!(!output.status.success(), "{expr}");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.is_empty(), "{expr}");
        assert!(!stderr.contains("panicked"), "{stderr}");
    }
}

#[test]
fn latex_output_is_a_document_with_exact_fractions() {
    let output = la()
        .args(["--latex", "-e", "inv([1 2; 3 4])"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\\documentclass"), "{stdout}");
    assert!(
        stdout.contains("\\setcounter{MaxMatrixCols}{64}"),
        "{stdout}"
    );
    assert!(stdout.contains("\\begin{pmatrix}"), "{stdout}");
    assert!(stdout.contains("\\frac"), "{stdout}");
    assert!(stdout.contains("\\end{document}"), "{stdout}");
}

#[test]
fn latex_handles_unicode_identifiers_in_values_titles_and_conditions() {
    let output = la()
        .args([
            "--latex",
            "--mode",
            "symbolic",
            "--steps",
            "-e",
            "ο = ς + 1; W = [文 ο; 0 α]; inv(W); :vars; :help",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.is_ascii(),
        "pdfLaTeX source contains raw Unicode: {stdout}"
    );
    assert!(stdout.contains("\\varsigma"), "{stdout}");
    assert!(stdout.contains("\\mathrm{o}"), "{stdout}");
    assert!(stdout.contains("[U+6587]"), "{stdout}");
    assert!(stdout.contains("Assumptions:"), "{stdout}");
}

#[test]
fn latex_keeps_multiletter_parameters_distinct_from_products() {
    let output = la()
        .args(["--latex", "--mode", "symbolic", "-e", "ab^2 + (a*b)^2"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("\\mathrm{ab}^{2}"), "{stdout}");
    assert!(stdout.contains("a^{2} b^{2}"), "{stdout}");
}

#[test]
fn mode_and_help_flags_are_usable() {
    assert!(la().arg("--help").output().unwrap().status.success());
    let symbolic = la()
        .args(["--mode", "symbolic", "-e", "det([x 1; 0 x])"])
        .output()
        .unwrap();
    assert!(
        symbolic.status.success(),
        "{}",
        String::from_utf8_lossy(&symbolic.stderr)
    );
    assert!(String::from_utf8_lossy(&symbolic.stdout).contains("x^2"));
    let invalid = la().args(["--mode", "wrong", "-e", "1"]).output().unwrap();
    assert!(!invalid.status.success());
}

#[test]
fn each_batch_result_keeps_its_own_arithmetic_mode() {
    let output = la()
        .args([
            "--ascii",
            "-e",
            "1/3\n:mode float\n1/3\n:mode symbolic\nx + 1",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let exact = stdout.lines().find(|line| line.contains("= 1/3")).unwrap();
    assert!(exact.contains("exact"), "{stdout}");
    let approximate = stdout
        .lines()
        .find(|line| line.contains("= 0.333"))
        .unwrap();
    assert!(approximate.contains("approximate"), "{stdout}");
}
