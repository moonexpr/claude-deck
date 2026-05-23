import { useMemo } from 'react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from '@/components/ui/chart'
import { BarChart, Bar, XAxis, YAxis, CartesianGrid, Cell } from 'recharts'
import type { MonthlyUsage } from '@/types/usage'
import { formatMonth, formatCost, formatTokens } from './utils'

interface MonthlyUsageChartProps {
  data: MonthlyUsage[]
  loading: boolean
}

const chartConfig = {
  cost: {
    label: 'Cost',
    color: 'hsl(var(--chart-1))',
  },
} satisfies ChartConfig

export function MonthlyUsageChart({ data, loading }: MonthlyUsageChartProps) {
  const chartData = useMemo(() => {
    return [...data]
      .sort((a, b) => a.month.localeCompare(b.month))
      .map(d => ({
        month: formatMonth(d.month),
        fullMonth: d.month,
        cost: d.total_cost,
        tokens: d.input_tokens + d.output_tokens,
        models: d.models_used,
      }))
  }, [data])

  const stats = useMemo(() => {
    if (chartData.length === 0) return { total: 0, avg: 0 }
    const total = chartData.reduce((sum, d) => sum + d.cost, 0)
    return {
      total,
      avg: total / chartData.length,
    }
  }, [chartData])

  if (loading && chartData.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Monthly Cost</CardTitle>
          <CardDescription>Loading...</CardDescription>
        </CardHeader>
        <CardContent className="h-[250px] flex items-center justify-center text-muted-foreground">
          Loading chart data...
        </CardContent>
      </Card>
    )
  }

  if (chartData.length === 0) {
    return (
      <Card>
        <CardHeader>
          <CardTitle>Monthly Cost</CardTitle>
          <CardDescription>Cost by month</CardDescription>
        </CardHeader>
        <CardContent className="h-[250px] flex items-center justify-center text-muted-foreground">
          No monthly data available
        </CardContent>
      </Card>
    )
  }

  const maxCost = Math.max(...chartData.map(d => d.cost))

  return (
    <Card>
      <CardHeader>
        <CardTitle>Monthly Cost</CardTitle>
        <CardDescription>
          Total: {formatCost(stats.total)} | Avg: {formatCost(stats.avg)}/month
        </CardDescription>
      </CardHeader>
      <CardContent>
        <ChartContainer config={chartConfig} className="h-[250px] w-full">
          <BarChart data={chartData} accessibilityLayer>
            <CartesianGrid vertical={false} />
            <XAxis
              dataKey="month"
              tickLine={false}
              tickMargin={10}
              axisLine={false}
            />
            <YAxis
              tickLine={false}
              axisLine={false}
              tickFormatter={(value: number) => formatCost(value)}
            />
            <ChartTooltip
              content={
                <ChartTooltipContent
                  labelFormatter={(label, payload) => {
                    const raw = payload?.[0]?.payload
                    const fullMonth = raw && typeof raw['fullMonth'] === 'string' ? raw['fullMonth'] : undefined
                    const tokens = raw && typeof raw['tokens'] === 'number' ? raw['tokens'] : 0
                    return (
                      <div>
                        <div>{fullMonth ?? label}</div>
                        <div className="text-xs text-muted-foreground">
                          {formatTokens(tokens)} tokens
                        </div>
                      </div>
                    )
                  }}
                  formatter={(value) => formatCost(Number(value))}
                />
              }
            />
            <Bar dataKey="cost" radius={[4, 4, 0, 0]}>
              {chartData.map((entry, index) => (
                <Cell
                  key={`cell-${index}`}
                  fill={`hsl(var(--chart-1) / ${0.4 + (entry.cost / maxCost) * 0.6})`}
                />
              ))}
            </Bar>
          </BarChart>
        </ChartContainer>
      </CardContent>
    </Card>
  )
}
