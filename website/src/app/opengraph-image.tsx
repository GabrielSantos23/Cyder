import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { ImageResponse } from "next/og";

export const alt = "Cyder: Your Spotify, without the browser. A native Spotify app for Windows with word-by-word synced lyrics.";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

const dataUrl = (buf: Buffer, type: string) => `data:${type};base64,${buf.toString("base64")}`;

export default async function Image() {
  // Literal paths so the bundler can trace exactly these files.
  const [anton, inter, iconPng, shotJpg] = await Promise.all([
    readFile(join(process.cwd(), "assets", "Anton-Regular.ttf")),
    readFile(join(process.cwd(), "assets", "Inter-Medium.ttf")),
    readFile(join(process.cwd(), "public", "sites", "am-rs", "icons", "icon-192.png")),
    readFile(join(process.cwd(), "assets", "og-shot.jpg")),
  ]);
  const icon = dataUrl(iconPng, "image/png");
  const shot = dataUrl(shotJpg, "image/jpeg");

  const pill = (bg: string, color: string, border = bg) =>
    ({ display: "flex", padding: "10px 22px", borderRadius: 999, background: bg, color, border: `2px solid ${border}`, fontSize: 22 }) as const;

  return new ImageResponse(
    (
      <div style={{ width: "100%", height: "100%", display: "flex", position: "relative", background: "#020203", fontFamily: "Inter", color: "white", overflow: "hidden" }}>
        {/* lime → lilac glow behind the app window */}
        <div style={{ position: "absolute", left: 640, top: 360, width: 720, height: 260, borderRadius: 999, background: "linear-gradient(90deg, #bcf66e, #7c3aed 55%, #de9efc)", opacity: 0.45, filter: "blur(60px)" }} />
        <div style={{ position: "absolute", left: 600, top: 120, display: "flex", transform: "rotate(-5deg)", borderRadius: 26, border: "8px solid #35264a", overflow: "hidden", boxShadow: "0 30px 80px rgba(0,0,0,0.6)" }}>
          <img src={shot} alt="" width={640} height={360} style={{ objectFit: "cover" }} />
        </div>

        <div style={{ display: "flex", flexDirection: "column", padding: "64px 0 0 72px", width: 640 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <img src={icon} alt="" width={52} height={52} />
            <span style={{ fontFamily: "Anton", fontSize: 40 }}>cyder</span>
          </div>
          <div style={{ display: "flex", flexDirection: "column", marginTop: 44, fontFamily: "Anton", fontSize: 92, lineHeight: 1 }}>
            <span style={{ color: "rgba(255,255,255,0.3)" }}>Your Spotify.</span>
            <span>Without The</span>
            <span>Browser.</span>
          </div>
          <div style={{ display: "flex", marginTop: 26, fontSize: 25, lineHeight: 1.4, color: "rgba(255,255,255,0.75)", width: 500 }}>
            A native Spotify app in Rust, with word-by-word synced lyrics and romaji.
          </div>
          <div style={{ display: "flex", gap: 12, marginTop: 30 }}>
            <div style={pill("#bcf66e", "#000")}>Windows</div>
            <div style={pill("#de9efc", "#000")}>macOS &amp; Linux soon</div>
          </div>
        </div>
      </div>
    ),
    {
      ...size,
      fonts: [
        { name: "Anton", data: anton, weight: 400, style: "normal" },
        { name: "Inter", data: inter, weight: 500, style: "normal" },
      ],
    },
  );
}
