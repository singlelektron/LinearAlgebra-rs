//! Arithmetic independent of parsing and terminal presentation.
//!
//! Exact mode uses arbitrary precision rational numbers. Symbolic mode contains
//! rational functions of commuting indeterminates; it does not decide parameter
//! cases. Any divisions by expressions of unknown sign/nonzeroness retain their
//! domain restrictions in the value and report them through the context.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::str::FromStr;

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use crate::latex::symbol as latex_symbol;

pub const MAX_DIMENSION: usize = 64;
pub const MAX_CELLS: usize = 4096;
const MAX_BITS: u64 = 16_384;
const MAX_TERMS: usize = 256;
const MAX_POWER: u32 = 128;
const MAX_SYMBOLIC_DETERMINANT: usize = 8;

type Rational = BigRational;
type Monomial = Vec<(String, u16)>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalcError(pub String);

impl fmt::Display for CalcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for CalcError {}
impl From<String> for CalcError {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl From<&str> for CalcError {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}
pub type CalcResult<T> = Result<T, CalcError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Exact,
    Float,
    Symbolic,
}
impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Exact => "exact",
            Self::Float => "float",
            Self::Symbolic => "symbolic",
        })
    }
}
impl FromStr for Mode {
    type Err = CalcError;
    fn from_str(s: &str) -> CalcResult<Self> {
        match s {
            "exact" => Ok(Self::Exact),
            "float" => Ok(Self::Float),
            "symbolic" => Ok(Self::Symbolic),
            _ => Err(CalcError("Mode must be exact, float, or symbolic".into())),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Context {
    pub mode: Mode,
    /// Relative threshold for pivot selection, against the original coefficient
    /// matrix's maximum absolute entry. Scalar division only rejects actual zero.
    pub tolerance: f64,
    pub steps: Vec<String>,
    pub conditions: BTreeSet<String>,
}
impl Context {
    pub fn new(mode: Mode, tolerance: f64) -> Self {
        Self {
            mode,
            tolerance,
            steps: Vec::new(),
            conditions: BTreeSet::new(),
        }
    }
    fn check(&self) -> CalcResult<()> {
        if !self.tolerance.is_finite() || self.tolerance <= 0.0 || self.tolerance >= 1.0 {
            return Err(CalcError(
                "Tolerance must be finite and strictly between 0 and 1".into(),
            ));
        }
        Ok(())
    }
    fn observe(&mut self, scalar: &Scalar) {
        self.conditions.extend(scalar.conditions());
    }
}

fn checked_rational(value: Rational) -> CalcResult<Rational> {
    if value.numer().bits() > MAX_BITS || value.denom().bits() > MAX_BITS {
        Err(CalcError(format!(
            "Exact-number growth limit exceeded ({MAX_BITS} bits)"
        )))
    } else {
        Ok(value)
    }
}

fn parse_rational(s: &str) -> CalcResult<Rational> {
    if s.len() > 4096 {
        return Err(CalcError("Number literal exceeds 4096 characters".into()));
    }
    if let Some((numerator, denominator)) = s.split_once('/') {
        if denominator.contains('/') {
            return Err(CalcError("Invalid rational literal".into()));
        }
        let numerator = parse_rational(numerator)?;
        let denominator = parse_rational(denominator)?;
        if denominator.is_zero() {
            return Err(CalcError("Division by zero".into()));
        }
        return checked_rational(numerator / denominator);
    }
    let mut parts = s.split(['e', 'E']);
    let base = parts.next().unwrap_or("");
    let exponent: i32 = parts
        .next()
        .map(|e| e.parse::<i32>())
        .transpose()
        .map_err(|_| CalcError("Invalid scientific exponent".into()))?
        .unwrap_or(0);
    if parts.next().is_some() || exponent.unsigned_abs() > 4096 {
        return Err(CalcError(
            "Scientific exponent must have magnitude at most 4096".into(),
        ));
    }
    let negative = base.starts_with('-');
    let unsigned = base.strip_prefix(['-', '+']).unwrap_or(base);
    let mut decimals = unsigned.split('.');
    let whole = decimals.next().unwrap_or("");
    let fraction = decimals.next().unwrap_or("");
    if decimals.next().is_some()
        || (whole.is_empty() && fraction.is_empty())
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|c| c.is_ascii_digit())
    {
        return Err(CalcError(format!("Invalid number literal '{s}'")));
    }
    let digits = format!("{whole}{fraction}");
    let mut integer = BigInt::parse_bytes(digits.as_bytes(), 10)
        .ok_or_else(|| CalcError(format!("Invalid number literal '{s}'")))?;
    if negative {
        integer = -integer;
    }
    let scale = i32::try_from(fraction.len()).unwrap_or(i32::MAX) - exponent;
    if scale.unsigned_abs() > 4096 {
        return Err(CalcError("Decimal scale exceeds 4096 places".into()));
    }
    let power = BigInt::from(10u8).pow(scale.unsigned_abs());
    checked_rational(if scale >= 0 {
        Rational::new(integer, power)
    } else {
        Rational::from_integer(integer * power)
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Polynomial(BTreeMap<Monomial, Rational>);
impl Polynomial {
    fn constant(value: Rational) -> Self {
        let mut terms = BTreeMap::new();
        if !value.is_zero() {
            terms.insert(Vec::new(), value);
        }
        Self(terms)
    }
    fn symbol(name: &str) -> Self {
        Self(BTreeMap::from([(
            vec![(name.to_owned(), 1)],
            Rational::one(),
        )]))
    }
    fn zero() -> Self {
        Self(BTreeMap::new())
    }
    fn one() -> Self {
        Self::constant(Rational::one())
    }
    fn is_zero(&self) -> bool {
        self.0.is_empty()
    }
    fn as_constant(&self) -> Option<Rational> {
        if self.is_zero() {
            Some(Rational::zero())
        } else if self.0.len() == 1 {
            self.0.get(&Vec::new()).cloned()
        } else {
            None
        }
    }
    fn check(&self) -> CalcResult<()> {
        if self.0.len() > MAX_TERMS {
            return Err(CalcError(format!(
                "Symbolic expansion exceeds {MAX_TERMS} terms"
            )));
        }
        for coefficient in self.0.values() {
            checked_rational(coefficient.clone())?;
        }
        Ok(())
    }
    fn neg(&self) -> Self {
        Self(self.0.iter().map(|(m, c)| (m.clone(), -c)).collect())
    }
    fn add(&self, other: &Self) -> CalcResult<Self> {
        let mut result = self.clone();
        for (m, c) in &other.0 {
            let new = result.0.get(m).cloned().unwrap_or_else(Rational::zero) + c;
            if new.is_zero() {
                result.0.remove(m);
            } else {
                result.0.insert(m.clone(), new);
            }
        }
        result.check()?;
        Ok(result)
    }
    fn mul(&self, other: &Self) -> CalcResult<Self> {
        let mut result = Self::zero();
        for (left, lc) in &self.0 {
            for (right, rc) in &other.0 {
                let mut powers: BTreeMap<String, u16> = left.iter().cloned().collect();
                for (symbol, power) in right {
                    *powers.entry(symbol.clone()).or_default() += power;
                }
                if powers.values().map(|v| u32::from(*v)).sum::<u32>() > MAX_POWER {
                    return Err(CalcError(format!(
                        "Symbolic total degree exceeds {MAX_POWER}"
                    )));
                }
                let monomial: Monomial = powers.into_iter().collect();
                let coefficient = result
                    .0
                    .get(&monomial)
                    .cloned()
                    .unwrap_or_else(Rational::zero)
                    + lc * rc;
                if coefficient.is_zero() {
                    result.0.remove(&monomial);
                } else {
                    result.0.insert(monomial, checked_rational(coefficient)?);
                }
                if result.0.len() > MAX_TERMS {
                    return Err(CalcError(format!(
                        "Symbolic expansion exceeds {MAX_TERMS} terms"
                    )));
                }
            }
        }
        Ok(result)
    }
    fn divide_coefficient(&mut self, coefficient: &Rational) -> CalcResult<()> {
        for c in self.0.values_mut() {
            *c = checked_rational(&*c / coefficient)?;
        }
        Ok(())
    }
    fn common_monomial(&self) -> BTreeMap<String, u16> {
        let mut terms = self.0.keys();
        let Some(first) = terms.next() else {
            return BTreeMap::new();
        };
        let mut common: BTreeMap<String, u16> = first.iter().cloned().collect();
        for monomial in terms {
            common.retain(|symbol, power| {
                if let Some((_, other)) = monomial.iter().find(|(s, _)| s == symbol) {
                    *power = (*power).min(*other);
                    true
                } else {
                    false
                }
            });
        }
        common
    }
    fn remove_monomial(&mut self, common: &BTreeMap<String, u16>) {
        self.0 = self
            .0
            .iter()
            .map(|(monomial, coefficient)| {
                let monomial = monomial
                    .iter()
                    .filter_map(|(s, p)| {
                        let remaining = p - common.get(s).copied().unwrap_or(0);
                        (remaining > 0).then(|| (s.clone(), remaining))
                    })
                    .collect();
                (monomial, coefficient.clone())
            })
            .collect();
    }
    fn format(&self, latex: bool) -> String {
        if self.is_zero() {
            return "0".into();
        }
        let mut ordered: Vec<_> = self.0.iter().collect();
        ordered.sort_by(|(a, _), (b, _)| {
            let da: u16 = a.iter().map(|(_, p)| p).sum();
            let db: u16 = b.iter().map(|(_, p)| p).sum();
            db.cmp(&da).then_with(|| {
                let symbols: BTreeSet<_> = a.iter().chain(b.iter()).map(|(s, _)| s).collect();
                for symbol in symbols {
                    let ap = a.iter().find(|(s, _)| s == symbol).map_or(0, |(_, p)| *p);
                    let bp = b.iter().find(|(s, _)| s == symbol).map_or(0, |(_, p)| *p);
                    let order = bp.cmp(&ap);
                    if !order.is_eq() {
                        return order;
                    }
                }
                std::cmp::Ordering::Equal
            })
        });
        let mut result = String::new();
        for (monomial, coefficient) in ordered {
            let negative = coefficient.is_negative();
            if result.is_empty() {
                if negative {
                    result.push('-');
                }
            } else {
                result.push_str(if negative { " - " } else { " + " });
            }
            let absolute = coefficient.abs();
            if monomial.is_empty() || !absolute.is_one() {
                result.push_str(&rational_format(&absolute, latex));
                if !monomial.is_empty() {
                    result.push_str(if latex { " " } else { "*" });
                }
            }
            for (index, (symbol, power)) in monomial.iter().enumerate() {
                if index > 0 {
                    result.push_str(if latex { " " } else { "*" });
                }
                if latex {
                    result.push_str(&latex_symbol(symbol));
                } else {
                    result.push_str(symbol);
                }
                if *power != 1 {
                    if latex {
                        result.push_str(&format!("^{{{power}}}"));
                    } else {
                        result.push_str(&format!("^{power}"));
                    }
                }
            }
        }
        result
    }
}

/// A limited rational function. Conditions survive cancellation: `a/a = 1`
/// retains `a != 0`, including when that value is stored in a variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RationalFunction {
    numerator: Polynomial,
    denominator: Polynomial,
    conditions: BTreeSet<String>,
}
impl RationalFunction {
    fn constant(value: Rational) -> Self {
        Self {
            numerator: Polynomial::constant(value),
            denominator: Polynomial::one(),
            conditions: BTreeSet::new(),
        }
    }
    fn symbol(name: &str) -> Self {
        Self {
            numerator: Polynomial::symbol(name),
            denominator: Polynomial::one(),
            conditions: BTreeSet::new(),
        }
    }
    fn normalize(mut self) -> CalcResult<Self> {
        if self.denominator.is_zero() {
            return Err(CalcError("Division by zero".into()));
        }
        if self.numerator.is_zero() {
            self.denominator = Polynomial::one();
            return Ok(self);
        }
        if self.numerator == self.denominator {
            self.numerator = Polynomial::one();
            self.denominator = Polynomial::one();
            return Ok(self);
        }
        let mut common = self.numerator.common_monomial();
        let denominator_common = self.denominator.common_monomial();
        common.retain(|symbol, power| {
            if let Some(other) = denominator_common.get(symbol) {
                *power = (*power).min(*other);
                true
            } else {
                false
            }
        });
        if !common.is_empty() {
            self.numerator.remove_monomial(&common);
            self.denominator.remove_monomial(&common);
        }
        let coefficient = self
            .denominator
            .0
            .values()
            .next()
            .expect("nonzero polynomial")
            .clone();
        self.numerator.divide_coefficient(&coefficient)?;
        self.denominator.divide_coefficient(&coefficient)?;
        self.numerator.check()?;
        self.denominator.check()?;
        Ok(self)
    }
    fn as_constant(&self) -> Option<Rational> {
        Some(self.numerator.as_constant()? / self.denominator.as_constant()?)
    }
    fn conditions_with(&self, other: &Self) -> BTreeSet<String> {
        self.conditions.union(&other.conditions).cloned().collect()
    }
    fn add(&self, other: &Self) -> CalcResult<Self> {
        if self.denominator == other.denominator {
            return Self {
                numerator: self.numerator.add(&other.numerator)?,
                denominator: self.denominator.clone(),
                conditions: self.conditions_with(other),
            }
            .normalize();
        }
        Self {
            numerator: self
                .numerator
                .mul(&other.denominator)?
                .add(&other.numerator.mul(&self.denominator)?)?,
            denominator: self.denominator.mul(&other.denominator)?,
            conditions: self.conditions_with(other),
        }
        .normalize()
    }
    fn mul(&self, other: &Self) -> CalcResult<Self> {
        // These exact equal-factor cancellations keep common inverse/rref cases
        // compact without claiming general polynomial factorization.
        let (ln, rd) = if self.numerator == other.denominator {
            (Polynomial::one(), Polynomial::one())
        } else {
            (self.numerator.clone(), other.denominator.clone())
        };
        let (rn, ld) = if other.numerator == self.denominator {
            (Polynomial::one(), Polynomial::one())
        } else {
            (other.numerator.clone(), self.denominator.clone())
        };
        Self {
            numerator: ln.mul(&rn)?,
            denominator: ld.mul(&rd)?,
            conditions: self.conditions_with(other),
        }
        .normalize()
    }
    fn div(&self, other: &Self) -> CalcResult<Self> {
        if other.numerator.is_zero() {
            return Err(CalcError("Division by zero".into()));
        }
        let mut reciprocal = Self {
            numerator: other.denominator.clone(),
            denominator: other.numerator.clone(),
            conditions: other.conditions.clone(),
        };
        if other.numerator.as_constant().is_none() {
            reciprocal
                .conditions
                .insert(format!("{} != 0", other.numerator.format(false)));
        }
        self.mul(&reciprocal)
    }
    fn format(&self, latex: bool) -> String {
        if let Some(value) = self.as_constant() {
            return rational_format(&value, latex);
        }
        let numerator = self.numerator.format(latex);
        if self.denominator == Polynomial::one() {
            numerator
        } else if latex {
            format!("\\frac{{{numerator}}}{{{}}}", self.denominator.format(true))
        } else {
            format!("({numerator})/({})", self.denominator.format(false))
        }
    }
}

fn rational_format(value: &Rational, latex: bool) -> String {
    if value.is_integer() {
        value.numer().to_string()
    } else if latex && value.is_negative() {
        format!("-\\frac{{{}}}{{{}}}", value.numer().abs(), value.denom())
    } else if latex {
        format!("\\frac{{{}}}{{{}}}", value.numer(), value.denom())
    } else {
        format!("{}/{}", value.numer(), value.denom())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Scalar {
    Exact(Rational),
    Float(f64),
    Symbolic(RationalFunction),
}
impl Scalar {
    pub fn parse(text: &str, mode: Mode) -> CalcResult<Self> {
        if text.len() > 4096 {
            return Err(CalcError("Number literal exceeds 4096 characters".into()));
        }
        match mode {
            Mode::Exact => Ok(Self::Exact(parse_rational(text)?)),
            Mode::Symbolic => Ok(Self::Symbolic(RationalFunction::constant(parse_rational(
                text,
            )?))),
            Mode::Float => {
                if let Some((a, b)) = text.split_once('/') {
                    if b.contains('/') {
                        return Err(CalcError("Invalid rational literal".into()));
                    }
                    let a = Self::parse(a, mode)?;
                    let b = Self::parse(b, mode)?;
                    a.div(&b, &mut Context::new(mode, 1e-12))
                } else {
                    let value = text
                        .parse::<f64>()
                        .map_err(|_| CalcError(format!("Invalid float literal '{text}'")))?;
                    Self::finite(value)
                }
            }
        }
    }
    pub fn symbol(name: &str) -> Self {
        Self::Symbolic(RationalFunction::symbol(name))
    }
    pub fn integer(value: i64, mode: Mode) -> Self {
        match mode {
            Mode::Exact => Self::Exact(Rational::from_integer(value.into())),
            Mode::Float => Self::Float(value as f64),
            Mode::Symbolic => Self::Symbolic(RationalFunction::constant(Rational::from_integer(
                value.into(),
            ))),
        }
    }
    fn finite(value: f64) -> CalcResult<Self> {
        if value.is_finite() {
            Ok(Self::Float(value))
        } else {
            Err(CalcError(
                "Floating-point result is non-finite (overflow or invalid operation)".into(),
            ))
        }
    }
    pub fn mode(&self) -> Mode {
        match self {
            Self::Exact(_) => Mode::Exact,
            Self::Float(_) => Mode::Float,
            Self::Symbolic(_) => Mode::Symbolic,
        }
    }
    pub fn conditions(&self) -> BTreeSet<String> {
        match self {
            Self::Symbolic(value) => value.conditions.clone(),
            _ => BTreeSet::new(),
        }
    }
    pub fn is_zero(&self) -> bool {
        match self {
            Self::Exact(v) => v.is_zero(),
            Self::Float(v) => *v == 0.0,
            Self::Symbolic(v) => v.numerator.is_zero(),
        }
    }
    fn is_known_nonzero(&self) -> bool {
        match self {
            Self::Symbolic(v) => v.as_constant().is_some_and(|n| !n.is_zero()),
            _ => !self.is_zero(),
        }
    }
    fn check(&self, context: &Context) -> CalcResult<()> {
        context.check()?;
        if self.mode() != context.mode {
            return Err(CalcError(
                "Arithmetic modes cannot be mixed; change modes explicitly and re-enter values"
                    .into(),
            ));
        }
        match self {
            Self::Float(value) if !value.is_finite() => {
                Err(CalcError("Non-finite floating-point input".into()))
            }
            Self::Exact(value) => checked_rational(value.clone()).map(|_| ()),
            Self::Symbolic(value) => {
                value.numerator.check()?;
                value.denominator.check()
            }
            _ => Ok(()),
        }
    }
    pub fn add(&self, other: &Self, context: &mut Context) -> CalcResult<Self> {
        self.check(context)?;
        other.check(context)?;
        context.observe(self);
        context.observe(other);
        let result = match (self, other) {
            (Self::Exact(a), Self::Exact(b)) => Self::Exact(checked_rational(a + b)?),
            (Self::Float(a), Self::Float(b)) => Self::finite(a + b)?,
            (Self::Symbolic(a), Self::Symbolic(b)) => Self::Symbolic(a.add(b)?),
            _ => unreachable!("modes checked"),
        };
        context.observe(&result);
        Ok(result)
    }
    pub fn neg(&self) -> Self {
        match self {
            Self::Exact(v) => Self::Exact(-v),
            Self::Float(v) => Self::Float(-v),
            Self::Symbolic(v) => Self::Symbolic(RationalFunction {
                numerator: v.numerator.neg(),
                denominator: v.denominator.clone(),
                conditions: v.conditions.clone(),
            }),
        }
    }
    pub fn sub(&self, other: &Self, context: &mut Context) -> CalcResult<Self> {
        self.add(&other.neg(), context)
    }
    pub fn mul(&self, other: &Self, context: &mut Context) -> CalcResult<Self> {
        self.check(context)?;
        other.check(context)?;
        context.observe(self);
        context.observe(other);
        let result = match (self, other) {
            (Self::Exact(a), Self::Exact(b)) => Self::Exact(checked_rational(a * b)?),
            (Self::Float(a), Self::Float(b)) => Self::finite(a * b)?,
            (Self::Symbolic(a), Self::Symbolic(b)) => Self::Symbolic(a.mul(b)?),
            _ => unreachable!("modes checked"),
        };
        context.observe(&result);
        Ok(result)
    }
    pub fn div(&self, other: &Self, context: &mut Context) -> CalcResult<Self> {
        self.check(context)?;
        other.check(context)?;
        context.observe(self);
        context.observe(other);
        if other.is_zero() {
            return Err(CalcError("Division by zero".into()));
        }
        let result = match (self, other) {
            (Self::Exact(a), Self::Exact(b)) => Self::Exact(checked_rational(a / b)?),
            (Self::Float(a), Self::Float(b)) => Self::finite(a / b)?,
            (Self::Symbolic(a), Self::Symbolic(b)) => Self::Symbolic(a.div(b)?),
            _ => unreachable!("modes checked"),
        };
        context.observe(&result);
        Ok(result)
    }
    pub fn pow(&self, power: i32, context: &mut Context) -> CalcResult<Self> {
        self.check(context)?;
        context.observe(self);
        if power.unsigned_abs() > MAX_POWER {
            return Err(CalcError(format!("Power magnitude exceeds {MAX_POWER}")));
        }
        let mut base = if power < 0 {
            Self::integer(1, context.mode).div(self, context)?
        } else {
            self.clone()
        };
        let mut result = Self::integer(1, context.mode);
        // Preserve domain restrictions even for a zero exponent.
        if let Self::Symbolic(r) = &mut result {
            r.conditions = self.conditions();
        }
        let mut exponent = power.unsigned_abs();
        while exponent > 0 {
            if exponent % 2 == 1 {
                result = result.mul(&base, context)?;
            }
            exponent /= 2;
            if exponent > 0 {
                base = base.mul(&base, context)?;
            }
        }
        Ok(result)
    }
    pub fn as_i64(&self) -> CalcResult<i64> {
        let value = match self {
            Self::Exact(v) if v.is_integer() => v.to_integer().to_i64(),
            Self::Symbolic(v) => v
                .as_constant()
                .filter(|v| v.is_integer())
                .and_then(|v| v.to_integer().to_i64()),
            Self::Float(v)
                if v.is_finite()
                    && v.fract() == 0.0
                    && *v >= i64::MIN as f64
                    && *v < -(i64::MIN as f64) =>
            {
                Some(*v as i64)
            }
            _ => None,
        };
        value.ok_or_else(|| CalcError("Expected an integer in the signed 64-bit range".into()))
    }
    pub fn format(&self, precision: usize) -> String {
        match self {
            Self::Exact(v) => rational_format(v, false),
            Self::Symbolic(v) => v.format(false),
            Self::Float(v) => {
                if *v == 0.0 {
                    return "0".into();
                }
                let precision = precision.clamp(1, 16);
                let scientific = format!("{:.*e}", precision - 1, v);
                let (mantissa, exponent) = scientific.split_once('e').expect("scientific format");
                let exponent: i32 = exponent.parse().expect("formatted exponent");
                if exponent < -4 || exponent >= precision as i32 {
                    let mantissa = if mantissa.contains('.') {
                        mantissa.trim_end_matches('0').trim_end_matches('.')
                    } else {
                        mantissa
                    };
                    format!("{mantissa}e{exponent}")
                } else {
                    let decimals = (precision as i32 - 1 - exponent).max(0) as usize;
                    let fixed = format!("{v:.decimals$}");
                    if fixed.contains('.') {
                        fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
                    } else {
                        fixed
                    }
                }
            }
        }
    }
    pub fn to_latex(&self, precision: usize) -> String {
        match self {
            Self::Exact(v) => rational_format(v, true),
            Self::Symbolic(v) => v.format(true),
            Self::Float(_) => {
                let formatted = self.format(precision);
                if let Some((mantissa, exponent)) = formatted.split_once('e') {
                    format!("{mantissa}\\times 10^{{{exponent}}}")
                } else {
                    formatted
                }
            }
        }
    }
}

// Keep a determinant's pivot product in binary scientific notation. Applying
// the exponent only at the end avoids intermediate overflow/underflow even
// when the final determinant is representable. Pivot selection is unchanged.
struct FloatDeterminantProduct {
    mantissa: f64,
    exponent: i32,
}
impl FloatDeterminantProduct {
    fn new() -> Self {
        Self {
            mantissa: 1.0,
            exponent: 0,
        }
    }

    fn multiply(&mut self, mut value: f64) {
        debug_assert!(value.is_finite() && value != 0.0);
        let mut adjustment = 0;
        if value.is_subnormal() {
            // Scaling a subnormal by 2^52 is exact and makes it normal, so its
            // leading bit can be recovered using the same decomposition.
            value *= (1u64 << 52) as f64;
            adjustment = -52;
        }
        let bits = value.to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as i32 - 1023 + adjustment;
        let fraction_and_sign = bits & ((1u64 << 63) | ((1u64 << 52) - 1));
        let mantissa = f64::from_bits(fraction_and_sign | (1023u64 << 52));
        self.mantissa *= mantissa;
        self.exponent += exponent;
        if self.mantissa.abs() >= 2.0 {
            self.mantissa *= 0.5;
            self.exponent += 1;
        }
    }

    fn finish(self) -> CalcResult<f64> {
        if self.exponent > 1023 {
            return Err(CalcError("Floating-point determinant overflow".into()));
        }
        let result = if self.exponent >= -1022 {
            self.mantissa * f64::from_bits(((self.exponent + 1023) as u64) << 52)
        } else if self.exponent >= -1074 {
            self.mantissa * f64::from_bits(1u64 << (self.exponent + 1074))
        } else if self.exponent == -1075 {
            // 2^-1075 itself is not representable. Halving the normalized
            // mantissa first still permits correct rounding to 2^-1074.
            (self.mantissa * 0.5) * f64::from_bits(1)
        } else {
            0.0
        };
        if result == 0.0 {
            return Err(CalcError(
                "Floating-point determinant underflow: nonzero result rounds to zero".into(),
            ));
        }
        Ok(result)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    data: Vec<Vec<Scalar>>,
}
impl Matrix {
    pub fn new(data: Vec<Vec<Scalar>>) -> CalcResult<Self> {
        let rows = data.len();
        let cols = data.first().map_or(0, Vec::len);
        check_shape(rows, cols)?;
        if data.iter().any(|row| row.len() != cols) {
            return Err(CalcError(
                "Matrix rows must all have the same length".into(),
            ));
        }
        let mode = data[0][0].mode();
        if data.iter().flatten().any(|value| value.mode() != mode) {
            return Err(CalcError("A matrix cannot mix arithmetic modes".into()));
        }
        let context = Context::new(mode, 1e-12);
        for scalar in data.iter().flatten() {
            scalar.check(&context)?;
        }
        Ok(Self { data })
    }
    pub fn rows(&self) -> usize {
        self.data.len()
    }
    pub fn cols(&self) -> usize {
        self.data[0].len()
    }
    pub fn get(&self, row: usize, col: usize) -> &Scalar {
        &self.data[row][col]
    }
    pub fn data(&self) -> &[Vec<Scalar>] {
        &self.data
    }
    pub fn mode(&self) -> Mode {
        self.data[0][0].mode()
    }
    fn check(&self, context: &mut Context) -> CalcResult<()> {
        for scalar in self.data.iter().flatten() {
            scalar.check(context)?;
            context.observe(scalar);
        }
        Ok(())
    }
    pub fn zeros(rows: usize, cols: usize, mode: Mode) -> CalcResult<Self> {
        check_shape(rows, cols)?;
        Self::new(vec![vec![Scalar::integer(0, mode); cols]; rows])
    }
    pub fn identity(n: usize, mode: Mode) -> CalcResult<Self> {
        let mut matrix = Self::zeros(n, n, mode)?;
        for i in 0..n {
            matrix.data[i][i] = Scalar::integer(1, mode);
        }
        Ok(matrix)
    }
    fn same_shape(&self, other: &Self) -> CalcResult<()> {
        if self.rows() != other.rows() || self.cols() != other.cols() {
            Err(CalcError(format!(
                "Matrix dimensions must match: {}×{} versus {}×{}",
                self.rows(),
                self.cols(),
                other.rows(),
                other.cols()
            )))
        } else {
            Ok(())
        }
    }
    fn square(&self) -> CalcResult<()> {
        if self.rows() != self.cols() {
            Err(CalcError(format!(
                "Operation requires a square matrix; got {}×{}",
                self.rows(),
                self.cols()
            )))
        } else {
            Ok(())
        }
    }
    pub fn add(&self, other: &Self, context: &mut Context) -> CalcResult<Self> {
        self.same_shape(other)?;
        let data = self
            .data
            .iter()
            .zip(&other.data)
            .map(|(a, b)| a.iter().zip(b).map(|(x, y)| x.add(y, context)).collect())
            .collect::<CalcResult<Vec<_>>>()?;
        Self::new(data)
    }
    pub fn sub(&self, other: &Self, context: &mut Context) -> CalcResult<Self> {
        self.same_shape(other)?;
        let data = self
            .data
            .iter()
            .zip(&other.data)
            .map(|(a, b)| a.iter().zip(b).map(|(x, y)| x.sub(y, context)).collect())
            .collect::<CalcResult<Vec<_>>>()?;
        Self::new(data)
    }
    pub fn mul(&self, other: &Self, context: &mut Context) -> CalcResult<Self> {
        self.check(context)?;
        other.check(context)?;
        if self.cols() != other.rows() {
            return Err(CalcError(format!(
                "Cannot multiply {}×{} by {}×{}: inner dimensions differ",
                self.rows(),
                self.cols(),
                other.rows(),
                other.cols()
            )));
        }
        let mut output = Self::zeros(self.rows(), other.cols(), context.mode)?;
        for row in 0..self.rows() {
            for col in 0..other.cols() {
                let mut sum = Scalar::integer(0, context.mode);
                for k in 0..self.cols() {
                    sum = sum.add(
                        &self.data[row][k].mul(&other.data[k][col], context)?,
                        context,
                    )?;
                }
                output.data[row][col] = sum;
            }
        }
        Ok(output)
    }
    pub fn scale(&self, scalar: &Scalar, context: &mut Context) -> CalcResult<Self> {
        Self::new(
            self.data
                .iter()
                .map(|row| row.iter().map(|value| value.mul(scalar, context)).collect())
                .collect::<CalcResult<_>>()?,
        )
    }
    pub fn div(&self, scalar: &Scalar, context: &mut Context) -> CalcResult<Self> {
        Self::new(
            self.data
                .iter()
                .map(|row| row.iter().map(|value| value.div(scalar, context)).collect())
                .collect::<CalcResult<_>>()?,
        )
    }
    pub fn transpose(&self) -> Self {
        Self {
            data: (0..self.cols())
                .map(|col| {
                    (0..self.rows())
                        .map(|row| self.data[row][col].clone())
                        .collect()
                })
                .collect(),
        }
    }
    pub fn pow(&self, power: i32, context: &mut Context) -> CalcResult<Self> {
        self.square()?;
        self.check(context)?;
        if power.unsigned_abs() > MAX_POWER {
            return Err(CalcError(format!("Power magnitude exceeds {MAX_POWER}")));
        }
        let mut base = if power < 0 {
            self.inverse(context)?
        } else {
            self.clone()
        };
        let mut output = Self::identity(self.rows(), context.mode)?;
        let inherited: BTreeSet<_> = self
            .data
            .iter()
            .flatten()
            .flat_map(Scalar::conditions)
            .collect();
        for value in output.data.iter_mut().flatten() {
            if let Scalar::Symbolic(value) = value {
                value.conditions.extend(inherited.clone());
            }
        }
        let mut exponent = power.unsigned_abs();
        while exponent > 0 {
            if exponent % 2 == 1 {
                output = output.mul(&base, context)?;
            }
            exponent /= 2;
            if exponent > 0 {
                base = base.mul(&base, context)?;
            }
        }
        Ok(output)
    }
    pub fn trace(&self, context: &mut Context) -> CalcResult<Scalar> {
        self.square()?;
        self.check(context)?;
        let mut result = Scalar::integer(0, context.mode);
        for i in 0..self.rows() {
            result = result.add(&self.data[i][i], context)?;
        }
        Ok(result)
    }
    pub fn augment(&self, other: &Self, context: &mut Context) -> CalcResult<Self> {
        self.check(context)?;
        other.check(context)?;
        if self.rows() != other.rows() {
            return Err(CalcError(
                "Augmented matrices must have the same number of rows".into(),
            ));
        }
        check_shape(self.rows(), self.cols() + other.cols())?;
        Self::new(
            self.data
                .iter()
                .zip(&other.data)
                .map(|(a, b)| a.iter().chain(b).cloned().collect())
                .collect(),
        )
    }
    pub fn det(&self, context: &mut Context) -> CalcResult<Scalar> {
        self.square()?;
        self.check(context)?;
        if context.mode == Mode::Symbolic {
            return self.symbolic_det(context);
        }
        let n = self.rows();
        let mut data = self.data.clone();
        let threshold = pivot_threshold(&data, n, context);
        let mut result = Scalar::integer(1, context.mode);
        let mut float_product = FloatDeterminantProduct::new();
        for col in 0..n {
            let Some(pivot) = choose_pivot(&data, col, col, threshold, context.mode) else {
                context.steps.push(format!(
                    "Column {} has no nonzero pivot; determinant = 0",
                    col + 1
                ));
                return Ok(Scalar::integer(0, context.mode));
            };
            if pivot != col {
                data.swap(pivot, col);
                result = result.neg();
                context.steps.push(format!(
                    "R{} ↔ R{} (determinant changes sign)",
                    col + 1,
                    pivot + 1
                ));
            }
            let pivot_value = data[col][col].clone();
            if let Scalar::Float(value) = &pivot_value {
                float_product.multiply(*value);
            } else {
                result = result.mul(&pivot_value, context)?;
            }
            for row in col + 1..n {
                if data[row][col].is_zero() {
                    continue;
                }
                let factor = data[row][col].div(&pivot_value, context)?;
                let pivot_row = data[col].clone();
                for (index, value) in pivot_row.iter().enumerate().skip(col) {
                    data[row][index] =
                        data[row][index].sub(&factor.mul(value, context)?, context)?;
                }
                data[row][col] = Scalar::integer(0, context.mode);
                context.steps.push(format!(
                    "R{} ← R{} − ({}) R{}",
                    row + 1,
                    row + 1,
                    factor.format(12),
                    col + 1
                ));
            }
        }
        context
            .steps
            .push("Determinant = row-swap sign × product of diagonal pivots".into());
        if let Scalar::Float(sign) = result {
            Ok(Scalar::Float(sign * float_product.finish()?))
        } else {
            Ok(result)
        }
    }
    fn symbolic_det(&self, context: &mut Context) -> CalcResult<Scalar> {
        let n = self.rows();
        if n > MAX_SYMBOLIC_DETERMINANT {
            return Err(CalcError(format!(
                "Division-free symbolic determinant is limited to {MAX_SYMBOLIC_DETERMINANT}×{MAX_SYMBOLIC_DETERMINANT} matrices"
            )));
        }
        let mut determinants = vec![Scalar::integer(0, Mode::Symbolic); 1 << n];
        determinants[0] = Scalar::integer(1, Mode::Symbolic);
        for mask in 1usize..1 << n {
            let row = mask.count_ones() as usize - 1;
            let count = row + 1;
            let mut position = 0;
            let mut result = Scalar::integer(0, Mode::Symbolic);
            for col in 0..n {
                if mask & (1 << col) == 0 {
                    continue;
                }
                let mut term =
                    self.data[row][col].mul(&determinants[mask ^ (1 << col)], context)?;
                if (count - 1 + position) % 2 == 1 {
                    term = term.neg();
                }
                result = result.add(&term, context)?;
                position += 1;
            }
            determinants[mask] = result;
        }
        context
            .steps
            .push("Division-free determinant expansion (no pivot assumptions)".into());
        Ok(determinants.pop().expect("nonempty determinant table"))
    }
    pub fn rref(&self, context: &mut Context) -> CalcResult<Self> {
        self.check(context)?;
        let (data, _) = eliminate(self.data.clone(), self.cols(), context)?;
        Self::new(data)
    }
    pub fn rank(&self, context: &mut Context) -> CalcResult<usize> {
        self.check(context)?;
        Ok(eliminate(self.data.clone(), self.cols(), context)?.1.len())
    }
    pub fn inverse(&self, context: &mut Context) -> CalcResult<Self> {
        self.square()?;
        let identity = Self::identity(self.rows(), context.mode)?;
        self.solve(&identity, context).map_err(|error| {
            if error.0.starts_with("No unique solution") || error.0.starts_with("Inconsistent system") {
                CalcError("Matrix is singular (or numerically singular at the selected tolerance); inverse does not exist on this branch".into())
            } else { error }
        })
    }
    pub fn solve(&self, rhs: &Self, context: &mut Context) -> CalcResult<Self> {
        self.check(context)?;
        rhs.check(context)?;
        if self.rows() != rhs.rows() {
            return Err(CalcError(format!(
                "A and B in solve(A, B) must have the same row count; got {} and {}",
                self.rows(),
                rhs.rows()
            )));
        }
        let combined: Vec<Vec<Scalar>> = self
            .data
            .iter()
            .zip(&rhs.data)
            .map(|(a, b)| a.iter().chain(b).cloned().collect())
            .collect();
        let (data, pivots) = eliminate(combined, self.cols(), context)?;
        // Each right-hand side defines an independent system. Its numerical
        // consistency must not depend on the scale of another RHS column.
        let rhs_thresholds: Vec<f64> = (0..rhs.cols())
            .map(|col| {
                rhs.data
                    .iter()
                    .filter_map(|row| match row[col] {
                        Scalar::Float(value) => Some(value.abs()),
                        _ => None,
                    })
                    .fold(0.0f64, f64::max)
                    * context.tolerance
            })
            .collect();
        for row in data.iter().skip(pivots.len()) {
            for (value, threshold) in row.iter().skip(self.cols()).zip(&rhs_thresholds) {
                let nonzero = match value {
                    Scalar::Float(v) => v.abs() > *threshold,
                    _ => !value.is_zero(),
                };
                if nonzero {
                    if matches!(value, Scalar::Symbolic(v) if v.as_constant().is_none()) {
                        return Err(CalcError(format!(
                            "System consistency requires {} = 0; parameter case splitting is not supported",
                            value.format(12)
                        )));
                    }
                    return Err(CalcError(
                        "Inconsistent system: no solution on the evaluated branch".into(),
                    ));
                }
            }
        }
        if pivots.len() < self.cols() {
            return Err(CalcError(format!(
                "No unique solution: rank {} < {} unknowns; parametric solutions are not yet supported",
                pivots.len(),
                self.cols()
            )));
        }
        let mut result = Self::zeros(self.cols(), rhs.cols(), context.mode)?;
        for (row, &pivot) in pivots.iter().enumerate() {
            result.data[pivot] = data[row][self.cols()..].to_vec();
        }
        let all_conditions = context.conditions.clone();
        for value in result.data.iter_mut().flatten() {
            if let Scalar::Symbolic(value) = value {
                value.conditions.extend(all_conditions.clone());
            }
        }
        Ok(result)
    }
}

fn check_shape(rows: usize, cols: usize) -> CalcResult<()> {
    if rows == 0 || cols == 0 {
        return Err(CalcError(
            "Matrices must have at least one row and one column".into(),
        ));
    }
    if rows > MAX_DIMENSION || cols > MAX_DIMENSION || rows.saturating_mul(cols) > MAX_CELLS {
        return Err(CalcError(format!(
            "Matrix exceeds the {MAX_DIMENSION}×{MAX_DIMENSION} / {MAX_CELLS}-cell limit"
        )));
    }
    Ok(())
}

fn pivot_threshold(data: &[Vec<Scalar>], columns: usize, context: &Context) -> f64 {
    if context.mode != Mode::Float {
        return 0.0;
    }
    let maximum = data
        .iter()
        .flat_map(|row| row.iter().take(columns))
        .filter_map(|value| {
            if let Scalar::Float(v) = value {
                Some(v.abs())
            } else {
                None
            }
        })
        .fold(0.0f64, f64::max);
    maximum * context.tolerance
}

fn choose_pivot(
    data: &[Vec<Scalar>],
    start: usize,
    col: usize,
    threshold: f64,
    mode: Mode,
) -> Option<usize> {
    if mode == Mode::Float {
        (start..data.len())
            .filter_map(|row| {
                if let Scalar::Float(v) = data[row][col] {
                    (v.abs() > threshold).then_some((row, v.abs()))
                } else {
                    None
                }
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(row, _)| row)
    } else {
        (start..data.len())
            .find(|&row| data[row][col].is_known_nonzero())
            .or_else(|| (start..data.len()).find(|&row| !data[row][col].is_zero()))
    }
}

type Elimination = (Vec<Vec<Scalar>>, Vec<usize>);
fn eliminate(
    mut data: Vec<Vec<Scalar>>,
    coefficient_columns: usize,
    context: &mut Context,
) -> CalcResult<Elimination> {
    let rows = data.len();
    let threshold = pivot_threshold(&data, coefficient_columns, context);
    if context.mode == Mode::Float {
        context.steps.push(format!(
            "Partial pivoting; numerical-zero threshold = tolerance × max|Aᵢⱼ| = {threshold:.6e}"
        ));
    }
    let mut pivots = Vec::new();
    for col in 0..coefficient_columns {
        let next_row = pivots.len();
        if next_row == rows {
            break;
        }
        let Some(pivot) = choose_pivot(&data, next_row, col, threshold, context.mode) else {
            if context.mode == Mode::Float {
                let changed = data.iter().skip(next_row).any(|row| !row[col].is_zero());
                for row in data.iter_mut().skip(next_row) {
                    row[col] = Scalar::integer(0, context.mode);
                }
                if changed {
                    context.steps.push(format!(
                        "Column {} below R{} is treated as zero at the selected tolerance",
                        col + 1,
                        next_row + 1
                    ));
                }
            }
            continue;
        };
        if pivot != next_row {
            data.swap(pivot, next_row);
            context
                .steps
                .push(format!("R{} ↔ R{}", next_row + 1, pivot + 1));
        }
        let divisor = data[next_row][col].clone();
        for value in &mut data[next_row] {
            *value = value.div(&divisor, context)?;
        }
        context.steps.push(format!(
            "R{} ← R{} / ({})",
            next_row + 1,
            next_row + 1,
            divisor.format(12)
        ));
        data[next_row][col] = Scalar::integer(1, context.mode);
        let pivot_values = data[next_row].clone();
        for (row_index, row) in data.iter_mut().enumerate() {
            if row_index == next_row || row[col].is_zero() {
                continue;
            }
            let factor = row[col].clone();
            for (value, pivot_value) in row.iter_mut().zip(&pivot_values) {
                *value = value.sub(&factor.mul(pivot_value, context)?, context)?;
            }
            row[col] = Scalar::integer(0, context.mode);
            context.steps.push(format!(
                "R{} ← R{} − ({}) R{}",
                row_index + 1,
                row_index + 1,
                factor.format(12),
                next_row + 1
            ));
        }
        pivots.push(col);
    }
    // Row operations can remove an assumption-bearing pivot from the resulting
    // coefficients. Attach the branch assumptions to every returned entry.
    for scalar in data.iter_mut().flatten() {
        if let Scalar::Symbolic(value) = scalar {
            value.conditions.extend(context.conditions.clone());
        }
    }
    Ok((data, pivots))
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Scalar(Scalar),
    Matrix(Matrix),
}
impl Value {
    pub fn conditions(&self) -> BTreeSet<String> {
        match self {
            Self::Scalar(value) => value.conditions(),
            Self::Matrix(matrix) => matrix
                .data
                .iter()
                .flatten()
                .flat_map(Scalar::conditions)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matrix(rows: &[&[&str]], mode: Mode) -> Matrix {
        Matrix::new(
            rows.iter()
                .map(|r| r.iter().map(|s| Scalar::parse(s, mode).unwrap()).collect())
                .collect(),
        )
        .unwrap()
    }
    fn exact(rows: &[&[&str]]) -> Matrix {
        matrix(rows, Mode::Exact)
    }
    fn ctx(mode: Mode) -> Context {
        Context::new(mode, 1e-12)
    }

    #[test]
    fn exact_literals_and_arithmetic_never_round() {
        let mut c = ctx(Mode::Exact);
        let a = Scalar::parse("0.1", Mode::Exact).unwrap();
        let b = Scalar::parse("2e-1", Mode::Exact).unwrap();
        assert_eq!(a.add(&b, &mut c).unwrap().format(12), "3/10");
        assert_eq!(
            Scalar::parse("-1.25e2", Mode::Exact).unwrap().format(12),
            "-125"
        );
        assert_eq!(
            Scalar::parse(".125", Mode::Exact).unwrap().format(12),
            "1/8"
        );
        assert!(Scalar::parse("1e9999999", Mode::Exact).is_err());
        assert!(Scalar::parse("1/0", Mode::Exact).is_err());
        assert!(a.div(&Scalar::integer(0, Mode::Exact), &mut c).is_err());
        assert!(a.add(&Scalar::integer(1, Mode::Float), &mut c).is_err());
    }

    #[test]
    fn matrix_dimensions_and_rectangular_products() {
        let mut c = ctx(Mode::Exact);
        let a = exact(&[&["1", "2", "3"], &["4", "5", "6"]]);
        let b = exact(&[&["7", "8"], &["9", "10"], &["11", "12"]]);
        assert_eq!(
            a.mul(&b, &mut c).unwrap(),
            exact(&[&["58", "64"], &["139", "154"]])
        );
        assert_eq!(
            a.add(&a, &mut c).unwrap(),
            exact(&[&["2", "4", "6"], &["8", "10", "12"]])
        );
        assert!(a.add(&b, &mut c).is_err());
        assert!(a.mul(&a, &mut c).is_err());
        assert!(Matrix::new(vec![]).is_err());
        assert!(Matrix::new(vec![vec![Scalar::integer(1, Mode::Exact)], vec![]]).is_err());
        assert!(Matrix::zeros(65, 1, Mode::Exact).is_err());
        assert!(a.det(&mut c).is_err());
    }

    #[test]
    fn exact_elimination_inverse_and_solve_contracts() {
        let mut c = ctx(Mode::Exact);
        let a = exact(&[&["0", "2"], &["3", "4"]]);
        assert_eq!(a.det(&mut c).unwrap().format(12), "-6");
        let inverse = a.inverse(&mut c).unwrap();
        assert_eq!(inverse, exact(&[&["-2/3", "1/3"], &["1/2", "0"]]));
        assert_eq!(
            a.mul(&inverse, &mut c).unwrap(),
            Matrix::identity(2, Mode::Exact).unwrap()
        );
        let rhs = exact(&[&["2"], &["7"]]);
        let solution = a.solve(&rhs, &mut c).unwrap();
        assert_eq!(solution, exact(&[&["1"], &["1"]]));
        assert_eq!(a.mul(&solution, &mut c).unwrap(), rhs);
        assert!(c.steps.iter().any(|s| s.contains('↔')));
        assert_eq!(
            a.pow(0, &mut c).unwrap(),
            Matrix::identity(2, Mode::Exact).unwrap()
        );
        assert_eq!(a.pow(-1, &mut c).unwrap(), inverse);
        let singular = exact(&[&["1", "2"], &["2", "4"]]);
        assert_eq!(singular.rank(&mut c).unwrap(), 1);
        assert_eq!(
            singular.rref(&mut c).unwrap(),
            exact(&[&["1", "2"], &["0", "0"]])
        );
        assert!(singular.inverse(&mut c).is_err());
        assert!(
            singular
                .solve(&exact(&[&["1"], &["3"]]), &mut c)
                .unwrap_err()
                .0
                .contains("Inconsistent")
        );
        assert!(
            singular
                .solve(&exact(&[&["1"], &["2"]]), &mut c)
                .unwrap_err()
                .0
                .contains("No unique")
        );
    }

    #[test]
    fn overdetermined_system_accepts_only_consistent_unique_solutions() {
        let mut c = ctx(Mode::Exact);
        let a = exact(&[&["1", "0"], &["0", "1"], &["1", "1"]]);
        let b = exact(&[&["2"], &["3"], &["5"]]);
        assert_eq!(a.solve(&b, &mut c).unwrap(), exact(&[&["2"], &["3"]]));
        assert!(a.solve(&exact(&[&["2"], &["3"], &["6"]]), &mut c).is_err());
    }

    #[test]
    fn float_solve_checks_each_rhs_column_at_its_own_scale() {
        let a = matrix(&[&["1"], &["1"]], Mode::Float);
        for b in [
            matrix(&[&["1", "1e20"], &["2", "1e20"]], Mode::Float),
            matrix(&[&["1e20", "1"], &["1e20", "2"]], Mode::Float),
            matrix(&[&["1e-20", "1e20"], &["2e-20", "1e20"]], Mode::Float),
            matrix(&[&["0", "1e-20"], &["0", "2e-20"]], Mode::Float),
        ] {
            let error = a.solve(&b, &mut ctx(Mode::Float)).unwrap_err();
            assert!(error.0.contains("Inconsistent"));
        }
    }

    #[test]
    fn float_solve_accepts_consistent_rhs_columns_with_different_scales() {
        let a = matrix(&[&["1"], &["1"]], Mode::Float);
        let b = matrix(
            &[&["1e-20", "1e20", "0"], &["1e-20", "1e20", "0"]],
            Mode::Float,
        );
        assert_eq!(
            a.solve(&b, &mut ctx(Mode::Float)).unwrap(),
            matrix(&[&["1e-20", "1e20", "0"]], Mode::Float)
        );

        let nearly_consistent = matrix(
            &[&["1", "1e20"], &["1.0000000000005", "1.0000000000005e20"]],
            Mode::Float,
        );
        assert!(a.solve(&nearly_consistent, &mut ctx(Mode::Float)).is_ok());
    }

    #[test]
    fn float_solve_preserves_single_rhs_tolerance_and_zero_rhs() {
        let a = matrix(&[&["1"], &["1"]], Mode::Float);
        let nearby = matrix(&[&["1"], &["1.0000000000005"]], Mode::Float);
        assert!(a.solve(&nearby, &mut ctx(Mode::Float)).is_ok());
        let inconsistent = matrix(&[&["1"], &["1.000000000005"]], Mode::Float);
        assert!(
            a.solve(&inconsistent, &mut ctx(Mode::Float))
                .unwrap_err()
                .0
                .contains("Inconsistent")
        );
        let zero = matrix(&[&["0"], &["0"]], Mode::Float);
        assert_eq!(
            a.solve(&zero, &mut ctx(Mode::Float)).unwrap(),
            matrix(&[&["0"]], Mode::Float)
        );
    }

    #[test]
    fn float_partial_pivoting_relative_tolerance_and_finiteness() {
        let mut c = ctx(Mode::Float);
        let tiny = matrix(&[&["1e-100", "0"], &["0", "2e-100"]], Mode::Float);
        assert_eq!(tiny.rank(&mut c).unwrap(), 2);
        let a = matrix(&[&["1e-20", "1"], &["1", "1"]], Mode::Float);
        let x = a
            .solve(&matrix(&[&["1"], &["2"]], Mode::Float), &mut c)
            .unwrap();
        for row in x.data() {
            let Scalar::Float(v) = row[0] else {
                panic!("float expected")
            };
            assert!((v - 1.0).abs() < 1e-12);
        }
        let near_singular = matrix(&[&["1", "1"], &["1", "1.00000000000001"]], Mode::Float);
        assert_eq!(near_singular.rank(&mut c).unwrap(), 1);
        let mut strict = Context::new(Mode::Float, 1e-16);
        assert_eq!(near_singular.rank(&mut strict).unwrap(), 2);
        assert!(Scalar::parse("inf", Mode::Float).is_err());
        assert!(Scalar::parse("1e999", Mode::Float).is_err());
        assert!(
            Scalar::parse("1e308", Mode::Float)
                .unwrap()
                .pow(2, &mut c)
                .is_err()
        );
        assert_eq!(
            Scalar::parse("1e-100", Mode::Float).unwrap().format(6),
            "1e-100"
        );
    }

    #[test]
    fn float_determinants_preserve_representable_products_at_extreme_scales() {
        for (small, small_count, large, large_count, expected) in
            [(1e-10, 33, 10.0, 31, 1e-299), (0.1, 32, 1e10, 32, 1e288)]
        {
            let mut diagonal = vec![small; small_count];
            diagonal.extend(vec![large; large_count]);
            for _ in 0..2 {
                let mut a = Matrix::zeros(diagonal.len(), diagonal.len(), Mode::Float).unwrap();
                for (index, &value) in diagonal.iter().enumerate() {
                    a.data[index][index] = Scalar::Float(value);
                }
                let Scalar::Float(actual) = a.det(&mut ctx(Mode::Float)).unwrap() else {
                    panic!("float expected");
                };
                assert!((actual / expected - 1.0).abs() < 1e-13, "{actual:e}");
                diagonal.reverse();
            }
        }
    }

    #[test]
    fn float_determinants_keep_signs_subnormals_and_report_range_errors() {
        for (entries, expected) in [
            ([0.0, 2.0, 3.0, 4.0], -6.0),
            ([0.0, -2.0, 3.0, 4.0], 6.0),
            ([-2.0, 0.0, 0.0, 3.0], -6.0),
            (
                [2f64.powi(-537), 0.0, 0.0, 2f64.powi(-537)],
                f64::from_bits(1),
            ),
            (
                [2f64.powi(-537), 0.0, 0.0, 2f64.powi(-538) * 1.5],
                f64::from_bits(1),
            ),
        ] {
            let a = Matrix::new(vec![
                vec![Scalar::Float(entries[0]), Scalar::Float(entries[1])],
                vec![Scalar::Float(entries[2]), Scalar::Float(entries[3])],
            ])
            .unwrap();
            assert_eq!(
                a.det(&mut ctx(Mode::Float)).unwrap(),
                Scalar::Float(expected)
            );
        }
        for value in [
            f64::from_bits(1),
            -f64::from_bits(1),
            f64::MIN_POSITIVE,
            f64::MAX,
        ] {
            let a = Matrix::new(vec![vec![Scalar::Float(value)]]).unwrap();
            assert_eq!(a.det(&mut ctx(Mode::Float)).unwrap(), Scalar::Float(value));
        }
        for (value, message) in [(1e200, "overflow"), (1e-200, "underflow")] {
            let a = Matrix::new(vec![
                vec![Scalar::Float(value), Scalar::Float(0.0)],
                vec![Scalar::Float(0.0), Scalar::Float(value)],
            ])
            .unwrap();
            assert!(
                a.det(&mut ctx(Mode::Float))
                    .unwrap_err()
                    .0
                    .contains(message)
            );
        }
    }

    #[test]
    fn symbolic_cancellation_keeps_domains_and_commutes() {
        let mut c = ctx(Mode::Symbolic);
        let a = Scalar::symbol("a");
        let b = Scalar::symbol("b");
        let result = a.div(&a, &mut c).unwrap();
        assert_eq!(result.format(12), "1");
        assert!(result.conditions().contains("a != 0"));
        assert!(
            result
                .pow(0, &mut c)
                .unwrap()
                .conditions()
                .contains("a != 0")
        );
        assert!(
            a.mul(&b, &mut c)
                .unwrap()
                .sub(&b.mul(&a, &mut c).unwrap(), &mut c)
                .unwrap()
                .is_zero()
        );
        let sum = a.add(&b, &mut c).unwrap();
        assert_eq!(sum.pow(2, &mut c).unwrap().format(12), "a^2 + 2*a*b + b^2");
        assert_eq!(
            a.div(&Scalar::integer(2, Mode::Symbolic), &mut c)
                .unwrap()
                .format(12),
            "1/2*a"
        );
    }

    #[test]
    fn symbolic_determinants_are_unconditional_but_pivots_are_not() {
        let mut c = ctx(Mode::Symbolic);
        let a = Scalar::symbol("a");
        let zero = Scalar::integer(0, Mode::Symbolic);
        let one = Scalar::integer(1, Mode::Symbolic);
        let diagonal = Matrix::new(vec![
            vec![a.clone(), zero.clone()],
            vec![zero.clone(), one.clone()],
        ])
        .unwrap();
        assert_eq!(diagonal.det(&mut c).unwrap().format(12), "a");
        assert!(c.conditions.is_empty());
        assert_eq!(diagonal.rank(&mut c).unwrap(), 2);
        assert!(c.conditions.contains("a != 0"));
        let mut fresh = ctx(Mode::Symbolic);
        let inverse = diagonal.inverse(&mut fresh).unwrap();
        assert_eq!(inverse.get(0, 0).format(12), "(1)/(a)");
        assert!(inverse.get(1, 1).conditions().contains("a != 0"));
        let swap = Matrix::new(vec![vec![a, one.clone()], vec![one, zero]]).unwrap();
        assert_eq!(swap.det(&mut ctx(Mode::Symbolic)).unwrap().format(12), "-1");
        let mut no_assumption = ctx(Mode::Symbolic);
        swap.rref(&mut no_assumption).unwrap();
        assert!(no_assumption.conditions.is_empty());
    }

    #[test]
    fn symbolic_generic_two_by_two_inverse_is_verified_by_multiplication() {
        let mut c = ctx(Mode::Symbolic);
        let a = Matrix::new(vec![
            vec![Scalar::symbol("a"), Scalar::symbol("b")],
            vec![Scalar::symbol("c"), Scalar::symbol("d")],
        ])
        .unwrap();
        let inverse = a.inverse(&mut c).unwrap();
        let product = a.mul(&inverse, &mut c).unwrap();
        for i in 0..2 {
            for j in 0..2 {
                assert_eq!(product.get(i, j).format(12), if i == j { "1" } else { "0" });
            }
        }
        assert!(!c.conditions.is_empty());
    }

    #[test]
    fn core_resource_limits_are_errors() {
        let mut c = ctx(Mode::Exact);
        assert!(
            Scalar::integer(2, Mode::Exact)
                .pow(i32::MIN, &mut c)
                .is_err()
        );
        assert!(Matrix::zeros(usize::MAX, 2, Mode::Exact).is_err());
        assert!(
            Scalar::integer(1, Mode::Exact)
                .add(
                    &Scalar::integer(1, Mode::Exact),
                    &mut Context::new(Mode::Exact, f64::NAN)
                )
                .is_err()
        );
        let big = Scalar::parse(&"9".repeat(4096), Mode::Exact).unwrap();
        assert!(big.pow(2, &mut c).is_err());
    }
}
