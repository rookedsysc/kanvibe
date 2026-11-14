import { useCallback, useEffect, useState } from "react";
import { useTranslations } from "next-intl";
import { AiProviderIcon } from "@/components/AiProviderIcon";
import AiAccountLoginTerminal from "@/desktop/renderer/components/AiAccountLoginTerminal";
import AiUsagePanel from "@/desktop/renderer/components/AiUsagePanel";
import InlineLoadError from "@/components/InlineLoadError";
import { Link } from "@/desktop/renderer/navigation";
import { useTrackedAction } from "@/desktop/renderer/hooks/useTrackedAction";
import {
  addAiAccount,
  listAiAccounts,
  removeAiAccount,
} from "@/desktop/renderer/actions/aiAccounts";
import type { AiAccountSummary } from "@/desktop/main/services/aiAccountService";
import type { AiUsageProvider } from "@/lib/aiUsage/types";

const AI_USAGE_PROVIDERS: AiUsageProvider[] = ["claude", "codex", "gemini"];

/** 로그인 화면이 열려 있는 계정. 계정 루트가 세션을 가리킨다 */
interface ActiveLoginSession {
  provider: AiUsageProvider;
  accountRoot: string;
}

interface ProviderAccountGroup {
  provider: AiUsageProvider;
  accounts: AiAccountSummary[];
}

function groupAccountsByProvider(accounts: AiAccountSummary[]): ProviderAccountGroup[] {
  return AI_USAGE_PROVIDERS.map((provider) => ({
    provider,
    accounts: accounts.filter((account) => account.provider === provider),
  }));
}

function AccountRow({
  account,
  isLoginOpen,
  isRemoving,
  isRemoveDisabled,
  removeFailureReason,
  onLogin,
  onRemove,
  onRetryRemove,
}: {
  account: AiAccountSummary;
  isLoginOpen: boolean;
  isRemoving: boolean;
  /** 다른 계정을 지우는 중이면 겹쳐 지우지 않도록 막는다 */
  isRemoveDisabled: boolean;
  removeFailureReason: string | null;
  onLogin: () => void;
  onRemove: () => void;
  onRetryRemove: () => void;
}) {
  const t = useTranslations("aiAccounts");

  return (
    <div
      className="flex flex-wrap items-center gap-2 rounded-md border border-border-subtle bg-bg-page px-3 py-2"
      data-testid="ai-account-row"
      aria-busy={isRemoving || undefined}
    >
      <div className="min-w-0 flex-1">
        <p
          className="truncate text-sm text-text-primary"
          title={account.label}
          data-testid="ai-account-label"
        >
          {account.label}
        </p>
        <p className="mt-0.5 text-[11px] text-text-muted">
          {account.isLoggedIn ? t("loggedIn") : t("loggedOut")}
        </p>
      </div>

      {account.planName ? (
        <span className="shrink-0 rounded bg-bg-surface px-1.5 py-0.5 text-[10px] uppercase text-text-muted">
          {account.planName}
        </span>
      ) : null}

      <button
        type="button"
        onClick={onLogin}
        disabled={isLoginOpen}
        className="shrink-0 rounded-md bg-brand-primary px-2 py-1 text-[11px] text-white transition-colors hover:bg-brand-hover disabled:opacity-50"
        data-testid="ai-account-login"
      >
        {account.isLoggedIn ? t("relogin") : t("login")}
      </button>

      {account.isRemovable ? (
        <button
          type="button"
          onClick={onRemove}
          disabled={isRemoveDisabled}
          className="shrink-0 rounded-md border border-border-default px-2 py-1 text-[11px] text-text-secondary transition-colors hover:text-text-primary disabled:opacity-50"
          data-testid="ai-account-remove"
        >
          {isRemoving ? t("removing") : t("remove")}
        </button>
      ) : null}

      {removeFailureReason ? (
        <InlineLoadError
          className="basis-full"
          message={t("removeFailed", { reason: removeFailureReason })}
          onRetry={onRetryRemove}
        />
      ) : null}
    </div>
  );
}

