/* eslint-disable @typescript-eslint/no-require-imports */
/**
 * Electron main 프로세스에만 `electron -r <이 파일>` 인자로 들어가는 스크린샷용 shim이다. TS 앱을 지울 때 함께 지운다.
 *
 * 하는 일은 세 가지다.
 * 첫째, 시계를 고정 시각(KANVIBE_PARITY_FIXED_NOW_MS)에서 출발하게 한다. 실행 중에는 실제 경과 시간만큼만 흐르므로
 * 앱 안의 대기 루프는 그대로 동작하고, "N분 전" 같은 표시는 fixture 시각 기준으로 계산된다.
 * 둘째, 바깥 네트워크를 막는다. fixture 표(KANVIBE_PARITY_NETWORK_FIXTURE)에 있는 URL은 정해진 응답을 돌려주고,
 * localhost가 아닌 나머지는 즉시 실패시킨다.
 * 셋째, IPC 처리기를 감싸 드라이버가 특정 서비스 호출을 실패·지연·무한 대기로 바꿀 수 있게 한다(globalThis.__kanvibeParity.faults).
 *
 * Electron main이 아닌 node 프로세스(앱이 띄운 셸 안의 node 등)에서는 아무것도 하지 않는다.
 */
const fs = require("node:fs");

function isElectronMainProcess() {
  return Boolean(process.versions.electron) && process.type === "browser";
}

function installFixedClock(fixedNowMs) {
  const RealDate = Date;
  const realStartMs = RealDate.now();

  class ParityDate extends RealDate {
    constructor(...args) {
      if (args.length === 0) {
        super(ParityDate.now());
      } else {
        super(...args);
      }
    }

    static now() {
      return fixedNowMs + (RealDate.now() - realStartMs);
    }
  }

  globalThis.Date = ParityDate;
}

function isLoopbackHost(hostname) {
  return hostname === "localhost" || hostname === "127.0.0.1" || hostname === "::1" || hostname === "[::1]";
}

function readNetworkFixture(filePath) {
  if (!filePath) return [];
  const parsed = JSON.parse(fs.readFileSync(filePath, "utf8"));
  return Array.isArray(parsed.responses) ? parsed.responses : [];
}

function findFixtureResponse(responses, url) {
  return responses.find((entry) => url.startsWith(entry.urlPrefix)) ?? null;
}

function installNetworkGuard(responses) {
  const realFetch = globalThis.fetch;
  globalThis.fetch = async (input, init) => {
    const url = typeof input === "string" ? input : input instanceof URL ? input.href : input.url;
    const parsedUrl = new URL(url);
    if (isLoopbackHost(parsedUrl.hostname)) {
      return realFetch(input, init);
    }

    const fixture = findFixtureResponse(responses, url);
    if (fixture) {
      const body = typeof fixture.body === "string" ? fixture.body : JSON.stringify(fixture.body ?? null);
      return new Response(body, { status: fixture.status ?? 200, headers: fixture.headers ?? { "content-type": "application/json" } });
    }

    throw new TypeError(`fetch failed: network blocked by screenshot fixture (${parsedUrl.hostname})`);
  };

  for (const moduleName of ["http", "https"]) {
    const transport = require(moduleName);
    for (const methodName of ["request", "get"]) {
      const original = transport[methodName];
      transport[methodName] = function guardedRequest(...args) {
        const first = args[0];
        const hostname = typeof first === "string" || first instanceof URL
          ? new URL(first).hostname
          : (first?.hostname ?? first?.host ?? "localhost");
        if (!isLoopbackHost(String(hostname).replace(/:\d+$/, ""))) {
          throw new Error(`network blocked by screenshot fixture (${hostname})`);
        }
        return original.apply(this, args);
      };
    }
  }
}

function describeInvokeTarget(channel, args) {
  if (channel === "kanvibe:invoke") {
    return `${args[0]}.${args[1]}`;
  }
  return channel;
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * 응답하지 않는 호출의 resolve 함수를 붙잡아 둔다. 아무도 참조하지 않는 promise는 GC되고, 그러면 Electron이
 * 그 IPC 호출을 "reply was never sent" 오류로 끝내 버려 "로딩 중" 상태가 시간에 따라 "실패"로 바뀐다.
 */
const heldReplies = [];

async function applyFault(fault) {
  if (fault.mode === "hang") {
    return new Promise((resolve) => {
      heldReplies.push(resolve);
    });
  }
  if (fault.mode === "delay") {
    await delay(fault.ms ?? 1000);
    return undefined;
  }
  if (fault.mode === "fail") {
    if (fault.ms) await delay(fault.ms);
    throw new Error(fault.message ?? "parity fixture: injected failure");
  }
  return undefined;
}

/**
 * 대상 이름(`namespace.method` 또는 채널), `namespace.*` 와일드카드 순서로 찾는다.
 * match가 있으면 호출 인자(JSON 문자열)에 그 글자가 들어 있을 때만, afterCalls가 있으면 그 횟수를 넘긴 호출부터 적용한다.
 */
function findFault(faults, target, args, callCount) {
  const namespaceWildcard = target.includes(".") ? `${target.split(".")[0]}.*` : null;
  const candidates = [faults[target], namespaceWildcard ? faults[namespaceWildcard] : undefined]
    .flatMap((entry) => (Array.isArray(entry) ? entry : entry ? [entry] : []));
  const serializedArgs = JSON.stringify(args.slice(channelArgumentOffset(target)));
  return candidates.find((fault) => (
    (fault.afterCalls ?? 0) < callCount
    && (!fault.match || serializedArgs.includes(fault.match))
  )) ?? null;
}

function channelArgumentOffset(target) {
  return target.startsWith("kanvibe:") ? 0 : 2;
}

function installFaultInjection() {
  const parity = { faults: {}, calls: {} };
  globalThis.__kanvibeParity = parity;
  const { ipcMain } = require("electron");
  const originalHandle = ipcMain.handle.bind(ipcMain);

  ipcMain.handle = (channel, handler) => originalHandle(channel, async (event, ...args) => {
    const target = describeInvokeTarget(channel, args);
    parity.calls[target] = (parity.calls[target] ?? 0) + 1;
    const fault = findFault(parity.faults, target, args, parity.calls[target]);
    if (fault) {
      await applyFault(fault);
      if (fault.mode === "result") {
        return fault.value;
      }
    }
    return handler(event, ...args);
  });
}

if (isElectronMainProcess()) {
  const fixedNowMs = Number(process.env.KANVIBE_PARITY_FIXED_NOW_MS);
  if (Number.isFinite(fixedNowMs) && fixedNowMs > 0) {
    installFixedClock(fixedNowMs);
  }
  installNetworkGuard(readNetworkFixture(process.env.KANVIBE_PARITY_NETWORK_FIXTURE));
  installFaultInjection();
}
