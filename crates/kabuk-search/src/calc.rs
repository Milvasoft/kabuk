//! The launcher's calculator (SPEC §2.5): `+ - * / ^`, parentheses and unary minus, a
//! percent (`15%` is 0.15), square roots (`sqrt(2)`, `√2`) and hexadecimal numbers
//! (`0xff`; PLAN M19 U20); a decimal separator may be `,` (Turkish) or `.`. Only text
//! with a digit and an operator (or a hexadecimal number) counts as a calculation, so
//! "12" or "kabuk" never shows a result.

/// Results are rounded to this many decimal places (as the design page does).
pub const DECIMALS: i32 = 6;
/// What a percent divides by.
const PERCENT: f64 = 100.0;

struct Parser<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Parser<'_> {
    fn peek(&mut self) -> Option<char> {
        while self.chars.peek().is_some_and(|c| c.is_whitespace()) {
            self.chars.next();
        }
        self.chars.peek().copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.chars.next();
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        loop {
            if self.eat('+') {
                v += self.term()?;
            } else if self.eat('-') {
                v -= self.term()?;
            } else {
                return Some(v);
            }
        }
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.unary()?;
        loop {
            if self.eat('*') || self.eat('×') {
                v *= self.unary()?;
            } else if self.eat('/') || self.eat('÷') {
                v /= self.unary()?;
            } else {
                return Some(v);
            }
        }
    }

    /// Minus binds looser than `^`: `-2^2` is -4.
    fn unary(&mut self) -> Option<f64> {
        if self.eat('-') {
            return Some(-self.unary()?);
        }
        if self.eat('+') {
            return self.unary();
        }
        self.power()
    }

    /// Right-associative: `2^3^2` is 2^9.
    fn power(&mut self) -> Option<f64> {
        let base = self.percent()?;
        if self.eat('^') {
            return Some(base.powf(self.unary()?));
        }
        Some(base)
    }

    /// A value with a percent sign after it is that many hundredths.
    fn percent(&mut self) -> Option<f64> {
        let v = self.atom()?;
        Some(if self.eat('%') { v / PERCENT } else { v })
    }

    fn atom(&mut self) -> Option<f64> {
        if self.eat('(') {
            let v = self.expr()?;
            return self.eat(')').then_some(v);
        }
        if self.eat('√') {
            return Some(self.power()?.sqrt());
        }
        if self.peek() == Some('s') {
            for c in "sqrt".chars() {
                if !self.eat(c) {
                    return None;
                }
            }
            return Some(self.atom()?.sqrt());
        }
        if self.peek() == Some('0') {
            self.chars.next();
            if self.chars.peek().is_some_and(|c| *c == 'x' || *c == 'X') {
                self.chars.next();
                return self.hex();
            }
            return self.decimal(String::from("0"));
        }
        self.decimal(String::new())
    }

    /// The hexadecimal digits after `0x`.
    fn hex(&mut self) -> Option<f64> {
        let mut text = String::new();
        while let Some(c) = self.chars.peek().copied().filter(char::is_ascii_hexdigit) {
            text.push(c);
            self.chars.next();
        }
        u64::from_str_radix(&text, 16).ok().map(|v| v as f64)
    }

    /// A decimal number, `text` its digits read so far.
    fn decimal(&mut self, mut text: String) -> Option<f64> {
        let mut separator = false;
        while let Some(c) = self.chars.peek().copied() {
            if c.is_ascii_digit() {
                text.push(c);
            } else if (c == ',' || c == '.') && !separator {
                separator = true;
                text.push('.');
            } else {
                break;
            }
            self.chars.next();
        }
        if !text.chars().any(|c| c.is_ascii_digit()) {
            return None;
        }
        text.parse().ok()
    }
}