function AddAccountForm({
  provider,
  onAdded,
}: {
  provider: AiUsageProvider;
  onAdded: (accountRoot: string) => void;
}) {
  const t = useTranslations("aiAccounts");
  const [accountName, setAccountName] = useState("");
  const [errorKey, setErrorKey] = useState<string | null>(null);
  /** 응답 전에 다시 눌러 같은 계정을 두 번 만들지 않게 막고, 요청 자체가 실패하면 사유를 남긴다 */
  const accountAddition = useTrackedAction(async (trimmedName: string) => {
    const result = await addAiAccount(provider, trimmedName);
    if (result.outcome !== "ok" || !result.accountRoot) {
      setErrorKey(result.outcome);
      return;
    }

    setErrorKey(null);
    setAccountName("");
    onAdded(result.accountRoot);
  });

  function submitAccountName() {
    setErrorKey(null);
    void accountAddition.run(accountName.trim());
  }

  return (
    <div className="mt-2">
      <div className="flex items-center gap-2">
        <input
          type="text"
          value={accountName}
          onChange={(event) => setAccountName(event.target.value)}
          placeholder={t("accountNamePlaceholder")}
          className="min-w-0 flex-1 rounded-md border border-border-default bg-bg-page px-2 py-1 text-xs text-text-primary"
          data-testid="ai-account-name-input"
        />
        <button
          type="button"
          onClick={submitAccountName}
          disabled={!accountName.trim() || accountAddition.isPending}
          aria-busy={accountAddition.isPending || undefined}
          className="shrink-0 rounded-md border border-border-default px-2 py-1 text-[11px] text-text-secondary transition-colors hover:text-text-primary disabled:opacity-50"
          data-testid="ai-account-add"
        >
          {accountAddition.isPending ? t("adding") : t("addAccount")}
        </button>
      </div>
      {errorKey ? (
        <p className="mt-1 text-[11px] text-status-error">{t(`errors.${errorKey}`)}</p>
      ) : null}
      {accountAddition.error ? (
        <InlineLoadError
          className="mt-1"
          message={t("addFailed", { reason: accountAddition.error })}
          onRetry={() => void accountAddition.retry()}
        />
      ) : null}
    </div>
  );
}

