"""골든 기대값의 플레이스홀더 문법과 비교 규칙.

README의 "Volatile-field rules"가 이 파일의 규칙을 사람 말로 옮긴 것이다. 둘이 어긋나면 이 파일이 기준이다.
"""

from __future__ import annotations

import re
from typing import Any

WHOLE_MATCHER = re.compile(r"^\{\{(any(?::[a-z0-9_?]+)?|var:[A-Za-z0-9_]+|regex:.+|contains:.+)\}\}$", re.S)
VAR_REFERENCE = re.compile(r"\{\{var:([A-Za-z0-9_]+)\}\}")

ISO_8601 = re.compile(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,9})?(Z|[+-]\d{2}:\d{2})$")
UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")
HEX = re.compile(r"^[0-9a-f]+$")
DIGITS = re.compile(r"^\d+$")
BASE64 = re.compile(r"^[A-Za-z0-9+/]+={0,2}$")


def _type_check(kind: str, actual: Any) -> bool:
    nullable = kind.endswith("?")
    kind = kind.rstrip("?")
    if actual is None:
        return nullable or kind == ""
    checks = {
        "": lambda value: True,
        "string": lambda value: isinstance(value, str),
        "nonempty": lambda value: isinstance(value, str) and value != "",
        "iso8601": lambda value: isinstance(value, str) and bool(ISO_8601.match(value)),
        "uuid": lambda value: isinstance(value, str) and bool(UUID.match(value)),
        "hex64": lambda value: isinstance(value, str) and bool(HEX64.match(value)),
        "hex": lambda value: isinstance(value, str) and bool(HEX.match(value)),
        "digits": lambda value: isinstance(value, str) and bool(DIGITS.match(value)),
        "base64": lambda value: isinstance(value, str) and bool(BASE64.match(value)),
        "int": lambda value: isinstance(value, int) and not isinstance(value, bool),
        "number": lambda value: isinstance(value, (int, float)) and not isinstance(value, bool),
        "bool": lambda value: isinstance(value, bool),
        "object": lambda value: isinstance(value, dict),
        "array": lambda value: isinstance(value, list),
    }
    if kind not in checks:
        raise ValueError(f"unknown matcher kind any:{kind}")
    return checks[kind](actual)


def interpolate(template: Any, variables: dict[str, Any]) -> Any:
    """요청 쪽 템플릿의 {{var:x}}를 실제 값으로 바꾼다. 문자열 전체가 참조 하나면 값의 타입을 유지한다"""
    if isinstance(template, str):
        whole = VAR_REFERENCE.fullmatch(template)
        if whole:
            return variables[whole.group(1)]
        return VAR_REFERENCE.sub(lambda match: str(variables[match.group(1)]), template)
    if isinstance(template, list):
        return [interpolate(item, variables) for item in template]
    if isinstance(template, dict):
        return {interpolate(key, variables): interpolate(value, variables) for key, value in template.items()}
    return template


def match(expected: Any, actual: Any, variables: dict[str, Any], path: str = "$") -> list[str]:
    """기대값 패턴과 실제 값을 비교해 어긋난 곳의 설명 목록을 돌려준다. 빈 목록이면 일치다"""
    if isinstance(expected, str):
        whole = WHOLE_MATCHER.match(expected)
        if whole:
            token = whole.group(1)
            if token == "any":
                return []
            if token.startswith("any:"):
                return [] if _type_check(token[4:], actual) else [f"{path}: expected {expected}, got {actual!r}"]
            if token.startswith("var:"):
                name = token[4:]
                if name not in variables:
                    return [f"{path}: variable {name} is not defined"]
                return [] if variables[name] == actual else [f"{path}: expected var {name}={variables[name]!r}, got {actual!r}"]
            if token.startswith("regex:"):
                ok = isinstance(actual, str) and re.fullmatch(token[6:], actual, re.S)
                return [] if ok else [f"{path}: expected {expected}, got {actual!r}"]
            if token.startswith("contains:"):
                needle = interpolate(token[9:], variables)
                ok = isinstance(actual, str) and needle in actual
                return [] if ok else [f"{path}: expected text containing {needle!r}, got {str(actual)[:200]!r}"]
        if VAR_REFERENCE.search(expected):
            try:
                expected = interpolate(expected, variables)
            except KeyError as error:
                return [f"{path}: variable {error} is not defined"]
        if not isinstance(actual, str) or actual != expected:
            return [f"{path}: expected {expected!r}, got {actual!r}"]
        return []
    if isinstance(expected, dict):
        if not isinstance(actual, dict):
            return [f"{path}: expected object, got {actual!r}"]
        try:
            # 응답 객체의 키가 id인 경우(diff-stats 등) 키 자리의 {{var:x}}도 실제 값으로 바꿔 비교한다
            expected = {interpolate(key, variables): value for key, value in expected.items()}
        except KeyError as error:
            return [f"{path}: variable {error} is not defined"]
        errors = []
        missing = [key for key in expected if key not in actual]
        extra = [key for key in actual if key not in expected]
        if missing:
            errors.append(f"{path}: missing keys {missing}")
        if extra:
            errors.append(f"{path}: unexpected keys {extra}")
        for key in expected:
            if key in actual:
                errors.extend(match(expected[key], actual[key], variables, f"{path}.{key}"))
        return errors
    if isinstance(expected, list):
        if not isinstance(actual, list):
            return [f"{path}: expected array, got {actual!r}"]
        if len(expected) != len(actual):
            return [f"{path}: expected {len(expected)} items, got {len(actual)}"]
        errors = []
        for index, (item_expected, item_actual) in enumerate(zip(expected, actual)):
            errors.extend(match(item_expected, item_actual, variables, f"{path}[{index}]"))
        return errors
    if type(expected) is not type(actual) and not (
        isinstance(expected, (int, float)) and isinstance(actual, (int, float))
        and not isinstance(expected, bool) and not isinstance(actual, bool)
    ):
        return [f"{path}: expected {expected!r}, got {actual!r}"]
    if expected != actual:
        return [f"{path}: expected {expected!r}, got {actual!r}"]
    return []


def extract(document: Any, selector: str) -> Any:
    """캡처 경로. `a.b[0].c`, 배열 원소를 조건으로 고르는 `items[name=demo].id`를 지원한다"""
    current = document
    for part in re.findall(r"[^.\[\]]+|\[[^\]]*\]", selector):
        if part.startswith("["):
            inner = part[1:-1]
            if "=" in inner:
                field, wanted = inner.split("=", 1)
                current = next(item for item in current if str(item.get(field)) == wanted)
            else:
                current = current[int(inner)]
        else:
            current = current[part]
    return current
