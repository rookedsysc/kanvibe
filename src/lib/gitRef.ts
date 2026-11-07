/**
 * 네트워크에서 받은 브랜치 이름으로 받아들일 모양.
 *
 * 이 값은 `createWorktreeWithSession`에서 `git ... -b "<이름>"` 꼴로 셸 명령 문자열에 끼워지고,
 * worktree 경로에도 그대로 들어간다. 큰따옴표 안이라도 셸은 `$(...)`와 백틱을 펼치므로,
 * 데스크탑 화면에서만 오던 값과 달리 모바일·CLI처럼 네트워크에서 오는 값은 여기서 막지 않으면
 * 요청자가 이 머신에서 임의의 명령을 돌리고 프로젝트 밖 경로를 짚을 수 있다.
 * git이 받아 주는 이름 중 셸과 경로에 뜻이 없는 문자만 통과시킨다.
 */
const SAFE_GIT_REF_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._/-]*$/;

export function isSafeGitRef(value: string): boolean {
  return SAFE_GIT_REF_PATTERN.test(value) && !value.includes("..");
}
