const { readJsonBody, writeJson } = require("./httpBody.js");
const { handleMobileAiRequest } = require("./mobileAiRoutes.js");
const { handleMobileDiagramRequest } = require("./mobileDiagramRoutes.js");

function invalid(message) {
  return Object.assign(new Error(message), { name: "MobileFeatureError", statusCode: 400 });
}

function pageInteger(value, fallback, minimum, maximum) {
  if (value === null) return fallback;
  if (!/^\d+$/.test(value) || !Number.isSafeInteger(Number(value)) || Number(value) < minimum || Number(value) > maximum) {
    throw invalid("페이지 값이 올바르지 않습니다");
  }
  return Number(value);
}

// 각 항목이 허용한 서비스만 호출한다. 경로나 본문의 문자열을 IPC 메서드 이름으로 사용하지 않는다.
const FEATURE_ROUTES = [
  { method: "GET", pattern: /^\/api\/mobile\/tasks\/search$/, run: async (bridge) => ({ tasks: await bridge.getMobileSearchableTasks() }) },
  { method: "GET", pattern: /^\/api\/mobile\/tasks\/done$/, run: async (bridge, ids, params) => bridge.getMobileDoneTasks(pageInteger(params.get("offset"), 0, 0, Number.MAX_SAFE_INTEGER), pageInteger(params.get("limit"), 20, 1, 100)) },
  { method: "PATCH", pattern: /^\/api\/mobile\/tasks\/([^/]+)\/details$/, body: true, run: async (bridge, ids, params, body) => ({ task: await bridge.updateMobileTaskDetails(ids[0], body) }) },
  { method: "POST", pattern: /^\/api\/mobile\/tasks\/([^/]+)\/branch$/, body: true, run: async (bridge, ids, params, body) => ({ task: await bridge.branchMobileTask(ids[0], body) }) },
  { method: "POST", pattern: /^\/api\/mobile\/tasks\/([^/]+)\/pr\/refresh$/, run: async (bridge, ids) => ({ prUrl: await bridge.refreshMobileTaskPr(ids[0]) }) },
  { method: "GET", pattern: /^\/api\/mobile\/hosts$/, run: async (bridge) => ({ hosts: await bridge.getMobileHosts() }) },
  { method: "PATCH", pattern: /^\/api\/mobile\/projects\/([^/]+)\/color$/, body: true, run: async (bridge, ids, params, body) => ({ color: await bridge.updateMobileProjectColor(ids[0], body) }) },
  { method: "GET", pattern: /^\/api\/mobile\/projects\/([^/]+)\/tasks$/, run: async (bridge, ids) => ({ tasks: await bridge.getMobileProjectTasks(ids[0]) }) },
  { method: "GET", pattern: /^\/api\/mobile\/projects\/([^/]+)\/task$/, run: async (bridge, ids, params) => ({ task: await bridge.getMobileProjectBranchTask(ids[0], params.get("branch")) }) },
  { method: "DELETE", pattern: /^\/api\/mobile\/projects\/([^/]+)$/, run: async (bridge, ids) => { await bridge.deleteMobileProject(ids[0]); return {}; } },
  { method: "GET", pattern: /^\/api\/mobile\/diff-stats$/, run: async (bridge) => ({ stats: await bridge.getMobileDiffStats() }) },
  { method: "GET", pattern: /^\/api\/mobile\/tasks\/([^/]+)\/diff$/, run: async (bridge, ids) => ({ files: await bridge.getMobileDiff(ids[0]) }) },
  { method: "GET", pattern: /^\/api\/mobile\/tasks\/([^/]+)\/files$/, run: async (bridge, ids, params) => bridge.getMobileDiffFile(ids[0], params.get("path")) },
  { method: "PUT", pattern: /^\/api\/mobile\/tasks\/([^/]+)\/files$/, body: true, maxBodyBytes: 6 * 1024 * 1024 + 1024, run: async (bridge, ids, params, body) => { await bridge.saveMobileDiffFile(ids[0], params.get("path"), body); return {}; } },
  { method: "PATCH", pattern: /^\/api\/mobile\/tasks\/([^/]+)\/tabs\/([^/]+)$/, body: true, run: async (bridge, ids, params, body) => ({ tabs: await bridge.renameMobileTab(ids[0], ids[1], body) }) },
  { method: "POST", pattern: /^\/api\/mobile\/tasks\/([^/]+)\/tabs\/([^/]+)\/move$/, body: true, run: async (bridge, ids, params, body) => ({ tabs: await bridge.moveMobileTab(ids[0], ids[1], body) }) },
  { method: "POST", pattern: /^\/api\/mobile\/tasks\/([^/]+)\/panes\/([^/]+)\/image$/, body: true, maxBodyBytes: 3 * 1024 * 1024, run: async (bridge, ids, params, body) => ({ remotePath: await bridge.pasteMobileRemoteImage(ids[0], ids[1], body) }) },
  { method: "GET", pattern: /^\/api\/mobile\/pane-layouts$/, run: async (bridge) => ({ layouts: await bridge.getMobilePaneLayouts() }) },
  { method: "PUT", pattern: /^\/api\/mobile\/pane-layouts$/, body: true, run: async (bridge, ids, params, body) => ({ layout: await bridge.saveMobilePaneLayout(body) }) },
  { method: "DELETE", pattern: /^\/api\/mobile\/pane-layouts\/([^/]+)$/, run: async (bridge, ids) => { await bridge.deleteMobilePaneLayout(ids[0]); return {}; } },
  { method: "GET", pattern: /^\/api\/mobile\/settings$/, run: async (bridge) => ({ settings: await bridge.getMobileSettings() }) },
  { method: "PATCH", pattern: /^\/api\/mobile\/settings$/, body: true, run: async (bridge, ids, params, body) => ({ settings: await bridge.updateMobileSettings(body) }) },
  { method: "GET", pattern: /^\/api\/mobile\/sessions\/change-targets$/, run: async (bridge, ids, params) => ({ targets: await bridge.getMobileSessionChangeTargets(params.get("sessionType")) }) },
  { method: "POST", pattern: /^\/api\/mobile\/sessions\/change-all$/, body: true, run: async (bridge, ids, params, body) => ({ result: await bridge.changeAllMobileTaskSessions(body) }) },
  { method: "POST", pattern: /^\/api\/mobile\/sync\/run$/, run: async (bridge) => ({ result: await bridge.runMobileSync() }) },
  { method: "GET", pattern: /^\/api\/mobile\/sync\/status$/, run: async (bridge) => ({ status: await bridge.getMobileSyncStatus() }) },
  { method: "POST", pattern: /^\/api\/mobile\/sync\/start$/, run: async (bridge) => ({ status: await bridge.startMobileSync() }) },
  { method: "GET", pattern: /^\/api\/mobile\/notifications\/([^/]+)\/review$/, run: async (bridge, ids) => ({ review: await bridge.getMobileSyncReview(ids[0]) }) },
  { method: "POST", pattern: /^\/api\/mobile\/notifications\/([^/]+)\/review\/delete$/, body: true, run: async (bridge, ids, params, body) => ({ deletedTaskIds: await bridge.deleteMobileSyncReviewTasks(ids[0], body) }) },
  { method: "POST", pattern: /^\/api\/mobile\/tasks\/([^/]+)\/notifications\/read$/, run: async (bridge, ids) => ({ updatedCount: await bridge.markMobileTaskNotificationsRead(ids[0]) }) },
];

