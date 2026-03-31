import * as m from '@/paraglide/messages';

type MessageFunction = (params?: Record<string, unknown>) => string;

const messages = m as unknown as Record<string, MessageFunction>;

/**
 * Access a translation by dynamic key.
 * Use this when the key is constructed at runtime (e.g., template literals).
 * For static keys, prefer m["key"]() directly.
 */
export function dynamicTranslation(key: string, params?: Record<string, unknown>): string {
  const fn = messages[key];
  if (typeof fn === 'function') {
    return fn(params);
  }
  return key;
}
