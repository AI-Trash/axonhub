import { Cross2Icon } from '@radix-ui/react-icons';
import { Table } from '@tanstack/react-table';
import { useMemo } from 'react';

import { DataTableFacetedFilter } from '@/components/data-table-faceted-filter';
import { DateRangePicker } from '@/components/date-range-picker';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { useMe } from '@/features/auth/data/auth';
import { useUsers } from '@/features/users/data/users';
import { useAuthStore } from '@/stores/authStore';
import type { DateTimeRangeValue } from '@/utils/date-range';

import { ApiKeyStatus } from '../data/schema';
import * as m from '@/paraglide/messages';

interface DataTableToolbarProps<TData> {
  table: Table<TData>;
  dateRange?: DateTimeRangeValue;
  onDateRangeChange?: (range: DateTimeRangeValue | undefined) => void;
  onResetFilters?: () => void;
}

export function DataTableToolbar<TData>({ table, dateRange, onDateRangeChange, onResetFilters }: DataTableToolbarProps<TData>) {
  const hasDateRange = !!dateRange?.from || !!dateRange?.to;
  const isFiltered = table.getState().columnFilters.length > 0 || hasDateRange;

  const { user: authUser } = useAuthStore((state) => state.auth);
  const { data: meData } = useMe();
  const user = meData || authUser;
  const userScopes = user?.scopes || [];
  const isOwner = user?.isOwner || false;

  const canViewUsers = isOwner || userScopes.includes('*') || (userScopes.includes('read_users') && userScopes.includes('read_apikeys'));

  const { data: usersData } = useUsers(
    {
      first: 100,
      orderBy: { field: 'CREATED_AT', direction: 'DESC' },
    },
    {
      disableAutoFetch: !canViewUsers,
    }
  );

  const userOptions = useMemo(() => {
    if (!canViewUsers || !usersData?.edges) return [];

    return usersData.edges.map((edge) => ({
      value: edge.node.id,
      label: `${edge.node.firstName} ${edge.node.lastName} (${edge.node.email})`,
    }));
  }, [canViewUsers, usersData]);

  const statusOptions = [
    {
      value: 'enabled' as ApiKeyStatus,
      label: m["apikeys.status.enabled"](),
    },
    {
      value: 'disabled' as ApiKeyStatus,
      label: m["apikeys.status.disabled"](),
    },
    {
      value: 'archived' as ApiKeyStatus,
      label: m["apikeys.status.archived"](),
    },
  ];

  return (
    <div className='flex items-center justify-between'>
      <div className='flex flex-1 items-center space-x-2'>
        <Input
          placeholder={m["apikeys.filters.filterName"]()}
          value={(table.getColumn('name')?.getFilterValue() as string) ?? ''}
          onChange={(event) => table.getColumn('name')?.setFilterValue(event.target.value)}
          className='h-8 w-[150px] lg:w-[250px]'
        />
        {table.getColumn('status') && (
          <DataTableFacetedFilter column={table.getColumn('status')} title={m["apikeys.filters.status"]()} options={statusOptions} />
        )}
        {canViewUsers && table.getColumn('creator') && userOptions.length > 0 && usersData?.edges && (
          <DataTableFacetedFilter column={table.getColumn('creator')} title={m["apikeys.filters.creator"]()} options={userOptions} />
        )}
        <DateRangePicker value={dateRange} onChange={onDateRangeChange} />
        {isFiltered && (
          <Button
            variant='ghost'
            onClick={() => {
              table.resetColumnFilters();
              onDateRangeChange?.(undefined);
              onResetFilters?.();
            }}
            className='h-8 px-2 lg:px-3'
          >
            {m["common.filters.reset"]()}
            <Cross2Icon className='ml-2 h-4 w-4' />
          </Button>
        )}
      </div>
    </div>
  );
}
