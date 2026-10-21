interface Usage { date: string; client: string; model_id: string; request_count: number; input_tokens: number; output_tokens: number }

export function usageSeries(rows: Usage[], dates: string[]) {
  const window = new Set(dates)
  const selected = rows.filter(row => window.has(row.date))
  const clientTotals = new Map<string, number>()
  for (const row of selected) clientTotals.set(row.client, (clientTotals.get(row.client) || 0) + row.request_count)
  const clients = [...clientTotals.keys()].sort((a, b) => clientTotals.get(b)! - clientTotals.get(a)! || a.localeCompare(b))
  const clientIndex = new Map(clients.map((client, index) => [client, index]))
  const models = new Map<string, { model_id: string; request_count: number; requests: number[]; tokens: number[] }>()
  for (const row of selected) {
    const index = clientIndex.get(row.client)!
    const model = models.get(row.model_id) || { model_id: row.model_id, request_count: 0, requests: clients.map(() => 0), tokens: clients.map(() => 0) }
    model.request_count += row.request_count
    model.requests[index] += row.request_count
    model.tokens[index] += row.input_tokens + row.output_tokens
    models.set(row.model_id, model)
  }
  const series = [...models.values()].sort((a, b) => b.request_count - a.request_count || a.model_id.localeCompare(b.model_id))
  return { clients, series }
}
