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
  }
  interface DbVariant {
    variantId: string;
    srcSha256: string;
    keysTranslated: number;
    keysTarget: number;
  }
  interface ReviewItem {
    key: string;
    en: string;
    candidates: string[];
    violations: string[];
    edited?: string;
  }

  let status = $state<{ root: string; language: string | null } | null>(null);
  let units = $state<Unit[]>([]);
  let dbPacks = $state<Map<string, DbVariant[]>>(new Map());
  let loading = $state("");
  let error = $state("");
  let toast = $state("");

  // 번역 모달 상태
  let translating = $state<string | null>(null);
  let progress = $state({ done: 0, total: 0, cost: 0 });
  let result = $state<{
    ok: number;
    review: ReviewItem[];
    failed: number;
    cost: number;
    pack_dir: string;
  } | null>(null);

  // 키 에디터
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

  // 설정
  let showSettings = $state(false);
  let settings = $state<{ ksp_root: string | null; gemini_api_key: string | null; nick: string | null }>({
    ksp_root: null,
    gemini_api_key: null,
    nick: null,
  });

  async function refresh() {
    loading = "스캔 중…";
    error = "";
    try {
      status = await invoke("game_status");
      units = await invoke("scan_units");
      try {
        const idx: any = await invoke("db_index");
        dbPacks = new Map(idx.packs.map((p: any) => [p.modId, p.variants]));
      } catch (e) {
        // DB 접근 실패는 치명적이지 않음 (오프라인)
        console.warn("DB 인덱스 실패", e);
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = "";
    }
  }

  async function toggleLanguage() {
    const target = status?.language === "ko" ? "en-us" : "ko";
    await invoke("set_language", { lang: target });
    status = await invoke("game_status");
    toast = `언어: ${target} (게임 재시작 필요)`;
  }

  async function installFromDb(modId: string) {
    loading = `${modId} 설치 중…`;
    try {
      await invoke("install_from_db", { modId, variant: null });
      toast = `${modId} 설치 완료`;
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      loading = "";
    }
  }

  async function removePack(modId: string) {
    await invoke("remove_pack", { modId });
    toast = `${modId} 제거됨`;
    await refresh();
  }

  async function translate(modId: string) {
    translating = modId;
    progress = { done: 0, total: 0, cost: 0 };
    result = null;
    error = "";
    try {
      result = await invoke("translate_mod", { modId });
    } catch (e) {
      error = String(e);
      translating = null;
    }
  }

  async function installResult() {
    if (!result) return;
    await invoke("install_local_pack", { packDir: result.pack_dir });
    toast = "번역 팩 설치 완료 (게임 재시작 필요)";
    translating = null;
    result = null;
    await refresh();
  }

  async function shareResult() {
    if (!result) return;
    loading = "공유 중…";
    try {
      const prUrl: string = await invoke("share_pack_cmd", { packDir: result.pack_dir });
      toast = `공유 완료! ${prUrl}`;
    } catch (e) {
      error = String(e);
    } finally {
      loading = "";
    }
  }

  async function openKeyEditor(modId: string) {
    loading = `${modId} 키 로드 중…`;
    keyErrors = [];
    keyFilter = "";
    keyLimit = 300;
    try {
      const keys: ModKey[] = await invoke("get_mod_keys", { modId });
      keyEditor = { modId, keys };
    } catch (e) {
      error = String(e);
    } finally {
      loading = "";
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
      // 현재 값(기존 번역 유지 + 편집 반영) 전체를 보낸다
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
        toast = `${keyEditor.modId} 저장·설치 완료 (${edits.length}키, 게임 재시작 필요)`;
        keyEditor = null;
        await refresh();
      }
    } catch (e) {
      keyErrors = [String(e)];
    } finally {
      keySaving = false;
    }
  }

  async function saveSettings() {
    await invoke("save_settings", { settings });
    showSettings = false;
    toast = "설정 저장됨";
    await refresh();
  }

  onMount(async () => {
    settings = await invoke("load_settings");
    await listen<{ done: number; total: number; cost: number }>("translate-progress", (e) => {
      progress = e.payload;
    });
    await refresh();
  });
</script>

