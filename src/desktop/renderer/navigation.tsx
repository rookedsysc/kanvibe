import { useMemo, type AnchorHTMLAttributes, type PropsWithChildren } from "react";
import { Link as RouterLink, useLocation, useNavigate } from "react-router-dom";
import { DEFAULT_LOCALE, SUPPORTED_LOCALES, getSafeLocale, type SupportedLocale } from "@/desktop/renderer/utils/locales";
import { triggerDesktopRefresh } from "@/desktop/renderer/utils/refresh";

function getRefreshScope(pathname: string) {
  if (pathname.includes("/task/") && pathname.endsWith("/diff")) {
    return "diff" as const;
  }

  if (pathname.includes("/task/")) {
    return "task-detail" as const;
  }

  if (pathname.endsWith("/pane-layout")) {
    return "pane-layout" as const;
  }

  return "board" as const;
}

function getLocaleFromPathname(pathname: string): SupportedLocale {
  const firstSegment = pathname.split("/").filter(Boolean)[0];
  return getSafeLocale(firstSegment);
}

function canNavigateBack() {
  const historyIndex = window.history.state?.idx;
  return typeof historyIndex === "number" && historyIndex > 0;
}

export function localizeHref(href: string, currentLocale?: string): string {
  if (!href.startsWith("/")) {
    return href;
  }

  const locale = getSafeLocale(currentLocale);
  const firstSegment = href.split("/").filter(Boolean)[0];
  if (SUPPORTED_LOCALES.includes(firstSegment as SupportedLocale)) {
    return href;
  }

  if (href === "/") {
    return `/${locale}`;
  }

  return `/${locale}${href}`;
}

interface LinkProps extends PropsWithChildren<Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "href">> {
  href: string;
  prefetch?: boolean;
}

export function Link({ href, children, prefetch: _prefetch, ...props }: LinkProps) {
  const location = useLocation();
  const localizedHref = localizeHref(href, getLocaleFromPathname(location.pathname));
  return (
    <RouterLink to={localizedHref} {...props}>
      {children}
    </RouterLink>
  );
}

export function usePathname() {
  return useLocation().pathname;
}

export function useRouter() {
  const navigate = useNavigate();
  const location = useLocation();
  const currentLocale = getLocaleFromPathname(location.pathname);

  return useMemo(
    () => ({
      back: () => {
        if (canNavigateBack()) {
          navigate(-1);
          return;
        }

        navigate(localizeHref("/", currentLocale), { replace: true });
      },
      forward: () => navigate(1),
      push: (href: string) => navigate(localizeHref(href, currentLocale)),
      replace: (href: string) => navigate(localizeHref(href, currentLocale), { replace: true }),
      refresh: () => triggerDesktopRefresh(getRefreshScope(location.pathname)),
    }),
    [currentLocale, location.pathname, navigate],
  );
}

export function redirect(input: { href: string; locale?: string } | string) {
  const href = typeof input === "string" ? input : input.href;
  const locale = typeof input === "string" ? DEFAULT_LOCALE : input.locale;
  window.location.hash = `#${localizeHref(href, locale)}`;
}
