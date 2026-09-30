/// <reference types="vitest" />
import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1421,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  test: {
    environment: "jsdom",
    // No globals: every test file imports what it uses, so a stray
    // `expect` cannot compile against an ambient one and the files stay
    // readable without knowing the runner's conventions.
    globals: false,
    setupFiles: ["./src/test/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    // `format.stamp`/`dayAndTime` and `search.windowStart("today")` all read
    // the machine's timezone - local midnight, not the last 24h. Pinned so a
    // developer's TZ and a CI runner's cannot disagree about what "today" is.
    env: { TZ: "UTC" },
    coverage: {
      provider: "v8",
      // Codecov wants the raw json; the text summary is for humans locally.
      reporter: ["text", "json", "html"],
      reportsDirectory: "./coverage",
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        // Entry point: mounts the DOM, nothing worth asserting.
        "src/main.tsx",
        // A fixture, not logic - counting it would flatter the number.
        "src/lib/mock.ts",
        // The vitest setup itself.
        "src/test/**",
        "src/**/*.test.{ts,tsx}",
      ],
    },
  },
});
