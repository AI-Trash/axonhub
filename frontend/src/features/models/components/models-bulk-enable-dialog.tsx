'use client';

import { IconAlertTriangle, IconCheck } from '@tabler/icons-react';

import { ConfirmDialog } from '@/components/confirm-dialog';

import { useModels } from '../context/models-context';
import { useBulkEnableModels } from '../data/models';
import * as m from '@/paraglide/messages';

export function ModelsBulkEnableDialog() {
  const { open, setOpen, selectedModels, resetRowSelection, setSelectedModels } = useModels();
  const bulkEnableModels = useBulkEnableModels();

  const isDialogOpen = open === 'bulkEnable';
  const selectedCount = selectedModels.length;

  if (selectedCount === 0 && !isDialogOpen) {
    return null;
  }

  const handleConfirm = async () => {
    try {
      const ids = selectedModels.map((model) => model.id);
      if (ids.length === 0) {
        return;
      }

      await bulkEnableModels.mutateAsync(ids);
      resetRowSelection?.();
      setSelectedModels([]);
      setOpen(null);
    } catch (error) {}
  };

  return (
    <ConfirmDialog
      open={isDialogOpen}
      onOpenChange={(isOpen) => {
        if (!isOpen) {
          setOpen(null);
        } else {
          setOpen('bulkEnable');
        }
      }}
      handleConfirm={handleConfirm}
      disabled={selectedCount === 0}
      isLoading={bulkEnableModels.isPending}
      confirmText={m["common.buttons.enable"]()}
      cancelBtnText={m["common.buttons.cancel"]()}
      title={
        <span className='text-primary flex items-center gap-2'>
          <IconAlertTriangle className='h-4 w-4' />
          {m["models.dialogs.bulkEnable.title"]()}
        </span>
      }
      desc={m["models.dialogs.bulkEnable.description"]({ count: selectedCount })}
    >
      <div className='flex items-start gap-3 rounded-md border border-green-200 bg-green-50 p-3 text-sm dark:border-green-900 dark:bg-green-900/20'>
        <IconCheck className='mt-0.5 h-4 w-4 text-green-600 dark:text-green-400' />
        <div className='space-y-1 text-left'>
          <p>{m["models.dialogs.bulkEnable.warning"]()}</p>
        </div>
      </div>
    </ConfirmDialog>
  );
}
