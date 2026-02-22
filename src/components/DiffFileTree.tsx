"use client";

import { useState, useMemo } from "react";
import type { DiffFile } from "@/app/actions/diff";

interface DiffFileTreeProps {
  files: DiffFile[];
  selectedFile: string | null;
  onSelectFile: (path: string) => void;
  viewedFiles?: Set<string>;
}

/** 파일 상태별 약어 색상 */
const STATUS_COLORS: Record<DiffFile["status"], string> = {
  added: "text-green-600",
  modified: "text-yellow-600",
  deleted: "text-red-500",
  renamed: "text-blue-500",
};

/** 파일 상태별 약어 표시 */
const STATUS_LABELS: Record<DiffFile["status"], string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  renamed: "R",
};

/** GitHub 스타일 변경량 도트를 생성한다. 최대 5개의 도트로 추가/삭제 비율을 시각화한다 */
function ChangeDots({ additions, deletions }: { additions: number; deletions: number }) {
  const total = additions + deletions;
  if (total === 0) return null;

  const MAX_DOTS = 5;
  const addDots = total > 0 ? Math.round((additions / total) * MAX_DOTS) : 0;
  const delDots = MAX_DOTS - addDots;

  return (
    <span className="flex gap-px ml-1.5 shrink-0">
      {Array.from({ length: addDots }, (_, i) => (
        <span key={`a${i}`} className="w-1.5 h-1.5 rounded-full bg-green-500" />
      ))}
      {Array.from({ length: delDots }, (_, i) => (
        <span key={`d${i}`} className="w-1.5 h-1.5 rounded-full bg-red-500" />
      ))}
    </span>
  );
}

function FolderIcon({ open }: { open: boolean }) {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" className="shrink-0 text-blue-500">
      {open ? (
        <path
          d="M1.5 3.5C1.5 2.67 2.17 2 3 2h3.38a1.5 1.5 0 011.12.5L8.5 3.5H13c.83 0 1.5.67 1.5 1.5v7c0 .83-.67 1.5-1.5 1.5H3c-.83 0-1.5-.67-1.5-1.5v-8.5z"
          fill="currentColor"
          opacity="0.2"
          stroke="currentColor"
          strokeWidth="1"
        />
      ) : (
        <path
          d="M1.5 3.5C1.5 2.67 2.17 2 3 2h3.38a1.5 1.5 0 011.12.5L8.5 3.5H13c.83 0 1.5.67 1.5 1.5v7c0 .83-.67 1.5-1.5 1.5H3c-.83 0-1.5-.67-1.5-1.5v-8.5z"
          fill="currentColor"
          opacity="0.15"
          stroke="currentColor"
          strokeWidth="1"
        />
      )}
    </svg>
  );
}

function FileIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" className="shrink-0 text-text-muted">
      <path
        d="M4 1.5h5.5L13 5v8.5a1 1 0 01-1 1H4a1 1 0 01-1-1v-12a1 1 0 011-1z"
        stroke="currentColor"
        strokeWidth="1"
        fill="none"
      />
      <path d="M9.5 1.5V5H13" stroke="currentColor" strokeWidth="1" fill="none" />
    </svg>
  );
}

interface TreeNode {
  name: string;
  fullPath: string;
  isFile: boolean;
  status?: DiffFile["status"];
  additions?: number;
  deletions?: number;
  children: Map<string, TreeNode>;
}

/** 파일 경로 목록을 트리 구조로 변환한다 */
function buildFileTree(files: DiffFile[]): TreeNode {
  const root: TreeNode = {
    name: "",
    fullPath: "",
    isFile: false,
    children: new Map(),
  };

  for (const file of files) {
    const segments = file.path.split("/");
    let current = root;

    for (let i = 0; i < segments.length; i++) {
      const segment = segments[i];
      const isLastSegment = i === segments.length - 1;

      if (!current.children.has(segment)) {
        current.children.set(segment, {
          name: segment,
          fullPath: segments.slice(0, i + 1).join("/"),
          isFile: isLastSegment,
          status: isLastSegment ? file.status : undefined,
          additions: isLastSegment ? file.additions : undefined,
          deletions: isLastSegment ? file.deletions : undefined,
          children: new Map(),
        });
      }

      current = current.children.get(segment)!;
    }
  }

  return root;
}

