import { IconDotsVertical, IconEdit, IconTrash } from '@tabler/icons-react';
import { Row } from '@tanstack/react-table';

import { PermissionGuard } from '@/components/permission-guard';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';

import { usePrompts } from '../context/prompts-context';
import { Prompt } from '../data/schema';
import * as m from '@/paraglide/messages';

interface DataTableRowActionsProps {
  row: Row<Prompt>;
}

export function DataTableRowActions({ row }: DataTableRowActionsProps) {
  const { setOpen, setCurrentRow } = usePrompts();
  const prompt = row.original;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant='ghost' className='data-[state=open]:bg-muted flex h-8 w-8 p-0'>
          <IconDotsVertical className='h-4 w-4' />
          <span className='sr-only'>{m["common.buttons.openMenu"]()}</span>
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align='end' className='w-[160px]'>
        <PermissionGuard requiredScope='write_prompts'>
          <>
            <DropdownMenuItem
              onClick={() => {
                setCurrentRow(prompt);
                setOpen('edit');
              }}
            >
              <IconEdit className='mr-2 h-4 w-4' />
              {m["common.buttons.edit"]()}
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onClick={() => {
                setCurrentRow(prompt);
                setOpen('delete');
              }}
              className='text-destructive focus:text-destructive'
            >
              <IconTrash className='mr-2 h-4 w-4' />
              {m["common.buttons.delete"]()}
            </DropdownMenuItem>
          </>
        </PermissionGuard>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
