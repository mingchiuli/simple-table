import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { openUrl } from '@tauri-apps/plugin-opener'
import { invoke } from '@tauri-apps/api/core'
import { platform } from '@tauri-apps/plugin-os'
import { getVersion } from '@tauri-apps/api/app'
import { requestApplicationExit } from '@/composables/useApplicationExit'

export type UpdateStatus = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'error' | 'no-update'

export interface UpdateInfo {
  version: string
  tag_name: string
  release_url: string
  apk_url: string | null
}

export function useUpdater() {
  const status = ref<UpdateStatus>('idle')
  const updateInfo = shallowRef<Update | null>(null)
  const mobileUpdateInfo = ref<UpdateInfo | null>(null)
  const downloadProgress = ref({ downloaded: 0, total: 0, percentage: 0 })
  const errorMessage = ref<string | null>(null)
  const currentVersion = ref('')
  let currentVersionPromise: Promise<string> | null = null
  let operationToken = 0

  // 初始化时获取应用版本
  onMounted(() => {
    const token = operationToken
    void ensureCurrentVersion().catch((e) => {
      if (!isCurrentOperation(token)) return

      errorMessage.value = String(e)
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
      const appVersion = await ensureCurrentVersion()
      if (!isCurrentOperation(token)) return

      if (isDesktop.value) {
        // 桌面端：使用 tauri-plugin-updater
        const update = await check()
        if (!isCurrentOperation(token)) return

        if (update) {
          updateInfo.value = update
          status.value = 'available'
        } else {
          status.value = 'no-update'
        }
      } else {
        // 移动端：调用 Rust command
        const info = await invoke<UpdateInfo | null>('check_update_mobile', {
          currentVersion: appVersion
        })
        if (!isCurrentOperation(token)) return

        if (info) {
          mobileUpdateInfo.value = info
          status.value = 'available'
        } else {
          status.value = 'no-update'
        }
      }
    } catch (e) {
      if (!isCurrentOperation(token)) return

      status.value = 'error'
      errorMessage.value = String(e)
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
      errorMessage.value = String(e)
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
      if (isAndroid.value && info.apk_url) {
        // Android: 打开 APK 下载链接
        await openUrl(info.apk_url)
      } else {
        // iOS 或 Android 没有 APK: 打开 Releases 页面
        await openUrl(info.release_url)
      }
    } catch (e) {
      if (!isCurrentOperation(token)) return

      status.value = 'error'
      errorMessage.value = String(e)
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
