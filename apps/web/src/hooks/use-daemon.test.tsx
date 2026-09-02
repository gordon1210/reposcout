import { act, renderHook, waitFor } from "@testing-library/react"
import { afterEach, describe, expect, it, vi } from "vitest"

import { useDaemon } from "@/hooks/use-daemon"
import { makeSnapshot } from "@/test/fixtures"

class MockEventSource {
  static instances: MockEventSource[] = []

  onopen: (() => void) | null = null
  onerror: (() => void) | null = null
  readonly listeners = new Map<string, () => void>()
  readonly close = vi.fn()

  constructor(readonly url: string) {
    MockEventSource.instances.push(this)
  }

  addEventListener(type: string, listener: () => void) {
    this.listeners.set(type, listener)
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

function snapshotResponse(revision: number) {
  return {
    ok: true,
    status: 200,
    json: vi.fn().mockResolvedValue(makeSnapshot({ revision })),
  }
}

afterEach(() => {
  MockEventSource.instances = []
  vi.unstubAllGlobals()
})

describe("useDaemon", () => {
  it("reconciles on every SSE open and ignores older snapshot responses", async () => {
    const initial = deferred<ReturnType<typeof snapshotResponse>>()
    const opened = deferred<ReturnType<typeof snapshotResponse>>()
    const reconnected = deferred<ReturnType<typeof snapshotResponse>>()
    const fetch = vi
      .fn()
      .mockReturnValueOnce(initial.promise)
      .mockReturnValueOnce(opened.promise)
      .mockReturnValueOnce(reconnected.promise)
    vi.stubGlobal("fetch", fetch)
    vi.stubGlobal("EventSource", MockEventSource)

    const { result } = renderHook(() => useDaemon())
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(1))
    const source = MockEventSource.instances[0]

    act(() => source.onopen?.())
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2))
    expect(fetch.mock.calls[0][1].signal.aborted).toBe(true)

    await act(async () => opened.resolve(snapshotResponse(2)))
    await waitFor(() => expect(result.current.snapshot?.revision).toBe(2))
    await act(async () => initial.resolve(snapshotResponse(1)))
    expect(result.current.snapshot?.revision).toBe(2)
    expect(result.current.connection).toBe("live")

    act(() => source.onerror?.())
    expect(result.current.connection).toBe("offline")
    act(() => source.onopen?.())
    await waitFor(() => expect(fetch).toHaveBeenCalledTimes(3))
    await act(async () => reconnected.resolve(snapshotResponse(3)))
    await waitFor(() => expect(result.current.snapshot?.revision).toBe(3))
    expect(result.current.connection).toBe("live")
  })
})
