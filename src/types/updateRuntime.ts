export type UpdatePlatform = 'desktop' | 'android' | 'ios';

export type MobileUpdateState = {
  version: string;
  tagName: string;
  releaseUrl: string;
  apkUrl: string | null;
};
