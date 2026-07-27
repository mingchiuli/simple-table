import { isAppErrorPayload } from '@/utils/appError';

export type IdempotentInvokeResult<T> =
  | { status: 'response'; response: T }
  | { status: 'ambiguous'; error: unknown };

type IdempotentInvokeOptions<T> = {
  operationId: string;
  invoke: (operationId: string) => Promise<T>;
  isDefinitiveFailure?: (error: unknown) => boolean;
};

export async function invokeIdempotently<T>({
  operationId,
  invoke,
  isDefinitiveFailure = isAppErrorPayload,
}: IdempotentInvokeOptions<T>): Promise<IdempotentInvokeResult<T>> {
  try {
    return { status: 'response', response: await invoke(operationId) };
  } catch (error) {
    if (isDefinitiveFailure(error)) throw error;
  }

  try {
    return { status: 'response', response: await invoke(operationId) };
  } catch (error) {
    return { status: 'ambiguous', error };
  }
}
