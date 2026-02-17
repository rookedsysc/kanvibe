import createMiddleware from "next-intl/middleware";
import { NextResponse } from "next/server";
import type { NextRequest } from "next/server";
import { routing } from "./i18n/routing";

const intlMiddleware = createMiddleware(routing);

const PUBLIC_PATHS = ["/login"];

/**
 * locale 라우팅과 인증을 결합한 프록시.
 * next-intl이 locale을 처리한 뒤, 공개 경로가 아니면 세션 쿠키를 확인한다.
 */
export function proxy(request: NextRequest) {
  const { pathname } = request.nextUrl;

  const isApiPath = pathname.startsWith("/api/");
  if (isApiPath) {
    return NextResponse.next();
  }

  const pathnameWithoutLocale = pathname.replace(
    /^\/(ko|en|zh)/,
    ""
  ) || "/";

  const isPublicPath = PUBLIC_PATHS.some((path) =>
    pathnameWithoutLocale.startsWith(path)
  );

  if (!isPublicPath) {
    const sessionToken = request.cookies.get("kanvibe_session")?.value;
    if (!sessionToken) {
      const locale = pathname.match(/^\/(ko|en|zh)/)?.[1] || routing.defaultLocale;
      return NextResponse.redirect(
        new URL(`/${locale}/login`, request.url)
      );
    }
  }

  return intlMiddleware(request);
}

export const config = {
  matcher: ["/((?!_next/static|_next/image|favicon.ico|icon|sw.js|api/hooks|.*\\.(?:png|jpg|jpeg|svg|gif|ico|webp)$).*)"],
};
