import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  serverExternalPackages: ["node-pty", "ssh2"],
  allowedDevOrigins: ["http://192.168.36.150:4885"],
};

export default nextConfig;
