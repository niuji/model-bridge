export async function adminRequest(path: string, init?: RequestInit): Promise<Response> {
  const response = await fetch(`/api/admin${path}`, init)
  if (!response.ok) {
    const data = await response.json().catch(() => null)
    throw new Error(data?.error || `请求失败（HTTP ${response.status}）`)
  }
  return response
}

export function errorMessage(error: unknown, fallback: string): string {
  return error instanceof Error ? `${fallback}：${error.message}` : fallback
}
