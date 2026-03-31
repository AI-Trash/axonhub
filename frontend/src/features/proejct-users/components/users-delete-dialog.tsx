'use client';

import { IconAlertTriangle } from '@tabler/icons-react';
import { useState } from 'react';
import { toast } from 'sonner';

import { ConfirmDialog } from '@/components/confirm-dialog';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

import { User } from '../data/schema';
import { useRemoveUserFromProject } from '../data/users';
import * as m from '@/paraglide/messages';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentRow: User;
}

export function UsersDeleteDialog({ open, onOpenChange, currentRow }: Props) {
  const [confirmText, setConfirmText] = useState('');
  const removeUser = useRemoveUserFromProject();

  const fullName = `${currentRow.firstName} ${currentRow.lastName}`;

  const handleRemove = async () => {
    if (confirmText.trim() !== fullName) return;

    try {
      await removeUser.mutateAsync(currentRow.id);
      toast.success(m["users.messages.removeFromProjectSuccess"]());
      onOpenChange(false);
      setConfirmText('');
    } catch (error) {
      toast.error(m["common.errors.somethingWentWrong"]());
    }
  };

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={(state) => {
        onOpenChange(state);
        if (!state) setConfirmText('');
      }}
      handleConfirm={handleRemove}
      disabled={confirmText.trim() !== fullName || removeUser.isPending}
      title={
        <span className='text-destructive'>
          <IconAlertTriangle className='stroke-destructive mr-1 inline-block' size={18} /> {m["users.dialogs.remove.title"]()}
        </span>
      }
      desc={
        <div className='space-y-4'>
          <p className='mb-2'>{m["users.dialogs.remove.description"]({ name: fullName })}</p>

          <Label className='my-2'>
            {m["users.dialogs.remove.confirmLabel"]()}
            <Input
              value={confirmText}
              onChange={(e) => setConfirmText(e.target.value)}
              placeholder={m["users.dialogs.remove.confirmPlaceholder"]()}
              data-testid='remove-confirmation-input'
            />
          </Label>

          <Alert variant='destructive'>
            <AlertTitle>{m["users.dialogs.remove.warningTitle"]()}</AlertTitle>
            <AlertDescription>{m["users.dialogs.remove.warningDescription"]()}</AlertDescription>
          </Alert>
        </div>
      }
      confirmText={removeUser.isPending ? m["users.buttons.removing"]() : m["users.buttons.remove"]()}
      destructive
      data-testid='remove-dialog'
    />
  );
}
