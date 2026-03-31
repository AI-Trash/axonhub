'use client';

import { ColumnDef, Row, Table } from '@tanstack/react-table';

import LongText from '@/components/long-text';
import { Badge } from '@/components/ui/badge';
import { Checkbox } from '@/components/ui/checkbox';

import { User } from '../data/schema';
import { DataTableRowActions } from './data-table-row-actions';
import * as m from '@/paraglide/messages';

export const createColumns = (t: (key: string, params?: Record<string, unknown>) => string, canWrite: boolean = false): ColumnDef<User>[] => {
  const columns: ColumnDef<User>[] = [];

  // Only show select column if user has write permissions (for potential bulk operations)
  if (canWrite) {
    columns.push({
      id: 'select',
      header: ({ table }: { table: Table<User> }) => (
        <Checkbox
          checked={table.getIsAllPageRowsSelected() || (table.getIsSomePageRowsSelected() && 'indeterminate')}
          onCheckedChange={(value) => table.toggleAllPageRowsSelected(!!value)}
          aria-label='Select all'
        />
      ),
      cell: ({ row }: { row: Row<User> }) => (
        <Checkbox checked={row.getIsSelected()} onCheckedChange={(value) => row.toggleSelected(!!value)} aria-label='Select row' />
      ),
      enableSorting: false,
      enableHiding: false,
    });
  }

  // Add other columns
  columns.push(
    {
      accessorKey: 'firstName',
      header: m["users.columns.firstName"](),
      cell: ({ row }) => <LongText>{row.getValue('firstName')}</LongText>,
    },
    {
      accessorKey: 'lastName',
      header: m["users.columns.lastName"](),
      cell: ({ row }) => <LongText>{row.getValue('lastName')}</LongText>,
    },
    {
      accessorKey: 'email',
      header: m["users.columns.email"](),
      cell: ({ row }) => <LongText>{row.getValue('email')}</LongText>,
    },
    {
      accessorKey: 'isOwner',
      header: m["users.columns.owner"](),
      cell: ({ row }) => {
        const isOwner = row.getValue('isOwner') as boolean;
        return isOwner ? (
          <Badge variant='default'>{m["users.badges.owner"]()}</Badge>
        ) : (
          <Badge variant='secondary'>{m["users.badges.user"]()}</Badge>
        );
      },
    },
    {
      accessorKey: 'roles',
      header: m["users.columns.roles"](),
      cell: ({ row }) => {
        const user = row.original;
        const roles = user.roles?.edges?.map((edge) => edge.node);
        if (!roles || roles.length === 0) {
          return <span className='text-muted-foreground'>{m["users.badges.noRoles"]()}</span>;
        }
        return (
          <div className='flex flex-wrap gap-1'>
            {roles.map((role) => (
              <Badge key={role.id} variant='outline'>
                {role.name}
              </Badge>
            ))}
          </div>
        );
      },
    },
    {
      accessorKey: 'status',
      header: m["common.columns.status"](),
      cell: ({ row }) => {
        const status = row.getValue('status') as string;
        return (
          <Badge variant={status === 'activated' ? 'default' : 'secondary'}>
            {status === 'activated' ? m["users.status.activated"]() : m["users.status.deactivated"]()}
          </Badge>
        );
      },
    },
    {
      accessorKey: 'createdAt',
      header: m["common.columns.createdAt"](),
      cell: ({ row }) => {
        const date = new Date(row.getValue('createdAt'));
        return date.toLocaleDateString();
      },
    },
    {
      accessorKey: 'updatedAt',
      header: m["common.columns.updatedAt"](),
      cell: ({ row }) => {
        const date = new Date(row.getValue('updatedAt'));
        return date.toLocaleDateString();
      },
    },
    {
      id: 'actions',
      header: m["common.columns.actions"](),
      cell: ({ row }) => <DataTableRowActions row={row} />,
    }
  );

  return columns;
};
