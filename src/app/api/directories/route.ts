import { NextResponse } from "next/server";
import { homedir } from "os";
import path from "path";
import { execGit } from "@/lib/gitOperations";

/** 지정 디렉토리의 직속 하위 디렉토리 이름 목록을 반환한다 */
export async function GET(request: Request) {
  const { searchParams } = new URL(request.url);
  const parentPath = searchParams.get("path") || "~";
  const sshHost = searchParams.get("sshHost") || null;

  const resolvedPath = parentPath.startsWith("~")
    ? parentPath.replace(/^~/, homedir())
    : parentPath;

  try {
    const output = await execGit(
      `find "${resolvedPath}" -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sort`,
      sshHost
    );

    if (!output) {
      return NextResponse.json([]);
    }

    const dirs = output
      .split("\n")
      .filter(Boolean)
      .map((dir) => path.basename(dir))
      .filter((name) => !name.startsWith("."));

    return NextResponse.json(dirs);
  } catch {
    return NextResponse.json([]);
  }
}
