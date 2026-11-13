const response = http.get(`${PROBE_API_FROM_HOST}/state`);
if (response.status !== 200) throw new Error(`Registration probe failed: ${response.status} ${response.body}`);
const state = json(response.body);
output.browsePath = state.fixture.browsePath;
output.projectName = state.fixture.projectName;
if (String(EXPECT_REGISTERED) === "true") {
  const projects = state.board.projects.filter((project) => project.repoPath === state.fixture.repoPath);
  if (projects.length !== 1) throw new Error(`Expected exactly one persisted project: ${response.body}`);
  const tasks = Object.keys(state.board.tasks).reduce((all, key) => all.concat(state.board.tasks[key]), []);
  if (!tasks.some((task) => task.projectId === projects[0].id && task.branchName === "main")) {
    throw new Error(`Registered project's root task missing: ${response.body}`);
  }
}
