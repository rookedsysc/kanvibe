import path from "path";

/** 같은 PC 안에서 이름과 상위 폴더까지 모두 겹쳐 표시 이름을 구분할 수 없을 때 알리는 문구 */
export const DUPLICATE_PROJECT_NAME_ERROR = "같은 PC에 이름과 상위 폴더가 모두 같은 프로젝트가 이미 있습니다.";

/**
 * 같은 PC 안에서 이름이 겹칠 때만 상위 폴더를 덧붙여 프로젝트 표시 이름을 구분한다.
 * 다른 PC의 같은 이름 프로젝트는 UI가 sshHost로 구분해 보여주므로 이름을 바꾸지 않는다.
 * 상위 폴더까지 같아 구분할 수 없으면 null을 반환해 호출부가 등록을 막게 한다.
 *
 * @param preferredName 사용자가 지정했거나 경로에서 뽑은 우선 이름
 * @param repoPath 저장소 경로. 이름이 겹칠 때 상위 폴더를 뽑는 근거가 된다
 * @param namesOnSameHost 같은 PC에 이미 등록된 프로젝트 이름. 확정한 이름이 여기에 추가된다
 * @returns 확정한 표시 이름, 구분할 수 없으면 null
 */
export function resolveUniqueProjectName(
  preferredName: string,
  repoPath: string,
  namesOnSameHost: Set<string>,
): string | null {
  const baseName = path.basename(repoPath);
  const parentName = path.basename(path.dirname(repoPath));
  const combinedName = `${parentName}/${baseName}`;
  const candidates = [preferredName];

  if (baseName && baseName !== preferredName) {
    candidates.push(baseName);
  }

  if (combinedName && combinedName !== preferredName) {
    candidates.push(combinedName);
  }

  for (const candidate of candidates) {
    if (namesOnSameHost.has(candidate)) {
      continue;
    }

    namesOnSameHost.add(candidate);
    return candidate;
  }

  return null;
}
