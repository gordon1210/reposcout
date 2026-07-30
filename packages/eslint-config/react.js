import js from "@eslint/js"
import importAlias from "@limegrass/eslint-plugin-import-alias"
import prettierRecommended from "eslint-plugin-prettier/recommended"
import reactHooks from "eslint-plugin-react-hooks"
import reactRefresh from "eslint-plugin-react-refresh"
import globals from "globals"
import tseslint from "typescript-eslint"
import { defineConfig, globalIgnores } from "eslint/config"

import { formattingRules, qualityRules } from "./rules.js"

export function reactConfig({ aliasConfigPath, ignoreShadcn = false } = {}) {
  return defineConfig([
    globalIgnores(["coverage", "dist"]),
    ...(ignoreShadcn ? [globalIgnores(["src/components/ui/**"])] : []),
    {
      files: ["**/*.{js,mjs,cjs,ts,tsx}"],
      extends: [js.configs.recommended, tseslint.configs.recommended],
      languageOptions: {
        globals: {
          ...globals.browser,
          ...globals.node,
        },
      },
      rules: qualityRules,
    },
    {
      files: ["**/*.{jsx,tsx}"],
      extends: [reactHooks.configs.flat.recommended, reactRefresh.configs.vite],
    },
    {
      files: ["**/*.{ts,tsx}"],
      rules: {
        "@typescript-eslint/no-explicit-any": "error",
        "@typescript-eslint/no-unused-vars": [
          "error",
          {
            argsIgnorePattern: "^_",
            varsIgnorePattern: "^_",
          },
        ],
        "@typescript-eslint/consistent-type-assertions": [
          "error",
          {
            assertionStyle: "never",
          },
        ],
      },
    },
    ...(aliasConfigPath
      ? [
          {
            files: ["**/*.{ts,tsx}"],
            plugins: {
              "@limegrass/import-alias": importAlias,
            },
            rules: {
              "@limegrass/import-alias/import-alias": [
                "error",
                {
                  aliasConfigPath,
                },
              ],
            },
          },
        ]
      : []),
    {
      files: ["**/*.{test,spec}.{ts,tsx}"],
      rules: {
        complexity: "off",
        "max-lines": "off",
        "@typescript-eslint/consistent-type-assertions": "off",
        "@typescript-eslint/no-explicit-any": "off",
        "react-hooks/purity": "off",
        "react-hooks/set-state-in-effect": "off",
        "react-refresh/only-export-components": "off",
      },
    },
    prettierRecommended,
    formattingRules,
  ])
}