export default function AiAccountsRoute() {
  const t = useTranslations("aiAccounts");
  const tc = useTranslations("common");
  const [accounts, setAccounts] = useState<AiAccountSummary[] | null>(null);
  const [activeLogin, setActiveLogin] = useState<ActiveLoginSession | null>(null);
  /** 목록 조회 실패. 계정이 없는 것과 구분해, 빈 목록 대신 사유와 재시도를 보인다 */
  const [loadFailureReason, setLoadFailureReason] = useState<string | null>(null);
  const [removalTargetRoot, setRemovalTargetRoot] = useState<string | null>(null);

  const loadAccounts = useCallback(async () => {
    try {
      setAccounts(await listAiAccounts());
      setLoadFailureReason(null);
    } catch (error) {
      console.error("Failed to load AI accounts:", error);
      setLoadFailureReason(error instanceof Error ? error.message : String(error));
    }
  }, []);

  useEffect(() => {
    document.title = "AI Accounts";
  }, []);

  useEffect(() => {
    void loadAccounts();
  }, [loadAccounts]);

  const finishLogin = useCallback(() => {
    setActiveLogin(null);
    void loadAccounts();
  }, [loadAccounts]);

  /** 한 번에 한 계정만 지운다. 실패하면 그 계정 행에 사유와 재시도를 남긴다 */
  const accountRemoval = useTrackedAction(async (account: AiAccountSummary) => {
    setRemovalTargetRoot(account.accountRoot);
    await removeAiAccount(account.provider, account.accountRoot);
    await loadAccounts();
  });

  function removeAccount(account: AiAccountSummary) {
    if (!window.confirm(t("removeConfirm", { label: account.label }))) {
      return;
    }

    void accountRemoval.run(account);
  }

  if (!accounts) {
    return (
      <div className="flex min-h-screen items-center justify-center bg-bg-page p-6 text-text-muted">
        {loadFailureReason ? (
          <InlineLoadError
            className="w-full max-w-md"
            message={tc("loadFailed", { reason: loadFailureReason })}
            onRetry={() => void loadAccounts()}
          />
        ) : (
          "Loading..."
        )}
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-bg-page p-6" data-testid="ai-accounts-route">
      <div className="mx-auto max-w-2xl space-y-8">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-bold text-text-primary">{t("title")}</h1>
            <p className="mt-1 text-sm text-text-secondary">{t("description")}</p>
          </div>
          <Link href="/settings" className="text-sm text-brand-primary hover:underline">
            {t("backToSettings")}
          </Link>
        </div>

        {loadFailureReason ? (
          <InlineLoadError
            message={tc("refreshFailed", { reason: loadFailureReason })}
            onRetry={() => void loadAccounts()}
          />
        ) : null}

        {activeLogin ? (
          <section className="rounded-xl border border-border-default bg-bg-surface p-5">
            <div className="mb-2 flex items-center justify-between">
              <h2 className="text-sm font-semibold text-text-primary">{t("loginInProgress")}</h2>
              <button
                type="button"
                onClick={finishLogin}
                className="rounded-md border border-border-default px-2 py-1 text-[11px] text-text-secondary hover:text-text-primary"
                data-testid="ai-account-login-close"
              >
                {t("closeLogin")}
              </button>
            </div>
            <p className="mb-3 text-xs text-text-muted">{t("loginHint")}</p>
            <AiAccountLoginTerminal
              provider={activeLogin.provider}
              accountRoot={activeLogin.accountRoot}
              onExit={finishLogin}
            />
          </section>
        ) : null}

        <section className="rounded-xl border border-border-default bg-bg-surface p-5">
          <h2 className="mb-3 text-sm font-semibold text-text-primary">{t("usageSection")}</h2>
          {/* 구독과 남은 사용량을 한 화면에서 보려면 사용량 패널을 그대로 세우는 편이 낫다 */}
          <AiUsagePanel isOpen />
        </section>

        {groupAccountsByProvider(accounts).map((group) => (
          <section
            key={group.provider}
            className="rounded-xl border border-border-default bg-bg-surface p-5"
            data-testid={`ai-accounts-provider-${group.provider}`}
          >
            <header className="mb-3 flex items-center gap-2">
              <AiProviderIcon provider={group.provider} size={16} />
              <h2 className="text-sm font-semibold capitalize text-text-primary">
                {group.provider}
              </h2>
            </header>

            {group.accounts.length === 0 ? (
              <p className="text-xs text-text-muted">{t("noAccounts")}</p>
            ) : (
              <div className="space-y-2">
                {group.accounts.map((account) => (
                  <AccountRow
                    key={account.accountRoot}
                    account={account}
                    isLoginOpen={activeLogin?.accountRoot === account.accountRoot}
                    isRemoving={accountRemoval.pendingArgs?.[0].accountRoot === account.accountRoot}
                    isRemoveDisabled={accountRemoval.isPending}
                    removeFailureReason={
                      removalTargetRoot === account.accountRoot ? accountRemoval.error : null
                    }
                    onLogin={() => setActiveLogin({
                      provider: account.provider,
                      accountRoot: account.accountRoot,
                    })}
                    onRemove={() => removeAccount(account)}
                    onRetryRemove={() => void accountRemoval.retry()}
                  />
                ))}
              </div>
            )}

            <AddAccountForm
              provider={group.provider}
              onAdded={(accountRoot) => {
                void loadAccounts();
                setActiveLogin({ provider: group.provider, accountRoot });
              }}
            />
          </section>
        ))}
    </div>
  </div>
);
}
