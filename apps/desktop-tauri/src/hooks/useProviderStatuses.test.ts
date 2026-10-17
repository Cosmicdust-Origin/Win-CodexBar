import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderStatusEntry } from "../types/bridge";

type Handler = (event: { payload: ProviderStatusEntry[] }) => void;

const mocks = vi.hoisted(() => ({
  getProviderStatuses: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("../lib/tauri", () => ({ getProviderStatuses: mocks.getProviderStatuses }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
import { PROVIDER_STATUSES_UPDATED, useProviderStatuses } from "./useProviderStatuses";

const entry = (indicator: "none" | "major"): ProviderStatusEntry[] => [
  { providerId: "claude", status: { indicator, description: null, updatedAt: null } },
];

describe("useProviderStatuses", () => {
  let handler: Handler | undefined;
  let registered: (() => void) | undefined;

  beforeEach(() => {
    vi.resetAllMocks();
    handler = undefined;
    mocks.listen.mockImplementation((_name: string, h: Handler) => {
      handler = h;
      return new Promise<() => void>((resolve) => {
        registered = () => resolve(() => {});
      });
    });
  });

  it("reads the snapshot only after the listener is registered", async () => {
    mocks.getProviderStatuses.mockResolvedValue(entry("none"));
    const { result } = renderHook(() => useProviderStatuses());
    expect(mocks.listen).toHaveBeenCalledWith(PROVIDER_STATUSES_UPDATED, expect.any(Function));
    expect(mocks.getProviderStatuses).not.toHaveBeenCalled();

    await act(async () => registered?.());
    await waitFor(() => expect(result.current).toEqual(entry("none")));
    expect(mocks.getProviderStatuses).toHaveBeenCalledTimes(1);
  });

  it("keeps a broadcast that lands while the snapshot read is in flight", async () => {
    let resolveSnapshot: (value: ProviderStatusEntry[]) => void = () => {};
    mocks.getProviderStatuses.mockReturnValue(
      new Promise<ProviderStatusEntry[]>((resolve) => {
        resolveSnapshot = resolve;
      }),
    );
    const { result } = renderHook(() => useProviderStatuses());
    await act(async () => registered?.());
    await waitFor(() => expect(mocks.getProviderStatuses).toHaveBeenCalledTimes(1));

    act(() => handler?.({ payload: entry("major") }));
    await act(async () => resolveSnapshot(entry("none")));
    expect(result.current).toEqual(entry("major"));
  });

  it("still reads the snapshot when the listener cannot register", async () => {
    mocks.listen.mockRejectedValue(new Error("no event bridge"));
    mocks.getProviderStatuses.mockResolvedValue(entry("major"));
    const { result } = renderHook(() => useProviderStatuses());
    await waitFor(() => expect(result.current).toEqual(entry("major")));
  });
});
