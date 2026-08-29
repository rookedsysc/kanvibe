#!/usr/bin/env node

const { existsSync, mkdirSync, readFileSync, writeFileSync } = require("node:fs");
const path = require("node:path");

const REPO = "rookedsysc/kanvibe";
const DATA_FILE = path.join(__dirname, "..", "docs", "data", "downloads.json");
const CHART_FILE = path.join(__dirname, "..", "docs", "images", "readme", "downloads-chart.svg");
const ACCENT_COLOR = "#0064FF";

async function fetchAllReleases() {
  const releases = [];
  let page = 1;

  while (true) {
    const response = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=100&page=${page}`, {
      headers: {
        Accept: "application/vnd.github+json",
        ...(process.env.GITHUB_TOKEN ? { Authorization: `Bearer ${process.env.GITHUB_TOKEN}` } : {}),
      },
    });
    if (!response.ok) {
      throw new Error(`GitHub releases 조회 실패: ${response.status} ${response.statusText}`);
    }

    const pageReleases = await response.json();
    releases.push(...pageReleases);
    if (pageReleases.length < 100) break;
    page += 1;
  }

  return releases;
}

function sumDmgDownloadCount(releases) {
  return releases
    .flatMap((release) => release.assets)
    .filter((asset) => asset.name.endsWith(".dmg"))
    .reduce((total, asset) => total + asset.download_count, 0);
}

function readHistory() {
  if (!existsSync(DATA_FILE)) return [];
  return JSON.parse(readFileSync(DATA_FILE, "utf8"));
}

function upsertTodayCount(history, count) {
  const today = new Date().toISOString().slice(0, 10);
  const todayEntry = history.find((entry) => entry.date === today);
  if (todayEntry) {
    todayEntry.count = count;
  } else {
    history.push({ date: today, count });
  }
  return history;
}

function writeHistory(history) {
  mkdirSync(path.dirname(DATA_FILE), { recursive: true });
  writeFileSync(DATA_FILE, `${JSON.stringify(history, null, 2)}\n`);
}

function renderChartSvg(history) {
  const width = 760;
  const height = 260;
  const paddingLeft = 60;
  const paddingRight = 30;
  const paddingTop = 40;
  const paddingBottom = 40;
  const plotWidth = width - paddingLeft - paddingRight;
  const plotHeight = height - paddingTop - paddingBottom;

  const counts = history.map((entry) => entry.count);
  const maxCount = Math.max(...counts);
  const minCount = Math.min(...counts);
  const countRange = Math.max(maxCount - minCount, 1);

  const points = history.map((entry, index) => {
    const x = paddingLeft + (history.length === 1 ? plotWidth / 2 : (plotWidth * index) / (history.length - 1));
    const y = paddingTop + plotHeight - ((entry.count - minCount) / countRange) * plotHeight;
    return { x, y };
  });

  const linePath = points.map((point, index) => `${index === 0 ? "M" : "L"} ${point.x.toFixed(1)} ${point.y.toFixed(1)}`).join(" ");
  const circles = points
    .map((point) => `<circle cx="${point.x.toFixed(1)}" cy="${point.y.toFixed(1)}" r="3" fill="${ACCENT_COLOR}" />`)
    .join("\n  ");

  const firstDate = history[0].date;
  const lastDate = history[history.length - 1].date;

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" font-family="-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif">
  <rect width="${width}" height="${height}" fill="#ffffff" />
  <text x="${width / 2}" y="24" text-anchor="middle" font-size="14" font-weight="600" fill="#202632">KanVibe DMG Downloads</text>
  <line x1="${paddingLeft}" y1="${paddingTop}" x2="${paddingLeft}" y2="${paddingTop + plotHeight}" stroke="#d0d5dd" stroke-width="1" />
  <line x1="${paddingLeft}" y1="${paddingTop + plotHeight}" x2="${paddingLeft + plotWidth}" y2="${paddingTop + plotHeight}" stroke="#d0d5dd" stroke-width="1" />
  <path d="${linePath}" fill="none" stroke="${ACCENT_COLOR}" stroke-width="2" />
  ${circles}
  <text x="${paddingLeft - 10}" y="${paddingTop + 4}" text-anchor="end" font-size="11" fill="#667085">${maxCount}</text>
  <text x="${paddingLeft - 10}" y="${paddingTop + plotHeight + 4}" text-anchor="end" font-size="11" fill="#667085">${minCount}</text>
  <text x="${paddingLeft}" y="${height - 12}" text-anchor="start" font-size="11" fill="#667085">${firstDate}</text>
  <text x="${paddingLeft + plotWidth}" y="${height - 12}" text-anchor="end" font-size="11" fill="#667085">${lastDate}</text>
</svg>
`;
}

async function main() {
  const releases = await fetchAllReleases();
  const count = sumDmgDownloadCount(releases);
  const history = upsertTodayCount(readHistory(), count);

  writeHistory(history);
  mkdirSync(path.dirname(CHART_FILE), { recursive: true });
  writeFileSync(CHART_FILE, renderChartSvg(history));
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
