import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

// ---- types mirroring the Rust models ----

type SourceStatus =
  | { kind: "ok" }
  | { kind: "stale" }
  | { kind: "auth_error" }
  | { kind: "not_configured" }
  | { kind: "need_relogin" };

interface QuotaWindow {
  label: string;
  used: number;
  quota: number;
  reset_at: string | null;
}

interface SourceExtras {
  membership?: string | null;
  extra_balance_cny?: number | null;
  extra_total_cny?: number | null;
  plan_tier?: string | null;
  credential_source?: string | null;
  overage_enabled?: boolean | null;
  overage_credits?: number | null;
  overage_cost_usd?: number | null;
  account_email?: string | null;
}

interface UsageSnapshot {
  source: "kimi_code" | "ark_agent_plan" | "kiro_cli";
  status: SourceStatus;
  windows: QuotaWindow[];
  extras: SourceExtras;
  fetched_at: string;
}

interface AppConfig {
  poll_interval_secs: number;
  show_kimi: boolean;
  show_ark: boolean;
  show_kiro: boolean;
}

interface ArkCredStatus {
  configured: boolean;
  source: "aksk" | "arkcli" | null;
  state: string;
}

interface KiroCredStatus {
  configured: boolean;
}

// ---- helpers ----

const SOURCE_NAME: Record<string, string> = {
  kimi_code: "Kimi Code",
  ark_agent_plan: "火山 AgentPlan",
  kiro_cli: "Kiro",
};

function statusText(s: SourceStatus): { text: string; cls: string } {
  switch (s.kind) {
    case "ok":
      return { text: "正常", cls: "ok" };
    case "stale":
      return { text: "数据过期", cls: "warn" };
    case "auth_error":
      return { text: "鉴权失败", cls: "err" };
    case "not_configured":
      return { text: "未配置", cls: "err" };
    case "need_relogin":
      return { text: "需要重新登录", cls: "err" };
  }
}

