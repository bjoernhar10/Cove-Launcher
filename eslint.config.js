import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: [
      "**/node_modules/**",
      "**/dist/**",
      "**/dist-e2e/**",
      "**/test-results/**",
      "**/playwright-report/**",
      "**/target/**",
      "**/bin/**",
      "**/obj/**",
      "**/.venv/**",
      "apps/desktop/src-tauri/gen/**",
      "**/.wrangler/**",
    ],
  },

  // TypeScript (UI + shared): strict, type-aware, no `any`.
  {
    files: ["apps/desktop/**/*.{ts,tsx}", "packages/shared/**/*.ts", "services/catalog-proxy/**/*.ts"],
    extends: [...tseslint.configs.strictTypeChecked, ...tseslint.configs.stylisticTypeChecked],
    languageOptions: {
      parserOptions: { projectService: true, tsconfigRootDir: import.meta.dirname },
      globals: { ...globals.browser },
    },
    plugins: { "react-hooks": reactHooks },
    rules: {
      ...reactHooks.configs.recommended.rules,
      "@typescript-eslint/no-explicit-any": "error",
      "@typescript-eslint/consistent-type-imports": "error",
    },
  },

  // Architecture + design guards for the desktop UI.
  {
    files: ["apps/desktop/src/**/*.{ts,tsx}"],
    ignores: ["apps/desktop/src/api/**", "apps/desktop/src/mocks/**", "apps/desktop/src/test/**"],
    rules: {
      "no-restricted-imports": [
        "error",
        {
          patterns: [
            {
              group: ["**/mocks", "**/mocks/**"],
              message: "UI code must not import mocks. Use the data layer in src/api instead.",
            },
            {
              group: ["@tauri-apps/api/core", "@tauri-apps/api/event", "@tauri-apps/api/mocks"],
              message: "Talk to the core only through src/api (typed call()/subscribe()).",
            },
          ],
        },
      ],
      "no-restricted-syntax": [
        "error",
        {
          selector: "Literal[value=/#[0-9a-fA-F]{3,8}\\b|rgba?\\(|hsla?\\(/]",
          message: "No hard-coded colors in components. Use design tokens (Tailwind theme / tokens.css).",
        },
        {
          selector: "TemplateElement[value.raw=/#[0-9a-fA-F]{3,8}\\b|rgba?\\(|hsla?\\(/]",
          message: "No hard-coded colors in components. Use design tokens (Tailwind theme / tokens.css).",
        },
      ],
    },
  },

  // Plain JavaScript plugins (ES modules) and root config files.
  {
    files: ["plugins/**/*.js", "*.js"],
    extends: [js.configs.recommended],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "module",
      globals: { ...globals.node },
    },
  },
);
