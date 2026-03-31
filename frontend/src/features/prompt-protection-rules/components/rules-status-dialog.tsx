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

import { useUpdatePromptProtectionRuleStatus } from '../data/rules';
import { PromptProtectionRule } from '../data/schema';
import * as m from '@/paraglide/messages';
import { dynamicTranslation } from '@/lib/paraglide-helpers';

interface RulesStatusDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentRow: PromptProtectionRule;
}

export function RulesStatusDialog({ open, onOpenChange, currentRow }: RulesStatusDialogProps) {
  const updateStatusMutation = useUpdatePromptProtectionRuleStatus();
  const newStatus = currentRow.status === 'enabled' ? 'disabled' : 'enabled';

  const handleConfirm = useCallback(async () => {
    await updateStatusMutation.mutateAsync({
      id: currentRow.id,
      status: newStatus,
    });
    onOpenChange(false);
  }, [currentRow.id, newStatus, onOpenChange, updateStatusMutation]);

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{m["promptProtectionRules.dialogs.statusChange.title"]()}</AlertDialogTitle>
          <AlertDialogDescription>
            {dynamicTranslation(`promptProtectionRules.dialogs.statusChange.description.${newStatus}`, { name: currentRow.name })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{m["common.buttons.cancel"]()}</AlertDialogCancel>
          <AlertDialogAction onClick={handleConfirm} disabled={updateStatusMutation.isPending}>
            {m["common.buttons.confirm"]()}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
