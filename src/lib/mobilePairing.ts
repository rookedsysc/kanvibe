import { randomBytes, timingSafeEqual } from "crypto";

/**
 * 모바일 기기를 데스크탑에 한 번 연결하고, 그 뒤로는 저장된 토큰으로 계속 인증하는 규칙.
 *
 * hook 서버는 LAN에 열려 있고 `/api/hooks/*`는 인증이 없다. 태스크 상태를 바꾸는 것까지는 그대로 두지만
 * 터미널 pane을 읽고 쓰는 경로는 반드시 이 파일의 토큰을 통과해야 한다. 그것이 이 파일이 존재하는 이유다.
 *
 * 페어링 코드는 짧고 사람이 옮겨 적는 값이라 화면에 떠 있는 동안에만 살아 있고 한 번 쓰면 사라진다.
 * 반대로 기기 토큰은 길고 기기에만 저장되므로 만료시키지 않는다. 한 번 연결한 기기는 계속 쓸 수 있어야 한다.
 *
 * 이 파일은 부수효과 없는 순수 함수와, 아직 쓰이지 않은 페어링 코드를 담는 모듈 지역 상태만 가진다.
 * 기기 토큰의 영속화는 `mobileBridgeService`가 `app_settings`에 맡긴다.
 */

/** 사람이 화면을 보고 옮겨 적는 값이라 자릿수를 늘리지 않는다. 대신 수명이 짧고 한 번만 쓰인다 */
const PAIRING_CODE_DIGITS = 6;

/** 코드가 화면에 떠 있을 법한 시간. hook 설치 티켓과 같은 값을 쓴다 */
const PAIRING_CODE_TTL_MS = 5 * 60_000;

/**
 * 코드 하나가 견디는 실패 횟수. 넘어서면 코드를 버려 데스크탑에서 새로 받아야 한다.
 *
 * 오타 한 번에 새 코드를 받으러 가야 하면 못 쓸 물건이 된다는 판단은 그대로 두되, 여유를 무한히 주지는 않는다.
 * 5회는 사람이 여섯 자리를 옮겨 적다 틀리는 횟수보다 넉넉하고, 10^6 중 다섯 개를 찍는 쪽에는 아무것도 아니다.
 * 이 상한이 없으면 코드 수명 5분 안에 LAN의 누구나 전체 공간을 훑어 만료되지 않는 기기 토큰을 가져간다.
 */
const PAIRING_CODE_MAX_FAILURES = 5;

/**
 * 실패한 시도만 이만큼 늦춘다. 성공은 늦추지 않는다. 맞게 입력한 사용자까지 기다리게 할 이유가 없다.
 * 상한 5회가 코드 하나의 훑기를 막고, 이 지연은 코드를 다시 받아 가며 반복하는 쪽의 속도를 떨어뜨린다.
 * 사람이 한 자리 잘못 눌렀을 때는 눈에 띄지 않는 길이다.
 */
const PAIRING_CODE_FAILURE_DELAY_MS = 300;

/** 기기 토큰은 저장되어 계속 쓰이므로 추측 시도를 무의미하게 만들 만큼 길어야 한다 */
const DEVICE_TOKEN_BYTES = 32;

interface PendingPairing {
  code: string;
  expiresAt: number;
  /** 이 코드로 틀린 횟수. 코드마다 따로 세므로 새로 발급하면 0에서 다시 시작한다 */
  failedAttempts: number;
}

/** 아직 교환되지 않은 페어링 코드. 앱이 꺼지면 같이 사라지는 것이 맞다 */
let pendingPairing: PendingPairing | null = null;

/** 연결된 기기 하나. `token`은 기기가 보관하고 데스크탑은 대조용으로만 가진다 */
export interface PairedDevice {
  /**
   * 기기를 가리키는 식별자. 연결 해제처럼 기기를 지목하는 조작에 쓴다.
   * 연결 시각으로 대신할 수 없다. 두 기기가 같은 밀리초에 연결되면 한쪽을 끊을 때 둘 다 지워진다.
   * 비밀값이 아니므로 화면과 목록에 그대로 실어 보낸다.
   */
  deviceId: string;
  token: string;
  deviceName: string;
  pairedAt: string;
}

/** 기기 식별자. 추측을 막을 필요가 없으므로 토큰보다 짧다 */
export function mintDeviceId(): string {
  return randomBytes(8).toString("hex");
}

/**
 * 앞자리 0이 있는 6자리 코드를 만든다.
 * `randomBytes`를 십진수로 접으면 값이 고르지 않아지므로 범위를 넘는 표본은 버리고 다시 뽑는다.
 */
function generatePairingCode(): string {
  const limit = 10 ** PAIRING_CODE_DIGITS;
  const ceiling = Math.floor(0xffffffff / limit) * limit;

  for (;;) {
    const sample = randomBytes(4).readUInt32BE(0);
    if (sample < ceiling) {
      return String(sample % limit).padStart(PAIRING_CODE_DIGITS, "0");
    }
  }
}

/**
 * 화면에 띄울 페어링 코드를 발급한다.
 * 코드는 한 번에 하나만 살아 있다. 여러 개를 동시에 열어 두면 어느 것이 화면의 값인지 사용자가 알 수 없다.
 */
