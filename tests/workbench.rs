use linearalgebra::core::{Mode, Value};
use linearalgebra::session::Session;

fn scalar(session: &mut Session, expr: &str) -> String {
    match session.execute(expr).unwrap().value.unwrap() {
        Value::Scalar(value) => value.format(12),
        Value::Matrix(_) => panic!("expected scalar for {expr}"),
    }
}

fn matrix(session: &mut Session, expr: &str) -> Vec<Vec<String>> {
    match session.execute(expr).unwrap().value.unwrap() {
        Value::Matrix(value) => value
            .data()
            .iter()
            .map(|row| row.iter().map(|entry| entry.format(12)).collect())
            .collect(),
        Value::Scalar(_) => panic!("expected matrix for {expr}"),
    }
}

#[test]
fn exact_workflow_from_the_original_design() {
    let mut session = Session::new(Mode::Exact);
    session.execute("A = [[1, 2], [3, 4]]").unwrap();
    session.execute("b = [5, 6]").unwrap();
    assert_eq!(scalar(&mut session, "det(A)"), "-2");
    assert_eq!(matrix(&mut session, "A * A"), [["7", "10"], ["15", "22"]]);
    assert_eq!(matrix(&mut session, "solve(A, b)"), [["-4"], ["9/2"]]);
    assert_eq!(matrix(&mut session, "A * solve(A, b)"), [["5"], ["6"]]);
}

#[test]
fn exact_decimal_input_and_conventional_precedence() {
    let mut session = Session::new(Mode::Exact);
    assert_eq!(scalar(&mut session, "0.1 + 0.2"), "3/10");
    assert_eq!(scalar(&mut session, "1e-3 + 2/1000"), "3/1000");
    assert_eq!(scalar(&mut session, "-2^2"), "-4");
    assert_eq!(scalar(&mut session, "2^3^2"), "512");
    assert_eq!(scalar(&mut session, "2^-3"), "1/8");
}

#[test]
fn compact_matrix_notation_and_transpose() {
    let mut session = Session::new(Mode::Exact);
    session.execute("A = [1 2 3; 4 5 6]").unwrap();
    assert_eq!(matrix(&mut session, "A' * [1, 2]"), [["9"], ["12"], ["15"]]);
    assert_eq!(scalar(&mut session, "rank(A)"), "2");
}

#[test]
fn exact_inverse_and_determinant_identity_for_varied_pivots() {
    let mut session = Session::new(Mode::Exact);
    for source in [
        "[0 1; 2 3]",
        "[2 1 0; 0 3 1; 1 0 4]",
        "[1/7 2/3; -3/5 11/13]",
        "[0 0 1; 0 2 3; 4 5 6]",
    ] {
        session.execute(&format!("A = {source}")).unwrap();
        let n = match session.variables().get("A").unwrap() {
            Value::Matrix(a) => a.rows(),
            _ => unreachable!(),
        };
        let actual = matrix(&mut session, "A * inv(A)");
        for (i, row) in actual.iter().enumerate() {
            for (j, cell) in row.iter().enumerate() {
                assert_eq!(cell, if i == j { "1" } else { "0" }, "{source}");
            }
        }
        assert_eq!(actual.len(), n);
        assert_eq!(scalar(&mut session, "det(A) * det(inv(A))"), "1");
    }
}

#[test]
fn errors_leave_stored_matrices_available() {
    let mut session = Session::new(Mode::Exact);
    session.execute("A = [1 2; 3 4]").unwrap();
    for bad in [
        "A = [1 2; 3]",
        "A = [1 2] + [1 2; 3 4]",
        "A = inv([1 2; 2 4])",
        "A = solve([1 2; 2 4], [1, 3])",
        "A = 1/0",
        "A = unknown + 1",
    ] {
        assert!(session.execute(bad).is_err(), "{bad}");
        assert_eq!(scalar(&mut session, "det(A)"), "-2");
    }
}

#[test]
fn symbolic_division_assumptions_survive_variable_reuse() {
    let mut session = Session::new(Mode::Symbolic);
    session.execute("S = [x 1; 0 x]").unwrap();
    assert_eq!(scalar(&mut session, "det(S)"), "x^2");
    let inverse = session.execute("B = inv(S)").unwrap();
    assert!(!inverse.conditions.is_empty());
    let reused = session.execute("S * B").unwrap();
    assert!(!reused.conditions.is_empty());
    assert_eq!(matrix(&mut session, "S * B"), [["1", "0"], ["0", "1"]]);
    session.execute(":clear").unwrap();
    assert!(session.execute("x + 1").unwrap().conditions.is_empty());
}

#[test]
fn float_solve_has_a_small_residual() {
    let mut session = Session::new(Mode::Float);
    session.execute("A = [4 1 -1; 2 7 1; 1 -3 12]").unwrap();
    session.execute("b = [3, 19, 31]").unwrap();
    let result = session.execute("A * solve(A, b) - b").unwrap();
    let Value::Matrix(residual) = result.value.unwrap() else {
        panic!()
    };
    for row in residual.data() {
        for entry in row {
            assert!(entry.format(15).parse::<f64>().unwrap().abs() < 1e-10);
        }
    }
}

#[test]
fn three_by_three_determinants_match_an_independent_cofactor_formula() {
    let mut session = Session::new(Mode::Exact);
    let mut state = 17_u64;
    for _ in 0..64 {
        let mut a = [0_i64; 9];
        for item in &mut a {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            *item = ((state >> 32) % 19) as i64 - 9;
        }
        let expected = a[0] * (a[4] * a[8] - a[5] * a[7]) - a[1] * (a[3] * a[8] - a[5] * a[6])
            + a[2] * (a[3] * a[7] - a[4] * a[6]);
        let expr = format!(
            "det([{} {} {}; {} {} {}; {} {} {}])",
            a[0], a[1], a[2], a[3], a[4], a[5], a[6], a[7], a[8]
        );
        assert_eq!(scalar(&mut session, &expr), expected.to_string(), "{expr}");
    }
}
