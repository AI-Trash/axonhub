'use client';

import { IconAlertTriangle } from '@tabler/icons-react';
import { useState } from 'react';

import { ConfirmDialog } from '@/components/confirm-dialog';
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';

import { useDeleteChannel } from '../data/channels';
import { Channel } from '../data/schema';
import * as m from '@/paraglide/messages';

interface Props {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentRow: Channel;
}

export function ChannelsDeleteDialog({ open, onOpenChange, currentRow }: Props) {
  const [value, setValue] = useState('');
  const deleteChannel = useDeleteChannel();

  const handleDelete = async () => {
    if (value.trim() !== currentRow.name) return;

    try {
      await deleteChannel.mutateAsync(currentRow.id);
      onOpenChange(false);
      setValue('');
    } catch (error) {}
  };

  return (
    <ConfirmDialog
      open={open}
      onOpenChange={(state) => {
        if (!state) setValue('');
        onOpenChange(state);
      }}
      handleConfirm={handleDelete}
      disabled={value.trim() !== currentRow.name || deleteChannel.isPending}
      title={
        <span className='text-destructive'>
          <IconAlertTriangle className='stroke-destructive mr-1 inline-block' size={18} /> {m["channels.dialogs.delete.title"]()}
        </span>
      }
      desc={
        <div className='space-y-4'>
          <Alert variant='destructive'>
            <IconAlertTriangle className='h-4 w-4' />
            <AlertTitle>{m["channels.dialogs.delete.warning"]()}</AlertTitle>
            <AlertDescription>{m["channels.dialogs.delete.warningTitle"]()}</AlertDescription>
          </Alert>
          <div className='space-y-2'>
            <Label htmlFor='channel-name'>
              {m["channels.dialogs.delete.confirmLabel"]()} <strong>{currentRow.name}</strong>{' '}
              {m["channels.dialogs.delete.confirmLabelStrong"]()}
            </Label>
            <Input id='channel-name' placeholder={currentRow.name} value={value} onChange={(e) => setValue(e.target.value)} />
          </div>
        </div>
      }
      confirmText={deleteChannel.isPending ? m["channels.dialogs.delete.deletingButton"]() : m["channels.dialogs.delete.confirmButton"]()}
      cancelBtnText={m["common.buttons.cancel"]()}
    />
  );
}
