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
import { useBulkEnablePrompts } from '../data/prompts';
import * as m from '@/paraglide/messages';

export function PromptsBulkEnableDialog() {
  const { open, setOpen, selectedPrompts, resetRowSelection } = usePrompts();
  const bulkEnableMutation = useBulkEnablePrompts();

  const handleConfirm = useCallback(async () => {
    const ids = selectedPrompts.map((prompt) => prompt.id);
    await bulkEnableMutation.mutateAsync(ids);
    setOpen(null);
    resetRowSelection?.();
  }, [selectedPrompts, bulkEnableMutation, setOpen, resetRowSelection]);

  return (
    <AlertDialog open={open === 'bulkEnable'} onOpenChange={(isOpen) => !isOpen && setOpen(null)}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{m["prompts.dialogs.bulkEnable.title"]()}</AlertDialogTitle>
          <AlertDialogDescription>{m["prompts.dialogs.bulkEnable.description"]({ count: selectedPrompts.length })}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{m["common.buttons.cancel"]()}</AlertDialogCancel>
          <AlertDialogAction onClick={handleConfirm} disabled={bulkEnableMutation.isPending}>
            {m["common.buttons.confirm"]()}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
