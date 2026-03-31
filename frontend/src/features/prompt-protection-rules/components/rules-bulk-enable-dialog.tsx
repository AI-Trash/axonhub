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
import { useBulkEnablePromptProtectionRules } from '../data/rules';
import * as m from '@/paraglide/messages';

export function RulesBulkEnableDialog() {
  const { open, setOpen, selectedRules, resetRowSelection } = usePromptProtectionRules();
  const mutation = useBulkEnablePromptProtectionRules();

  const handleConfirm = useCallback(async () => {
    await mutation.mutateAsync(selectedRules.map((rule) => rule.id));
    setOpen(null);
    resetRowSelection?.();
  }, [mutation, resetRowSelection, selectedRules, setOpen]);

  return (
    <AlertDialog open={open === 'bulkEnable'} onOpenChange={(isOpen) => !isOpen && setOpen(null)}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{m["promptProtectionRules.dialogs.bulkEnable.title"]()}</AlertDialogTitle>
          <AlertDialogDescription>
            {m["promptProtectionRules.dialogs.bulkEnable.description"]({ count: selectedRules.length })}
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
