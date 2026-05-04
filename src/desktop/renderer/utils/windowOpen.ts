function normalizeInternalRoute(route: string) {
  if (route.startsWith("#/")) {
    return route.slice(1);
  }

  if (route.startsWith("/")) {
    return route;
  }

  return `/${route}`;
}

export function getInternalRouteWindowUrl(route: string) {
  const targetUrl = new URL(window.location.href);
  targetUrl.hash = normalizeInternalRoute(route);
  return targetUrl.href;
}

export function openInternalRouteInNewWindow(route: string) {
  window.open(getInternalRouteWindowUrl(route), "_blank", "noopener,noreferrer");
}
