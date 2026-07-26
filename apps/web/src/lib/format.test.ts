import { describe, expect, it } from "vitest"

import { formatPercent, formatRatio } from "@/lib/format"

describe("percentage formatting", () => {
  it("distinguishes fractional ratios from percentage-valued metrics", () => {
    expect(formatRatio(0.0289)).toBe("2.9%")
    expect(formatPercent(7.5)).toBe("7.5%")
  })
})
