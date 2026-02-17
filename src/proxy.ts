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

  // Service Worker 스크립트는 인증 없이 접근 가능해야 함
  if (pathname === "/sw.js") {
    return NextResponse.next();
  }

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
  matcher: [
    // 모든 경로를 매칭하되, 다음 파일들은 제외:
    // - 정적 자산 (_next/static, _next/image)
    // - 파비콘 및 아이콘
    // - Service Worker 스크립트 (sw.js)
    // - API 훅
    // - 이미지 파일들 (.png, .jpg, 등)
    "/((?!_next/static|_next/image|favicon\\.ico|icon|sw\\.js|api/hooks|.*\\.(?:png|jpg|jpeg|svg|gif|ico|webp)$).*)"
  ],
};
