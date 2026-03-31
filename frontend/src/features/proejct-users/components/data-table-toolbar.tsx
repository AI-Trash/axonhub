import { Cross2Icon } from '@radix-ui/react-icons';
import { Table } from '@tanstack/react-table';

import { DataTableFacetedFilter } from '@/components/data-table-faceted-filter';
import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';

import { DataTableViewOptions } from './data-table-view-options';
import * as m from '@/paraglide/messages';

interface DataTableToolbarProps<TData> {
  table: Table<TData>;
}

export function DataTableToolbar<TData>({ table }: DataTableToolbarProps<TData>) {
  const isFiltered = table.getState().columnFilters.length > 0;

  return (
    <div className='flex items-center justify-between'>
      <div className='flex flex-1 flex-col-reverse items-start gap-y-2 sm:flex-row sm:items-center sm:space-x-2'>
        <Input
          placeholder={m["users.filterUsers"]()}
          value={(table.getColumn('firstName')?.getFilterValue() as string) ?? ''}
          onChange={(event) => table.getColumn('firstName')?.setFilterValue(event.target.value)}
          className='h-8 w-[150px] lg:w-[250px]'
        />
        <div className='flex gap-x-2'>
          {table.getColumn('status') && (
            <DataTableFacetedFilter
              column={table.getColumn('status')}
              title={m["users.filters.status"]()}
              options={[
                { label: m["users.status.activated"](), value: 'activated' },
                { label: m["users.status.deactivated"](), value: 'deactivated' },
              ]}
            />
          )}
        </div>
        {isFiltered && (
          <Button variant='ghost' onClick={() => table.resetColumnFilters()} className='h-8 px-2 lg:px-3'>
            {m["common.filters.reset"]()}
            <Cross2Icon className='ml-2 h-4 w-4' />
          </Button>
        )}
      </div>
      {/* <DataTableViewOptions table={table} /> */}
    </div>
  );
}
