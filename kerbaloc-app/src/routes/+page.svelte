<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  interface Unit {
    mod_id: string;
    display_name: string;
    version: string | null;
    keys: number;
    source_hash: string;
    installed: boolean;
    blacklisted: string | null;
  }
  interface DbVariant {
    variantId: string;
    srcSha256: string;
    keysTranslated: number;
    keysTarget: number;
  }
  let status = $state<{ root: string; language: string | null } | null>(null);
  let units = $state<Unit[]>([]);
  let dbPacks = $state<Map<string, DbVariant[]>>(new Map());
  let busy = $state(false);

  // ── 토스트 ──
  interface Toast {
    id: number;
    msg: string;
    kind: "info" | "ok" | "err" | "busy";
  }
  let toasts = $state<Toast[]>([]);
  let toastSeq = 0;
  function toast(msg: string, kind: Toast["kind"] = "ok", ttlMs = 5000): number {
    const id = ++toastSeq;
    toasts.push({ id, msg, kind });
    if (ttlMs > 0) setTimeout(() => dismiss(id), ttlMs);
    return id;
  }
  function dismiss(id: number) {
    toasts = toasts.filter((t) => t.id !== id);
  }
  async function withBusy<T>(msg: string, fn: () => Promise<T>): Promise<T | undefined> {
    const id = toast(msg, "busy", 0);
    busy = true;
    try {
      return await fn();
    } catch (e) {
      toast(String(e), "err", 12000);
      return undefined;
    } finally {
      dismiss(id);
      busy = false;
    }
  }

  // ── 선택 ──
  let selected = $state<Record<string, boolean>>({});
  const selectedIds = () => Object.keys(selected).filter((k) => selected[k]);

  // ── 일괄 번역 모달 ──
  interface BatchRow {
    mod_id: string;
    status: "대기" | "진행" | "완료" | "오류";
    detail: string;
    pack_dir: string | null;
    shared: boolean;
  }
  let batch = $state<BatchRow[] | null>(null);
  let batchTotalCost = $state(0);

  // ── DB 번역 다이얼로그 ──
  interface DbOffer {
    mod_id: string;
    display_name: string;
    source_hash: string;
    variants: DbVariant[];
    selectedVariant: string;
    install: boolean;
  }
  let dbOffers = $state<DbOffer[] | null>(null);
  let dbDialogAutoShown = false; // 세션당 1회만 자동 표시

  /** 미설치인데 DB에 번역이 있는 모드 목록. */
  function availableDbOffers(): DbOffer[] {
    return units
      .filter((u) => !u.installed && (dbPacks.get(u.mod_id)?.length ?? 0) > 0)
      .map((u) => {
        const variants = dbPacks.get(u.mod_id)!;
        // 버전(소스 해시) 일치 변형 우선, 없으면 최신(마지막)
        const fresh = variants.find((v) => v.srcSha256 === u.source_hash);
        return {
          mod_id: u.mod_id,
          display_name: u.display_name,
          source_hash: u.source_hash,
          variants,
          selectedVariant: (fresh ?? variants[variants.length - 1]).variantId,
          install: true,
        };
      });
  }

  function openDbDialog() {
    const offers = availableDbOffers();
    if (offers.length === 0) {
      toast("DB에 새로 받을 번역이 없습니다", "info");
      return;
    }
    dbOffers = offers;
  }

  async function installDbSelected() {
    if (!dbOffers) return;
    const targets = dbOffers.filter((o) => o.install);
    if (targets.length === 0) return;
    dbOffers = null;
    await withBusy(`DB 번역 ${targets.length}개 설치 중…`, async () => {
      let ok = 0;
      for (const o of targets) {
        try {
          await invoke("install_from_db", { modId: o.mod_id, variant: o.selectedVariant });
          ok++;
        } catch (e) {
          toast(`${o.mod_id}: ${e}`, "err", 12000);
        }
      }
      if (ok > 0) toast(`DB 번역 ${ok}개 설치 완료 (게임 재시작 필요)`);
      units = await invoke("scan_units", { force: false });
    });
  }

  // ── 키 에디터 ──
  interface ModKey {
    key: string;
    en: string;
    ko: string | null;
    edited?: string;
  }
  let keyEditor = $state<{ modId: string; keys: ModKey[] } | null>(null);
  let keyFilter = $state("");
  let keyLimit = $state(300);
  let keyErrors = $state<string[]>([]);
  let keySaving = $state(false);

  // ── 모드 용어집 (키 에디터의 탭) ──
  interface GlossEntry {
    term: string;
    policy: string; // keep | translate | translit
    ko: string | null;
    aliases?: string[];
    why?: string | null;
    count: number;
    confirmed: boolean;
  }
  let editorTab = $state<"keys" | "glossary">("keys");
  let modGlossary = $state<GlossEntry[]>([]);
  let glossaryGenerating = $state(false);
  let glossarySaving = $state(false);

  async function genGlossary() {
    if (!keyEditor) return;
    glossaryGenerating = true;
    try {
      const r: { entries: GlossEntry[]; cost: number } = await invoke("gen_mod_glossary", {
        modId: keyEditor.modId,
      });
      modGlossary = r.entries;
      toast(`용어집 초안 ${r.entries.length}개 생성 (비용 $${r.cost.toFixed(4)})`);
    } catch (e) {
      toast(String(e), "err", 12000);
    } finally {
      glossaryGenerating = false;
    }
  }

  async function saveGlossary() {
    if (!keyEditor) return;
    glossarySaving = true;
    try {
      await invoke("save_mod_glossary", { modId: keyEditor.modId, entries: modGlossary });
      for (const e of modGlossary) e.confirmed = true;
      toast("용어집 저장·확정 완료 — 다음 번역부터 반영됩니다");
    } catch (e) {
      toast(String(e), "err", 12000);
    } finally {
      glossarySaving = false;
    }
  }

  // ── 설정 ──
  interface AppSettings {
    ksp_root: string | null;
    nick: string | null;
    provider: string | null;
    model: string | null;
    gemini_api_key: string | null;
    openai_api_key: string | null;
    anthropic_api_key: string | null;
    ollama_url: string | null;
    lmstudio_url: string | null;
    price_in: number | null;
    price_out: number | null;
    max_keys: number | null;
    max_payload_tokens: number | null;
    max_retries: number | null;
    workers: number | null;
  }
  let showSettings = $state(false);
  let settings = $state<AppSettings>({
    ksp_root: null,
    nick: null,
    provider: null,
    model: null,
    gemini_api_key: null,
    openai_api_key: null,
    anthropic_api_key: null,
    ollama_url: null,
    lmstudio_url: null,
    price_in: null,
    price_out: null,
    max_keys: null,
    max_payload_tokens: null,
    max_retries: null,
    workers: null,
  });

  // ── 제공자/모델 목록 ──
  const PROVIDERS = [
    { id: "gemini", label: "Gemini" },
    { id: "openai", label: "OpenAI" },
    { id: "anthropic", label: "Anthropic" },
    { id: "claude-code", label: "Claude Code (claude -p)" },
    { id: "ollama", label: "Ollama" },
    { id: "lmstudio", label: "LM Studio" },
  ];
  // API 조회 실패 시 폴백 목록
  const FALLBACK_MODELS: Record<string, string[]> = {
    gemini: ["gemini-3.1-flash-lite", "gemini-3-flash", "gemini-3-pro"],
    openai: ["gpt-5.1", "gpt-5.1-mini", "gpt-5.1-nano"],
    anthropic: ["claude-haiku-4-5", "claude-sonnet-5", "claude-opus-5"],
    "claude-code": ["sonnet", "opus", "haiku"],
    ollama: [],
    lmstudio: [],
  };
  const DEFAULT_MODELS: Record<string, string> = {
    gemini: "gemini-3.1-flash-lite",
    openai: "gpt-5.1-mini",
    anthropic: "claude-haiku-4-5",
    "claude-code": "sonnet",
    ollama: "",
    lmstudio: "",
  };
  let modelList = $state<string[]>([]);
  let modelListLoading = $state(false);

  function currentProvider(): string {
    return settings.provider || "gemini";
  }

  async function loadModelList() {
    modelListLoading = true;
    try {
      modelList = await invoke("list_models_cmd", { settings });
      if (modelList.length === 0) modelList = FALLBACK_MODELS[currentProvider()] ?? [];
    } catch {
      modelList = FALLBACK_MODELS[currentProvider()] ?? [];
    } finally {
      modelListLoading = false;
    }
  }

  function openSettings() {
    if (!settings.provider) settings.provider = "gemini";
    modelList = FALLBACK_MODELS[currentProvider()] ?? [];
    showSettings = true;
    void loadModelList();
  }

  function onProviderChange() {
    settings.model = DEFAULT_MODELS[currentProvider()] || null;
    modelList = FALLBACK_MODELS[currentProvider()] ?? [];
    void loadModelList();
  }

  async function refresh(force = false) {
    await withBusy(force ? "재스캔 중…" : "로드 중…", async () => {
      status = await invoke("game_status");
      units = await invoke("scan_units", { force });
      try {
        const idx: any = await invoke("db_index");
        dbPacks = new Map(idx.packs.map((p: any) => [p.modId, p.variants]));
      } catch (e) {
        console.warn("DB 인덱스 실패", e);
      }
    });
  }

  async function toggleLanguage() {
    const target = status?.language === "ko" ? "en-us" : "ko";
    await invoke("set_language", { lang: target });
    status = await invoke("game_status");
    toast(`언어: ${target} (게임 재시작 필요)`);
  }

  async function installFromDb(modId: string) {
    await withBusy(`${modId} 설치 중…`, async () => {
      await invoke("install_from_db", { modId, variant: null });
      toast(`${modId} 설치 완료`);
      units = await invoke("scan_units", { force: false });
    });
  }

  async function removePack(modId: string) {
    await invoke("remove_pack", { modId });
    toast(`${modId} 제거됨`);
    units = await invoke("scan_units", { force: false });
  }

  async function runBatch(ids: string[]) {
    if (ids.length === 0) return;
    batch = ids.map((m) => ({ mod_id: m, status: "대기", detail: "", pack_dir: null, shared: false }));
    batchTotalCost = 0;
    try {
      await invoke("translate_batch", { modIds: ids });
      toast(`일괄 번역 완료 (${ids.length}개 모드, $${batchTotalCost.toFixed(4)})`);
      selected = {};
      units = await invoke("scan_units", { force: false });
    } catch (e) {
      toast(String(e), "err", 12000);
    }
  }

  function untranslatedIds(): string[] {
    return units.filter((u) => !u.installed && !u.blacklisted).map((u) => u.mod_id);
  }

  function installedIds(): string[] {
    return units.filter((u) => u.installed).map((u) => u.mod_id);
  }

  // 일괄 번역 결과 팩 공유 (행 단위)
  async function shareBatchRow(row: BatchRow) {
    if (!row.pack_dir) return;
    await withBusy(`${row.mod_id} 공유 중…`, async () => {
      const prUrl: string = await invoke("share_pack_cmd", { packDir: row.pack_dir });
      row.shared = true;
      toast(`${row.mod_id} 공유 완료! ${prUrl}`, "ok", 15000);
    });
  }

  // 일괄 번역 결과 전체를 한 번의 PR로 공유
  async function shareBatchAll() {
    if (!batch) return;
    const rows = batch.filter((r) => r.pack_dir && !r.shared);
    if (rows.length === 0) return;
    await withBusy(`${rows.length}개 팩 공유 중…`, async () => {
      const prUrl: string = await invoke("share_packs_cmd", {
        packDirs: rows.map((r) => r.pack_dir),
      });
      for (const r of rows) r.shared = true;
      toast(`${rows.length}개 팩 공유 완료! ${prUrl}`, "ok", 15000);
    });
  }

  interface BatchShareResult {
    pr_url: string | null;
    shared: string[];
    skipped: { mod_id: string; error: string }[];
  }

  // 설치된 번역 공유 (전체/선택) — 한 번의 PR
  async function shareInstalled(ids: string[]) {
    const targets = ids.filter((id) => units.find((u) => u.mod_id === id)?.installed);
    if (targets.length === 0) {
      toast("공유할 설치된 번역이 없습니다", "info");
      return;
    }
    await withBusy(`${targets.length}개 팩 공유 중…`, async () => {
      const r: BatchShareResult = await invoke("share_installed", { modIds: targets });
      if (r.pr_url) toast(`${r.shared.length}개 팩 공유 완료! ${r.pr_url}`, "ok", 15000);
      else toast("공유할 수 있는 팩이 없습니다", "info");
      for (const s of r.skipped.slice(0, 5)) toast(`${s.mod_id}: ${s.error}`, "err", 12000);
      if (r.skipped.length > 5) toast(`외 ${r.skipped.length - 5}건 제외됨`, "err", 12000);
    });
  }

  async function openKeyEditor(modId: string) {
    keyErrors = [];
    keyFilter = "";
    keyLimit = 300;
    editorTab = "keys";
    modGlossary = [];
    const keys = await withBusy(`${modId} 키 로드 중…`, () =>
      invoke<ModKey[]>("get_mod_keys", { modId }),
    );
    if (keys) {
      keyEditor = { modId, keys };
      try {
        modGlossary = await invoke("get_mod_glossary", { modId });
      } catch (e) {
        console.warn("용어집 로드 실패", e);
      }
    }
  }

  function filteredKeys(): ModKey[] {
    if (!keyEditor) return [];
    const q = keyFilter.trim().toLowerCase();
    if (!q) return keyEditor.keys;
    return keyEditor.keys.filter(
      (k) =>
        k.key.toLowerCase().includes(q) ||
        k.en.toLowerCase().includes(q) ||
        (k.ko ?? "").toLowerCase().includes(q) ||
        (k.edited ?? "").toLowerCase().includes(q),
    );
  }

  async function saveKeys() {
    if (!keyEditor) return;
    keySaving = true;
    keyErrors = [];
    try {
      const edits = keyEditor.keys
        .map((k) => ({ key: k.key, ko: (k.edited ?? k.ko ?? "").trim() }))
        .filter((e) => e.ko.length > 0);
      const r: { errors: string[]; installed: string | null } = await invoke("save_mod_keys", {
        modId: keyEditor.modId,
        edits,
      });
      if (r.errors.length > 0) {
        keyErrors = r.errors;
      } else {
        toast(`${keyEditor.modId} 저장·설치 완료 (${edits.length}키, 게임 재시작 필요)`);
        keyEditor = null;
        units = await invoke("scan_units", { force: false });
      }
    } catch (e) {
      keyErrors = [String(e)];
    } finally {
      keySaving = false;
    }
  }

  async function saveSettings() {
    // 숫자 입력이 비면 null(기본값 사용)로 저장
    for (const k of [
      "price_in",
      "price_out",
      "max_keys",
      "max_payload_tokens",
      "max_retries",
      "workers",
    ] as const) {
      settings[k] = Number.isFinite(settings[k]) ? settings[k] : null;
    }
    await invoke("save_settings", { settings });
    showSettings = false;
    toast("설정 저장됨");
    await refresh();
  }

  onMount(async () => {
    settings = await invoke("load_settings");
    await listen<{ mod_id: string; done: number; total: number; cost: number }>(
      "translate-progress",
      (e) => {
        if (batch) {
          const row = batch.find((r) => r.mod_id === e.payload.mod_id);
          if (row) {
            row.status = "진행";
            row.detail = `${e.payload.done}/${e.payload.total} — $${e.payload.cost.toFixed(4)}`;
          }
        }
      },
    );
    await listen<{
      mod_id: string;
      ok: number;
      review: number;
      failed: number;
      cost: number;
      error: string | null;
      installed: boolean;
      pack_dir: string | null;
    }>("batch-mod-done", (e) => {
      if (!batch) return;
      const row = batch.find((r) => r.mod_id === e.payload.mod_id);
      if (!row) return;
      batchTotalCost += e.payload.cost; // 실패한 모드의 지출도 합산
      row.pack_dir = e.payload.pack_dir;
      if (e.payload.error) {
        row.status = "오류";
        row.detail = `${e.payload.error}${e.payload.cost > 0 ? ` · $${e.payload.cost.toFixed(4)}` : ""}`;
      } else {
        row.status = "완료";
        row.detail = `${e.payload.ok}키 · $${e.payload.cost.toFixed(4)}${e.payload.installed ? " · 설치됨" : ""}${e.payload.review > 0 ? ` · 검수 ${e.payload.review}` : ""}${e.payload.failed > 0 ? ` · 실패 ${e.payload.failed}` : ""}`;
      }
    });
    await refresh();
    // 시작 시 1회: 받을 수 있는 DB 번역이 있으면 다이얼로그 표시
    if (!dbDialogAutoShown) {
      dbDialogAutoShown = true;
      const offers = availableDbOffers();
      if (offers.length > 0) dbOffers = offers;
    }
  });
