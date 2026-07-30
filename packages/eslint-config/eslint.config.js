import js from "@eslint/js"
import prettierRecommended from "eslint-plugin-prettier/recommended"
import { defineConfig, globalIgnores } from "eslint/config"

import { formattingRules, qualityRules } from "./rules.js"

export default defineConfig([
  globalIgnores(["coverage", "dist"]),
  {
    files: ["**/*.{js,mjs,cjs}"],
    extends: [js.configs.recommended],
    rules: qualityRules,
  },
  prettierRecommended,
  formattingRules,
])
