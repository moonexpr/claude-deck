/**
 * In-memory cache for usage data — scoped to the usage feature.
 * The legacy app used @/lib/usageCache which is not present in this tree;
 * the cache is usage-specific so it lives here.
 */

const STALE_AFTER_MS = 60_000 // 1 minute

interface CacheEntry<T> {
  data: T
  timestamp: number
}

const store = new Map<string, CacheEntry<unknown>>()

function cacheKey(projectPath: string | null): string {
  return projectPath ?? '__all__'
}

export function getFromCache<T>(projectPath: string | null): T | null {
  const entry = store.get(cacheKey(projectPath)) as CacheEntry<T> | undefined
  return entry?.data ?? null
}

export function saveToCache<T>(projectPath: string | null, data: T): void {
  store.set(cacheKey(projectPath), { data, timestamp: Date.now() })
}

export function isCacheStale(projectPath: string | null): boolean {
  const entry = store.get(cacheKey(projectPath))
  if (!entry) return true
  return Date.now() - entry.timestamp > STALE_AFTER_MS
}

export function invalidateCache(projectPath: string | null): void {
  store.delete(cacheKey(projectPath))
}
