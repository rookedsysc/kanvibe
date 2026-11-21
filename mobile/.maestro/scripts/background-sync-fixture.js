const health = http.get(`${SYNC_PROBE_URL}/health`);
if (health.status !== 200) throw new Error(`Sync fixture unavailable: ${health.status}`);
const identity = json(health.body);
if (output.syncRunId && output.syncRunId !== identity.runId) throw new Error("Sync fixture identity changed");
output.syncRunId = identity.runId;
const action = String(SYNC_ACTION);
if (["pairing", "baseline", "start", "release", "assert"].indexOf(action) < 0) throw new Error(`Unknown sync action: ${action}`);
if (action === "pairing") {
  const pairing = http.post(`${SYNC_PROBE_URL}/pairing`, { body: "" });
  if (pairing.status !== 200) throw new Error(`Sync pairing failed: ${pairing.status}`);
  const value = json(pairing.body);
  output.syncPairingCode = value.code;
  output.syncMobilePort = String(value.mobilePort);
}
if (action === "start" || action === "release") {
  const response = http.post(`${SYNC_PROBE_URL}/${action}`, { body: "" });
  if (response.status !== 200) throw new Error(`Sync ${action} failed: ${response.status} ${response.body}`);
}
const response = http.get(`${SYNC_PROBE_URL}/status`);
if (response.status !== 200) throw new Error(`Sync status failed: ${response.status}`);
const state = json(response.body);
if (state.runId !== output.syncRunId) throw new Error("Status came from a different sync fixture");
if (action === "baseline") {
  output.syncBaselineWorktrees = state.counts.worktrees;
  output.syncBaselinePullRequests = state.counts.pullRequests;
  output.syncBaselinePulls = state.counts.pulls;
}
if (action === "assert" || action === "start") {
  if (state.status.phase !== String(SYNC_PHASE)) throw new Error(`Expected ${SYNC_PHASE}, received ${state.status.phase}`);
  const expected = [Number(SYNC_WORKTREES), Number(SYNC_PULL_REQUESTS), Number(SYNC_PULLS)];
  const actual = [state.counts.worktrees - output.syncBaselineWorktrees, state.counts.pullRequests - output.syncBaselinePullRequests, state.counts.pulls - output.syncBaselinePulls];
  if (String(actual) !== String(expected)) throw new Error(`Sync stages duplicated: expected ${expected}, received ${actual}`);
}