async function handleMobileFeatureRequest(request, response, { bridge, requestUrl }) {
  const route = FEATURE_ROUTES.find((candidate) => candidate.method === request.method && candidate.pattern.test(requestUrl.pathname));
  if (!route) {
    if (await handleMobileDiagramRequest(request, response, { bridge, requestUrl })) return true;
    return handleMobileAiRequest(request, response, { bridge, requestUrl });
  }
  try {
    let ids;
    try {
      ids = requestUrl.pathname.match(route.pattern).slice(1).map(decodeURIComponent);
    } catch {
      throw invalid("잘못된 식별자입니다");
    }
    let body;
    if (route.body) {
      try {
        body = await readJsonBody(request, route.maxBodyBytes);
      } catch {
        throw invalid("잘못된 요청입니다");
      }
    }
    const result = await route.run(bridge, ids, requestUrl.searchParams, body);
    writeJson(response, 200, { success: true, ...result });
  } catch (error) {
    if (error?.name === "MobileFeatureError" && [400, 404, 409].includes(error.statusCode)) {
      writeJson(response, error.statusCode, { success: false, error: error.message });
    } else {
      console.error("[kanvibe] Mobile feature request failed:", error);
      writeJson(response, 500, { success: false, error: "서버 오류" });
    }
  }
  return true;
}

module.exports = { handleMobileFeatureRequest };
