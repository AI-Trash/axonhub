'use client';

import { Loader2, Save } from 'lucide-react';
import React, { useState } from 'react';

import { AutoCompleteSelect } from '@/components/auto-complete-select';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Label } from '@/components/ui/label';

import { useSystemContext } from '../context/system-context';
import { currencyCodes } from '../data/currencies';
import { useGeneralSettings, useUpdateGeneralSettings } from '../data/system';
import { GMTTimeZoneOptions } from '../data/timezones';
import * as m from '@/paraglide/messages';
import { dynamicTranslation } from '@/lib/paraglide-helpers';

export function GeneralSettings() {
  const { data: settings, isLoading: isLoadingSettings } = useGeneralSettings();
  const updateSettings = useUpdateGeneralSettings();
  const { isLoading, setIsLoading } = useSystemContext();

  const [currencyCode, setCurrencyCode] = useState('USD');
  const [timezone, setTimezone] = useState('UTC');

  const currencyItems = React.useMemo(
    () =>
      currencyCodes.map((code) => ({
        value: code,
        label: dynamicTranslation(`currencies.${code}`),
      })),
    []
  );

  const timezoneItems = React.useMemo(() => GMTTimeZoneOptions, []);

  // Update local state when settings are loaded
  React.useEffect(() => {
    if (settings) {
      setCurrencyCode(settings.currencyCode || 'USD');
      setTimezone(settings.timezone || 'UTC');
    }
  }, [settings]);

  const handleSave = async () => {
    setIsLoading(true);
    try {
      await updateSettings.mutateAsync({
        currencyCode: currencyCode.trim(),
        timezone: timezone.trim(),
      });
    } finally {
      setIsLoading(false);
    }
  };

  const hasChanges = settings ? settings.currencyCode !== currencyCode || settings.timezone !== timezone : false;

  if (isLoadingSettings) {
    return (
      <div className='flex h-32 items-center justify-center'>
        <Loader2 className='h-6 w-6 animate-spin' />
        <span className='text-muted-foreground ml-2'>{m["common.loading"]()}</span>
      </div>
    );
  }

  return (
    <div className='space-y-6'>
      <Card>
        <CardHeader>
          <CardTitle>{m["system.general.title"]()}</CardTitle>
          <CardDescription>{m["system.general.description"]()}</CardDescription>
        </CardHeader>
        <CardContent className='space-y-6'>
          <div className='space-y-2'>
            <Label htmlFor='currency-code'>{m["system.general.currencyCode.label"]()}</Label>
            <div className='max-w-md'>
              <AutoCompleteSelect
                selectedValue={currencyCode}
                onSelectedValueChange={setCurrencyCode}
                items={currencyItems}
                placeholder={m["system.general.currencyCode.placeholder"]()}
                isLoading={isLoadingSettings}
              />
            </div>
            <div className='text-muted-foreground text-sm'>{m["system.general.currencyCode.description"]()}</div>
          </div>

          <div className='space-y-2'>
            <Label htmlFor='timezone'>{m["system.general.timezone.label"]()}</Label>
            <div className='max-w-md'>
              <AutoCompleteSelect
                selectedValue={timezone}
                onSelectedValueChange={setTimezone}
                items={timezoneItems}
                placeholder={m["system.general.timezone.placeholder"]()}
                isLoading={isLoadingSettings}
              />
            </div>
            <div className='text-muted-foreground text-sm'>{m["system.general.timezone.description"]()}</div>
          </div>
        </CardContent>
      </Card>

      {hasChanges && (
        <div className='flex justify-end'>
          <Button onClick={handleSave} disabled={isLoading || updateSettings.isPending} className='min-w-[100px]'>
            {isLoading || updateSettings.isPending ? (
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
    </div>
  );
}
