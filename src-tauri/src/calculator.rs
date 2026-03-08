use anyhow::{bail, Result};

pub fn evaluate(expression: &str) -> Result<f64> {
    let mut parser = Parser::new(expression);
    let value = parser.parse_expression()?;
    parser.skip_whitespace();
    if !parser.is_eof() {
        bail!("Expresion invalida");
    }
    Ok(value)
}

struct Parser {
    source: Vec<char>,
    index: usize,
}

impl Parser {
    fn new(source: &str) -> Self {
        Self {
            source: source.chars().collect(),
            index: 0,
        }
    }

    fn is_eof(&self) -> bool {
        self.index >= self.source.len()
    }

    fn current(&self) -> Option<char> {
        self.source.get(self.index).copied()
    }

    fn consume(&mut self, token: char) -> bool {
        if self.current() == Some(token) {
            self.index += 1;
            return true;
        }
        false
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.current(), Some(c) if c.is_whitespace()) {
            self.index += 1;
        }
    }

    fn parse_expression(&mut self) -> Result<f64> {
        let mut value = self.parse_term()?;
        loop {
            self.skip_whitespace();
            if self.consume('+') {
                value += self.parse_term()?;
            } else if self.consume('-') {
                value -= self.parse_term()?;
            } else {
                break;
            }
        }
        Ok(value)
    }

    fn parse_term(&mut self) -> Result<f64> {
        let mut value = self.parse_power()?;
        loop {
            self.skip_whitespace();
            if self.consume('*') {
                value *= self.parse_power()?;
            } else if self.consume('/') {
                let divisor = self.parse_power()?;
                if divisor.abs() < f64::EPSILON {
                    bail!("Division por cero");
                }
                value /= divisor;
            } else if self.consume('%') {
                let divisor = self.parse_power()?;
                if divisor.abs() < f64::EPSILON {
                    bail!("Division por cero");
                }
                value %= divisor;
            } else if self.should_apply_implicit_multiplication() {
                value *= self.parse_power()?;
            } else {
                break;
            }
        }
        Ok(value)
    }

    fn parse_power(&mut self) -> Result<f64> {
        let base = self.parse_unary()?;
        self.skip_whitespace();
        if self.consume('^') {
            let exponent = self.parse_power()?;
            return Ok(base.powf(exponent));
        }
        Ok(base)
    }

    fn parse_unary(&mut self) -> Result<f64> {
        self.skip_whitespace();
        if self.consume('+') {
            return self.parse_unary();
        }
        if self.consume('-') {
            return Ok(-self.parse_unary()?);
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<f64> {
        self.skip_whitespace();
        if self.consume('(') {
            let value = self.parse_expression()?;
            self.skip_whitespace();
            if !self.consume(')') {
                bail!("Falta parentesis de cierre");
            }
            return Ok(value);
        }

        if matches!(self.current(), Some(character) if character.is_ascii_alphabetic() || character == '_')
        {
            return self.parse_identifier_or_function();
        }

        self.parse_number()
    }

    fn parse_identifier_or_function(&mut self) -> Result<f64> {
        let name = self.parse_identifier().to_lowercase();
        self.skip_whitespace();

        if self.consume('(') {
            let args = self.parse_arguments()?;
            return self.evaluate_function(&name, &args);
        }

        self.resolve_constant(&name)
    }

    fn parse_identifier(&mut self) -> String {
        let start = self.index;
        while let Some(character) = self.current() {
            if character.is_ascii_alphabetic() || character == '_' {
                self.index += 1;
            } else {
                break;
            }
        }

        self.source[start..self.index].iter().collect()
    }

    fn parse_arguments(&mut self) -> Result<Vec<f64>> {
        let mut args = Vec::new();
        self.skip_whitespace();
        if self.consume(')') {
            return Ok(args);
        }

        loop {
            args.push(self.parse_expression()?);
            self.skip_whitespace();
            if self.consume(')') {
                break;
            }
            if self.consume(',') || self.consume(';') {
                continue;
            }
            bail!("Falta separador de argumentos o parentesis de cierre");
        }

        Ok(args)
    }

    fn resolve_constant(&self, name: &str) -> Result<f64> {
        match name {
            "pi" => Ok(std::f64::consts::PI),
            "e" => Ok(std::f64::consts::E),
            "tau" => Ok(std::f64::consts::TAU),
            _ => bail!("Identificador no reconocido: {name}"),
        }
    }

    fn evaluate_function(&self, name: &str, args: &[f64]) -> Result<f64> {
        match name {
            "sin" => one_arg(name, args).map(f64::sin),
            "cos" => one_arg(name, args).map(f64::cos),
            "tan" => one_arg(name, args).map(f64::tan),
            "asin" => one_arg(name, args).map(f64::asin),
            "acos" => one_arg(name, args).map(f64::acos),
            "atan" => one_arg(name, args).map(f64::atan),
            "sinh" => one_arg(name, args).map(f64::sinh),
            "cosh" => one_arg(name, args).map(f64::cosh),
            "tanh" => one_arg(name, args).map(f64::tanh),
            "sqrt" => one_arg(name, args).map(f64::sqrt),
            "cbrt" => one_arg(name, args).map(f64::cbrt),
            "ln" => one_arg(name, args).map(f64::ln),
            "log10" => one_arg(name, args).map(f64::log10),
            "exp" => one_arg(name, args).map(f64::exp),
            "abs" => one_arg(name, args).map(f64::abs),
            "floor" => one_arg(name, args).map(f64::floor),
            "ceil" => one_arg(name, args).map(f64::ceil),
            "round" => one_arg(name, args).map(f64::round),
            "trunc" => one_arg(name, args).map(f64::trunc),
            "sign" => one_arg(name, args).map(f64::signum),
            "rad" => one_arg(name, args).map(f64::to_radians),
            "deg" => one_arg(name, args).map(f64::to_degrees),
            "pow" => two_args(name, args).map(|(a, b)| a.powf(b)),
            "min" => min_args(name, args).map(|slice| {
                slice
                    .iter()
                    .fold(f64::INFINITY, |acc, &value| acc.min(value))
            }),
            "max" => min_args(name, args).map(|slice| {
                slice
                    .iter()
                    .fold(f64::NEG_INFINITY, |acc, &value| acc.max(value))
            }),
            "clamp" => {
                let (value, min, max) = three_args(name, args)?;
                Ok(value.clamp(min, max))
            }
            "fact" => one_arg(name, args).and_then(factorial),
            "perm" => two_args(name, args).and_then(|(n, r)| permutation(n, r)),
            "comb" => two_args(name, args).and_then(|(n, r)| combination(n, r)),
            "log" => {
                if args.len() == 1 {
                    return Ok(args[0].ln());
                }
                let (value, base) = two_args(name, args)?;
                if base <= 0.0 || (base - 1.0).abs() < f64::EPSILON {
                    bail!("Base invalida para log");
                }
                Ok(value.log(base))
            }
            _ => bail!("Funcion no reconocida: {name}"),
        }
    }

    fn should_apply_implicit_multiplication(&self) -> bool {
        matches!(
            self.current(),
            Some(character)
                if character == '('
                    || character == '.'
                    || character == ','
                    || character.is_ascii_digit()
                    || character.is_ascii_alphabetic()
                    || character == '_'
        )
    }

    fn parse_number(&mut self) -> Result<f64> {
        self.skip_whitespace();
        let start = self.index;
        let mut saw_digit = false;

        while let Some(character) = self.current() {
            if character.is_ascii_digit() {
                saw_digit = true;
                self.index += 1;
            } else {
                break;
            }
        }

        if matches!(self.current(), Some('.') | Some(',')) {
            self.index += 1;
            while let Some(character) = self.current() {
                if character.is_ascii_digit() {
                    saw_digit = true;
                    self.index += 1;
                } else {
                    break;
                }
            }
        }

        if matches!(self.current(), Some('e') | Some('E')) {
            let exponent_start = self.index;
            self.index += 1;
            if matches!(self.current(), Some('+') | Some('-')) {
                self.index += 1;
            }

            let mut exponent_has_digit = false;
            while let Some(character) = self.current() {
                if character.is_ascii_digit() {
                    exponent_has_digit = true;
                    self.index += 1;
                } else {
                    break;
                }
            }

            if !exponent_has_digit {
                self.index = exponent_start;
            }
        }

        if start == self.index || !saw_digit {
            bail!("Se esperaba un numero");
        }

        let token: String = self.source[start..self.index].iter().collect();
        token
            .replace(',', ".")
            .parse::<f64>()
            .map_err(|_| anyhow::anyhow!("Numero invalido"))
    }
}

fn one_arg(name: &str, args: &[f64]) -> Result<f64> {
    if args.len() != 1 {
        bail!("La funcion {name} requiere 1 argumento");
    }
    Ok(args[0])
}

fn two_args(name: &str, args: &[f64]) -> Result<(f64, f64)> {
    if args.len() != 2 {
        bail!("La funcion {name} requiere 2 argumentos");
    }
    Ok((args[0], args[1]))
}

fn three_args(name: &str, args: &[f64]) -> Result<(f64, f64, f64)> {
    if args.len() != 3 {
        bail!("La funcion {name} requiere 3 argumentos");
    }
    Ok((args[0], args[1], args[2]))
}

fn min_args<'a>(name: &str, args: &'a [f64]) -> Result<&'a [f64]> {
    if args.is_empty() {
        bail!("La funcion {name} requiere al menos 1 argumento");
    }
    Ok(args)
}