</script>

<main>
  <header>
    <h1>KerbaLoc 스튜디오</h1>
    <div class="header-actions">
      {#if status}
        <button class="lang" onclick={toggleLanguage}>
          한국어 {status.language === "ko" ? "ON" : "OFF"}
        </button>
      {/if}
      <button disabled={busy} onclick={openDbDialog}>DB 번역 ({availableDbOffers().length})</button>
      <button onclick={openSettings}>설정</button>
      <button disabled={busy} onclick={() => refresh(true)}>재스캔</button>
    </div>
  </header>

  <div class="bulk-bar">
    <button
      class="primary"
      disabled={busy || selectedIds().length === 0}
      onclick={() => runBatch(selectedIds())}
    >
      선택 번역 ({selectedIds().length})
    </button>
    <button disabled={busy} onclick={() => runBatch(untranslatedIds())}>
      미설치 전체 번역 ({untranslatedIds().length})
    </button>
    <button
      disabled={busy || selectedIds().length === 0}
      onclick={() => shareInstalled(selectedIds())}
    >
      선택 공유 ({selectedIds().length})
    </button>
    <button disabled={busy} onclick={() => shareInstalled(installedIds())}>
      설치됨 전체 공유 ({installedIds().length})
    </button>
  </div>

  <table>
    <thead>
      <tr>
        <th class="chk">
          <input
            type="checkbox"
            onchange={(e) => {
              const on = (e.target as HTMLInputElement).checked;
              const next: Record<string, boolean> = {};
              if (on) for (const u of units) if (!u.blacklisted) next[u.mod_id] = true;
              selected = next;
            }}
          />
        </th>
        <th>모드</th><th>버전</th><th>키수</th><th>상태</th><th>액션</th>
      </tr>
    </thead>
    <tbody>
      {#each units as u (u.mod_id)}
        {@const variants = dbPacks.get(u.mod_id) ?? []}
        {@const fresh = variants.some((v) => v.srcSha256 === u.source_hash)}
        <tr class="row" onclick={() => openKeyEditor(u.mod_id)}>
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <td class="chk" onclick={(e) => e.stopPropagation()}>
            <input type="checkbox" disabled={!!u.blacklisted} bind:checked={selected[u.mod_id]} />
          </td>
          <td title={u.mod_id}>{u.display_name}</td>
          <td>{u.version ?? "-"}</td>
          <td class="num">{u.keys}</td>
          <td>
            {#if u.blacklisted}<span class="badge err" title={u.blacklisted}>번역 차단</span>
            {:else if u.installed}<span class="badge ok">설치됨</span>
            {:else if variants.length > 0}<span class="badge db">DB {variants.length}변형{fresh ? "" : " (버전 다름)"}</span>
            {:else}<span class="badge none">미번역</span>{/if}
          </td>
          <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
          <td class="actions" onclick={(e) => e.stopPropagation()}>
            {#if u.installed}
              <button onclick={() => removePack(u.mod_id)}>제거</button>
            {:else if variants.length > 0}
              <button class="primary" onclick={() => installFromDb(u.mod_id)}>설치</button>
            {/if}
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</main>

<!-- 토스트 스택 -->
<div class="toast-stack">
  {#each toasts as t (t.id)}
    <div class="toast-item {t.kind}">
      {#if t.kind === "busy"}<span class="spinner"></span>{/if}
      <span class="toast-msg">{t.msg}</span>
      <button class="toast-x" onclick={() => dismiss(t.id)}>×</button>
    </div>
  {/each}
</div>

{#if dbOffers}
  <div class="modal-backdrop">
    <div class="modal wide">
      <h2>DB에 받을 수 있는 번역 ({dbOffers.length}개 모드)</h2>
      <p class="hint">설치되어 있지 않은 모드 중 번역 DB에 팩이 있는 목록입니다. 변형을 고르고 설치하세요.</p>
      <div class="key-table">
        <table>
          <thead>
            <tr><th class="chk"></th><th>모드</th><th style="width:44%">번역 선택</th><th style="width:18%">상태</th></tr>
          </thead>
          <tbody>
            {#each dbOffers as o (o.mod_id)}
              {@const sel = o.variants.find((v) => v.variantId === o.selectedVariant)}
              <tr>
                <td class="chk"><input type="checkbox" bind:checked={o.install} /></td>
                <td title={o.mod_id}>{o.display_name}</td>
                <td>
                  <select class="variant" bind:value={o.selectedVariant}>
                    {#each o.variants as v (v.variantId)}
                      <option value={v.variantId}>
                        {v.variantId} — {v.keysTranslated}/{v.keysTarget}키{v.srcSha256 === o.source_hash ? " · 버전 일치" : " · 버전 다름"}
                      </option>
                    {/each}
                  </select>
                </td>
                <td>
                  {#if sel && sel.srcSha256 !== o.source_hash}
                    <span class="badge warn" title="이 번역은 다른 버전의 모드 기준입니다. 일부 키가 누락되거나 원문과 다를 수 있습니다.">⚠ 버전 다름</span>
                  {:else}
                    <span class="badge ok">버전 일치</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <div class="modal-actions">
        <button
          class="primary"
          disabled={busy || !dbOffers.some((o) => o.install)}
          onclick={installDbSelected}
        >선택 설치 ({dbOffers.filter((o) => o.install).length})</button>
        <button onclick={() => (dbOffers = null)}>닫기</button>
        <span class="hint">버전이 다른 번역도 설치할 수 있지만, 누락 키는 영어로 표시됩니다.</span>
      </div>
    </div>
  </div>
{/if}

{#if batch}
  <div class="modal-backdrop">
    <div class="modal wide">
      <h2>일괄 번역 ({batch.filter((r) => r.status === "완료").length}/{batch.length}) — 누적 ${batchTotalCost.toFixed(4)}</h2>
      <div class="key-table">
        <table>
          <thead><tr><th>모드</th><th style="width:12%">상태</th><th>상세</th><th style="width:10%"></th></tr></thead>
          <tbody>
            {#each batch as r (r.mod_id)}
              <tr>
                <td>{r.mod_id}</td>
                <td><span class="badge {r.status === '완료' ? 'ok' : r.status === '오류' ? 'err' : r.status === '진행' ? 'db' : 'none'}">{r.status}</span></td>
                <td class="key-en">{r.detail}</td>
                <td>
                  {#if r.pack_dir}
                    <button disabled={busy || r.shared} onclick={() => shareBatchRow(r)}>
                      {r.shared ? "공유됨" : "공유"}
                    </button>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <div class="modal-actions">
        <button
          disabled={busy || !batch.some((r) => r.pack_dir && !r.shared) || batch.some((r) => r.status === "대기" || r.status === "진행")}
          onclick={shareBatchAll}
        >성공한 모드 전체 공유</button>
        <button
          disabled={batch.some((r) => r.status === "대기" || r.status === "진행")}
          onclick={() => (batch = null)}
        >닫기</button>
        <span class="hint">병렬 번역 (설정 워커 수) — 성공한 팩은 즉시 게임에 설치됩니다.</span>
      </div>
    </div>
  </div>
{/if}

{#if keyEditor}
  {@const shown = filteredKeys()}
  <div class="modal-backdrop">
    <div class="modal wide">
      <h2>{keyEditor.modId}</h2>
      <div class="tabs">
        <button class="tab {editorTab === 'keys' ? 'active' : ''}" onclick={() => (editorTab = "keys")}>
          번역 키 ({keyEditor.keys.length})
        </button>
        <button class="tab {editorTab === 'glossary' ? 'active' : ''}" onclick={() => (editorTab = "glossary")}>
          용어집 ({modGlossary.length})
        </button>
      </div>

      {#if editorTab === "keys"}
        <div class="key-toolbar">
          <input placeholder="키·원문·번역 검색…" bind:value={keyFilter} />
          <span class="hint">{shown.length}개 일치</span>
        </div>
        {#if keyErrors.length > 0}
          <div class="error">{keyErrors.slice(0, 10).join("\n")}{keyErrors.length > 10 ? `\n… 외 ${keyErrors.length - 10}건` : ""}</div>
        {/if}
        <div class="key-table">
          <table>
            <thead>
              <tr><th style="width:26%">키</th><th style="width:37%">원문</th><th style="width:37%">번역 (편집 가능)</th></tr>
            </thead>
            <tbody>
              {#each shown.slice(0, keyLimit) as k (k.key)}
                <tr>
                  <td class="key-name" title={k.key}>{k.key}</td>
                  <td class="key-en">{k.en}</td>
                  <td>
                    <input
                      class="key-ko"
                      value={k.edited ?? k.ko ?? ""}
                      placeholder="(미번역 — 영어 폴백)"
                      oninput={(e) => (k.edited = (e.target as HTMLInputElement).value)}
                    />
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
          {#if shown.length > keyLimit}
            <button class="more" onclick={() => (keyLimit += 500)}>
              더 보기 ({shown.length - keyLimit}개 남음)
            </button>
          {/if}
        </div>
        <div class="modal-actions">
          <button class="primary" disabled={keySaving} onclick={saveKeys}>
            {keySaving ? "저장 중…" : "저장·게임에 설치"}
          </button>
          <button onclick={() => (keyEditor = null)}>닫기</button>
          <span class="hint">저장 시 검증 후 manual 팩으로 설치됩니다. 빈 칸은 영어 폴백.</span>
        </div>
      {:else}
        <div class="key-toolbar">
          <button disabled={glossaryGenerating} onclick={genGlossary}>
            {glossaryGenerating ? "생성 중…" : "초안 생성 (LLM)"}
          </button>
          <span class="hint">원문에서 용어 후보를 추출·분류합니다. 저장한 항목만 번역 프롬프트에 주입됩니다.</span>
        </div>
        {#if modGlossary.length === 0}
          <p class="hint">용어집이 비어 있습니다. "초안 생성"으로 시작하세요.</p>
        {:else}
          <div class="key-table">
            <table>
              <thead>
                <tr>
                  <th style="width:20%">용어</th>
                  <th style="width:12%">정책</th>
                  <th style="width:20%">한국어</th>
                  <th style="width:34%">근거</th>
                  <th style="width:7%">횟수</th>
                  <th style="width:7%"></th>
                </tr>
              </thead>
              <tbody>
                {#each modGlossary as g, i (g.term)}
                  <tr>
                    <td class="key-name" title={g.term}>
                      {g.term}{#if !g.confirmed}<span class="badge none" title="저장 시 확정됩니다"> 초안</span>{/if}
                    </td>
                    <td>
                      <select class="variant" bind:value={g.policy}>
                        <option value="translate">번역</option>
                        <option value="translit">음차</option>
                        <option value="keep">영어 유지</option>
                      </select>
                    </td>
                    <td>
                      <input
                        class="key-ko"
                        bind:value={g.ko}
                        disabled={g.policy === "keep"}
                        placeholder={g.policy === "keep" ? "(영어 유지)" : "한국어"}
                      />
                    </td>
                    <td class="key-en">{g.why ?? ""}</td>
                    <td class="num">{g.count}</td>
                    <td>
                      <button onclick={() => (modGlossary = modGlossary.filter((_, j) => j !== i))}>×</button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
        <div class="modal-actions">
          <button class="primary" disabled={glossarySaving || modGlossary.length === 0} onclick={saveGlossary}>
            {glossarySaving ? "저장 중…" : "저장·확정"}
          </button>
          <button onclick={() => (keyEditor = null)}>닫기</button>
          <span class="hint">번역 정책인데 한국어가 비면 매칭 시 "(영어 유지)"로 주입됩니다.</span>
        </div>
      {/if}
    </div>
  </div>
{/if}

{#if showSettings}
  <div class="modal-backdrop">
    <div class="modal">
      <h2>설정</h2>
      <label>KSP 경로 (비우면 자동 감지)
        <input bind:value={settings.ksp_root} placeholder="C:\...\Kerbal Space Program" />
      </label>
      <label>닉네임 (공유 시 표시)
        <input bind:value={settings.nick} placeholder="anon" />
      </label>

      <h3>번역 모델</h3>
      <label>AI 제공자
        <select bind:value={settings.provider} onchange={onProviderChange}>
          {#each PROVIDERS as p (p.id)}
            <option value={p.id} selected={currentProvider() === p.id}>{p.label}</option>
          {/each}
        </select>
      </label>
      <label>모델 — 목록에서 선택하거나 직접 입력
        <div class="model-row">
          <input
            list="model-options"
            bind:value={settings.model}
            placeholder={DEFAULT_MODELS[currentProvider()] || "모델 ID 입력"}
          />
          <button disabled={modelListLoading} onclick={loadModelList} title="제공자 API에서 모델 목록 조회">
            {modelListLoading ? "조회 중…" : "목록 갱신"}
          </button>
        </div>
        <datalist id="model-options">
          {#each modelList as m (m)}<option value={m}></option>{/each}
        </datalist>
      </label>
      {#if currentProvider() === "gemini"}
        <label>Gemini API 키
          <input type="password" bind:value={settings.gemini_api_key} />
        </label>
      {:else if currentProvider() === "openai"}
        <label>OpenAI API 키
          <input type="password" bind:value={settings.openai_api_key} />
        </label>
      {:else if currentProvider() === "anthropic"}
        <label>Anthropic API 키
          <input type="password" bind:value={settings.anthropic_api_key} />
        </label>
      {:else if currentProvider() === "claude-code"}
        <p class="hint">로컬에 설치·로그인된 claude CLI를 사용합니다 (API 키·비용 없음).</p>
      {:else if currentProvider() === "ollama"}
        <label>Ollama URL (기본 http://localhost:11434/v1)
          <input bind:value={settings.ollama_url} placeholder="http://localhost:11434/v1" />
        </label>
      {:else if currentProvider() === "lmstudio"}
        <label>LM Studio URL (기본 http://localhost:1234/v1)
          <input bind:value={settings.lmstudio_url} placeholder="http://localhost:1234/v1" />
        </label>
      {/if}
      <div class="grid2">
        <label>입력 단가 ($/1M 토큰, 비우면 기본값)
          <input type="number" min="0" step="0.01" bind:value={settings.price_in} placeholder="모델 기본값" />
        </label>
        <label>출력 단가 ($/1M 토큰, 비우면 기본값)
          <input type="number" min="0" step="0.01" bind:value={settings.price_out} placeholder="모델 기본값" />
        </label>
      </div>

      <h3>세부 옵션</h3>
      <div class="grid2">
        <label>청크당 최대 키 수 (기본 40)
          <input type="number" min="1" max="200" bind:value={settings.max_keys} placeholder="40" />
        </label>
        <label>청크당 최대 토큰 (기본 2000)
          <input type="number" min="100" max="20000" bind:value={settings.max_payload_tokens} placeholder="2000" />
        </label>
        <label>검증 실패 재시도 횟수 (기본 2)
          <input type="number" min="0" max="10" bind:value={settings.max_retries} placeholder="2" />
        </label>
        <label>병렬 워커 수 (기본 20)
          <input type="number" min="1" max="100" bind:value={settings.workers} placeholder="20" />
        </label>
      </div>
      <div class="modal-actions">
        <button class="primary" onclick={saveSettings}>저장</button>
        <button onclick={() => (showSettings = false)}>취소</button>
      </div>
    </div>
  </div>
{/if}

<style>
  :global(body) { margin: 0; font-family: "Segoe UI", "Malgun Gothic", sans-serif; background: #14161a; color: #e6e6e6; }
  main { max-width: 1000px; margin: 0 auto; padding: 1rem; }
  header { display: flex; justify-content: space-between; align-items: center; }
  h1 { font-size: 1.3rem; }
  .header-actions { display: flex; gap: 0.5rem; align-items: center; }
  button { background: #2a2e35; color: #e6e6e6; border: 1px solid #444; border-radius: 6px; padding: 0.35rem 0.8rem; cursor: pointer; }
  button:hover:not(:disabled) { background: #353a43; }
  button:disabled { opacity: 0.5; cursor: default; }
  button.primary { background: #2b5cab; border-color: #3a6fc4; }
  button.lang { background: #244; }
  .bulk-bar { display: flex; gap: 0.5rem; margin-top: 0.8rem; }
  table { width: 100%; border-collapse: collapse; margin-top: 1rem; }
  th, td { text-align: left; padding: 0.45rem 0.6rem; border-bottom: 1px solid #2a2e35; font-size: 0.9rem; }
  th.chk, td.chk { width: 2rem; }
  td.num { text-align: right; }
  td.actions { display: flex; gap: 0.3rem; }
  .badge { padding: 0.15rem 0.5rem; border-radius: 999px; font-size: 0.78rem; }
  .badge.ok { background: #1d4028; color: #8fdba3; }
  .badge.db { background: #1d3350; color: #8fbadb; }
  .badge.none { background: #333; color: #999; }
  .badge.err { background: #4a1d1d; color: #db8f8f; }
  .badge.warn { background: #4a3a1d; color: #dbc48f; }
  select.variant { width: 100%; box-sizing: border-box; background: #14161a; color: #e6e6e6; border: 1px solid #444; border-radius: 6px; padding: 0.35rem; }
  .error { background: #4a1d1d; padding: 0.6rem; border-radius: 6px; margin-top: 0.6rem; white-space: pre-wrap; }
  .modal-backdrop { position: fixed; inset: 0; background: rgba(0,0,0,0.6); display: flex; align-items: center; justify-content: center; }
  .modal { background: #1b1e24; border: 1px solid #333; border-radius: 10px; padding: 1.2rem; width: min(640px, 90vw); max-height: 85vh; overflow-y: auto; }
  label { display: block; margin: 0.6rem 0; font-size: 0.9rem; }
  label input, label select { width: 100%; box-sizing: border-box; margin-top: 0.25rem; background: #14161a; color: #e6e6e6; border: 1px solid #444; border-radius: 6px; padding: 0.4rem; }
  .modal h3 { font-size: 0.95rem; margin: 1rem 0 0.2rem; color: #aac; border-top: 1px solid #2a2e35; padding-top: 0.8rem; }
  .model-row { display: flex; gap: 0.4rem; margin-top: 0.25rem; }
  .model-row input { flex: 1; margin-top: 0; }
  .model-row button { white-space: nowrap; }
  .grid2 { display: grid; grid-template-columns: 1fr 1fr; gap: 0 0.8rem; }
  .tabs { display: flex; gap: 0.3rem; border-bottom: 1px solid #2a2e35; margin: 0.4rem 0 0.6rem; }
  .tab { background: none; border: none; border-bottom: 2px solid transparent; border-radius: 0; padding: 0.4rem 0.8rem; color: #999; }
  .tab.active { color: #e6e6e6; border-bottom-color: #3a6fc4; }
  .modal-actions { display: flex; gap: 0.5rem; margin-top: 1rem; align-items: center; }
  .hint { color: #999; font-size: 0.8rem; }
  tr.row { cursor: pointer; }
  tr.row:hover { background: #1d2128; }
  .modal.wide { width: min(980px, 95vw); }
  .key-toolbar { display: flex; gap: 0.6rem; align-items: center; margin: 0.5rem 0; }
  .key-toolbar input { flex: 1; background: #14161a; color: #e6e6e6; border: 1px solid #444; border-radius: 6px; padding: 0.4rem; }
  .key-table { max-height: 55vh; overflow-y: auto; border: 1px solid #2a2e35; border-radius: 8px; }
  .key-table table { margin-top: 0; }
  .key-name { font-family: monospace; font-size: 0.78rem; color: #8fbadb; word-break: break-all; }
  .key-en { font-size: 0.85rem; color: #bbb; word-break: break-word; }
  input.key-ko { width: 100%; box-sizing: border-box; background: #14161a; color: #e6e6e6; border: 1px solid #3a3f48; border-radius: 6px; padding: 0.3rem; }
  button.more { width: 100%; margin: 0.4rem 0; }
  /* 토스트 */
  .toast-stack { position: fixed; right: 1rem; bottom: 1rem; display: flex; flex-direction: column; gap: 0.5rem; z-index: 100; max-width: min(420px, 90vw); }
  .toast-item { display: flex; align-items: center; gap: 0.5rem; background: #23272f; border: 1px solid #3a3f48; border-left: 4px solid #666; border-radius: 8px; padding: 0.55rem 0.7rem; box-shadow: 0 4px 16px rgba(0,0,0,0.4); animation: toast-in 0.15s ease-out; }
  .toast-item.ok { border-left-color: #4caf7a; }
  .toast-item.err { border-left-color: #d05555; }
  .toast-item.busy { border-left-color: #4a86d0; }
  .toast-item.info { border-left-color: #888; }
  .toast-msg { flex: 1; font-size: 0.85rem; word-break: break-all; }
  .toast-x { background: none; border: none; padding: 0 0.2rem; color: #888; font-size: 1rem; }
  .spinner { width: 14px; height: 14px; border: 2px solid #4a86d0; border-top-color: transparent; border-radius: 50%; animation: spin 0.8s linear infinite; flex-shrink: 0; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @keyframes toast-in { from { opacity: 0; transform: translateY(8px); } to { opacity: 1; transform: none; } }
</style>
