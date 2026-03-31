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
import { useBulkDisablePromptProtectionRules } from '../data/rules';
import * as m from '@/paraglide/messages';

export function RulesBulkDisableDialog() {
  const { open, setOpen, selectedRules, resetRowSelection } = usePromptProtectionRules();
  const mutation = useBulkDisablePromptProtectionRules();

  const handleConfirm = useCallback(async () => {
    await mutation.mutateAsync(selectedRules.map((rule) => rule.id));
    setOpen(null);
    resetRowSelection?.();
  }, [mutation, resetRowSelection, selectedRules, setOpen]);

  return (
    <AlertDialog open={open === 'bulkDisable'} onOpenChange={(isOpen) => !isOpen && setOpen(null)}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{m["promptProtectionRules.dialogs.bulkDisable.title"]()}</AlertDialogTitle>
          <AlertDialogDescription>
            {m["promptProtectionRules.dialogs.bulkDisable.description"]({ count: selectedRules.length })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{m["common.buttons.cancel"]()}</AlertDialogCancel>
          <AlertDialogAction onClick={handleConfirm} disabled={mutation.isPending}>
            {m["common.buttons.confirm"]()}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
