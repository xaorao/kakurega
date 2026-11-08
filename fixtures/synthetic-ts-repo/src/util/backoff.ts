export function retryDelay(attempt: number): number {
  return Math.min(2 ** attempt * 100, 30000);
}
