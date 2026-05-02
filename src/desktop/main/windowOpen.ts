export type WindowOpenAction<T> =
  | {
      type: "external";
    }
  | {
      type: "open-internal";
      route: string;
      outlivesOpener: true;
    }
  | {
      type: "focus-existing";
      route: string;
      existingWindow: T;
    };

interface ResolveWindowOpenActionOptions<T> {
  targetUrl: string;
  rendererDevUrl: string | null;
  openWindows: readonly T[];
  getWindowUrl: (window: T) => string;
  excludeWindow?: T | null;
}

interface ResolveNavigationTargetWindowOptions<T> {
  preferredWindow?: T | null;
  targetUrl: string;
  rendererDevUrl: string | null;
  openWindows: readonly T[];
  getWindowUrl: (window: T) => string;
}

interface NotificationActivationLike {
  taskId?: string | null;
  action?: {
    type?: string;
  } | null;
}

export function shouldKeepCurrentRouteForNotificationActivation(
  notification: NotificationActivationLike | null | undefined,
): boolean {
  return notification?.action?.type === "background-sync-review";
}

function normalizeInternalRoute(route: string | null | undefined): string | null {
  if (!route) {
    return null;
  }

  if (route.startsWith("/#/")) {
    return route.slice(2);
  }

  if (route.startsWith("#/")) {
    return route.slice(1);
  }

  if (route.startsWith("/")) {
    return route;
  }

  return `/${route}`;
}

function extractRouteFromParsedUrl(parsedUrl: URL, rendererDevUrl: string | null): string | null {
  const hashRoute = normalizeInternalRoute(parsedUrl.hash);
  if (parsedUrl.protocol === "file:") {
    return hashRoute;
  }

  if (!rendererDevUrl) {
    return null;
  }

  let rendererOrigin: string;
  try {
    rendererOrigin = new URL(rendererDevUrl).origin;
  } catch {
    return null;
  }

  if (parsedUrl.origin !== rendererOrigin) {
    return null;
  }

  return hashRoute ?? normalizeInternalRoute(parsedUrl.pathname);
}

export function extractInternalRoute(targetUrl: string, rendererDevUrl: string | null): string | null {
  if (!targetUrl) {
    return null;
  }

  if (targetUrl.startsWith("/")) {
    return normalizeInternalRoute(targetUrl);
  }

  if (targetUrl.startsWith("#")) {
    return normalizeInternalRoute(targetUrl);
  }

  try {
    return extractRouteFromParsedUrl(new URL(targetUrl), rendererDevUrl);
  } catch {
    return null;
  }
}

function findExistingInternalWindow<T>({
  targetUrl,
  rendererDevUrl,
  openWindows,
  getWindowUrl,
  excludeWindow = null,
}: ResolveWindowOpenActionOptions<T>): {
  route: string | null;
  existingWindow: T | null;
} {
  const route = extractInternalRoute(targetUrl, rendererDevUrl);
  if (!route) {
    return {
      route: null,
      existingWindow: null,
    };
  }

  const existingWindow = openWindows.find((window) => {
    if (window === excludeWindow) {
      return false;
    }

    return extractInternalRoute(getWindowUrl(window), rendererDevUrl) === route;
  }) ?? null;

  return {
    route,
    existingWindow,
  };
}

export function resolveNavigationTargetWindow<T>({
  preferredWindow = null,
  targetUrl,
  rendererDevUrl,
  openWindows,
  getWindowUrl,
}: ResolveNavigationTargetWindowOptions<T>): T | null {
  const { existingWindow } = findExistingInternalWindow({
    targetUrl,
    rendererDevUrl,
    openWindows,
    getWindowUrl,
  });

  return existingWindow ?? preferredWindow;
}

export function resolveExistingNavigationTargetWindow<T>({
  targetUrl,
  rendererDevUrl,
  openWindows,
  getWindowUrl,
}: ResolveNavigationTargetWindowOptions<T>): T | null {
  const { existingWindow } = findExistingInternalWindow({
    targetUrl,
    rendererDevUrl,
    openWindows,
    getWindowUrl,
  });

  return existingWindow;
}

export function resolveWindowOpenAction<T>({
  targetUrl,
  rendererDevUrl,
  openWindows,
  getWindowUrl,
  excludeWindow = null,
}: ResolveWindowOpenActionOptions<T>): WindowOpenAction<T> {
  const { route, existingWindow } = findExistingInternalWindow({
    targetUrl,
    rendererDevUrl,
    openWindows,
    getWindowUrl,
    excludeWindow,
  });

  if (!route) {
    return { type: "external" };
  }

  if (existingWindow) {
    return {
      type: "focus-existing",
      route,
      existingWindow,
    };
  }

  return {
    type: "open-internal",
    route,
    outlivesOpener: true,
  };
}
