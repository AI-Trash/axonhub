
import { useChannelPerformanceStats } from '../data/dashboard';
import { PerformanceChart } from './performance-chart';
import type { PerformanceDataPoint } from './performance-chart';
import * as m from '@/paraglide/messages';

interface ChannelPerformanceStatsProps {
  onTotalRequestsChange?: (total: number) => void;
}

export function ChannelPerformanceStats({ onTotalRequestsChange }: ChannelPerformanceStatsProps) {
  const { data: performanceStats, isLoading, error } = useChannelPerformanceStats();

  const mappedData = performanceStats?.map((stat) => ({
    date: stat.date,
    id: stat.channelId,
    name: stat.channelName,
    throughput: stat.throughput,
    ttftMs: stat.ttftMs,
    requestCount: stat.requestCount,
  }));

  return (
    <PerformanceChart
      data={mappedData}
      isLoading={isLoading}
      error={error}
      onTotalRequestsChange={onTotalRequestsChange}
      emptyMessage={m["dashboard.charts.noChannelData"]()}
      errorMessage={m["dashboard.charts.errorLoadingChannelData"]()}
      idField='channelId'
      nameField='channelName'
    />
  );
}
