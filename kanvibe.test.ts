/** @vitest-environment node */
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

function runNodeInstall(script: string) {
  const functions = ["install_node_apt", "version_gte", "check_single_dep"].map((name) => {
    const match = script.match(new RegExp(`^${name}\\(\\) \\{\\n[\\s\\S]*?^\\}`, "m"));
    if (!match) throw new Error(`missing shell function ${name}`);
    return match[0];
  }).join("\n");
  return spawnSync("bash", ["-c", `
${functions}
apt-get() { printf 'apt-get %s\\n' "$*"; }
curl() { printf 'curl %s\\n' "$*" >&2; }
bash() { cat > /dev/null; }
msg() { printf '%s' "$*"; }
SUDO=''
install_node_apt
node() { printf 'v24.15.0\\n'; }
MISSING_REQUIRED=()
check_single_dep Node.js node required 24.0.0 ''
[ "\${#MISSING_REQUIRED[@]}" -eq 0 ] || exit 91
node() { printf 'v22.0.0\\n'; }
check_single_dep Node.js node required 24.0.0 '' && exit 92
[ "\${#MISSING_REQUIRED[@]}" -eq 1 ]
`], { encoding: "utf8" });
}

describe("Linux Node 설치 채널", () => {
  it("필수 Node 24를 설치하고 설치 후 24는 허용하며 22는 거부한다", () => {
    const result = runNodeInstall(readFileSync(new URL("./kanvibe.sh", import.meta.url), "utf8"));
    expect(result.status).toBe(0);
    expect(result.stderr).toContain("https://deb.nodesource.com/setup_24.x");
    expect(result.stdout).toContain("apt-get install -y nodejs");
  });

});
