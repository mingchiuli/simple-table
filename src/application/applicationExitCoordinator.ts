export type ApplicationExitGuard = () => Promise<boolean>;
export type ExitAction = () => Promise<void>;

const exitGuards = new Set<ApplicationExitGuard>();
let activeExitRequest: Promise<boolean> | null = null;

export function registerApplicationExitGuard(guard: ApplicationExitGuard): () => void {
  exitGuards.add(guard);
  return () => exitGuards.delete(guard);
}

export function requestApplicationExit(exit: ExitAction): Promise<boolean> {
  if (activeExitRequest) return activeExitRequest;

  activeExitRequest = runApplicationExit(exit).finally(() => {
    activeExitRequest = null;
  });
  return activeExitRequest;
}

async function runApplicationExit(exit: ExitAction): Promise<boolean> {
  const guards = Array.from(exitGuards).reverse();
  for (const guard of guards) {
    if (!(await guard())) return false;
  }

  await exit();
  return true;
}
