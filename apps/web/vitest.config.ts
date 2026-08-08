import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    // 足場の時点ではテストがまだ無い。Wave 1c/1d が入れば実際の件数で検証する。
    passWithNoTests: true,
  },
});
