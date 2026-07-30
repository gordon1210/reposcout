export const qualityRules = {
  complexity: ["error", 20],
  curly: ["error", "all"],
  "max-lines": [
    "error",
    {
      max: 900,
      skipBlankLines: true,
      skipComments: true,
    },
  ],
}

export const formattingRules = {
  rules: {
    "prettier/prettier": [
      "error",
      {
        semi: false,
        singleQuote: false,
        trailingComma: "es5",
      },
    ],
  },
}
