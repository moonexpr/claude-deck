import React from 'react'
import { type LucideIcon } from 'lucide-react'
import { cn } from '@/lib/utils'

interface PageHeaderProps {
  title: string
  description?: string
  icon?: LucideIcon
  children?: React.ReactNode
  className?: string
}

export function PageHeader({ title, description, icon: Icon, children, className }: PageHeaderProps) {
  return (
    <div className={cn("flex flex-col sm:flex-row sm:items-center justify-between gap-4", className)}>
      <div>
        <h1 className="text-2xl md:text-3xl font-bold flex items-center gap-2">
          {Icon && <Icon className="h-6 w-6 md:h-8 md:w-8" />}
          {title}
        </h1>
        {description && (
          <p className="text-xs md:text-sm text-muted-foreground mt-1">
            {description}
          </p>
        )}
      </div>
      <div className="flex flex-wrap items-center gap-2 self-end sm:self-center">
        {children}
      </div>
    </div>
  )
}
