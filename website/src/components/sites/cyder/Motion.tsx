"use client";

import { useEffect } from "react";

const clamp = (v: number) => Math.min(1, Math.max(0, v));

/* Scroll and motion layer:
   [data-reveal]   gets data-in once it scrolls into view (masked text and drop-ins key off it)
   [data-progress] gets --p (0..1): "pin" = progress through a sticky track, otherwise progress of the element across the viewport
   [data-wave]     lays its [data-tile] children along a moving sine wave */
export function Motion() {
  useEffect(() => {
    const reduce = matchMedia("(prefers-reduced-motion: reduce)").matches;

    const io = new IntersectionObserver(
      (entries) =>
        entries.forEach((e) => {
          if (!e.isIntersecting) return;
          e.target.setAttribute("data-in", "");
          io.unobserve(e.target);
        }),
      { threshold: 0.25 },
    );
    document.querySelectorAll("[data-reveal]").forEach((el) => io.observe(el));

    const tracks = [...document.querySelectorAll<HTMLElement>("[data-progress]")];
    const waves = [...document.querySelectorAll<HTMLElement>("[data-wave]")].map((el) => ({
      el,
      tiles: [...el.querySelectorAll<HTMLElement>("[data-tile]")],
      gap: Number(el.dataset.wave) || 88,
    }));

    let raf = 0;
    const frame = (t: number) => {
      const vh = innerHeight;
      for (const el of tracks) {
        const r = el.getBoundingClientRect();
        if (r.bottom < -vh || r.top > 2 * vh) continue;
        const p = el.dataset.progress === "pin" ? -r.top / Math.max(1, r.height - vh) : (vh - r.top) / (vh + r.height);
        el.style.setProperty("--p", clamp(p).toFixed(4));
      }
      for (const { el, tiles, gap } of waves) {
        const r = el.getBoundingClientRect();
        if (r.bottom < 0 || r.top > vh) continue;
        const w = r.width;
        const loop = tiles.length * gap;
        const shift = reduce ? 0 : (t * 0.045) % loop;
        const phase = reduce ? 0 : t / 1800;
        tiles.forEach((tile, i) => {
          const x = ((i * gap + shift) % loop) - gap;
          const a = (x / w) * Math.PI * 2 + phase;
          tile.style.transform = `translate3d(${x}px, ${Math.sin(a) * 38}px, 0) rotate(${Math.cos(a) * 9}deg)`;
        });
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);

    return () => {
      cancelAnimationFrame(raf);
      io.disconnect();
    };
  }, []);
  return null;
}
