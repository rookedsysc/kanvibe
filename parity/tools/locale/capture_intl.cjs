/**
 * TS 앱의 Electron이 날짜·시각을 locale별로 어떻게 적는지 기준값을 받는다. TS 앱을 지울 때 함께 지운다.
 *
 * TS 화면이 부르는 다섯 가지 옵션(toLocaleString, toLocaleDateString, toLocaleTimeString 두 가지,
 * 월·일·시각 toLocaleDateString)을 앱이 지원하는 세 언어(ko·en·zh)와 그 지역 표기로 돌린다.
 * Node 모드(ELECTRON_RUN_AS_NODE)도 렌더러와 같은 V8·icudtl.dat를 쓰므로 렌더러 출력과 같다.
 *
 * TZ=UTC ELECTRON_RUN_AS_NODE=1 xvfb-run -a pnpm exec electron parity/tools/locale/capture_intl.cjs \
 *   > parity/fixtures/locale/electron-intl.json
 */
const LOCALES = ["ko", "ko-KR", "en", "en-US", "zh", "zh-CN"];
// 오전·오후, 자정·정오, 한 자리·두 자리 월·일·시를 고루 담은 UTC 벽시계 [연, 월, 일, 시, 분, 초]
const WALL_CLOCKS = [
  [2026, 1, 15, 2, 57, 0],
  [2026, 1, 15, 14, 5, 9],
  [2026, 12, 31, 0, 0, 0],
  [2026, 10, 5, 12, 30, 45],
  [2026, 3, 9, 23, 59, 59],
  [2026, 11, 20, 9, 7, 3],
];
const STYLES = {
  dateTime: (date, locale) => date.toLocaleString(locale),
  date: (date, locale) => date.toLocaleDateString(locale),
  timeHourMinute: (date, locale) => date.toLocaleTimeString(locale, { hour: "numeric", minute: "2-digit" }),
  monthDayTimeHourMinute: (date, locale) =>
    date.toLocaleDateString(locale, { month: "numeric", day: "numeric", hour: "numeric", minute: "2-digit" }),
  timeTwoDigitHourMinute: (date, locale) => date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" }),
};

const outputs = {};
for (const locale of LOCALES) {
  outputs[locale] = {};
  for (const [style, format] of Object.entries(STYLES)) {
    outputs[locale][style] = WALL_CLOCKS.map(([year, month, day, hour, minute, second]) =>
      format(new Date(Date.UTC(year, month - 1, day, hour, minute, second)), locale),
    );
  }
}
const { electron, chrome, icu, cldr } = process.versions;
process.stdout.write(`${JSON.stringify({ electron, chrome, icu, cldr, wallClocks: WALL_CLOCKS, outputs }, null, 2)}\n`);
