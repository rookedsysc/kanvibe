import { cookies } from "next/headers";
import { randomUUID, createHmac, timingSafeEqual } from "crypto";

const SESSION_COOKIE = "kanvibe_session";

/**
 * HMAC 서명 비밀키.
 * 서버 사이드 세션 스토어 없이 쿠키 자체를 검증할 수 있어
 * Turbopack 워커와 메인 프로세스 간 상태 공유 문제를 우회한다.
 */
function getSecret(): string {
  return process.env.KANVIBE_PASSWORD || "kanvibe-default-secret";
}

/** 토큰에 HMAC 서명을 붙여 반환한다 */
function signToken(token: string): string {
  const sig = createHmac("sha256", getSecret()).update(token).digest("hex");
  return `${token}.${sig}`;
}

/** 서명된 토큰을 검증하고 원본 토큰을 반환한다. 유효하지 않으면 null */
function verifySignedToken(signedToken: string): string | null {
  const dotIndex = signedToken.lastIndexOf(".");
  if (dotIndex === -1) return null;

  const token = signedToken.slice(0, dotIndex);
  const providedSig = signedToken.slice(dotIndex + 1);
  const expectedSig = createHmac("sha256", getSecret()).update(token).digest("hex");

  if (providedSig.length !== expectedSig.length) return null;

  const isValid = timingSafeEqual(
    Buffer.from(providedSig),
    Buffer.from(expectedSig)
  );

  return isValid ? token : null;
}

/** env에 설정된 자격 증명과 비교하여 로그인을 검증한다 */
export function validateCredentials(username: string, password: string): boolean {
  return (
    username === process.env.KANVIBE_USER &&
    password === process.env.KANVIBE_PASSWORD
  );
}

/** 새 세션 토큰을 생성하고 서명된 쿠키에 설정한다 */
export async function createSession(): Promise<string> {
  const token = randomUUID();
  const signed = signToken(token);

  const cookieStore = await cookies();
  cookieStore.set(SESSION_COOKIE, signed, {
    httpOnly: true,
    secure: process.env.NODE_ENV === "production",
    sameSite: "lax",
    path: "/",
    maxAge: 60 * 60 * 24 * 7,
  });

  return token;
}

/** 쿠키의 세션 토큰이 유효한지 확인한다 */
export async function validateSession(): Promise<boolean> {
  const cookieStore = await cookies();
  const signed = cookieStore.get(SESSION_COOKIE)?.value;
  if (!signed) return false;
  return verifySignedToken(signed) !== null;
}

/** 쿠키 문자열에서 서명된 세션 토큰을 추출하여 검증한다 (WebSocket용) */
export function validateSessionFromCookie(cookieHeader: string): boolean {
  const match = cookieHeader.match(new RegExp(`${SESSION_COOKIE}=([^;]+)`));
  if (!match) return false;
  return verifySignedToken(decodeURIComponent(match[1])) !== null;
}
/** 세션 쿠키를 제거한다 */
export async function destroySession(): Promise<void> {
  const cookieStore = await cookies();
  cookieStore.delete(SESSION_COOKIE);
}
