import type { Metadata, Viewport } from "next";
import { Anton } from "next/font/google";
import "./globals.css";

const anton = Anton({ weight: "400", subsets: ["latin"], variable: "--font-anton" });

export const metadata: Metadata = {
  title: "Cyder: a fast, native Spotify desktop app for Windows, with macOS and Linux coming soon",
  description:
    "Cyder is a free, open-source Spotify desktop client written in Rust for Windows, with macOS and Linux coming soon. An 11 MB download with word-by-word synced lyrics, romanization and no Electron.",
};

export const viewport: Viewport = {
  themeColor: "#020203",
  colorScheme: "dark",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" className={`${anton.variable} h-full antialiased`}>
      <body className="overflow-x-clip bg-ink text-white">{children}</body>
    </html>
  );
}