/// The value of `query` if it is a calculation.
pub fn evaluate(query: &str) -> Option<f64> {
    let q = query.trim();
    let has_digit = q.chars().any(|c| c.is_ascii_digit());
    // A leading minus alone ("-5") is a number, not a calculation; a percent, a root or a
    // hexadecimal number is one by itself ("15%", "sqrt(2)", "0xff").
    let lower = q.to_ascii_lowercase();
    let has_operator = q.chars().skip(1).any(|c| "+-*/^×÷%".contains(c))
        || q.starts_with(['+', '*', '/', '^', '√'])
        || lower.contains("sqrt")
        || lower.contains("0x");
    if !has_digit || !has_operator {
        return None;
    }
    let mut p = Parser {
        chars: q.chars().peekable(),
    };
    let v = p.expr()?;
    if p.peek().is_some() || !v.is_finite() {
        return None;
    }
    let scale = 10f64.powi(DECIMALS);
    let rounded = (v * scale).round() / scale;
    Some(if rounded.is_finite() { rounded } else { v })
}

/// `v` without trailing zeros, with `decimal` as the separator.
pub fn format(v: f64, decimal: char) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    let s = format!("{v}");
    if decimal == '.' {
        s
    } else {
        s.replace('.', &decimal.to_string())
    }
}

#[cfg(test)]
mod tests {
    //! Edge cases (TESTING §3): `12*8`, `(3+4)/2`, `2^10`, `9,8+1`; precedence, unary
    //! minus, right-associative power, spaces, division by zero, unbalanced parentheses,
    //! two separators in a number, letters, a plain number, an operator alone, rounding,
    //! formatting with either separator and negative zero.
    use super::*;

    #[test]
    fn the_testing_examples() {
        assert_eq!(evaluate("12*8"), Some(96.0));
        assert_eq!(evaluate("(3+4)/2"), Some(3.5));
        assert_eq!(evaluate("2^10"), Some(1024.0));
        assert_eq!(evaluate("9,8+1"), Some(10.8));
    }

    #[test]
    fn precedence_and_signs() {
        assert_eq!(evaluate("2+3*4"), Some(14.0));
        assert_eq!(evaluate("-2^2"), Some(-4.0));
        assert_eq!(evaluate("2^3^2"), Some(512.0));
        assert_eq!(evaluate(" 10 - -3 "), Some(13.0));
        assert_eq!(evaluate("6×7"), Some(42.0));
        assert_eq!(evaluate("1.5*2"), Some(3.0));
        assert_eq!(evaluate("1/3"), Some(0.333333));
    }

    #[test]
    fn invalid_expressions_give_no_result() {
        for q in [
            "1/0", "(1+2", "1+2)", "1,2,3+1", "12+x", "kabuk", "12", "-5", "+", "", "1+", "*3",
        ] {
            assert_eq!(evaluate(q), None, "{q}");
        }
    }

    #[test]
    fn results_are_written_with_the_languages_separator() {
        assert_eq!(format(10.8, ','), "10,8");
        assert_eq!(format(10.8, '.'), "10.8");
        assert_eq!(format(96.0, ','), "96");
        assert_eq!(format(-0.0, ','), "0");
    }

    /// PLAN M19 U20: percent, square root and hexadecimal.
    #[test]
    fn percent_roots_and_hexadecimal() {
        assert_eq!(evaluate("15%"), Some(0.15));
        assert_eq!(evaluate("200*15%"), Some(30.0));
        assert_eq!(evaluate("sqrt(16)"), Some(4.0));
        assert_eq!(evaluate("√9+1"), Some(4.0));
        assert_eq!(evaluate("sqrt 2.25"), Some(1.5));
        assert_eq!(evaluate("0xff"), Some(255.0));
        assert_eq!(evaluate("0x10+1"), Some(17.0));
        assert_eq!(evaluate("0,5*4"), Some(2.0));
        assert_eq!(evaluate("sqr(4)"), None);
        assert_eq!(evaluate("0x"), None);
        assert_eq!(evaluate("sqrt(-1)"), None);
    }
}
