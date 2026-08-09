#!/usr/bin/env python3
"""
Captures unstaged git changes and formats them for Serena memory.
Outputs markdown-formatted diff data to stdout.
"""

import subprocess
import sys
from datetime import datetime


def run_git_command(cmd):
    """Run git command and return output."""
    try:
        result = subprocess.run(
            cmd,
            shell=True,
            capture_output=True,
            text=True,
            check=True
        )
        return result.stdout.strip()
    except subprocess.CalledProcessError as e:
        return f"Error: {e.stderr}"


def get_unstaged_changes():
    """Capture all unstaged changes with context."""

    # Get current branch
    branch = run_git_command("git rev-parse --abbrev-ref HEAD")

    # Get unstaged file list
    unstaged_files = run_git_command("git diff --name-only")

    # Get detailed diff with stats
    diff_stat = run_git_command("git diff --stat")

    # Get full diff content
    diff_content = run_git_command("git diff")

    # Get untracked files
    untracked_files = run_git_command("git ls-files --others --exclude-standard")

    # Build structured output
    data = {
        "timestamp": datetime.now().isoformat(),
        "branch": branch,
        "unstaged_files": unstaged_files.split('\n') if unstaged_files else [],
        "untracked_files": untracked_files.split('\n') if untracked_files else [],
        "diff_stat": diff_stat,
        "diff_content": diff_content,
        "total_files_changed": len(unstaged_files.split('\n')) if unstaged_files else 0
    }

    return data


def format_for_memory(data):
    """Format captured data for Serena memory (.md format)."""
    output = f"""# Session Changes

Branch: `{data['branch']}`
Timestamp: {data['timestamp']}

## Summary
- Changed files: {data['total_files_changed']}
- Untracked files: {len(data['untracked_files'])}

## Diff Statistics
```
{data['diff_stat']}
```

## Unstaged Files
{chr(10).join(f"- {f}" for f in data['unstaged_files']) if data['unstaged_files'] else "No unstaged changes"}

## Untracked Files
{chr(10).join(f"- {f}" for f in data['untracked_files']) if data['untracked_files'] else "No untracked files"}

## Full Diff
```diff
{data['diff_content']}
```
"""
    return output


def main():
    """Main execution - capture changes and output markdown."""
    data = get_unstaged_changes()
    print(format_for_memory(data))


if __name__ == "__main__":
    main()
