
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

import { useModels } from '../context/models-context';
import { useDeleteModel } from '../data/models';
import * as m from '@/paraglide/messages';

export function ModelsDeleteDialog() {
  const { open, setOpen, currentRow, selectedModels } = useModels();
  const deleteModel = useDeleteModel();

  const isOpen = open === 'delete';
  const isBulk = selectedModels.length > 1;
  const modelToDelete = selectedModels.length > 0 ? selectedModels : currentRow ? [currentRow] : [];

  const handleDelete = async () => {
    try {
      for (const model of modelToDelete) {
        await deleteModel.mutateAsync(model.id);
      }
      setOpen(null);
    } catch (error) {}
  };

  const handleClose = () => {
    setOpen(null);
  };

  return (
    <AlertDialog open={isOpen} onOpenChange={handleClose}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{m["models.dialogs.delete.title"]()}</AlertDialogTitle>
          <AlertDialogDescription>
            {isBulk
              ? m["models.dialogs.delete.bulkDescription"]({ count: modelToDelete.length })
              : m["models.dialogs.delete.description"]({ name: currentRow?.name })}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>{m["common.buttons.cancel"]()}</AlertDialogCancel>
          <AlertDialogAction
            onClick={handleDelete}
            disabled={deleteModel.isPending}
            className='bg-destructive text-destructive-foreground hover:bg-destructive/90'
          >
            {m["common.buttons.delete"]()}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
