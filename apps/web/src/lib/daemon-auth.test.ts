import { beforeEach, describe, expect, it } from "vitest"

import { daemonAuthHeaders, daemonAuthToken } from "@/lib/daemon-auth"

const VALID_TOKEN = "a".repeat(64)

describe("daemon browser authentication", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/")
    window.sessionStorage.clear()
  })

  it("does not replace a stored token with an unrelated URL fragment", () => {
    window.history.replaceState(null, "", `/#token=${VALID_TOKEN}`)
    expect(daemonAuthToken()).toBe(VALID_TOKEN)

    window.history.replaceState(null, "", "/#section-heading")

    expect(daemonAuthHeaders().get("Authorization")).toBe(`Bearer ${VALID_TOKEN}`)
  })

  it("does not replace a stored token with a malformed named token fragment", () => {
    window.history.replaceState(null, "", `/#token=${VALID_TOKEN}`)
    expect(daemonAuthToken()).toBe(VALID_TOKEN)

    window.history.replaceState(null, "", "/#token=section-heading")

    expect(daemonAuthHeaders().get("Authorization")).toBe(`Bearer ${VALID_TOKEN}`)
  })

  it("does not authenticate with a malformed query token", () => {
    window.history.replaceState(null, "", "/?token=section-heading")

    expect(daemonAuthHeaders().has("Authorization")).toBe(false)
  })
})
