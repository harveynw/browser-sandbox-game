import js from "@eslint/js";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      "**/dist/**",
      "**/node_modules/**",
      "target/**",
      "packages/sim-wasm/**",
      "coverage/**",
      "**/src/generated/**",
    ],
  },
  js.configs.recommended,
  ...tseslint.configs.strict,
  {
    files: [
      "apps/client/src/**/*.ts",
      "packages/protocol/src/**/*.ts",
      "packages/content/src/**/*.ts",
    ],
    languageOptions: { globals: globals.browser },
  },
  {
    files: [
      "*.mjs",
      "*.ts",
      "apps/client/vite.config.ts",
      "packages/**/tests/**/*.ts",
    ],
    languageOptions: { globals: globals.node },
  },
);
