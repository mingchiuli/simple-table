import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { openUrl } from '@tauri-apps/plugin-opener'
import { invokeCommand } from '@/tauriInvoke'
import { platform } from '@tauri-apps/plugin-os'
import { getVersion } from '@tauri-apps/api/app'
import { requestApplicationExit } from '@/composables/useApplicationExit'
import type { UpdateInfo } from '@/types'
import { appErrorMessage } from '@/utils/appError'

export type { UpdateInfo } from '@/types'

export type UpdateStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'error' | 'no-update'

type UpdateCheckResult =
  | { platform: 'desktop'; appVersion: string; update: Update | null }
  | { platform: 'mobile'; appVersion: string; update: UpdateInfo | null }

export function useUpdater() {
  const status = ref<UpdateStatus>('idle')
  const updateInfo = shallowRef<Update | null>(null)
  const mobileUpdateInfo = ref<UpdateInfo | null>(null)
  const downloadProgress = ref({ downloaded: 0, total: 0, percentage: 0 })
  const errorMessage = ref<string | null>(null)
  const currentVersion = ref('')
  let currentVersionPromise: Promise<string> | null = null
  let updateCheckPromise: Promise<UpdateCheckResult> | null = null
  let operationToken = 0

  // 初始化时获取应用版本
  onMounted(() => {
    const token = operationToken
    void ensureCurrentVersion().catch((e) => {
      if (!isCurrentOperation(token)) return

      errorMessage.value = appErrorMessage(e)
    })
  })

  const isChecking = computed(() => status.value === 'checking')
  const isDownloading = computed(() => status.value === 'downloading')
  const hasUpdate = computed(() => status.value === 'available' || status.value === 'downloading' || status.value === 'ready')

  // 检查是否是桌面端
  const isDesktop = computed(() => {
    const osPlatform = platform()
    return osPlatform === 'macos' || osPlatform === 'windows' || osPlatform === 'linux'
  })

  // 检查是否是 Android
  const isAndroid = computed(() => platform() === 'android')

  // 检查是否是 iOS
  const isIOS = computed(() => platform() === 'ios')

  async function checkForUpdate() {
    const token = beginOperation()
    status.value = 'checking'
    errorMessage.value = null

    try {
      updateCheckPromise ??= runUpdateCheck().finally(() => {
        updateCheckPromise = null
      })
      const result = await updateCheckPromise
      if (!isCurrentOperation(token)) return

      currentVersion.value = result.appVersion
      if (result.platform === 'desktop') {
        if (result.update) {
          updateInfo.value = result.update
          status.value = 'available'
        } else {
          status.value = 'no-update'
        }
      } else {
        if (result.update) {
          mobileUpdateInfo.value = result.update
          status.value = 'available'
        } else {
          status.value = 'no-update'
        }
      }
    } catch (e) {
      if (!isCurrentOperation(token)) return

      status.value = 'error'
      errorMessage.value = appErrorMessage(e)
    }
  }

  async function runUpdateCheck(): Promise<UpdateCheckResult> {
    const appVersion = await ensureCurrentVersion()
    if (isDesktop.value) {
      return { platform: 'desktop', appVersion, update: await check() }
    }
    return {
      platform: 'mobile',
      appVersion,
      update: await invokeCommand('check_update_mobile', { currentVersion: appVersion })
    }
  }

  async function downloadAndInstall() {
    const update = updateInfo.value
    if (!update) return

    const token = beginOperation()
    if (status.value === 'ready') {
      await relaunchWhenReady(token)
      return
    }
    status.value = 'downloading'
    downloadProgress.value = { downloaded: 0, total: 0, percentage: 0 }

    try {
      await update.downloadAndInstall((event) => {
        if (!isCurrentOperation(token)) return

        switch (event.event) {
          case 'Started':
            downloadProgress.value.total = event.data.contentLength ?? 0
            break
          case 'Progress':
            downloadProgress.value.downloaded += event.data.chunkLength
            if (downloadProgress.value.total > 0) {
              downloadProgress.value.percentage =
                Math.round((downloadProgress.value.downloaded / downloadProgress.value.total) * 100)
            }
            break
          case 'Finished':
            status.value = 'ready'
            break
        }
      })
      if (!isCurrentOperation(token)) return

      await relaunchWhenReady(token)
    } catch (e) {
      if (!isCurrentOperation(token)) return

      status.value = 'error'
      errorMessage.value = appErrorMessage(e)
    }
  }

  async function relaunchWhenReady(token: number) {
    if (!isCurrentOperation(token)) return
    await requestApplicationExit(relaunch)
  }

  async function handleMobileUpdate() {
    const info = mobileUpdateInfo.value
    if (!info) return

    const token = beginOperation()
    try {
      if (isAndroid.value && info.apkUrl) {
        // Android: 打开 APK 下载链接
        await openUrl(info.apkUrl)
      } else {
        // iOS 或 Android 没有 APK: 打开 Releases 页面
        await openUrl(info.releaseUrl)
      }
    } catch (e) {
      if (!isCurrentOperation(token)) return

      status.value = 'error'
      errorMessage.value = appErrorMessage(e)
    }
  }

  function reset() {
    operationToken += 1
    status.value = 'idle'
    updateInfo.value = null
    mobileUpdateInfo.value = null
    downloadProgress.value = { downloaded: 0, total: 0, percentage: 0 }
    errorMessage.value = null
  }

  async function ensureCurrentVersion(): Promise<string> {
    if (currentVersion.value) {
      return currentVersion.value
    }
    currentVersionPromise ??= getVersion()
      .then((version) => {
        currentVersion.value = version
        return version
      })
      .finally(() => {
        currentVersionPromise = null
      })
    return currentVersionPromise
  }

  function beginOperation() {
    operationToken += 1
    return operationToken
  }

  function isCurrentOperation(token: number) {
    return token === operationToken
  }

  return {
    status,
    updateInfo,
    mobileUpdateInfo,
    downloadProgress,
    errorMessage,
    currentVersion,
    isChecking,
    isDownloading,
    hasUpdate,
    isDesktop,
    isAndroid,
    isIOS,
    checkForUpdate,
    downloadAndInstall,
    handleMobileUpdate,
    reset
  }
}
