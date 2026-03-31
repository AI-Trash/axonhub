'use client';

import { IconUserCheck, IconUserOff } from '@tabler/icons-react';
import { useState } from 'react';
import { toast } from 'sonner';

import { ConfirmDialog } from '@/components/confirm-dialog';

import { User } from '../data/schema';
import { useUpdateUserStatus } from '../data/users';
import * as m from '@/paraglide/messages';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentRow: User;
}

export function UsersStatusDialog({ open, onOpenChange, currentRow }: Props) {
  const updateUserStatus = useUpdateUserStatus();
  const isActivated = currentRow.status === 'activated';
  const newStatus = isActivated ? 'deactivated' : 'activated';
  const actionText = isActivated ? m["users.actions.deactivate"]() : m["users.actions.activate"]();

  const handleStatusChange = async () => {
    try {
      await updateUserStatus.mutateAsync({
        id: currentRow.id,
        status: newStatus,
      });
      onOpenChange(false);
    } catch (error) {
      toast.error(m["common.errors.somethingWentWrong"]());
    }
  };

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={onOpenChange}
      handleConfirm={handleStatusChange}
      disabled={updateUserStatus.isPending}
      title={
        <span className={isActivated ? 'text-destructive' : 'text-green-600'}>
          {isActivated ? (
            <IconUserOff className='mr-1 inline-block' size={18} />
          ) : (
            <IconUserCheck className='mr-1 inline-block' size={18} />
          )}
          {m["users.dialogs.statusChange.title"]({ action: actionText })}
        </span>
      }
      desc={
        <div className='space-y-2'>
          <p>
            {t('users.dialogs.statusChange.confirmMessage', {
              action: actionText,
              name: `${currentRow.firstName} ${currentRow.lastName}`,
            })}
          </p>
          <p className='text-muted-foreground text-sm'>
            {isActivated ? m["users.dialogs.statusChange.deactivateWarning"]() : m["users.dialogs.statusChange.activateInfo"]()}
          </p>
        </div>
      }
      confirmText={actionText}
      cancelBtnText={m["common.buttons.cancel"]()}
    />
  );
}
