/**
 * @vitest-environment node
 */
import { describe, it, expect, beforeEach } from "vitest";
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
  revokePairingCode();
});

describe("페어링 코드", () => {
  it("사람이 옮겨 적을 수 있는 6자리 숫자다", () => {
    expect(issuePairingCode(NOW).code).toMatch(/^\d{6}$/);
  });

  it("새로 발급하면 이전 코드는 죽는다", () => {
    const first = issuePairingCode(NOW).code;
    issuePairingCode(NOW);

    expect(redeemPairingCode(first, NOW)).toBe(false);
  });

  it("맞는 코드는 교환된다", () => {
    const { code } = issuePairingCode(NOW);

    expect(redeemPairingCode(code, NOW)).toBe(true);
  });

  it("한 번 교환한 코드는 다시 쓸 수 없다", () => {
    const { code } = issuePairingCode(NOW);
    redeemPairingCode(code, NOW);

    expect(redeemPairingCode(code, NOW)).toBe(false);
  });

  it("틀린 코드는 코드를 소모하지 않아 다시 시도할 수 있다", () => {
    const { code } = issuePairingCode(NOW);

    expect(redeemPairingCode("000000" === code ? "111111" : "000000", NOW)).toBe(false);
    expect(redeemPairingCode(code, NOW)).toBe(true);
  });

  it("발급 5분 뒤에는 만료된다", () => {
    const { code } = issuePairingCode(NOW);

    expect(readPendingPairingCode(NOW + PAIRING_CODE_TTL_MS)).toBeNull();
    expect(redeemPairingCode(code, NOW + PAIRING_CODE_TTL_MS)).toBe(false);
  });

  it("페어링 화면을 닫으면 코드가 사라진다", () => {
    const { code } = issuePairingCode(NOW);
    revokePairingCode();

    expect(redeemPairingCode(code, NOW)).toBe(false);
  });

  it("코드가 없을 때 아무 값이나 넣어도 통과하지 않는다", () => {
    expect(redeemPairingCode("000000", NOW)).toBe(false);
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
