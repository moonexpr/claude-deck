/**
 * Lightweight date-distance formatter.
 * Replaces date-fns formatDistanceToNow (not in package.json).
 */
export function formatDistanceToNow(date: Date): string {
  const now = new Date()
  const diff = now.getTime() - date.getTime()
  const seconds = Math.floor(diff / 1000)
  const minutes = Math.floor(seconds / 60)
  const hours = Math.floor(minutes / 60)
  const days = Math.floor(hours / 24)
  const months = Math.floor(days / 30)
  const years = Math.floor(days / 365)

  if (years > 1) return `${years} years ago`
  if (years === 1) return 'about 1 year ago'
  if (months > 1) return `${months} months ago`
  if (months === 1) return 'about 1 month ago'
  if (days > 1) return `${days} days ago`
  if (days === 1) return 'about 1 day ago'
  if (hours > 1) return `${hours} hours ago`
  if (hours === 1) return 'about 1 hour ago'
  if (minutes > 1) return `${minutes} minutes ago`
  if (minutes === 1) return '1 minute ago'
  return 'just now'
}
