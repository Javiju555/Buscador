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
            source: source.replace(',', ".").chars().collect(),
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
        self.parse_number()
    }

    fn parse_number(&mut self) -> Result<f64> {
        self.skip_whitespace();
        let start = self.index;
        while let Some(character) = self.current() {
            if character.is_ascii_digit() || character == '.' {
                self.index += 1;
            } else {
                break;
            }
        }

        if start == self.index {
            bail!("Se esperaba un numero");
        }

        let token: String = self.source[start..self.index].iter().collect();
        token
            .parse::<f64>()
            .map_err(|_| anyhow::anyhow!("Numero invalido"))
    }
}
