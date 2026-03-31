'use client';

import { Loader2, Save, Play } from 'lucide-react';
import React, { useState } from 'react';

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/ui/alert-dialog';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Switch } from '@/components/ui/switch';

import { useSystemContext } from '../context/system-context';
import { useStoragePolicy, useUpdateStoragePolicy, useTriggerGcCleanup, CleanupOption } from '../data/system';
import * as m from '@/paraglide/messages';
import { dynamicTranslation } from '@/lib/paraglide-helpers';

export function StoragePolicySettings() {
  const { isLoading, setIsLoading } = useSystemContext();

  const { data: storagePolicy, isLoading: isLoadingStoragePolicy } = useStoragePolicy();
  const updateStoragePolicy = useUpdateStoragePolicy();
  const triggerGcCleanup = useTriggerGcCleanup();

  const [storagePolicyState, setStoragePolicyState] = useState({
    storeChunks: storagePolicy?.storeChunks ?? false,
    storeRequestBody: storagePolicy?.storeRequestBody ?? true,
    storeResponseBody: storagePolicy?.storeResponseBody ?? true,
    cleanupOptions: storagePolicy?.cleanupOptions ?? [],
  });

  React.useEffect(() => {
    if (storagePolicy) {
      setStoragePolicyState({
        storeChunks: storagePolicy.storeChunks,
        storeRequestBody: storagePolicy.storeRequestBody,
        storeResponseBody: storagePolicy.storeResponseBody,
        cleanupOptions: storagePolicy.cleanupOptions,
      });
    }
  }, [storagePolicy]);

  const handleSave = async () => {
    setIsLoading(true);
    try {
      await updateStoragePolicy.mutateAsync({
        storeChunks: storagePolicyState.storeChunks,
        storeRequestBody: storagePolicyState.storeRequestBody,
        storeResponseBody: storagePolicyState.storeResponseBody,
        cleanupOptions: storagePolicyState.cleanupOptions.map((option) => ({
          resourceType: option.resourceType,
          enabled: option.enabled,
          cleanupDays: option.cleanupDays,
        })),
      });
    } finally {
      setIsLoading(false);
    }
  };

  const handleCleanupOptionChange = (index: number, field: keyof CleanupOption, value: any) => {
    const newOptions = [...storagePolicyState.cleanupOptions];
    newOptions[index] = {
      ...newOptions[index],
      [field]: value,
    };
    setStoragePolicyState({
      ...storagePolicyState,
      cleanupOptions: newOptions,
    });
  };

  const hasChanges =
    storagePolicy &&
    (storagePolicy.storeChunks !== storagePolicyState.storeChunks ||
      storagePolicy.storeRequestBody !== storagePolicyState.storeRequestBody ||
      storagePolicy.storeResponseBody !== storagePolicyState.storeResponseBody ||
      JSON.stringify(storagePolicy.cleanupOptions) !== JSON.stringify(storagePolicyState.cleanupOptions));

  if (isLoadingStoragePolicy) {
    return (
      <div className='flex h-32 items-center justify-center'>
        <Loader2 className='h-6 w-6 animate-spin' />
        <span className='text-muted-foreground ml-2'>{m["common.loading"]()}</span>
      </div>
    );
  }

  return (
    <>
      <Card>
        <CardHeader className='flex flex-row items-center justify-between space-y-0 pb-2'>
          <div className='space-y-1.5'>
            <CardTitle>{m["system.storage.policy.title"]()}</CardTitle>
            <CardDescription>{m["system.storage.policy.description"]()}</CardDescription>
          </div>
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant='outline' size='sm' disabled={triggerGcCleanup.isPending || isLoading}>
                {triggerGcCleanup.isPending ? <Loader2 className='mr-2 h-4 w-4 animate-spin' /> : <Play className='mr-2 h-4 w-4' />}
                {m["system.storage.policy.runCleanupNow"]()}
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>{m["system.storage.policy.runCleanupConfirmTitle"]()}</AlertDialogTitle>
                <AlertDialogDescription>{m["system.storage.policy.runCleanupConfirmDescription"]()}</AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel>{m["system.storage.policy.runCleanupCancel"]()}</AlertDialogCancel>
                <AlertDialogAction onClick={() => triggerGcCleanup.mutate()}>
                  {m["system.storage.policy.runCleanupConfirm"]()}
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        </CardHeader>
        <CardContent className='space-y-6'>
          <div className='flex items-center justify-between' id='storage-enabled-switch'>
            <div className='space-y-0.5'>
              <Label htmlFor='storage-policy-store-chunks'>{m["system.storage.policy.storeChunks.label"]()}</Label>
              <div className='text-muted-foreground text-sm'>{m["system.storage.policy.storeChunks.description"]()}</div>
            </div>
            <Switch
              id='storage-policy-store-chunks'
              checked={storagePolicyState.storeChunks}
              onCheckedChange={(checked) =>
                setStoragePolicyState({
                  ...storagePolicyState,
                  storeChunks: checked,
                })
              }
              disabled={isLoading}
            />
          </div>

          <div className='flex items-center justify-between'>
            <div className='space-y-0.5'>
              <Label htmlFor='storage-policy-store-request-body'>{m["system.storage.policy.storeRequestBody.label"]()}</Label>
              <div className='text-muted-foreground text-sm'>{m["system.storage.policy.storeRequestBody.description"]()}</div>
            </div>
            <Switch
              id='storage-policy-store-request-body'
              checked={storagePolicyState.storeRequestBody}
              onCheckedChange={(checked) =>
                setStoragePolicyState({
                  ...storagePolicyState,
                  storeRequestBody: checked,
                })
              }
              disabled={isLoading}
            />
          </div>

          <div className='flex items-center justify-between'>
            <div className='space-y-0.5'>
              <Label htmlFor='storage-policy-store-response-body'>{m["system.storage.policy.storeResponseBody.label"]()}</Label>
              <div className='text-muted-foreground text-sm'>{m["system.storage.policy.storeResponseBody.description"]()}</div>
            </div>
            <Switch
              id='storage-policy-store-response-body'
              checked={storagePolicyState.storeResponseBody}
              onCheckedChange={(checked) =>
                setStoragePolicyState({
                  ...storagePolicyState,
                  storeResponseBody: checked,
                })
              }
              disabled={isLoading}
            />
          </div>

          <div className='space-y-4'>
            <div className='space-y-2'>
              <div className='text-lg font-medium'>{m["system.storage.policy.cleanupOptions"]()}</div>
              <div className='text-muted-foreground text-sm'>{m["system.storage.policy.cleanupDescription"]()}</div>
            </div>
            {storagePolicyState.cleanupOptions.map((option, index) => (
              <div
                key={option.resourceType}
                className='flex flex-col gap-4 rounded-lg border p-4'
                id={'storage-cleanup-option-' + option.resourceType}
              >
                <div className='flex items-center justify-between'>
                  <div className='font-medium'>{dynamicTranslation(`system.storage.policy.resourceTypes.${option.resourceType}`)}</div>
                  <Switch
                    checked={option.enabled}
                    onCheckedChange={(checked) => handleCleanupOptionChange(index, 'enabled', checked)}
                    disabled={isLoading}
                  />
                </div>
                {option.enabled && (
                  <div className='flex items-center gap-2'>
                    <Label htmlFor={`cleanup-days-${index}`}>{m["system.storage.policy.cleanupDays"]()}</Label>
                    <Input
                      id={`cleanup-days-${index}`}
                      type='number'
                      min='0'
                      max='365'
                      value={option.cleanupDays}
                      onChange={(e) => handleCleanupOptionChange(index, 'cleanupDays', parseInt(e.target.value) || 0)}
                      className='w-24'
                      disabled={isLoading}
                    />
                    <span>{m["system.storage.policy.days"]()}</span>
                  </div>
                )}
              </div>
            ))}
          </div>

          <div className='flex justify-end'>
            <Button onClick={handleSave} disabled={isLoading || updateStoragePolicy.isPending || !hasChanges} size='sm'>
              {isLoading || updateStoragePolicy.isPending ? (
                <>
                  <Loader2 className='mr-2 h-4 w-4 animate-spin' />
                  {m["system.buttons.saving"]()}
                </>
              ) : (
                <>
                  <Save className='mr-2 h-4 w-4' />
                  {m["system.buttons.save"]()}
                </>
              )}
            </Button>
          </div>
        </CardContent>
      </Card>
    </>
  );
}
