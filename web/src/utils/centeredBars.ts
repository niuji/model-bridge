export function centeredBar(index: number, active: number[], bandWidth: number) {
  const position = active.indexOf(index)
  if (position < 0) return null
  const width = Math.min(22, bandWidth * 0.7 / (active.length * 1.25))
  return { offset: (position - (active.length - 1) / 2) * width * 1.25 - width / 2, width }
}
