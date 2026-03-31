'use client';

import { zodResolver } from '@hookform/resolvers/zod';
import React from 'react';
import { useForm } from 'react-hook-form';
import { z } from 'zod';

import { ConfirmDialog } from '@/components/confirm-dialog';
import { ScopesSelect } from '@/components/scopes-select';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage } from '@/components/ui/form';
import { Input } from '@/components/ui/input';

import { useRolesContext } from '../context/roles-context';
import { useCreateRole, useUpdateRole, useDeleteRole, useBulkDeleteRoles } from '../data/roles';
import { createRoleInputSchema, updateRoleInputSchema } from '../data/schema';
import * as m from '@/paraglide/messages';

// Create Role Dialog
export function CreateRoleDialog() {
  const { isDialogOpen, closeDialog } = useRolesContext();
  const createRole = useCreateRole();
  const [dialogContent, setDialogContent] = React.useState<HTMLDivElement | null>(null);

  const form = useForm<z.infer<typeof createRoleInputSchema>>({
    resolver: zodResolver(createRoleInputSchema),
    defaultValues: {
      name: '',
      scopes: [],
    },
  });

  const onSubmit = async (values: z.infer<typeof createRoleInputSchema>) => {
    try {
      await createRole.mutateAsync(values);
      closeDialog('create');
      form.reset();
    } catch (_error) {
      // Error is handled by the mutation
    }
  };

  const handleClose = () => {
    closeDialog('create');
    form.reset();
  };

  return (
    <Dialog open={isDialogOpen.create} onOpenChange={handleClose}>
      <DialogContent className='max-w-2xl' ref={setDialogContent}>
        <DialogHeader>
          <DialogTitle>{m["roles.dialogs.create.title"]()}</DialogTitle>
          <DialogDescription>{m["roles.dialogs.create.description"]()}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className='space-y-6'>
            <FormField
              control={form.control}
              name='name'
              render={({ field, fieldState }) => (
                <FormItem>
                  <FormLabel>{m["roles.dialogs.fields.name.label"]()}</FormLabel>
                  <FormControl>
                    <Input placeholder={m["roles.dialogs.fields.name.placeholder"]()} aria-invalid={!!fieldState.error} {...field} />
                  </FormControl>
                  <FormDescription>{m["roles.dialogs.fields.name.description"]()}</FormDescription>
                  <div className='min-h-[1.25rem]'>
                    <FormMessage />
                  </div>
                </FormItem>
              )}
            />

            <FormField
              control={form.control}
              name='scopes'
              render={({ field }) => (
                <FormItem>
                  <div className='mb-4'>
                    <FormLabel className='text-base'>{m["roles.dialogs.fields.scopes.label"]()}</FormLabel>
                    <FormDescription>{m["roles.dialogs.fields.scopes.description"]()}</FormDescription>
                  </div>
                  <FormControl>
                    <ScopesSelect
                      level='system'
                      value={field.value || []}
                      onChange={field.onChange}
                      portalContainer={dialogContent}
                      enablePermissionFilter={true}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />

            <DialogFooter>
              <Button type='button' variant='outline' onClick={handleClose}>
                {m["common.buttons.cancel"]()}
              </Button>
              <Button type='submit' disabled={createRole.isPending}>
                {createRole.isPending ? m["common.buttons.creating"]() : m["common.buttons.create"]()}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

// Edit Role Dialog
export function EditRoleDialog() {
  const { editingRole, isDialogOpen, closeDialog } = useRolesContext();
  const updateRole = useUpdateRole();
  const [dialogContent, setDialogContent] = React.useState<HTMLDivElement | null>(null);

  const form = useForm<z.infer<typeof updateRoleInputSchema>>({
    resolver: zodResolver(updateRoleInputSchema),
    defaultValues: {
      name: '',
      scopes: [],
    },
  });

  React.useEffect(() => {
    if (editingRole) {
      form.reset({
        name: editingRole.name,
        scopes: editingRole.scopes?.map((scope: string) => scope) || [],
      });
    }
  }, [editingRole, form]);

  const onSubmit = async (values: z.infer<typeof updateRoleInputSchema>) => {
    if (!editingRole) return;

    try {
      await updateRole.mutateAsync({ id: editingRole.id, input: values });
      closeDialog('edit');
    } catch (_error) {
      // Error is handled by the mutation
    }
  };

  const handleClose = () => {
    closeDialog('edit');
    form.reset();
  };

  if (!editingRole) return null;

  return (
    <Dialog open={isDialogOpen.edit} onOpenChange={handleClose}>
      <DialogContent className='max-w-2xl' ref={setDialogContent}>
        <DialogHeader>
          <DialogTitle>{m["roles.dialogs.edit.title"]()}</DialogTitle>
          <DialogDescription>{m["roles.dialogs.edit.description"]()}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className='space-y-6'>
            <FormField
              control={form.control}
              name='name'
              render={({ field, fieldState }) => (
                <FormItem>
                  <FormLabel>{m["roles.dialogs.fields.name.label"]()}</FormLabel>
                  <FormControl>
                    <Input placeholder={m["roles.dialogs.fields.name.placeholder"]()} aria-invalid={!!fieldState.error} {...field} />
                  </FormControl>
                  <FormDescription>{m["roles.dialogs.fields.name.description"]()}</FormDescription>
                  <div className='min-h-[1.25rem]'>
                    <FormMessage />
                  </div>
                </FormItem>
              )}
            />

            <FormField
              control={form.control}
              name='scopes'
              render={({ field }) => (
                <FormItem>
                  <div className='mb-4'>
                    <FormLabel className='text-base'>{m["roles.dialogs.fields.scopes.label"]()}</FormLabel>
                    <FormDescription>{m["roles.dialogs.fields.scopes.description"]()}</FormDescription>
                  </div>
                  <FormControl>
                    <ScopesSelect
                      value={field.value || []}
                      onChange={field.onChange}
                      portalContainer={dialogContent}
                      level='system'
                      enablePermissionFilter={true}
                    />
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />

            <DialogFooter>
              <Button type='button' variant='outline' onClick={handleClose}>
                {m["common.buttons.cancel"]()}
              </Button>
              <Button type='submit' disabled={updateRole.isPending}>
                {updateRole.isPending ? m["common.buttons.saving"]() : m["common.buttons.save"]()}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

// Delete Role Dialog
export function DeleteRoleDialog() {
  const { deletingRole, isDialogOpen, closeDialog } = useRolesContext();
  const deleteRole = useDeleteRole();

  const handleConfirm = async () => {
    if (!deletingRole) return;

    try {
      await deleteRole.mutateAsync(deletingRole.id);
      closeDialog('delete');
    } catch (_error) {
      // Error is handled by the mutation
    }
  };

  return (
    <ConfirmDialog
      open={isDialogOpen.delete}
      onOpenChange={() => closeDialog('delete')}
      title={m["roles.dialogs.delete.title"]()}
      desc={m["roles.dialogs.delete.description"]({ name: deletingRole?.name })}
      confirmText={m["common.buttons.delete"]()}
      cancelBtnText={m["common.buttons.cancel"]()}
      handleConfirm={handleConfirm}
      isLoading={deleteRole.isPending}
      destructive
    />
  );
}

// Bulk Delete Roles Dialog
export function BulkDeleteRolesDialog() {
  const { isDialogOpen, closeDialog, selectedRoles, resetRowSelection } = useRolesContext();
  const bulkDeleteRoles = useBulkDeleteRoles();

  const handleConfirm = async () => {
    if (selectedRoles.length === 0) return;

    try {
      const ids = selectedRoles.map((role) => role.id);
      await bulkDeleteRoles.mutateAsync(ids);
      resetRowSelection();
      closeDialog('bulkDelete');
    } catch (_error) {
      // Error is handled by the mutation
    }
  };

  return (
    <ConfirmDialog
      open={isDialogOpen.bulkDelete}
      onOpenChange={() => closeDialog('bulkDelete')}
      title={m["roles.dialogs.bulkDelete.title"]()}
      desc={m["roles.dialogs.bulkDelete.description"]({ count: selectedRoles.length })}
      confirmText={m["common.buttons.delete"]()}
      cancelBtnText={m["common.buttons.cancel"]()}
      handleConfirm={handleConfirm}
      isLoading={bulkDeleteRoles.isPending}
      destructive
    />
  );
}

// Combined Dialogs Component
export function RolesDialogs() {
  return (
    <>
      <CreateRoleDialog />
      <EditRoleDialog />
      <DeleteRoleDialog />
      <BulkDeleteRolesDialog />
    </>
  );
}