fn require_non_negative_integer(value: f64, label: &str) -> Result<u64> {
    if value < 0.0 {
        bail!("{label} debe ser no negativo");
    }

    let rounded = value.round();
    if (value - rounded).abs() > 1e-9 {
        bail!("{label} debe ser entero");
    }

    Ok(rounded as u64)
}

fn factorial(value: f64) -> Result<f64> {
    let n = require_non_negative_integer(value, "n")?;
    if n > 170 {
        bail!("n demasiado grande para fact");
    }

    let mut acc = 1.0;
    for current in 2..=n {
        acc *= current as f64;
    }
    Ok(acc)
}

fn permutation(n: f64, r: f64) -> Result<f64> {
    let n_int = require_non_negative_integer(n, "n")?;
    let r_int = require_non_negative_integer(r, "r")?;
    if r_int > n_int {
        bail!("r no puede ser mayor que n");
    }

    if n_int > 170 {
        bail!("n demasiado grande para perm");
    }

    let mut acc = 1.0;
    for current in (n_int - r_int + 1)..=n_int {
        acc *= current as f64;
    }
    Ok(acc)
}

fn combination(n: f64, r: f64) -> Result<f64> {
    let n_int = require_non_negative_integer(n, "n")?;
    let r_int = require_non_negative_integer(r, "r")?;
    if r_int > n_int {
        bail!("r no puede ser mayor que n");
    }

    if n_int > 170 {
        bail!("n demasiado grande para comb");
    }

    let k = r_int.min(n_int - r_int);
    if k == 0 {
        return Ok(1.0);
    }

    let mut acc = 1.0;
    for i in 1..=k {
        let numerator = (n_int - k + i) as f64;
        let denominator = i as f64;
        acc *= numerator / denominator;
    }
    Ok(acc)
}
