import path from "node:path";
import { buildSeedDatabase } from "../src/lib/sqliteSchema";

const outputPath = path.join(process.cwd(), "resources", "database", "app.seed.db");
buildSeedDatabase(outputPath);

console.log(`[db:prepare] Seed database ready: ${outputPath}`);
