/**
 * Platform module - Dynamic OS-based loading
 *
 * Usage:
 *   import { getPlatformAPI, pickOpenFile, prepareOpenFile, saveFile } from '@/platform';
 *
 *   const api = await getPlatformAPI();
 *   const selection = await pickOpenFile();
 *   const prepared = selection ? await prepareOpenFile(selection.path) : null;
 */
export * from './types';
export * from './loader';
