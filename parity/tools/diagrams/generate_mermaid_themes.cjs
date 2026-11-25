#!/usr/bin/env node
// Mermaid 11.15 테마 클래스(theme-base·dark·default·forest·neutral)를 Rust로 옮기고, sanitizeDirective가 쓰는 설정 열쇠 목록을 남긴다.
//
// node_modules/mermaid/dist/mermaid.js에서 각 테마 클래스의 constructor와 updateColors를 acorn으로 읽고, 문장 하나를
// Rust 문장 하나로 바꿔 crates/kanvibe-diagram/src/mermaid_theme/themes.rs에 쓴다. JS의 `||`·`??`·삼항식·`?.`는
// 런타임 도우미(JsValue)로 같은 의미를 지키고, khroma 호출은 mermaid_theme::khroma의 같은 이름 함수로 잇는다.
// Mermaid를 올리면 이 스크립트를 다시 돌리고 `cargo nextest run -p kanvibe-diagram mermaid_theme`로 골든 값을 확인한다.
//
//   node parity/tools/diagrams/generate_mermaid_themes.cjs
"use strict";

const fs = require("fs");
const path = require("path");

const repoRoot = path.resolve(__dirname, "../../..");
const acorn = require(path.join(repoRoot, "node_modules/acorn"));
const bundlePath = path.join(repoRoot, "node_modules/mermaid/dist/mermaid.js");
const outputPath = path.join(repoRoot, "crates/kanvibe-diagram/src/mermaid_theme/themes.rs");

// Mermaid 테마 이름 → 번들 안 클래스 이름
const THEME_CLASSES = { base: "Theme", dark: "Theme2", default: "Theme3", forest: "Theme4", neutral: "Theme5" };
// khroma·theme-helpers 함수 → Rust 함수
const CALLS = {
  adjust_default: "adjust",
  darken_default: "darken",
  lighten_default: "lighten",
  invert_default: "invert",
  is_dark_default: "is_dark",
  rgba_default: "rgba",
  mkBorder: "mk_border",
};
// erDiagram-oldHardcodedValues.ts
const IMPORTED_CONSTANTS = {
  oldAttributeBackgroundColorOdd: "#ffffff",
  oldAttributeBackgroundColorEven: "#f2f2f2",
};

const source = fs.readFileSync(bundlePath, "utf8");

function parseThemeClass(className) {
  const marker = `\n      ${className} = class {`;
  const start = source.indexOf(marker);
  if (start < 0) throw new Error(`${className} 클래스를 찾지 못했습니다`);
  const end = source.indexOf("\n      };\n", start);
  const text = source.slice(start + marker.length - "class {".length, end + "\n      }".length);
  return acorn.parseExpressionAt(`(${text})`, 0, { ecmaVersion: 2022 });
}

function rustString(text) {
  return JSON.stringify(text);
}

function rustNumber(value) {
  return Number.isInteger(value) ? `${value}.0` : `${value}`;
}

class Emitter {
  constructor(functionName) {
    this.functionName = functionName;
    this.locals = new Set();
    this.loopCounters = new Set();
  }

  propertyKey(member) {
    if (!member.computed) return rustString(member.property.name);
    return `&${this.expression(member.property)}.to_property_key()`;
  }

  expression(node) {
    switch (node.type) {
      case "Literal":
        if (typeof node.value === "string") return `JsValue::from(${rustString(node.value)})`;
        if (typeof node.value === "number") return `JsValue::Number(${rustNumber(node.value)})`;
        if (typeof node.value === "boolean") return `JsValue::Bool(${node.value})`;
        throw new Error(`모르는 리터럴 ${node.raw}`);
      case "Identifier":
        if (node.name in IMPORTED_CONSTANTS) return `JsValue::from(${rustString(IMPORTED_CONSTANTS[node.name])})`;
        if (this.loopCounters.has(node.name)) return `JsValue::Number(${node.name})`;
        if (this.locals.has(node.name)) return `${node.name}.clone()`;
        throw new Error(`모르는 이름 ${node.name}`);
      case "ChainExpression":
        return this.expression(node.expression);
      case "MemberExpression":
        if (node.object.type === "ThisExpression") return `theme.get(${this.propertyKey(node)})`;
        return `${this.expression(node.object)}.member(${this.propertyKey(node)})`;
      case "UnaryExpression":
        if (node.operator === "-" && node.argument.type === "Literal") return `JsValue::Number(${rustNumber(-node.argument.value)})`;
        if (node.operator === "-") return `${this.expression(node.argument)}.negate()`;
        if (node.operator === "void") return "JsValue::Undefined";
        throw new Error(`모르는 단항 연산 ${node.operator}`);
      case "BinaryExpression":
        return this.binary(node);
      case "LogicalExpression":
        return this.logical(node);
      case "ConditionalExpression":
        return `if ${this.expression(node.test)}.is_truthy() { ${this.expression(node.consequent)} } else { ${this.expression(node.alternate)} }`;
      case "CallExpression":
        return this.call(node);
      case "ObjectExpression":
        return `JsValue::object(vec![${node.properties
          .map((property) => `(${rustString(property.key.name ?? property.key.value)}, ${this.expression(property.value)})`)
          .join(", ")}])`;
      default:
        throw new Error(`모르는 식 ${node.type}`);
    }
  }

