'use client';

import { ColumnDef, Row, Table } from '@tanstack/react-table';
import { format } from 'date-fns';

import { Checkbox } from '@/components/ui/checkbox';

import { Role } from '../data/schema';
import { DataTableRowActions } from './data-table-row-actions';
import { ScopesCell } from './scopes-cell';
import * as m from '@/paraglide/messages';

export const createColumns = (t: (key: string, params?: Record<string, unknown>) => string, canWrite: boolean = false): ColumnDef<Role>[] => {
  const columns: ColumnDef<Role>[] = [
    {
      id: 'search',
      header: () => null,
      cell: () => null,
      enableSorting: false,
      enableHiding: false,
      enableColumnFilter: true,
      enableGlobalFilter: false,
      getUniqueValues: () => [],
    },
  ];

  // Only show select column if user has write permissions (for potential bulk operations)
  if (canWrite) {
    columns.push({
      id: 'select',
      header: ({ table }: { table: Table<Role> }) => (
        <Checkbox
          checked={table.getIsAllPageRowsSelected() || (table.getIsSomePageRowsSelected() && 'indeterminate')}
          onCheckedChange={(value) => table.toggleAllPageRowsSelected(!!value)}
          aria-label={m["common.columns.selectAll"]()}
          className='translate-y-[2px]'
        />
      ),
      cell: ({ row }: { row: Row<Role> }) => (
        <Checkbox
          checked={row.getIsSelected()}
          onCheckedChange={(value) => row.toggleSelected(!!value)}
          aria-label={m["common.columns.selectRow"]()}
          className='translate-y-[2px]'
        />
      ),
      enableSorting: false,
      enableHiding: false,
    });
  }

  // Add other columns
  columns.push(
    {
      accessorKey: 'name',
      header: m["common.columns.name"](),
      cell: ({ row }) => {
        const name = row.getValue('name') as string;
        return <div className='font-medium'>{name}</div>;
      },
    },
    {
      accessorKey: 'scopes',
      header: m["roles.columns.scopes"](),
      cell: ({ row }) => {
        const scopes = row.getValue('scopes') as string[];
        return <ScopesCell scopes={scopes} />;
      },
    },
    {
      accessorKey: 'createdAt',
      header: m["common.columns.createdAt"](),
      cell: ({ row }) => {
        const date = row.getValue('createdAt') as Date;
        return <div className='text-muted-foreground'>{format(date, 'yyyy-MM-dd HH:mm')}</div>;
      },
    },
    {
      accessorKey: 'updatedAt',
      header: m["common.columns.updatedAt"](),
      cell: ({ row }) => {
        const date = row.getValue('updatedAt') as Date;
        return <div className='text-muted-foreground'>{format(date, 'yyyy-MM-dd HH:mm')}</div>;
      },
    },
    {
      id: 'actions',
      cell: ({ row }) => <DataTableRowActions row={row} />,
    }
  );

  return columns;
};