export function issuePairingCode(now: number = Date.now()): { code: string; expiresAt: number } {
  pendingPairing = {
    code: generatePairingCode(),
    expiresAt: now + PAIRING_CODE_TTL_MS,
    failedAttempts: 0,
  };
  return { code: pendingPairing.code, expiresAt: pendingPairing.expiresAt };
}

/** 발급된 코드를 취소한다. 페어링 화면을 닫으면 코드도 죽어야 한다 */
export function revokePairingCode(): void {
  pendingPairing = null;
}

/** 화면에 떠 있는 코드. 없거나 만료됐으면 null */
export function readPendingPairingCode(now: number = Date.now()): string | null {
  if (!pendingPairing || pendingPairing.expiresAt <= now) {
    return null;
  }
  return pendingPairing.code;
}

/**
 * 코드를 확인하고 성공하면 소모한다.
 * 한 번 틀렸다고 곧바로 소모하지 않는 이유는, 사용자가 한 자리 잘못 눌렀다고 데스크탑에 다시 가서 새 코드를 받아야 하면
 * 못 쓸 물건이 되기 때문이다. 대신 그 여유를 [PAIRING_CODE_MAX_FAILURES]회로 묶고 실패한 시도만 늦춰,
 * 사람의 오타는 그대로 넘어가되 코드 공간을 훑는 쪽은 코드를 잃게 한다.
 */
export async function redeemPairingCode(
  submittedCode: string,
  now: number = Date.now(),
): Promise<boolean> {
  const expectedCode = readPendingPairingCode(now);
  if (expectedCode === null || !isEqualSecret(submittedCode, expectedCode)) {
    /** 지연보다 먼저 센다. 동시에 몰려온 시도가 전부 잠든 사이 코드가 살아 있으면 상한이 의미를 잃는다 */
    countFailedRedeem();
    await new Promise((resolve) => setTimeout(resolve, PAIRING_CODE_FAILURE_DELAY_MS));
    return false;
  }

  pendingPairing = null;
  return true;
}

/** 실패를 세고 상한에 닿으면 코드를 버린다. 버린 뒤에는 맞는 코드를 넣어도 통하지 않는 것이 의도다 */
function countFailedRedeem(): void {
  if (!pendingPairing) {
    return;
  }

  pendingPairing.failedAttempts += 1;
  if (pendingPairing.failedAttempts >= PAIRING_CODE_MAX_FAILURES) {
    pendingPairing = null;
  }
}

/** 기기가 저장하고 이후 모든 요청에 실어 보낼 토큰 */
export function mintDeviceToken(): string {
  return randomBytes(DEVICE_TOKEN_BYTES).toString("hex");
}

/**
 * 길이가 다르면 비교 자체가 성립하지 않으므로 먼저 거른다.
 * 길이가 같을 때만 `timingSafeEqual`을 써서 앞자리부터 몇 글자가 맞았는지가 응답 시간으로 새지 않게 한다.
 */
function isEqualSecret(candidate: string, expected: string): boolean {
  const candidateBytes = Buffer.from(candidate, "utf8");
  const expectedBytes = Buffer.from(expected, "utf8");

  if (candidateBytes.length !== expectedBytes.length) {
    return false;
  }

  return timingSafeEqual(candidateBytes, expectedBytes);
}

/** `Authorization: Bearer <token>` 헤더에서 토큰만 꺼낸다 */
export function parseBearerToken(authorizationHeader: string | undefined | null): string | null {
  if (!authorizationHeader) {
    return null;
  }

  const match = authorizationHeader.match(/^Bearer\s+(\S+)$/i);
  return match ? match[1] : null;
}

/** 저장된 기기 목록에서 토큰의 주인을 찾는다. 없으면 null */
export function findPairedDevice(devices: PairedDevice[], token: string | null): PairedDevice | null {
  if (!token) {
    return null;
  }

  return devices.find((device) => isEqualSecret(token, device.token)) ?? null;
}

/**
 * `app_settings`에 담긴 문자열을 기기 목록으로 되돌린다.
 * 저장값이 깨져 있으면 기기 전체를 잃는 대신 빈 목록으로 시작한다. 다시 페어링하면 복구되는 종류의 손실이다.
 */
export function parsePairedDevices(rawValue: string | null | undefined): PairedDevice[] {
  if (!rawValue) {
    return [];
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(rawValue);
  } catch {
    return [];
  }

  if (!Array.isArray(parsed)) {
    return [];
  }

  return parsed.filter(isPairedDevice);
}

function isPairedDevice(value: unknown): value is PairedDevice {
  const device = value as PairedDevice;
  return (
    typeof device?.deviceId === "string" &&
    device.deviceId.length > 0 &&
    typeof device?.token === "string" &&
    device.token.length > 0 &&
    typeof device?.deviceName === "string" &&
    typeof device?.pairedAt === "string"
  );
}

/** 기기 목록을 `app_settings`에 담을 문자열로 만든다 */
export function serializePairedDevices(devices: PairedDevice[]): string {
  return JSON.stringify(devices);
}
