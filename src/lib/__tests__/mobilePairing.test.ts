/**
 * @vitest-environment node
 */
import { describe, it, expect, afterEach, beforeEach, vi } from "vitest";
import {
  findPairedDevice,
  issuePairingCode,
  mintDeviceId,
  mintDeviceToken,
  parseBearerToken,
  parsePairedDevices,
  readPendingPairingCode,
  redeemPairingCode,
  revokePairingCode,
  serializePairedDevices,
  type PairedDevice,
} from "@/lib/mobilePairing";

const NOW = 1_800_000_000_000;
const PAIRING_CODE_TTL_MS = 5 * 60_000;
const PAIRING_CODE_MAX_FAILURES = 5;
const PAIRING_CODE_FAILURE_DELAY_MS = 300;

/** 발급된 코드와 반드시 다른 여섯 자리 */
function wrongCodeFor(code: string): string {
  return code === "000000" ? "111111" : "000000";
}

/**
 * 실패한 교환은 300ms를 기다린다. 실제로 재우면 시도 횟수만큼 테스트가 느려지므로 시계를 대신 민다.
 * 성공한 교환에는 걸린 타이머가 없어 미는 것이 아무 일도 하지 않는다.
 */
async function redeem(submittedCode: string, now: number = NOW): Promise<boolean> {
  const redeeming = redeemPairingCode(submittedCode, now);
  await vi.advanceTimersByTimeAsync(PAIRING_CODE_FAILURE_DELAY_MS);
  return redeeming;
}

/** 대기 중인 마이크로태스크만 흘려보낸다. 타이머는 밀지 않아 실패 지연이 그대로 남는다 */
async function flushMicrotasks(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
}

