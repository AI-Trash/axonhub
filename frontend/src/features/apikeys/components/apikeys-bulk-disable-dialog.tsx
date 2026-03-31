'use client';

import { IconAlertTriangle } from '@tabler/icons-react';

import { ConfirmDialog } from '@/components/confirm-dialog';

import { useApiKeysContext } from '../context/apikeys-context';
import { useBulkDisableApiKeys } from '../data/apikeys';
import { ApiKey } from '../data/schema';
import * as m from '@/paraglide/messages';

export function ApiKeysBulkDisableDialog() {
  const { isDialogOpen, closeDialog, selectedApiKeys, resetRowSelection, setSelectedApiKeys } = useApiKeysContext();
  const bulkDisableApiKeys = useBulkDisableApiKeys();

  if (!selectedApiKeys || selectedApiKeys.length === 0) return null;

  const handleBulkDisable = async () => {
    try {
      const ids = selectedApiKeys.map((apiKey) => apiKey.id);
      await bulkDisableApiKeys.mutateAsync(ids);
      resetRowSelection();
      setSelectedApiKeys([]);
      closeDialog();
    } catch (error) {}
  };

  return (
    <ConfirmDialog
      open={isDialogOpen.bulkDisable}
      onOpenChange={() => closeDialog('bulkDisable')}
      handleConfirm={handleBulkDisable}
      disabled={bulkDisableApiKeys.isPending}
      title={
        <span className='text-destructive'>
          <IconAlertTriangle className='stroke-destructive mr-1 inline-block' size={18} />
          {m["apikeys.dialogs.bulkDisable.title"]()}
        </span>
      }
      desc={m["apikeys.dialogs.bulkDisable.description"]({ count: selectedApiKeys.length })}
      confirmText={m["common.buttons.disable"]()}
      cancelBtnText={m["common.buttons.cancel"]()}
    />
  );
}
