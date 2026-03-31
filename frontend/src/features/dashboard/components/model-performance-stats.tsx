import { useMemo } from 'react';

import { useModelPerformanceStats, ModelPerformanceStat } from '../data/dashboard';
import { PerformanceChart, PerformanceDataPoint } from './performance-chart';
import * as m from '@/paraglide/messages';

interface ModelPerformanceStatsProps {
  onTotalRequestsChange?: (total: number) => void;
}

export function ModelPerformanceStats({ onTotalRequestsChange }: ModelPerformanceStatsProps) {
  const { data: performanceStats, isLoading, error } = useModelPerformanceStats();

  const mappedData: PerformanceDataPoint[] | undefined = useMemo(
    () =>
      performanceStats?.map((stat: ModelPerformanceStat) => ({
        id: stat.modelId,
        name: stat.modelId,
        throughput: stat.throughput,
        ttftMs: stat.ttftMs,
        requestCount: stat.requestCount,
        date: stat.date,
      })),
    [performanceStats]
  );

  return (
    <PerformanceChart
      data={mappedData}
      isLoading={isLoading}
      error={error}
      onTotalRequestsChange={onTotalRequestsChange}
      emptyMessage={m["dashboard.charts.noModelData"]()}
      errorMessage={m["dashboard.charts.errorLoadingModelData"]()}
      idField='modelId'
    />
  );
}
