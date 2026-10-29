// Harness de medição (spikes 1–4), não é a UI do produto.
import { createSignal, createMemo, For, Show, onCleanup, onMount } from "solid-js";
import { createVirtualizer } from "@tanstack/solid-virtual";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Track = { uri: string; name: string; artist: string; album: string; duration_ms: number; cover64?: string; cover640?: string };
type Line = { t: number; text: string };

function parseLrc(lrc: string): Line[] {
  const out: Line[] = [];
  for (const raw of lrc.split("\n")) {
    const tags = [...raw.matchAll(/\[(\d+):(\d+(?:\.\d+)?)\]/g)];
    const text = raw.replace(/\[[^\]]*\]/g, "").trim();
    for (const m of tags) out.push({ t: (+m[1] * 60 + +m[2]) * 1000, text });
  }
  return out.sort((a, b) => a.t - b.t);
}

const fmt = (ms: number) => `${Math.floor(ms / 60000)}:${String(Math.floor(ms / 1000) % 60).padStart(2, "0")}`;

export default function App() {
  const [view, setView] = createSignal<"spike" | "library" | "immersive">("spike");
  const [user, setUser] = createSignal("");
  const [tracks, setTracks] = createSignal<Track[]>([]);
  const [current, setCurrent] = createSignal<Track>();
  const [playing, setPlaying] = createSignal(false);
  const [pos, setPos] = createSignal(0);
  const [lines, setLines] = createSignal<Line[]>([]);
  const [log, setLog] = createSignal("");
  const [busy, setBusy] = createSignal(false);
  const [clientId, setClientId] = createSignal((() => { try { return localStorage.getItem("clientId") ?? ""; } catch { return ""; } })());

  let base = 0, t0 = 0, raf = 0;
  const tick = () => { if (playing()) setPos(base + performance.now() - t0); raf = requestAnimationFrame(tick); };

  onMount(async () => {
    raf = requestAnimationFrame(tick);
    const unProgress = await listen<string>("progress", ({ payload }) => setLog(payload));
    const un = await listen<any>("player", ({ payload: e }) => {
      if (e.position_ms !== undefined) { base = e.position_ms; t0 = performance.now(); setPos(base); }
      if (e.kind === "playing") setPlaying(true);
      if (e.kind === "paused" || e.kind === "end") setPlaying(false);
    });
    onCleanup(() => { un(); unProgress(); cancelAnimationFrame(raf); });
  });

  const run = async (label: string, fn: () => Promise<unknown>) => {
    setBusy(true); setLog(`${label}…`);
    try { setLog(JSON.stringify(await fn(), null, 2)); } catch (e) { setLog(`erro: ${e}`); }
    setBusy(false);
  };

  const play = async (t: Track) => {
    setCurrent(t); setLines([]);
    await invoke("play", { uri: t.uri });
    const l = await invoke<any>("lyrics", { name: t.name, artist: t.artist, album: t.album, durationMs: t.duration_ms });
    setLines(l?.syncedLyrics ? parseLrc(l.syncedLyrics) : l?.plainLyrics ? l.plainLyrics.split("\n").map((text: string) => ({ t: Infinity, text })) : []);
  };

  const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
  const mark = (phase: string) => invoke("mark", { phase });
  let scroller: HTMLDivElement | undefined;

  // Um clique: login → spikes 2 e 4 → toca → biblioteca rolando 30 s → imersivo 30 s.
  const runAll = () => run("Rodar tudo", async () => {
    await mark("login"); setUser(await invoke<string>("login"));
    const id = clientId().trim();
    try { localStorage.setItem("clientId", id); } catch {}
    if (id) await invoke("web_login", { clientId: id });
    await mark("probes");
    const api = await invoke("probe_web_api");
    const t = await invoke<Track[]>("liked_tracks", { limit: 200 }); setTracks(t);
    const lrc = await invoke("probe_lrclib", { limit: 200 });
    // Sem Web API ainda dá para testar o áudio do librespot com uma faixa fixa.
    const first = t.find((x) => x.duration_ms > 120000) ?? t[0] ??
      { uri: "spotify:track:0DiWol3AO6WpXZgp0goxAV", name: "One More Time", artist: "Daft Punk", album: "Discovery", duration_ms: 320357 };
    await mark("play"); await play(first);
    await wait(10000);
    await mark("library"); setView("library"); await wait(500);
    for (let i = 0; i < 30; i++) { scroller?.scrollBy({ top: 1500 }); await wait(1000); }
    await mark("immersive"); setView("immersive"); await wait(30000);
    await mark("done");
    return { api, lrc };
  });

  // 5 000 linhas a partir das faixas reais, para medir virtualização + imagens.
  const library = createMemo(() => {
    const src = tracks();
    return src.length ? Array.from({ length: 5000 }, (_, i) => src[i % src.length]) : [];
  });

  const navBtn = (id: ReturnType<typeof view>, label: string) => (
    <button
      class={`w-full rounded-lg px-3 py-1.5 text-left text-sm ${view() === id ? "bg-white/10 text-ink-1" : "text-ink-2 hover:bg-white/5"}`}
      onClick={() => setView(id)}
    >{label}</button>
  );

  return (
    <div class="flex h-full">
      <Show when={view() !== "immersive"}>
        <aside class="flex w-56 shrink-0 flex-col gap-1 border-r border-glass-border bg-black/30 p-3">
          <p class="px-3 pb-2 text-xs text-ink-3">{user() ? `Conectado: ${user()}` : "Desconectado"}</p>
          {navBtn("spike", "Spikes")}
          {navBtn("library", "Biblioteca (5 000)")}
          {navBtn("immersive", "Imersivo")}
        </aside>
      </Show>

      <main class="relative min-w-0 flex-1">
        <Show when={view() === "spike"}>
          <div class="flex h-full flex-col gap-3 p-6">
            <label class="flex items-center gap-2 text-sm text-ink-2">
              Client ID (Web API)
              <input value={clientId()} onInput={(e) => setClientId(e.currentTarget.value)} placeholder="cole o Client ID do seu app no dashboard"
                class="w-96 rounded-lg bg-white/5 px-3 py-1.5 text-ink-1 ring-1 ring-glass-border outline-none focus:ring-accent" />
            </label>
            <div class="flex flex-wrap gap-2">
              <button disabled={busy()} class="rounded-full bg-accent px-4 py-1.5 text-sm font-semibold disabled:opacity-40" onClick={runAll}>Rodar tudo</button>
              {[
                ["Login", () => invoke<string>("login").then((u) => (setUser(u), u))],
                ["Carregar 200 curtidas", async () => {
                  if (!user()) setUser(await invoke<string>("login"));
                  if (clientId().trim()) await invoke("web_login", { clientId: clientId().trim() });
                  const t = await invoke<Track[]>("liked_tracks", { limit: 200 });
                  setTracks(t); setView("library");
                  return `${t.length} faixas`;
                }],
                ["Spike 2: Web API", () => invoke("probe_web_api")],
                ["Spike 4: LRCLIB (200)", () => invoke("probe_lrclib", { limit: 200 })],
              ].map(([label, fn]) => (
                <button disabled={busy()} class="rounded-full bg-white px-4 py-1.5 text-sm font-semibold text-black disabled:opacity-40"
                  onClick={() => run(label as string, fn as () => Promise<unknown>)}>{label as string}</button>
              ))}
              <button class="rounded-full bg-white/10 px-4 py-1.5 text-sm" onClick={() => invoke(playing() ? "pause" : "resume")}>
                {playing() ? "Pausar" : "Retomar"}
              </button>
            </div>
            <pre class="min-h-0 flex-1 overflow-auto rounded-row bg-black/40 p-4 text-xs text-ink-2 ring-1 ring-glass-border">{log()}</pre>
          </div>
        </Show>

        <Show when={view() === "library"}>
          <LibraryList items={library()} onPlay={play} current={current()?.uri} ref={(el) => (scroller = el)} />
        </Show>

        <Show when={view() === "immersive"}>
          <Immersive track={current()} lines={lines()} pos={pos()} />
        </Show>

        <PlayerBar track={current()} playing={playing()} pos={pos()} floating={view() === "immersive"}
          onToggle={() => invoke(playing() ? "pause" : "resume")} onExit={() => setView(view() === "immersive" ? "library" : "immersive")} />
      </main>
    </div>
  );
}