<main>
  <header>
    <h1>KerbaLoc 스튜디오</h1>
    <div class="header-actions">
      {#if status}
        <span class="root" title={status.root}>KSP 감지됨</span>
        <button class="lang" onclick={toggleLanguage}>
          한국어 {status.language === "ko" ? "ON" : "OFF"}
        </button>
      {/if}
      <button onclick={() => (showSettings = true)}>설정</button>
      <button onclick={refresh}>새로고침</button>
    </div>
  </header>

  {#if error}<div class="error">{error} <button onclick={() => (error = "")}>×</button></div>{/if}
  {#if toast}<div class="toast">{toast} <button onclick={() => (toast = "")}>×</button></div>{/if}
  {#if loading}<div class="loading">{loading}</div>{/if}

  <table>
    <thead>
      <tr><th>모드</th><th>버전</th><th>키수</th><th>상태</th><th>액션</th></tr>
    </thead>
    <tbody>
      {#each units as u (u.mod_id)}
        {@const variants = dbPacks.get(u.mod_id) ?? []}
        {@const fresh = variants.some((v) => v.srcSha256 === u.source_hash)}
        <tr class="row" onclick={() => openKeyEditor(u.mod_id)}>
          <td title={u.mod_id}>{u.display_name}</td>
          <td>{u.version ?? "-"}</td>
          <td class="num">{u.keys}</td>
          <td>
            {#if u.installed}<span class="badge ok">설치됨</span>
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
            <button onclick={() => translate(u.mod_id)}>번역</button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</main>

{#if translating}
  <div class="modal-backdrop">
    <div class="modal">
      <h2>{translating} 번역</h2>
      {#if !result}
        <p>배치 {progress.done}/{progress.total} — 누적 ${progress.cost.toFixed(4)}</p>
        <progress value={progress.done} max={progress.total || 1}></progress>
      {:else}
        <p>
          완료: <b>{result.ok}</b>키 번역 / 검수 필요 {result.review.length} / 실패 {result.failed}
          — 비용 ${result.cost.toFixed(4)}
        </p>
        {#if result.review.length > 0}
          <div class="review">
            {#each result.review as r (r.key)}
              <div class="review-item">
                <div class="key">{r.key}</div>
                <div class="en">{r.en}</div>
                <div class="violations">{r.violations.join(" / ")}</div>
                <input placeholder={r.candidates.at(-1) ?? "번역 입력"} bind:value={r.edited} />
              </div>
            {/each}
            <p class="hint">검수 항목은 v1에서는 팩에서 제외됩니다 (영어 폴백 — 안전).</p>
          </div>
        {/if}
        <div class="modal-actions">
          <button class="primary" onclick={installResult}>게임에 설치</button>
          <button onclick={shareResult}>공유 (익명 PR)</button>
          <button onclick={() => { translating = null; result = null; }}>닫기</button>
        </div>
      {/if}
    </div>
  </div>
{/if}

{#if keyEditor}
  {@const shown = filteredKeys()}
  <div class="modal-backdrop">
    <div class="modal wide">
      <h2>{keyEditor.modId} — 번역 키 ({keyEditor.keys.length}개)</h2>
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
      <label>Gemini API 키 (번역 시에만 필요)
        <input type="password" bind:value={settings.gemini_api_key} />
      </label>
      <label>닉네임 (공유 시 표시)
        <input bind:value={settings.nick} placeholder="anon" />
      </label>
      <div class="modal-actions">
        <button class="primary" onclick={saveSettings}>저장</button>
        <button onclick={() => (showSettings = false)}>취소</button>
      </div>
    </div>
  </div>
{/if}

<style>
  :global(body) { margin: 0; font-family: "Segoe UI", "Malgun Gothic", sans-serif; background: #14161a; color: #e6e6e6; }
  main { max-width: 960px; margin: 0 auto; padding: 1rem; }
  header { display: flex; justify-content: space-between; align-items: center; }
  h1 { font-size: 1.3rem; }
  .header-actions { display: flex; gap: 0.5rem; align-items: center; }
  .root { color: #7a8; font-size: 0.85rem; }
  button { background: #2a2e35; color: #e6e6e6; border: 1px solid #444; border-radius: 6px; padding: 0.35rem 0.8rem; cursor: pointer; }
  button:hover { background: #353a43; }
  button.primary { background: #2b5cab; border-color: #3a6fc4; }
  button.lang { background: #244; }
  table { width: 100%; border-collapse: collapse; margin-top: 1rem; }
  th, td { text-align: left; padding: 0.45rem 0.6rem; border-bottom: 1px solid #2a2e35; font-size: 0.9rem; }
  td.num { text-align: right; }
  td.actions { display: flex; gap: 0.3rem; }
  .badge { padding: 0.15rem 0.5rem; border-radius: 999px; font-size: 0.78rem; }
  .badge.ok { background: #1d4028; color: #8fdba3; }
  .badge.db { background: #1d3350; color: #8fbadb; }
  .badge.none { background: #333; color: #999; }
  .error { background: #4a1d1d; padding: 0.6rem; border-radius: 6px; margin-top: 0.6rem; white-space: pre-wrap; }
  .toast { background: #1d4028; padding: 0.6rem; border-radius: 6px; margin-top: 0.6rem; word-break: break-all; }
  .loading { color: #8fbadb; margin-top: 0.6rem; }
  .modal-backdrop { position: fixed; inset: 0; background: rgba(0,0,0,0.6); display: flex; align-items: center; justify-content: center; }
  .modal { background: #1b1e24; border: 1px solid #333; border-radius: 10px; padding: 1.2rem; width: min(640px, 90vw); max-height: 85vh; overflow-y: auto; }
  progress { width: 100%; }
  label { display: block; margin: 0.6rem 0; font-size: 0.9rem; }
  label input { width: 100%; box-sizing: border-box; margin-top: 0.25rem; background: #14161a; color: #e6e6e6; border: 1px solid #444; border-radius: 6px; padding: 0.4rem; }
  .modal-actions { display: flex; gap: 0.5rem; margin-top: 1rem; }
  .review { border: 1px solid #333; border-radius: 8px; padding: 0.6rem; margin-top: 0.6rem; }
  .review-item { border-bottom: 1px solid #2a2e35; padding: 0.5rem 0; }
  .review-item .key { font-family: monospace; color: #8fbadb; font-size: 0.8rem; }
  .review-item .en { margin: 0.2rem 0; }
  .review-item .violations { color: #db8f8f; font-size: 0.8rem; }
  .review-item input { width: 100%; box-sizing: border-box; background: #14161a; color: #e6e6e6; border: 1px solid #444; border-radius: 6px; padding: 0.35rem; }
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
</style>
