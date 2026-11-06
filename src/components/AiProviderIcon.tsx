import { AI_SESSION_PROVIDER_LABELS, type AiSessionProvider } from "@/desktop/shared/contracts/lib-aiSessions-providers";
import claudeIconUrl from "@lobehub/icons-static-svg/icons/claude-color.svg";
import codexIconUrl from "@lobehub/icons-static-svg/icons/codex-color.svg";
import geminiIconUrl from "@lobehub/icons-static-svg/icons/gemini-color.svg";
import openCodeIconUrl from "@lobehub/icons-static-svg/icons/opencode.svg";

export type AiProviderIconName = AiSessionProvider;

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
} as const satisfies Partial<Record<AiProviderIconName, { displayName: string; src: string }>>;

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
  const meta = (AI_PROVIDER_ICON_META as Partial<Record<AiProviderIconName, { displayName: string; src: string }>>)[provider];
  if (!meta) {
    const label = AI_SESSION_PROVIDER_LABELS[provider];
    return (
      <span aria-hidden="true" className={className} data-icon-name={label}
        data-icon-source="initials" data-testid={testId} title={label}>
        <span className="inline-flex items-center justify-center text-[10px] font-bold" style={{ width: size, height: size }}>
          {label.slice(0, 2).toUpperCase()}
        </span>
      </span>
    );
  }

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
