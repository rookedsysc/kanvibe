import type { NextConfig } from "next";
import createNextIntlPlugin from "next-intl/plugin";

const withNextIntl = createNextIntlPlugin("./src/i18n/request.ts");

const port = process.env.PORT || "4885";

const nextConfig: NextConfig = {
  serverExternalPackages: ["typeorm", "reflect-metadata", "node-pty", "ssh2"],
  allowedDevOrigins: [`http://192.168.36.150:${port}`],
};

export default withNextIntl(nextConfig);
