// 새 worktree가 main이 아니라 source 태스크의 브랜치 commit에서 시작했는지 확인한다.
const response = http.get(`${PROBE_API_FROM_HOST}/command-ran?marker=${MARKER}&branch=${encodeURIComponent(BRANCH_NAME)}&timeoutMs=15000`);
if (response.status !== 200) {
  throw new Error(`생성된 태스크 probe 실패: HTTP ${response.status}`);
}
const state = json(response.body);
if (!state.ran || !state.sourceBranchInherited) {
  throw new Error(`새 태스크에서 명령 실행 또는 source 브랜치 상속 실패: ${response.body}`);
}
output.sourceBranchInherited = state.sourceBranchInherited;
