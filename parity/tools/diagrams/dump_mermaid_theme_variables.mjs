#!/usr/bin/env node
// Mermaid 11.15 getThemeVariables 결과를 골든 값으로 남긴다.
//
// 테마 5종(base·dark·default·forest·neutral) × 덮을 값 묶음마다 Mermaid 번들의 테마 클래스를 그대로 돌려
// parity/diagrams/mermaid-theme-variables.json에 쓴다. kanvibe-diagram의 mermaid_theme 테스트가 이 파일과 견준다.
// 함수가 아닌 속성만 남기고, khroma가 던지면 `error`에 메시지를 적는다.
//
//   node parity/tools/diagrams/dump_mermaid_theme_variables.mjs
import { writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "../../..");
const chunk = await import(join(repoRoot, "node_modules/mermaid/dist/chunks/mermaid.core/chunk-CSCIHK7Q.mjs"));

const THEMES = ["base", "dark", "default", "forest", "neutral"];
const OVERRIDE_CASES = {
  none: undefined,
  empty: {},
  primary: { primaryColor: "#ff0000" },
  docsExample: {
    primaryColor: "#BB2528",
    primaryTextColor: "#fff",
    primaryBorderColor: "#7C0000",
    lineColor: "#F8B229",
    secondaryColor: "#006100",
    tertiaryColor: "#fff",
  },
  darkMode: { darkMode: true, background: "#222", primaryColor: "hsl(200, 50%, 40%)" },
  nested: { radar: { curveOpacity: 0.25 }, xyChart: { plotColorPalette: "#f00,#0f0" }, fontSize: "20px", pieOpacity: "0.5" },
  functionColors: { primaryColor: "rgb(10 20 30 / 50%)", secondaryColor: "transparent", tertiaryColor: "HSLA(0.5turn, 40%, 60%, 0.8)" },
  scaleLimit: { THEME_COLOR_LIMIT: 4, cScale0: "#123456", git0: "red" },
  unsupportedKeyword: { primaryColor: "steelblue" },
};

function computeVariables(theme, overrides) {
  const variables = chunk.themes_default[theme].getThemeVariables(overrides === undefined ? undefined : structuredClone(overrides));
  const plain = {};
  for (const [name, value] of Object.entries(variables)) {
    if (typeof value !== "function") plain[name] = value;
  }
  return plain;
}

// 파일을 줄이려고 `none`이 아닌 묶음은 `none`과 달라진 변수(changed)와 사라진 변수(removed)만 남긴다
const result = {};
for (const theme of THEMES) {
  const plainTheme = computeVariables(theme, undefined);
  for (const [caseName, overrides] of Object.entries(OVERRIDE_CASES)) {
    const key = `${theme}/${caseName}`;
    if (overrides === undefined) {
      result[key] = { overrides: null, variables: plainTheme };
      continue;
    }
    try {
      const variables = computeVariables(theme, overrides);
      const changed = Object.fromEntries(
        Object.entries(variables).filter(([name, value]) => JSON.stringify(value) !== JSON.stringify(plainTheme[name])),
      );
      const removed = Object.keys(plainTheme).filter((name) => !(name in variables));
      result[key] = { overrides, changed, removed };
    } catch (error) {
      result[key] = { overrides, error: String(error.message) };
    }
  }
}
const outputPath = join(repoRoot, "parity/diagrams/mermaid-theme-variables.json");
writeFileSync(outputPath, JSON.stringify(result) + "\n");
console.log(`wrote ${Object.keys(result).length} cases`);
