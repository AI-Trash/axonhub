import { Activity } from 'lucide-react';

import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';
import { Skeleton } from '@/components/ui/skeleton';
import { formatNumber } from '@/utils/format-number';

import { useDashboardStats } from '../data/dashboard';
import * as m from '@/paraglide/messages';

export function TodayRequestsCard() {
  const { data: stats, isLoading, error } = useDashboardStats();

  if (isLoading) {
    return (
      <Card>
        <CardHeader className='flex flex-row items-center justify-between space-y-0 pb-2'>
          <Skeleton className='h-4 w-[120px]' />
          <Skeleton className='h-4 w-4' />
        </CardHeader>
        <CardContent>
          <div className='space-y-2'>
            <Skeleton className='h-8 w-[80px]' />
            <Skeleton className='mt-1 h-4 w-[140px]' />
          </div>
        </CardContent>
      </Card>
    );
  }

  if (error) {
    return (
      <Card>
        <CardHeader className='flex flex-row items-center justify-between space-y-0 pb-2'>
          <div className='flex items-center gap-2'>
            <div className='bg-primary/10 text-primary dark:bg-primary/20 rounded-lg p-1.5'>
              <Activity className='h-4 w-4' />
            </div>
            <CardTitle className='text-sm font-medium'>{m["dashboard.stats.todayRequests"]()}</CardTitle>
          </div>
        </CardHeader>
        <CardContent>
          <div className='text-sm text-red-500'>{m["common.loadError"]()}</div>
        </CardContent>
      </Card>
    );
  }

  return (
    <Card className='bg-primary text-primary-foreground hover-card'>
      <CardHeader className='flex flex-row items-center justify-between space-y-0 pb-2'>
        <div className='flex items-center gap-2'>
          <Activity className='text-primary-foreground/70 h-4 w-4' />
          <CardTitle className='text-primary-foreground/90 text-sm font-medium'>{m["dashboard.stats.todayRequests"]()}</CardTitle>
        </div>
        <div className='bg-primary-foreground h-2 w-2 animate-ping rounded-full' />
      </CardHeader>
      <CardContent>
        <div className='space-y-4'>
          <div className='mt-2 font-mono text-4xl font-bold tracking-tight'>{formatNumber(stats?.requestStats?.requestsToday || 0)}</div>
          <div className='border-primary-foreground/10 text-primary-foreground/70 mt-4 flex justify-between border-t pt-3 text-xs'>
            <span>
              {m["dashboard.stats.thisWeek"]()}: {formatNumber(stats?.requestStats?.requestsThisWeek || 0)}
            </span>
            <span>
              {m["dashboard.stats.thisMonth"]()}: {formatNumber(stats?.requestStats?.requestsThisMonth || 0)}
            </span>
          </div>
        </div>
      </CardContent>
    </Card>
  );
}
