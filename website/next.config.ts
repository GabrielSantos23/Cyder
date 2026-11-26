import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
  // Pin the workspace root to this folder, so a lockfile or workspace file in a parent folder isn't picked up.
  turbopack: { root: __dirname },
};

export default nextConfig;
