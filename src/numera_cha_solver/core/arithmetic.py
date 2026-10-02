"""A deliberately small integer grammar, never executable expressions."""

import re

from .contracts import InputError

_EXPRESSION = re.compile(r"\s*([+-]?[0-9]{1,6})\s*([+*/-])\s*([+-]?[0-9]{1,6})\s*(?:=\s*\??)?\s*\Z")


def evaluate(expression: str, limit: int = 100_000) -> str:
    match = _EXPRESSION.fullmatch(expression)
    if not match:
        raise InputError(
            "invalid_expression", "Use two bounded integers and one +, -, *, or / operator."
        )
    left, operator, right = match.groups()
    a, b = int(left), int(right)
    if abs(a) > limit or abs(b) > limit:
        raise InputError("invalid_expression", "Arithmetic operand exceeds the configured bound.")
    if operator == "/":
        if b == 0 or a % b:
            raise InputError(
                "invalid_expression", "Division must have a nonzero divisor and integer result."
            )
        result = a // b
    elif operator == "*":
        result = a * b
    elif operator == "+":
        result = a + b
    else:
        result = a - b
    if abs(result) > limit:
        raise InputError("invalid_expression", "Arithmetic result exceeds the configured bound.")
    return str(result)
