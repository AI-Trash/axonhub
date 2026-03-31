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

import { usePromptProtectionRules } from '../context/rules-context';
import { useBulkDeletePromptProtectionRules } from '../data/rules';
import * as m from '@/paraglide/messages';

export function RulesBulkDeleteDialog() {
  const { open, setOpen, selectedRules, resetRowSelection } = usePromptProtectionRules();
  const mutation = useBulkDeletePromptProtectionRules();

  const handleConfirm = useCallback(async () => {
    await mutation.mutateAsync(selectedRules.map((rule) => rule.id));
    setOpen(null);
    resetRowSelection?.();
  }, [mutation, resetRowSelection, selectedRules, setOpen]);

  return (
    <AlertDialog open={open === 'bulkDelete'} onOpenChange={(isOpen) => !isOpen && setOpen(null)}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{m["promptProtectionRules.dialogs.bulkDelete.title"]()}</AlertDialogTitle>
          <AlertDialogDescription>
            {m["promptProtectionRules.dialogs.bulkDelete.description"]({ count: selectedRules.length })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{m["common.buttons.cancel"]()}</AlertDialogCancel>
          <AlertDialogAction
            onClick={handleConfirm}
            disabled={mutation.isPending}
            className='bg-destructive text-destructive-foreground hover:bg-destructive/90'
          >
            {m["common.buttons.delete"]()}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
