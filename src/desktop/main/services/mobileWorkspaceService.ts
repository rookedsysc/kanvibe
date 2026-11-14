import { PaneLayoutType } from "@/entities/PaneLayoutConfig";
import { listTerminalTabs, renameTerminalTab, moveTerminalTab } from "./terminalTabService";
import { getAllPaneLayouts, savePaneLayout, deletePaneLayout } from "./paneLayoutService";
import { pasteImageToTerminal } from "@/desktop/main/terminalBridge";
import { getTaskSurfaces, writeToPane } from "./mobileBridgeService";
import { quoteShellArgument } from "@/lib/hostFileAccess";
import { MobileFeatureError, readMobileBoolean, readMobileEnum, readMobileObject, readMobileText, requireMobileProject, requireMobileTask } from "./mobileFeatureValidation";

const LAYOUT_PANE_COUNTS: Record<PaneLayoutType, number> = { single: 1, horizontal_2: 2, vertical_2: 2, left_right_tb: 3, left_tb_right: 3, quad: 4 };
const MAX_IMAGE_BYTES = 2 * 1024 * 1024;

async function requireMobileTab(taskId: string, tabId: string) {
  await requireMobileTask(taskId);
  const result = await listTerminalTabs(taskId);
  if (!result.ok) throw new MobileFeatureError(409, "터미널 탭을 조회하지 못했습니다");
  if (!result.tabs.some((tab) => tab.id === tabId)) throw new MobileFeatureError(404, "없는 터미널 탭입니다");
  return result.tabs;
}

export async function renameMobileTab(taskId: string, tabId: string, input: unknown) {
  const name = readMobileText(readMobileObject(input).name, "탭 이름", 128);
  await requireMobileTab(taskId, tabId);
  if (!(await renameTerminalTab(taskId, tabId, name)).ok) throw new MobileFeatureError(409, "탭 이름을 바꾸지 못했습니다");
  const refreshed = await listTerminalTabs(taskId);
  if (!refreshed.ok) throw new MobileFeatureError(409, "최신 탭 목록을 조회하지 못했습니다");
  return refreshed.tabs;
}

export async function moveMobileTab(taskId: string, tabId: string, input: unknown) {
  const index = readMobileObject(input).index;
  if (!Number.isSafeInteger(index) || (index as number) < 0) throw new MobileFeatureError(400, "탭 위치가 올바르지 않습니다");
  const tabs = await requireMobileTab(taskId, tabId);
  if ((index as number) >= tabs.length) throw new MobileFeatureError(400, "탭 위치가 목록을 벗어났습니다");
  if (!(await moveTerminalTab(taskId, tabId, index as number)).ok) throw new MobileFeatureError(409, "탭을 옮기지 못했습니다");
  const refreshed = await listTerminalTabs(taskId);
  if (!refreshed.ok) throw new MobileFeatureError(409, "최신 탭 목록을 조회하지 못했습니다");
  return refreshed.tabs;
}

export async function getMobilePaneLayouts() {
  return getAllPaneLayouts();
}

export async function saveMobilePaneLayout(input: unknown) {
  const body = readMobileObject(input);
  const layoutType = readMobileEnum(Object.values(PaneLayoutType), body.layoutType, "레이아웃");
  const isGlobal = readMobileBoolean(body.isGlobal, "글로벌 설정");
  const projectId = isGlobal ? undefined : readMobileText(body.projectId, "프로젝트", 128);
  if (isGlobal && body.projectId != null) throw new MobileFeatureError(400, "글로벌 설정에는 프로젝트를 지정할 수 없습니다");
  if (!Array.isArray(body.panes) || body.panes.length !== LAYOUT_PANE_COUNTS[layoutType]) throw new MobileFeatureError(400, "레이아웃의 pane 수가 맞지 않습니다");
  const panes = body.panes.map((value, index) => {
    const pane = readMobileObject(value);
    if (pane.position !== index || typeof pane.command !== "string" || pane.command.length > 4096 || pane.command.includes("\0")) throw new MobileFeatureError(400, "pane 명령이 올바르지 않습니다");
    return { position: index, command: pane.command };
  });
  if (projectId) await requireMobileProject(projectId);
  return savePaneLayout({ layoutType, isGlobal, projectId, panes });
}

export async function deleteMobilePaneLayout(id: string) {
  if (!(await deletePaneLayout(readMobileText(id, "레이아웃", 128)))) throw new MobileFeatureError(404, "없는 레이아웃입니다");
}

export async function pasteMobileRemoteImage(taskId: string, paneId: string, input: unknown) {
  const body = readMobileObject(input);
  const mimeType = readMobileEnum(["image/png", "image/jpeg"], body.mimeType, "이미지 형식");
  if (typeof body.base64 !== "string" || body.base64.length > Math.ceil(MAX_IMAGE_BYTES / 3) * 4 || (!/^[A-Za-z0-9+/]*={0,2}$/.test(body.base64) || body.base64.length % 4 !== 0)) {
    throw new MobileFeatureError(400, "이미지가 올바르지 않거나 너무 큽니다");
  }
  const image = Buffer.from(body.base64, "base64");
  const isPng = image.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]));
  const isJpeg = image.length >= 3 && image[0] === 255 && image[1] === 216 && image[2] === 255;
  if (!image.length || image.toString("base64") !== body.base64 || image.length > MAX_IMAGE_BYTES || (mimeType === "image/png" ? !isPng : !isJpeg)) throw new MobileFeatureError(400, "이미지 내용과 형식이 맞지 않습니다");
  const task = await requireMobileTask(taskId);
  if (!task.sshHost) throw new MobileFeatureError(409, "원격 터미널에서만 이미지를 붙여넣을 수 있습니다");
  const surfaces = await getTaskSurfaces(taskId);
  if (!surfaces.tabs.some((tab) => tab.panes.some((pane) => pane.id === paneId))) throw new MobileFeatureError(404, "없는 터미널 pane입니다");
  const result = await pasteImageToTerminal(taskId, `data:${mimeType};base64,${body.base64}`);
  if (!result.ok) throw new MobileFeatureError(409, "이미지를 전송하지 못했습니다");
  await writeToPane(taskId, paneId, quoteShellArgument(result.imagePath));
  return result.imagePath;
}
