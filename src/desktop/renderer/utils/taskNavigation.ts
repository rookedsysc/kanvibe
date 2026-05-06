import { localizeHref, redirect } from "@/desktop/renderer/navigation";
import { openInternalRouteInNewWindow } from "@/desktop/renderer/utils/windowOpen";

interface NavigateToTaskDetailOptions {
  currentLocale?: string;
  navigate?: (href: string) => void;
  openInNewWindow?: boolean;
}

interface TaskNavigationClickEvent {
  defaultPrevented: boolean;
  button: number;
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
}

export function shouldHandleTaskNavigationClick(event: TaskNavigationClickEvent) {
  return (
    !event.defaultPrevented
    && event.button === 0
    && !event.altKey
    && !event.ctrlKey
    && !event.metaKey
  );
}

export function getTaskDetailHref(taskId: string, currentLocale?: string) {
  return localizeHref(`/task/${taskId}`, currentLocale);
}

export async function focusExistingTaskDetailWindow(taskId: string, currentLocale?: string) {
  const focusExistingInternalRoute = window.kanvibeDesktop?.focusExistingInternalRoute;
  if (!focusExistingInternalRoute) {
    return false;
  }

  try {
    return await focusExistingInternalRoute(getTaskDetailHref(taskId, currentLocale));
  } catch {
    return false;
  }
}

export async function navigateToTaskDetail(
  taskId: string,
  {
    currentLocale,
    navigate = redirect,
    openInNewWindow = false,
  }: NavigateToTaskDetailOptions = {},
) {
  const href = getTaskDetailHref(taskId, currentLocale);

  if (openInNewWindow) {
    openInternalRouteInNewWindow(href);
    return;
  }

  const didFocusExistingWindow = await focusExistingTaskDetailWindow(taskId, currentLocale);
  if (!didFocusExistingWindow) {
    navigate(href);
  }
}
