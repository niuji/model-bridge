export async function adminRequest(path: string, init?: RequestInit): Promise<Response> {
  const response = await fetch(`/api/admin${path}`, init)
  if (!response.ok) {
    const data = await response.json().catch(() => null)
    throw new Error(data?.error || `请求失败（HTTP ${response.status}）`)
  }
  return response
}

export function proxyScopedPath(path: string, systemOne = false): string {
  const scope = systemOne ? 'system_one' : 'llm'
  return `${path}${path.includes('?') ? '&' : '?'}scope=${scope}`
}

export function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? `${fallback}：${error.message}` : fallback
}
