import path from "node:path"
import { loadConfig } from "tsconfig-paths"

function prefix(pattern) {
  const wildcard = pattern.endsWith("/*")
  const value = wildcard ? pattern.slice(0, -2) : pattern
  if (value.includes("*")) {
    throw new Error(
      `Unsupported alias pattern: ${pattern}; use exact paths or /*`
    )
  }
  return { value, wildcard }
}

function loadMappings(configPath, cwd) {
  const config = loadConfig(path.resolve(cwd, configPath))
  if (config.resultType !== "success") {
    throw new Error(config.message)
  }
  return Object.entries(config.paths).flatMap(([alias, targets]) => {
    const name = prefix(alias)
    return targets.map((target) => {
      const location = prefix(target)
      if (name.wildcard !== location.wildcard) {
        throw new Error(
          `Alias and target must use matching wildcards: ${alias}`
        )
      }
      return {
        alias: name.value,
        target: path.resolve(config.absoluteBaseUrl, location.value),
        wildcard: name.wildcard,
      }
    })
  })
}

function matches(value, base, wildcard, separator) {
  return value === base || (wildcard && value.startsWith(base + separator))
}

function absoluteImport(value, filename, mappings) {
  if (value.startsWith("./") || value.startsWith("../")) {
    return path.resolve(path.dirname(filename), value)
  }
  const mapping = [...mappings]
    .sort((a, b) => b.alias.length - a.alias.length)
    .find(({ alias, wildcard }) => matches(value, alias, wildcard, "/"))
  if (mapping) {
    return path.resolve(mapping.target, "." + value.slice(mapping.alias.length))
  }
}

export const importAliasRule = {
  meta: {
    type: "suggestion",
    fixable: "code",
    schema: [
      {
        type: "object",
        properties: { aliasConfigPath: { type: "string" } },
        required: ["aliasConfigPath"],
        additionalProperties: false,
      },
    ],
    messages: {
      alias: 'Use "{{alias}}" instead of "{{original}}".',
      config: "Cannot load import aliases: {{message}}",
    },
  },
  create(context) {
    let mappings
    try {
      mappings = loadMappings(
        context.options[0].aliasConfigPath,
        context.cwd
      ).sort((a, b) => b.target.length - a.target.length)
    } catch (error) {
      return {
        Program(node) {
          context.report({
            node,
            messageId: "config",
            data: { message: error.message },
          })
        },
      }
    }

    function check(node) {
      if (!node || typeof node.value !== "string") {
        return
      }
      const absolute = absoluteImport(node.value, context.filename, mappings)
      if (!absolute) {
        return
      }
      const mapping = mappings.find(({ target, wildcard }) =>
        matches(absolute, target, wildcard, path.sep)
      )
      if (!mapping) {
        return
      }
      const alias =
        mapping.alias +
        absolute.slice(mapping.target.length).split(path.sep).join("/")
      if (alias === node.value) {
        return
      }
      context.report({
        node,
        messageId: "alias",
        data: { alias, original: node.value },
        fix(fixer) {
          const quote = context.sourceCode.getText(node)[0]
          const escaped = JSON.stringify(alias).slice(1, -1)
          const content =
            quote === "'" ? escaped.replaceAll("'", "\\'") : escaped
          return fixer.replaceText(node, quote + content + quote)
        },
      })
    }

    return {
      ImportDeclaration: (node) => check(node.source),
      ExportAllDeclaration: (node) => check(node.source),
      ExportNamedDeclaration: (node) => check(node.source),
      CallExpression(node) {
        const callee = node.callee
        const name =
          callee.type === "Identifier"
            ? callee.name
            : callee.type === "MemberExpression" && !callee.computed
              ? callee.property.name
              : undefined
        if (name === "require" || name === "mock") {
          check(node.arguments[0])
        }
      },
    }
  },
}

export default { rules: { "import-alias": importAliasRule } }
