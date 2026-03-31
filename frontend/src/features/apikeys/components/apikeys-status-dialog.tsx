'use client';

import { IconAlertTriangle } from '@tabler/icons-react';

import { ConfirmDialog } from '@/components/confirm-dialog';

import { useApiKeysContext } from '../context/apikeys-context';
import { useUpdateApiKeyStatus } from '../data/apikeys';
import * as m from '@/paraglide/messages';

export function ApiKeysStatusDialog() {
  const { isDialogOpen, closeDialog, selectedApiKey, resetRowSelection } = useApiKeysContext();
  const updateApiKeyStatus = useUpdateApiKeyStatus();

  if (!selectedApiKey) return null;

  const handleStatusChange = async () => {
    const newStatus = selectedApiKey.status === 'enabled' ? 'disabled' : 'enabled';

    try {
      await updateApiKeyStatus.mutateAsync({
        id: selectedApiKey.id,
        status: newStatus,
      });
      closeDialog('status');
      resetRowSelection(); // 清空选中的行
    } catch (error) {}
  };

  const isDisabling = selectedApiKey.status === 'enabled';

  return (
    <ConfirmDialog
      open={isDialogOpen.status}
      onOpenChange={() => closeDialog('status')}
      handleConfirm={handleStatusChange}
      disabled={updateApiKeyStatus.isPending}
      title={
        <span className={isDisabling ? 'text-destructive' : 'text-green-600'}>
          <IconAlertTriangle className={`${isDisabling ? 'stroke-destructive' : 'stroke-green-600'} mr-1 inline-block`} size={18} />
          {isDisabling ? m["apikeys.dialogs.status.disableTitle"]() : m["apikeys.dialogs.status.enableTitle"]()}
        </span>
      }
      desc={
        isDisabling
          ? m["apikeys.dialogs.status.disableDescription"]({ name: selectedApiKey.name })
          : m["apikeys.dialogs.status.enableDescription"]({ name: selectedApiKey.name })
      }
      confirmText={isDisabling ? m["common.buttons.disable"]() : m["common.buttons.enable"]()}
      cancelBtnText={m["common.buttons.cancel"]()}
    />
  );
}
