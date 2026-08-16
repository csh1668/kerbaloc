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
        <tr>
          <td title={u.mod_id}>{u.display_name}</td>
          <td>{u.version ?? "-"}</td>
          <td class="num">{u.keys}</td>
          <td>
            {#if u.installed}<span class="badge ok">설치됨</span>
            {:else if variants.length > 0}<span class="badge db">DB {variants.length}변형{fresh ? "" : " (버전 다름)"}</span>
            {:else}<span class="badge none">미번역</span>{/if}
          </td>
          <td class="actions">
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
</style>
