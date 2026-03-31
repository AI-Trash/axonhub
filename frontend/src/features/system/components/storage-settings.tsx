'use client';

import { Loader2, Save } from 'lucide-react';
import React, { useState } from 'react';

import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { useDataStorages } from '@/features/data-storages/data/data-storages';

import { useSystemContext } from '../context/system-context';
import { useDefaultDataStorageID, useUpdateDefaultDataStorage } from '../data/system';
import { StoragePolicySettings } from './storage-policy-settings';
import { VideoStorageSettings } from './video-storage-settings';
import * as m from '@/paraglide/messages';

export function StorageSettings() {
  const { data: defaultDataStorageID, isLoading: isLoadingDefaultDataStorage } = useDefaultDataStorageID();
  const { data: dataStorages } = useDataStorages({
    first: 100,
    where: { statusIn: ['active'] },
  });
  const updateDefaultDataStorage = useUpdateDefaultDataStorage();
  const { isLoading, setIsLoading } = useSystemContext();

  const [selectedDataStorageID, setSelectedDataStorageID] = useState<string | undefined>(defaultDataStorageID || undefined);

  // Update selected data storage when loaded
  React.useEffect(() => {
    if (defaultDataStorageID) {
      setSelectedDataStorageID(defaultDataStorageID);
    }
  }, [defaultDataStorageID]);

  const handleSaveDefaultDataStorage = async () => {
    if (!selectedDataStorageID) return;

    setIsLoading(true);
    try {
      await updateDefaultDataStorage.mutateAsync({
        dataStorageID: selectedDataStorageID,
      });
    } finally {
      setIsLoading(false);
    }
  };

  const hasDataStorageChanges = defaultDataStorageID !== selectedDataStorageID;

  if (isLoadingDefaultDataStorage) {
    return (
      <div className='flex h-32 items-center justify-center'>
        <Loader2 className='h-6 w-6 animate-spin' />
        <span className='text-muted-foreground ml-2'>{m["common.loading"]()}</span>
      </div>
    );
  }

  return (
    <div className='space-y-6'>
      {/* Data Storage Selection */}
      <Card>
        <CardHeader>
          <CardTitle>{m["system.storage.dataStorage.title"]()}</CardTitle>
          <CardDescription>{m["system.storage.dataStorage.description"]()}</CardDescription>
        </CardHeader>
        <CardContent className='space-y-4'>
          <div className='grid gap-2'>
            <Label htmlFor='default-data-storage'>{m["system.storage.dataStorage.label"]()}</Label>
            <Select value={selectedDataStorageID} onValueChange={setSelectedDataStorageID} disabled={isLoading}>
              <SelectTrigger id='default-data-storage'>
                <SelectValue placeholder={m["system.storage.dataStorage.placeholder"]()} />
              </SelectTrigger>
              <SelectContent>
                {dataStorages?.edges?.map((edge) => (
                  <SelectItem key={edge.node.id} value={edge.node.id}>
                    {edge.node.name} ({edge.node.type})
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {hasDataStorageChanges && (
            <div className='flex justify-end'>
              <Button onClick={handleSaveDefaultDataStorage} disabled={isLoading || updateDefaultDataStorage.isPending} size='sm'>
                {isLoading || updateDefaultDataStorage.isPending ? (
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
          )}
        </CardContent>
      </Card>

      <StoragePolicySettings />

      <VideoStorageSettings />
    </div>
  );
}
