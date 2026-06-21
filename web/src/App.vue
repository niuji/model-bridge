<template>
  <n-config-provider :theme="theme" :locale="zhCN" :date-locale="dateZhCN">
    <n-message-provider>
      <n-layout style="min-height: 100vh">
        <n-layout-header bordered>
          <div class="header">
            <h1 class="title">Model Bridge</h1>
            <n-menu
              mode="horizontal"
              :options="menuOptions"
              :value="currentRoute"
              @update:value="navigate"
            />
          </div>
        </n-layout-header>
        <n-layout-content>
          <div class="content">
            <router-view />
          </div>
        </n-layout-content>
      </n-layout>
    </n-message-provider>
  </n-config-provider>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useRouter, useRoute } from 'vue-router'
import {
  NConfigProvider,
  NLayout,
  NLayoutHeader,
  NLayoutContent,
  NMenu,
  NMessageProvider,
  zhCN,
  dateZhCN,
} from 'naive-ui'

const router = useRouter()
const route = useRoute()

const currentRoute = computed(() => route.path)

const menuOptions = [
  { label: 'Dashboard', key: '/' },
  { label: 'Providers', key: '/providers' },
  { label: 'API Keys', key: '/api-keys' },
  { label: 'Logs', key: '/logs' },
]

function navigate(key: string) {
  router.push(key)
}
</script>

<style>
body {
  margin: 0;
  padding: 0;
  font-family: 'Inter', -apple-system, BlinkMacSystemFont, sans-serif;
}
.header {
  display: flex;
  align-items: center;
  padding: 0 24px;
  height: 48px;
}
.title {
  font-size: 18px;
  font-weight: 700;
  margin: 0 32px 0 0;
  white-space: nowrap;
  color: #18a058;
}
.content {
  padding: 24px;
  max-width: 1200px;
  margin: 0 auto;
}
</style>
