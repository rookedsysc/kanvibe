import { execGit } from "@/lib/gitOperations";
import { quoteShellArgument } from "@/lib/hostFileAccess";

/** GitHub avatar 응답 크기. 보드 카드와 목록에서 쓰는 최대 크기의 2배(retina)로 맞춘다 */
const ICON_PIXEL_SIZE = 64;
const ICON_FETCH_TIMEOUT_MS = 5_000;
/** data URL로 DB에 저장하므로 비정상적으로 큰 응답은 저장하지 않는다 */
const MAX_ICON_BYTES = 512 * 1024;
const ALLOWED_ICON_CONTENT_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
/** GitHub 소유자(사용자/조직) 이름 규칙 */
const GITHUB_OWNER_PATTERN = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$/;
/**
 * 같은 owner의 저장소를 연달아 등록할 때 아바타를 반복해서 내려받지 않도록 캐싱한다.
 * 실패도 캐싱해 오프라인 상태에서 저장소마다 타임아웃을 다시 기다리지 않게 한다.
 */
const OWNER_ICON_CACHE_TTL_MS = 10 * 60 * 1000;
const ownerIconCache = new Map<string, { expiresAt: number; iconDataUrl: Promise<string | null> }>();

export function clearGitHubOwnerIconCache(): void {
  ownerIconCache.clear();
}

export interface GitHubRepositoryReference {
  owner: string;
  repository: string;
}

function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * git remote URL에서 GitHub owner/repository를 뽑아낸다.
 * `https://github.com/o/r.git`, `git@github.com:o/r.git`, `ssh://git@github.com/o/r` 형식을 지원한다.
 * GitHub이 아니거나 형식이 다르면 null을 반환한다.
 */
export function parseGitHubRepositoryReference(remoteUrl: string): GitHubRepositoryReference | null {
  const normalizedRemoteUrl = remoteUrl.trim();
  if (!normalizedRemoteUrl) {
    return null;
  }

  const match = normalizedRemoteUrl.match(
    /^(?:https?:\/\/|ssh:\/\/)?(?:[^@/]+@)?github\.com[:/]+([^/]+)\/([^/]+?)(?:\.git)?\/?$/i,
  );
  if (!match) {
    return null;
  }

  const [, owner, repository] = match;
  if (!GITHUB_OWNER_PATTERN.test(owner)) {
    return null;
  }

  return { owner, repository };
}

/** 저장소의 origin remote를 읽어 GitHub owner/repository를 확인한다 */
export async function resolveGitHubRepositoryReference(
  repoPath: string,
  sshHost?: string | null,
): Promise<GitHubRepositoryReference | null> {
  try {
    const remoteUrl = await execGit(
      `git -C ${quoteShellArgument(repoPath)} config --get remote.origin.url`,
      sshHost,
    );
    return parseGitHubRepositoryReference(remoteUrl);
  } catch {
    /** remote가 없는 로컬 전용 저장소는 아이콘 없이 동작한다 */
    return null;
  }
}

/**
 * GitHub 사용자/조직 아바타를 내려받아 data URL로 변환한다.
 * 네트워크 실패, 비정상 응답, 과도한 크기는 모두 아이콘 없음으로 처리한다.
 */
export async function fetchGitHubOwnerIconDataUrl(owner: string): Promise<string | null> {
  if (!GITHUB_OWNER_PATTERN.test(owner)) {
    return null;
  }

  const cached = ownerIconCache.get(owner);
  if (cached && cached.expiresAt > Date.now()) {
    return cached.iconDataUrl;
  }

  const iconDataUrl = downloadGitHubOwnerIconDataUrl(owner);
  ownerIconCache.set(owner, { expiresAt: Date.now() + OWNER_ICON_CACHE_TTL_MS, iconDataUrl });
  return iconDataUrl;
}

async function downloadGitHubOwnerIconDataUrl(owner: string): Promise<string | null> {
  try {
    const response = await fetch(
      `https://github.com/${encodeURIComponent(owner)}.png?size=${ICON_PIXEL_SIZE}`,
      { signal: AbortSignal.timeout(ICON_FETCH_TIMEOUT_MS) },
    );
    if (!response.ok) {
      return null;
    }

    const contentType = (response.headers.get("content-type") ?? "").split(";")[0].trim().toLowerCase();
    if (!ALLOWED_ICON_CONTENT_TYPES.includes(contentType)) {
      return null;
    }

    const iconBytes = Buffer.from(await response.arrayBuffer());
    if (iconBytes.byteLength === 0 || iconBytes.byteLength > MAX_ICON_BYTES) {
      return null;
    }

    return `data:${contentType};base64,${iconBytes.toString("base64")}`;
  } catch (error) {
    console.warn("[project-icon] GitHub 아이콘 다운로드 실패", {
      owner,
      error: getErrorMessage(error),
    });
    return null;
  }
}

/**
 * 프로젝트 저장소의 GitHub 아이콘을 확보한다.
 * GitHub 저장소가 아니거나 아이콘을 받지 못하면 null을 반환해 아이콘 없이 등록되게 한다.
 */
export async function resolveProjectIconDataUrl(
  repoPath: string,
  sshHost?: string | null,
): Promise<string | null> {
  const repositoryReference = await resolveGitHubRepositoryReference(repoPath, sshHost);
  if (!repositoryReference) {
    return null;
  }

  return fetchGitHubOwnerIconDataUrl(repositoryReference.owner);
}
