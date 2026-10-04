import assert from "node:assert/strict"
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import process from "node:process"
import { after, test } from "node:test"
import { Linter } from "eslint"
import tseslint from "typescript-eslint"

import importAlias from "./import-alias.js"

const root = mkdtempSync(path.join(tmpdir(), "reposcout-alias-test-"))
after(() => rmSync(root, { recursive: true, force: true }))
mkdirSync(path.join(root, "src", "components"), { recursive: true })
writeFileSync(
  path.join(root, "base.json"),
  `{
  // Keep JSONC and inherited paths supported.
  "compilerOptions": {
    "baseUrl": ".",
    "paths": { "@/*": ["src/*"], "@ui/*": ["src/components/*"], "settings": ["config.ts"] }
  }
}`
)
writeFileSync(path.join(root, "tsconfig.json"), '{"extends":"./base.json"}')
writeFileSync(path.join(root, "broken.json"), "{ invalid")
writeFileSync(
  path.join(root, "unsupported.json"),
  JSON.stringify({
    compilerOptions: { paths: { "@*": ["src/*"] } },
  })
)
writeFileSync(
  path.join(root, "no-base.json"),
  JSON.stringify({
    compilerOptions: { paths: { "@/*": ["src/*"] } },
  })
)
mkdirSync(path.join(root, "shared", "src"), { recursive: true })
writeFileSync(
  path.join(root, "shared", "base.json"),
  JSON.stringify({
    compilerOptions: { paths: { "@/*": ["src/*"] } },
  })
)
writeFileSync(
  path.join(root, "inherited.json"),
  '{"extends":"./shared/base.json"}'
)
writeFileSync(path.join(root, "empty.json"), '{"compilerOptions":{}}')
writeFileSync(
  path.join(root, "fallback.json"),
  JSON.stringify({
    compilerOptions: { paths: { "@/*": ["src/*", "generated/*"] } },
  })
)
writeFileSync(
  path.join(root, "exact-directory.json"),
  JSON.stringify({
    compilerOptions: { paths: { folder: ["src/data"] } },
  })
)

function lint(code, config = "tsconfig.json", filename = "src/view.js") {
  const linter = new Linter({ cwd: root })
  return linter.verifyAndFix(
    code,
    [
      {
        files: ["**/*.{js,ts,tsx}"],
        languageOptions: { parser: tseslint.parser },
        plugins: { reposcout: importAlias },
        rules: {
          "reposcout/import-alias": ["error", { aliasConfigPath: config }],
        },
      },
    ],
    { filename: path.join(root, filename) }
  )
}

for (const [name, before, expected] of [
  ["relative import", 'import x from "./data"', 'import x from "@/data"'],
  ["single quotes", "import x from './data'", "import x from '@/data'"],
  [
    "most specific alias",
    'import x from "./components/button"',
    'import x from "@ui/button"',
  ],
  [
    "existing broad alias",
    'import x from "@/components/button"',
    'import x from "@ui/button"',
  ],
  ["export all", 'export * from "./data"', 'export * from "@/data"'],
  [
    "named re-export",
    'export { x } from "./data"',
    'export { x } from "@/data"',
  ],
  ["require", 'const x = require("./data")', 'const x = require("@/data")'],
  ["mock", 'vi.mock("./data")', 'vi.mock("@/data")'],
  ["exact path", 'import x from "../config.ts"', 'import x from "settings"'],
  ["escaped quote", "import x from './it\\'s'", "import x from '@/it\\'s'"],
  [
    "trailing directory slash",
    'import x from "./data/"',
    'import x from "@/data/"',
  ],
]) {
  test(`fixes ${name}`, () => {
    const result = lint(before)
    assert.equal(result.fixed, true)
    assert.equal(result.output, expected)
    assert.deepEqual(result.messages, [])
  })
}

for (const code of [
  'import x from "react"',
  'import x from "node:fs"',
  'import x from "@/data"',
  'import x from "@ui/button"',
  'import x from "../scripts/tool"',
  'import x from "../src-other/tool"',
  'import x from "@/components-other/tool"',
  'import x from "../config.ts/child"',
  'import x from "./"',
  'import x from "../src"',
  'import x from "."',
  'import x from "@/"',
  "const x = require(variable)",
  "vi.mock(variable)",
  "export const x = 1",
]) {
  test(`preserves ${code}`, () => {
    const result = lint(code)
    assert.equal(result.fixed, false)
    assert.equal(result.output, code)
    assert.deepEqual(result.messages, [])
  })
}

test("supports paths without an explicit baseUrl", () => {
  const result = lint('import x from "./data"', "no-base.json")
  assert.equal(result.output, 'import x from "@/data"')
  assert.deepEqual(result.messages, [])
})

test("resolves inherited paths relative to the declaring config without baseUrl", () => {
  const result = lint(
    'import x from "./data"',
    "inherited.json",
    "shared/src/view.ts"
  )
  assert.equal(result.output, 'import x from "@/data"')
  assert.deepEqual(result.messages, [])
})

test("checks TypeScript type-only imports", () => {
  const result = lint(
    'import type { Data } from "./data"',
    "tsconfig.json",
    "src/view.ts"
  )
  assert.equal(result.output, 'import type { Data } from "@/data"')
  assert.deepEqual(result.messages, [])
})

test("preserves directory syntax when only an exact alias could match", () => {
  const result = lint('import x from "./data/"', "exact-directory.json")
  assert.equal(result.fixed, false)
  assert.equal(result.output, 'import x from "./data/"')
  assert.deepEqual(result.messages, [])
})

for (const [specifier, filename] of [
  [".", "src/features/view.tsx"],
  ["..", "src/features/widgets/view.tsx"],
]) {
  test(`checks directory import ${specifier}`, () => {
    const result = lint(
      `import x from "${specifier}"`,
      "tsconfig.json",
      filename
    )
    assert.equal(result.output, 'import x from "@/features/"')
    assert.deepEqual(result.messages, [])
  })
}

for (const key of ["TS_NODE_PROJECT", "TS_NODE_BASEURL"]) {
  test(`uses the selected config regardless of ${key}`, () => {
    const previous = process.env[key]
    process.env[key] = path.join(
      root,
      key === "TS_NODE_PROJECT" ? "empty.json" : "elsewhere"
    )
    try {
      const result = lint('import x from "./data"')
      assert.equal(result.output, 'import x from "@/data"')
      assert.deepEqual(result.messages, [])
    } finally {
      if (previous === undefined) {
        delete process.env[key]
      } else {
        process.env[key] = previous
      }
    }
  })
}

for (const config of [
  "missing.json",
  "broken.json",
  "unsupported.json",
  "empty.json",
  "fallback.json",
]) {
  test(`reports ${config} instead of silently disabling enforcement`, () => {
    const result = lint('import x from "./data"', config)
    assert.equal(result.fixed, false)
    assert.equal(result.messages.length, 1)
    assert.equal(result.messages[0].messageId, "config")
  })
}
