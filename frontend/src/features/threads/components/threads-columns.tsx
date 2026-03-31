'use client';

import { ColumnDef } from '@tanstack/react-table';
import { format } from 'date-fns';
import { zhCN, enUS } from 'date-fns/locale';
import { FileText } from 'lucide-react';
import { useCallback } from 'react';

import { DataTableColumnHeader } from '@/components/data-table-column-header';
import { Badge } from '@/components/ui/badge';
import { Button } from '@/components/ui/button';
import { usePaginationSearch } from '@/hooks/use-pagination-search';
import { extractNumberID } from '@/lib/utils';

import type { Thread } from '../data/schema';
import * as m from '@/paraglide/messages';
import { getLocale } from '@/paraglide/runtime';

export function useThreadsColumns(): ColumnDef<Thread>[] {
  const locale = getLocale() === 'zh' ? zhCN : enUS;
  const { navigateWithSearch } = usePaginationSearch({ defaultPageSize: 20 });

  const columns: ColumnDef<Thread>[] = [
    {
      accessorKey: 'id',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["common.columns.id"]()} />,
      cell: ({ row }) => {
        const handleClick = useCallback(() => {
          navigateWithSearch({
            to: '/project/threads/$threadId',
            params: { threadId: row.original.id },
          });
        }, [row.original.id, navigateWithSearch]);

        return (
          <button onClick={handleClick} className='text-primary cursor-pointer font-mono text-xs hover:underline'>
            #{extractNumberID(row.getValue('id'))}
          </button>
        );
      },
      enableSorting: true,
      enableHiding: false,
    },
    {
      accessorKey: 'threadID',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["threads.columns.threadId"]()} />,
      cell: ({ row }) => {
        const threadID = row.getValue('threadID') as string;
        return (
          <div className='max-w-64 truncate font-mono text-xs' title={threadID}>
            {threadID}
          </div>
        );
      },
      enableSorting: false,
    },
    {
      accessorKey: 'firstUserQuery',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["threads.columns.firstUserQuery"]()} />,
      cell: ({ row }) => {
        const query = row.getValue('firstUserQuery') as string | null | undefined;
        return (
          <div className='max-w-96 truncate text-xs' title={query || ''}>
            {query || '-'}
          </div>
        );
      },
      enableSorting: false,
    },
    // {
    //   id: 'project',
    //   header: ({ column }) => <DataTableColumnHeader column={column} title={m["threads.columns.project"]()} />,
    //   cell: ({ row }) => {
    //     const project = row.original.project
    //     return (
    //       <div className='max-w-48 truncate text-xs' title={project?.name || ''}>
    //         {project?.name || m["threads.columns.unknownProject"]()}
    //       </div>
    //     )
    //   },
    //   enableSorting: false,
    // },
    {
      id: 'traceCount',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["threads.columns.traceCount"]()} />,
      cell: ({ row }) => {
        const count = row.original.tracesSummary?.totalCount ?? 0;
        return (
          <Badge variant='secondary' className='font-mono text-xs'>
            {count}
          </Badge>
        );
      },
      enableSorting: false,
    },

    {
      accessorKey: 'createdAt',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["common.columns.createdAt"]()} />,
      cell: ({ row }) => {
        const date = new Date(row.getValue('createdAt'));
        return <div className='text-xs'>{format(date, 'yyyy-MM-dd HH:mm:ss', { locale })}</div>;
      },
    },
    {
      id: 'details',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["threads.columns.details"]()} />,
      cell: ({ row }) => {
        const handleViewDetails = () => {
          navigateWithSearch({
            to: '/project/threads/$threadId',
            params: { threadId: row.original.id },
          });
        };

        return (
          <Button variant='outline' size='sm' onClick={handleViewDetails}>
            <FileText className='mr-2 h-4 w-4' />
            {m["threads.actions.viewDetails"]()}
          </Button>
        );
      },
    },
  ];

  return columns;
}
