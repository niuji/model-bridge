interface Subset { sum: number; mask: number }

function subsets(weights: number[]): Subset[] {
  const result: Subset[] = [{ sum: 0, mask: 0 }]
  weights.forEach((weight, index) => {
    const length = result.length
    for (let i = 0; i < length; i++) result.push({ sum: result[i].sum + weight, mask: result[i].mask | (1 << index) })
  })
  return result.sort((a, b) => a.sum - b.sum)
}

export function balancedColumns<T>(items: T[], weight: (item: T) => number, closeHeight = 0): [T[], T[]] {
  const weights = items.map(weight)
  const selected = new Set<number>()
  if (items.length >= 40) {
    // 超出折半枚举的适用规模时，避免指数级分配阻塞页面。
    const totals = [0, 0]
    weights.map((value, index) => ({ value, index })).sort((a, b) => b.value - a.value).forEach(({ value, index }) => {
      const column = totals[0] <= totals[1] ? 0 : 1
      totals[column] += value
      if (column === 0) selected.add(index)
    })
  } else {
    const middle = Math.floor(weights.length / 2)
    const left = subsets(weights.slice(0, middle))
    const right = subsets(weights.slice(middle))
    const total = weights.reduce((sum, value) => sum + value, 0)
    let i = 0
    let j = right.length - 1
    let bestDifference = Infinity
    let leftMask = 0
    let rightMask = 0
    while (i < left.length && j >= 0) {
      const sum = left[i].sum + right[j].sum
      const difference = Math.abs(total - 2 * sum)
      if (difference < bestDifference) {
        bestDifference = difference
        leftMask = left[i].mask
        rightMask = right[j].mask
      }
      if (difference === 0) break
      if (2 * sum < total) i++
      else j--
    }
    weights.forEach((_, index) => {
      if (index < middle ? leftMask & (1 << index) : rightMask & (1 << (index - middle))) selected.add(index)
    })
  }
  const columns: [T[], T[]] = [[], []]
  // 列内保持原有排序，分组完成后只调整左右位置。
  const firstSelected = selected.has(0)
  items.forEach((item, index) => columns[selected.has(index) === firstSelected ? 0 : 1].push(item))
  const totals = columns.map(column => column.reduce((sum, item) => sum + weight(item), 0))
  const close = Math.abs(totals[0] - totals[1]) <= closeHeight
  const swap = close && columns[0].length !== columns[1].length
    ? columns[0].length > columns[1].length
    : totals[0] < totals[1]
  if (columns[0].length && columns[1].length && swap) return [columns[1], columns[0]]
  return columns
}
