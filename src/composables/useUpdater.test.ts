import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Update } from "@tauri-apps/plugin-updater";
import { effectScope } from "vue";
import { useUpdater, type UpdateInfo } from "@/composables/useUpdater";
import { useApplicationExitGuard } from "@/composables/useApplicationExit";

const tauriMocks = vi.hoisted(() => ({
  check: vi.fn(),
  relaunch: vi.fn(),
  openUrl: vi.fn(),
  invoke: vi.fn(),
  platform: vi.fn(),
  getVersion: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: tauriMocks.check,
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: tauriMocks.relaunch,
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: tauriMocks.openUrl,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriMocks.invoke,
}));

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: tauriMocks.platform,
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: tauriMocks.getVersion,
}));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

async function flushPromises() {
  await Promise.resolve();
  await Promise.resolve();
}

function mobileUpdateInfo(): UpdateInfo {
  return {
    version: "0.12.0",
    tag_name: "v0.12.0",
    release_url: "https://example.com/releases/v0.12.0",
    apk_url: "https://example.com/app.apk",
  };
}

describe("useUpdater", () => {
  let warnSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.platform.mockReturnValue("android");
    tauriMocks.getVersion.mockResolvedValue("0.11.1");
    tauriMocks.invoke.mockResolvedValue(null);
    tauriMocks.check.mockResolvedValue(null);
    tauriMocks.relaunch.mockResolvedValue(undefined);
    tauriMocks.openUrl.mockResolvedValue(undefined);
    warnSpy = vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });

  afterEach(() => {
    warnSpy.mockRestore();
  });

  it("passes the resolved app version to mobile update checks", async () => {
    const updater = useUpdater();

    await updater.checkForUpdate();

    expect(tauriMocks.invoke).toHaveBeenCalledWith("check_update_mobile", {
      currentVersion: "0.11.1",
    });
    expect(updater.status.value).toBe("no-update");
  });

  it("ignores mobile update check results after reset", async () => {
    const pendingCheck = deferred<UpdateInfo | null>();
    tauriMocks.invoke.mockReturnValue(pendingCheck.promise);
    const updater = useUpdater();

    const checkPromise = updater.checkForUpdate();
    await flushPromises();
    updater.reset();
    pendingCheck.resolve(mobileUpdateInfo());
    await checkPromise;

    expect(updater.status.value).toBe("idle");
    expect(updater.mobileUpdateInfo.value).toBeNull();
  });

  it("ignores app version load failures after reset", async () => {
    const pendingVersion = deferred<string>();
    tauriMocks.getVersion.mockReturnValue(pendingVersion.promise);
    const updater = useUpdater();

    const checkPromise = updater.checkForUpdate();
    await flushPromises();
    updater.reset();
    pendingVersion.reject(new Error("version unavailable"));
    await checkPromise;

    expect(updater.status.value).toBe("idle");
    expect(updater.errorMessage.value).toBeNull();
  });

  it("ignores desktop update check results after reset", async () => {
    tauriMocks.platform.mockReturnValue("macos");
    const update = { version: "0.12.0" } as Update;
    const pendingCheck = deferred<Update | null>();
    tauriMocks.check.mockReturnValue(pendingCheck.promise);
    const updater = useUpdater();

    const checkPromise = updater.checkForUpdate();
    await flushPromises();
    updater.reset();
    pendingCheck.resolve(update);
    await checkPromise;

    expect(updater.status.value).toBe("idle");
    expect(updater.updateInfo.value).toBeNull();
  });

  it("ignores desktop download progress and relaunch after reset", async () => {
    tauriMocks.platform.mockReturnValue("macos");
    const continueDownload = deferred<void>();
    const update = {
      version: "0.12.0",
      downloadAndInstall: vi.fn(async (onEvent) => {
        onEvent({ event: "Started", data: { contentLength: 100 } });
        await continueDownload.promise;
        onEvent({ event: "Progress", data: { chunkLength: 100 } });
        onEvent({ event: "Finished", data: {} });
      }),
    } as unknown as Update;
    const updater = useUpdater();
    updater.updateInfo.value = update;

    const downloadPromise = updater.downloadAndInstall();
    await flushPromises();
    expect(updater.status.value).toBe("downloading");
    expect(updater.downloadProgress.value.total).toBe(100);

    updater.reset();
    continueDownload.resolve();
    await downloadPromise;

    expect(updater.status.value).toBe("idle");
    expect(updater.downloadProgress.value).toEqual({
      downloaded: 0,
      total: 0,
      percentage: 0,
    });
    expect(tauriMocks.relaunch).not.toHaveBeenCalled();
  });

  it("keeps an installed update ready when application exit is cancelled", async () => {
    tauriMocks.platform.mockReturnValue("macos");
    const update = {
      version: "0.12.0",
      downloadAndInstall: vi.fn(async (onEvent) => {
        onEvent({ event: "Finished", data: {} });
      }),
    } as unknown as Update;
    const scope = effectScope();
    scope.run(() => {
      useApplicationExitGuard(vi.fn().mockResolvedValue(false));
    });
    const updater = useUpdater();
    updater.updateInfo.value = update;
    updater.status.value = "available";

    try {
      await updater.downloadAndInstall();

      expect(update.downloadAndInstall).toHaveBeenCalledTimes(1);
      expect(updater.status.value).toBe("ready");
      expect(tauriMocks.relaunch).not.toHaveBeenCalled();
    } finally {
      scope.stop();
    }
  });

  it("retries a ready relaunch without downloading the update again", async () => {
    tauriMocks.platform.mockReturnValue("macos");
    const update = {
      version: "0.12.0",
      downloadAndInstall: vi.fn(),
    } as unknown as Update;
    const updater = useUpdater();
    updater.updateInfo.value = update;
    updater.status.value = "ready";

    await updater.downloadAndInstall();

    expect(update.downloadAndInstall).not.toHaveBeenCalled();
    expect(tauriMocks.relaunch).toHaveBeenCalledTimes(1);
  });

  it("ignores mobile update launch failures after reset", async () => {
    const pendingOpen = deferred<void>();
    tauriMocks.openUrl.mockReturnValue(pendingOpen.promise);
    const updater = useUpdater();
    updater.mobileUpdateInfo.value = mobileUpdateInfo();

    const openPromise = updater.handleMobileUpdate();
    await flushPromises();
    updater.reset();
    pendingOpen.reject(new Error("cannot open browser"));
    await openPromise;

    expect(updater.status.value).toBe("idle");
    expect(updater.errorMessage.value).toBeNull();
  });
});
