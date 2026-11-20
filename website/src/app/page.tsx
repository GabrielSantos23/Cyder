import type { CSSProperties, ReactNode } from "react";
import type { Metadata } from "next";
import { CassetteTape, ChevronRight, HandMetal, Headphones, Mic, Minus, Plus, Sparkle } from "lucide-react";
import { Motion } from "@/components/sites/cyder/Motion";
import { SiteData } from "@/components/sites/cyder/SiteData";
import { PhysicsPills, type PhysicsItem } from "@/components/sites/cyder/PhysicsPills";

export const metadata: Metadata = {
  title: "Cyder: a fast, native Spotify desktop app for Windows, with macOS and Linux coming soon",
  description:
    "Cyder is a free, open-source Spotify desktop client written in Rust for Windows, with macOS and Linux coming soon. Synced lyrics, Apple Music in beta and no Electron, in a 34 MB download.",
  keywords: [
    "Spotify desktop app",
    "Spotify client",
    "Spotify Windows",
    "Spotify Mac",
    "Spotify Linux",
    "native Spotify player",
    "Rust music player",
    "GPUI",
    "Apple Music beta",
    "synced lyrics",
    "Electron alternative",
    "open source",
  ],
  robots: { index: true, follow: true },
};

const REPO_URL = "https://github.com/GabrielSantos23/Cyder";
const RELEASES_URL = `${REPO_URL}/releases/latest`;
// Direct link to the newest Windows build; GitHub redirects /latest/download/<asset> to the current release.
const WINDOWS_URL = `${RELEASES_URL}/download/cyder.exe`;
const TILE = "/sites/am-rs/img/tiles";
const ICON = "/sites/am-rs/icons/icon-192.png";

const wrap = "mx-auto w-full max-w-6xl px-4 sm:px-6 lg:px-8";
const display = "font-heading leading-[0.95] tracking-tight";
const bezel = "overflow-hidden rounded-[22px] border-[6px] border-[#35264a] bg-panel shadow-[0_30px_80px_rgba(0,0,0,0.6)]";

/* Masked line that slides up into place when it scrolls into view. */
function Reveal({ children, className = "", delay = 0 }: { children: ReactNode; className?: string; delay?: number }) {
  return (
    <span data-reveal className={`group/r block overflow-hidden pb-[0.06em] ${className}`}>
      <span
        className="block translate-y-[110%] transition-transform duration-[900ms] ease-[cubic-bezier(.2,.8,.2,1)] group-data-in/r:translate-y-0"
        style={{ transitionDelay: `${delay}ms` }}
      >
        {children}
      </span>
    </span>
  );
}

/* Each letter rises on its own, staggered. */
function Letters({ text, delay = 0, className = "" }: { text: string; delay?: number; className?: string }) {
  return (
    <span data-reveal className={`group/r block overflow-hidden pb-[0.06em] ${className}`}>
      {[...text].map((c, i) => (
        <span
          key={i}
          className="inline-block translate-y-[110%] rotate-6 transition-transform duration-[800ms] ease-[cubic-bezier(.2,.9,.25,1.15)] group-data-in/r:translate-y-0 group-data-in/r:rotate-0"
          style={{ transitionDelay: `${delay + i * 45}ms` }}
        >
          {c === " " ? "\u00a0" : c}
        </span>
      ))}
    </span>
  );
}

// Album-art fill that slowly pans; a thin white outline keeps the letters legible over dark art.
const artText =
  "animate-pan bg-[url(/sites/am-rs/img/tiles/collage.webp)] [background-size:170%] bg-clip-text text-transparent brightness-150 saturate-150 [-webkit-text-stroke:1.5px_rgba(255,255,255,0.8)]";

