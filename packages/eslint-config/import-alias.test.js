import assert from "node:assert/strict"
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import path from "node:path"
import { after, test } from "node:test"
import { Linter } from "eslint"

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

function lint(code, config = "tsconfig.json") {
  const linter = new Linter({ cwd: root })
  return linter.verifyAndFix(
    code,
    [
      {
        plugins: { reposcout: importAlias },
        rules: {
          "reposcout/import-alias": ["error", { aliasConfigPath: config }],
        },
      },
    ],
    { filename: path.join(root, "src", "view.js") }
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

for (const config of ["missing.json", "broken.json", "unsupported.json"]) {
  test(`reports ${config} instead of silently disabling enforcement`, () => {
    const result = lint('import x from "./data"', config)
    assert.equal(result.fixed, false)
    assert.equal(result.messages.length, 1)
    assert.equal(result.messages[0].messageId, "config")
  })
}