  binary(node) {
    const left = this.expression(node.left);
    const right = this.expression(node.right);
    switch (node.operator) {
      case "+":
        return `${left}.add(&${right})`;
      case "*":
        return `${left}.multiply(&${right})`;
      case "===":
        return `JsValue::Bool(${left}.strict_equals(&${right}))`;
      case "!==":
        return `JsValue::Bool(!${left}.strict_equals(&${right}))`;
      default:
        throw new Error(`모르는 이항 연산 ${node.operator}`);
    }
  }

  logical(node) {
    const left = this.expression(node.left);
    const right = this.expression(node.right);
    switch (node.operator) {
      case "||":
        return `{ let left = ${left}; if left.is_truthy() { left } else { ${right} } }`;
      case "&&":
        return `{ let left = ${left}; if left.is_truthy() { ${right} } else { left } }`;
      case "??":
        return `{ let left = ${left}; if left.is_nullish() { ${right} } else { left } }`;
      default:
        throw new Error(`모르는 논리 연산 ${node.operator}`);
    }
  }

  call(node) {
    const name = CALLS[node.callee.name];
    if (!name) throw new Error(`모르는 함수 ${node.callee.name}`);
    const args = node.arguments.map((argument) => `&${this.expression(argument)}`);
    return `${name}(&[${args.join(", ")}])?`;
  }

  statements(nodes, indent) {
    return nodes.map((node) => this.statement(node, indent)).join("");
  }

  statement(node, indent) {
    const pad = " ".repeat(indent);
    switch (node.type) {
      case "ExpressionStatement": {
        const assignment = node.expression;
        if (this.isUpdateColorsCall(assignment)) return `${pad}${this.functionName}_update_colors(theme)?;\n`;
        if (assignment.type !== "AssignmentExpression" || assignment.operator !== "=") {
          throw new Error(`대입이 아닌 문장 ${assignment.type}`);
        }
        if (assignment.left.type !== "MemberExpression" || assignment.left.object.type !== "ThisExpression") {
          throw new Error("this가 아닌 곳에 대입");
        }
        const key = this.propertyKey(assignment.left);
        return `${pad}let value = ${this.expression(assignment.right)};\n${pad}theme.set(${key}, value);\n`;
      }
      case "VariableDeclaration": {
        return node.declarations
          .map((declaration) => {
            this.locals.add(declaration.id.name);
            return `${pad}let ${declaration.id.name} = ${this.expression(declaration.init)};\n`;
          })
          .join("");
      }
      case "IfStatement": {
        const consequent = this.statements(this.blockBody(node.consequent), indent + 4);
        let text = `${pad}if ${this.expression(node.test)}.is_truthy() {\n${consequent}${pad}}`;
        if (node.alternate) {
          text += ` else {\n${this.statements(this.blockBody(node.alternate), indent + 4)}${pad}}`;
        }
        return `${text}\n`;
      }
      case "ForStatement":
        return this.forStatement(node, indent);
      default:
        throw new Error(`모르는 문장 ${node.type}`);
    }
  }

  // theme-default.js의 constructor는 끝에서 this.updateColors()를 한 번 더 부른다
  isUpdateColorsCall(expression) {
    return (
      expression.type === "CallExpression" &&
      expression.callee.type === "MemberExpression" &&
      expression.callee.object.type === "ThisExpression" &&
      expression.callee.property.name === "updateColors"
    );
  }

  blockBody(node) {
    return node.type === "BlockStatement" ? node.body : [node];
  }

