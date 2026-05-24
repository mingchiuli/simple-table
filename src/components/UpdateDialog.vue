<script setup lang="ts">
import { ref, computed } from 'vue'
import { useUpdater } from '@/composables/useUpdater'
import { ElDialog, ElProgress, ElButton, ElIcon } from 'element-plus'
import { Loading, CircleCheckFilled, CircleCloseFilled } from '@element-plus/icons-vue'

const {
  status,
  updateInfo,
  mobileUpdateInfo,
  downloadProgress,
  errorMessage,
  currentVersion,
  isChecking,
  isDownloading,
  isDesktop,
  isAndroid,
  checkForUpdate,
  downloadAndInstall,
  handleMobileUpdate,
  reset
} = useUpdater()

const visible = ref(false)

const dialogTitle = computed(() => {
  if (status.value === 'downloading') return 'Downloading Update'
  if (status.value === 'ready') return 'Update Ready'
  if (status.value === 'available') return 'Update Available'
  if (status.value === 'no-update') return 'No Updates Available'
  if (status.value === 'error') return 'Update Error'
  return 'Check for Updates'
})

const newVersion = computed(() => {
  if (isDesktop.value && updateInfo.value) {
    return updateInfo.value.version
  }
  if (mobileUpdateInfo.value) {
    return mobileUpdateInfo.value.version
  }
  return ''
})

const actionButtonText = computed(() => {
  if (isDesktop.value) {
    if (status.value === 'available') return 'Download & Install'
    if (status.value === 'ready') return 'Restart Now'
  } else {
    if (isAndroid.value) return 'Download APK'
    return 'Open Release Page'
  }
  return ''
})

async function handleAction() {
  if (isDesktop.value) {
    await downloadAndInstall()
  } else {
    await handleMobileUpdate()
  }
}

function handleClose() {
  visible.value = false
  reset()
}

function show() {
  visible.value = true
}

defineExpose({ show, checkForUpdate })
</script>

<template>
  <ElDialog
    v-model="visible"
    :title="dialogTitle"
    width="400px"
    @close="handleClose"
  >
    <div class="update-content">
      <!-- Checking state -->
      <div v-if="isChecking" class="state">
        <ElIcon class="icon loading"><Loading /></ElIcon>
        <p>Checking for updates...</p>
      </div>

      <!-- No update state -->
      <div v-else-if="status === 'no-update'" class="state">
        <ElIcon class="icon success"><CircleCheckFilled /></ElIcon>
        <p>You're using the latest version</p>
        <p class="version-info">Current: v{{ currentVersion }}</p>
      </div>

      <!-- Update available state -->
      <div v-else-if="status === 'available'" class="state">
        <p>New version <strong>v{{ newVersion }}</strong> is available</p>
        <p class="version-info">Current: v{{ currentVersion }}</p>
        <p v-if="!isDesktop" class="mobile-note">
          {{ isAndroid ? 'APK will be downloaded via browser' : 'Please visit GitHub to download' }}
        </p>
      </div>

      <!-- Downloading state -->
      <div v-else-if="isDownloading" class="state">
        <ElProgress
          :percentage="downloadProgress.percentage"
          :stroke-width="8"
          :show-text="true"
        />
        <p class="download-size">
          {{ Math.round(downloadProgress.downloaded / 1024) }} KB /
          {{ Math.round(downloadProgress.total / 1024) }} KB
        </p>
      </div>

      <!-- Ready state -->
      <div v-else-if="status === 'ready'" class="state">
        <ElIcon class="icon success"><CircleCheckFilled /></ElIcon>
        <p>Update downloaded successfully!</p>
        <p>Restarting application...</p>
      </div>

      <!-- Error state -->
      <div v-else-if="status === 'error'" class="state">
        <ElIcon class="icon error"><CircleCloseFilled /></ElIcon>
        <p>Update failed</p>
        <p class="error-message">{{ errorMessage }}</p>
      </div>

      <!-- Idle state -->
      <div v-else class="state">
        <p>Current version: v{{ currentVersion }}</p>
      </div>
    </div>

    <template #footer>
      <div class="dialog-footer">
        <ElButton @click="handleClose">Close</ElButton>
        <ElButton
          v-if="status === 'idle'"
          type="primary"
          @click="checkForUpdate"
          :loading="isChecking"
        >
          Check for Updates
        </ElButton>
        <ElButton
          v-if="status === 'available'"
          type="primary"
          @click="handleAction"
        >
          {{ actionButtonText }}
        </ElButton>
        <ElButton
          v-if="status === 'error'"
          type="primary"
          @click="checkForUpdate"
        >
          Retry
        </ElButton>
      </div>
    </template>
  </ElDialog>
</template>

<style scoped>
.update-content {
  text-align: center;
  padding: 20px;
}

.state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
}

.icon {
  font-size: 48px;
}

.icon.loading {
  color: var(--el-color-primary);
}

.icon.success {
  color: var(--el-color-success);
}

.icon.error {
  color: var(--el-color-danger);
}

.version-info {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}

.mobile-note {
  color: var(--el-text-color-secondary);
  font-size: 12px;
  margin-top: 8px;
}

.download-size {
  color: var(--el-text-color-secondary);
  font-size: 12px;
}

.error-message {
  color: var(--el-color-danger);
  font-size: 12px;
  word-break: break-word;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>