function fmtReset(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, "0")}:${String(
    d.getMinutes(),
  ).padStart(2, "0")} 重置`;
}

function fmtNum(n: number): string {
  if (n >= 10000) return `${(n / 10000).toFixed(1)}w`;
  return n.toLocaleString();
}

// ---- components ----

function WindowRow({ w }: { w: QuotaWindow }) {
  const pct = w.quota > 0 ? Math.min(100, (w.used / w.quota) * 100) : 0;
  const level = pct >= 80 ? "high" : pct >= 50 ? "mid" : "low";
  return (
    <div className="window-row">
      <div className="window-head">
        <span className="window-label">{w.label}</span>
        <span className="window-nums">
          {fmtNum(w.used)} / {fmtNum(w.quota)} ({pct.toFixed(0)}%)
        </span>
      </div>
      <div className="bar">
        <div className={`bar-fill ${level}`} style={{ width: `${pct}%` }} />
      </div>
      <div className="window-reset">{fmtReset(w.reset_at)}</div>
    </div>
  );
}

function SourceCard({ snap }: { snap: UsageSnapshot }) {
  const st = statusText(snap.status);
  const e = snap.extras;
  return (
    <div className="card">
      <div className="card-head">
        <span className="card-title">{SOURCE_NAME[snap.source]}</span>
        <span className={`status ${st.cls}`}>{st.text}</span>
      </div>
      {snap.windows.map((w) => (
        <WindowRow key={w.label} w={w} />
      ))}
      <div className="extras">
        {e.membership && <span>会员: {e.membership}</span>}
        {e.extra_balance_cny != null && (
          <span>
            Extra 余额: ¥{e.extra_balance_cny.toFixed(2)}
            {e.extra_total_cny != null && ` / ¥${e.extra_total_cny.toFixed(2)}`}
          </span>
        )}
        {e.plan_tier && <span>套餐: {e.plan_tier}</span>}
        {e.overage_enabled != null && (
          <span>超额: {e.overage_enabled ? "已开启" : "未开启"}</span>
        )}
        {e.overage_credits != null && e.overage_credits > 0 && (
          <span>
            超额用量: {e.overage_credits.toFixed(1)} credits
            {e.overage_cost_usd != null && ` (~$${e.overage_cost_usd.toFixed(2)})`}
          </span>
        )}
        {e.account_email && <span>{e.account_email}</span>}
        {e.credential_source && (
          <span>凭证: {e.credential_source === "aksk" ? "手动配置 AK/SK" : "arkcli 登录态"}</span>
        )}
      </div>
      <div className="fetched">更新于 {new Date(snap.fetched_at).toLocaleTimeString()}</div>
    </div>
  );
}

function UsageTab({ snapshots }: { snapshots: UsageSnapshot[] }) {
  if (snapshots.length === 0) {
    return <div className="empty">加载中…</div>;
  }
  return (
    <div>
      {snapshots.map((s) => (
        <SourceCard key={s.source} snap={s} />
      ))}
    </div>
  );
}

function SettingsTab({ onSaved }: { onSaved: () => void }) {
  const [cfg, setCfg] = useState<AppConfig | null>(null);
  const [cred, setCred] = useState<ArkCredStatus | null>(null);
  const [ak, setAk] = useState("");
  const [sk, setSk] = useState("");
  const [kiroCred, setKiroCred] = useState<KiroCredStatus | null>(null);
  const [kiroKey, setKiroKey] = useState("");
  const [kiroMsg, setKiroMsg] = useState<{ text: string; ok: boolean } | null>(null);
  const [kiroBusy, setKiroBusy] = useState(false);
  const [msg, setMsg] = useState<{ text: string; ok: boolean } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    invoke<AppConfig>("get_config").then(setCfg);
    invoke<ArkCredStatus>("get_ark_cred_status").then(setCred);
    invoke<KiroCredStatus>("get_kiro_cred_status").then(setKiroCred);
  }, []);

  const saveConfig = async (next: AppConfig) => {
    setCfg(next);
    await invoke("save_config", { config: next });
  };

  const saveCreds = async () => {
    setBusy(true);
    setMsg(null);
    try {
      await invoke("save_ark_credentials", { ak, sk });
      setMsg({ text: "验证成功，已保存到 Keychain", ok: true });
      setAk("");
      setSk("");
      setCred(await invoke("get_ark_cred_status"));
      onSaved();
    } catch (e) {
      setMsg({ text: String(e), ok: false });
    } finally {
      setBusy(false);
    }
  };

  const clearCreds = async () => {
    await invoke("clear_ark_credentials");
    setCred(await invoke("get_ark_cred_status"));
    setMsg({ text: "已清除保存的 AK/SK", ok: true });
    onSaved();
  };

  const saveKiroKey = async () => {
    setKiroBusy(true);
    setKiroMsg(null);
    try {
      const snap = await invoke<UsageSnapshot>("save_kiro_credentials", { key: kiroKey });
      setKiroMsg({
        text: `验证成功，已保存到 Keychain${snap.extras.plan_tier ? `（套餐: ${snap.extras.plan_tier}）` : ""}`,
        ok: true,
      });
      setKiroKey("");
      setKiroCred(await invoke("get_kiro_cred_status"));
      onSaved();
    } catch (e) {
      setKiroMsg({ text: String(e), ok: false });
    } finally {
      setKiroBusy(false);
    }
  };

  const clearKiroKey = async () => {
    await invoke("clear_kiro_credentials");
    setKiroCred(await invoke("get_kiro_cred_status"));
    setKiroMsg({ text: "已清除保存的 Kiro API Key", ok: true });
    onSaved();
  };

  if (!cfg) return <div className="empty">加载中…</div>;

  return (
    <div className="settings">
      <h3>火山引擎凭证</h3>
      <div className="cred-status">
        当前来源:{" "}
        {cred?.configured
          ? cred.source === "aksk"
            ? "手动配置的 AK/SK"
            : "arkcli 登录态（零配置）"
          : cred?.state === "expired"
            ? "arkcli 登录态已过期 — 请运行 arkcli auth login 或配置 AK/SK"
            : "未配置"}
      </div>
      <input
        type="text"
        placeholder="Access Key"
        value={ak}
        onChange={(e) => setAk(e.target.value)}
      />
      <input
        type="password"
        placeholder="Secret Key"
        value={sk}
        onChange={(e) => setSk(e.target.value)}
      />
      <div className="hint">
        建议创建仅带 ArkReadOnlyAccess 权限的 IAM 子用户 Access Key（控制台 → IAM → Access Key
        管理）。凭证仅存入 macOS Keychain。
      </div>
      <div className="btn-row">
        <button disabled={busy || !ak || !sk} onClick={saveCreds}>
          {busy ? "验证中…" : "保存并验证"}
        </button>
        {cred?.source === "aksk" && <button onClick={clearCreds}>清除已保存的 AK/SK</button>}
      </div>
      {msg && <div className={`msg ${msg.ok ? "ok" : "err"}`}>{msg.text}</div>}

      <h3>Kiro 凭证</h3>
      <div className="cred-status">
        当前状态: {kiroCred?.configured ? "已配置 API Key" : "未配置"}
      </div>
      <input
        type="password"
        placeholder="Kiro API Key（ksk_...）"
        value={kiroKey}
        onChange={(e) => setKiroKey(e.target.value)}
      />
      <div className="hint">
        在 Kiro 中生成 API Key（ksk_ 前缀）。凭证仅存入 macOS Keychain。
      </div>
      <div className="btn-row">
        <button disabled={kiroBusy || !kiroKey} onClick={saveKiroKey}>
          {kiroBusy ? "验证中…" : "保存并验证"}
        </button>
        {kiroCred?.configured && <button onClick={clearKiroKey}>清除已保存的 Key</button>}
      </div>
      {kiroMsg && <div className={`msg ${kiroMsg.ok ? "ok" : "err"}`}>{kiroMsg.text}</div>}

      <h3>轮询</h3>
      <label className="row">
        间隔（秒，≥30）
        <input
          type="number"
          min={30}
          value={cfg.poll_interval_secs}
          onChange={(e) =>
            saveConfig({ ...cfg, poll_interval_secs: Math.max(30, Number(e.target.value) || 300) })
          }
        />
      </label>

      <h3>状态栏显示</h3>
      <label className="row">
        <input
          type="checkbox"
          checked={cfg.show_kimi}
          onChange={(e) => saveConfig({ ...cfg, show_kimi: e.target.checked })}
        />
        显示 Kimi Code
      </label>
      <label className="row">
        <input
          type="checkbox"
          checked={cfg.show_ark}
          onChange={(e) => saveConfig({ ...cfg, show_ark: e.target.checked })}
        />
        显示火山 AgentPlan
      </label>
      <label className="row">
        <input
          type="checkbox"
          checked={cfg.show_kiro}
          onChange={(e) => saveConfig({ ...cfg, show_kiro: e.target.checked })}
        />
        显示 Kiro
      </label>
    </div>
  );
}

// ---- root ----

export default function App() {
  const [tab, setTab] = useState<"usage" | "settings">("usage");
  const [snapshots, setSnapshots] = useState<UsageSnapshot[]>([]);

  const reload = () => invoke<UsageSnapshot[]>("get_snapshots").then(setSnapshots);

  useEffect(() => {
    reload();
    const un1 = listen<UsageSnapshot[]>("snapshots-updated", (e) => setSnapshots(e.payload));
    const un2 = listen<string>("navigate", (e) => {
      if (e.payload === "settings") setTab("settings");
    });
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
    };
  }, []);

  return (
    <div className="app">
      <nav className="tabs">
        <button className={tab === "usage" ? "active" : ""} onClick={() => setTab("usage")}>
          用量
        </button>
        <button
          className={tab === "settings" ? "active" : ""}
          onClick={() => setTab("settings")}
        >
          设置
        </button>
        <button className="refresh" onClick={() => invoke("refresh_now")}>
          ⟳
        </button>
      </nav>
      <main>
        {tab === "usage" ? <UsageTab snapshots={snapshots} /> : <SettingsTab onSaved={reload} />}
      </main>
    </div>
  );
}
