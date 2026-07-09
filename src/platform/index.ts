/**
 * Platform module - Dynamic OS-based loading
 *
 * Usage:
 *   import { getPlatformAPI, pickOpenFile, readFile, saveFile } from '@/platform';
 *
 *   const api = await getPlatformAPI();
 *   const selection = await pickOpenFile();
 *   const opened = selection ? await readFile(selection.path) : null;
 */
export * from './types';
export * from './loader';
