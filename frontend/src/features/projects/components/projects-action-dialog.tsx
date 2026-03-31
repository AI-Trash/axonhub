'use client';

import { zodResolver } from '@hookform/resolvers/zod';
import React from 'react';
import { useForm } from 'react-hook-form';
import { z } from 'zod';

import { ConfirmDialog } from '@/components/confirm-dialog';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { Form, FormControl, FormDescription, FormField, FormItem, FormLabel, FormMessage } from '@/components/ui/form';
import { Input } from '@/components/ui/input';
import { Textarea } from '@/components/ui/textarea';

import { useProjectsContext } from '../context/projects-context';
import { useCreateProject, useUpdateProject, useArchiveProject, useActivateProject } from '../data/projects';
import { createProjectInputSchema, updateProjectInputSchema } from '../data/schema';
import * as m from '@/paraglide/messages';

// Create Project Dialog
export function CreateProjectDialog() {
  const { isCreateDialogOpen, setIsCreateDialogOpen } = useProjectsContext();
  const createProject = useCreateProject();

  const form = useForm<z.infer<typeof createProjectInputSchema>>({
    resolver: zodResolver(createProjectInputSchema),
    defaultValues: {
      name: '',
      description: '',
    },
  });

  const onSubmit = async (values: z.infer<typeof createProjectInputSchema>) => {
    try {
      await createProject.mutateAsync(values);
      setIsCreateDialogOpen(false);
      form.reset();
    } catch (error) {
      // Error is handled by the mutation
    }
  };

  const handleClose = () => {
    setIsCreateDialogOpen(false);
    form.reset();
  };

  return (
    <Dialog open={isCreateDialogOpen} onOpenChange={handleClose}>
      <DialogContent className='max-w-2xl'>
        <DialogHeader>
          <DialogTitle>{m["projects.dialogs.create.title"]()}</DialogTitle>
          <DialogDescription>{m["projects.dialogs.create.description"]()}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className='space-y-6'>
            <FormField
              control={form.control}
              name='name'
              render={({ field, fieldState }) => (
                <FormItem>
                  <FormLabel>{m["projects.dialogs.fields.name.label"]()}</FormLabel>
                  <FormControl>
                    <Input placeholder={m["projects.dialogs.fields.name.placeholder"]()} aria-invalid={!!fieldState.error} {...field} />
                  </FormControl>
                  <FormDescription>{m["projects.dialogs.fields.name.description"]()}</FormDescription>
                  <div className='min-h-[1.25rem]'>
                    <FormMessage />
                  </div>
                </FormItem>
              )}
            />

            <FormField
              control={form.control}
              name='description'
              render={({ field, fieldState }) => (
                <FormItem>
                  <FormLabel>{m["projects.dialogs.fields.description.label"]()}</FormLabel>
                  <FormControl>
                    <Textarea
                      placeholder={m["projects.dialogs.fields.description.placeholder"]()}
                      aria-invalid={!!fieldState.error}
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>{m["projects.dialogs.fields.description.description"]()}</FormDescription>
                  <div className='min-h-[1.25rem]'>
                    <FormMessage />
                  </div>
                </FormItem>
              )}
            />

            <DialogFooter>
              <Button type='button' variant='outline' onClick={handleClose}>
                {m["common.buttons.cancel"]()}
              </Button>
              <Button type='submit' disabled={createProject.isPending}>
                {createProject.isPending ? m["common.buttons.creating"]() : m["common.buttons.create"]()}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

// Edit Project Dialog
export function EditProjectDialog() {
  const { editingProject, setEditingProject } = useProjectsContext();
  const updateProject = useUpdateProject();

  const form = useForm<z.infer<typeof updateProjectInputSchema>>({
    resolver: zodResolver(updateProjectInputSchema),
    defaultValues: {
      name: '',
      description: '',
    },
  });

  React.useEffect(() => {
    if (editingProject) {
      form.reset({
        name: editingProject.name,
        description: editingProject.description || '',
      });
    }
  }, [editingProject, form]);

  const onSubmit = async (values: z.infer<typeof updateProjectInputSchema>) => {
    if (!editingProject) return;

    try {
      await updateProject.mutateAsync({ id: editingProject.id, input: values });
      setEditingProject(null);
    } catch (error) {
      // Error is handled by the mutation
    }
  };

  const handleClose = () => {
    setEditingProject(null);
    form.reset();
  };

  if (!editingProject) return null;

  return (
    <Dialog open={!!editingProject} onOpenChange={handleClose}>
      <DialogContent className='max-w-2xl'>
        <DialogHeader>
          <DialogTitle>{m["projects.dialogs.edit.title"]()}</DialogTitle>
          <DialogDescription>{m["projects.dialogs.edit.description"]()}</DialogDescription>
        </DialogHeader>
        <Form {...form}>
          <form onSubmit={form.handleSubmit(onSubmit)} className='space-y-6'>
            <FormField
              control={form.control}
              name='name'
              render={({ field, fieldState }) => (
                <FormItem>
                  <FormLabel>{m["projects.dialogs.fields.name.label"]()}</FormLabel>
                  <FormControl>
                    <Input placeholder={m["projects.dialogs.fields.name.placeholder"]()} aria-invalid={!!fieldState.error} {...field} />
                  </FormControl>
                  <FormDescription>{m["projects.dialogs.fields.name.description"]()}</FormDescription>
                  <div className='min-h-[1.25rem]'>
                    <FormMessage />
                  </div>
                </FormItem>
              )}
            />

            <FormField
              control={form.control}
              name='description'
              render={({ field, fieldState }) => (
                <FormItem>
                  <FormLabel>{m["projects.dialogs.fields.description.label"]()}</FormLabel>
                  <FormControl>
                    <Textarea
                      placeholder={m["projects.dialogs.fields.description.placeholder"]()}
                      aria-invalid={!!fieldState.error}
                      {...field}
                    />
                  </FormControl>
                  <FormDescription>{m["projects.dialogs.fields.description.description"]()}</FormDescription>
                  <div className='min-h-[1.25rem]'>
                    <FormMessage />
                  </div>
                </FormItem>
              )}
            />

            <DialogFooter>
              <Button type='button' variant='outline' onClick={handleClose}>
                {m["common.buttons.cancel"]()}
              </Button>
              <Button type='submit' disabled={updateProject.isPending}>
                {updateProject.isPending ? m["common.buttons.saving"]() : m["common.buttons.save"]()}
              </Button>
            </DialogFooter>
          </form>
        </Form>
      </DialogContent>
    </Dialog>
  );
}

// Archive Project Dialog
export function ArchiveProjectDialog() {
  const { archivingProject, setArchivingProject } = useProjectsContext();
  const archiveProject = useArchiveProject();

  const handleConfirm = async () => {
    if (!archivingProject) return;

    try {
      await archiveProject.mutateAsync(archivingProject.id);
      setArchivingProject(null);
    } catch (error) {
      // Error is handled by the mutation
    }
  };

  return (
    <ConfirmDialog
      open={!!archivingProject}
      onOpenChange={() => setArchivingProject(null)}
      title={m["projects.dialogs.archive.title"]()}
      desc={m["projects.dialogs.archive.description"]({ name: archivingProject?.name })}
      confirmText={m["common.buttons.archive"]()}
      cancelBtnText={m["common.buttons.cancel"]()}
      handleConfirm={handleConfirm}
      isLoading={archiveProject.isPending}
      destructive
    />
  );
}

// Activate Project Dialog
export function ActivateProjectDialog() {
  const { activatingProject, setActivatingProject } = useProjectsContext();
  const activateProject = useActivateProject();

  const handleConfirm = async () => {
    if (!activatingProject) return;

    try {
      await activateProject.mutateAsync(activatingProject.id);
      setActivatingProject(null);
    } catch (error) {
      // Error is handled by the mutation
    }
  };

  return (
    <ConfirmDialog
      open={!!activatingProject}
      onOpenChange={() => setActivatingProject(null)}
      title={m["projects.dialogs.activate.title"]()}
      desc={m["projects.dialogs.activate.description"]({ name: activatingProject?.name })}
      confirmText={m["common.buttons.activate"]()}
      cancelBtnText={m["common.buttons.cancel"]()}
      handleConfirm={handleConfirm}
      isLoading={activateProject.isPending}
    />
  );
}

// Combined Dialogs Component
export function ProjectsDialogs() {
  return (
    <>
      <CreateProjectDialog />
      <EditProjectDialog />
      <ArchiveProjectDialog />
      <ActivateProjectDialog />
    </>
  );
}
