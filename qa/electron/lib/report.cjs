const fs = require("node:fs");
const path = require("node:path");

function ensureDir(dirPath) {
  fs.mkdirSync(dirPath, { recursive: true });
}

function createQaRun(options = {}) {
  const rootDir = options.rootDir || process.cwd();
  const outputRoot = options.outputRoot || path.join(rootDir, "qa-output");
  const runId = options.runId || new Date().toISOString().replace(/[:.]/g, "-");
  const runDir = options.runDir || path.join(outputRoot, runId);
  const screenshotsDir = path.join(runDir, "screenshots");
  const diagnosticsDir = path.join(runDir, "diagnostics");
  ensureDir(screenshotsDir);
  ensureDir(diagnosticsDir);

  return {
    rootDir,
    outputRoot,
    runId,
    runDir,
    screenshotsDir,
    diagnosticsDir,
    reportPath: path.join(runDir, "report.md"),
    jsonPath: path.join(runDir, "result.json"),
    videoPath: path.join(runDir, "run.mp4"),
    tracePath: path.join(diagnosticsDir, "playwright-trace.zip"),
    cdpDiagnosticsPath: path.join(diagnosticsDir, "cdp-diagnostics.json"),
  };
}

function writeJson(filePath, data) {
  fs.writeFileSync(filePath, `${JSON.stringify(data, null, 2)}\n`, "utf8");
}

function renderMarkdown(result) {
  const lines = [];
  lines.push(`# KanVibe Electron QA Report`);
  lines.push("");
  lines.push(`- Run ID: \`${result.runId}\``);
  lines.push(`- Branch: \`${result.branch || "unknown"}\``);
  lines.push(`- Commit: \`${result.commit || "unknown"}\``);
  lines.push(`- Scope: ${result.scope}`);
  lines.push(`- Status: **${result.ok ? "PASS" : "FAIL"}**`);
  lines.push("");
  lines.push("## PR Regression Scope");
  lines.push("");
  lines.push("- #275 `fix(worktree): 실제 worktree 경로로 task 리소스 정리`");
  lines.push("- #276 `fix: task 삭제 리소스 정리 정책 공통화`");
  lines.push("");
  lines.push("## Checks");
  lines.push("");
  for (const check of result.checks || []) {
    lines.push(`- ${check.ok ? "✅" : "❌"} **${check.name}**${check.detail ? ` — ${check.detail}` : ""}`);
  }
  lines.push("");
  lines.push("## Console / Runtime Errors");
  lines.push("");
  if (result.errors?.length) {
    for (const error of result.errors) {
      lines.push(`- ${error}`);
    }
  } else {
    lines.push("No blocking console/runtime errors captured.");
  }
  lines.push("");
  lines.push("## Evidence");
  lines.push("");
  for (const shot of result.screenshots || []) {
    lines.push(`- ${shot.label}: \`${shot.path}\``);
  }
  if (result.videoPath) {
    lines.push(`- Video: \`${result.videoPath}\``);
  }
  lines.push("");
  lines.push("## Diagnostics");
  lines.push("");
  if (result.tracePath) {
    lines.push(`- Playwright trace: \`${result.tracePath}\``);
  }
  if (result.cdpDiagnosticsPath) {
    lines.push(`- CDP diagnostics: \`${result.cdpDiagnosticsPath}\``);
  }
  if (result.diagnostics?.length) {
    for (const diagnostic of result.diagnostics) {
      lines.push(`- ${diagnostic}`);
    }
  }
  lines.push("");
  lines.push("## Notes");
  lines.push("");
  for (const note of result.notes || []) {
    lines.push(`- ${note}`);
  }
  lines.push("");
  return `${lines.join("\n")}\n`;
}

function writeReport(run, result) {
  const fullResult = { ...result, runId: run.runId };
  writeJson(run.jsonPath, fullResult);
  fs.writeFileSync(run.reportPath, renderMarkdown(fullResult), "utf8");
}

module.exports = {
  createQaRun,
  ensureDir,
  writeReport,
};