/** 폴더 노드를 재귀적으로 렌더링한다 */
function TreeNodeItem({
  node,
  depth,
  selectedFile,
  onSelectFile,
  expandedFolders,
  toggleFolder,
  viewedFiles,
}: {
  node: TreeNode;
  depth: number;
  selectedFile: string | null;
  onSelectFile: (path: string) => void;
  expandedFolders: Set<string>;
  toggleFolder: (path: string) => void;
  viewedFiles?: Set<string>;
}) {
  const isExpanded = expandedFolders.has(node.fullPath);
  const sortedChildren = Array.from(node.children.values()).sort((a, b) => {
    if (a.isFile !== b.isFile) return a.isFile ? 1 : -1;
    return a.name.localeCompare(b.name);
  });

  if (node.isFile) {
    const isSelected = selectedFile === node.fullPath;
    const isViewed = viewedFiles?.has(node.fullPath) ?? false;
    return (
      <button
        onClick={() => onSelectFile(node.fullPath)}
        className={`w-full flex items-center gap-1.5 px-2 py-1 text-xs text-left rounded transition-colors group ${
          isSelected
            ? "bg-brand-subtle text-text-brand font-medium"
            : isViewed
              ? "text-text-muted hover:bg-bg-page hover:text-text-secondary"
              : "text-text-secondary hover:bg-bg-page hover:text-text-primary"
        }`}
        style={{ paddingLeft: `${depth * 14 + 8}px` }}
      >
        {isViewed ? (
          <svg width="16" height="16" viewBox="0 0 16 16" fill="none" className="shrink-0 text-green-500">
            <path d="M3 8l3.5 3.5L13 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        ) : (
          <FileIcon />
        )}
        <span className={`truncate flex-1 ${isViewed ? "line-through opacity-60" : ""}`}>{node.name}</span>
        {!isViewed && <ChangeDots additions={node.additions ?? 0} deletions={node.deletions ?? 0} />}
        {node.status && (
          <span
            className={`shrink-0 font-mono text-[10px] font-bold ${isViewed ? "opacity-40" : ""} ${STATUS_COLORS[node.status]}`}
          >
            {STATUS_LABELS[node.status]}
          </span>
        )}
      </button>
    );
  }

  return (
    <div>
      <button
        onClick={() => toggleFolder(node.fullPath)}
        className="w-full flex items-center gap-1.5 px-2 py-1 text-xs text-text-primary hover:bg-bg-page rounded transition-colors"
        style={{ paddingLeft: `${depth * 14 + 8}px` }}
      >
        {/* 토글 화살표 */}
        <svg
          width="12"
          height="12"
          viewBox="0 0 12 12"
          fill="none"
          className={`shrink-0 transition-transform ${isExpanded ? "rotate-90" : ""}`}
        >
          <path d="M4.5 2.5L8 6L4.5 9.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
        <FolderIcon open={isExpanded} />
        <span className="truncate font-medium">{node.name}</span>
      </button>
      {isExpanded &&
        sortedChildren.map((child) => (
          <TreeNodeItem
            key={child.fullPath}
            node={child}
            depth={depth + 1}
            selectedFile={selectedFile}
            onSelectFile={onSelectFile}
            expandedFolders={expandedFolders}
            toggleFolder={toggleFolder}
            viewedFiles={viewedFiles}
          />
        ))}
    </div>
  );
}

export default function DiffFileTree({
  files,
  selectedFile,
  onSelectFile,
  viewedFiles,
}: DiffFileTreeProps) {
  const tree = useMemo(() => buildFileTree(files), [files]);

  /** 모든 폴더를 기본적으로 펼쳐서 시작한다 */
  const [expandedFolders, setExpandedFolders] = useState<Set<string>>(() => {
    const folders = new Set<string>();
    function collectFolders(node: TreeNode) {
      if (!node.isFile && node.fullPath) {
        folders.add(node.fullPath);
      }
      for (const child of node.children.values()) {
        collectFolders(child);
      }
    }
    collectFolders(tree);
    return folders;
  });

  const toggleFolder = (path: string) => {
    setExpandedFolders((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  const sortedChildren = Array.from(tree.children.values()).sort((a, b) => {
    if (a.isFile !== b.isFile) return a.isFile ? 1 : -1;
    return a.name.localeCompare(b.name);
  });

  return (
    <div className="flex flex-col gap-0.5">
      {sortedChildren.map((child) => (
        <TreeNodeItem
          key={child.fullPath}
          node={child}
          depth={0}
          selectedFile={selectedFile}
          onSelectFile={onSelectFile}
          expandedFolders={expandedFolders}
          toggleFolder={toggleFolder}
          viewedFiles={viewedFiles}
        />
      ))}
    </div>
  );
}
