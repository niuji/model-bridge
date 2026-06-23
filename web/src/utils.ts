// 后端返回的时间均为 UTC（SQLite CURRENT_TIMESTAMP），统一转成浏览器本地时区显示。

function pad(n: number): string {
  return String(n).padStart(2, '0')
}

// ISO UTC 字符串 -> 本地 'YYYY-MM-DD HH:mm:ss'（用于日志等完整时间展示）
export function formatLocalTime(iso: string): string {
  if (!iso) return ''
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`
}

// ISO UTC 小时桶 -> 本地 'MM-DD HH:00'（用于图表坐标轴标签）
export function formatLocalHour(iso: string): string {
  if (!iso) return ''
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:00`
}
