<template>
  <div>
    <h2>Dashboard</h2>

    <!-- 概览卡片 -->
    <n-grid :cols="4" :x-gap="16" style="margin-bottom: 24px">
      <n-grid-item>
        <n-card :bordered="false" class="stat-card">
          <n-statistic label="Total Requests" :value="overview.total_requests" />
        </n-card>
      </n-grid-item>
      <n-grid-item>
        <n-card :bordered="false" class="stat-card">
          <n-statistic label="Input Tokens" :value="overview.total_input_tokens" />
        </n-card>
      </n-grid-item>
      <n-grid-item>
        <n-card :bordered="false" class="stat-card">
          <n-statistic label="Output Tokens" :value="overview.total_output_tokens" />
        </n-card>
      </n-grid-item>
      <n-grid-item>
        <n-card :bordered="false" class="stat-card">
          <n-statistic label="Avg Latency (ms)" :value="Math.round(overview.avg_latency_ms)" />
        </n-card>
      </n-grid-item>
    </n-grid>

    <!-- 图表 -->
    <n-grid :cols="2" :x-gap="16">
      <n-grid-item>
        <n-card title="Daily Requests" :bordered="false">
          <v-chart class="chart" :option="dailyOption" autoresize />
        </n-card>
      </n-grid-item>
      <n-grid-item>
        <n-card title="Model Usage" :bordered="false">
          <v-chart class="chart" :option="modelOption" autoresize />
        </n-card>
      </n-grid-item>
    </n-grid>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue'
import { NGrid, NGridItem, NCard, NStatistic } from 'naive-ui'
import VChart from 'vue-echarts'
import { use } from 'echarts/core'
import { CanvasRenderer } from 'echarts/renderers'
import { LineChart, BarChart, PieChart } from 'echarts/charts'
import { GridComponent, TooltipComponent, LegendComponent, TitleComponent } from 'echarts/components'

use([CanvasRenderer, LineChart, BarChart, PieChart, GridComponent, TooltipComponent, LegendComponent, TitleComponent])

const API_BASE = '/api/admin'

const overview = ref({ total_requests: 0, total_input_tokens: 0, total_output_tokens: 0, avg_latency_ms: 0, error_count: 0 })
const dailyData = ref<any[]>([])
const modelData = ref<any[]>([])

const dailyOption = ref({
  tooltip: { trigger: 'axis' },
  legend: { data: ['Requests', 'Input Tokens', 'Output Tokens'] },
  xAxis: { type: 'category', data: [] as string[] },
  yAxis: { type: 'value' },
  series: [
    { name: 'Requests', type: 'line', data: [] as number[], smooth: true },
    { name: 'Input Tokens', type: 'line', data: [] as number[], smooth: true },
    { name: 'Output Tokens', type: 'line', data: [] as number[], smooth: true },
  ],
})

const modelOption = ref({
  tooltip: { trigger: 'item' },
  legend: { orient: 'vertical', left: 'left' },
  series: [
    {
      name: 'Model Usage',
      type: 'pie',
      radius: '60%',
      data: [] as { name: string; value: number }[],
      emphasis: { itemStyle: { shadowBlur: 10, shadowOffsetX: 0, shadowColor: 'rgba(0, 0, 0, 0.5)' } },
    },
  ],
})

async function loadData() {
  try {
    const [overviewRes, dailyRes, modelsRes] = await Promise.all([
      fetch(`${API_BASE}/stats/overview`).then(r => r.json()),
      fetch(`${API_BASE}/stats/daily?days=7`).then(r => r.json()),
      fetch(`${API_BASE}/stats/models`).then(r => r.json()),
    ])
    overview.value = overviewRes

    // 日统计图表
    dailyData.value = dailyRes
    dailyOption.value = {
      ...dailyOption.value,
      xAxis: { type: 'category', data: dailyRes.map((d: any) => d.date) },
      series: [
        { name: 'Requests', type: 'line', data: dailyRes.map((d: any) => d.request_count), smooth: true },
        { name: 'Input Tokens', type: 'line', data: dailyRes.map((d: any) => d.input_tokens), smooth: true },
        { name: 'Output Tokens', type: 'line', data: dailyRes.map((d: any) => d.output_tokens), smooth: true },
      ],
    }

    // 模型使用占比
    modelData.value = modelsRes
    modelOption.value = {
      ...modelOption.value,
      series: [
        {
          name: 'Model Usage',
          type: 'pie',
          radius: '60%',
          data: modelsRes.map((m: any) => ({ name: m.model_id, value: m.request_count })),
        },
      ],
    }
  } catch (e) {
    console.error('Failed to load dashboard data:', e)
  }
}

onMounted(loadData)
</script>

<style scoped>
.stat-card {
  text-align: center;
}
.chart {
  height: 320px;
}
</style>