function pairedDevice(overrides: Partial<PairedDevice> = {}): PairedDevice {
  return {
    deviceId: "d1",
    token: "a".repeat(64),
    deviceName: "iPhone",
    pairedAt: "2026-09-08T00:00:00.000Z",
    ...overrides,
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  revokePairingCode();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("페어링 코드", () => {
  it("사람이 옮겨 적을 수 있는 6자리 숫자다", () => {
    expect(issuePairingCode(NOW).code).toMatch(/^\d{6}$/);
  });

  it("새로 발급하면 이전 코드는 죽는다", async () => {
    const first = issuePairingCode(NOW).code;
    issuePairingCode(NOW);

    expect(await redeem(first)).toBe(false);
  });

  it("맞는 코드는 교환된다", async () => {
    const { code } = issuePairingCode(NOW);

    expect(await redeem(code)).toBe(true);
  });

  it("한 번 교환한 코드는 다시 쓸 수 없다", async () => {
    const { code } = issuePairingCode(NOW);
    await redeem(code);

    expect(await redeem(code)).toBe(false);
  });

  it("틀린 코드는 코드를 소모하지 않아 다시 시도할 수 있다", async () => {
    const { code } = issuePairingCode(NOW);

    expect(await redeem(wrongCodeFor(code))).toBe(false);
    expect(await redeem(code)).toBe(true);
  });

  it("발급 5분 뒤에는 만료된다", async () => {
    const { code } = issuePairingCode(NOW);

    expect(readPendingPairingCode(NOW + PAIRING_CODE_TTL_MS)).toBeNull();
    expect(await redeem(code, NOW + PAIRING_CODE_TTL_MS)).toBe(false);
  });

  it("페어링 화면을 닫으면 코드가 사라진다", async () => {
    const { code } = issuePairingCode(NOW);
    revokePairingCode();

    expect(await redeem(code)).toBe(false);
  });

  it("코드가 없을 때 아무 값이나 넣어도 통과하지 않는다", async () => {
    expect(await redeem("000000")).toBe(false);
  });
});

/**
 * 코드 공간은 10^6뿐이고 `/api/mobile/pair`는 인증 앞단에 있다.
 * 실패를 세지 않으면 LAN에 있는 누구든 코드 수명 안에 전부 훑어 만료되지 않는 기기 토큰을 가져간다.
 */
describe("페어링 코드 시도 제한", () => {
  it("한도 직전까지 틀려도 맞는 코드로는 연결된다", async () => {
    const { code } = issuePairingCode(NOW);

    for (let attempt = 0; attempt < PAIRING_CODE_MAX_FAILURES - 1; attempt += 1) {
      expect(await redeem(wrongCodeFor(code))).toBe(false);
    }

    expect(await redeem(code)).toBe(true);
  });

  it("다섯 번 틀리면 코드를 버려 맞는 코드마저 통하지 않는다", async () => {
    const { code } = issuePairingCode(NOW);

    for (let attempt = 0; attempt < PAIRING_CODE_MAX_FAILURES; attempt += 1) {
      expect(await redeem(wrongCodeFor(code))).toBe(false);
    }

    expect(readPendingPairingCode(NOW)).toBeNull();
    expect(await redeem(code)).toBe(false);
  });

  it("코드를 새로 받으면 실패 횟수도 처음부터 다시 센다", async () => {
    const stale = issuePairingCode(NOW).code;
    for (let attempt = 0; attempt < PAIRING_CODE_MAX_FAILURES - 1; attempt += 1) {
      await redeem(wrongCodeFor(stale));
    }

    const { code } = issuePairingCode(NOW);
    expect(await redeem(wrongCodeFor(code))).toBe(false);
    expect(await redeem(code)).toBe(true);
  });

  it("틀린 코드는 지연이 지나기 전에는 답하지 않는다", async () => {
    const { code } = issuePairingCode(NOW);
    let isSettled = false;
    const redeeming = redeemPairingCode(wrongCodeFor(code), NOW).then((result) => {
      isSettled = true;
      return result;
    });

    await flushMicrotasks();
    expect(isSettled).toBe(false);

    await vi.advanceTimersByTimeAsync(PAIRING_CODE_FAILURE_DELAY_MS);
    expect(await redeeming).toBe(false);
  });

  it("맞는 코드는 지연 없이 곧바로 답한다", async () => {
    const { code } = issuePairingCode(NOW);
    let isSettled = false;
    const redeeming = redeemPairingCode(code, NOW).then((result) => {
      isSettled = true;
      return result;
    });

    await flushMicrotasks();

    expect(isSettled).toBe(true);
    expect(await redeeming).toBe(true);
  });
});

describe("기기 토큰", () => {
  it("추측하기 어려운 길이로 발급된다", () => {
    expect(mintDeviceToken()).toMatch(/^[0-9a-f]{64}$/);
  });

  it("두 번 발급하면 서로 다르다", () => {
    expect(mintDeviceToken()).not.toBe(mintDeviceToken());
  });

  it("기기 식별자는 같은 순간에 발급해도 겹치지 않는다", () => {
    expect(mintDeviceId()).not.toBe(mintDeviceId());
  });
});

describe("Bearer 헤더", () => {
  it("토큰만 꺼낸다", () => {
    expect(parseBearerToken("Bearer abc123")).toBe("abc123");
  });

  it("대소문자가 달라도 읽는다", () => {
    expect(parseBearerToken("bearer abc123")).toBe("abc123");
  });

  it("헤더가 없으면 null이다", () => {
    expect(parseBearerToken(undefined)).toBeNull();
    expect(parseBearerToken("")).toBeNull();
  });

  it("Bearer가 아닌 방식은 받지 않는다", () => {
    expect(parseBearerToken("Basic abc123")).toBeNull();
  });
});

describe("기기 조회", () => {
  it("저장된 토큰의 주인을 찾는다", () => {
    const device = pairedDevice({ token: "b".repeat(64), deviceName: "iPad" });

    expect(findPairedDevice([pairedDevice(), device], "b".repeat(64))).toEqual(device);
  });

  it("모르는 토큰은 거절한다", () => {
    expect(findPairedDevice([pairedDevice()], "c".repeat(64))).toBeNull();
  });

  it("토큰이 없으면 거절한다", () => {
    expect(findPairedDevice([pairedDevice()], null)).toBeNull();
  });

  it("길이가 다른 토큰도 예외 없이 거절한다", () => {
    expect(findPairedDevice([pairedDevice()], "short")).toBeNull();
  });
});

describe("기기 목록 저장", () => {
  it("저장하고 되읽으면 같다", () => {
    const devices = [pairedDevice(), pairedDevice({ token: "d".repeat(64), deviceName: "iPad" })];

    expect(parsePairedDevices(serializePairedDevices(devices))).toEqual(devices);
  });

  it("저장값이 없으면 빈 목록이다", () => {
    expect(parsePairedDevices(null)).toEqual([]);
  });

  it("저장값이 깨져 있어도 던지지 않는다", () => {
    expect(parsePairedDevices("{not json")).toEqual([]);
  });

  it("모양이 맞지 않는 항목은 버리고 나머지는 남긴다", () => {
    const raw = JSON.stringify([{ token: 1 }, { ...pairedDevice(), deviceId: "" }, pairedDevice()]);

    expect(parsePairedDevices(raw)).toEqual([pairedDevice()]);
  });
});
