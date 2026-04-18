/**
 * Platform module - Dynamic OS-based loading
 *
 * Usage:
 *   import { getPlatformAPI, pickFile, saveFile } from '@/platform';
 *
 *   const api = await getPlatformAPI();
 *   const result = await pickFile();
 */
export * from './types';
export * from './loader';
