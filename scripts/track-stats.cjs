#!/usr/bin/env node

const { existsSync, mkdirSync, readFileSync, writeFileSync } = require("node:fs");
const path = require("node:path");

const REPO = "rookedsysc/kanvibe";
const ACCENT_COLOR = "#0064FF";
const AUTH_HEADERS = process.env.GITHUB_TOKEN ? { Authorization: `Bearer ${process.env.GITHUB_TOKEN}` } : {};

async function fetchAllReleases() {
  const releases = [];
  let page = 1;

  while (true) {
    const response = await fetch(`https://api.github.com/repos/${REPO}/releases?per_page=100&page=${page}`, {
      headers: { Accept: "application/vnd.github+json", ...AUTH_HEADERS },
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

async function fetchStarCount() {
  const response = await fetch(`https://api.github.com/repos/${REPO}`, {
    headers: { Accept: "application/vnd.github+json", ...AUTH_HEADERS },
  });
  if (!response.ok) {
    throw new Error(`GitHub repo 조회 실패: ${response.status} ${response.statusText}`);
  }

  const repo = await response.json();
  return repo.stargazers_count;
}

function readHistory(dataFile) {
  if (!existsSync(dataFile)) return [];
  return JSON.parse(readFileSync(dataFile, "utf8"));
}

function upsertPeriodCount(history, periodKey, count) {
  const periodEntry = history.find((entry) => entry.date === periodKey);
  if (periodEntry) {
    periodEntry.count = count;
  } else {
    history.push({ date: periodKey, count });
  }
  return history;
}

function writeHistory(dataFile, history) {
  mkdirSync(path.dirname(dataFile), { recursive: true });
  writeFileSync(dataFile, `${JSON.stringify(history, null, 2)}\n`);
}

function renderChartSvg(history, title) {
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
  <text x="${width / 2}" y="24" text-anchor="middle" font-size="14" font-weight="600" fill="#202632">${title}</text>
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

function updateMetric({ dataFile, chartFile, title, count, periodKey, maxEntries }) {
  let history = upsertPeriodCount(readHistory(dataFile), periodKey, count);
  if (maxEntries) history = history.slice(-maxEntries);
  writeHistory(dataFile, history);
  mkdirSync(path.dirname(chartFile), { recursive: true });
  writeFileSync(chartFile, renderChartSvg(history, title));
}

const DAILY_WINDOW_SIZE = 30;

async function main() {
  const docsDir = path.join(__dirname, "..", "docs");
  const now = new Date().toISOString();
  const dayKey = now.slice(0, 10);
  const monthKey = now.slice(0, 7);

  const releases = await fetchAllReleases();
  const downloadCount = sumDmgDownloadCount(releases);
  updateMetric({
    dataFile: path.join(docsDir, "data", "downloads.json"),
    chartFile: path.join(docsDir, "images", "readme", "downloads-chart.svg"),
    title: "KanVibe DMG Downloads (Last 30 Days)",
    count: downloadCount,
    periodKey: dayKey,
    maxEntries: DAILY_WINDOW_SIZE,
  });
  updateMetric({
    dataFile: path.join(docsDir, "data", "downloads-monthly.json"),
    chartFile: path.join(docsDir, "images", "readme", "downloads-chart-monthly.svg"),
    title: "KanVibe DMG Downloads (Monthly)",
    count: downloadCount,
    periodKey: monthKey,
  });

  const starCount = await fetchStarCount();
  updateMetric({
    dataFile: path.join(docsDir, "data", "stars.json"),
    chartFile: path.join(docsDir, "images", "readme", "star-history-chart.svg"),
    title: "KanVibe GitHub Stars (Last 30 Days)",
    count: starCount,
    periodKey: dayKey,
    maxEntries: DAILY_WINDOW_SIZE,
  });
  updateMetric({
    dataFile: path.join(docsDir, "data", "stars-monthly.json"),
    chartFile: path.join(docsDir, "images", "readme", "star-history-chart-monthly.svg"),
    title: "KanVibe GitHub Stars (Monthly)",
    count: starCount,
    periodKey: monthKey,
  });
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