/* A glowing lime-to-lilac ring with cards circling it in 3D. Sizes are in design pixels. */
function Orbit({ cards, radius, w, h, ring }: { cards: string[]; radius: number; w: number; h: number; ring: number }) {
  const conic = "bg-[conic-gradient(from_210deg,#bcf66e,#8fd14f_18%,#7c3aed_45%,#de9efc_68%,#bcf66e)]";
  return (
    <div aria-hidden="true" className="absolute inset-0 [transform-style:preserve-3d]">
      <div className="absolute top-1/2 left-1/2" style={{ width: ring, height: ring, transform: `translate(-50%, -50%) rotateX(-9deg) translateY(${h / 2 + 28}px) rotateX(90deg)` }}>
        <div className={`absolute inset-0 rounded-full ${conic} opacity-70 blur-2xl [mask:radial-gradient(closest-side,transparent_76%,#000_88%,transparent_100%)]`} />
        <div className={`absolute inset-0 rounded-full ${conic} [mask:radial-gradient(closest-side,transparent_88%,#000_91%,#000_95%,transparent_99%)]`} />
      </div>
      <div className="absolute top-1/2 left-1/2 animate-orbit [transform-style:preserve-3d]">
        {cards.map((src, i) => {
          const a = (i * 360) / cards.length;
          return (
            <div
              key={src + i}
              className="absolute [transform-style:preserve-3d]"
              style={{ width: w, height: h, left: -w / 2, top: -h / 2, transform: `rotateY(${a}deg) translateZ(${radius}px)` }}
            >
              <img
                src={src}
                alt=""
                loading="lazy"
                width={w}
                height={h}
                className="size-full max-w-none animate-billboard rounded-xl object-cover shadow-[0_20px_50px_rgba(0,0,0,0.55)]"
                style={{ "--a": `${a}deg` } as CSSProperties}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}

function Vinyl({ label, tone, style }: { label: string; tone: string; style?: CSSProperties }) {
  return (
    <div aria-hidden="true" className="absolute bottom-0 left-1/2 aspect-square w-[min(78vw,560px)]" style={style}>
      <div className="size-full animate-spin-slow rounded-full bg-[repeating-radial-gradient(circle,#0b0b0c_0_3px,#1d1d20_4px_7px)] shadow-[0_-20px_60px_rgba(0,0,0,0.25)]">
        <div className={`absolute inset-[33%] grid place-items-center rounded-full ${tone} ring-4 ring-black/80`}>
          <span className="font-heading text-[clamp(1rem,2.6vw,1.75rem)] text-black">{label}</span>
          <span className="absolute top-[62%] size-2 rounded-full bg-black" />
        </div>
      </div>
    </div>
  );
}

function Sticker({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <span aria-hidden="true" className={`inline-grid place-items-center rounded-2xl bg-white shadow-[0_8px_24px_rgba(0,0,0,0.35)] ring-4 ring-white ${className}`}>
      {children}
    </span>
  );
}

function Toggle({ dark = false }: { dark?: boolean }) {
  return (
    <span
      className={`grid size-6 shrink-0 place-items-center rounded-full bg-lime text-black transition-transform duration-300 group-open/d:rotate-180 ${dark ? "" : "group-open/d:bg-black group-open/d:text-lime"}`}
    >
      <Plus className="size-4 group-open/d:hidden" strokeWidth={2.5} aria-hidden="true" />
      <Minus className="hidden size-4 group-open/d:block" strokeWidth={2.5} aria-hidden="true" />
    </span>
  );
}

const CARD = "/sites/am-rs/img/cards";
// Real Cyder screens orbiting the hero video.
const heroCards = [
  `${CARD}/artist-drake.webp`,
  `${CARD}/now-playing-mj.webp`,
  `${CARD}/home-queue.webp`,
  `${CARD}/album-dangerous.webp`,
  `${CARD}/artist-ariana-grande.webp`,
  `${CARD}/playlist-all-out-2000s.webp`,
];

type Tile = { img?: string; icon?: ReactNode; bg?: string };
const ART = "/sites/am-rs/img/artists";
const tileSet: Tile[] = [
  { img: `${ART}/billie-eilish.webp` },
  { img: `${ART}/the-weeknd.webp` },
  { img: `${ART}/dua-lipa.webp` },
  { icon: <Headphones className="size-7" />, bg: "bg-[#6d5cf6] text-white" },
  { img: `${ART}/bruno-mars.webp` },
  { img: `${ART}/taylor-swift.webp` },
  { img: ICON, bg: "bg-lime" },
  { img: `${ART}/adam-levine.webp` },
  { img: `${ART}/lady-gaga.webp` },
  { img: `${ART}/bad-bunny.webp` },
  { img: `${ART}/olivia-rodrigo.webp` },
  { img: `${ART}/ed-sheeran.webp` },
];
const tiles = [...tileSet, ...tileSet];

const features = [
  {
    k: "home",
    title: "Your home and queue",
    body: "Your home feed with Spotify's mixes and playlists, and a queue you reorder by dragging, with shuffle, repeat and a smart shuffle that mixes recommendations in.",
    src: `${CARD}/home-queue-1280.webp`,
    alt: "Cyder home page with Spotify playlists, and the queue open beside it",
  },
  {
    k: "artists",
    title: "Artist pages",
    body: "Top songs, the latest release and the whole discography, filtered by albums or by singles and EPs, with play and shuffle one click away.",
    src: `${CARD}/artist-drake-1280.webp`,
    alt: "Drake's artist page in Cyder with the latest release, most played songs and discography",
  },
  {
    k: "playlists",
    title: "Albums and playlists",
    body: "Every album and playlist in your library opens as a full page, with its track count, running time, and play or shuffle at the top.",
    src: `${CARD}/playlist-all-out-2000s-1280.webp`,
    alt: "The All Out 2000s playlist in Cyder, with 150 tracks",
  },
  {
    k: "lyrics",
    title: "Synced lyrics",
    body: "Lyrics come from Apple Music, Spotify and LRCLIB, and the highlight follows the singer's voice word by word instead of jumping from line to line.",
    src: "/sites/am-rs/video/now-playing-poster.webp",
    alt: "Cyder playing This Love by Maroon 5, with synced lyrics over the album art",
  },
  {
    k: "together",
    title: "Listen together, on any device",
    body: "Invite someone with a code and they hear what you play, each on their own account. Cyder also lists your Spotify Connect devices and hands playback to any of them.",
    src: `${CARD}/album-dangerous-1280.webp`,
    alt: "The Dangerous album page by Michael Jackson in Cyder",
  },
];
// Literal class names so Tailwind can see them: each screenshot shows while its accordion item is open.
const showWhenOpen: Record<string, string> = {
  home: "group-has-[[data-k=home][open]]/f:opacity-100",
  artists: "group-has-[[data-k=artists][open]]/f:opacity-100",
  playlists: "group-has-[[data-k=playlists][open]]/f:opacity-100",
  lyrics: "group-has-[[data-k=lyrics][open]]/f:opacity-100",
  together: "group-has-[[data-k=together][open]]/f:opacity-100",
};

const floaters: { src?: string; icon?: ReactNode; pos: string; speed: number; rot: number; size: string }[] = [
  { src: `${TILE}/mix0.webp`, pos: "left-[8%] top-[18%]", speed: 160, rot: -8, size: "w-24 sm:w-32" },
  { src: `${TILE}/xscape.webp`, pos: "left-[18%] top-[8%] max-sm:hidden", speed: 90, rot: 6, size: "w-28" },
  { src: `${TILE}/idol.webp`, pos: "left-[44%] top-[2%]", speed: 220, rot: 0, size: "w-20 sm:w-24" },
  { src: `${TILE}/mix4.webp`, pos: "right-[10%] top-[12%]", speed: 120, rot: 10, size: "w-28 sm:w-36" },
  { src: `${TILE}/mix2.webp`, pos: "left-[6%] bottom-[16%]", speed: 200, rot: -6, size: "w-24 sm:w-28" },
  { src: `${TILE}/mix5.webp`, pos: "right-[8%] bottom-[18%]", speed: 140, rot: 4, size: "w-24 sm:w-32" },
  { src: `${TILE}/mix3.webp`, pos: "right-[24%] bottom-[4%] max-sm:hidden", speed: 260, rot: 8, size: "w-28" },
  { icon: <Sparkle className="size-14 fill-[#67e8f9] text-[#67e8f9] drop-shadow-[0_0_12px_#67e8f9]" />, pos: "left-[22%] top-[52%]", speed: 180, rot: 0, size: "" },
  { icon: <Sticker className="size-20 -rotate-6"><HandMetal className="size-11 text-black" /></Sticker>, pos: "left-[12%] bottom-[6%]", speed: 300, rot: 0, size: "" },
  { icon: <Sticker className="size-16 rotate-12"><CassetteTape className="size-10 text-[#f97316]" /></Sticker>, pos: "right-[14%] top-[34%] max-sm:hidden", speed: 240, rot: 0, size: "" },
  { icon: <Sticker className="size-14 -rotate-12"><Mic className="size-8 text-[#7c3aed]" /></Sticker>, pos: "left-[34%] top-[14%] max-sm:hidden", speed: 130, rot: 0, size: "" },
];

const notes = [
  ["soloist", "Spotify's own player", "Spotify songs play through Soloist, Spotify's official player, with a key you create once using your Premium account. librespot still handles the session: playlists, lyrics and metadata."],
  ["musickit", "Apple Music, played by Apple", "The Apple Music beta runs Apple's MusicKit player in a hidden WebView2. Each song is matched by its ISRC in your account's store; the first one starts in about 3.5 seconds and the next ones in about 1.3."],
  ["gpui", "Drawn on the GPU", "The interface is GPUI, the framework behind the Zed editor, in a fork with real background blur. Every pixel in the window is drawn natively, with no browser engine behind it."],
  ["lyrics", "Lyrics that follow the voice", "Lyrics come from Apple Music, Spotify and LRCLIB and are cached on your computer. The highlight chases the singer's position with critical damping, so it glides instead of jumping."],
  ["cache", "Light on Spotify's limits", "Spotify caps how many requests each app can make. Cyder caches almost everything so it rarely has to ask, which keeps your library quick and inside the limit."],
  ["client-id", "Your own Spotify app", "Cyder runs on a Spotify Client ID you create for yourself. Apps in development mode allow up to 5 users, so everyone gets their own and the limits stay yours."],
];

const pills: PhysicsItem[] = [
  { key: "lyrics", label: "Synced lyrics", x: 0.08, rot: -4 },
  { key: "mic", icon: <Mic className="size-6 text-[#6d5cf6] sm:size-8" />, x: 0.16, rot: -30 },
  { key: "shuffle", label: "Smart shuffle", x: 0.24, rot: -62 },
  { key: "queue", label: "Queue", x: 0.32, rot: 6 },
  { key: "connect", label: "Spotify Connect", x: 0.38, rot: 0 },
  { key: "headphones", icon: <Headphones className="size-7 text-zinc-500 sm:size-9" />, x: 0.48, rot: 18 },
  { key: "together", label: "Listen together", x: 0.56, rot: 90 },
  { key: "cassette", icon: <CassetteTape className="size-7 text-[#f97316] sm:size-10" />, x: 0.64, rot: 0 },
  { key: "lastfm", label: "Last.fm", x: 0.7, rot: 0 },
  { key: "discord", label: "Discord", x: 0.78, rot: -24 },
  { key: "keys", label: "Media keys", x: 0.86, rot: 0 },
  { key: "tray", label: "Tray", x: 0.94, rot: 90 },
];

const faqs = [
  ["Is Cyder free?", "Yes. Cyder is free and open source under the GPL-3.0 license. The app has no ads, accounts or paid tiers of its own."],
  ["Is it an official Spotify app?", "No. Cyder is an unofficial client built by an independent developer. It uses Spotify's Web API and Spotify's official Soloist player, so changes on Spotify's side can occasionally affect it."],
  ["Is Apple Music supported?", "Yes, in beta. Songs play through Apple's own MusicKit player inside Cyder, with your Apple Music library, home and search. You need an Apple Music subscription, and changes on Apple's side can occasionally affect playback."],
  ["Do I need Spotify Premium?", "Yes, to play from Spotify. Spotify only lets apps like Cyder play music for Premium accounts, through its official Soloist player and a key you create once. With the Apple Music beta, songs can play through Apple Music instead."],
  ["Does it run on macOS or Linux?", "Not yet. Cyder runs on Windows today as a single cyder.exe download. Native macOS and Linux versions are coming soon."],
  ["How does signing in work?", "The first time you open Cyder, it walks you through creating a free app in the Spotify developer dashboard and pasting its Client ID, then you sign in with Spotify. To play music, you also create a Soloist key with your Premium account."],
  ["Where do the lyrics come from?", "From Apple Music, Spotify and LRCLIB, cached on your computer. If none of them has the lyrics for a song, Cyder says so."],
  ["How does it update?", "Cyder checks GitHub for new versions and downloads them for you. When one is ready, it offers to install it."],
];

// Artist photos in the tile wave: Wikimedia Commons, credited in the footer as their licenses require.
const photoCredits = [
  ["Billie Eilish", "crommelincklars", "CC BY 2.0", "https://commons.wikimedia.org/wiki/File:Billie_Eilish_at_Pukkelpop_Festival_-_18_AUGUST_2019_(08)_(cropped).jpg"],
  ["Taylor Swift", "iHeartRadioCA", "CC BY 3.0", "https://commons.wikimedia.org/wiki/File:Taylor_Swift_at_the_2023_MTV_Video_Music_Awards_4.png"],
  ["The Weeknd", "Brian Ziff", "CC BY-SA 4.0", "https://commons.wikimedia.org/wiki/File:The_Weeknd_Portrait_by_Brian_Ziff.jpg"],
  ["Dua Lipa", "Raph_PH", "CC BY 2.0", "https://commons.wikimedia.org/wiki/File:DuaLipaO2020522_(101_of_110)_(52052470251)_(cropped).jpg"],
  ["Bruno Mars", "Brothers Le", "CC BY 2.0", "https://commons.wikimedia.org/wiki/File:Bruno_Mars_b%26w_(cropped).jpg"],
  ["Lady Gaga", "Carlos M. Vazquez II", "CC BY 2.0", "https://commons.wikimedia.org/wiki/File:Lady_Gaga_at_Joe_Biden%27s_inauguration_(cropped_5).jpg"],
  ["Ed Sheeran", "Harald Krichel", "CC BY-SA 3.0", "https://commons.wikimedia.org/wiki/File:Ed_Sheeran-6886_(cropped).jpg"],
  ["Adam Levine", "gashleygoh", "CC BY-SA 2.0", "https://commons.wikimedia.org/wiki/File:AdamLevine2011.jpg"],
  ["Bad Bunny", "Toglenn", "CC BY-SA 4.0", "https://commons.wikimedia.org/wiki/File:Bad_Bunny_2019_by_Glenn_Francis_(cropped).jpg"],
  ["Olivia Rodrigo", "Raph_PH", "CC BY 2.0", "https://commons.wikimedia.org/wiki/File:OlivaRO2150524_(38)_(53727178601)_(cropped).jpg"],
];

const platformLink = "flex items-center justify-between gap-4 rounded-2xl bg-black/10 px-5 py-4 transition-colors hover:bg-black/15 data-yours:ring-2 data-yours:ring-black";

export default function Page() {
  return (
    <>
      <Motion />
      <SiteData />
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:top-3 focus:left-3 focus:z-50 focus:rounded-full focus:bg-lime focus:px-4 focus:py-2 focus:text-sm focus:text-black"
      >
        Skip to content
      </a>

      <header className="absolute inset-x-0 top-0 z-30">
        <div className="mx-auto flex h-20 w-full max-w-7xl items-center justify-between px-4 sm:px-8">
          <a href="#top" className="flex items-center gap-2" aria-label="Cyder home">
            <img src={ICON} alt="" width={28} height={28} className="rounded-lg" />
            <span className="font-heading text-xl lowercase">cyder</span>
          </a>
          <a href="#download" className="rounded-full bg-lime px-5 py-2 text-sm font-medium text-black transition-colors hover:bg-lime-soft">
            <span data-os-label>Download</span>
          </a>
        </div>
      </header>

      <main id="main">
        {/* Hero: stacked headline, app window, orbiting ring of cards */}
        <section id="top" aria-labelledby="hero-title" className="relative overflow-hidden pt-28 text-center sm:pt-32">
          <h1 id="hero-title" className={`${display} relative px-4 text-[clamp(3rem,10vw,9.5rem)]`}>
            <Reveal className="text-white/25">Your Spotify.</Reveal>
            <Reveal delay={120}>Without The Browser.</Reveal>
          </h1>

          <div className="relative -mt-[7vw] h-[230px] [perspective:1600px] sm:h-[330px] md:h-[420px] lg:h-[540px] xl:h-[640px]">
            <div className="absolute top-0 left-1/2 h-[640px] w-[1200px] origin-top -translate-x-1/2 scale-[.36] [transform-style:preserve-3d] sm:scale-[.52] md:scale-[.66] lg:scale-[.85] xl:scale-100">
              <Orbit cards={heroCards} radius={500} w={320} h={180} ring={1060} />
              <figure className={`absolute top-[120px] left-1/2 w-[520px] -translate-x-1/2 ${bezel}`}>
                <video
                  src="/sites/am-rs/video/now-playing.mp4"
                  poster="/sites/am-rs/video/now-playing-poster.webp"
                  width={960}
                  height={540}
                  autoPlay
                  muted
                  loop
                  playsInline
                  preload="auto"
                  aria-label="Cyder playing This Love by Maroon 5, with word-by-word synced lyrics over the album art"
                  className="block h-auto w-full"
                />
              </figure>
            </div>
          </div>
        </section>

        {/* Intro: line, wave of tiles, headline, sticky note */}
        <section aria-labelledby="intro-title" className="relative pt-16 pb-28 text-center">
          <p className="mx-auto max-w-md px-4 text-sm leading-relaxed text-white/80">
            Cyder is a native Spotify app for Windows, written in Rust, with macOS and Linux coming soon. Synced lyrics, your whole library and Apple Music in beta, in one 34&nbsp;MB file.
          </p>
          <div data-reveal className="mx-auto mt-8 h-24 w-px origin-top scale-y-0 bg-white/40 transition-transform duration-1000 data-in:scale-y-100" />

          <div data-wave="92" aria-hidden="true" className="relative mt-4 h-44 overflow-hidden">
            {tiles.map((t, i) => (
              <span
                key={i}
                data-tile
                className={`absolute top-1/2 left-0 -mt-9 grid size-[72px] place-items-center overflow-hidden rounded-[20px] shadow-[0_10px_24px_rgba(0,0,0,0.5)] ${t.bg ?? "bg-panel"}`}
              >
                {t.img ? <img src={t.img} alt="" width={72} height={72} loading="lazy" className={t.img === ICON ? "size-12" : "size-full object-cover"} /> : t.icon}
              </span>
            ))}
          </div>

          <h2 id="intro-title" className={`${display} mt-6 px-4 text-[clamp(2.75rem,6vw,5rem)]`}>
            <Reveal>Everything You Use</Reveal>
            <Reveal delay={100}>Spotify For</Reveal>
          </h2>
          <p className="mx-auto mt-6 max-w-sm px-4 text-sm leading-relaxed text-white/70">
            Your home, library, playlists and search, with a queue you drag to reorder, smart shuffle, media keys, Discord and Last.fm, in a native app that updates itself.
          </p>
          <p
            data-reveal
            className="mx-auto mt-8 max-w-xs translate-y-6 rounded-xl bg-lilac px-6 py-4 font-heading text-base leading-snug text-black opacity-0 shadow-[0_14px_40px_rgba(222,158,252,0.25)] transition duration-700 data-in:translate-y-0 data-in:-rotate-3 data-in:opacity-100"
          >
            Free and open source under GPL-3.0, with no ads, accounts or paid tiers of its own.
          </p>
        </section>

        {/* Big statement: letters rise, lines drift apart on scroll, album art pans inside the middle words */}
        <section data-progress aria-label="Breaking out of the browser" className="relative overflow-hidden py-28 text-center">
          <h2 aria-label="Breaking out of the browser for good." className={`${display} text-[clamp(3.5rem,13vw,11.5rem)] leading-[0.9]`}>
            <span aria-hidden="true" className="block" style={{ transform: "translateX(calc((var(--p) - 0.5) * -16vw))" }}>
              <Letters text="Breaking" />
            </span>
            <span aria-hidden="true" className="block" style={{ transform: "translateX(calc((var(--p) - 0.5) * 12vw))" }}>
              <Reveal className={artText} delay={120}>
                Out Of The
              </Reveal>
            </span>
            <span aria-hidden="true" className="block" style={{ transform: "translateX(calc((var(--p) - 0.5) * -12vw))" }}>
              <Reveal className={`${artText} [animation-delay:-7s]`} delay={220}>
                Browser
              </Reveal>
            </span>
            <span aria-hidden="true" className="block" style={{ transform: "translateX(calc((var(--p) - 0.5) * 16vw))" }}>
              <Letters text="For Good." delay={300} className="text-lime" />
            </span>
          </h2>
          <span aria-hidden="true" className="absolute top-[36%] right-0 h-[3px] bg-lime shadow-[0_0_14px_#bcf66e]" style={{ width: "calc(clamp(0, (var(--p) - 0.2) * 2.5, 1) * 60%)" }} />
          <span aria-hidden="true" className="absolute top-[60%] left-0 h-[3px] bg-lilac shadow-[0_0_14px_#de9efc]" style={{ width: "calc(clamp(0, (var(--p) - 0.3) * 2.5, 1) * 60%)" }} />
        </section>

        {/* Spotify → Apple Music: pinned, the lilac panel wipes over the lime one */}
        <section id="services" data-progress="pin" aria-label="Spotify and Apple Music" className="relative h-[300vh]">
          <div className="sticky top-0 h-[100svh] overflow-hidden">
            <div className="absolute inset-0 isolate overflow-hidden bg-lime text-black">
              <div className="relative z-10 mx-auto max-w-xl px-4 pt-[16vh] text-center">
                <h2 className={`${display} relative inline-block text-[clamp(3rem,7vw,5.5rem)]`}>
                  For Spotify
                  <Sticker className="absolute -top-10 -right-6 size-14 rotate-12 sm:-top-8 sm:-right-28 sm:size-20">
                    <Headphones className="size-10 text-[#7c3aed] sm:size-12" />
                  </Sticker>
                </h2>
                <p className="mx-auto mt-5 max-w-md text-sm leading-relaxed text-black/75">
                  Cyder plays through Soloist, Spotify&apos;s official player, so every song is the real thing. Browse your library and playlists, and send playback to any of your Spotify Connect devices.
                </p>
              </div>
              <span aria-hidden="true" className="absolute inset-x-0 bottom-[-3vw] text-center font-heading text-[27vw] leading-none whitespace-nowrap text-lime-soft select-none">
                Spotify
              </span>
              <Vinyl label="CYDER" tone="bg-lime" style={{ transform: "translate(-50%, calc(50% + (1 - clamp(0, var(--p) * 3.4, 1)) * 45%))" }} />
            </div>

            <div className="absolute inset-0 isolate overflow-hidden bg-lilac text-black" style={{ transform: "translateX(calc((1 - clamp(0, (var(--p) - 0.34) * 3, 1)) * 100%))" }}>
              <div className="relative z-10 mx-auto max-w-xl px-4 pt-[16vh] text-center">
                <h2 className={`${display} relative inline-block text-[clamp(3rem,7vw,5.5rem)]`}>
                  Apple Music
                  <Sticker className="absolute -top-8 -right-4 -rotate-12 px-3 py-1 sm:-top-5 sm:-right-16 font-heading text-xl text-black sm:text-2xl">Beta</Sticker>
                </h2>
                <p className="mx-auto mt-5 max-w-md text-sm leading-relaxed text-black/75">
                  Sign in with your Apple account and your library, playlists and word-by-word synced lyrics work the way they do everywhere else in Cyder, on the same native player. Expect rough edges while it settles.
                </p>
                <a href="#download" className="mt-6 inline-flex rounded-full bg-black px-5 py-2.5 text-sm font-medium text-white hover:bg-black/80">
                  Try the Apple Music beta
                </a>
              </div>
              <span aria-hidden="true" className="absolute inset-x-0 bottom-[-3vw] text-center font-heading text-[27vw] leading-none whitespace-nowrap text-lilac-soft select-none">
                Apple
              </span>
              <Vinyl label="BETA" tone="bg-lilac" style={{ transform: "translate(-50%, 50%)" }} />
            </div>
          </div>
        </section>

        {/* Listen. Read. Share. — headline slides away, screenshot + accordion slide in */}
        <section
          id="features"
          data-progress="pin"
          aria-labelledby="features-title"
          className="group/f relative lg:h-[260vh]"
          style={
            {
              "--e": "clamp(0, (var(--p) - 0.08) * 2.4, 1)",
              "--hx": "calc(var(--e) * -110vw)",
              "--mx": "calc((1 - var(--e)) * 42vw)",
              "--ax": "calc((1 - var(--e)) * 65vw)",
            } as CSSProperties
          }
        >
          <div className="lg:sticky lg:top-0 lg:h-[100svh] lg:overflow-hidden">
            <h2
              id="features-title"
              className={`${display} px-4 pt-24 text-[clamp(3.5rem,9vw,8rem)] sm:px-8 lg:absolute lg:top-1/2 lg:left-[6vw] lg:-translate-y-1/2 lg:p-0 lg:[transform:translateX(var(--hx))]`}
            >
              <Reveal>Listen.</Reveal>
              <Reveal delay={100}>
                <span className="flex items-center gap-3">
                  <ChevronRight className="size-[0.7em] text-lilac" strokeWidth={3} aria-hidden="true" />
                  Read.
                </span>
              </Reveal>
              <Reveal delay={200}>Share.</Reveal>
            </h2>

            <div className="grid items-center gap-8 px-4 py-14 sm:px-8 lg:absolute lg:inset-0 lg:grid-cols-[1.15fr_1fr] lg:gap-14 lg:px-[6vw] lg:py-0">
              <div className="lg:[transform:translateX(var(--mx))]">
                <figure className={`relative aspect-[16/10] ${bezel}`}>
                  {features.map((f) => (
                    <img
                      key={f.k}
                      src={f.src}
                      alt={f.alt}
                      loading="lazy"
                      className={`absolute inset-0 size-full object-cover object-top opacity-0 transition-opacity duration-500 ${showWhenOpen[f.k]}`}
                    />
                  ))}
                </figure>
              </div>
              <div className="space-y-3 lg:opacity-[var(--e)] lg:[transform:translateX(var(--ax))]">
                {features.map((f, i) => (
                  <details key={f.k} name="features" data-k={f.k} open={i === 0} className="group/d rounded-2xl border border-white/5 bg-panel transition-colors open:border-lime open:bg-lime open:text-black">
                    <summary className="flex cursor-pointer list-none items-center justify-between gap-4 px-5 py-4 font-heading text-lg [&::-webkit-details-marker]:hidden">
                      {f.title}
                      <Toggle />
                    </summary>
                    <p className="px-5 pb-5 text-sm leading-relaxed text-black/80">{f.body}</p>
                  </details>
                ))}
              </div>
            </div>
          </div>
        </section>

        {/* No Electron anywhere: floating stickers with parallax */}
        <section id="engine" data-progress aria-labelledby="engine-title" className="relative overflow-hidden px-4 py-44 text-center sm:py-56">
          {floaters.map((f, i) => (
            <div key={i} aria-hidden="true" className={`absolute ${f.pos}`} style={{ transform: `translateY(calc((0.5 - var(--p)) * ${f.speed}px)) rotate(${f.rot}deg)` }}>
              <div className="animate-float" style={{ animationDelay: `${-i * 0.9}s` }}>
                {f.src ? <img src={f.src} alt="" loading="lazy" className={`${f.size} aspect-[4/5] rounded-2xl object-cover shadow-[0_18px_40px_rgba(0,0,0,0.6)]`} /> : f.icon}
              </div>
            </div>
          ))}
          <div className="relative">
            <h2 id="engine-title" className={`${display} text-[clamp(3.25rem,8vw,7rem)]`}>
              <Reveal className="text-white/35">No Electron</Reveal>
              <Reveal delay={100}>Anywhere.</Reveal>
            </h2>
            <p className="mx-auto mt-6 max-w-md font-heading text-lg leading-snug text-lime">
              The interface is <a className="underline underline-offset-4" href="https://github.com/sonorahq/gpui" rel="noopener">GPUI</a>, the GPU framework behind Zed. Spotify plays through{" "}
              <a className="underline underline-offset-4" href="https://developer.spotify.com/blog/2026-08-13-introducing-spotify-soloist" rel="noopener">Soloist</a>, its official player, and browsing goes straight to the Web API.
            </p>
            <p className="mx-auto mt-5 max-w-xs text-xs leading-relaxed text-white/60">
              Apple Music plays through Apple&apos;s own MusicKit player, out of sight. Lyrics come from Apple Music, Spotify and LRCLIB.
            </p>
          </div>
        </section>

        {/* Under the hood: engine notes */}
        <section aria-labelledby="notes-title" className={`${wrap} max-w-4xl py-24`}>
          <h2 id="notes-title" className={`${display} text-[clamp(3rem,6.5vw,5.5rem)]`}>
            <Reveal>
              <span className="text-lilac">Under</span> The Hood
            </Reveal>
          </h2>
          <div className="mt-8 space-y-3">
            {notes.map(([k, title, body], i) => (
              <details key={k} name="notes" open={i === 0} className="group/d rounded-2xl border border-white/5 bg-panel">
                <summary className="flex cursor-pointer list-none items-center justify-between gap-4 px-5 py-4 font-heading text-lg [&::-webkit-details-marker]:hidden">
                  {title}
                  <Toggle dark />
                </summary>
                <div className="grid gap-2 px-5 pb-5 sm:grid-cols-[7rem_1fr]">
                  <span className="font-mono text-xs text-lilac">{k}</span>
                  <p className="text-sm leading-relaxed text-white/70">{body}</p>
                </div>
              </details>
            ))}
          </div>
        </section>

        {/* MUSIC: image-filled word; feature pills drop in, pile up, and can be grabbed and thrown */}
        <section aria-labelledby="music-title" className="relative overflow-x-clip pt-28 text-center">
          <p className="font-heading text-xl leading-tight sm:text-2xl">
            Taking Control Of
            <br />
            How You Listen To
          </p>
          <div className="relative mt-4">
            <h2 id="music-title" className="bg-[url(/sites/am-rs/img/tiles/collage.webp)] bg-cover bg-center bg-clip-text font-heading text-[31vw] leading-[0.82] text-transparent">
              Music
            </h2>
            <PhysicsPills items={pills} label="Highlights, drag and throw them" />
          </div>
        </section>

        {/* Download: two tilted cards with a pill between them */}
        <section id="download" aria-labelledby="download-title" className={`${wrap} py-28`}>
          <p className="text-center font-heading text-sm tracking-wide text-white/45 uppercase">
            Free and open source · <span data-version>v0.3.0</span> · <span data-date>November 2026</span>
          </p>
          <h2 id="download-title" className="sr-only">
            Download Cyder
          </h2>
          <div className="relative mt-6 grid gap-4 md:grid-cols-2">
            <article data-reveal className="relative flex min-h-[600px] flex-col overflow-hidden rounded-[28px] bg-lilac p-6 text-black transition duration-700 sm:p-8 md:translate-y-10 md:opacity-0 md:data-in:translate-y-0 md:data-in:opacity-100">
              <h3 className={`${display} text-right text-[clamp(2.25rem,4.2vw,3.75rem)]`}>
                Native For
                <br />
                Windows
              </h3>
              <div className="relative z-10 mt-6 ml-auto w-full max-w-xs space-y-2 md:mr-12">
                <a className={platformLink} href={WINDOWS_URL} data-os="windows" rel="noopener">
                  <span className="font-heading text-lg">Windows</span>
                  <span className="text-sm text-black/70">
                    <span data-size="windows">cyder.exe</span> · Windows
                  </span>
                </a>
                <div className="flex items-center justify-between gap-4 rounded-2xl bg-black/5 px-5 py-4 text-black/60">
                  <span className="font-heading text-lg">macOS</span>
                  <span className="rounded-full bg-black px-2.5 py-1 text-[11px] font-medium tracking-wide text-lime uppercase">Coming soon</span>
                </div>
              </div>
              <img src={`${CARD}/home-queue-1280.webp`} alt="" loading="lazy" className={`absolute -bottom-32 -left-10 w-[66%] -rotate-[16deg] ${bezel}`} />
            </article>

            <article data-reveal className="relative flex min-h-[600px] flex-col overflow-hidden rounded-[28px] bg-lime p-6 text-black transition delay-150 duration-700 sm:p-8 md:translate-y-10 md:opacity-0 md:data-in:translate-y-0 md:data-in:opacity-100">
              <h3 className={`${display} text-[clamp(2.25rem,4.2vw,3.75rem)]`}>
                Linux Is
                <br />
                On The Way
              </h3>
              <div className="relative z-10 mt-6 max-w-sm space-y-3 md:ml-12">
                <div className="flex items-center justify-between gap-4 rounded-2xl bg-black/5 px-5 py-4 text-black/60">
                  <span className="font-heading text-lg">Linux</span>
                  <span className="rounded-full bg-black px-2.5 py-1 text-[11px] font-medium tracking-wide text-lime uppercase">Coming soon</span>
                </div>
                <p className="text-sm leading-relaxed text-black/75">
                  A native Linux build with a one-line installer is coming soon. Watch the repository to hear the moment it lands.
                </p>
                <a href={REPO_URL} rel="noopener" className="inline-flex rounded-full bg-black px-5 py-2.5 text-sm font-medium text-white hover:bg-black/80">
                  Watch on GitHub
                </a>
              </div>
              <img src={`${CARD}/artist-drake-1280.webp`} alt="" loading="lazy" className={`absolute -right-12 -bottom-36 w-[62%] rotate-[14deg] ${bezel}`} />
            </article>

            <a
              href={RELEASES_URL}
              rel="noopener"
              className="absolute top-1/2 left-1/2 z-20 hidden -translate-1/2 rounded-full border-[8px] border-ink bg-gradient-to-b from-white to-zinc-300 px-7 py-3 font-heading text-base whitespace-nowrap text-black shadow-[0_10px_30px_rgba(0,0,0,0.5)] transition-transform hover:scale-105 md:block"
            >
              All downloads
            </a>
          </div>
          <p className="mt-6 text-center text-sm text-white/60">
            One file, nothing to install, and Cyder updates itself.{" "}
            <a className="text-white underline underline-offset-4" href={REPO_URL} rel="noopener">
              Build from source
            </a>
          </p>
        </section>

        {/* FAQ in the same accordion style */}
        <section id="faq" aria-labelledby="faq-title" className={`${wrap} max-w-4xl pb-28`}>
          <h2 id="faq-title" className={`${display} text-[clamp(3rem,6.5vw,5.5rem)]`}>
            <Reveal>
              <span className="text-lilac">Questions,</span> Answered
            </Reveal>
          </h2>
          <div className="mt-8 space-y-3">
            {faqs.map(([q, a]) => (
              <details key={q} name="faq" className="group/d rounded-2xl border border-white/5 bg-panel">
                <summary className="flex cursor-pointer list-none items-center justify-between gap-4 px-5 py-4 font-heading text-lg [&::-webkit-details-marker]:hidden">
                  {q}
                  <Toggle dark />
                </summary>
                <p className="px-5 pb-5 text-sm leading-relaxed text-white/70">{a}</p>
              </details>
            ))}
          </div>
        </section>
      </main>

      {/* Footer: concentric arcs, closing line, mini orbit */}
      <footer className="relative overflow-hidden pt-20">
        {[0, 120, 240].map((top, i) => (
          <span
            key={top}
            aria-hidden="true"
            className="absolute left-1/2 aspect-square -translate-x-1/2 rounded-full border border-white/[0.06] bg-gradient-to-b from-white/[0.07] to-transparent"
            style={{ top: top + 80, width: 1500 - i * 240 }}
          />
        ))}
        <div className="relative flex flex-col items-center px-4 text-center">
          <img src={ICON} alt="" width={80} height={80} loading="lazy" className="size-20 rounded-[22px] shadow-[0_12px_40px_rgba(255,59,68,0.35)]" />
          <h2 className={`${display} mt-6 text-[clamp(2.75rem,6vw,4.75rem)] leading-[1.05]`}>
            <Reveal>For Those</Reveal>
            <Reveal delay={100}>Who Breathe Music</Reveal>
          </h2>
          <a href="#download" className="mt-6 inline-flex items-center gap-2 rounded-full border border-white/30 py-1.5 pr-1.5 pl-4 text-sm hover:border-white/60">
            Download Cyder
            <span className="grid size-6 place-items-center rounded-full bg-lime text-black">
              <Plus className="size-4" strokeWidth={2.5} aria-hidden="true" />
            </span>
          </a>
          <nav aria-label="Project links" className="mt-16 grid w-full max-w-3xl grid-cols-2 gap-y-4 font-heading text-lg sm:grid-cols-4">
            <a className="hover:text-lime" href={REPO_URL} rel="noopener">GitHub</a>
            <a className="hover:text-lime" href={`${REPO_URL}/releases`} rel="noopener">Releases</a>
            <a className="hover:text-lime" href={`${REPO_URL}/issues`} rel="noopener">Report a bug</a>
            <a className="hover:text-lime" href={`${REPO_URL}/blob/main/LICENSE`} rel="noopener">GPL-3.0</a>
          </nav>

          <div className="relative mt-14 h-[200px] w-full max-w-md">
            <div className="absolute inset-x-[10%] top-0 bottom-[-200px] rounded-full bg-white/[0.05]" aria-hidden="true" />
            <div className="relative flex justify-center gap-5 pt-8">
              <a href={REPO_URL} rel="noopener" aria-label="Cyder on GitHub" className="text-white/80 hover:text-white">
                <svg viewBox="0 0 16 16" className="size-5" aria-hidden="true"><path fill="currentColor" d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" /></svg>
              </a>
            </div>
            <div className="absolute inset-x-0 bottom-0 h-[130px] [perspective:900px]">
              <div className="absolute top-0 left-1/2 h-[220px] w-[520px] origin-top -translate-x-1/2 scale-[.7] [transform-style:preserve-3d]">
                <Orbit cards={[`${TILE}/mix1.webp`, `${TILE}/idol.webp`, `${TILE}/mix3.webp`, `${TILE}/xscape.webp`, `${TILE}/mix5.webp`, `${TILE}/mix0.webp`]} radius={170} w={110} h={130} ring={400} />
              </div>
            </div>
          </div>
        </div>
        <div className="relative border-t border-white/5 bg-ink">
          <div className="mx-auto flex max-w-7xl flex-col gap-2 px-4 py-6 text-xs text-white/50 sm:flex-row sm:justify-between sm:px-8">
            <span>© 2026 Cyder. Free and open source under GPL-3.0.</span>
            <span>An independent, unofficial client, not affiliated with or endorsed by Spotify or Apple. Album art in screenshots belongs to its owners.</span>
          </div>
          <p className="mx-auto max-w-7xl px-4 pb-6 text-[11px] leading-relaxed text-white/35 sm:px-8">
            Artist photos via Wikimedia Commons:{" "}
            {photoCredits.map(([name, author, license, page], i) => (
              <span key={name}>
                <a className="underline-offset-2 hover:text-white/70 hover:underline" href={page} rel="noopener">
                  {name}
                </a>{" "}
                by {author} ({license}){i < photoCredits.length - 1 ? ", " : "."}
              </span>
            ))}
          </p>
        </div>
      </footer>
    </>
  );
}
