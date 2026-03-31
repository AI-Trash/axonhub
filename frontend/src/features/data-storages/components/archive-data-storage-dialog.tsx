'use client';


import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';

import { useDataStoragesContext } from '../context/data-storages-context';
import { useArchiveDataStorage } from '../data/data-storages';
import * as m from '@/paraglide/messages';

export function ArchiveDataStorageDialog() {
  const { isArchiveDialogOpen, setIsArchiveDialogOpen, archiveDataStorage, setArchiveDataStorage } = useDataStoragesContext();
  const archiveMutation = useArchiveDataStorage();

  const resetArchiveContext = () => {
    setIsArchiveDialogOpen(false);
    setArchiveDataStorage(null);
  };

  return (
    <Dialog open={isArchiveDialogOpen} onOpenChange={setIsArchiveDialogOpen}>
      <DialogContent className='sm:max-w-[480px]'>
        <DialogHeader>
          <DialogTitle>{m["dataStorages.dialogs.status.archiveTitle"]()}</DialogTitle>
          <DialogDescription>
            {m["dataStorages.dialogs.status.archiveDescription"]({
              name: archiveDataStorage?.name ?? '' })}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button type='button' variant='outline' onClick={resetArchiveContext}>
            {m["common.buttons.cancel"]()}
          </Button>
          <Button
            type='button'
            variant='destructive'
            disabled={archiveMutation.isPending}
            onClick={async () => {
              if (!archiveDataStorage) return;
              try {
                await archiveMutation.mutateAsync(archiveDataStorage.id);
                resetArchiveContext();
              } catch (_error) {
                // handled in mutation
              }
            }}
          >
            {archiveMutation.isPending ? m["common.buttons.archiving"]() : m["common.buttons.archive"]()}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
