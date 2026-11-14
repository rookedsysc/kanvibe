import { useCallback, useEffect, useId, useRef, useState } from "react";
import { useTranslations } from "next-intl";
import * as sessions from "@/desktop/renderer/actions/herdrSessions";
import { navigateToTaskDetail } from "@/desktop/renderer/utils/taskNavigation";
import {
  HerdrOperationPhase,
  HerdrSessionMode,
  type HerdrSessionOperation,
  type HerdrSessionPlan,
  type HerdrSessionScan,
} from "@/desktop/shared/herdrSessions";

const FIELD = "w-full rounded-md border border-border-default bg-bg-page px-3 py-2 text-sm text-text-primary disabled:opacity-50";
const BUTTON_BASE = "rounded-md border border-border-default px-3 py-2 text-sm disabled:cursor-not-allowed disabled:opacity-50 focus-visible:outline focus-visible:outline-2 focus-visible:outline-brand-primary";
const BUTTON = `${BUTTON_BASE} text-text-primary hover:bg-bg-page`;
const PRIMARY_BUTTON = `${BUTTON_BASE} bg-brand-primary text-text-inverse hover:bg-brand-hover`;
const ACTIVE_PHASES = [HerdrOperationPhase.APPLYING, HerdrOperationPhase.VERIFYING, HerdrOperationPhase.COMMITTED];
const KNOWN_PHASES: string[] = Object.values(HerdrOperationPhase);

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function HerdrSessionManager({ sshHosts }: { sshHosts: string[] }) {
  const t = useTranslations("herdrSessions");
  const id = useId();
  const [host, setHost] = useState("");
  const [scan, setScan] = useState<HerdrSessionScan | null>(null);
  const [sessionName, setSessionName] = useState("");
  const [workspaceId, setWorkspaceId] = useState("");
  const [taskId, setTaskId] = useState("");
  const [plan, setPlan] = useState<HerdrSessionPlan | null>(null);
  const [operation, setOperation] = useState<HerdrSessionOperation | null>(null);
  const [operations, setOperations] = useState<HerdrSessionOperation[]>([]);
  const [pending, setPending] = useState<"scan" | "plan" | "execute" | null>(null);
  const [controlPending, setControlPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const generation = useRef(0);
  const historyGeneration = useRef(0);
  const mounted = useRef(false);
  const execution = useRef<{ planId: string; operationId: string } | null>(null);
  const mutating = pending === "execute" || controlPending;
  const selectedSession = scan?.sessions.find((session) => session.name === sessionName);
  const selectedWorkspace = selectedSession?.workspaces.find((workspace) => workspace.workspaceId === workspaceId);
  const canPreview = Boolean(scan && sessionName && workspaceId && taskId && !pending && !controlPending);

  const reloadOperations = useCallback(async () => {
    const requestId = ++historyGeneration.current;
    setHistoryLoading(true);
    try {
      const result = await sessions.listOperations();
      if (!mounted.current || requestId !== historyGeneration.current) return;
      if (result.ok) {
        setOperations(result.value);
        setOperation((current) => {
          const saved = result.value.find((record) => record.operationId === current?.operationId);
          return saved && current && saved.updatedAt >= current.updatedAt ? saved : current;
        });
        setHistoryError(null);
      } else setHistoryError(result.error.message);
    } catch (cause) {
      if (mounted.current && requestId === historyGeneration.current) setHistoryError(errorMessage(cause));
    } finally {
      if (mounted.current && requestId === historyGeneration.current) setHistoryLoading(false);
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    const requests = generation;
    const historyRequests = historyGeneration;
    void reloadOperations();
    const onVisible = () => { if (document.visibilityState === "visible") void reloadOperations(); };
    const onFocus = () => { void reloadOperations(); };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      mounted.current = false;
      requests.current++;
      historyRequests.current++;
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [reloadOperations]);

  const operationId = operation?.operationId;
  const operationPhase = operation?.phase;
  useEffect(() => {
    if (!operationId || !operationPhase || !ACTIVE_PHASES.includes(operationPhase)) return;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const result = await sessions.getOperation({ operationId });
        if (cancelled) return;
        if (!result.ok) {
          setError(result.error.message);
        } else {
          setOperation(result.value);
          if (!ACTIVE_PHASES.includes(result.value.phase)) {
            void reloadOperations();
            return;
          }
        }
      } catch (cause) {
        if (!cancelled) setError(errorMessage(cause));
      }
      if (!cancelled) timer = setTimeout(poll, 1500);
    };
    timer = setTimeout(poll, 1500);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [operationId, operationPhase, reloadOperations]);

  function invalidatePlan() {
    generation.current++;
    setPlan(null);
    setOperation(null);
    execution.current = null;
    setPending(null);
    setError(null);
  }

  async function scanHost() {
    invalidatePlan();
    setScan(null);
    setSessionName("");
    setWorkspaceId("");
    setTaskId("");
    const requestId = generation.current;
    setPending("scan");
    try {
      const result = await sessions.scanSessions({ sshHost: host || null });
      if (!mounted.current || generation.current !== requestId) return;
      if (result.ok) setScan(result.value);
      else setError(result.error.message);
    } catch (cause) {
      if (mounted.current && generation.current === requestId) setError(errorMessage(cause));
    } finally {
      if (mounted.current && generation.current === requestId) setPending(null);
    }
  }

  async function preview(mode: HerdrSessionMode) {
    if (!scan || !canPreview) return;
    invalidatePlan();
    const requestId = generation.current;
    setPending("plan");
    try {
      const result = await sessions.createPlan({ scanId: scan.scanId, taskId, sourceSession: sessionName, sourceWorkspaceId: workspaceId, mode });
      if (!mounted.current || generation.current !== requestId) return;
      if (result.ok) setPlan(result.value);
      else setError(result.error.message);
    } catch (cause) {
      if (mounted.current && generation.current === requestId) setError(errorMessage(cause));
    } finally {
      if (mounted.current && generation.current === requestId) setPending(null);
    }
  }

  async function execute() {
    if (!plan || plan.eligibility !== "ready" || pending || controlPending) return;
    if (execution.current?.planId !== plan.planId) execution.current = { planId: plan.planId, operationId: crypto.randomUUID() };
    const requestId = generation.current;
    setPending("execute");
    setError(null);
    const now = new Date().toISOString();
    setOperation({ operationId: execution.current.operationId, plan, phase: HerdrOperationPhase.APPLYING,
      targetWorkspaceId: null, backupLocation: null, error: null, createdAt: now, updatedAt: now });
    try {
      const result = await sessions.executePlan({ planId: plan.planId, expectedFingerprint: plan.fingerprint, operationId: execution.current.operationId });
      if (!mounted.current || generation.current !== requestId) return;
      if (result.ok) setOperation(result.value);
      else { setOperation(null); setError(result.error.message); }
      void reloadOperations();
    } catch (cause) {
      // Retain this operation ID: the server may have committed before transport failed.
      if (mounted.current && generation.current === requestId) { setOperation(null); setError(errorMessage(cause)); }
    } finally {
      if (mounted.current && generation.current === requestId) setPending(null);
    }
  }

  async function control(action: "cancel" | "revertBinding") {
    if (!operation || controlPending || (pending && pending !== "execute")) return;
    setControlPending(true);
    setError(null);
    try {
      const result = await sessions.controlOperation({ operationId: operation.operationId, action });
      if (!mounted.current) return;
      if (result.ok) setOperation(result.value);
      else setError(result.error.message);
      void reloadOperations();
    } catch (cause) {
      if (mounted.current) setError(errorMessage(cause));
    } finally {
      if (mounted.current) setControlPending(false);
    }
  }

  function blockerLabel(blocker: string) {
    const code = blocker.split(":", 1)[0].trim();
    return /^[A-Z_]+$/.test(code) && t.has(`blockers.${code}`) ? t(`blockers.${code}`) : blocker;
  }

  function phaseLabel(phase: string) {
    return KNOWN_PHASES.includes(phase) ? t(`phases.${phase}`) : t("phases.attention");
  }

  return (
    <section id="herdr-sessions" className="border-b border-border-default p-4" aria-labelledby={`${id}-title`}>
      <h3 id={`${id}-title`} className="mb-3 text-xs uppercase tracking-wide text-text-muted">{t("title")}</h3>
      <p className="mb-2 text-sm text-text-secondary">{t("description")}</p>
      <p className="mb-4 text-xs text-text-muted">{t("safetyNotice")}</p>
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="space-y-1 text-sm text-text-secondary" htmlFor={`${id}-host`}>
          <span>{t("host")}</span>
          <select id={`${id}-host`} className={FIELD} value={host} disabled={mutating} onChange={(event) => {
            invalidatePlan(); setHost(event.target.value); setScan(null); setSessionName(""); setWorkspaceId(""); setTaskId("");
          }}>
            <option value="">{t("local")}</option>
            {sshHosts.map((sshHost) => <option key={sshHost} value={sshHost}>{sshHost}</option>)}
          </select>
        </label>
        <div className="flex items-end"><button type="button" className={BUTTON} disabled={Boolean(pending) || controlPending} onClick={() => void scanHost()}>{pending === "scan" ? t("scanning") : t("scan")}</button></div>
        <label className="space-y-1 text-sm text-text-secondary" htmlFor={`${id}-session`}>
          <span>{t("session")}</span>
          <select id={`${id}-session`} className={FIELD} value={sessionName} disabled={!scan || mutating} onChange={(event) => { invalidatePlan(); setSessionName(event.target.value); setWorkspaceId(""); }}>
            <option value="">{t("choose")}</option>
            {scan?.sessions.map((session) => <option key={session.name} value={session.name}>{session.name} · {session.running ? t("running") : t("stopped")}</option>)}
          </select>
        </label>
        <label className="space-y-1 text-sm text-text-secondary" htmlFor={`${id}-space`}>
          <span>{t("space")}</span>
          <select id={`${id}-space`} className={FIELD} value={workspaceId} disabled={!selectedSession || mutating} onChange={(event) => { invalidatePlan(); setWorkspaceId(event.target.value); }}>
            <option value="">{t("choose")}</option>
            {selectedSession?.workspaces.map((workspace) => <option key={workspace.workspaceId} value={workspace.workspaceId}>{workspace.label} · {workspace.cwd} · {t("tabs", { count: workspace.tabCount })}</option>)}
          </select>
        </label>
        <label className="space-y-1 text-sm text-text-secondary sm:col-span-2" htmlFor={`${id}-task`}>
          <span>{t("task")}</span>
          <select id={`${id}-task`} className={FIELD} value={taskId} disabled={!scan || mutating} onChange={(event) => { invalidatePlan(); setTaskId(event.target.value); }}>
            <option value="">{t("choose")}</option>
            {scan ? <option value="__import__">{t("importTask")}</option> : null}
            {scan?.tasks.map((task) => <option key={task.id} value={task.id}>{task.title} · {task.sessionName}</option>)}
          </select>
        </label>
      </div>
      {selectedWorkspace?.cwd ? <p className="mt-2 break-all text-xs text-text-muted">{t("workingDirectory")}: {selectedWorkspace.cwd}</p> : null}
      {scan ? <p className="mt-2 text-xs text-text-muted">{t("observedAt", { time: scan.observedAt })}{scan.sessions.length === 0 ? ` · ${t("emptySessions")}` : ""}{scan.tasks.length === 0 ? ` · ${t("emptyTasks")}` : ""}</p> : null}
      {selectedSession?.error ? <p role="alert" className="mt-2 text-sm text-status-error">{selectedSession.error}</p> : null}
      <div className="mt-4 flex flex-wrap gap-2">
        <button type="button" className={BUTTON} disabled={!canPreview} onClick={() => void preview(HerdrSessionMode.ATTACH_EXISTING)}>{t("previewAttach")}</button>
        <button type="button" className={BUTTON} disabled={!canPreview} onClick={() => void preview(HerdrSessionMode.REBUILD_STOPPED)}>{t("previewRebuild")}</button>
        {pending === "plan" ? <span role="status" className="self-center text-xs text-text-muted">{t("planning")}</span> : null}
      </div>
      {plan ? <div className="mt-4 space-y-3 rounded-md border border-border-default bg-bg-page p-3">
        <h4 className="text-sm font-medium text-text-primary">{t("planTitle")}</h4>
        {plan.importTask ? <p className="text-sm text-text-primary">{t("importTaskPreview", { title: plan.importTask.title })}</p> : null}
        <p className="break-words text-xs text-text-secondary">{plan.source.sshHost || t("local")} / {plan.source.sessionName} / {plan.source.workspaceId} → {plan.targetSession}</p>
        <div className="grid gap-3 sm:grid-cols-3">
          {(["preserved", "recreated", "unsupported"] as const).map((category) => <div key={category}>
            <h5 className="text-xs font-medium text-text-primary">{t(category)}</h5>
            {plan[category].length ? <ul className="mt-1 list-inside list-disc text-xs text-text-secondary">{plan[category].map((item) => <li key={item}>{item}</li>)}</ul> : <p className="mt-1 text-xs text-text-muted">{t("none")}</p>}
          </div>)}
        </div>
        {plan.blockers.length ? <div role="alert" className="text-sm text-status-error"><h5>{t("blocked")}</h5><ul className="list-inside list-disc">{plan.blockers.map((blocker) => <li key={blocker}>{blockerLabel(blocker)}</li>)}</ul></div> : null}
        <p className="text-xs text-text-muted">{plan.mode === HerdrSessionMode.ATTACH_EXISTING ? t("attachNotice") : t("rebuildNotice")}</p>
        <button type="button" className={PRIMARY_BUTTON} disabled={plan.eligibility !== "ready" || Boolean(pending) || Boolean(operation)} onClick={() => void execute()}>{pending === "execute" ? t("executing") : t("execute")}</button>
      </div> : null}
      {error && execution.current && !operation ? <p className="mt-2 break-all text-xs text-text-muted">{t("retryNotice", { operationId: execution.current.operationId })}</p> : null}
      {error ? <p role="alert" className="mt-3 break-words text-sm text-status-error">{error}</p> : null}
      {operation ? <div className="mt-4 space-y-2 rounded-md border border-border-default p-3" aria-live="polite">
        <h4 className="text-sm font-medium text-text-primary">{phaseLabel(operation.phase)}</h4>
        <dl className="grid gap-1 break-all text-xs text-text-secondary"><dt>{t("operationId")}</dt><dd>{operation.operationId}</dd>
          {operation.targetWorkspaceId ? <><dt>{t("targetSpace")}</dt><dd>{operation.targetWorkspaceId}</dd></> : null}
          {operation.backupLocation ? <><dt>{t("backup")}</dt><dd>{operation.backupLocation}</dd></> : null}
        </dl>
        {operation.error ? <p className="text-sm text-status-error">{operation.error.message}</p> : null}
        <p className="text-xs text-text-muted">{t("recoveryNotice")}</p>
        <div className="flex flex-wrap gap-2">
          <button type="button" className={BUTTON} onClick={() => { void navigateToTaskDetail(operation.plan.taskId).catch((cause) => setError(errorMessage(cause))); }}>{t("openTask")}</button>
          {[HerdrOperationPhase.APPLYING, HerdrOperationPhase.VERIFYING].includes(operation.phase) ? <button type="button" className={BUTTON} disabled={controlPending} onClick={() => void control("cancel")}>{t("cancel")}</button> : null}
          {([HerdrOperationPhase.COMMITTED, HerdrOperationPhase.COMPLETED].includes(operation.phase) || (operation.phase === HerdrOperationPhase.ATTENTION && operation.bindingCommitted))
            && !(operation.plan.importTask && operation.plan.mode === HerdrSessionMode.ATTACH_EXISTING)
            ? <button type="button" className={BUTTON} disabled={Boolean(pending) || controlPending} onClick={() => void control("revertBinding")}>{t("revert")}</button> : null}
        </div>
      </div> : null}
      <div className="mt-5 border-t border-border-default pt-3">
        <div className="flex items-center justify-between gap-2"><h4 className="text-sm text-text-primary">{t("history")}</h4><button type="button" className={BUTTON} disabled={historyLoading} onClick={() => void reloadOperations()}>{t("refresh")}</button></div>
        {historyError ? <p role="alert" className="mt-2 text-sm text-status-error">{historyError}</p> : null}
        {historyLoading ? <p role="status" className="mt-2 text-xs text-text-muted">{t("loadingHistory")}</p> : null}
        {!historyLoading && !operations.length ? <p className="mt-2 text-xs text-text-muted">{t("emptyHistory")}</p> : null}
        <ul className="mt-2 space-y-1">{operations.map((record) => <li key={record.operationId}><button type="button" className={`${BUTTON} w-full text-left`} disabled={mutating} onClick={() => { setOperation(record); setError(null); }}>
          {record.plan.source.sshHost || t("local")} / {record.plan.source.sessionName} · {phaseLabel(record.phase)}<span className="ml-2 text-xs text-text-muted">{record.updatedAt}</span>
        </button></li>)}</ul>
      </div>
    </section>
  );
}
