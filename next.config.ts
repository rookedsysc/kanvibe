import type { NextConfig } from "next";
import createNextIntlPlugin from "next-intl/plugin";

const withNextIntl = createNextIntlPlugin("./src/i18n/request.ts");

const nextConfig: NextConfig = {
  serverExternalPackages: ["typeorm", "reflect-metadata", "node-pty", "ssh2"],
};

export default withNextIntl(nextConfig);
