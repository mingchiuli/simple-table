import { getCurrent, onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { listen } from "@tauri-apps/api/event";
import type { Router } from "vue-router";
import { onMounted, onUnmounted } from "vue";
import { filePathFromDeepLinkTarget } from "@/utils/fileFormats";

type Unlisten = () => void;

type DeepLinkDependencies = {
  listen: (
    event: "deep-link-received",
    handler: (event: { payload: string }) => void
  ) => Promise<Unlisten>;
  onOpenUrl: (handler: (urls: string[]) => void) => Promise<Unlisten>;
  getCurrent: () => Promise<string[] | null>;
  pushFilePath: (filePath: string) => Promise<unknown>;
  reportError: (message: string, error: unknown) => void;
};

export type DeepLinkLifecycle = {
  start: () => void;
  stop: () => void;
};

export function useDeepLinks(router: Pick<Router, "push">) {
  const lifecycle = createDeepLinkLifecycle({
    listen,
    onOpenUrl,
    getCurrent,
    pushFilePath: (filePath) => router.push({ name: "table", query: { file: filePath } }),
    reportError: (message, error) => console.error(message, error),
  });

  onMounted(lifecycle.start);
  onUnmounted(lifecycle.stop);
}

export function createDeepLinkLifecycle({
  listen,
  onOpenUrl,
  getCurrent,
  pushFilePath,
  reportError,
}: DeepLinkDependencies): DeepLinkLifecycle {
  let lifecycleId = 0;
  const unlisteners: Unlisten[] = [];

  function start() {
    stop();
    const currentLifecycleId = ++lifecycleId;
    void registerSingleInstanceDeepLinks(currentLifecycleId);
    void registerFileAssociationDeepLinks(currentLifecycleId);
    void handleInitialDeepLinks(currentLifecycleId);
  }

  function stop() {
    lifecycleId += 1;
    while (unlisteners.length > 0) {
      safeUnlisten(unlisteners.pop());
    }
  }

  async function registerSingleInstanceDeepLinks(currentLifecycleId: number) {
    try {
      registerUnlistener(
        await listen("deep-link-received", (event) => {
          if (!isCurrentLifecycle(currentLifecycleId)) return;
          handleDeepLink(event.payload);
        }),
        currentLifecycleId
      );
    } catch (error) {
      reportError("Failed to initialize single instance deep links:", error);
    }
  }

  async function registerFileAssociationDeepLinks(currentLifecycleId: number) {
    try {
      registerUnlistener(
        await onOpenUrl((urls) => {
          if (!isCurrentLifecycle(currentLifecycleId)) return;
          if (urls.length > 0) {
            handleDeepLink(urls[0]);
          }
        }),
        currentLifecycleId
      );
    } catch (error) {
      reportError("Failed to initialize file association deep links:", error);
    }
  }

  async function handleInitialDeepLinks(currentLifecycleId: number) {
    try {
      const startUrls = await getCurrent();
      if (!isCurrentLifecycle(currentLifecycleId)) return;
      if (startUrls && startUrls.length > 0) {
        handleDeepLink(startUrls[0]);
      }
    } catch (error) {
      reportError("Failed to read initial deep links:", error);
    }
  }

  function registerUnlistener(unlisten: Unlisten, currentLifecycleId: number) {
    if (isCurrentLifecycle(currentLifecycleId)) {
      unlisteners.push(unlisten);
      return;
    }
    safeUnlisten(unlisten);
  }

  function handleDeepLink(target: string) {
    try {
      const filePath = filePathFromDeepLinkTarget(target);
      void pushFilePath(filePath).catch((error) => {
        reportError("Failed to route deep link:", error);
      });
    } catch (error) {
      reportError("Failed to parse deep link:", error);
    }
  }

  function isCurrentLifecycle(currentLifecycleId: number) {
    return lifecycleId === currentLifecycleId;
  }

  function safeUnlisten(unlisten: Unlisten | undefined) {
    try {
      unlisten?.();
    } catch (error) {
      reportError("Failed to clean up deep link listener:", error);
    }
  }

  return { start, stop };
}
