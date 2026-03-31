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
import { useBulkDisablePrompts } from '../data/prompts';
import * as m from '@/paraglide/messages';

export function PromptsBulkDisableDialog() {
  const { open, setOpen, selectedPrompts, resetRowSelection } = usePrompts();
  const bulkDisableMutation = useBulkDisablePrompts();

  const handleConfirm = useCallback(async () => {
    const ids = selectedPrompts.map((prompt) => prompt.id);
    await bulkDisableMutation.mutateAsync(ids);
    setOpen(null);
    resetRowSelection?.();
  }, [selectedPrompts, bulkDisableMutation, setOpen, resetRowSelection]);

  return (
    <AlertDialog open={open === 'bulkDisable'} onOpenChange={(isOpen) => !isOpen && setOpen(null)}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{m["prompts.dialogs.bulkDisable.title"]()}</AlertDialogTitle>
          <AlertDialogDescription>{m["prompts.dialogs.bulkDisable.description"]({ count: selectedPrompts.length })}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{m["common.buttons.cancel"]()}</AlertDialogCancel>
          <AlertDialogAction onClick={handleConfirm} disabled={bulkDisableMutation.isPending}>
            {m["common.buttons.confirm"]()}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
