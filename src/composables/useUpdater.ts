import { ref, shallowRef, computed, onMounted } from 'vue'
import { check, type Update } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { openUrl } from '@tauri-apps/plugin-opener'
import { invoke } from '@tauri-apps/api/core'
import { platform } from '@tauri-apps/plugin-os'
import { getVersion } from '@tauri-apps/api/app'

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

  // 初始化时获取应用版本
  onMounted(async () => {
    currentVersion.value = await getVersion()
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
    status.value = 'checking'
    errorMessage.value = null

    try {
      if (isDesktop.value) {
        // 桌面端：使用 tauri-plugin-updater
        const update = await check()
        if (update) {
          updateInfo.value = update
          status.value = 'available'
        } else {
          status.value = 'no-update'
        }
      } else {
        // 移动端：调用 Rust command
        const info = await invoke<UpdateInfo | null>('check_update_mobile', {
          currentVersion: currentVersion.value
        })
        if (info) {
          mobileUpdateInfo.value = info
          status.value = 'available'
        } else {
          status.value = 'no-update'
        }
      }
    } catch (e) {
      status.value = 'error'
      errorMessage.value = String(e)
    }
  }

  async function downloadAndInstall() {
    if (!updateInfo.value) return

    status.value = 'downloading'
    downloadProgress.value = { downloaded: 0, total: 0, percentage: 0 }

    try {
      await updateInfo.value.downloadAndInstall((event) => {
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

      // 自动重启
      await relaunch()
    } catch (e) {
      status.value = 'error'
      errorMessage.value = String(e)
    }
  }

  async function handleMobileUpdate() {
    if (!mobileUpdateInfo.value) return

    try {
      if (isAndroid.value && mobileUpdateInfo.value.apk_url) {
        // Android: 打开 APK 下载链接
        await openUrl(mobileUpdateInfo.value.apk_url)
      } else {
        // iOS 或 Android 没有 APK: 打开 Releases 页面
        await openUrl(mobileUpdateInfo.value.release_url)
      }
    } catch (e) {
      status.value = 'error'
      errorMessage.value = String(e)
    }
  }

  function reset() {
    status.value = 'idle'
    updateInfo.value = null
    mobileUpdateInfo.value = null
    downloadProgress.value = { downloaded: 0, total: 0, percentage: 0 }
    errorMessage.value = null
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