  // for (let i = 0; i < limit; i++) — limit은 정의가 덮을 수 있는 THEME_COLOR_LIMIT일 수 있어 loop_limit이 상한을 둔다
  forStatement(node, indent) {
    const pad = " ".repeat(indent);
    const counter = node.init.declarations[0].id.name;
    if (node.init.declarations[0].init.value !== 0 || node.update.operator !== "++" || node.test.operator !== "<") {
      throw new Error("모르는 for 모양");
    }
    const limit = this.expression(node.test.right);
    this.loopCounters.add(counter);
    const body = this.statements(this.blockBody(node.body), indent + 4);
    this.loopCounters.delete(counter);
    return `${pad}for ${counter} in 0..loop_limit(&${limit}) {\n${pad}    let ${counter} = f64::from(${counter});\n${body}${pad}}\n`;
  }
}

function methodBody(classNode, name) {
  const method = classNode.body.body.find((member) => member.type === "MethodDefinition" && member.key.name === name);
  if (!method) throw new Error(`${name} 메서드가 없습니다`);
  return method.value.body.body;
}

function emitTheme(themeName, className) {
  const classNode = parseThemeClass(className);
  const functionName = themeName === "default" ? "default_theme" : themeName;
  const constructorBody = new Emitter(functionName).statements(methodBody(classNode, "constructor"), 4);
  const updateBody = new Emitter(functionName).statements(methodBody(classNode, "updateColors"), 4);
  return [
    `/// theme-${themeName}.js constructor`,
    `pub(super) fn ${functionName}_constructor(theme: &mut ThemeObject) -> Result<(), KhromaError> {`,
    constructorBody + "    Ok(())",
    "}",
    "",
    `/// theme-${themeName}.js updateColors`,
    `pub(super) fn ${functionName}_update_colors(theme: &mut ThemeObject) -> Result<(), KhromaError> {`,
    updateBody + "    Ok(())",
    "}",
    "",
  ].join("\n");
}

const header = `//! Mermaid 11.15 테마 클래스의 constructor·updateColors를 문장 단위로 옮긴 코드.
//!
//! parity/tools/diagrams/generate_mermaid_themes.cjs가 node_modules/mermaid/dist/mermaid.js에서 만든다. 손으로 고치지 않는다.

#![allow(clippy::all, clippy::pedantic, unused_braces)]

use super::khroma::{KhromaError, adjust, darken, invert, is_dark, lighten, mk_border, rgba};
use super::{JsValue, ThemeObject, loop_limit};

`;

const body = Object.entries(THEME_CLASSES)
  .map(([themeName, className]) => emitTheme(themeName, className))
  .join("\n");
fs.writeFileSync(outputPath, header + body);
console.log(`wrote ${path.relative(repoRoot, outputPath)}`);

// sanitizeDirective가 남기는 열쇠 이름: Mermaid `configKeys = new Set(keyify(defaultConfig))`와 같은 방법으로 모은다
function keyify(object) {
  return Object.keys(object).reduce((keys, key) => {
    const value = object[key];
    if (Array.isArray(value)) return keys;
    if (typeof value === "object" && value !== null) return [...keys, key, ...keyify(value)];
    return [...keys, key];
  }, []);
}

async function writeConfigKeys() {
  const chunkPath = path.join(repoRoot, "node_modules/mermaid/dist/chunks/mermaid.core/chunk-CSCIHK7Q.mjs");
  const chunk = await import(chunkPath);
  const keys = [...new Set(keyify(chunk.defaultConfig_default))].sort();
  const configKeysPath = path.join(repoRoot, "crates/kanvibe-diagram/src/mermaid_theme/config_keys.rs");
  const lines = keys.map((key) => `    ${rustString(key)},`).join("\n");
  fs.writeFileSync(
    configKeysPath,
    `//! Mermaid 11.15 \`configKeys\`(기본 설정의 모든 열쇠 이름). \`sanitizeDirective\`는 이 목록에 없는 열쇠를 지운다.\n//!\n` +
      `//! parity/tools/diagrams/generate_mermaid_themes.cjs가 만든다. 손으로 고치지 않는다.\n\n` +
      `/// 정렬된 열쇠 이름\npub(crate) const CONFIG_KEYS: [&str; ${keys.length}] = [\n${lines}\n];\n`,
  );
  console.log(`wrote ${path.relative(repoRoot, configKeysPath)} (${keys.length} keys)`);
}

writeConfigKeys();
