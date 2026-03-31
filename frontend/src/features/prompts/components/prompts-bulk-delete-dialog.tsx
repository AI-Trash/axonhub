import { useCallback } from 'react';

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/ui/alert-dialog';

import { usePrompts } from '../context/prompts-context';
import { useBulkDeletePrompts } from '../data/prompts';
import * as m from '@/paraglide/messages';

export function PromptsBulkDeleteDialog() {
  const { open, setOpen, selectedPrompts, resetRowSelection } = usePrompts();
  const bulkDeleteMutation = useBulkDeletePrompts();

  const handleConfirm = useCallback(async () => {
    const ids = selectedPrompts.map((prompt) => prompt.id);
    await bulkDeleteMutation.mutateAsync(ids);
    setOpen(null);
    resetRowSelection?.();
  }, [selectedPrompts, bulkDeleteMutation, setOpen, resetRowSelection]);

  return (
    <AlertDialog open={open === 'bulkDelete'} onOpenChange={(isOpen) => !isOpen && setOpen(null)}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{m["prompts.dialogs.bulkDelete.title"]()}</AlertDialogTitle>
          <AlertDialogDescription>{m["prompts.dialogs.bulkDelete.description"]({ count: selectedPrompts.length })}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{m["common.buttons.cancel"]()}</AlertDialogCancel>
          <AlertDialogAction
            onClick={handleConfirm}
            disabled={bulkDeleteMutation.isPending}
            className='bg-destructive text-destructive-foreground hover:bg-destructive/90'
          >
            {m["common.buttons.delete"]()}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
