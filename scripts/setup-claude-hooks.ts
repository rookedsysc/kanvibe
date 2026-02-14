#!/usr/bin/env tsx

/**
 * Claude Code Hooks CLI 설정 스크립트.
 * 지정된 폴더의 .claude/settings.json에 KanVibe hooks를 추가한다.
 *
 * @example
 * npx tsx scripts/setup-claude-hooks.ts ./my-project --project my-app --url http://localhost:4885
 */

import { setupClaudeHooks } from "../src/lib/claudeHooksSetup";
import path from "path";

interface CliArgs {
  targetPath: string;
  projectName: string;
  kanvibeUrl: string;
}

function parseArgs(argv: string[]): CliArgs {
  const args = argv.slice(2);

  if (args.length === 0 || args.includes("--help") || args.includes("-h")) {
    console.log(`Usage: npx tsx scripts/setup-claude-hooks.ts <folder> [options]

Options:
  --project <name>  KanVibe 프로젝트 이름 (기본값: 폴더명)
  --url <url>       KanVibe 서버 URL (기본값: http://localhost:4885)
  -h, --help        도움말 표시

Examples:
  npx tsx scripts/setup-claude-hooks.ts ./my-project
  npx tsx scripts/setup-claude-hooks.ts /path/to/repo --project my-app --url http://localhost:3000`);
    process.exit(0);
  }

  const targetPath = path.resolve(args[0]);
  let projectName = path.basename(targetPath);
  let kanvibeUrl = `http://localhost:${process.env.PORT || 4885}`;

  for (let i = 1; i < args.length; i++) {
    if (args[i] === "--project" && args[i + 1]) {
      projectName = args[++i];
    } else if (args[i] === "--url" && args[i + 1]) {
      kanvibeUrl = args[++i];
    }
  }

  return { targetPath, projectName, kanvibeUrl };
}

async function main() {
  const { targetPath, projectName, kanvibeUrl } = parseArgs(process.argv);

  console.log(`KanVibe Claude Hooks 설정`);
  console.log(`  대상: ${targetPath}`);
  console.log(`  프로젝트: ${projectName}`);
  console.log(`  서버: ${kanvibeUrl}`);
  console.log();

  try {
    await setupClaudeHooks(targetPath, projectName, kanvibeUrl);
    console.log("설정 완료:");
    console.log(`  - ${targetPath}/.claude/hooks/kanvibe-prompt-hook.sh`);
    console.log(`  - ${targetPath}/.claude/hooks/kanvibe-stop-hook.sh`);
    console.log(`  - ${targetPath}/.claude/settings.json (hooks 추가)`);
  } catch (error) {
    console.error("설정 실패:", error instanceof Error ? error.message : error);
    process.exit(1);
  }
}

main();
