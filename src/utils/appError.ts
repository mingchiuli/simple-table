export type AppErrorPayload = {
  code: string;
  message: string;
};

export function isAppErrorCode(error: unknown, code: string): boolean {
  return isAppErrorPayload(error) && error.code === code;
}

export function appErrorMessage(error: unknown): string {
  if (isAppErrorPayload(error)) return error.message;
  if (error instanceof Error) return error.toString();
  return String(error);
}

export function isAppErrorPayload(error: unknown): error is AppErrorPayload {
  if (typeof error !== "object" || error === null) return false;
  const candidate = error as Partial<AppErrorPayload>;
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}
