import claudeIconUrl from "@lobehub/icons-static-svg/icons/claude-color.svg";
import codexIconUrl from "@lobehub/icons-static-svg/icons/codex-color.svg";
import geminiIconUrl from "@lobehub/icons-static-svg/icons/gemini-color.svg";
import openCodeIconUrl from "@lobehub/icons-static-svg/icons/opencode.svg";

export type AiProviderIconName = "claude" | "gemini" | "codex" | "opencode";

const AI_PROVIDER_ICON_META = {
  claude: {
    displayName: "Claude",
    src: claudeIconUrl,
  },
  gemini: {
    displayName: "Gemini",
    src: geminiIconUrl,
  },
  codex: {
    displayName: "Codex",
    src: codexIconUrl,
  },
  opencode: {
    displayName: "OpenCode",
    src: openCodeIconUrl,
  },
} as const satisfies Record<AiProviderIconName, { displayName: string; src: string }>;

type AiProviderIconProps = {
  provider: AiProviderIconName;
  testId?: string;
  className?: string;
  imageClassName?: string;
  size?: number;
};

export function AiProviderIcon({
  provider,
  testId,
  className,
  imageClassName,
  size = 18,
}: AiProviderIconProps) {
  const meta = AI_PROVIDER_ICON_META[provider];

  return (
    <span
      aria-hidden="true"
      className={className}
      data-icon-name={meta.displayName}
      data-icon-source="lobehub-icons"
      data-testid={testId}
      title={meta.displayName}
    >
      <img
        alt=""
        className={imageClassName ?? "block object-contain"}
        draggable={false}
        height={size}
        src={meta.src}
        width={size}
      />
    </span>
  );
}