function LibraryList(p: { items: Track[]; onPlay: (t: Track) => void; current?: string; ref?: (el: HTMLDivElement) => void }) {
  let el!: HTMLDivElement;
  onMount(() => p.ref?.(el));
  const v = createVirtualizer({
    get count() { return p.items.length; },
    getScrollElement: () => el,
    estimateSize: () => 56,
    overscan: 8,
  });
  return (
    <div ref={el} class="h-full overflow-auto px-6 pt-6 pb-28">
      <Show when={p.items.length} fallback={<p class="text-ink-3">Faça login e carregue as curtidas na aba Spikes.</p>}>
        <div class="relative" style={{ height: `${v.getTotalSize()}px` }}>
          <For each={v.getVirtualItems()}>
            {(row) => {
              const t = () => p.items[row.index];
              return (
                <button
                  class={`absolute inset-x-0 flex h-[52px] items-center gap-3 rounded-row px-3 text-left hover:bg-white/5 ${p.current === t().uri ? "bg-white/10" : ""}`}
                  style={{ transform: `translateY(${row.start}px)` }}
                  onDblClick={() => p.onPlay(t())}
                >
                  <span class="w-10 text-right text-xs text-ink-3 tabular-nums">{row.index + 1}</span>
                  <img src={t().cover64} width="40" height="40" loading="lazy" decoding="async" class="size-10 rounded-md" alt="" />
                  <span class="min-w-0 flex-1">
                    <span class="block truncate text-sm">{t().name}</span>
                    <span class="block truncate text-xs text-ink-2">{t().artist} — {t().album}</span>
                  </span>
                  <span class="text-xs text-ink-3 tabular-nums">{fmt(t().duration_ms)}</span>
                </button>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
}

function Immersive(p: { track?: Track; lines: Line[]; pos: number }) {
  const idx = createMemo(() => {
    let i = -1;
    for (let k = 0; k < p.lines.length && p.lines[k].t <= p.pos; k++) i = k;
    return i;
  });
  return (
    <div class="absolute inset-0 overflow-hidden">
      {/* doc 03 regra 3: capa pequena + blur forte para o fundo */}
      <img src={p.track?.cover64} alt="" class="absolute inset-0 size-full scale-125 object-cover blur-3xl saturate-150" />
      <div class="absolute inset-0 bg-black/45" />
      <div class="absolute inset-y-10 left-10 flex w-[38%] items-center">
        <img src={p.track?.cover640} alt="" class="aspect-square w-full rounded-card object-cover shadow-2xl" />
      </div>
      <section class="absolute inset-y-10 right-10 w-[48%] overflow-hidden rounded-card bg-glass ring-1 ring-glass-border backdrop-blur-2xl backdrop-saturate-150">
        <Show when={p.lines.length} fallback={<p class="p-12 text-2xl font-bold text-ink-3">Sem letra</p>}>
          {/* top-[30%] é relativo ao cartão; translateY em % seria relativo à lista inteira */}
          <div class="absolute inset-x-0 top-[30%] px-12 transition-transform duration-500 ease-out" style={{ transform: `translateY(-${Math.max(idx(), 0) * 84}px)` }}>
            <For each={p.lines}>
              {(l, i) => {
                const d = () => Math.abs(i() - idx());
                return (
                  <p class="flex min-h-[84px] items-center text-4xl leading-tight font-extrabold transition-all duration-500"
                    style={{ opacity: d() === 0 ? 1 : Math.max(0.12, 0.45 - d() * 0.08), filter: `blur(${d() === 0 ? 0 : Math.min(d() * 0.8, 4)}px)` }}>
                    {l.text || "…"}
                  </p>
                );
              }}
            </For>
          </div>
        </Show>
      </section>
    </div>
  );
}

function PlayerBar(p: { track?: Track; playing: boolean; pos: number; floating: boolean; onToggle: () => void; onExit: () => void }) {
  const dur = () => p.track?.duration_ms ?? 0;
  return (
    <div class={`absolute bottom-4 left-1/2 flex w-[min(900px,92%)] -translate-x-1/2 items-center gap-4 rounded-full bg-glass px-5 py-2 ring-1 ring-glass-border backdrop-blur-2xl ${p.floating ? "" : "shadow-xl"}`}>
      <button class="text-xl" onClick={p.onToggle} aria-label={p.playing ? "Pausar" : "Tocar"}>{p.playing ? "❚❚" : "▶"}</button>
      <span class="text-xs text-ink-2 tabular-nums">{fmt(p.pos)} / -{fmt(Math.max(dur() - p.pos, 0))}</span>
      <div class="min-w-0 flex-1 rounded-lg bg-white/5 px-3 py-1">
        <p class="truncate text-sm">{p.track?.name ?? "Nada tocando"}</p>
        <p class="truncate text-xs text-ink-2">{p.track ? `${p.track.album} – ${p.track.artist}` : ""}</p>
        <div class="mt-1 h-0.5 rounded bg-white/10"><div class="h-full rounded bg-ink-2" style={{ width: `${dur() ? (p.pos / dur()) * 100 : 0}%` }} /></div>
      </div>
      <button class="text-sm text-ink-2" onClick={p.onExit}>{p.floating ? "Sair" : "Imersivo"}</button>
    </div>
  );
}
