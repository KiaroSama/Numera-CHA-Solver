use super::contracts::Error;
use regex::Regex;
use std::sync::LazyLock;

// Python \s additionally accepts the four ASCII information separators.
static EXPRESSION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A[\s\x1c-\x1f]*([+-]?[0-9]{1,6})[\s\x1c-\x1f]*([+*/-])[\s\x1c-\x1f]*([+-]?[0-9]{1,6})[\s\x1c-\x1f]*(?:=[\s\x1c-\x1f]*\??)?[\s\x1c-\x1f]*\z")
        .expect("The bounded arithmetic grammar is static.")
});

pub fn evaluate(expression: &str) -> std::result::Result<String, Error> {
    let invalid = || {
        Error::new(
            "invalid_expression",
            "Use two bounded integers and one +, -, *, or / operator.",
        )
    };
    let captures = EXPRESSION.captures(expression).ok_or_else(invalid)?;
    let a: i64 = captures[1].parse().map_err(|_| invalid())?;
    let b: i64 = captures[3].parse().map_err(|_| invalid())?;
    if a.abs() > 100_000 || b.abs() > 100_000 {
        return Err(Error::new(
            "invalid_expression",
            "Arithmetic operand exceeds the configured bound.",
        ));
    }
    let result = match &captures[2] {
        "/" => {
            if b == 0 || a % b != 0 {
                return Err(Error::new(
                    "invalid_expression",
                    "Division must have a nonzero divisor and integer result.",
                ));
            }
            a / b
        }
        "*" => a * b,
        "+" => a + b,
        _ => a - b,
    };
    if result.abs() > 100_000 {
        return Err(Error::new(
            "invalid_expression",
            "Arithmetic result exceeds the configured bound.",
        ));
    }
    Ok(result.to_string())
}
