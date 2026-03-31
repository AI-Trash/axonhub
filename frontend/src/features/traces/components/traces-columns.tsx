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

import { Trace } from '../data/schema';
import * as m from '@/paraglide/messages';
import { getLocale } from '@/paraglide/runtime';

export function useTracesColumns(): ColumnDef<Trace>[] {
  const locale = getLocale() === 'zh' ? zhCN : enUS;
  const { navigateWithSearch } = usePaginationSearch({ defaultPageSize: 20 });

  const columns: ColumnDef<Trace>[] = [
    {
      accessorKey: 'id',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["common.columns.id"]()} />,
      cell: ({ row }) => {
        const handleClick = useCallback(() => {
          navigateWithSearch({
            to: '/project/traces/$traceId',
            params: { traceId: row.original.id },
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

    // {
    //   id: 'project',
    //   header: ({ column }) => <DataTableColumnHeader column={column} title={m["traces.columns.project"]()} />,
    //   enableSorting: false,
    //   cell: ({ row }) => {
    //     const project = row.original.project
    //     return <div className='font-mono text-xs'>{project?.name || m["traces.columns.unknown"]()}</div>
    //   },
    // },
    {
      accessorKey: 'firstUserQuery',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["traces.columns.userQuery"]()} />,
      enableSorting: false,
      cell: ({ row }) => {
        const query = row.getValue('firstUserQuery') as string | null | undefined;
        return (
          <div className='max-w-64 truncate text-xs' title={query || ''}>
            {query || '-'}
          </div>
        );
      },
    },
    {
      accessorKey: 'traceID',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["traces.columns.traceId"]()} />,
      enableSorting: false,
      cell: ({ row }) => {
        const traceID = row.getValue('traceID') as string;
        return (
          <div className='max-w-64 truncate font-mono text-xs' title={traceID}>
            {traceID}
          </div>
        );
      },
    },
    {
      id: 'thread',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["traces.columns.thread"]()} />,
      enableSorting: false,
      cell: ({ row }) => {
        const thread = row.original.thread;
        if (!thread) {
          return <div className='text-muted-foreground font-mono text-xs'>{m["traces.columns.noThread"]()}</div>;
        }

        const handleNavigate = () => {
          navigateWithSearch({
            to: '/project/threads/$threadId',
            params: { threadId: thread.id },
          });
        };
        return (
          <Button variant='link' size='sm' onClick={handleNavigate} className='hover:text-primary h-auto p-0 font-mono text-xs'>
            #{extractNumberID(thread.id)}
          </Button>
        );
      },
    },
    {
      id: 'requestCount',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["traces.columns.requestCount"]()} />,
      enableSorting: false,
      cell: ({ row }) => {
        const count = row.original.requests?.totalCount || 0;
        return (
          <Badge variant='secondary' className='font-mono text-xs'>
            {count}
          </Badge>
        );
      },
    },
    {
      id: 'details',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["traces.columns.details"]()} />,
      cell: ({ row }) => {
        const handleViewDetails = () => {
          navigateWithSearch({ to: '/project/traces/$traceId', params: { traceId: row.original.id } });
        };

        return (
          <Button variant='outline' size='sm' onClick={handleViewDetails}>
            <FileText className='mr-2 h-4 w-4' />
            {m["traces.actions.viewDetails"]()}
          </Button>
        );
      },
    },
    {
      accessorKey: 'createdAt',
      header: ({ column }) => <DataTableColumnHeader column={column} title={m["common.columns.createdAt"]()} />,
      cell: ({ row }) => {
        const date = new Date(row.getValue('createdAt'));
        return <div className='text-xs'>{format(date, 'yyyy-MM-dd HH:mm:ss', { locale })}</div>;
      },
    },
    // {
    //   accessorKey: 'updatedAt',
    //   header: ({ column }) => <DataTableColumnHeader column={column} title={m["common.columns.updatedAt"]()} />,
    //   cell: ({ row }) => {
    //     const date = new Date(row.getValue('updatedAt'))
    //     return <div className='text-xs'>{format(date, 'yyyy-MM-dd HH:mm:ss', { locale })}</div>
    //   },
    // },
    // {
    //   id: 'actions',
    //   cell: ({ row }) => {
    //     const trace = row.original
    //     const navigate = useNavigate()

    //     return (
    //       <DropdownMenu>
    //         <DropdownMenuTrigger asChild>
    //           <Button variant='ghost' className='h-8 w-8 p-0'>
    //             <span className='sr-only'>{m["traces.actions.openMenu"]()}</span>
    //             <MoreHorizontal className='h-4 w-4' />
    //           </Button>
    //         </DropdownMenuTrigger>
    //         <DropdownMenuContent align='end'>
    //           <DropdownMenuItem onClick={() => {
    //             navigate({ to: '/project/traces/$traceId', params: { traceId: trace.id } })
    //           }}>
    //             <Eye className='mr-2 h-4 w-4' />
    //             {m["traces.actions.viewDetails"]()}
    //           </DropdownMenuItem>
    //         </DropdownMenuContent>
    //       </DropdownMenu>
    //     )
    //   },
    // },
  ];

  return columns;
}
