'use client';

import { IconAlertTriangle } from '@tabler/icons-react';

import { ConfirmDialog } from '@/components/confirm-dialog';

import { useApiKeysContext } from '../context/apikeys-context';
import { useBulkArchiveApiKeys } from '../data/apikeys';
import * as m from '@/paraglide/messages';

export function ApiKeysBulkArchiveDialog() {
  const { isDialogOpen, closeDialog, selectedApiKeys, resetRowSelection, setSelectedApiKeys } = useApiKeysContext();
  const bulkArchiveApiKeys = useBulkArchiveApiKeys();

  if (!selectedApiKeys || selectedApiKeys.length === 0) return null;

  const handleBulkArchive = async () => {
    try {
      const ids = selectedApiKeys.map((apiKey) => apiKey.id);
      await bulkArchiveApiKeys.mutateAsync(ids);
      resetRowSelection();
      setSelectedApiKeys([]);
      closeDialog();
    } catch (error) {}
  };

  return (
    <ConfirmDialog
      open={isDialogOpen.bulkArchive}
      onOpenChange={() => closeDialog('bulkArchive')}
      handleConfirm={handleBulkArchive}
      disabled={bulkArchiveApiKeys.isPending}
      title={
        <span className='text-destructive'>
          <IconAlertTriangle className='stroke-destructive mr-1 inline-block' size={18} />
          {m["apikeys.dialogs.bulkArchive.title"]()}
        </span>
      }
      desc={m["apikeys.dialogs.bulkArchive.description"]({ count: selectedApiKeys.length })}
      confirmText={m["common.buttons.archive"]()}
      cancelBtnText={m["common.buttons.cancel"]()}
    />
  );
}
