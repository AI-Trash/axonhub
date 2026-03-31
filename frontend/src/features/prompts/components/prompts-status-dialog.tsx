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

import { useUpdatePromptStatus } from '../data/prompts';
import { Prompt } from '../data/schema';
import * as m from '@/paraglide/messages';
import { dynamicTranslation } from '@/lib/paraglide-helpers';

interface PromptsStatusDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentRow: Prompt;
}

export function PromptsStatusDialog({ open, onOpenChange, currentRow }: PromptsStatusDialogProps) {
  const updateStatusMutation = useUpdatePromptStatus();

  const newStatus = currentRow.status === 'enabled' ? 'disabled' : 'enabled';

  const handleConfirm = useCallback(async () => {
    await updateStatusMutation.mutateAsync({
      id: currentRow.id,
      status: newStatus,
    });
    onOpenChange(false);
  }, [currentRow.id, newStatus, updateStatusMutation, onOpenChange]);

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{m["prompts.dialogs.statusChange.title"]()}</AlertDialogTitle>
          <AlertDialogDescription>
            {dynamicTranslation(`prompts.dialogs.statusChange.description.${newStatus}`, { name: currentRow.name })}
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
