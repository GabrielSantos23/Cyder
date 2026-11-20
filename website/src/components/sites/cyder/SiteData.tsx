"use client";

import { useEffect } from "react";

const REPO = "GabrielSantos23/Cyder";

type Asset = { name: string; size: number; browser_download_url: string };
type Release = { tag_name?: string; published_at?: string; assets?: Asset[] };
type UAData = { platform?: string; getHighEntropyValues?: (h: string[]) => Promise<{ architecture?: string }> };

const all = (s: string) => document.querySelectorAll<HTMLElement>(s);
const gh = <T,>(path: string): Promise<T> =>
  fetch(`https://api.github.com/repos/${REPO}${path}`, { headers: { Accept: "application/vnd.github+json" } }).then((r) =>
    r.ok ? r.json() : Promise.reject(r.status),
  );

/* Fills in live GitHub data (stars, latest release, sizes), marks the visitor's OS and wires the copy button. */
export function SiteData() {
  useEffect(() => {
    gh<{ stargazers_count?: number }>("")
      .then((r) => {
        if (typeof r.stargazers_count === "number") all("[data-stars]").forEach((el) => (el.textContent = r.stargazers_count!.toLocaleString()));
      })
      .catch(() => {});

    const uaData = (navigator as Navigator & { userAgentData?: UAData }).userAgentData;
    const ua = uaData?.platform ?? navigator.platform ?? navigator.userAgent;
    const os = /iPhone|iPad|Android/i.test(navigator.userAgent) ? null : /mac/i.test(ua) ? "macos" : /win/i.test(ua) ? "windows" : /linux|x11/i.test(ua) ? "linux" : null;
    // Only Windows ships today; macOS and Linux visitors keep the plain "Download" label.
    if (os === "windows") {
      all("[data-os-label]").forEach((el) => (el.textContent = "Download for Windows"));
      all('[data-os="windows"]').forEach((el) => el.setAttribute("data-yours", ""));
    }

    gh<Release>("/releases/latest")
      .then((rel) => {
        if (!rel.tag_name) return;
        all("[data-version]").forEach((el) => (el.textContent = rel.tag_name!));
        const assets = rel.assets ?? [];
        const find = (re: RegExp) => assets.find((a) => re.test(a.name));
        const show = (platform: string, file: Asset | undefined) => {
          if (!file) return;
          all(`[data-os="${platform}"]`).forEach((el) => ((el as HTMLAnchorElement).href = file.browser_download_url));
          all(`[data-size="${platform}"]`).forEach((el) => (el.textContent = `${(file.size / 1048576).toFixed(1)} MB`));
        };
        // Only Windows ships today: a single cyder.exe per release.
        show("windows", find(/^cyder\.exe$/i));
        if (rel.published_at) {
          const d = new Date(rel.published_at).toLocaleDateString("en-US", { month: "long", day: "numeric", year: "numeric" });
          all("[data-date]").forEach((el) => (el.textContent = d));
        }
      })
      .catch(() => {});

    const copyButtons = [...all("[data-copy]")];
    const onCopy = async (e: Event) => {
      const b = e.currentTarget as HTMLButtonElement;
      const text = document.querySelector<HTMLElement>(b.dataset.copy!)?.dataset.text;
      if (!text) return;
      try {
        await navigator.clipboard.writeText(text);
        b.textContent = "Copied";
      } catch {
        b.textContent = "Select and copy";
      }
      setTimeout(() => (b.textContent = "Copy"), 1800);
    };
    copyButtons.forEach((b) => b.addEventListener("click", onCopy));
    return () => copyButtons.forEach((b) => b.removeEventListener("click", onCopy));
  }, []);
  return null;
}
