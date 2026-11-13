# Composer capability reference checklist

Source review only for Orca/Paseo; no claim that their runtime was exercised.

| Capability | Orca reference | Paseo reference | KanVibe actual result |
|---|---|---|---|
| Image/file picker, attachment state | `NativeChatComposerPane.tsx`: `useNativeChatFileDrops`, `imageAttachments`, `pickAttachments` | `composer/index.tsx`: `useImageAttachmentPicker`, `useFilePicker` | NOT_IMPLEMENTED: no button/input/attachment state |
| Paste/drop intake and preview | `NativeChatFileDropSurface`, `handlePasteEvent` | `useFileDrop`, `onPasteImages` | NOT_IMPLEMENTED: renderer image events add no preview or payload |
| Image-aware submission | `sendPty({text,imagePaths})` | attachment service and submit format | NOT_IMPLEMENTED: prompt API accepts string; raw PTY contains only caption |
| Installed skill discovery/selection | `hooks/skills/discovery/useInstalledAgentSkills.ts` | `agent-skills/use-agent-skills.ts`, `selection-sheet.tsx` | NOT_IMPLEMENTED in app: no skill picker |
| `$`, slash and file mention UI | composer mention hooks and native input | composer skill/file mention paths | NOT_IMPLEMENTED in app: `$qa`, `/`, `@qa` produce no suggestions |
| Native CLI skill expansion | Native CLI path depends on provider | Native CLI path depends on provider | Real Codex expansion observed after an extra CLI Enter; initial Send failed |

KanVibe source: `src/desktop/renderer/components/PaneChatView.tsx`, `src/desktop/renderer/actions/paneChat.ts`, `src/desktop/main/services/paneChatService.ts`. Skills typed as literal strings are distinct from app-managed skill discovery and selection.
